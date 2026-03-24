# Live Weight Pipeline — Plan & Critique

**Status:** Draft (Rev 3)
**Date:** 2026-03-23
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
See Section 5 for the full camera-edge mapping design.

**Production recommendation:** See Section 5.3 for the database-backed
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

**Decision:** Use `Channel<SpeedPacket>` (Kotlin coroutines) for simulation.
Design the ingest layer as an interface so a Kafka/NATS consumer can be
swapped in later without touching pipeline logic. See Section 6.4.

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

See Section 6.5 for the full TTL design.

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
│  │  Camera Sim   │    SpeedPacket             │  Influence Map   │        │
│  │  (N coroutines │───→ Channel<SpeedPacket>  │  Precomputation  │        │
│  │  batched)      │                           │  (BFS from       │        │
│  └───────────────┘                            │   covered edges) │        │
│         │                                     └────────┬─────────┘        │
│         ▼                                              │                  │
│  ┌───────────────┐    Tumbling 30s windows             │                  │
│  │  Aggregator    │────→ Map<EdgeId, WindowSummary>    │                  │
│  └───────────────┘                                     │                  │
│         │                                              │                  │
│         ▼                                              │                  │
│  ┌───────────────┐    Per-edge: level, trend,          │                  │
│  │  Huber DES     │    lastUpdateMs                    │                  │
│  │  Smoother      │────→ Map<EdgeId, SmoothedState>    │                  │
│  └───────────────┘                                     │                  │
│         │                                              │                  │
│         ▼                                              │                  │
│  ┌─────────────────────────────────────────────────────┴────────────┐     │
│  │  Weight Model (separate module)                                   │    │
│  │                                                                   │    │
│  │  Input: smoothed states + baseline + influence map + sim clock    │    │
│  │                                                                   │    │
│  │  Covered edges:                                                   │    │
│  │    smoothed_speed → travel_time_ms (via geo_distance)             │    │
│  │    confidence blend with baseline                                 │    │
│  │    staleness TTL check                                            │    │
│  │                                                                   │    │
│  │  Uncovered edges:                                                 │    │
│  │    time-of-day Gaussian modulation (always on)                    │    │
│  │    + neighbor congestion propagation (if influence exists)        │    │
│  │                                                                   │    │
│  │  Output: IntArray(1_869_499) — one u32 weight per edge            │    │
│  └────────────────────────────┬──────────────────────────────────────┘    │
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

```
┌──────────┐    SpeedPacket     ┌──────────┐   List<WindowSummary>  ┌──────────┐
│  Ingest  │ ─────────────────→ │ Aggregator│ ────────────────────→ │ Smoother │
│          │  Channel<Packet>   │          │                        │          │
└──────────┘                    └──────────┘                        └────┬─────┘
                                                                        │
                                                          SmootherSnapshot│
                                                                        ▼
┌──────────┐    IntArray         ┌──────────┐                     ┌──────────┐
│  Output  │ ←────────────────── │  Weight  │ ←───────────────────│  (state) │
│          │   (weight vector)   │  Model   │  SmootherSnapshot   │          │
└──────────┘                     └──────────┘                     └──────────┘
```

Each interface is a Kotlin `interface` or `data class`:

```kotlin
// === Ingest → Aggregator ===
// Contract: SpeedPacket on a Channel. Ingest produces, Aggregator consumes.
// The Channel is the only coupling point.

data class SpeedPacket(
    val cameraId: Int,
    val timestampMs: Long,
    val speedKmh: Float,
    val confidence: Float       // 0.0–1.0
)

interface PacketSource {
    fun packets(): ReceiveChannel<SpeedPacket>
}

// === Aggregator → Smoother ===
// Contract: a batch of per-edge window summaries, emitted every window tick.

data class WindowSummary(
    val edgeId: Int,
    val meanSpeedKmh: Float,
    val observationCount: Int,
    val variance: Float,
    val windowEndMs: Long
)

interface Aggregator {
    fun summaries(): ReceiveChannel<List<WindowSummary>>
}

// === Smoother → Weight Model ===
// Contract: a snapshot of all smoother states. The weight model reads this
// immutably — the smoother owns the mutable state.

data class SmootherState(
    val level: Double,          // current smoothed speed
    val trend: Double,          // speed change per window
    val lastUpdateMs: Long,     // for staleness detection
    val observationCount: Int   // for confidence
)

interface Smoother {
    fun update(summaries: List<WindowSummary>)
    fun snapshot(): Map<Int, SmootherState>
}

// === Weight Model → Output ===
// Contract: a complete weight vector (IntArray of exactly numEdges elements).

interface WeightModel {
    fun computeWeights(states: Map<Int, SmootherState>, nowMs: Long): IntArray
}

interface WeightOutput {
    suspend fun deliver(weights: IntArray)
}
```

### 4.2 Why This Level of Separation Matters


| Scenario                               | What You Swap           | Everything Else Unchanged                 |
| -------------------------------------- | ----------------------- | ----------------------------------------- |
| Switch from simulation to real cameras | `PacketSource` impl     | Aggregator, Smoother, WeightModel, Output |
| Try a different smoothing algorithm    | `Smoother` impl         | Ingest, Aggregator, WeightModel, Output   |
| Change how uncovered edges are handled | `WeightModel` impl      | Ingest, Aggregator, Smoother, Output      |
| Send weights to a file instead of HTTP | `WeightOutput` impl     | Everything upstream                       |
| Replace tumbling windows with sliding  | `Aggregator` impl       | Ingest, Smoother, WeightModel, Output     |
| Add Kafka as the ingest source         | New `PacketSource` impl | All downstream stages                     |
| Replace Huber DES with EMA/Kalman      | New `Smoother` impl     | All other stages                          |


### 4.3 Testing Strategy per Module

Each module is testable in isolation with synthetic inputs:

- **Ingest:** Verify it produces `SpeedPacket` at the expected rate and
distribution. Test: create a `SimulatedPacketSource`, collect 1000 packets,
check statistical properties.
- **Aggregator:** Feed a known sequence of `SpeedPacket` into a channel, verify
the emitted `WindowSummary` values (mean, count, variance) match hand-
computed expectations.
- **Smoother:** Feed a known sequence of `WindowSummary` batches. Verify
convergence (steady input → level converges to input speed), outlier
rejection (spike → level barely moves), trend tracking (linear ramp →
trend matches slope).
- **Weight Model:** Create synthetic `SmootherState` maps. Verify: covered
edges get speed-to-weight conversion, uncovered edges get time-of-day
modulation, stale edges decay toward baseline, all weights are in
`[1, INFINITY-1]`.
- **Output:** Mock HTTP server. Verify the POST body is exactly
`numEdges * 4` bytes, little-endian u32.

---

## 5. Camera-Edge Mapping

### 4.1 Data Model

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

### 4.2 Simulation Backbone: Config-Driven Mapping

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

### 4.3 Production Recommendation: Database-Backed Mapping

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

## 6. Tech Stack & Module Design

### 5.1 Language: Kotlin

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

### 5.2 Project Structure

New Kotlin/Gradle project: `Live_Network_Routing/`

```
Live_Network_Routing/
├── build.gradle.kts
├── settings.gradle.kts
├── gradle/
│   └── libs.versions.toml          # Version catalog
└── src/
    └── main/kotlin/vts/hanoi/pipeline/
        ├── Main.kt                  # Application entry point, CLI args
        │
        ├── graph/                   # Graph data loading (RoutingKit binary format)
        │   ├── GraphData.kt         #   Loads first_out, head, travel_time, geo_distance, lat, lng
        │   └── BinaryIO.kt          #   Little-endian u32/f32 vector I/O
        │
        ├── mapping/                 # Camera-edge mapping
        │   ├── CameraMappingSource.kt   # Interface
        │   ├── JsonFileMappingSource.kt # Simulation: reads JSON config
        │   └── AutoGenerator.kt         # Auto-generate camera configs from graph
        │
        ├── ingest/                  # Data ingest layer
        │   ├── PacketSource.kt      #   Interface: simulation vs production
        │   ├── CameraSimulator.kt   #   Simulation: batched coroutine camera generators
        │   └── SpeedPacket.kt       #   Data class
        │
        ├── aggregator/              # Windowed aggregation
        │   ├── WindowAggregator.kt  #   Tumbling window, per-edge statistics
        │   └── WindowSummary.kt     #   Data class
        │
        ├── smoother/                # Huber DES
        │   ├── HuberDesSmoother.kt  #   Core algorithm
        │   ├── SmootherState.kt     #   Per-edge state: level, trend, lastUpdateMs
        │   └── SmootherConfig.kt    #   alpha, beta, delta parameters
        │
        ├── weight/                  # Weight modelling (separate module)
        │   ├── WeightModel.kt       #   Interface
        │   ├── LiveWeightModel.kt   #   Main implementation
        │   ├── TimeOfDay.kt         #   Gaussian time-of-day modulation
        │   ├── InfluenceMap.kt      #   Neighbor congestion propagation
        │   └── StalenessPolicy.kt   #   TTL handling
        │
        └── output/                  # Weight delivery
            └── CustomizeClient.kt   #   HTTP POST to hanoi-server /customize
```

### 5.3 Dependencies

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

### 5.4 Coroutine Architecture

```kotlin
fun main() = runBlocking {
    val graph = GraphData.load(graphDir)
    val mapping = mappingSource.loadMappings()
    val influenceMap = InfluenceMap.precompute(graph, mapping.coveredEdges())
    val smoother = HuberDesSmoother(config, graph.numEdges)
    val weightModel = LiveWeightModel(graph, influenceMap, stalenessPolicy)

    // In-process channel — replaces Kafka for simulation
    val packetChannel = Channel<SpeedPacket>(capacity = Channel.BUFFERED)

    // Camera simulators: batched coroutines producing packets
    val simJob = launch {
        CameraSimulator(mapping, graph, simClock).run(packetChannel)
    }

    // Aggregator: consumes packets, emits window summaries every 30s
    val summaryChannel = Channel<List<WindowSummary>>(capacity = 1)
    val aggJob = launch {
        WindowAggregator(packetChannel, simClock, windowDuration).run(summaryChannel)
    }

    // Main pipeline loop
    for (summaries in summaryChannel) {
        smoother.update(summaries)
        val weights = weightModel.computeWeights(smoother.states, simClock.now())
        customizeClient.post(weights)
        logger.info { "Window complete: ${summaries.size} edges updated" }
    }
}
```

**Key design point:** The `Channel<SpeedPacket>` is the simulation stand-in
for Kafka/NATS. The `PacketSource` interface abstracts this:

```kotlin
interface PacketSource {
    /** Infinite stream of speed packets. */
    fun packets(): ReceiveChannel<SpeedPacket>
}

class SimulatedPacketSource(
    private val simulator: CameraSimulator,
    private val scope: CoroutineScope
) : PacketSource {
    override fun packets(): ReceiveChannel<SpeedPacket> {
        val channel = Channel<SpeedPacket>(Channel.BUFFERED)
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

### 5.5 Staleness TTL Design

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

**How it integrates with the weight model:**

```
For each covered edge e:
    staleness_conf = stalenessPolicy.confidence(smoother[e].lastUpdateMs, now)
    observation_conf = min(smoother[e].observationCount / CONF_SATURATION, 1.0)

    effective_conf = staleness_conf * observation_conf

    if effective_conf > 0:
        live_weight = speed_to_travel_time(smoother[e].level + smoother[e].trend)
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

## 7. Detailed Algorithm Design

### 6.1 Huber DES Algorithm

Standard Double Exponential Smoothing (Holt's method) with Huber loss for
robust parameter updates:

```
For each window summary (edge_id, observed_speed):
    predicted = level + trend
    residual  = observed_speed - predicted

    // Huber weighting: downweight large residuals
    if |residual| <= delta:
        w = 1.0
    else:
        w = delta / |residual|

    // Weighted DES update
    level = alpha * (w * observed_speed + (1-w) * predicted) + (1-alpha) * (level + trend)
    trend = beta  * (level - prev_level) + (1-beta) * trend
```

**Parameters:**

- `alpha` (level smoothing): 0.3 — reasonable start for 30s windows.
- `beta` (trend smoothing): 0.1 — trend should change slowly.
- `delta` (Huber threshold): ~15 km/h — observations more than 15 km/h off
from prediction are downweighted.

### 6.2 Weight Model (Separate Module)

The weight model is explicitly separated from the smoother. Its interface:

```kotlin
interface WeightModel {
    /**
     * Given smoother output and the current simulation time,
     * produce a complete weight vector for ALL edges.
     *
     * The returned IntArray has exactly numEdges elements,
     * each a u32 travel time in milliseconds, in [1, 2_147_483_646].
     */
    fun computeWeights(
        smootherStates: Map<Int, SmootherState>,
        nowMs: Long
    ): IntArray
}
```

`**LiveWeightModel` implementation responsibilities:**

1. **Covered edges with fresh data:** Convert smoothed speed to travel time
  via `geo_distance`. Confidence-blend with baseline. Apply staleness TTL.
2. **Covered edges gone stale:** Gradual blend back to baseline per TTL policy.
  Once dead, treat as uncovered.
3. **Uncovered edges with influence:** Apply time-of-day modulation +
  neighbor congestion propagation from the influence map.
4. **Uncovered edges without influence:** Apply time-of-day modulation only.
5. **Clamping:** All weights clamped to `[1, 2_147_483_646]`. Zero-weight
  edges create routing black holes. Weights >= INFINITY (2,147,483,647)
   break CCH triangle relaxation.

### 6.3 Camera Simulator Design

```kotlin
/**
 * Batched camera simulation. With 166K cameras and 64 coroutines,
 * each coroutine handles ~2,600 cameras per tick.
 *
 * Each camera generates readings with:
 *   - baseSpeed: derived from the edge's baseline travel_time + geo_distance
 *   - noise: Gaussian with configurable std dev
 *   - spikeProbability: chance of outlier (tests Huber robustness)
 *   - timeOfDayModulation: rush hour slowdown, night speedup
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

## 8. Coverage Model: Quantified Analysis

### 7.1 Hanoi Graph Breakdown by Road Class

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


### 7.2 Camera Placement Is Decoupled from Highway Class

In reality, camera placement doesn't follow highway classification neatly.
A tertiary road might have no camera; a busy residential road near a school
might have one. The simulation should accept an **arbitrary set of covered
edge IDs** — not derive it from road class.

For simulation convenience, the `--auto-cameras tertiary-plus` flag generates
a config from the speed-based heuristic (~166K cameras). But the architecture
must not assume this — the same pipeline should work with 500 cameras or
500,000.

### 7.3 Ingest Rate Analysis


| Camera Count     | Interval | Packets/sec |
| ---------------- | -------- | ----------- |
| 166K (tertiary+) | 5s       | 33,300      |
| 166K (tertiary+) | 10s      | 16,650      |
| 166K (tertiary+) | 30s      | 5,550       |
| 2,000 (sparse)   | 5s       | 400         |


All well within Kotlin `Channel` capacity (millions/sec in-process).

---

## 9. Handling Uncovered Edges — The Central Design Problem

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

## 10. Simulation Scenarios


| Scenario               | Purpose                                                                                                                                        |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **Steady state**       | All cameras report normal speed. Weights should converge to ~baseline. Validates the pipeline doesn't drift.                                   |
| **Rush hour**          | Cameras on arterials report 40% speed drop. Weight vector should increase travel times on those edges; queries should route around congestion. |
| **Camera dropout**     | Kill 30% of cameras mid-run. Affected edges should gracefully degrade via staleness TTL (blend toward baseline over 25 min).                   |
| **Outlier burst**      | 10% of readings are 5x normal speed (sensor glitch). Huber weighting should suppress them; weights should not spike.                           |
| **Gradual congestion** | Speed drops linearly over 10 minutes. DES trend component should anticipate continued degradation.                                             |
| **Sparse coverage**    | Only 2,000 cameras (0.1% of edges). Test that neighbor propagation still produces coherent profiles.                                           |


---

## 11. What NOT to Build Yet

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

## 12. Implementation Order

### Phase 1: Foundation

- `graph/` — RoutingKit binary I/O (read `first_out`, `head`, `travel_time`,
`geo_distance`, `latitude`, `longitude` as little-endian vectors)
- `mapping/` — `CameraMappingSource` interface + JSON loader + auto-generator
- `smoother/` — Huber DES with unit tests
- `weight/WeightModel.kt` — Interface definition
- **Deliverable:** Unit tests pass. Smoother converges on synthetic sequences.
Graph loads correctly from Hanoi data directory.

### Phase 2: Weight Model + Time-of-Day

- `weight/LiveWeightModel.kt` — Baseline blending, confidence, staleness TTL
- `weight/TimeOfDay.kt` — Gaussian modulation
- `weight/StalenessPolicy.kt` — TTL with linear decay
- **Deliverable:** Given synthetic smoother states, produces valid `IntArray`
weight vectors that pass `/customize` validation. Time-of-day creates
visible variation across simulated hours.

### Phase 3: Simulator + Aggregator

- `ingest/CameraSimulator.kt` — Batched coroutines with time-of-day patterns
- `aggregator/WindowAggregator.kt` — Tumbling 30s windows
- **Deliverable:** Cameras produce packets, aggregator emits window summaries.

### Phase 4: End-to-End Pipeline

- `Main.kt` — Wire everything, POST to hanoi-server
- `output/CustomizeClient.kt` — HTTP client
- CLI args (graph dir, server URL, window size, camera config, time accel)
- **Deliverable:** Run alongside `hanoi_server`, observe weight customization,
run queries and see route changes during simulated rush hour.

### Phase 5: Neighbor Propagation

- `weight/InfluenceMap.kt` — BFS precomputation + per-window spillover
- **Deliverable:** Congestion on arterials visibly affects adjacent residential
street weights. Run sparse-coverage scenario to validate.

### Phase 6: Validation

- Run all 6 simulation scenarios from Section 10
- Verify smoother stability, staleness recovery, outlier rejection
- Measure customization latency (window close → server ACK)
- **Deliverable:** Documented test results.

---

## 13. Summary of Recommendations


| Topic                          | Recommendation                                                                                                                               |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| **Language**                   | Kotlin, new `Live_Network_Routing` Gradle project                                                                                            |
| **Streaming**                  | Kotlin coroutine `Channel` for simulation; `PacketSource` interface for future broker swap                                                   |
| **Camera mapping**             | Config-driven (JSON) now; `CameraMappingSource` interface for database-backed production                                                     |
| **Smoothing**                  | Huber DES with windowed aggregation; separate module                                                                                         |
| **Weight model**               | Separate module; takes smoother output → produces `IntArray` for CCH customization                                                           |
| **Uncovered edges**            | Time-of-day Gaussian modulation + neighbor congestion propagation (inverted BFS)                                                             |
| **Staleness**                  | Two-tier TTL (stale at 5min, dead at 30min) with linear confidence decay                                                                     |
| **Broker**                     | Deferred to DE team; `PacketSource` interface is the extension point                                                                         |
| **Customization frequency**    | 30s windows; sub-10s documented as future work with three candidate approaches                                                               |
| **Camera → edge (production)** | Database-backed via `camera_edge_map` table; GIS snapping is a separate project                                                              |
| **Modularity**                 | Each stage is an interface (`PacketSource`, `Aggregator`, `Smoother`, `WeightModel`, `WeightOutput`); swap any stage without touching others |


