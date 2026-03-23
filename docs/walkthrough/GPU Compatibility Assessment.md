# GPU Compatibility Assessment

Assessment of GPU viability for the CCH routing pipeline and a planned DES+VDF data processing pipeline. Evaluated against deployment on server-grade CPUs (EPYC/Xeon, 64-96 cores) with 2x NVIDIA A30 GPUs available.

---

## Deployment Context

| Parameter | Value |
|---|---|
| **Loaded graph** | ~900k nodes, ~1.2M edges |
| **Line graph** (turn-expanded) | ~2.7M nodes, ~3.6M edges (est. 3x loaded) |
| **Traffic data source** | Camera systems, return packet every 10-30s |
| **Pipeline latency budget** | 10 seconds total (acceptable for camera cycle) |
| **Server CPU** | EPYC 9004 or Xeon Scalable (64-96 cores, 300-460 GB/s DDR5) |
| **Available GPU** | 2x NVIDIA A30 24GB HBM2 (Ampere GA100, 3804 CUDA cores, 933 GB/s, MIG capable) |

---

## Current State

No GPU code exists in the project. All parallelism is CPU-based:

| Component | Tech | Notes |
|---|---|---|
| CCH Customization | rayon | Separator-based parallel triangle relaxation |
| RoutingKit | OpenMP | Dynamic scheduling |
| InertialFlowCutter | Intel TBB | Flow partitioning |
| Dijkstra / Query | None | Sequential per query, <1ms |
| CCH Contraction | None | Sequential, one-time preprocessing |

---

## CCH Customization: GPU Not Viable

CCH customization — the only recurring compute-heavy stage — is a poor GPU candidate. The hierarchy topology is fixed from one-time contraction preprocessing. Re-customization only propagates new edge weights through existing shortcuts via two sub-phases:

1. **Respecting** — `par_iter` copy of weights into CCH slots (milliseconds)
2. **Triangle relaxation** — bottom-up through separator tree, relaxing shortcut weights via `min(edge_a + edge_b)`. Independent cells parallelize via rayon; separator level transitions are sequential barriers.

**Why not GPU:** Irregular triangle counts per edge (2 to thousands), scattered CSR memory access (defeats coalesced reads), and mandatory sequential barriers at separator levels. The rayon implementation already exploits all available independence. GPU would require a complete rewrite for marginal gains (<2x) over a high-core-count server CPU.

Re-customization cost is **independent of time series length** — it only sees the final `travel_time` vector. Other pipeline stages (contraction, query, IFC) are either one-time preprocessing or already sub-millisecond — not relevant to the update cycle.

---

## DES+VDF Data Processing Pipeline: GPU Viable

The planned pipeline is a textbook data-parallel workload — identical computation applied independently per edge:

```
Raw traffic time series (per edge)
  → Huber-Robust Double ES smoothing → smoothed estimates
    → VDF model → travel_time per edge (ms)
      → CCH re-customization → serve queries
```

| Property | GPU Suitability |
|---|---|
| Independent per edge | Perfect — one thread per edge |
| Uniform computation | Perfect — same kernel everywhere |
| Memory access | Good — sequential reads along time series |
| Branching (Huber `\|r\| < δ`) | Minor penalty (~5-10%, warps mostly agree) |

**Integration point:** Output plugs into the `/customize` endpoint in `server/src/main.rs` or as a `travel_time` binary file.

---

## Pipeline Budget Analysis

24-hour sliding window (288 five-minute samples per edge). Budget: 10 seconds total.

### DES+VDF Stage

| Scenario | Mid-range server (32-48 cores) | High-end server (96 cores) | GPU (single A30) |
|---|---|---|---|
| Loaded graph (1.2M edges) | ~0.6-1.5 s | ~0.3-0.6 s | ~0.1 s |
| Line graph (3.6M edges) | ~1.5-4.0 s | ~0.8-2.0 s | ~0.3 s |

### CCH Re-Customization Stage (CPU-only, independent of DES window)

| Scenario | Mid-range server (32-48 cores) | High-end server (96 cores) |
|---|---|---|
| Loaded graph (1.2M edges) | ~0.4-1.2 s | ~0.2-0.8 s |
| Line graph (3.6M edges) | ~1.0-3.0 s | ~0.5-2.0 s |

### Total Pipeline (DES+VDF + CCH re-customization)

| Scenario | Mid-range (32-48 cores) | High-end (96 cores) | GPU DES + CPU CCH |
|---|---|---|---|
| Loaded graph | ~1.0-2.7 s ✅ | ~0.5-1.4 s ✅ | ~0.3-1.0 s ✅ |
| Line graph | ~2.5-7.0 s ✅ | ~1.3-4.0 s ✅ | ~0.8-2.3 s ✅ |

All scenarios fit within the 10-second budget across all CPU tiers. The VDF pipeline operates on real-time smoothed data only — no historical batch processing — so the DES window is bounded by the camera cycle (recent observations, not week-long accumulation).

---

## GPU Memory Requirements (if using A30)

| Scenario | Data Size | Fits in |
|---|---|---|
| Loaded graph | ~1.4 GB | 1/4 MIG slice (6 GB) |
| Line graph | ~4.3 GB | 1/4 MIG slice (tight) |

Both scenarios fit comfortably in a single A30 (24 GB). A 1/4 MIG slice (6 GB) suffices for the loaded graph; the line graph is tight but feasible in a single slice. The second A30 is not needed for the DES+VDF workload.

---

## Implementation Trade-offs

| Concern | CPU (rayon) | GPU (CUDA/wgpu) |
|---|---|---|
| Dev effort | ~1-2 days | ~1-2 weeks |
| Code volume | ~100-200 lines | ~400-800 lines |
| Debugging | Standard tools | Nsight/RenderDoc, opaque faults |
| Testing | `cargo test` anywhere | Needs GPU hardware in CI |
| Deployment | Any server | NVIDIA drivers, nvidia-container-toolkit |
| Portability | Universal | CUDA = NVIDIA only; wgpu = broader but ~20-30% slower |
| Maintenance | Zero driver deps | Driver/toolkit version pinning |

### A30-Specific Considerations

**Benefits:** 933 GB/s HBM2 (8-10x server DDR5), MIG for multi-tenant sharing, passive-cooled 165W (datacenter-friendly), ECC memory, NVLink for multi-GPU if needed.

**Drawbacks:** CUDA lock-in for best performance, passive cooling requires rack-mount airflow, 2x A30 is overprovisioned for DES+VDF alone (single MIG slice suffices), 330W idle power if underutilized.

---

## Recommendation

```
Primary path: CPU-only with server-grade hardware

  - Implement DES+VDF as rayon-parallel Rust code
  - Deploy on EPYC/Xeon (32+ cores)
  - Total pipeline: 1-7 seconds (mid-range) or 0.5-4 seconds (high-end)
  - All scenarios fit within 10s budget, no GPU needed
  - No GPU code, no CUDA dependency, no driver maintenance

GPU path (A30s): justified only if ANY of these hold

  1. Other GPU workloads share the server
     → MIG-slice DES+VDF alongside ML inference, etc.
     → Marginal added complexity to an already-GPU stack

  2. Scale increases beyond Hanoi (10M+ edges, multiple cities)
     → Server CPUs start to struggle at that scale

  3. Latency requirements tighten below 1 second
     → GPU DES+VDF (~100-300ms) reclaims budget
     → Still bounded by CPU re-customization (~0.5-2s)

If none hold: defer GPU adoption. CPU-only is simpler and fast enough.
```

**Architectural takeaway:** The pipeline naturally separates at the `travel_time` weight vector boundary. Graph algorithms (CCH customization + query) stay on CPU. Data processing (DES + VDF) is the only GPU candidate, but since the VDF operates on real-time smoothed data only (not historical batches), server CPUs handle it comfortably within budget. Keep the A30s for other workloads or future scale.
