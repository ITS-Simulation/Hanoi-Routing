# Camera Way Propagation — Plan

**Status:** Draft  
**Date:** 2026-04-02  
**Relation:** Extends
[Live Weight MVP.md](Live%20Weight%20MVP.md) (camera → arc mapping) and
[Live Weight Pipeline.md](Live%20Weight%20Pipeline.md) (full architecture)

---

## 0. Problem Statement

A real-world camera observes traffic conditions for an entire road between major
junctions. In the graph, that road is subdivided into many arcs at every minor
intersection (alleys, residential side-streets, driveways). Currently, the
pipeline maps each camera to **exactly one arc** — leaving sibling arcs on the
same road and in the same direction governed only by the generic time-of-day
heuristic.

### What goes wrong today

1. **Inconsistent speed on the same road.** The camera arc might report 14 km/h
  during rush hour while its neighbours on the same physical road report ~38
   km/h (free-flow with a mild ToD bump). This is physically impossible.
2. **Router exploitation.** The CCH treats each arc independently. With only 1
  of N arcs congested, the router can route *through* the fast sibling arcs or
   zigzag off and back on to avoid the single slow arc, producing unrealistic
   routes.
3. **Underestimated travel time.** Routes traversing the uncovered arcs of a
  camera-monitored road undercount travel time because the camera data is not
   applied to them.

### Goal

Propagate each camera's speed profile to **all arcs on the same way that travel
in the same direction**, so that the camera data is representative of the entire
physical road it observes.

---

## 1. Available Data

All data needed for this feature already exists:


| Data                                        | Location                  | Purpose                                                                     |
| ------------------------------------------- | ------------------------- | --------------------------------------------------------------------------- |
| `wayIndex: IntArray`                        | `GraphInputs.wayIndex`    | `wayIndex[arcId] → routingWayId` (see note below on way ID semantics)       |
| `RoadIndex`                                 | `GraphInputs.roadIndex`   | CSR index: `way → [arcIds]` via `firstArcOffsetByWay` and `arcIdsByWay`     |
| `ArcManifest.isAntiparallelToWay(arcId)`    | `GraphInputs.arcManifest` | `Boolean` — `false` = follows OSM digitization direction, `true` = opposite |
| `ArcManifest.bearingDeg(arcId)`             | `GraphInputs.arcManifest` | Arc bearing in degrees (0–360) — secondary validation signal                |
| `CameraPlacement.Coordinate.flowBearingDeg` | `cameras.yaml`            | Camera observation direction                                                |
| `ResolvedCamera.bearingDeg`                 | `CameraResolver` output   | Bearing of the resolved anchor arc                                          |


### Note on `routingWayId` vs `osmWayId`

The plan groups arcs by `routingWayId` (the compact 0-based local index), not by
`osmWayId` (the sparse 64-bit OSM identifier). The two are **1:1 and
bijective** within the loaded graph:

- RoutingKit's `LocalIDMapper` (bitvector + rank) maps each OSM way to exactly
  one `routingWayId`. It never splits a single OSM way across multiple local
  IDs.
- `buildRoadIndex()` in `GraphLoader.kt` (lines 437–447) explicitly validates
  that all arcs sharing a `routingWayId` also share the same `osmWayId`, road
  name, and highway class.
- Grouping by either produces the identical partition of arcs.

We use `routingWayId` because it is the native index of the `RoadIndex` CSR
(`firstArcOffsetByWay`, `arcIdsByWay`). `osmWayId` is stored for
debugging/traceability but would require an extra reverse lookup to iterate arcs.

### Key relationships

```
Camera → resolves to one anchor arc → anchor arc has:
  ├─ routingWayId (via wayIndex) ←── 1:1 with osmWayId
  ├─ isAntiparallelToWay (boolean direction flag)
  └─ bearingDeg (geometric direction)

RoadIndex → for any wayId, iterates all arcs:
  for offset in firstArcOffsetByWay[wayId] until firstArcOffsetByWay[wayId + 1]:
      siblingArcId = arcIdsByWay[offset]
```

---

## 2. Directional Filtering Strategy

### Primary filter: `isAntiparallelToWay` (required)

This flag is a discrete, exact property from the graph construction phase. Every
arc on a two-way road exists in two copies — one with `antiparallel = false`
(follows OSM way digitization) and one with `antiparallel = true` (opposite).

**Rule:** Propagate only to sibling arcs where
`isAntiparallelToWay == anchor.isAntiparallelToWay`.

This is the primary and most reliable filter because:

- It is binary — no tolerance thresholds or tuning required
- It comes from OSM way digitization, which is deterministic
- It perfectly partitions a two-way road into its two directional groups
- On one-way roads, all arcs have `antiparallel = false`, so propagation
naturally covers all arcs (correct, since there is only one traffic direction)

### Secondary check: bearing sanity warning (non-blocking)

The bearing check is **warning-only, not a hard filter**. The arc is always
propagated if it passes the `antiparallel` filter; a large bearing difference
only triggers a WARN log for operator review.

**Why not a hard rejection?** A road that curves significantly (e.g., a 120°
bend over its length) will have arcs near the ends with bearings that differ by
>90° from the anchor arc. A hard `bearingDiff <= 90°` rejection would exclude
those arcs, reintroducing exactly the mid-road speed discontinuity this feature
is designed to eliminate. Since `antiparallel` already guarantees same-direction
semantics (from OSM way digitization), the bearing difference on a curved road
is expected geometry, not an error.

```
bearingDiff = circularAngleDiff(anchor.bearingDeg, sibling.bearingDeg)
if bearingDiff > 90°: log WARN (but still propagate)
```

The 90° threshold for the warning is chosen to flag genuinely unusual geometry
(U-turns within a single way, complex junction artifacts) while staying silent
on normal curves. Operators can investigate flagged arcs and, if needed, split
the camera config into separate entries.

### Why `antiparallel` is the sole hard filter


| Signal                 | Pros                                                | Cons                                                   |
| ---------------------- | --------------------------------------------------- | ------------------------------------------------------ |
| `isAntiparallelToWay`  | Discrete, no threshold tuning, exact OSM semantics  | Only distinguishes 2 directions (sufficient for roads) |
| `bearingDeg`           | Works on any arc pair, handles curves               | Hard rejection can exclude valid arcs on curved roads  |
| **Chosen approach**    | `antiparallel` for filtering, bearing for warnings  | Operators must review warnings manually                |


---

## 3. Design

### 3.1 New class: `CameraProfileExpander`

A dedicated class that takes the resolved cameras and expands their profiles to
all directionally-matched sibling arcs. This keeps the expansion logic separate
from both camera resolution (which finds the anchor arc) and weight generation
(which consumes the profile map).

**Placement:** New file
`CCH_Data_Pipeline/app/src/main/kotlin/com/thomas/cch_app/CameraProfileExpander.kt`

```kotlin
class CameraProfileExpander(
    private val roadIndex: RoadIndex,
    private val arcManifest: ArcManifest,
    private val wayIndex: IntArray,
) {
    /**
     * Expands camera profiles from anchor arcs to all same-way,
     * same-direction sibling arcs.
     *
     * @param anchorProfiles Map of anchor arc ID → SpeedProfile
     *        (one entry per resolved camera)
     * @return Map of arc ID → SpeedProfile covering all propagated arcs
     */
    fun expand(anchorProfiles: Map<Int, SpeedProfile>): Map<Int, SpeedProfile> {
        val expanded = LinkedHashMap<Int, SpeedProfile>()

        for ((anchorArcId, profile) in anchorProfiles) {
            val wayId = wayIndex[anchorArcId]
            val anchorAntiparallel = arcManifest.isAntiparallelToWay(anchorArcId)
            val anchorBearing = arcManifest.bearingDeg(anchorArcId)

            val startOffset = roadIndex.firstArcOffsetByWay[wayId]
            val endOffset = roadIndex.firstArcOffsetByWay[wayId + 1]

            for (offset in startOffset until endOffset) {
                val siblingArcId = roadIndex.arcIdsByWay[offset]

                // Primary filter: same direction (hard reject)
                if (arcManifest.isAntiparallelToWay(siblingArcId) != anchorAntiparallel) continue

                // Bearing sanity warning (non-blocking — see Section 2)
                val bearingDiff = CameraResolver.circularAngleDiff(
                    anchorBearing, arcManifest.bearingDeg(siblingArcId)
                )
                if (bearingDiff > BEARING_WARN_THRESHOLD_DEG) {
                    logger.warn(
                        "arc {} on way {} has bearing diff {:.1f}° from anchor arc {} " +
                        "(>{:.0f}° threshold); propagating anyway — review geometry",
                        siblingArcId, wayId, bearingDiff, anchorArcId, BEARING_WARN_THRESHOLD_DEG,
                    )
                }

                // If another camera already claimed this arc, skip
                // (first-camera-wins; conflict logged)
                val existing = expanded.putIfAbsent(siblingArcId, profile)
                if (existing != null && siblingArcId != anchorArcId) {
                    logger.warn(
                        "arc {} already covered by another camera's propagation; " +
                        "skipping propagation from anchor arc {}",
                        siblingArcId, anchorArcId,
                    )
                }
            }
        }

        return expanded
    }

    companion object {
        private val logger = LoggerFactory.getLogger(CameraProfileExpander::class.java)
        private const val BEARING_WARN_THRESHOLD_DEG = 90.0
    }
}
```

### 3.2 Integration point: `Main.kt` (LiveWeightMvpCommand)

The expansion step is inserted between camera resolution and weight generation.
The change is minimal — replace the `cameraProfiles` map with the expanded
version:

**Current flow** (lines 46–64 of `Main.kt`):

```kotlin
val resolvedCameras = resolver.resolveAll(cameraConfig.cameras)
val cameraProfiles = LinkedHashMap<Int, SpeedProfile>(resolvedCameras.size)
for (resolved in resolvedCameras) {
    // ... duplicate check ...
    cameraProfiles[resolved.arcId] = profiles[resolved.camera.profileName]
}
// cameraProfiles has ONE entry per camera
```

**New flow:**

```kotlin
val resolvedCameras = resolver.resolveAll(cameraConfig.cameras)
val anchorProfiles = LinkedHashMap<Int, SpeedProfile>(resolvedCameras.size)
for (resolved in resolvedCameras) {
    // ... same duplicate check ...
    anchorProfiles[resolved.arcId] = profiles[resolved.camera.profileName]
}

// Expand to all same-way, same-direction sibling arcs
val expander = CameraProfileExpander(inputs.roadIndex, inputs.arcManifest, inputs.wayIndex)
val cameraProfiles = expander.expand(anchorProfiles)

logger.info(
    "camera propagation: anchorArcs={}, expandedArcs={}",
    anchorProfiles.size,
    cameraProfiles.size,
)
```

### 3.3 No changes to `WeightGenerator`

The `WeightGenerator` takes `cameraProfiles: Map<Int, SpeedProfile>` and already
handles per-arc weight computation using `geo_distance`. Because each arc has
its own `geo_distance`, the travel time is computed correctly per arc:

```kotlin
val baseTravelTimeMs = inputs.geoDistance[originalEdgeId] * 3600.0 / speedKmh
```

A 200m arc at 14 km/h → 51,429 ms. A 50m arc at 14 km/h → 12,857 ms. The total
across all arcs on the road sums correctly because speed is uniform (from the
camera profile) but distance varies per arc.

**No change needed here.**

### 3.4 No changes to `CameraResolver`

The resolver's job is to find the best anchor arc for each camera. It continues
to return one `ResolvedCamera` per `CameraSpec`. The propagation is a separate
concern handled downstream.

**No change needed here.**

---

## 4. Edge Cases

### 4.1 Multiple cameras on the same way, same direction

If two cameras resolve to different anchor arcs on the same way with the same
direction, their propagation zones overlap. The `expand()` method uses
`putIfAbsent` — **first camera wins** for overlapping arcs, with a warning
logged.

This is acceptable for the MVP. Future iterations could implement
distance-weighted blending between cameras.

### 4.2 Multiple cameras on the same way, opposite directions

This is correct and expected — e.g., inbound and outbound cameras on the same  
road. The `antiparallel` filter ensures they propagate independently to their  
respective directional arc groups. No conflict.

### 4.4 One-way streets

All arcs have `antiparallel = false`. The filter passes them all. This is
correct — a one-way road has only one traffic direction, so all arcs should
receive the camera profile.

### 4.5 Ways with ramps or highway type transitions

A single OSM way generally maintains its highway classification. If a way spans
a type boundary (rare but possible), the propagated profile still applies —
the camera measures speed on that road regardless of its classification. The
highway type only affects the ToD fallback for non-camera arcs, which is not
used when a camera profile is present.

---

## 5. Files Changed


| File                                 | Change                                                                             |
| ------------------------------------ | ---------------------------------------------------------------------------------- |
| `**CameraProfileExpander.kt`** (new) | New class: way-based directional propagation logic                                 |
| `**Main.kt**`                        | Insert expansion step between resolution and weight generation (~10 lines changed) |


**Files NOT changed:**


| File                 | Reason                                                |
| -------------------- | ----------------------------------------------------- |
| `WeightGenerator.kt` | Already handles per-arc `geo_distance` correctly      |
| `CameraResolver.kt`  | Still resolves to one anchor arc per camera           |
| `CameraConfig.kt`    | No YAML schema changes needed                         |
| `GraphLoader.kt`     | `RoadIndex`, `ArcManifest`, `wayIndex` already loaded |


---

## 6. Testing Strategy

### Unit tests for `CameraProfileExpander`

**File:**
`CCH_Data_Pipeline/app/src/test/kotlin/com/thomas/cch_app/CameraProfileExpanderTest.kt`


| Test case                                  | Setup                                                         | Expected                                                |
| ------------------------------------------ | ------------------------------------------------------------- | ------------------------------------------------------- |
| Single camera, two-way road                | Way with 4 arcs: 2 forward, 2 reverse. Camera on forward arc. | Profile propagated to both forward arcs only            |
| Single camera, one-way road                | Way with 3 arcs, all `antiparallel = false`                   | Profile propagated to all 3 arcs                        |
| Two cameras, same way, opposite directions | Way with 4 arcs. Camera A on forward, camera B on reverse     | Each camera covers its 2 directional arcs independently |
| Two cameras, same way, same direction      | Way with 3 forward arcs. Camera A on arc 0, camera B on arc 2 | First-camera-wins for shared arcs; warning logged       |
| Bearing warning (non-blocking)             | Sibling arc with same `antiparallel` but bearing diff > 90°   | Sibling included; WARN logged                           |
| Empty cameras                              | No cameras configured                                         | Empty expanded map returned                             |
| Camera on isolated arc (single-arc way)    | Way has only 1 arc                                            | Map contains only that arc                              |


### Integration verification

After running the MVP with propagation enabled, verify via logs:

1. `anchorArcs` count matches number of cameras in YAML
2. `expandedArcs` count is >= `anchorArcs` (more coverage)
3. `cameraCoveredEdges` in `WeightGenerator.logSummary()` matches `expandedArcs`
4. Routes through camera-monitored roads show consistent travel times across all
  arcs (no sudden speed jumps mid-road)

---

## 7. Logging

The `CameraProfileExpander` should log at INFO level:

```
camera propagation: anchor arc 42 (way=17, road='Nguyen Trai', antiparallel=false)
  → propagated to 5 sibling arcs [42, 43, 44, 45, 46]
```

And the summary in `Main.kt`:

```
camera propagation: anchorArcs=3, expandedArcs=14
```

This gives operators visibility into how many arcs each camera actually
influences.

---

## 8. Summary

This is a targeted, low-risk enhancement:

- **1 new file** (`CameraProfileExpander.kt`) with ~60 lines of logic
- **~10 lines changed** in `Main.kt`
- **0 changes** to `WeightGenerator`, `CameraResolver`, `CameraConfig`, or
`GraphLoader`
- All required data (`wayIndex`, `RoadIndex`, `ArcManifest.isAntiparallelToWay`)
already exists and is loaded
- The physical model is correct: same road + same direction = same traffic
conditions, with per-arc `geo_distance` ensuring proper travel time
distribution

