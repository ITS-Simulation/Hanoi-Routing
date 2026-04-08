# Live Weight Pipeline — Plan & Critique

**Status:** Draft (Rev 4)
**Date:** 2026-03-24
**Phase:** Data Handling Pipeline (horizontal T-bar)

---

## 1. Original Proposed Architecture

```
Camera Data Packets (many sources)
  → Workers (many, collect packets)
    → Streaming (Kafka/MQ, single instance, multiple topics)
      → Smoothing (Huber DES)
        → Weight Modelling
          → Weight Output (→ POST /customize on hanoi-server)
```

With simulation in the first iteration: RNG threads simulate cameras, workers
collect, push to stream, then through the pipeline.

---

## 2. Critique

### 2.1 What's Good

1. **T-shaped thinking is correct.** You've built the vertical (deep CCH engine
  with contraction → customization → query) and now need the horizontal
   (ingesting real-world signals into weight vectors). This is the right
   sequencing.
2. **Simulation-first approach is smart.** Building with synthetic data means
  you can develop and test the entire pipeline without depending on camera
   hardware, network connectivity, or HERE/OSM API access. You can also
   generate adversarial inputs (spike traffic, camera dropout, NaN bursts) that
   would be hard to reproduce with real cameras.
3. **Huber DES is a sound choice for smoothing.** Double Exponential Smoothing
  (Holt's method) captures both level and trend — important for traffic that
   has inertial momentum (congestion doesn't spike and vanish instantly). The
   Huber loss variant makes it robust to outlier speed readings from cameras
   (a misread plate, a parked car, a motorbike cutting through at 80 km/h on
   a 30 km/h alley). This is specifically better than vanilla DES for traffic
   because camera data is inherently noisy and fat-tailed.
4. **Separation of concerns** (camera → worker → stream → smooth → model →
  output) follows a clean dataflow topology. Each stage can be independently
   tested, scaled, and replaced.
5. **Weight Modelling as a separate module** — confirmed as the intended design.
  Takes Huber DES output, converts to a single usable weight value per edge
   for CCH customization. Clean interface boundary.

### 2.2 What's Missing or Underspecified

#### A. Camera → Edge Mapping Is Necessary Now

The pipeline's output contract is a weight vector of exactly `[u32; 1,869,499]`
for the Hanoi graph (or the line graph equivalent). Every camera observation
must be translated to one or more edge IDs. This mapping cannot be deferred —
without it, the pipeline has no way to produce output.

**Simulation backbone:** Use a config-driven mapping (JSON file listing
camera_id → edge_ids). For simulation, auto-generate this from graph metadata.
See Section 6 for the full camera-edge mapping design.

**Production recommendation:** See Section 6.3 for the database-backed
approach when the camera fleet is real.

#### B. No Backpressure or Windowing Semantics

The diagram shows a linear flow but doesn't specify time windows or
backpressure strategy.

**Decision:** Use **tumbling 30-second windows**. The aggregator collects
packets for 30s (simulated time), computes per-edge statistics, then emits a
batch to the smoother. This decouples ingest rate from processing rate.

#### C. Streaming/Queue Is a DE Department Concern

The Kafka/MQ layer is infrastructure that the Data Engineering team will own
in production. For simulation, we don't need an external broker — Kotlin's
coroutine channels provide the same semantics in-process.

**Decision:** Use `Channel<CameraPacket>` (Kotlin coroutines) for simulation.
Design the ingest layer as an interface so a Kafka/NATS consumer can be
swapped in later without touching pipeline logic. See Section 7.4.

#### D. Each Component Is a Camera

Acknowledged — in the simulation, each camera is a self-contained coroutine
that generates packets independently. No separate "worker" tier needed.

#### E. Staleness TTL Handling

When a camera goes silent, the pipeline must detect this and gracefully degrade:

- **Stale threshold (e.g., 5 minutes):** If no observation arrives for an edge
within this window, the smoother state for that edge is considered stale.
The weight model begins blending back toward baseline.
- **Dead threshold (e.g., 30 minutes):** The smoother state is reset entirely.
The edge reverts to pure baseline + time-of-day modulation.
- **Detection:** The weight model checks `lastUpdateMs` per edge on every
window tick — not just when data arrives.

See Section 7.5 for the full TTL design.

### 2.3 Architecture Risk: Full-Vector Customization Frequency

**Current behavior:** `POST /customize` accepts the entire `[u32; 1,869,499]`
weight vector (7.5 MB). The engine runs full CCH customization, which touches
the entire hierarchy.

**At 30s windows:** Comfortable. CCH customization takes 100–500ms for this
graph size. That's 0.3–1.7% duty cycle. Queries during customization see stale
weights but are not blocked.

**At 5s windows:** Starts to hurt. 2–10% duty cycle. Queries may frequently
see stale weights.

**At 1s windows:** Untenable. Customization can't keep up.

**Status:** Deferred — 30s windows are the target for now.

**Documented fix for the future:** If sub-10s updates are needed, two
approaches:

1. **Delta customization:** Only re-customize the subgraph affected by changed
  weights. Requires engine-side changes in `hanoi-core` to support partial
   `customize_with()` — the CCH triangle relaxation would need to track which
   shortcuts are affected by changed lower-triangle edges. This is a deep
   algorithmic change to the CCH customization phase.
2. **Batched partial updates:** Partition edges into N groups (e.g., by
  geographic region). Rotate which group gets re-customized each tick.
   Simpler to implement but introduces spatial staleness (some regions update
   before others).
3. **Dual-buffer engine:** Maintain two `CustomizedBasic` instances. While one
  serves queries, the other is being re-customized. Atomic swap when done.
   This already partially exists (the `watch` channel + background thread
   pattern), but currently blocks the engine thread during customization.
   Making it truly double-buffered would require a second engine thread or
   moving customization to an async task.

---

## 3. Agreed Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│                     KOTLIN SIMULATION APPLICATION                        │
│                                                                          │
│  ┌───────────────┐    RoutingKit binary format: first_out, head,         │
│  │  Graph Loader  │    travel_time, geo_distance, latitude, longitude    │
│  │  (binary I/O)  │────→ baseline weights + edge lengths + adjacency     │
│  └───────────────┘                                                       │
│         │                                                                │
│         ▼                                                                │
│  ┌───────────────┐    Config file OR auto-generated from graph           │
│  │  Camera-Edge   │────→ cameraId → List<EdgeMapping>                    │
│  │  Mapping       │────→ edgeId → List<CameraId> (reverse index)         │
│  └───────────────┘                                                       │
│         │                                                                │
│         ├──────────────────────────────────────────────┐                  │
│         ▼                                              ▼                  │
│  ┌───────────────┐                           ┌──────────────────┐        │
│  │  Camera Sim   │   CameraPacket              │  Influence Map   │        │
│  │  (N coroutines │───→ Channel<CameraPacket> │  Precomputation  │        │
│  │  batched)      │                           │  (BFS from       │        │
│  └───────────────┘                            │   covered edges) │        │
│         │                                     └────────┬─────────┘        │
│         ▼                                              │                  │
│  ┌───────────────┐    Tumbling 30s windows (shared boundary)          │   │
│  │ Dual Aggregator│                                                   │   │
│  │  ├─ Speed lane │────→ List<SpeedSummary>                           │   │
│  │  └─ Occup lane │────→ List<OccupancySummary>                       │   │
│  └───────────────┘                                                    │   │
│         │ (two parallel lanes)                                        │   │
│         ▼                                                             │   │
│  ┌───────────────┐    Per-edge: level, trend, lastUpdateMs            │   │
│  │ Speed Smoother │────→ Map<EdgeId, SmootherState>  ──┐              │   │
│  │ (Huber DES)    │                                    │              │   │
│  ├───────────────┤                                     ▼              │   │
│  │ Occup Smoother │────→ Map<EdgeId, SmootherState>  Joiner           │   │
│  │ (Huber DES)    │                                  (alignment)      │   │
│  └───────────────┘                                     │              │   │
│         │                                    Map<EdgeId,JoinedState>  │   │
│         ▼                                              │              │   │
│  ┌─────────────────────────────────────────────────────┴──────────┐   │   │
│  │  Weight Model (separate module)                                 │  │   │
│  │                                                                 │  │   │
│  │  Input: joined states + baseline + influence map + sim clock    │  │   │
│  │                                                                 │  │   │
│  │  Covered edges:                                                 │  │   │
│  │    smoothed_speed → travel_time_ms (via geo_distance)           │  │   │
│  │    × occupancy scaling factor                                   │  │   │
│  │    confidence blend with baseline                               │  │   │
│  │    staleness TTL check (per-lane, use worst)                    │  │   │
│  │                                                                 │  │   │
│  │  Uncovered edges:                                               │  │   │
│  │    time-of-day Gaussian modulation (always on)                  │  │   │
│  │    + neighbor congestion propagation (if influence exists)      │  │   │
│  │                                                                 │  │   │
│  │  Output: IntArray(1_869_499) — one u32 weight per edge          │  │   │
│  └────────────────────────────┬────────────────────────────────────┘  │   │
│                               │                                          │
│                               ▼                                          │
│                    POST /customize (7.5 MB, little-endian u32)           │
│                    → hanoi-server engine thread                           │
│                    → CCH re-customization (~100-500ms)                    │
│                    → live queries reflect updated weights                 │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Modularity Contracts

Each pipeline stage is a self-contained module with a defined interface. No
module knows about the internals of another — they communicate only through
these contracts. This means any stage can be replaced, tested in isolation,
or rewritten without affecting the rest of the pipeline.

### 4.1 Stage Interfaces

Each camera produces **two data signals**: speed (flow velocity) and occupancy
(road utilization fraction). These flow through parallel aggregation and
smoothing pipelines, then **join** at the weight model.

```
                              ┌──────────────┐   List<SpeedSummary>   ┌──────────────┐
                         ┌───→│ SpeedAggr.   │───────────────────────→│ SpeedSmoother│──┐
                         │    └──────────────┘                        └──────────────┘  │
┌──────────┐  CameraPacket│                                                              │
│  Ingest  │─────────────┤                                                   SmootherSnapshot
│          │  Channel     │                                                    (both)    │
└──────────┘             │    ┌──────────────┐   List<OccupancySummary>┌──────────────┐  │
                         └───→│ OccupAggr.   │───────────────────────→│ OccupSmoother│──┤
                              └──────────────┘                        └──────────────┘  │
                                                                                        ▼
┌──────────┐    IntArray       ┌──────────────┐                                   ┌──────────┐
│  Output  │←──────────────────│  Weight      │←──────────────────────────────────│  Joiner  │
│          │  (weight vector)  │  Model       │     JoinedEdgeState               │          │
└──────────┘                   └──────────────┘                                   └──────────┘
```

Each interface is a Kotlin `interface` or `data class`:

```kotlin
// === Ingest → Aggregators ===
// Contract: CameraPacket on a Channel. Ingest produces, both aggregators consume.
// Each packet carries BOTH speed and occupancy from the same camera snapshot.

data class CameraPacket(
    val cameraId: Int,
    val timestampMs: Long,      // simulation time (shared clock)
    val speedKmh: Float,        // flow velocity measurement
    val occupancy: Float,       // road occupancy ∈ [0.0, 1.0]
    val confidence: Float       // 0.0–1.0, sensor self-reported quality
)

interface PacketSource {
    fun packets(): ReceiveChannel<CameraPacket>
}

// === Aggregator → Smoother (speed lane) ===
// Contract: a batch of per-edge speed summaries, emitted every window tick.

data class SpeedSummary(
    val edgeId: Int,
    val meanSpeedKmh: Float,
    val observationCount: Int,
    val variance: Float,
    val windowEndMs: Long
)

// === Aggregator → Smoother (occupancy lane) ===
// Contract: a batch of per-edge occupancy summaries, emitted every window tick.

data class OccupancySummary(
    val edgeId: Int,
    val meanOccupancy: Float,   // ∈ [0.0, 1.0]
    val observationCount: Int,
    val variance: Float,
    val windowEndMs: Long
)

interface Aggregator<S> {
    fun summaries(): ReceiveChannel<List<S>>
}

// === Smoother → Joiner ===
// Contract: a snapshot of all smoother states. The joiner reads both
// snapshots immutably — each smoother owns its own mutable state.

data class SmootherState(
    val level: Double,          // current smoothed value (speed km/h or occupancy 0–1)
    val trend: Double,          // change per window
    val lastUpdateMs: Long,     // for staleness detection
    val observationCount: Int   // for confidence
)

interface Smoother<S> {
    fun update(summaries: List<S>)
    fun snapshot(): Map<Int, SmootherState>
}

// === Joiner → Weight Model ===
// Contract: a per-edge combined view of speed + occupancy state.
// The joiner resolves temporal misalignment between the two lanes.

data class JoinedEdgeState(
    val speedState: SmootherState,
    val occupancyState: SmootherState,
    val alignmentAge: Long      // |speed.lastUpdateMs - occupancy.lastUpdateMs|
)

interface EdgeJoiner {
    /**
     * Combine speed and occupancy snapshots into a single aligned view.
     * Handles the case where one lane has data and the other doesn't.
     */
    fun join(
        speedStates: Map<Int, SmootherState>,
        occupancyStates: Map<Int, SmootherState>,
        nowMs: Long
    ): Map<Int, JoinedEdgeState>
}

// === Weight Model → Output ===
// Contract: a complete weight vector (IntArray of exactly numEdges elements).

interface WeightModel {
    fun computeWeights(states: Map<Int, JoinedEdgeState>, nowMs: Long): IntArray
}

interface WeightOutput {
    suspend fun deliver(weights: IntArray)
}
```

### 4.2 Why This Level of Separation Matters


| Scenario                                    | What You Swap                | Everything Else Unchanged                          |
| ------------------------------------------- | ---------------------------- | -------------------------------------------------- |
| Switch from simulation to real cameras      | `PacketSource` impl          | Both aggregators, both smoothers, joiner, modeler  |
| Try Kalman for speed smoothing only         | Speed `Smoother` impl        | Occupancy lane entirely, ingest, joiner, modeler   |
| Change how uncovered edges are handled      | `WeightModel` impl           | Both lanes, joiner, output                         |
| Send weights to a file instead of HTTP      | `WeightOutput` impl          | Everything upstream                                |
| Replace tumbling windows with sliding       | Both `Aggregator` impls      | Ingest, both smoothers, joiner, modeler, output    |
| Add Kafka as the ingest source              | New `PacketSource` impl      | All downstream stages                              |
| Add a third signal (e.g. queue length)      | New aggregator + smoother    | Extend `EdgeJoiner`, existing lanes unchanged      |
| Change alignment strategy (e.g. hold-last)  | `EdgeJoiner` impl            | Both lanes, weight model, output                   |


### 4.3 Testing Strategy per Module

Each module is testable in isolation with synthetic inputs:

- **Ingest:** Verify it produces `CameraPacket` with both speed and occupancy
at the expected rate and distribution. Test: create a `SimulatedPacketSource`,
collect 1000 packets, check statistical properties of both signals.
- **Speed Aggregator:** Feed known `CameraPacket` sequence, verify emitted
`SpeedSummary` values (mean, count, variance) match hand-computed expectations.
- **Occupancy Aggregator:** Same, but verify `OccupancySummary` values. Confirm
occupancy stays clamped to [0.0, 1.0].
- **Smoother (either lane):** Feed a known sequence of summary batches. Verify
convergence, outlier rejection, trend tracking. Both lanes use the same
`Smoother<S>` interface, so the algorithm is tested once.
- **Joiner:** Feed two synthetic `Map<Int, SmootherState>` snapshots with
deliberately misaligned timestamps. Verify: aligned edges get both states,
speed-only edges get occupancy filled from interpolation, occupancy-only
edges get speed filled, stale gaps beyond threshold produce warnings.
- **Weight Model:** Create synthetic `JoinedEdgeState` maps. Verify: covered
edges use both speed and occupancy, uncovered edges get time-of-day
modulation, stale edges decay toward baseline, all weights are in
`[1, INFINITY-1]`.
- **Output:** Mock HTTP server. Verify the POST body is exactly
`numEdges * 4` bytes, little-endian u32.

---

## 5. Inter-Module I/O Data Formats

This section defines the **exact data types** that flow between modules. Each
boundary has one producer and one consumer. Modules are decoupled: a module
only needs to know the data type at its input and output boundaries, not the
internals of who produces or consumes it.

### 5.1 Module Boundary Map (Dual-Lane Architecture)

Each camera produces two signals. The pipeline splits into two parallel lanes
after ingest, then **joins** before the weight model:

```
                              ┌─────────────┐  SpeedSummary  ┌─────────────┐
                         ┌───→│ SpeedAggr.  │───────────────→│ SpeedSmooth │──┐
                         │    └─────────────┘                └─────────────┘  │
┌──────────┐ CameraPacket│                                                    │ Map<Int,SmootherState>
│ Ingest   │─────────────┤                                                    │  (×2)
│(simulat.)│  Channel    │                                                    ▼
└──────────┘             │    ┌─────────────┐  OccupSummary  ┌─────────────┐ ┌──────────┐
                         └───→│ OccupAggr.  │───────────────→│ OccupSmooth │→│  Joiner  │
                              └─────────────┘                └─────────────┘ └────┬─────┘
                                                                                  │ Map<Int,JoinedEdgeState>
                                                                                  ▼
                              ┌─────────────┐    IntArray    ┌─────────────┐
                              │  Output     │←───────────────│ WeightModel │
                              │(CustomizeHTP│                │  (modeler)  │
                              └─────────────┘                └─────────────┘
```

Gradle module boundaries:

| From Module    | To Module      | Crossing Type     | Data Format                          |
| -------------- | -------------- | ----------------- | ------------------------------------ |
| `simulation`   | `simulation`   | In-module channel | `Channel<CameraPacket>`              |
| `simulation`   | `smoother`     | Cross-module call | `List<SpeedSummary>` (speed lane)    |
| `simulation`   | `smoother`     | Cross-module call | `List<OccupancySummary>` (occup lane)|
| `smoother`     | `modeler`      | Cross-module call | `Map<Int, SmootherState>` (×2 maps)  |
| `modeler`      | `modeler`      | In-module join    | `Map<Int, JoinedEdgeState>`          |
| `modeler`      | hanoi-server   | HTTP POST         | `ByteArray` (little-endian u32)      |

Note: `simulation → smoother` and `smoother → modeler` are **function call
boundaries** orchestrated by `app/Main.kt`. The modules don't import each other
— `app` imports all three and wires them together. The joiner lives in `modeler`
because alignment is a weight-model concern (it decides how to handle gaps).

### 5.2 Boundary 1: Ingest → Aggregators (within `simulation`)

**Direction:** `CameraSimulator` produces → both aggregators consume
**Transport:** `Channel<CameraPacket>` (kotlinx.coroutines)
**Module:** All live in `simulation/`

```kotlin
/**
 * One observation from one camera at one point in time.
 * Carries BOTH speed and occupancy — the camera captures a single
 * snapshot that includes both measurements simultaneously.
 *
 * Lives in: simulation/ingest/CameraPacket.kt
 */
@Serializable
data class CameraPacket(
    val cameraId: Int,          // which camera produced this reading
    val timestampMs: Long,      // simulation time (NOT wall clock)
    val speedKmh: Float,        // flow velocity in km/h (may be noisy)
    val occupancy: Float,       // road occupancy fraction ∈ [0.0, 1.0]
    val confidence: Float       // 0.0–1.0, sensor self-reported quality
)
```

**Invariants:**
- `speedKmh >= 0.0` (negative speed is physically impossible)
- `occupancy ∈ [0.0, 1.0]` (0 = empty road, 1 = fully saturated)
- `confidence ∈ [0.0, 1.0]`
- `timestampMs` is monotonically non-decreasing per `cameraId`
- `cameraId` maps to a valid entry in `CameraEdgeMapping`

**Fan-out:** The ingest channel feeds a single `CameraPacket` to the simulation
module. Inside `simulation`, the packet is **demuxed** into the two aggregators:
the speed aggregator reads `speedKmh`, the occupancy aggregator reads `occupancy`.
Both receive the same packet — this guarantees they share the same `timestampMs`
and `cameraId`, which is the foundation of temporal alignment.

**Serialization:** In-memory only (channel). If Kafka is added later, serialize
as JSON or Protobuf via `PacketSource` implementation — the downstream
aggregators don't change.

### 5.3 Boundary 2a: Speed Aggregator → Speed Smoother

**Direction:** `SpeedAggregator` produces → speed `Smoother` consumes
**Transport:** `Channel<List<SpeedSummary>>` or direct function call from `app`
**Crossing:** `app` receives from the channel, passes to speed smoother

```kotlin
/**
 * Aggregated speed statistics for one edge over one tumbling window.
 *
 * Lives in: simulation/aggregator/SpeedSummary.kt
 */
@Serializable
data class SpeedSummary(
    val edgeId: Int,            // graph edge index (0-based, < numEdges)
    val meanSpeedKmh: Float,    // coverage-weighted mean speed in window
    val observationCount: Int,  // number of CameraPackets contributing
    val variance: Float,        // speed variance (for diagnostics)
    val windowEndMs: Long       // simulation time at window close
)
```

### 5.4 Boundary 2b: Occupancy Aggregator → Occupancy Smoother

**Direction:** `OccupancyAggregator` produces → occupancy `Smoother` consumes
**Transport:** `Channel<List<OccupancySummary>>` or direct function call
**Crossing:** `app` receives from the channel, passes to occupancy smoother

```kotlin
/**
 * Aggregated occupancy statistics for one edge over one tumbling window.
 *
 * Lives in: simulation/aggregator/OccupancySummary.kt
 */
@Serializable
data class OccupancySummary(
    val edgeId: Int,            // graph edge index (0-based, < numEdges)
    val meanOccupancy: Float,   // coverage-weighted mean occupancy ∈ [0.0, 1.0]
    val observationCount: Int,  // number of CameraPackets contributing
    val variance: Float,        // occupancy variance (for diagnostics)
    val windowEndMs: Long       // simulation time at window close
)
```

**Invariants (both summary types):**
- `edgeId ∈ [0, numEdges)` — always a valid graph edge index
- `observationCount >= 1` (empty edges are omitted, not sent with count 0)
- `meanSpeedKmh > 0.0` / `meanOccupancy ∈ [0.0, 1.0]`
- `variance >= 0.0`
- One summary per edge per window (duplicates are merged by aggregator)

**Batch semantics:** Each smoother receives its own `List<*Summary>` — one batch
per window tick. Both aggregators use the **same window boundary** (same
`windowEndMs`), so batches are naturally aligned in time. Not all edges appear
in every batch; absent edges retain their previous smoother state.

### 5.5 Boundary 3: Smoothers → Joiner (`smoother` → `modeler`)

**Direction:** Both smoothers produce snapshots → `EdgeJoiner` consumes
**Transport:** Direct function call via `app` orchestration
**Crossing:** `app` calls both `.snapshot()`, passes both maps to joiner in `modeler`

```kotlin
/**
 * The smoothed state of one edge for ONE signal (speed or occupancy).
 * The SmootherState type is shared between both lanes — the joiner
 * interprets the `level` field based on which map it came from.
 *
 * Lives in: smoother/SmootherState.kt
 */
@Serializable
data class SmootherState(
    val level: Double,          // smoothed value (km/h for speed, 0–1 for occupancy)
    val trend: Double,          // change per window
    val lastUpdateMs: Long,     // sim time of last observation (for TTL)
    val observationCount: Int   // cumulative observations (for confidence)
)
```

**Transfer format:** Two `Map<Int, SmootherState>` — one from the speed smoother,
one from the occupancy smoother. Keys are `edgeId`. Only edges that have ever
received data appear in each map. An edge may appear in one map but not the other
if a camera reported speed but not occupancy (or vice versa) — this is the
**misalignment problem** addressed by the joiner.

**Invariants:**
- `level > 0.0` for speed; `level ∈ [0.0, 1.0]` for occupancy
- `lastUpdateMs <= nowMs`
- `observationCount >= 1`
- Each map is an **immutable snapshot** — the joiner reads both, each smoother
  continues mutating its own internal state independently

### 5.6 Boundary 3.5: Joiner → Weight Model (within `modeler`)

**Direction:** `EdgeJoiner.join()` produces → `LiveWeightModel.computeWeights()` consumes
**Transport:** In-module function call
**Module:** Both live in `modeler/`

```kotlin
/**
 * Combined speed + occupancy state for one edge, after alignment.
 * This is what the weight model uses to compute travel times.
 *
 * Lives in: modeler/JoinedEdgeState.kt
 */
@Serializable
data class JoinedEdgeState(
    val speedState: SmootherState,      // smoothed speed for this edge
    val occupancyState: SmootherState,  // smoothed occupancy for this edge
    val alignmentAge: Long              // |speed.lastUpdateMs - occupancy.lastUpdateMs|
)
```

**Transfer format:** `Map<Int, JoinedEdgeState>` — key is `edgeId`. Only edges
with at least one signal present appear. The joiner resolves three cases:

| Speed data? | Occupancy data? | Joiner behavior                                   |
| ----------- | --------------- | ------------------------------------------------- |
| Yes         | Yes             | Direct join; `alignmentAge` = timestamp difference |
| Yes         | No              | Interpolate occupancy from speed (see §5.8)        |
| No          | Yes             | Interpolate speed from occupancy (see §5.8)        |
| No          | No              | Edge absent from map → uncovered                   |

`alignmentAge` is advisory — the weight model can use it to discount edges where
the two signals are far apart in time.

### 5.7 Boundary 4: Weight Model → HTTP Output (`modeler` → hanoi-server)

**Direction:** `LiveWeightModel.computeWeights()` produces → `CustomizeClient.deliver()` sends
**Transport:** In-memory `IntArray`, then serialized to HTTP POST body
**Crossing:** Both live in `modeler/`

```kotlin
/**
 * Complete weight vector for CCH customization.
 *
 * Produced by: modeler/LiveWeightModel.kt
 * Consumed by: modeler/output/CustomizeClient.kt
 * Delivered to: POST http://<server>/customize
 */
val weights: IntArray  // exactly numEdges elements (1,869,499 for Hanoi)
```

**Wire format (HTTP body):**
- Length: `numEdges × 4` bytes (7,477,996 bytes for Hanoi)
- Encoding: **little-endian unsigned 32-bit integers** (u32)
- Each element: travel time in **milliseconds**
- Byte order: native x86 little-endian (matches RoutingKit binary format)

```kotlin
// Serialization in CustomizeClient:
val buffer = ByteBuffer.allocate(weights.size * 4).order(ByteOrder.LITTLE_ENDIAN)
buffer.asIntBuffer().put(weights)
val body: ByteArray = buffer.array()
// POST body is exactly `body`, Content-Type: application/octet-stream
```

**Invariants:**
- `weights.size == numEdges` (exact match; server rejects mismatched sizes)
- `weights[i] ∈ [1, 2_147_483_646]` for all `i`
  - `0` creates routing black holes (edge appears free but is unreachable)
  - `2_147_483_647` (INFINITY) breaks CCH triangle relaxation
- No NaN, no negative values (IntArray can't hold these, but the `Float → Int`
  conversion in the weight model must guard against it)

### 5.8 Temporal Misalignment: Problem & Solution

**The problem:** Speed and occupancy observations come from the same camera, but
after aggregation and independent smoothing, the two signals can drift apart:
- A camera reports speed every 5s but occupancy every 10s (sensor quirk)
- A camera drops out on one signal but not the other
- Network jitter delivers packets out of order, so one aggregator window
  captures a reading that the other window missed

At the joiner, you might see `speed.lastUpdateMs = 120_000` and
`occupancy.lastUpdateMs = 90_000` for the same edge — a 30-second gap. Combining
a "current" speed with a "30 seconds stale" occupancy produces a physically
inconsistent snapshot.

**Solution: Three-tier alignment strategy in `EdgeJoiner`:**

```
Tier 1: Co-temporal (alignmentAge < windowSize, i.e. < 30s)
  → Use both values directly. They're from the same or adjacent windows.
  → alignmentAge is informational only; no adjustment needed.

Tier 2: Stale gap (windowSize ≤ alignmentAge < staleThresholdMs)
  → The fresher signal is trusted. The staler signal's SmootherState
  → is used but with a decayed confidence:
  →   staleConf = 1.0 - (alignmentAge - windowSize) / (staleThresholdMs - windowSize)
  → The weight model applies this as an additional multiplier.

Tier 3: Dead gap (alignmentAge ≥ staleThresholdMs)
  → The staler signal is discarded entirely. The edge is treated as
  → having only one signal. Interpolation fills the missing lane:
  →   - Missing occupancy: estimate from speed via fundamental diagram
  →     occupancy_est = 1.0 - (speed / freeFlowSpeed)
  →   - Missing speed: estimate from occupancy via fundamental diagram
  →     speed_est = freeFlowSpeed * (1.0 - occupancy)
```

```kotlin
/**
 * Lives in: modeler/EdgeJoiner.kt
 */
class DefaultEdgeJoiner(
    private val windowSizeMs: Long = 30_000,
    private val staleThresholdMs: Long = 5 * 60 * 1000,
    private val freeFlowSpeeds: FloatArray       // per-edge, from baseline
) : EdgeJoiner {

    override fun join(
        speedStates: Map<Int, SmootherState>,
        occupancyStates: Map<Int, SmootherState>,
        nowMs: Long
    ): Map<Int, JoinedEdgeState> {
        val allEdges = speedStates.keys + occupancyStates.keys
        return allEdges.associateWith { edgeId ->
            val speed = speedStates[edgeId]
            val occup = occupancyStates[edgeId]
            when {
                speed != null && occup != null -> joinBoth(edgeId, speed, occup)
                speed != null -> joinSpeedOnly(edgeId, speed)
                else -> joinOccupancyOnly(edgeId, occup!!)
            }
        }
    }
    // ...
}
```

**Why this works for simulation:** In the simulation scenario, both signals come
from the same `CameraPacket` and share the same `timestampMs`. Both aggregators
use the same window boundaries. So `alignmentAge` will be 0 for nearly all edges
in normal operation — Tier 1 applies almost everywhere. Tiers 2 and 3 are
safety nets for camera dropout, partial failure, and production scenarios where
speed and occupancy sensors may genuinely have different reporting cadences.

### 5.9 Supplementary Data: Graph + Mapping (loaded at startup)

These are not inter-module "flows" but **shared read-only data** loaded once at
startup by `app` and passed to modules that need them:

```kotlin
/**
 * Graph topology and baseline weights, loaded from RoutingKit binary files.
 *
 * Lives in: simulation/graph/GraphData.kt
 * Consumed by: simulation (camera sim), modeler (baseline weights, geo_distance)
 */
data class GraphData(
    val numNodes: Int,
    val numEdges: Int,
    val firstOut: IntArray,      // [numNodes + 1] CSR offset array
    val head: IntArray,          // [numEdges] target node per edge
    val travelTime: IntArray,    // [numEdges] baseline travel time (ms)
    val geoDistance: IntArray,   // [numEdges] edge length (meters)
    val latitude: FloatArray,    // [numNodes] node latitude
    val longitude: FloatArray    // [numNodes] node longitude
)

/**
 * Camera-to-edge mapping, loaded from JSON config or database.
 *
 * Lives in: simulation/mapping/CameraMappingSource.kt
 * Consumed by: simulation (camera sim + aggregator), modeler (influence map)
 */
data class CameraEdgeMapping(
    val cameraId: Int,
    val edges: List<EdgeInfluence>
)

data class EdgeInfluence(
    val edgeId: Int,
    val weight: Float            // 0.0–1.0, coverage fraction
)
```

**Passing convention:** `app/Main.kt` loads `GraphData` and mapping at startup,
then passes references to each module's constructor. Modules never load data
files themselves — they receive pre-loaded, validated data.

### 5.10 Complete Data Flow Summary

```
                         ┌─────── STARTUP (once) ───────┐
                         │                               │
                    GraphData                    CameraEdgeMapping
                    (binary files)               (JSON config)
                         │                               │
                         ▼                               ▼
              ┌─────── app/Main.kt ──────────────────────┐
              │  Loads graph, mapping, wires modules      │
              │  Passes references to constructors        │
              └──────┬──────────┬──────────┬──────────┬───┘
                     │          │          │          │
                     ▼          │          │          │
              simulation       │          │          │
              ┌────────────┐   │          │          │
              │ CameraSim  │   │          │          │
              │  → CameraPacket (channel) │          │
              │                │          │          │
              │ ┌────────────┐ │          │          │
              │ │SpeedAggr.  │─┼─→ List<SpeedSummary>│
              │ └────────────┘ │          │          │
              │ ┌────────────┐ │          │          │
              │ │OccupAggr.  │─┼─→ List<OccupancySummary>
              │ └────────────┘ │          │          │
              └────────────┘   │          │          │
                       │       │          │          │
                       ▼       │          │          │
              ┌── app loop ────┘          │          │
              │  receives both batches    │          │
              │  passes to smoothers ─────┘          │
              └────────┬──────────────────┘          │
                       │                             │
                       ▼                             │
              ┌────────────┐                         │
              │SpeedSmooth │→ Map<Int,SmootherState> │
              └────────────┘         │               │
              ┌────────────┐         │               │
              │OccupSmooth │→ Map<Int,SmootherState> │
              └────────────┘         │               │
                                     ▼               │
              ┌── app loop ──────────────────────────┘
              │  passes both snapshots to modeler
              └────────┬───────────────────┘
                       │
                       ▼
                 modeler
              ┌────────────────────────────┐
              │ EdgeJoiner                 │
              │  .join(speed, occup, now)  │
              │  → Map<Int,JoinedEdgeState>│
              │                            │
              │ LiveWeightModel            │
              │  .computeWeights(          │
              │     joinedStates,          │
              │     nowMs                  │
              │  ) → IntArray              │
              │                            │
              │ CustomizeClient            │
              │  .deliver(IntArray)        │
              │  → POST /customize         │
              │    (7.5 MB binary)         │
              └────────────────────────────┘
```

---

## 6. Camera-Edge Mapping

### 6.1 Data Model (Shared Types)

```kotlin
/** One camera's relationship to the road network. */
data class CameraEdgeMapping(
    val cameraId: Int,
    val edges: List<EdgeInfluence>
)

data class EdgeInfluence(
    val edgeId: Int,
    val weight: Float  // 0.0–1.0: how much of this edge the camera covers
)

/**
 * Reverse index: for each edge, which cameras observe it?
 * Used by the aggregator to merge observations from multiple cameras
 * covering the same edge.
 */
typealias ReverseIndex = Map<Int, List<CameraRef>>

data class CameraRef(
    val cameraId: Int,
    val weight: Float
)
```

### 6.2 Simulation Backbone: Config-Driven Mapping

For simulation, the mapping is loaded from a JSON config file:

```json
{
  "mappings": [
    { "cameraId": 0, "edges": [{ "edgeId": 42, "weight": 1.0 }] },
    { "cameraId": 1, "edges": [{ "edgeId": 108, "weight": 0.7 }, { "edgeId": 109, "weight": 0.3 }] }
  ]
}
```

**Auto-generation for simulation convenience:** A CLI flag
`--auto-cameras <strategy>` generates the config from graph metadata:


| Strategy             | Description                                                       |
| -------------------- | ----------------------------------------------------------------- |
| `tertiary-plus`      | One camera per edge with implied speed >= 30 km/h (~166K cameras) |
| `random:<N>`         | N cameras assigned to random edges                                |
| `uniform:<fraction>` | Fraction of all edges get a camera (e.g., `uniform:0.1` = 10%)    |


The auto-generator reads `travel_time` and `geo_distance` to compute implied
speeds, then outputs the same JSON format. The pipeline itself doesn't care
how the config was produced — it just reads the JSON.

### 6.3 Production Recommendation: Database-Backed Mapping

When real cameras exist and a database is available, the mapping source changes
but the interface stays the same:

```kotlin
/**
 * Abstraction over mapping source.
 * Simulation: reads JSON file.
 * Production: queries database.
 */
interface CameraMappingSource {
    suspend fun loadMappings(): List<CameraEdgeMapping>
    suspend fun reverseIndex(): ReverseIndex
}

class JsonFileMappingSource(private val path: Path) : CameraMappingSource { ... }

class DatabaseMappingSource(private val db: DataSource) : CameraMappingSource {
    // Queries a table like:
    //   camera_edge_map (camera_id INT, edge_id INT, weight FLOAT, active BOOLEAN)
    //
    // This table is maintained by a separate GIS process that:
    //   1. Takes camera GPS coordinates
    //   2. Snaps each camera to the nearest edge(s) using spatial indexing
    //   3. Computes coverage weight based on camera FOV and edge geometry
    //   4. Writes the mapping rows
    //
    // The pipeline just reads the result — it doesn't do the snapping itself.
    ...
}
```

**Database schema recommendation:**

```sql
CREATE TABLE camera_edge_map (
    camera_id   INT NOT NULL,
    edge_id     INT NOT NULL,
    weight      REAL NOT NULL DEFAULT 1.0,  -- coverage fraction
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (camera_id, edge_id)
);

CREATE INDEX idx_edge_cameras ON camera_edge_map(edge_id) WHERE active;
```

**The GIS snapping process** (separate from this pipeline, built when cameras
are deployed) would:

1. Read camera GPS coordinates from a camera registry
2. Load graph node coordinates (latitude/longitude)
3. Use a spatial index (KD-tree or R-tree) to find the nearest edge(s)
4. Account for camera direction/FOV to determine which edge direction(s) and
  what fraction of the edge is covered
5. Write to `camera_edge_map`

The pipeline polls this table at startup (and optionally on a refresh interval)
to pick up new cameras or deactivated ones.

---

## 7. Tech Stack & Module Design

### 7.1 Language: Kotlin

**Rationale:**

- Data pipelines are I/O-bound and exploratory — Kotlin's coroutines,
fast iteration (hot Gradle daemon ~2-3s incremental), and data class
ergonomics fit better than Rust for this workload.
- Structured concurrency maps naturally to the pipeline topology:
camera coroutines → channel → aggregator → smoother → weight model.
- JVM is battle-tested for long-running services. GC is mature.
- The boundary to the Rust routing engine is a single HTTP POST of binary
bytes — language-agnostic, already defined.
- Future ML integration (GNN traffic propagation, anomaly detection) is easier
from JVM (ONNX Runtime, DJL) or via Python subprocess than from Rust.

**Binary format handling:**

```kotlin
// Producing the weight vector for POST /customize:
val weights: IntArray = weightModel.computeWeights(...)
val buffer = ByteBuffer.allocate(weights.size * 4).order(ByteOrder.LITTLE_ENDIAN)
buffer.asIntBuffer().put(weights)
httpClient.post("http://localhost:9080/customize") {
    setBody(buffer.array())
}
```

### 7.2 Project Structure

Kotlin/Gradle multi-module project: `CCH_Data_Pipeline/`

Each Gradle subproject is a self-contained module with its own `build.gradle.kts`
and test suite. Modules communicate only through the data types defined in their
public APIs — no module imports internals from another.

```
CCH_Data_Pipeline/
├── build.gradle.kts                 # Root: plugin declarations (apply false)
├── settings.gradle.kts              # includes: app, simulation, smoother, modeler
├── gradle/
│   └── libs.versions.toml          # Version catalog (single source of truth)
│
├── app/                             # Application entry point + wiring
│   ├── build.gradle.kts            #   depends on: simulation, smoother, modeler
│   └── src/main/kotlin/com/thomas/cch_app/
│       └── Main.kt                 #   CLI (clikt), pipeline orchestration
│
├── simulation/                      # Ingest + dual-lane aggregation + graph I/O
│   ├── build.gradle.kts            #   depends on: (standalone, no project deps)
│   └── src/main/kotlin/com/thomas/simulation/
│       ├── graph/                  #   RoutingKit binary I/O
│       │   ├── GraphData.kt       #     Loads first_out, head, travel_time, geo_distance, lat, lng
│       │   └── BinaryIO.kt        #     Little-endian u32/f32 vector I/O
│       ├── mapping/               #   Camera-edge mapping
│       │   ├── CameraMappingSource.kt  # Interface
│       │   ├── JsonFileMappingSource.kt
│       │   └── AutoGenerator.kt   #     Auto-generate camera configs from graph
│       ├── ingest/                #   Camera simulation
│       │   ├── PacketSource.kt    #     Interface: produces ReceiveChannel<CameraPacket>
│       │   ├── CameraSimulator.kt #     Batched coroutine generators (speed + occupancy)
│       │   └── CameraPacket.kt    #     Data class (carries both signals)
│       └── aggregator/            #   Dual-lane windowed aggregation
│           ├── DualAggregator.kt  #     Demuxes CameraPacket into two summary channels
│           ├── SpeedSummary.kt    #     Data class (speed lane output)
│           └── OccupancySummary.kt#     Data class (occupancy lane output)
│
├── smoother/                        # Huber DES smoothing (generic, used for both lanes)
│   ├── build.gradle.kts            #   depends on: (standalone, no project deps)
│   └── src/main/kotlin/com/thomas/smoother/
│       ├── Smoother.kt            #   Interface: Smoother<S>
│       ├── HuberDesSmoother.kt    #   Core algorithm (generic over summary type)
│       ├── SmootherState.kt       #   Per-edge state (shared type for both lanes)
│       └── SmootherConfig.kt      #   alpha, beta, delta parameters (different per lane)
│
└── modeler/                         # Weight modelling + joining + output
    ├── build.gradle.kts            #   depends on: (standalone, no project deps)
    └── src/main/kotlin/com/thomas/modeler/
        ├── EdgeJoiner.kt          #   Interface + DefaultEdgeJoiner (alignment logic)
        ├── JoinedEdgeState.kt     #   Combined speed + occupancy per edge
        ├── WeightModel.kt         #   Interface
        ├── LiveWeightModel.kt     #   Main impl (speed→weight + occupancy scaling)
        ├── TimeOfDay.kt           #   Gaussian time-of-day modulation
        ├── InfluenceMap.kt        #   Neighbor congestion propagation (BFS)
        ├── StalenessPolicy.kt     #   TTL handling
        └── output/
            └── CustomizeClient.kt #   HTTP POST to hanoi-server /customize
```

### 7.3 Dependencies

```kotlin
// build.gradle.kts
dependencies {
    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")

    // HTTP client
    implementation("io.ktor:ktor-client-core:3.1.0")
    implementation("io.ktor:ktor-client-cio:3.1.0")

    // JSON (config files, logging)
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")

    // CLI argument parsing
    implementation("com.github.ajalt.clicker:clikt:5.0.2")

    // Logging
    implementation("io.github.oshai:kotlin-logging:7.0.3")
    implementation("ch.qos.logback:logback-classic:1.5.15")

    // Testing
    testImplementation("org.jetbrains.kotlin:kotlin-test")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
}
```

No Kafka, no Redis, no database driver. Pure Kotlin, single fat JAR.

### 7.4 Coroutine Architecture

```kotlin
fun main() = runBlocking {
    val graph = GraphData.load(graphDir)
    val mapping = mappingSource.loadMappings()
    val influenceMap = InfluenceMap.precompute(graph, mapping.coveredEdges())
    val smoother = HuberDesSmoother(config, graph.numEdges)
    val weightModel = LiveWeightModel(graph, influenceMap, stalenessPolicy)

    // In-process channel — replaces Kafka for simulation
    val packetChannel = Channel<CameraPacket>(capacity = Channel.BUFFERED)

    // Camera simulators: batched coroutines producing packets (speed + occupancy)
    val simJob = launch {
        CameraSimulator(mapping, graph, simClock).run(packetChannel)
    }

    // Dual-lane aggregators: both consume from the same packet channel
    val speedSummaryChannel = Channel<List<SpeedSummary>>(capacity = 1)
    val occupSummaryChannel = Channel<List<OccupancySummary>>(capacity = 1)
    val aggJob = launch {
        DualAggregator(packetChannel, simClock, windowDuration)
            .run(speedSummaryChannel, occupSummaryChannel)
    }

    // Main pipeline loop — collects both lanes per window tick
    for (speedBatch in speedSummaryChannel) {
        val occupBatch = occupSummaryChannel.receive()  // same window boundary

        speedSmoother.update(speedBatch)
        occupancySmoother.update(occupBatch)

        val joined = joiner.join(
            speedSmoother.snapshot(),
            occupancySmoother.snapshot(),
            simClock.now()
        )
        val weights = weightModel.computeWeights(joined, simClock.now())
        customizeClient.post(weights)
        logger.info { "Window complete: ${speedBatch.size} speed + ${occupBatch.size} occup edges" }
    }
}
```

**Key design point:** The `Channel<CameraPacket>` is the simulation stand-in
for Kafka/NATS. The `PacketSource` interface abstracts this:

```kotlin
interface PacketSource {
    /** Infinite stream of camera packets (speed + occupancy). */
    fun packets(): ReceiveChannel<CameraPacket>
}

class SimulatedPacketSource(
    private val simulator: CameraSimulator,
    private val scope: CoroutineScope
) : PacketSource {
    override fun packets(): ReceiveChannel<CameraPacket> {
        val channel = Channel<CameraPacket>(Channel.BUFFERED)
        scope.launch { simulator.run(channel) }
        return channel
    }
}

// Production (future):
class KafkaPacketSource(private val consumer: KafkaConsumer) : PacketSource { ... }
class NatsPacketSource(private val connection: Connection) : PacketSource { ... }
```

When the DE department provides a Kafka/NATS deployment, you implement the
corresponding `PacketSource` and swap it in `Main.kt`. The rest of the
pipeline — aggregator, smoother, weight model, HTTP output — stays untouched.

### 7.5 Staleness TTL Design

```kotlin
data class StalenessPolicy(
    val staleThresholdMs: Long = 5 * 60 * 1000,   // 5 minutes
    val deadThresholdMs: Long = 30 * 60 * 1000,    // 30 minutes
)

/**
 * Called by the weight model for each covered edge during weight computation.
 *
 * Returns a confidence multiplier in [0.0, 1.0]:
 *   - Fresh (< staleThreshold): 1.0
 *   - Stale (between stale and dead): linear decay from 1.0 → 0.0
 *   - Dead (> deadThreshold): 0.0 (pure baseline)
 */
fun StalenessPolicy.confidence(lastUpdateMs: Long, nowMs: Long): Float {
    val age = nowMs - lastUpdateMs
    return when {
        age < staleThresholdMs -> 1.0f
        age > deadThresholdMs -> 0.0f
        else -> 1.0f - (age - staleThresholdMs).toFloat() /
                        (deadThresholdMs - staleThresholdMs).toFloat()
    }
}
```

**How it integrates with the weight model (dual-lane):**

```
For each covered edge e in joinedStates:
    // Staleness is evaluated per-lane — use the WORSE of the two
    speed_conf = stalenessPolicy.confidence(joined[e].speedState.lastUpdateMs, now)
    occup_conf = stalenessPolicy.confidence(joined[e].occupancyState.lastUpdateMs, now)
    staleness_conf = min(speed_conf, occup_conf)

    observation_conf = min(joined[e].speedState.observationCount / CONF_SATURATION, 1.0)
    alignment_conf = if joined[e].alignmentAge < windowSize then 1.0
                     else max(0.0, 1.0 - alignmentAge / staleThresholdMs)

    effective_conf = staleness_conf * observation_conf * alignment_conf

    if effective_conf > 0:
        live_speed = joined[e].speedState.level + joined[e].speedState.trend
        occupancy_factor = 1.0 + OCCUPANCY_WEIGHT * (joined[e].occupancyState.level - 0.2)
        live_weight = speed_to_travel_time(live_speed) * occupancy_factor
        weight[e] = lerp(baseline[e], live_weight, effective_conf)
    else:
        // Dead — treat as uncovered
        weight[e] = baseline[e] * timeOfDayFactor(hour)
        // Also apply neighbor propagation if other nearby edges are still live
```

The linear decay between stale and dead thresholds creates a **graceful
degradation** — the edge doesn't snap from "fully live" to "fully baseline."
Instead, it smoothly blends back over 25 minutes (between the 5-minute stale
mark and the 30-minute dead mark).

---

## 8. Detailed Algorithm Design

### 8.1 Huber DES Algorithm

Standard Double Exponential Smoothing (Holt's method) with Huber loss for
robust parameter updates. The **same algorithm** is used for both the speed
lane and the occupancy lane — only the parameters differ.

```
For each window summary (edge_id, observed_value):
    predicted = level + trend
    residual  = observed_value - predicted

    // Huber weighting: downweight large residuals
    if |residual| <= delta:
        w = 1.0
    else:
        w = delta / |residual|

    // Weighted DES update
    level = alpha * (w * observed_value + (1-w) * predicted) + (1-alpha) * (level + trend)
    trend = beta  * (level - prev_level) + (1-beta) * trend
```

**Parameters per lane:**

| Parameter | Speed Lane  | Occupancy Lane | Rationale                                          |
| --------- | ----------- | -------------- | -------------------------------------------------- |
| `alpha`   | 0.3         | 0.2            | Occupancy changes more slowly; lower α = more inertia |
| `beta`    | 0.1         | 0.05           | Occupancy trend is even more gradual                |
| `delta`   | 15 km/h     | 0.15           | 15% occupancy jump is suspicious; scale matches unit |

The `Smoother<S>` generic interface means both lanes use the same Huber DES
implementation — only the config and the summary type differ.

### 8.2 Weight Model (Separate Module)

The weight model is explicitly separated from the smoother. It receives
**joined** speed + occupancy state and produces travel-time weights:

```kotlin
interface WeightModel {
    /**
     * Given joined speed/occupancy states and the current simulation time,
     * produce a complete weight vector for ALL edges.
     *
     * The returned IntArray has exactly numEdges elements,
     * each a u32 travel time in milliseconds, in [1, 2_147_483_646].
     */
    fun computeWeights(
        states: Map<Int, JoinedEdgeState>,
        nowMs: Long
    ): IntArray
}
```

**`LiveWeightModel` implementation responsibilities:**

1. **Covered edges with fresh data:** Convert smoothed speed to travel time
  via `geo_distance`. Apply **occupancy scaling** — high occupancy inflates
  the travel time even if speed hasn't dropped yet (anticipates imminent
  slowdown). Confidence-blend with baseline. Apply staleness TTL.
2. **Occupancy scaling formula:**
   `occupancy_factor = 1.0 + OCCUPANCY_WEIGHT * (occupancy - FREE_FLOW_OCCUPANCY)`
   where `OCCUPANCY_WEIGHT ≈ 0.5` and `FREE_FLOW_OCCUPANCY ≈ 0.2`.
   At occupancy 0.8: factor = 1.3 (30% travel time inflation).
3. **Covered edges gone stale:** Gradual blend back to baseline per TTL policy.
  Once dead, treat as uncovered.
4. **Uncovered edges with influence:** Apply time-of-day modulation +
  neighbor congestion propagation from the influence map.
5. **Uncovered edges without influence:** Apply time-of-day modulation only.
6. **Clamping:** All weights clamped to `[1, 2_147_483_646]`. Zero-weight
  edges create routing black holes. Weights >= INFINITY (2,147,483,647)
   break CCH triangle relaxation.

### 8.3 Camera Simulator Design

```kotlin
/**
 * Batched camera simulation. With 166K cameras and 64 coroutines,
 * each coroutine handles ~2,600 cameras per tick.
 *
 * Each camera generates readings with:
 *   - baseSpeed: derived from the edge's baseline travel_time + geo_distance
 *   - baseOccupancy: inversely correlated with speed via fundamental diagram
 *   - noise: Gaussian with configurable std dev (independent per signal)
 *   - spikeProbability: chance of outlier (tests Huber robustness)
 *   - timeOfDayModulation: rush hour slowdown + occupancy increase
 *
 * Speed and occupancy are generated from the same camera snapshot,
 * sharing the same timestampMs — guaranteeing co-temporality at ingest.
 *
 * Time acceleration: 1 real second = N sim minutes (configurable).
 * At 60x, a full 24-hour cycle completes in 24 real minutes.
 */
class CameraSimulator(
    private val mapping: List<CameraEdgeMapping>,
    private val graph: GraphData,
    private val clock: SimClock,
    private val batchCount: Int = 64,
    private val intervalMs: Long = 5000  // 5s between readings per camera
)
```

---

## 9. Coverage Model: Quantified Analysis

### 9.1 Hanoi Graph Breakdown by Road Class

Reverse-engineered from `travel_time` and `geo_distance` using RoutingKit's
speed-to-class mapping in `osm_profile.cpp`:


| Highway Class            | Default Speed | Edge Count    | % of Total |
| ------------------------ | ------------- | ------------- | ---------- |
| motorway                 | 100 km/h      | 906           | 0.05%      |
| trunk                    | 70 km/h       | 10,837        | 0.6%       |
| primary                  | 50 km/h       | 37,238        | 2.0%       |
| secondary                | 40 km/h       | 33,582        | 1.8%       |
| **tertiary**             | **30 km/h**   | **74,089**    | **4.0%**   |
| residential/unclassified | 20 km/h       | 1,250,644     | 66.9%      |
| living_street            | 10 km/h       | 1,003         | 0.1%       |
| service/track            | 4 km/h        | 439,933       | 23.5%      |
| junction                 | 15 km/h       | 2,316         | 0.1%       |
| ferry                    | 5 km/h        | 86            | 0.0%       |
| custom maxspeed          | varies        | 9,829         | 0.5%       |
| zero-length              | —             | 9,036         | 0.5%       |
| **TOTAL**                |               | **1,869,499** | **100%**   |


### 9.2 Camera Placement Is Decoupled from Highway Class

In reality, camera placement doesn't follow highway classification neatly.
A tertiary road might have no camera; a busy residential road near a school
might have one. The simulation should accept an **arbitrary set of covered
edge IDs** — not derive it from road class.

For simulation convenience, the `--auto-cameras tertiary-plus` flag generates
a config from the speed-based heuristic (~166K cameras). But the architecture
must not assume this — the same pipeline should work with 500 cameras or
500,000.

### 9.3 Ingest Rate Analysis


| Camera Count     | Interval | Packets/sec |
| ---------------- | -------- | ----------- |
| 166K (tertiary+) | 5s       | 33,300      |
| 166K (tertiary+) | 10s      | 16,650      |
| 166K (tertiary+) | 30s      | 5,550       |
| 2,000 (sparse)   | 5s       | 400         |


All well within Kotlin `Channel` capacity (millions/sec in-process).

---

## 10. Handling Uncovered Edges — The Central Design Problem

This is the most important section of the plan. The smoother, the simulator,
the channel — those are straightforward engineering. The hard question is:
**what happens to the 91% of edges that have no camera?**

If you get this wrong, the routing engine will exploit uncovered edges as
"free shortcuts" — routing through quiet residential alleys to dodge congested
arterials, because those alleys still show free-flow baseline weights while
the arterial weights have been inflated by live data. This is called
**rat-running** and it's the #1 artifact of partial-coverage live weight
systems.

The goal is: the weight profile should look like a coherent traffic snapshot
where the whole city is affected by the same time-of-day patterns and spatial
congestion patterns — not a patchwork of live islands in a frozen baseline.

### Strategy 1: Static Baseline (Starting Point)

```
weight[e] = baseline_travel_time[e]  // from disk, never changes
```

**When to use:** Initial pipeline bringup only. Proves the pipeline produces
valid weight vectors and that `/customize` accepts them.

**Why it's not enough:** Consider a covered primary road showing 2.5x travel
time (heavy congestion). The parallel residential street 50 meters away still
shows its free-flow 20 km/h baseline. The router sees a 20 km/h residential
street as *faster* than a 50 km/h primary road at 2.5x congestion (effective
20 km/h). It routes through the residential street. In reality, that
residential street is also congested — cars are queuing to turn onto the
primary road, there's school pickup traffic, motorbikes are everywhere. But
the baseline doesn't know that.

### Strategy 2: Time-of-Day Modulation

Apply a global time-of-day multiplier to all uncovered edges. This is the
cheapest way to make the entire graph "breathe" together.

```kotlin
fun timeOfDayFactor(hour: Double): Double {
    val morning = 0.25 * gaussian(hour, 7.5, 1.2)   // +25% at 7:30 AM peak
    val evening = 0.35 * gaussian(hour, 17.5, 1.5)   // +35% at 5:30 PM peak
    val night   = -0.15 * gaussian(hour, 2.0, 2.0)   // -15% at 2:00 AM dip
    return 1.0 + morning + evening + night
}

// For each uncovered edge:
weight[e] = (baseline[e] * timeOfDayFactor(simHour)).toInt()
```

**Why Gaussian bumps instead of discrete time blocks:** Traffic doesn't switch
from "normal" to "rush hour" at exactly 7:00 AM. A Gaussian gives smooth
transitions. For Hanoi, the morning peak is sharp (school + work converge at
7:00-8:00) while the evening peak is broader (staggered end times, dinner
traffic, social activity).

**Hanoi-specific parameters:**

- Morning peak: 7:30 AM, σ = 1.2 hours, amplitude +25%
- Evening peak: 5:30 PM, σ = 1.5 hours, amplitude +35%
- Night dip: 2:00 AM, σ = 2.0 hours, amplitude -15%

### Strategy 3: Neighbor Congestion Propagation

**congestion on a covered edge should "spill" into nearby uncovered edges**,
with intensity decaying by graph distance.

The physical intuition: if a primary road is jammed, the residential streets
that feed into it are also affected — cars queue at intersections, motorbikes
divert, pedestrians spill onto the road.

**Algorithm (inverted BFS — propagate FROM covered edges):**

```
// Precomputation (once at startup):
//   For each covered edge c, BFS outward through uncovered edges up to max_hops.
//   For each reached uncovered edge u at hop distance d:
//     influenceMap[u].add(Influence(coveredEdge=c, decay=0.5^d))

// Per window (after smoother runs):
for each uncovered edge u:
    if influenceMap[u] is empty:
        weight[u] = baseline[u] * timeOfDayFactor(hour)
        continue

    // Weighted average of congestion ratios from nearby covered edges
    avgRatio = weightedMean(
        influenceMap[u].map { coveredWeight[it.edge] / baseline[it.edge] },
        influenceMap[u].map { it.decay }
    )

    spilloverRatio = lerp(1.0, avgRatio, SPILLOVER_STRENGTH)
    weight[u] = baseline[u] * spilloverRatio * timeOfDayFactor(hour)
    weight[u] = clamp(weight[u], 1, INFINITY - 1)
```

`**SPILLOVER_STRENGTH**` ∈ [0.0, 1.0]:

- 0.3 = mild (2x congested primary → 1.3x residential at 1 hop)
- 0.5 = moderate (recommended starting point)

**Performance:**

- BFS precomputation: ~166K starting edges × ~84 edges per BFS ≈ 14M visits → ~2-5s
- Influence map: ~14M entries → ~112 MB
- Per-window update: ~~1.7M uncovered edges × ~8 influences = ~14M ops → **~~50-100ms on JVM** (still negligible vs. 30s window)

**Rat-running prevention example:**

Primary road goes 1.0x → 2.5x congestion:

- 1 hop residential: **1.375x** baseline
- 2 hops: **1.1875x** baseline
- 3 hops: **1.09x** baseline

The router sees the residential street isn't free — it's 37.5% slower.

### Strategy 4: Directional Congestion Awareness (Advanced, Deferred)

Strategy 3 propagates congestion isotropically. Real congestion has
directionality: feeder streets are more affected than parallel streets.
Defer unless rat-running artifacts persist after Strategy 3.

### Strategy 5: Historical Profile Overlay (Production-Only, Deferred)

After collecting weeks of real camera data, build per-edge historical speed
profiles in 15-minute time-of-day bins. Replaces Strategy 2's synthetic
Gaussian curves with empirical patterns. Requires time-series storage.

### Uncovered Edge Decision Flow

```
                    Uncovered Edge Weight
                           │
                ┌──────────┴──────────┐
                │                     │
         Has influence?          No influence
         (Strategy 3)           (too far from
                │                any camera)
                ▼                     │
    baseline × spilloverRatio         │
    × timeOfDayFactor            ┌────┘
                │                │
                │                ▼
                │        baseline × timeOfDayFactor
                │                (Strategy 2)
                │                │
                └───────┬────────┘
                        │
                        ▼
                 clamp(1, INFINITY-1)
                        │
                        ▼
                 weight[e] = result
```

### Implementation Order

```
Phase 1:  Strategy 1 only (static baseline) — pipeline validation
Phase 2:  Strategy 2 (time-of-day) — whole graph breathes
Phase 3:  Strategy 3 (neighbor propagation) — spatial coherence
Phase 4:  Strategy 4 (directional) — only if rat-running persists
```

---

## 11. Simulation Scenarios


| Scenario               | Purpose                                                                                                                                        |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **Steady state**       | All cameras report normal speed. Weights should converge to ~baseline. Validates the pipeline doesn't drift.                                   |
| **Rush hour**          | Cameras on arterials report 40% speed drop. Weight vector should increase travel times on those edges; queries should route around congestion. |
| **Camera dropout**     | Kill 30% of cameras mid-run. Affected edges should gracefully degrade via staleness TTL (blend toward baseline over 25 min).                   |
| **Outlier burst**      | 10% of readings are 5x normal speed (sensor glitch). Huber weighting should suppress them; weights should not spike.                           |
| **Gradual congestion** | Speed drops linearly over 10 minutes. DES trend component should anticipate continued degradation.                                             |
| **Sparse coverage**    | Only 2,000 cameras (0.1% of edges). Test that neighbor propagation still produces coherent profiles.                                           |


---

## 12. What NOT to Build Yet

1. **GIS camera snapping** — The spatial matching of camera GPS to graph
  edges. Use config-driven mapping for now.
2. **Persistent smoother state** — State lives in memory. Pipeline restart
  re-converges from baseline. Fine for simulation.
3. **Kafka/NATS integration** — The `PacketSource` interface is the extension
  point. Implement when the DE team has broker infrastructure.
4. **Dashboard/monitoring** — Structured logging (JSON) is sufficient.
  Grafana/Prometheus is a production concern.
5. **Delta customization** — Always send the full weight vector. See Section
  2.3 for the documented future fix if sub-10s updates are needed.

---

## 13. Implementation Order

### Phase 1: Foundation

- `simulation/graph/` — RoutingKit binary I/O (read `first_out`, `head`,
`travel_time`, `geo_distance`, `latitude`, `longitude` as little-endian vectors)
- `simulation/mapping/` — `CameraMappingSource` interface + JSON loader +
auto-generator
- `smoother/` — Generic `Smoother<S>` interface + Huber DES with unit tests
(test with both speed and occupancy summary types)
- `modeler/WeightModel.kt` — Interface definition
- `modeler/EdgeJoiner.kt` — Interface + `DefaultEdgeJoiner` with alignment tiers
- **Deliverable:** Unit tests pass. Smoother converges on synthetic sequences
for both lanes. Graph loads correctly from Hanoi data directory.

### Phase 2: Weight Model + Joiner + Time-of-Day

- `modeler/LiveWeightModel.kt` — Baseline blending, occupancy scaling,
confidence, staleness TTL
- `modeler/TimeOfDay.kt` — Gaussian modulation
- `modeler/StalenessPolicy.kt` — TTL with linear decay
- `modeler/DefaultEdgeJoiner.kt` — Three-tier alignment (co-temporal, stale,
dead) with fundamental-diagram interpolation
- **Deliverable:** Given synthetic `JoinedEdgeState` maps, produces valid
`IntArray` weight vectors. Occupancy scaling visibly inflates travel time
at high occupancy. Alignment tiers handle mismatched timestamps correctly.

### Phase 3: Simulator + Dual-Lane Aggregator

- `simulation/ingest/CameraSimulator.kt` — Batched coroutines generating
`CameraPacket` with both speed and occupancy (correlated via fundamental diagram)
- `simulation/aggregator/DualAggregator.kt` — Demuxes packets into
`SpeedSummary` and `OccupancySummary` channels with shared window boundaries
- **Deliverable:** Cameras produce dual-signal packets, aggregator emits
synchronized speed and occupancy summary batches.

### Phase 4: End-to-End Pipeline

- `app/Main.kt` — Wire everything: ingest → dual aggregators → dual smoothers
→ joiner → weight model → HTTP output
- `modeler/output/CustomizeClient.kt` — HTTP client
- CLI args (graph dir, server URL, window size, camera config, time accel)
- **Deliverable:** Run alongside `hanoi_server`, observe weight customization,
run queries and see route changes during simulated rush hour. Verify both
speed and occupancy contribute to weight computation.

### Phase 5: Neighbor Propagation

- `modeler/InfluenceMap.kt` — BFS precomputation + per-window spillover
- **Deliverable:** Congestion on arterials visibly affects adjacent residential
street weights. Run sparse-coverage scenario to validate.

### Phase 6: Validation

- Run all 6 simulation scenarios from Section 11
- Verify smoother stability on both lanes, staleness recovery, outlier rejection
- Test alignment tiers: simulate camera dropout on one signal only
- Measure customization latency (window close → server ACK)
- **Deliverable:** Documented test results.

---

## 14. Summary of Recommendations


| Topic                          | Recommendation                                                                                                                                          |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Language**                   | Kotlin, `CCH_Data_Pipeline` Gradle multi-module project                                                                                                  |
| **Dual signals**               | Each `CameraPacket` carries speed + occupancy; split into parallel lanes after ingest                                                                    |
| **Streaming**                  | Kotlin coroutine `Channel` for simulation; `PacketSource` interface for future broker swap                                                               |
| **Camera mapping**             | Config-driven (JSON) now; `CameraMappingSource` interface for database-backed production                                                                 |
| **Smoothing**                  | Generic `Smoother<S>` with Huber DES; two instances (speed lane, occupancy lane) with different parameters                                               |
| **Signal alignment**           | Three-tier joiner: co-temporal (< 30s), stale-gap (decay), dead-gap (fundamental-diagram interpolation)                                                  |
| **Weight model**               | Separate module; takes `JoinedEdgeState` (speed + occupancy) → produces `IntArray` with occupancy scaling                                                |
| **Uncovered edges**            | Time-of-day Gaussian modulation + neighbor congestion propagation (inverted BFS)                                                                         |
| **Staleness**                  | Two-tier TTL (stale at 5min, dead at 30min) with linear confidence decay; per-lane evaluation                                                            |
| **Broker**                     | Deferred to DE team; `PacketSource` interface is the extension point                                                                                     |
| **Customization frequency**    | 30s windows; sub-10s documented as future work with three candidate approaches                                                                           |
| **Camera → edge (production)** | Database-backed via `camera_edge_map` table; GIS snapping is a separate project                                                                          |
| **Modularity**                 | Each stage is an interface (`PacketSource`, `Aggregator<S>`, `Smoother<S>`, `EdgeJoiner`, `WeightModel`, `WeightOutput`); swap any without touching others |


