# rust_road_router Algorithm Families Walkthrough

## Purpose

This document answers a practical question:

> Beyond the CH/CCH stack, what other algorithm families or "toolkits" exist in
> `rust_road_router`?

The short answer is:

- Yes, there are other algorithms in the codebase.
- Most of them live as modules inside the `engine` crate.
- The extra workspace crates are mostly experiment and CLI/binary workbenches,
  not separate polished library toolkits.

---

## 1. Where To Look First

There are two useful ways to navigate `rust_road_router`:

1. The workspace members in `rust_road_router/Cargo.toml`
2. The algorithm registry in `rust_road_router/engine/src/algo/mod.rs`

### Important caveat: README vs workspace manifest

The top-level `rust_road_router/README.md` still says "There are two crates"
(`engine` and `server`). That description is stale. The current workspace
manifest lists:

- `engine`
- `server`
- `conversion`
- `visualization`
- `utils`
- `catchup`
- `chpot`
- `tdpot`
- `cchpp`

Treat `Cargo.toml` as the source of truth for current workspace structure.

---

## 2. Big Picture

The cleanest mental model is:

- `engine`: the real algorithm library
- `server`: HTTP/API layer on top of the engine
- `conversion`, `visualization`, `utils`: support tooling
- `catchup`, `chpot`, `tdpot`, `cchpp`: mostly binary crates for experiments,
  evaluations, and specialized preprocessing/query workflows

So if you are asking "what algorithm implementations exist?", start in:

- `../../rust_road_router/engine/src/algo/mod.rs`

If you are asking "what standalone toolkit crates exist?", the answer is more
limited: the specialized crates are real workspace members, but they are mostly
collections of binaries rather than large reusable library APIs.

---

## 3. Non-CH/CCH Algorithms In The Engine

These are the most obvious "other algorithm families" beyond the core
Contraction Hierarchy / Customizable Contraction Hierarchy code.

| Module | What it provides | How independent is it from CH/CCH? |
|---|---|---|
| `dijkstra` | Several Dijkstra variants and the baseline query machinery | Fully independent baseline shortest path family |
| `a_star` | Generic A* framework with pluggable potentials | Independent framework |
| `alt` | Landmark-based A* potentials (ALT) | Mostly independent; uses Dijkstra preprocessing |
| `hl` | Hub Labels | Distinct query family, but built from upward/downward graphs |
| `topocore` | TopoCore-style preprocessing / graph reduction | Separate preprocessing idea |

### 3.1 Dijkstra

`engine/src/algo/dijkstra.rs` is exactly what it says: several Dijkstra
variants.

Use this when you want:

- the simplest exact shortest-path baseline
- something easy to understand and debug
- a correctness reference for validating faster methods

This is the most obviously non-CH/CCH part of the engine.

### 3.2 A*

`engine/src/algo/a_star.rs` provides the general A* framework. It defines the
potential interfaces and the reusable query logic that other potential types can
plug into.

This means A* is present as a first-class concept, not only as an implementation
detail of CH/CCH experiments.

### 3.3 ALT

`engine/src/algo/alt.rs` implements ALT-style landmark potentials on top of the
A* framework.

This is one of the clearest examples of a different routing family in the repo:

- preprocessing: run landmark Dijkstras
- query: use A* with landmark-based lower bounds

If you want a "non-hierarchical but still accelerated" shortest-path method,
ALT is one of the best places to start reading.

### 3.4 Hub Labels

`engine/src/algo/hl.rs` implements hub labels.

This is a different query strategy from CH/CCH, even though the implementation
computes labels from upward/downward graphs. In other words:

- it is not a separate workspace toolkit
- but it is a distinct algorithm family inside the engine

### 3.5 TopoCore

`engine/src/algo/topocore.rs` implements TopoCore preprocessing.

This is useful to think of as graph reduction / core extraction machinery rather
than a full end-user query toolkit by itself. It sits outside the standard
CH/CCH customization/query flow.

---

## 4. CH-Derived But Still Distinct Algorithmic Modules

There are also several modules that are not "plain CH/CCH shortest path", but
are still heavily built on CH/CCH structures.

| Module | Role | Relationship to CH/CCH |
|---|---|---|
| `rphast` | Restricted PHAST-style queries | Usually used with CH-style ordered graphs |
| `ch_potentials` | CH/CCH-based A* potentials | Directly derived from CH/CCH |
| `catchup` | CATCHUp / floating TD-CCH | Explicitly CCH-based |
| `td_astar` | Time-dependent A* infrastructure | Often paired with CCH-based potentials |
| `time_dependent_sampling` | TD-S heuristic | Query stage is distinct, preprocessing uses CCH |
| `traffic_aware` | Live-traffic / blocked-path / alternative-route style logic | Reuses CH/CCH potentials and path tooling |
| `minimal_nonshortest_subpaths` | Path-quality / non-shortest subpath analysis and fixing | Built on CCH potentials |
| `metric_merging` | Merge similar metrics for multi-metric TD preprocessing | Support algorithm, not a standalone query family |

### 4.1 RPHAST

`engine/src/algo/rphast.rs` is a different query style from ordinary source to
target search. It is useful for one-to-many or many-to-many style workloads, but
in this codebase it is still closely tied to ordered forward/backward graphs and
CH-style infrastructure.

### 4.2 Time-Dependent Families

There are three different time-dependent directions to keep straight:

- `catchup`: CATCHUp / TD-CCH style algorithms
- `td_astar`: general time-dependent A* infrastructure
- `time_dependent_sampling`: TD-S heuristic that samples static windows and then
  runs a restricted time-dependent Dijkstra

So yes, time-dependent routing is present, but much of it still uses CCH as the
preprocessing backbone.

### 4.3 Traffic-Aware and Path-Repair Logic

`traffic_aware` and `minimal_nonshortest_subpaths` are best thought of as
workflow-oriented algorithm modules:

- handling live traffic effects
- working with blocked or forbidden paths
- analyzing or repairing path quality

These are useful if your interest is beyond "single shortest path on a static
graph".

---

## 5. What The Extra Workspace Crates Actually Are

The workspace has several crates that look like separate toolkits at first
glance:

- `catchup`
- `chpot`
- `tdpot`
- `cchpp`

In practice, they are mostly binary crates around experiments and workflows.

### 5.1 Why they do not feel like separate library toolkits

Their `src/lib.rs` files are empty or trivial, while their `src/bin/` folders
contain the real content.

That is a strong signal that these crates are mainly:

- command-line experiment drivers
- benchmarking entry points
- workflow scripts encoded as Rust binaries
- one-off research / evaluation tools

### 5.2 Crate-by-crate summary

| Crate | What it appears to focus on |
|---|---|
| `catchup` | TD-CCH / CATCHUp preprocessing, queries, and profile experiments |
| `chpot` | CH/CCH potential experiments, blocked/live/turn-aware variants, RPHAST comparisons |
| `tdpot` | Time-dependent potential experiments, live customization, predicted/live query studies |
| `cchpp` | CCH preprocessing/post-processing experiments, turn expansion, nearest-neighbor/query feature studies |

So these crates are useful, but they are mostly **CH/CCH-family workbenches**,
not separate "ALT toolkit", "Hub Labels toolkit", or "Dijkstra toolkit"
packages.

---

## 6. Practical Answer To "Are There Other Toolkits?"

### If you mean other algorithm implementations

Yes. The biggest ones are:

- Dijkstra
- A*
- ALT
- Hub Labels
- TopoCore
- time-dependent A* infrastructure
- time-dependent sampling

### If you mean separate top-level reusable toolkit crates

Not really, at least not in the same sense as the CH/CCH-oriented parts.

The non-CH/CCH algorithms mostly live inside the `engine` crate as modules. The
extra workspace crates are mostly experiment/binary wrappers, and the ones that
stand out are still largely CH/CCH-derived.

---

## 7. Where To Start Depending On Your Goal

### I want the simplest exact shortest path baseline

Start with:

- `engine/src/algo/dijkstra.rs`

### I want a classic heuristic speedup that is not CH/CCH

Start with:

- `engine/src/algo/a_star.rs`
- `engine/src/algo/alt.rs`

### I want a different query family altogether

Start with:

- `engine/src/algo/hl.rs`

### I want many-to-many style acceleration

Start with:

- `engine/src/algo/rphast.rs`

### I care about time-dependent routing

Start with:

- `engine/src/algo/td_astar.rs`
- `engine/src/algo/time_dependent_sampling.rs`
- `engine/src/algo/catchup.rs`

### I want the standalone experiment entry points

Browse:

- `../../rust_road_router/catchup/src/bin/`
- `../../rust_road_router/chpot/src/bin/`
- `../../rust_road_router/tdpot/src/bin/`
- `../../rust_road_router/cchpp/src/bin/`

---

## 8. Bottom Line

`rust_road_router` absolutely contains algorithms beyond the base CH/CCH query
stack, but the shape of the codebase matters:

- the `engine` crate is where the algorithm implementations really live
- the cleanest non-CH/CCH families are Dijkstra, A*, ALT, Hub Labels, and
  TopoCore
- many of the advanced time-dependent and traffic-aware modules are still built
  on CH/CCH ideas
- the extra workspace crates are mostly binary workbenches rather than separate
  library-style toolkits

If you are trying to choose a reading order, use this progression:

1. `dijkstra`
2. `a_star`
3. `alt`
4. `hl`
5. `topocore`
6. then the CH-derived advanced modules (`rphast`, `catchup`, `td_astar`,
   `traffic_aware`)
