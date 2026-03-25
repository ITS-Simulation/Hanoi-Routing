# Smoother Module — Implementation Guide

**Module:** `CCH_Data_Pipeline/smoother/`
**Status:** Empty shell (build.gradle.kts only, no source files)
**Goal:** Implement generic Huber DES smoother usable for both speed and
occupancy lanes

---

## 1. What This Module Does

The smoother sits between the aggregator and the weight model:

```
List<SpeedSummary>     →  Smoother (speed lane)     →  Map<Int, SmootherState>
List<OccupancySummary> →  Smoother (occupancy lane)  →  Map<Int, SmootherState>
```

It receives batches of per-edge window summaries (one batch per 30-second
window tick), applies Huber-weighted Double Exponential Smoothing to each edge's
time series, and exposes an immutable snapshot of all edge states for the
downstream joiner/weight model.

**This module has zero external dependencies.** No coroutines, no HTTP, no
graph data. Pure math and state management.

---

## 2. Files to Create

All files go under `smoother/src/main/kotlin/com/thomas/smoother/`:

```
smoother/src/main/kotlin/com/thomas/smoother/
├── SmootherConfig.kt       # Step 1: Parameters (alpha, beta, delta)
├── SmootherState.kt        # Step 2: Per-edge state data class
├── SummaryExtractor.kt     # Step 3: Functional interface for generics
├── Smoother.kt             # Step 4: Interface definition
└── HuberDesSmoother.kt     # Step 5: Core algorithm

smoother/src/test/kotlin/com/thomas/smoother/
├── HuberDesSmootherTest.kt # Step 6: Unit tests
└── SmootherConfigTest.kt   # Step 6: Parameter validation tests
```

---

## 3. Step-by-Step Implementation

### Step 1: `SmootherConfig.kt`

Holds the three Huber DES hyperparameters. Different instances for speed vs.
occupancy.

```kotlin
package com.thomas.smoother

/**
 * Configuration for one Huber DES smoother instance.
 *
 * @param alpha Level smoothing factor ∈ (0, 1). Higher = more reactive.
 * @param beta  Trend smoothing factor ∈ (0, 1). Higher = faster trend response.
 * @param delta Huber threshold in the same unit as the observed value.
 *              Residuals beyond ±delta are downweighted.
 */
data class SmootherConfig(
    val alpha: Double,
    val beta: Double,
    val delta: Double
) {
    init {
        require(alpha in 0.0..1.0) { "alpha must be in (0, 1), got $alpha" }
        require(beta in 0.0..1.0) { "beta must be in (0, 1), got $beta" }
        require(delta > 0.0) { "delta must be positive, got $delta" }
    }

    companion object {
        /** Recommended defaults for the speed lane (km/h). */
        val SPEED = SmootherConfig(alpha = 0.3, beta = 0.1, delta = 15.0)

        /** Recommended defaults for the occupancy lane (0–1 fraction). */
        val OCCUPANCY = SmootherConfig(alpha = 0.2, beta = 0.05, delta = 0.15)
    }
}
```

**Why these values:**
- Speed α=0.3: With 30-second windows, a step change reaches 95% of the new
  level in ~9 windows (~4.5 minutes). Responsive enough for rush-hour onset,
  stable enough to not chase noise.
- Occupancy α=0.2: Occupancy changes more slowly in reality (it's a spatial
  average, not a point measurement). Lower α gives more inertia.
- Speed δ=15 km/h: A camera reading 15+ km/h off from the predicted speed is
  suspicious. This threshold is about one standard deviation of typical camera
  noise on urban roads.
- Occupancy δ=0.15: A 15% occupancy jump in one window is unusual. This
  corresponds to roughly a 6-car difference on a 40-car road segment.

### Step 2: `SmootherState.kt`

The per-edge state. This is the smoother's output type — what the joiner and
weight model consume.

```kotlin
package com.thomas.smoother

import kotlinx.serialization.Serializable

/**
 * Smoothed state for a single edge on a single signal lane.
 *
 * Interpretation of [level] depends on which lane this state belongs to:
 * - Speed lane: current smoothed speed in km/h
 * - Occupancy lane: current smoothed occupancy ∈ [0.0, 1.0]
 *
 * @param level            Current smoothed value
 * @param trend            Change in value per window (positive = increasing)
 * @param lastUpdateMs     Simulation timestamp of the last observation
 * @param observationCount Cumulative number of observations processed
 */
@Serializable
data class SmootherState(
    val level: Double,
    val trend: Double,
    val lastUpdateMs: Long,
    val observationCount: Int
)
```

**Design note:** `SmootherState` is deliberately lane-agnostic. It doesn't know
whether it holds speed or occupancy — that context belongs to the caller. This
is what makes the `Smoother<S>` generic work: same state type, different input
summary types.

### Step 3: `SummaryExtractor.kt`

The generic smoother needs to pull `edgeId` and `observedValue` out of any
summary type. A functional interface solves this without requiring the summary
types to implement a common interface (which they can't, since they live in the
`simulation` module).

```kotlin
package com.thomas.smoother

/**
 * Extracts the fields the smoother needs from any window summary type.
 *
 * This decouples the smoother from the concrete summary data classes
 * (SpeedSummary, OccupancySummary) which live in the simulation module.
 */
fun interface SummaryExtractor<S> {
    fun extract(summary: S): Observation
}

/**
 * The two values the smoother needs from each summary.
 */
data class Observation(
    val edgeId: Int,
    val value: Double,
    val windowEndMs: Long
)
```

**Usage at wiring time** (in `app/Main.kt`, not in the smoother module):

```kotlin
// These lambdas are defined where SpeedSummary/OccupancySummary are visible
val speedExtractor = SummaryExtractor<SpeedSummary> { summary ->
    Observation(summary.edgeId, summary.meanSpeedKmh.toDouble(), summary.windowEndMs)
}
val occupancyExtractor = SummaryExtractor<OccupancySummary> { summary ->
    Observation(summary.edgeId, summary.meanOccupancy.toDouble(), summary.windowEndMs)
}
```

### Step 4: `Smoother.kt`

The interface that the rest of the pipeline depends on.

```kotlin
package com.thomas.smoother

/**
 * Stateful per-edge smoother. Receives batches of window summaries,
 * maintains internal state, and exposes immutable snapshots.
 *
 * @param S The summary type (e.g., SpeedSummary or OccupancySummary).
 *          The smoother extracts edgeId and observed value via [SummaryExtractor].
 */
interface Smoother<S> {
    /**
     * Process one window's worth of summaries. Updates internal state
     * for each edge that appears in the batch. Edges not present in
     * the batch retain their previous state (no decay here — that's
     * the weight model's job via staleness TTL).
     */
    fun update(summaries: List<S>)

    /**
     * Return an immutable snapshot of all edge states.
     * Safe to read while [update] is not running.
     * The returned map is a defensive copy — mutations to it do not
     * affect the smoother's internal state.
     */
    fun snapshot(): Map<Int, SmootherState>
}
```

### Step 5: `HuberDesSmoother.kt`

The core algorithm. This is the most important file.

```kotlin
package com.thomas.smoother

/**
 * Huber-weighted Double Exponential Smoothing (Holt's method with robust loss).
 *
 * Standard DES maintains two components per edge:
 *   - level: the current smoothed value
 *   - trend: the rate of change per window
 *
 * The Huber modification downweights observations that are far from the
 * prediction (|residual| > delta), making the smoother robust to outliers
 * like misread plates, sensor glitches, or a single motorbike blasting
 * through at 80 km/h on a 30 km/h street.
 *
 * Algorithm per observation:
 *   predicted = level + trend
 *   residual  = observed - predicted
 *   w = if |residual| <= delta then 1.0 else delta / |residual|
 *   effective_obs = w * observed + (1 - w) * predicted
 *   new_level = alpha * effective_obs + (1 - alpha) * predicted
 *   new_trend = beta * (new_level - old_level) + (1 - beta) * old_trend
 *
 * @param config    Hyperparameters (alpha, beta, delta)
 * @param extractor Pulls edgeId and observed value from summary type S
 */
class HuberDesSmoother<S>(
    private val config: SmootherConfig,
    private val extractor: SummaryExtractor<S>
) : Smoother<S> {

    // Mutable internal state. Key = edgeId.
    private val states = mutableMapOf<Int, MutableState>()

    override fun update(summaries: List<S>) {
        for (summary in summaries) {
            val obs = extractor.extract(summary)
            val state = states.getOrPut(obs.edgeId) {
                // First observation for this edge: initialize level to observed,
                // trend to zero (no prior information about direction).
                MutableState(
                    level = obs.value,
                    trend = 0.0,
                    lastUpdateMs = obs.windowEndMs,
                    observationCount = 0
                )
            }
            state.apply(obs.value, obs.windowEndMs, config)
        }
    }

    override fun snapshot(): Map<Int, SmootherState> {
        return states.entries.associate { (edgeId, state) ->
            edgeId to SmootherState(
                level = state.level,
                trend = state.trend,
                lastUpdateMs = state.lastUpdateMs,
                observationCount = state.observationCount
            )
        }
    }

    /**
     * Internal mutable state for one edge. Not exposed outside this class.
     */
    private class MutableState(
        var level: Double,
        var trend: Double,
        var lastUpdateMs: Long,
        var observationCount: Int
    ) {
        fun apply(observed: Double, windowEndMs: Long, config: SmootherConfig) {
            val predicted = level + trend
            val residual = observed - predicted

            // Huber weighting: full weight inside delta, reduced outside
            val w = if (kotlin.math.abs(residual) <= config.delta) {
                1.0
            } else {
                config.delta / kotlin.math.abs(residual)
            }

            // Effective observation: blend between raw and predicted
            val effectiveObs = w * observed + (1.0 - w) * predicted

            // DES update
            val prevLevel = level
            level = config.alpha * effectiveObs + (1.0 - config.alpha) * predicted
            trend = config.beta * (level - prevLevel) + (1.0 - config.beta) * trend

            lastUpdateMs = windowEndMs
            observationCount++
        }
    }
}
```

**Key design decisions explained:**

1. **First observation initializes level, not updates it.** When an edge is
   seen for the first time, we set `level = observed` and `trend = 0`. We
   do NOT run the DES formula because there's no meaningful `predicted` value
   yet (it would be 0.0, causing a massive residual). The `observationCount`
   starts at 0 and increments to 1 after `apply()` — this is intentional so
   the first real DES update happens on the second observation.

   **Wait — re-read the code.** `getOrPut` creates the state with the first
   observation's value, then `apply()` is called immediately on that same state.
   The predicted value is `level + trend = observed + 0 = observed`, the
   residual is `observed - observed = 0`, so w=1 and the level stays at
   `observed`. This is correct: the first observation is a no-op update that
   simply sets the count to 1. The first *real* smoothing happens on the second
   observation.

2. **`snapshot()` returns a defensive copy.** The `associate` call creates a new
   `Map<Int, SmootherState>` with immutable `SmootherState` data classes. The
   caller can hold onto this snapshot while the smoother continues processing
   new windows — no aliasing.

3. **No thread safety.** `update()` and `snapshot()` are not synchronized. This
   is by design: the pipeline orchestrator in `app/Main.kt` calls them
   sequentially (update → snapshot → pass to modeler → next window). If you
   later need concurrent access, wrap calls at the orchestration layer, not
   inside the smoother.

---

## 4. build.gradle.kts Update

The smoother needs `kotlinx-serialization` for `@Serializable` on
`SmootherState`. The current `build.gradle.kts` already has the serialization
plugin. Add the runtime library:

```kotlin
dependencies {
    testImplementation(kotlin("test"))
    implementation(libs.kotlinx.serialization.json)
}
```

No other dependencies needed. No coroutines, no HTTP, no Arrow.

---

## 5. Unit Tests

Create `smoother/src/test/kotlin/com/thomas/smoother/HuberDesSmootherTest.kt`:

### Test 1: Convergence

Feed a constant value. Level should converge to that value, trend should
converge to zero.

```kotlin
@Test
fun `constant input converges to input value`() {
    val smoother = HuberDesSmoother(SmootherConfig.SPEED, testExtractor)
    val constant = 50.0 // 50 km/h

    repeat(20) { window ->
        smoother.update(listOf(summary(edgeId = 0, value = constant, windowEnd = window * 30_000L)))
    }

    val state = smoother.snapshot()[0]!!
    assertEquals(constant, state.level, 0.5) // within 0.5 km/h
    assertEquals(0.0, state.trend, 0.1)      // trend ≈ 0
}
```

### Test 2: Outlier Rejection

Feed steady values with one spike. Level should barely move.

```kotlin
@Test
fun `outlier spike is suppressed by Huber weighting`() {
    val smoother = HuberDesSmoother(SmootherConfig.SPEED, testExtractor)
    val normal = 40.0

    // 10 windows of normal data to establish baseline
    repeat(10) { window ->
        smoother.update(listOf(summary(0, normal, window * 30_000L)))
    }
    val beforeSpike = smoother.snapshot()[0]!!.level

    // One massive spike: 120 km/h (80 km/h residual, well beyond delta=15)
    smoother.update(listOf(summary(0, 120.0, 10 * 30_000L)))
    val afterSpike = smoother.snapshot()[0]!!.level

    // Level should move by much less than the full residual
    val drift = kotlin.math.abs(afterSpike - beforeSpike)
    assertTrue(drift < 5.0, "Expected drift < 5 km/h, got $drift")
}
```

### Test 3: Trend Tracking

Feed a linearly increasing signal. Trend should become positive and
approximately match the slope.

```kotlin
@Test
fun `linear ramp produces positive trend`() {
    val smoother = HuberDesSmoother(SmootherConfig.SPEED, testExtractor)

    // Speed drops from 60 to 30 over 10 windows (3 km/h per window)
    repeat(10) { window ->
        val speed = 60.0 - 3.0 * window
        smoother.update(listOf(summary(0, speed, window * 30_000L)))
    }

    val state = smoother.snapshot()[0]!!
    assertTrue(state.trend < 0.0, "Trend should be negative for decreasing speed")
}
```

### Test 4: Multiple Edges Independent

Verify that two edges don't interfere with each other.

```kotlin
@Test
fun `edges are smoothed independently`() {
    val smoother = HuberDesSmoother(SmootherConfig.SPEED, testExtractor)

    repeat(10) { window ->
        smoother.update(listOf(
            summary(edgeId = 0, value = 50.0, windowEnd = window * 30_000L),
            summary(edgeId = 1, value = 20.0, windowEnd = window * 30_000L)
        ))
    }

    val snap = smoother.snapshot()
    assertEquals(50.0, snap[0]!!.level, 0.5)
    assertEquals(20.0, snap[1]!!.level, 0.5)
}
```

### Test 5: Missing Edge Retains State

An edge present in window 1 but absent in window 2 should keep its state.

```kotlin
@Test
fun `absent edge retains previous state`() {
    val smoother = HuberDesSmoother(SmootherConfig.SPEED, testExtractor)

    smoother.update(listOf(summary(0, 40.0, 0L)))
    val after1 = smoother.snapshot()[0]!!

    // Window 2: edge 0 absent, only edge 1 reported
    smoother.update(listOf(summary(1, 60.0, 30_000L)))

    val after2 = smoother.snapshot()[0]!!
    assertEquals(after1.level, after2.level, 0.001)
    assertEquals(after1.lastUpdateMs, after2.lastUpdateMs)
}
```

### Test 6: Occupancy Lane

Verify the same algorithm works with occupancy-scale parameters.

```kotlin
@Test
fun `occupancy config handles 0-1 scale values`() {
    val smoother = HuberDesSmoother(SmootherConfig.OCCUPANCY, testExtractor)

    repeat(15) { window ->
        smoother.update(listOf(summary(0, 0.6, window * 30_000L)))
    }

    val state = smoother.snapshot()[0]!!
    assertEquals(0.6, state.level, 0.05)
    assertEquals(0.0, state.trend, 0.02)
}
```

### Test Helper

```kotlin
private val testExtractor = SummaryExtractor<TestSummary> { s ->
    Observation(s.edgeId, s.value, s.windowEndMs)
}

private data class TestSummary(val edgeId: Int, val value: Double, val windowEndMs: Long)

private fun summary(edgeId: Int, value: Double, windowEnd: Long) =
    TestSummary(edgeId, value, windowEnd)
```

Using a `TestSummary` data class instead of `SpeedSummary`/`OccupancySummary`
proves that the smoother is truly generic — it doesn't depend on the simulation
module's types at all.

---

## 6. Verification Checklist

After implementation, verify these properties:

- [ ] `./gradlew :smoother:test` passes all 6 tests
- [ ] `./gradlew :smoother:build` compiles with no warnings
- [ ] `SmootherConfig.SPEED` and `SmootherConfig.OCCUPANCY` have the parameter
      values from the plan (§8.1)
- [ ] `SmootherState` is `@Serializable`
- [ ] `snapshot()` returns a defensive copy (modify returned map, call
      `snapshot()` again — should be unchanged)
- [ ] No imports from `simulation`, `modeler`, or `app` modules
- [ ] No coroutine dependencies (no `kotlinx.coroutines` in `build.gradle.kts`)
