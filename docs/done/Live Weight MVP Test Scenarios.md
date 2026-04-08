# Live Weight MVP Test Scenarios — Plan

**Status:** Draft
**Date:** 2026-03-31
**Relation:** Companion to [Live Weight MVP.md](Live%20Weight%20MVP.md) and
[Live Weight Pipeline.md](Live%20Weight%20Pipeline.md)

This document defines a **small, interpretable, and visibly reactive** scenario
suite for MVP validation. The objective is not to simulate all of Hanoi. The
objective is to create two bounded urban sectors where:

1. camera-triggered weight changes are large enough to visibly affect routes,
2. query distances are long enough for reroutes to matter,
3. the setup stays small enough to debug manually.

All sector and placement recommendations below are **topology-driven MVP
heuristics**, not claims about live traffic counts. Final `arc_id` selection
still happens against `road_manifest.arrow` and a visual map pass.

---

## 0. Goals

Prove four things with a manageable test harness:

1. **Reroute visibility:** When one main-road corridor is degraded, at least
   some routes crossing the sector choose a different corridor.
2. **Reversibility:** If the degraded corridor is switched, the preferred
   route shifts back toward the newly healthier alternative.
3. **Directionality:** Because each MVP camera maps to one `arc_id`, scenarios
   should be built around a dominant travel direction rather than requiring
   both directions at once.
4. **Interpretability:** Engineers should be able to explain route changes by
   looking at a small set of major roads, not a city-wide web of side streets.

---

## 1. Scenario Design Constraints

| Item | Target | Why |
| ---- | ------ | --- |
| Number of sectors | 2 | Enough diversity without making validation diffuse |
| Scenario sets per sector | 2 | Lets us test “corridor A bad” and “corridor B bad” reversibility |
| Sector width | 1.8-2.5 km | Small enough to reason about, large enough to contain alternatives |
| Query straight-line distance | 4.0-6.5 km | Long enough for reroute effect to be visible |
| OD placement | 0.8-1.5 km outside sector | Prevents trivial intra-pocket routes |
| Logical camera corridors per sector | 4 | Enough to create route-choice tension |
| Actual camera entries per sector | 6-8 `arc_id`s | One logical corridor often needs 1-2 adjacent same-direction edges |
| Covered roads | Main roads only | Easier to interpret and more likely to influence path choice |
| Queries per scenario set | 3 primary + 1 control | Enough signal without overwhelming manual review |

### 1.1 Logical Camera Corridor vs Actual MVP Camera Entry

The MVP config uses **one `arc_id` per camera entry**. In practice:

- One **logical camera corridor** = “we want to model congestion on this named
  main road in this direction”
- One **actual camera entry** = one specific `arc_id` on that road

For strong visible reroutes, do not stop at a single isolated edge if the
baseline route crosses a longer stretch of the corridor. Prefer:

1. one main-road location = 1 logical corridor,
2. that logical corridor = 1-2 adjacent same-direction `arc_id` entries,
3. longer corridors get 2 entries before adding more unique roads.

This keeps the camera set small while making the degraded segment long enough
to matter.

### 1.2 What We Explicitly Avoid in MVP

- **Old Quarter / Hoan Kiem core:** too fine-grained and one-way-heavy for
  clean interpretation.
- **Full Ring Road 3 corridor:** too large and system-level for first-pass
  debugging.
- **Pure residential sectors:** easy to generate noise, harder to see a clear
  reroute story.
- **City-wide OD pairs:** route changes become harder to attribute to the
  sector under test.

---

## 2. Recommended Sector Portfolio

| Sector | Primary role | Why it made the cut |
| ------ | ------------ | ------------------- |
| **Sector A: Cầu Giấy - Xuân Thủy - Nguyễn Khang - Phạm Hùng** | First and best MVP sector | Multiple major-road alternatives in a compact western Hanoi pocket |
| **Sector B: Ngã Tư Sở - Tây Sơn - Trường Chinh - Nguyễn Trãi - Láng** | Second sector for denser urban stress | Strong arterial choice structure, larger visible reroute potential |

These two sectors complement each other:

- **Sector A** is cleaner and easier to debug.
- **Sector B** is busier and better for proving that the behavior generalizes
  beyond one corridor.

---

## 3. Sector A — Cầu Giấy / Dịch Vọng Corridor

### 3.1 Sector Definition

Use a compact west-Hanoi sector centered on the interaction between:

- `Xuân Thủy`
- `Cầu Giấy`
- `Nguyễn Khang`
- `Phạm Hùng`

Optional supporting roads for visual inspection:

- `Duy Tân`
- `Trần Thái Tông`
- nearby approaches into `Quan Hoa`, `Yên Hòa`, and `Mai Dịch`

### 3.2 Why This Sector Works

- It has clear **main-road alternatives** rather than requiring residential cut-throughs.
- The roads are important enough that a localized slowdown should matter.
- The sector is compact enough to inspect by eye in OpenStreetMap or QGIS.
- It aligns well with the camera examples already used in the MVP doc such as
  `Cầu Giấy bridge` and `Đường Láng southbound` style corridor thinking in
  [Live Weight MVP.md](Live%20Weight%20MVP.md).

### 3.3 Logical Camera Corridors

| Corridor ID | Road | Preferred direction for MVP | Role in route choice | Implementation note |
| ----------- | ---- | --------------------------- | -------------------- | ------------------- |
| `A_XT` | `Xuân Thủy` | toward inner Hanoi / eastbound | Central inbound corridor | Use 2 adjacent same-direction edges if possible |
| `A_CG` | `Cầu Giấy` | toward inner Hanoi / eastbound | Continuation of central corridor | Prefer one edge before a route-choice node |
| `A_NK` | `Nguyễn Khang` | southeastbound / eastbound | Parallel relief corridor | Strong alternative when central corridor degrades |
| `A_PH` | `Phạm Hùng` | southbound / southeastbound | Outer bypass corridor | Useful for wider detours and reversal tests |

### 3.4 Camera Count Recommendation

Use **4 logical corridors** but implement **6 actual camera entries**:

- `A_XT_1`, `A_XT_2`
- `A_CG_1`
- `A_NK_1`, `A_NK_2`
- `A_PH_1`

If the reroute signal is weak, expand to **8 actual entries** by adding:

- `A_CG_2`
- `A_PH_2`

Do not add minor streets before doubling the main-road coverage on the same
major corridor.

### 3.5 Query Envelope

Pick OD points from these anchor areas:

| Anchor | Area role |
| ------ | --------- |
| `A_W` | `Mai Dịch / Mỹ Đình` side |
| `A_N` | `Nghĩa Tân / Nghĩa Đô` side |
| `A_E` | `Quan Hoa / Láng` side |
| `A_S` | `Yên Hòa / Trung Hòa` side |

Guideline:

- all OD pairs should either **cross the sector** or **force a real choice**
  between the central corridor and the outer/parallel corridor,
- none should start and end entirely within the camera pocket.

### 3.6 Sector A Query Families

| Query ID | Pattern | Intent |
| -------- | ------- | ------ |
| `A_Q1` | `A_W -> A_E` | Clean west-to-east crossing; should be highly sensitive to central corridor slowdown |
| `A_Q2` | `A_N -> A_S` | North-to-south diagonal; tests whether congestion influences connector selection |
| `A_Q3` | `A_W -> A_S` | West-to-southeast movement; good for central-vs-outer corridor competition |
| `A_CTRL` | `A_N -> A_E` skimming the sector edge | Control query; should move less than the three primaries |

### 3.7 Test Set A1 — Central Corridor Suppression

**Purpose:** Prove that degrading `Xuân Thủy + Cầu Giấy` pushes routes onto
`Nguyễn Khang` and/or `Phạm Hùng`.

**Active logical corridors:**

- `A_XT`
- `A_CG`

**Inactive but available alternatives:**

- `A_NK`
- `A_PH`

**Recommended profile shape:**

- `free_flow_kmh`: `40-45`
- peak at `hour=7.5`
- `speed_kmh`: `18-24`
- `occupancy`: `0.70-0.85`

**Run hours:**

- baseline comparison at `12.0`
- stressed comparison at `7.5`

**Expected route behavior:**

- `A_Q1` should be the clearest reroute candidate.
- `A_Q2` should partially or fully shift away from the central corridor.
- `A_Q3` may either detour outward via `Phạm Hùng` or use a mixed path.
- `A_CTRL` should move less than the primaries; if it changes heavily, the
  sector may be too large or the query too close to the hot corridor.

**What success looks like:**

- at least one primary query abandons most of the degraded `Xuân Thủy/Cầu Giấy`
  segment,
- another primary query shows either a different path geometry or a clearly
  longer travel time if no practical detour exists.

**What failure usually means:**

- the degraded segment is too short,
- the OD pair is too constrained,
- the baseline route never meaningfully used the corridor,
- or the alternative corridor is not actually competitive in the graph.

### 3.8 Test Set A2 — Parallel / Outer Corridor Suppression

**Purpose:** Prove reversibility by degrading `Nguyễn Khang + Phạm Hùng` and
letting the central `Xuân Thủy/Cầu Giấy` corridor become attractive again.

**Active logical corridors:**

- `A_NK`
- `A_PH`

**Preferred queries:**

- reuse `A_Q1`, `A_Q2`, `A_Q3`, `A_CTRL` exactly

**Recommended profile shape:**

- `free_flow_kmh`: `40-45`
- peak at `hour=7.5`
- `speed_kmh`: `18-24`
- `occupancy`: `0.65-0.80`

**Expected route behavior:**

- routes that were pushed outward in A1 should recover toward the central path,
- `A_Q1` and `A_Q3` are the best indicators of reversal,
- if nothing changes relative to A1, the outer corridor was probably never
  competitive enough to begin with.

**Special interpretation note:**

If A1 gives a visible reroute but A2 does not give a visible reverse-reroute,
that is not automatically a bug. It may simply mean the central corridor is
structurally dominant. In that case, adjust `A_Q3` or lengthen `A_NK` coverage
before expanding the sector.

---

## 4. Sector B — Ngã Tư Sở / Đống Đa - Thanh Xuân Corridor

### 4.1 Sector Definition

Use a denser urban sector around the major-road interaction between:

- `Tây Sơn`
- `Láng`
- `Trường Chinh`
- `Nguyễn Trãi`

This sector is centered on the broader `Ngã Tư Sở` decision area.

### 4.2 Why This Sector Works

- It has a strong “arterial vs arterial” structure.
- Reroutes should be more visible than in a purely local street grid.
- It is wide enough to expose path switching without becoming a city-wide test.
- It complements Sector A by being more urban and slightly harder, while still
  remaining understandable.

### 4.3 Logical Camera Corridors

| Corridor ID | Road | Preferred direction for MVP | Role in route choice | Implementation note |
| ----------- | ---- | --------------------------- | -------------------- | ------------------- |
| `B_TS` | `Tây Sơn` | eastbound / toward inner core | Northern central arterial | Use 2 adjacent same-direction edges if route overlap is short |
| `B_LA` | `Láng` | eastbound | Parallel arterial option | Good pair with `Tây Sơn` for same-side corridor stress |
| `B_TC` | `Trường Chinh` | westbound or southwestbound | Southern bypass arterial | Strong alternative under north-side stress |
| `B_NT` | `Nguyễn Trãi` | southwestbound / outbound | Major southwest arm | Important for broader detour structure |

### 4.4 Camera Count Recommendation

Use **4 logical corridors** but implement **6 actual camera entries**:

- `B_TS_1`, `B_TS_2`
- `B_LA_1`
- `B_TC_1`, `B_TC_2`
- `B_NT_1`

If the visible route shift is too weak, expand to **8 entries** by adding:

- `B_LA_2`
- `B_NT_2`

### 4.5 Query Envelope

Pick OD points from these anchor areas:

| Anchor | Area role |
| ------ | --------- |
| `B_NW` | `Láng Hạ / Thành Công / Hoàng Cầu` side |
| `B_NE` | `Ô Chợ Dừa / inner Đống Đa` side |
| `B_SW` | `Thượng Đình / Thanh Xuân` side |
| `B_S` | `Khương Trung / Trường Chinh south` side |

### 4.6 Sector B Query Families

| Query ID | Pattern | Intent |
| -------- | ------- | ------ |
| `B_Q1` | `B_NW -> B_SW` | Strong cross-sector route through the Ngã Tư Sở area |
| `B_Q2` | `B_NE -> B_SW` | Inner-to-southwest movement; likely sensitive to northern arterial degradation |
| `B_Q3` | `B_NW -> B_S` | Good for testing north-side vs south-side corridor preference |
| `B_CTRL` | `B_NE -> B_S` skimming one side of the sector | Control query; should not overreact |

### 4.7 Test Set B1 — North-Side Arterial Suppression

**Purpose:** Degrade `Tây Sơn + Láng` and see whether routes drop toward
`Trường Chinh` and `Nguyễn Trãi`.

**Active logical corridors:**

- `B_TS`
- `B_LA`

**Inactive but available alternatives:**

- `B_TC`
- `B_NT`

**Recommended profile shape:**

- `free_flow_kmh`: `35-40`
- peak at `hour=17.5`
- `speed_kmh`: `15-22`
- `occupancy`: `0.70-0.85`

**Run hours:**

- baseline comparison at `12.0`
- stressed comparison at `17.5`

**Expected route behavior:**

- `B_Q1` should provide the clearest “top side bad, bottom side chosen” signal,
- `B_Q2` should also react if the northern approach was initially attractive,
- `B_Q3` is useful for mixed behavior and partial segment avoidance,
- `B_CTRL` should move only modestly.

### 4.8 Test Set B2 — South-West Arm Suppression

**Purpose:** Degrade `Trường Chinh + Nguyễn Trãi` and let `Tây Sơn/Láng`
recover as the attractive pair.

**Active logical corridors:**

- `B_TC`
- `B_NT`

**Recommended profile shape:**

- `free_flow_kmh`: `35-40`
- peak at `hour=17.5`
- `speed_kmh`: `15-22`
- `occupancy`: `0.65-0.80`

**Expected route behavior:**

- `B_Q1` should switch back toward the northern arterials if they are viable,
- `B_Q3` is the best secondary signal,
- if the same route persists across B1 and B2, review whether the ODs are too
  constrained or whether one corridor is overwhelmingly dominant in the graph.

---

## 5. Camera Placement Strategy

### 5.1 Placement Rules

1. Put cameras on **main roads only** for the MVP.
2. Place cameras **before** major route-choice points, not after them.
3. Cover the **same travel direction** within a scenario set.
4. Prefer **2 adjacent edges on the same corridor** over one edge on a minor
   supporting road.
5. If a corridor bends, place the two entries on the stretch most likely to be
   shared by the baseline routes.
6. Do not try to model both carriageways unless the queries truly require both.

### 5.2 Corridor Prioritization Order

When mapping actual `arc_id`s, use this order:

1. corridor segment that appears most often in baseline routes,
2. corridor segment immediately upstream of a major decision node,
3. second adjacent edge on that same corridor,
4. only then add a second corridor or another road.

### 5.3 Profile Assignment Strategy

Keep the profile catalog tiny:

| Profile | Use case | Suggested behavior |
| ------- | -------- | ------------------ |
| `main_road_heavy_am` | Sector A stressed runs | Strong morning slowdown with high occupancy |
| `main_road_heavy_pm` | Sector B stressed runs | Strong evening slowdown with high occupancy |
| `main_road_moderate` | Optional softer tests | Noticeable slowdown without complete collapse |

Do not create one profile per camera unless the scenario truly needs that.
Reusing profiles keeps the MVP interpretable.

---

## 6. Query Design Rules

### 6.1 Query Shape Rules

Each OD pair should satisfy all of the following:

1. the baseline route uses at least one planned camera corridor,
2. a plausible alternative main-road route exists,
3. the route is long enough that a detour is still rational,
4. the OD is not so large that unrelated city-scale corridors dominate.

### 6.2 Query Distance Target

Recommended straight-line distance:

- minimum: `4.0 km`
- preferred: `4.5-6.0 km`
- maximum: `6.5 km`

Below `4 km`, the route often lacks enough room to visibly reroute.
Above `6.5 km`, the test becomes harder to explain because too many unrelated
roads can absorb the change.

### 6.3 Control Query Rule

Each scenario set includes one control query that:

- skirts the sector edge,
- touches at most one stressed corridor,
- should remain more stable than the three primary queries.

This guards against overfitting and helps reveal whether the scenario has
become too global.

---

## 7. Scenario Packaging

Use one YAML file per scenario set:

- `sector_a_set1_central_corridor.yaml`
- `sector_a_set2_outer_corridor.yaml`
- `sector_b_set1_north_arterials.yaml`
- `sector_b_set2_southwest_arms.yaml`

### 7.1 Suggested YAML Skeleton

```yaml
profiles:
  main_road_heavy_am:
    free_flow_kmh: 42
    free_flow_occupancy: 0.20
    peaks:
      - hour: 7.5
        speed_kmh: 20
        occupancy: 0.80

  main_road_heavy_pm:
    free_flow_kmh: 38
    free_flow_occupancy: 0.22
    peaks:
      - hour: 17.5
        speed_kmh: 18
        occupancy: 0.78

cameras:
  - id: 0
    label: "A_XT_1"
    arc_id: <fill_from_manifest>
    profile: main_road_heavy_am

  - id: 1
    label: "A_XT_2"
    arc_id: <fill_from_manifest>
    profile: main_road_heavy_am
```

The exact `arc_id`s are intentionally deferred until the visual route review is
complete.

---

## 8. Arc Mapping Workflow

Use the MVP mapping workflow from [Live Weight MVP.md](Live%20Weight%20MVP.md):

1. load `road_manifest.arrow`,
2. search for the target road by name,
3. inspect its `arc_ids`,
4. match the right direction and physical location,
5. prefer edges that appear on baseline route traces,
6. record the chosen `arc_id`s in the scenario YAML.

### 8.1 Practical Selection Checklist

For each candidate `arc_id`, verify:

- the road name matches the intended corridor,
- the direction matches the dominant flow for the scenario,
- the edge lies before a meaningful choice point,
- the baseline route actually uses it or passes very near it,
- it is not a tiny side connector that happens to share the road name.

### 8.2 When One Road Name Has Too Many `arc_id`s

This is expected. Use the shortest workable filter:

1. same road name,
2. same travel direction,
3. same local neighborhood / map location,
4. highest overlap with baseline routes.

---

## 9. Test Execution Runbook

### 9.1 Phase 1 — Sector Readiness Check

Before running any stressed scenario:

1. choose the four logical corridors,
2. map 6-8 actual `arc_id`s,
3. run all four queries at `hour=12.0` with no cameras,
4. confirm at least two primary queries use one of the intended corridors,
5. revise the OD points if the baseline completely avoids the sector.

Do not freeze the scenario set until this readiness check passes.

### 9.2 Phase 2 — Baseline Capture

For each sector:

1. run `A_Q1`, `A_Q2`, `A_Q3`, `A_CTRL` or `B_Q1`, `B_Q2`, `B_Q3`, `B_CTRL`,
2. store route geometry and reported travel time,
3. note which logical corridors each route uses.

### 9.3 Phase 3 — Stressed Scenario

For each scenario set:

1. start from the same OD pairs,
2. apply the scenario YAML,
3. run at the stressed hour,
4. compare route geometry and travel time against baseline,
5. mark whether the route:
   - stayed the same,
   - partially shifted,
   - fully changed corridor.

### 9.4 Phase 4 — Reversal Scenario

Run the companion set for the same sector:

1. switch which corridors are degraded,
2. keep the OD pairs unchanged,
3. compare against both baseline and the first stressed run,
4. verify whether the route preference moves back.

### 9.5 Phase 5 — Off-Peak Recovery Check

At least once per sector:

1. keep the scenario YAML active,
2. run at `hour=12.0`,
3. confirm that routes largely return toward baseline because the profile peak
   is no longer active.

This is the cleanest proof that the profile interpolation is time-varying and
not acting as a static override.

---

## 10. Success Criteria

### 10.1 Minimum Success Bar

The scenario suite is good enough for MVP sign-off if:

1. both sectors produce at least one clearly visible reroute,
2. each sector has at least one scenario pair showing meaningful reversal,
3. control queries stay more stable than the primary queries,
4. off-peak recovery moves routes back toward baseline behavior.

### 10.2 Strong Success Bar

The suite is especially healthy if:

1. two of the three primary queries per set show path change,
2. route changes remain on main roads rather than bizarre local cut-throughs,
3. the explanation of the reroute is obvious from the camera corridor layout,
4. results are reproducible across repeated runs.

---

## 11. Common Failure Modes and Fixes

| Symptom | Likely cause | First fix |
| ------- | ------------ | --------- |
| No route change at all | Camera coverage too short | Add a second adjacent edge on the same corridor |
| Route ignores degraded road | Baseline never used that road | Redesign the OD pair |
| Query overreacts city-wide | OD too long | Shorten the pair back toward 4-6 km |
| Query changes through side streets | Sector too dense or alternatives too weak | Move cameras to stronger main-road segments |
| Control query changes as much as primaries | Sector influence too global | Narrow the sector or relocate control OD |
| Reversal set shows same path as first set | One corridor structurally dominates | Strengthen alternative coverage or adjust the OD geometry |

---

## 12. Recommended Order of Work

1. Implement **Sector A / Test Set A1**
2. Add **Sector A / Test Set A2**
3. Once reversal works, implement **Sector B / Test Set B1**
4. Finish with **Sector B / Test Set B2**

This order maximizes learning while keeping the first debugging loop small.

---

## 13. Deliverables for the MVP Scenario Package

The final scenario package should include:

1. this planning doc,
2. four scenario YAML files,
3. one short query matrix per sector,
4. saved baseline and stressed route outputs,
5. a one-page summary of observed reroute behavior.

That is enough to demonstrate the MVP without prematurely building a full
simulation framework.
