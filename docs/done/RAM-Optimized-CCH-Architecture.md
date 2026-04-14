# RAM-Optimized CCH Architecture

Full serialization + mmap plan to reduce steady-state RAM from ~2.2 GB to
~400–600 MB for Hanoi motorcycle line graph.

## Status: AMENDED — 5+5+3+4+2+3 issues identified and addressed 2026-04-08

---

## 1. Current Memory Layout (Motorcycle Line Graph)

| Component                                              | RAM             | Source                     |
| ------------------------------------------------------ | --------------- | -------------------------- |
| Base graph (first_out, head, travel_time, lat, lng)    | ~63 MB          | Disk → Vec                 |
| DirectedCCH topology (fw/bw first_out, head, tail × 2) | ~400–600 MB     | Computed                   |
| Vecs\<EdgeIdT\> mappings (fw + bw edge→orig)           | ~300–500 MB     | Computed                   |
| ReversedGraphWithEdgeIds (fw + bw inverted)            | ~200–400 MB     | Computed                   |
| Elimination tree                                       | ~4 MB           | Computed                   |
| SeparatorTree                                          | ~10–30 MB       | Computed                   |
| NodeOrder                                              | ~15 MB          | Disk → Arc\<[T]\>          |
| CustomizedBasic (up/down weights + unpacking)          | ~150–250 MB     | Computed per customization |
| AlternativeServer workspace                            | ~150–200 MB     | Allocated at init          |
| KD-tree + SpatialIndex                                 | ~40–60 MB       | Computed                   |
| Misc (traffic overlay, route evaluator, etc.)          | ~40 MB          | Computed                   |
| **Total**                                              | **~1.4–2.2 GB** |                            |

---

## 2. What Can Be Mmap'd

### Classification

Every `Vec<T>` field in CCH/DirectedCCH is **write-once, read-many** after
construction. The CCHT trait exposes everything as `&[T]` slices. No field is
mutated post-construction.

| Category               | Fields                                 | Mmap-able       | Blocker                                            |
| ---------------------- | -------------------------------------- | --------------- | -------------------------------------------------- |
| **CCH topology**       | fw/bw first_out, head, tail            | Yes             | Not on disk yet                                    |
| **Edge→orig mappings** | Vecs\<EdgeIdT\> (first_idx + data)     | Yes             | Not on disk, needs flat layout                     |
| **Inverted graphs**    | ReversedGraphWithEdgeIds (3 Vecs each) | Yes             | Not on disk yet                                    |
| **Elimination tree**   | Vec\<InRangeOption\<NodeId\>\>         | Yes             | Not on disk yet                                    |
| **SeparatorTree**      | Recursive tree of ranges               | **Reconstruct** | Cannot mmap a tree; rebuild from elim tree (~5 ms) |
| **NodeOrder**          | Arc\<[NodeId]\> × 2                    | Yes             | Already Arc; can mmap ranks, rebuild order         |
| **Customized weights** | upward, downward Vecs                  | **No**          | Mutated every customization                        |
| **Unpacking tables**   | up_unpacking, down_unpacking           | **No**          | Mutated every customization                        |
| **Query workspaces**   | fw/bw*distances, parents, ttest*\*     | **No**          | Mutated every query                                |
| **Base graph**         | first_out, head, travel_time, lat, lng | Yes             | Already on disk                                    |

### Savings Estimate

| What                 | Current RAM | After mmap                | Saving          |
| -------------------- | ----------- | ------------------------- | --------------- |
| Base graph vectors   | 63 MB       | ~0 MB (file-backed)       | 63 MB           |
| DirectedCCH topology | 400–600 MB  | ~0 MB (file-backed)       | 400–600 MB      |
| Vecs mappings        | 300–500 MB  | ~0 MB (file-backed)       | 300–500 MB      |
| Inverted graphs      | 200–400 MB  | ~0 MB (file-backed)       | 200–400 MB      |
| Elimination tree     | 4 MB        | ~0 MB (file-backed)       | 4 MB            |
| SeparatorTree        | 10–30 MB    | 10–30 MB (rebuilt in RAM) | 0 MB            |
| NodeOrder            | 15 MB       | ~0 MB (file-backed)       | 15 MB           |
| **Subtotal savings** |             |                           | **~1.0–1.8 GB** |

Remaining in RAM: CustomizedBasic (~200 MB) + workspaces (~200 MB) +
SeparatorTree (~20 MB) + misc (~40 MB) = **~400–600 MB**.

---

## 3. Architecture: Two-Phase Startup

### Phase A: Build & Serialize (first run or data change)

```
Graph files → CCH::fix_order_and_build() → to_directed_cch()
                                                ↓
                                    serialize all structures
                                    to <graph_dir>/cch_cache/
```

Files written to `cch_cache/`:

| File                    | Content                                                       | Type                  |
| ----------------------- | ------------------------------------------------------------- | --------------------- |
| `directed_fw_first_out` | DirectedCCH.forward_first_out                                 | Vec\<u32\>            |
| `directed_fw_head`      | DirectedCCH.forward_head                                      | Vec\<u32\>            |
| `directed_fw_tail`      | DirectedCCH.forward_tail                                      | Vec\<u32\>            |
| `directed_bw_first_out` | DirectedCCH.backward_first_out                                | Vec\<u32\>            |
| `directed_bw_head`      | DirectedCCH.backward_head                                     | Vec\<u32\>            |
| `directed_bw_tail`      | DirectedCCH.backward_tail                                     | Vec\<u32\>            |
| `fw_edge_to_orig_idx`   | Vecs.first_idx (forward)                                      | Vec\<u32\>            |
| `fw_edge_to_orig_data`  | Vecs.data (forward)                                           | Vec\<u32\>            |
| `bw_edge_to_orig_idx`   | Vecs.first_idx (backward)                                     | Vec\<u32\>            |
| `bw_edge_to_orig_data`  | Vecs.data (backward)                                          | Vec\<u32\>            |
| `fw_inverted_first_out` | forward_inverted.first_out                                    | Vec\<u32\>            |
| `fw_inverted_head`      | forward_inverted.head                                         | Vec\<u32\>            |
| `fw_inverted_edge_ids`  | forward_inverted.edge_ids                                     | Vec\<u32\>            |
| `bw_inverted_first_out` | backward_inverted.first_out                                   | Vec\<u32\>            |
| `bw_inverted_head`      | backward_inverted.head                                        | Vec\<u32\>            |
| `bw_inverted_edge_ids`  | backward_inverted.edge_ids                                    | Vec\<u32\>            |
| `elimination_tree`      | elimination_tree                                              | Vec\<u32\>            |
| `ranks`                 | NodeOrder.ranks (via save_each key "ranks")                   | Vec\<u32\>            |
| `cache_meta`            | JSON: version, endianness, pointer_width, checksum, timestamp | Validity + ABI marker |

**Cache directory layout** — placed inside the graph directory being loaded,
which is already profile-scoped:

```
Maps/data/hanoi_motorcycle/line_graph/cch_cache/    ← DirectedCCH for motorcycle line graph
Maps/data/hanoi_motorcycle/graph/cch_cache/          ← CCH for motorcycle normal graph
Maps/data/hanoi_car/line_graph/cch_cache/            ← DirectedCCH for car line graph
Maps/data/hanoi_car/graph/cch_cache/                 ← CCH for car normal graph
```

Each `cch_cache/` is tied to the graph directory it lives in. No cross-profile
or cross-mode conflicts possible — the SHA-256 checksum covers the source files
in that specific directory.

### Phase B: Load from Cache (subsequent runs)

```
cch_cache/ exists && cache_meta valid (version + endianness + pointer_width + checksum)?
  → DirectedCCHReconstructor.reconstruct_from(&cache_dir) [validates all invariants]
  → rebuild SeparatorTree from elimination_tree (~5 ms)
  → rebuild NodeOrder from ranks (~2 ms)
  → proceed to customization (Phase 2)
```

---

## 4. Implementation Plan

> **APPROVED EXCEPTION:** rust_road_router changes are normally forbidden, but
> this exception has been explicitly authorized for Steps 1–2. Changes are
> strictly limited to: accessors (read-only getters), validated constructors
> (`from_raw`, `from_raw_validated`), and `Deconstruct` / `ReconstructPrepared`
> trait impls. Zero algorithm changes.

### Step 1a: `Vecs<T>` — Accessors + Validated Reconstruction (rust_road_router)

**File:** `engine/src/util.rs`

`first_idx` stays as `Vec<usize>` internally. The in-memory usize→u32 conversion
originally proposed is WITHDRAWN. The serialization path converts on-the-fly via
`first_idx_as_u32()`, and `from_raw()` converts back. This avoids touching any
existing `Vecs<T>` call sites.

```rust
impl<T> Vecs<T> {
    /// Convert usize indices to u32 for serialization.
    pub fn first_idx_as_u32(&self) -> Vec<u32> {
        self.first_idx.iter().map(|&x| x as u32).collect()
    }
    pub fn data_as_slice(&self) -> &[T] { &self.data }

    /// Reconstruct from pre-built index and data arrays.
    /// Returns io::Error on invalid input — cache files are external data.
    pub fn from_raw(first_idx: Vec<u32>, data: Vec<T>) -> std::io::Result<Self> {
        let check = |cond: bool, msg: &str| -> std::io::Result<()> {
            if cond { Ok(()) } else {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string()))
            }
        };
        check(!first_idx.is_empty(), "Vecs first_idx must be non-empty")?;
        check(first_idx[0] == 0, "Vecs first_idx must start at 0")?;
        check(*first_idx.last().unwrap() as usize == data.len(),
            "Vecs first_idx last entry must equal data.len()")?;
        for w in first_idx.windows(2) {
            check(w[0] <= w[1], "Vecs first_idx must be monotonically non-decreasing")?;
        }
        let idx_usize: Vec<usize> = first_idx.into_iter().map(|x| x as usize).collect();
        Ok(Vecs { first_idx: idx_usize, data })
    }
}
```

### Step 1b: `ReversedGraphWithEdgeIds` — Accessors + Validated Reconstruction (rust_road_router)

**File:** `engine/src/datastr/graph/first_out_graph.rs`

```rust
impl ReversedGraphWithEdgeIds {
    pub fn first_out(&self) -> &[EdgeId] { &self.first_out }
    pub fn head_slice(&self) -> &[NodeId] { &self.head }
    pub fn edge_ids(&self) -> &[EdgeId] { &self.edge_ids }

    /// Reconstruct from pre-built arrays with CSR validation.
    /// Returns io::Error on invalid input — cache files are external data.
    pub fn from_raw_validated(
        first_out: Vec<EdgeId>, head: Vec<NodeId>, edge_ids: Vec<EdgeId>,
    ) -> std::io::Result<Self> {
        let check = |cond: bool, msg: &str| -> std::io::Result<()> {
            if cond { Ok(()) } else {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string()))
            }
        };
        check(!first_out.is_empty(), "inverted first_out must be non-empty")?;
        check(*first_out.first().unwrap() == 0, "inverted first_out[0] != 0")?;
        check(*first_out.last().unwrap() as usize == head.len(),
            "inverted first_out sentinel != head.len")?;
        for w in first_out.windows(2) {
            check(w[0] <= w[1], "inverted first_out not monotonically non-decreasing")?;
        }
        check(head.len() == edge_ids.len(), "inverted head.len != edge_ids.len")?;
        Ok(ReversedGraphWithEdgeIds { first_out, head, edge_ids })
    }
}
```

### Step 2: `Deconstruct` + `ReconstructPrepared` for DirectedCCH (rust_road_router)

**File:** `engine/src/algo/customizable_contraction_hierarchy/mod.rs`

> **Crate boundary note:** `DirectedCCH` has all-private fields. Both
> `Deconstruct` and `ReconstructPrepared` MUST be implemented inside
> `rust_road_router`. This follows the existing pattern: `CCH` already has
> `Deconstruct` + `CCHReconstrctor` in the same file.

Add `Deconstruct` impl that serializes wrapper types through their inner `u32`
values (not raw byte reinterpretation — see P1):

```rust
impl Deconstruct for DirectedCCH {
    fn save_each(&self, store: &dyn Fn(&str, &dyn Save) -> std::io::Result<()>) -> std::io::Result<()> {
        store("directed_fw_first_out", &self.forward_first_out)?;
        store("directed_fw_head", &self.forward_head)?;
        store("directed_fw_tail", &self.forward_tail)?;
        store("directed_bw_first_out", &self.backward_first_out)?;
        store("directed_bw_head", &self.backward_head)?;
        store("directed_bw_tail", &self.backward_tail)?;
        // Vecs<EdgeIdT> — serialize index as u32, data as unwrapped u32
        store("fw_edge_to_orig_idx", &self.forward_cch_edge_to_orig_arc.first_idx_as_u32())?;
        let fw_data: Vec<u32> = self.forward_cch_edge_to_orig_arc.data_as_slice()
            .iter().map(|&EdgeIdT(x)| x).collect();
        store("fw_edge_to_orig_data", &fw_data)?;
        store("bw_edge_to_orig_idx", &self.backward_cch_edge_to_orig_arc.first_idx_as_u32())?;
        let bw_data: Vec<u32> = self.backward_cch_edge_to_orig_arc.data_as_slice()
            .iter().map(|&EdgeIdT(x)| x).collect();
        store("bw_edge_to_orig_data", &bw_data)?;
        // Inverted graphs (all fields are plain u32)
        store("fw_inverted_first_out", &self.forward_inverted.first_out())?;
        store("fw_inverted_head", &self.forward_inverted.head_slice())?;
        store("fw_inverted_edge_ids", &self.forward_inverted.edge_ids())?;
        store("bw_inverted_first_out", &self.backward_inverted.first_out())?;
        store("bw_inverted_head", &self.backward_inverted.head_slice())?;
        store("bw_inverted_edge_ids", &self.backward_inverted.edge_ids())?;
        // Elimination tree — unwrap InRangeOption<u32> to raw u32
        let elim: Vec<u32> = self.elimination_tree.iter()
            .map(|opt| opt.value().unwrap_or(u32::MAX)).collect();
        store("elimination_tree", &elim)?;
        // NodeOrder
        self.node_order.save_each(&|name, data| store(name, data))?;
        Ok(())
    }
}
```

Add `ReconstructPrepared` impl with comprehensive structural validation. All
checks use `io::ErrorKind::InvalidData` (not `assert!`) because cache files are
external input — corruption should trigger rebuild, not crash:

```rust
fn cache_check(cond: bool, msg: impl Into<String>) -> std::io::Result<()> {
    if cond { Ok(()) } else {
        Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into()))
    }
}

pub struct DirectedCCHReconstructor;

impl ReconstructPrepared<DirectedCCH> for DirectedCCHReconstructor {
    fn reconstruct_with(self, loader: Loader) -> std::io::Result<DirectedCCH> {
        let forward_first_out: Vec<EdgeId> = loader.load("directed_fw_first_out")?;
        let forward_head: Vec<NodeId> = loader.load("directed_fw_head")?;
        let forward_tail: Vec<NodeId> = loader.load("directed_fw_tail")?;
        let backward_first_out: Vec<EdgeId> = loader.load("directed_bw_first_out")?;
        let backward_head: Vec<NodeId> = loader.load("directed_bw_head")?;
        let backward_tail: Vec<NodeId> = loader.load("directed_bw_tail")?;

        // --- CSR invariants for forward/backward topology ---
        cache_check(!forward_first_out.is_empty(), "fw first_out empty")?;
        cache_check(*forward_first_out.first().unwrap() == 0, "fw first_out[0] != 0")?;
        cache_check(*forward_first_out.last().unwrap() as usize == forward_head.len(),
            format!("fw first_out sentinel ({}) != head.len ({})",
                forward_first_out.last().unwrap(), forward_head.len()))?;
        for w in forward_first_out.windows(2) {
            cache_check(w[0] <= w[1], "fw first_out not monotonically non-decreasing")?;
        }
        cache_check(forward_head.len() == forward_tail.len(),
            "fw head.len != tail.len")?;
        cache_check(!backward_first_out.is_empty(), "bw first_out empty")?;
        cache_check(*backward_first_out.first().unwrap() == 0, "bw first_out[0] != 0")?;
        cache_check(*backward_first_out.last().unwrap() as usize == backward_head.len(),
            format!("bw first_out sentinel ({}) != head.len ({})",
                backward_first_out.last().unwrap(), backward_head.len()))?;
        for w in backward_first_out.windows(2) {
            cache_check(w[0] <= w[1], "bw first_out not monotonically non-decreasing")?;
        }
        cache_check(backward_head.len() == backward_tail.len(),
            "bw head.len != tail.len")?;
        cache_check(forward_first_out.len() == backward_first_out.len(),
            "fw/bw first_out length mismatch (different num_nodes)")?;

        let num_nodes = forward_first_out.len() - 1;
        let num_fw_edges = forward_head.len();
        let num_bw_edges = backward_head.len();

        // --- Value-range checks for topology arrays (used as direct indices + get_unchecked_mut) ---
        // customization/directed.rs:84,122 cites "head nodes < n" as the safety invariant.
        // Corrupt cache can violate this without length checks catching it.
        for &h in &forward_head {
            cache_check((h as usize) < num_nodes,
                format!("fw head value {} >= num_nodes {}", h, num_nodes))?;
        }
        for &t in &forward_tail {
            cache_check((t as usize) < num_nodes,
                format!("fw tail value {} >= num_nodes {}", t, num_nodes))?;
        }
        for &h in &backward_head {
            cache_check((h as usize) < num_nodes,
                format!("bw head value {} >= num_nodes {}", h, num_nodes))?;
        }
        for &t in &backward_tail {
            cache_check((t as usize) < num_nodes,
                format!("bw tail value {} >= num_nodes {}", t, num_nodes))?;
        }

        // --- Vecs<EdgeIdT> (edge→orig mappings) ---
        let fw_idx: Vec<u32> = loader.load("fw_edge_to_orig_idx")?;
        let fw_data_raw: Vec<u32> = loader.load("fw_edge_to_orig_data")?;
        cache_check(fw_idx.len() == num_fw_edges + 1,
            format!("fw_edge_to_orig_idx.len ({}) != num_fw_edges+1 ({})",
                fw_idx.len(), num_fw_edges + 1))?;
        let fw_data: Vec<EdgeIdT> = fw_data_raw.into_iter().map(EdgeIdT).collect();
        let forward_cch_edge_to_orig_arc = Vecs::from_raw(fw_idx, fw_data)?;

        let bw_idx: Vec<u32> = loader.load("bw_edge_to_orig_idx")?;
        let bw_data_raw: Vec<u32> = loader.load("bw_edge_to_orig_data")?;
        cache_check(bw_idx.len() == num_bw_edges + 1,
            format!("bw_edge_to_orig_idx.len ({}) != num_bw_edges+1 ({})",
                bw_idx.len(), num_bw_edges + 1))?;
        let bw_data: Vec<EdgeIdT> = bw_data_raw.into_iter().map(EdgeIdT).collect();
        let backward_cch_edge_to_orig_arc = Vecs::from_raw(bw_idx, bw_data)?;

        // --- Inverted graphs (must match corresponding fw/bw topology) ---
        let fw_inv_first_out: Vec<EdgeId> = loader.load("fw_inverted_first_out")?;
        let fw_inv_head: Vec<NodeId> = loader.load("fw_inverted_head")?;
        let fw_inv_edge_ids: Vec<EdgeId> = loader.load("fw_inverted_edge_ids")?;
        cache_check(fw_inv_first_out.len() == num_nodes + 1,
            format!("fw_inverted first_out.len ({}) != num_nodes+1 ({})",
                fw_inv_first_out.len(), num_nodes + 1))?;
        cache_check(fw_inv_head.len() == num_fw_edges,
            format!("fw_inverted head.len ({}) != num_fw_edges ({})",
                fw_inv_head.len(), num_fw_edges))?;
        // inverted head values are back-references into fw topology → must be < num_nodes
        for &h in &fw_inv_head {
            cache_check((h as usize) < num_nodes,
                format!("fw_inverted head value {} >= num_nodes {}", h, num_nodes))?;
        }
        // inverted edge_ids are indices into fw edges → must be < num_fw_edges
        cache_check(fw_inv_edge_ids.len() == num_fw_edges,
            format!("fw_inverted edge_ids.len ({}) != num_fw_edges ({})",
                fw_inv_edge_ids.len(), num_fw_edges))?;
        for &eid in &fw_inv_edge_ids {
            cache_check((eid as usize) < num_fw_edges,
                format!("fw_inverted edge_id {} >= num_fw_edges {}", eid, num_fw_edges))?;
        }
        let forward_inverted = ReversedGraphWithEdgeIds::from_raw_validated(
            fw_inv_first_out, fw_inv_head, fw_inv_edge_ids)?;

        let bw_inv_first_out: Vec<EdgeId> = loader.load("bw_inverted_first_out")?;
        let bw_inv_head: Vec<NodeId> = loader.load("bw_inverted_head")?;
        let bw_inv_edge_ids: Vec<EdgeId> = loader.load("bw_inverted_edge_ids")?;
        cache_check(bw_inv_first_out.len() == num_nodes + 1,
            format!("bw_inverted first_out.len ({}) != num_nodes+1 ({})",
                bw_inv_first_out.len(), num_nodes + 1))?;
        cache_check(bw_inv_head.len() == num_bw_edges,
            format!("bw_inverted head.len ({}) != num_bw_edges ({})",
                bw_inv_head.len(), num_bw_edges))?;
        for &h in &bw_inv_head {
            cache_check((h as usize) < num_nodes,
                format!("bw_inverted head value {} >= num_nodes {}", h, num_nodes))?;
        }
        cache_check(bw_inv_edge_ids.len() == num_bw_edges,
            format!("bw_inverted edge_ids.len ({}) != num_bw_edges ({})",
                bw_inv_edge_ids.len(), num_bw_edges))?;
        for &eid in &bw_inv_edge_ids {
            cache_check((eid as usize) < num_bw_edges,
                format!("bw_inverted edge_id {} >= num_bw_edges {}", eid, num_bw_edges))?;
        }
        let backward_inverted = ReversedGraphWithEdgeIds::from_raw_validated(
            bw_inv_first_out, bw_inv_head, bw_inv_edge_ids)?;

        // --- Elimination tree (must match num_nodes) ---
        let elim_raw: Vec<u32> = loader.load("elimination_tree")?;
        cache_check(elim_raw.len() == num_nodes,
            format!("elimination_tree.len ({}) != num_nodes ({})",
                elim_raw.len(), num_nodes))?;
        // non-MAX values are parent node IDs → must be < num_nodes
        for &v in &elim_raw {
            if v != u32::MAX {
                cache_check((v as usize) < num_nodes,
                    format!("elimination_tree parent {} >= num_nodes {}", v, num_nodes))?;
            }
        }
        let elimination_tree: Vec<InRangeOption<NodeId>> = elim_raw.into_iter()
            .map(|v| if v == u32::MAX { InRangeOption::NONE } else { InRangeOption::some(v) })
            .collect();

        // --- NodeOrder: load ranks vector and validate as a bijection ---
        // save_each() writes "ranks" (node_order.rs:99). We load ranks, validate the
        // permutation, then construct NodeOrder.
        // NodeOrder::from_ranks uses debug_assert! (node_order.rs:58) — silent in release.
        // We must validate manually to guarantee InvalidData on corrupt cache, not wrong routes.
        let raw_ranks: Vec<Rank> = loader.load("ranks")?;
        cache_check(raw_ranks.len() == num_nodes,
            format!("ranks.len ({}) != num_nodes ({})", raw_ranks.len(), num_nodes))?;
        {
            // Validate ranks form a valid permutation [0, num_nodes)
            let mut seen = vec![false; num_nodes];
            for &rank in &raw_ranks {
                cache_check((rank as usize) < num_nodes,
                    format!("ranks contains out-of-range value {}", rank))?;
                cache_check(!seen[rank as usize],
                    format!("ranks contains duplicate value {}", rank))?;
                seen[rank as usize] = true;
            }
        }
        let node_order = NodeOrder::from_ranks(raw_ranks);

        // --- Elimination tree forest validation ---
        // SeparatorTree::new() assumes a valid forest and uses debug_assert!
        // (separator_decomposition.rs:136). new_subtree() (line 149) can infinite-loop
        // on a cycle (A→B→A chain). A cycle also causes unbounded recursion in the
        // Many-children case. We must validate acyclicity BEFORE calling new().
        //
        // Forest invariant: in a valid elimination tree, parent rank > child rank
        // (each node's parent has a higher index). This is guaranteed by CCH construction
        // but not by arbitrary cache files.
        for (node, parent_opt) in elimination_tree.iter().enumerate() {
            if let Some(parent) = parent_opt.value() {
                cache_check((parent as usize) > node,
                    format!("elimination_tree[{}] = {} — parent must have higher rank than child",
                        node, parent))?;
            }
        }
        let separator_tree = SeparatorTree::new(&elimination_tree);

        Ok(DirectedCCH {
            forward_first_out,
            forward_head,
            forward_tail,
            backward_first_out,
            backward_head,
            backward_tail,
            node_order,
            forward_cch_edge_to_orig_arc,
            backward_cch_edge_to_orig_arc,
            elimination_tree,
            forward_inverted,
            backward_inverted,
            separator_tree,
        })
    }
}
```

### Step 3: CCH Cache Module (CCH-Hanoi)

**New file:** `CCH-Hanoi/crates/hanoi-core/src/cch_cache.rs`

Responsibilities:

1. Compute checksum of source files (first_out, head, cch_perm)
2. On first run: build CCH → serialize via Deconstruct → write checksum + header
3. On subsequent runs: verify header + checksum → call
   `DirectedCCHReconstructor`

#### Cache file header

Every cache directory writes a `cache_meta` file:

```json
{
  "version": 1,
  "endianness": "little",
  "pointer_width": 8,
  "source_checksum": "<sha256 hex>",
  "created_utc": "2026-04-08T12:00:00Z"
}
```

On load, the loader checks `version`, `endianness`, and `pointer_width` match
the current process. If any mismatch → cache invalid → rebuild.

This protects against:

- Loading cache built by a different code version (schema drift)
- Loading cache built on a different architecture (endianness, pointer width)
- Deploying cache files across machines without explicit version agreement

```rust
const CACHE_VERSION: u32 = 1;

pub struct CchCache {
    cache_dir: PathBuf,  // <graph_dir>/cch_cache/
}

impl CchCache {
    /// Create cache handle. `graph_dir` is the directory containing the graph
    /// files (e.g. `Maps/data/hanoi_motorcycle/line_graph/`). Cache dir is
    /// `<graph_dir>/cch_cache/` — inherently profile- and mode-scoped.
    pub fn new(graph_dir: &Path) -> Self { ... }

    /// SHA-256 over first_out + head + cch_perm file contents.
    /// Returns true if cache exists, header matches, and checksum matches.
    pub fn is_valid(&self, source_files: &[&Path]) -> bool { ... }

    /// Serialize via `DirectedCCH::deconstruct_to()` + write header/checksum.
    pub fn save(&self, cch: &DirectedCCH, source_files: &[&Path]) -> io::Result<()> {
        cch.deconstruct_to(&self.cache_dir)?;
        self.write_meta(source_files)?;
        Ok(())
    }

    /// Load via `DirectedCCHReconstructor` (lives in rust_road_router).
    /// Reconstruction validates all CSR invariants internally.
    /// Uses `reconstruct_from()` — `Loader` has no public constructor.
    pub fn load(&self) -> io::Result<DirectedCCH> {
        DirectedCCHReconstructor.reconstruct_from(&self.cache_dir)
    }
}
```

> **Key design:** CCH-Hanoi does NOT construct `DirectedCCH` directly — it calls
> `DirectedCCHReconstructor` which lives inside rust_road_router and has access
> to private fields. This respects crate boundaries.

### Step 4: Integrate Cache into Load Path (CCH-Hanoi)

**File:** `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs`

Modify `LineGraphCchContext::load_and_build()`:

```rust
// Before: always builds from scratch
let cch = CCH::fix_order_and_build(&borrowed, order);
let directed_cch = cch.to_directed_cch();

// After: try cache first, fall back to rebuild on any error
let cache = CchCache::new(line_graph_dir);
let source_files = &[
    line_graph_dir.join("first_out"),
    line_graph_dir.join("head"),
    perm_path.to_path_buf(),
];
let source_refs: Vec<&Path> = source_files.iter().map(|p| p.as_path()).collect();

let directed_cch = 'build: {
    if cache.is_valid(&source_refs) {
        match cache.load() {
            Ok(loaded) => {
                // Post-load semantic check: edge mapping values must be valid indices
                // into the source graph. DirectedCCHReconstructor cannot check this
                // because it doesn't know the source graph's edge count.
                let num_metric_edges = graph.num_edges();
                if let Err(e) = validate_edge_mappings(&loaded, num_metric_edges) {
                    tracing::warn!("cached DirectedCCH edge mappings invalid: {e}; rebuilding");
                    break 'build None;
                }
                tracing::info!("loaded DirectedCCH from cache");
                break 'build Some(loaded);
            }
            Err(e) => {
                tracing::warn!("cached DirectedCCH failed validation: {e}; rebuilding");
            }
        }
    }
    None
}.unwrap_or_else(|| {
    tracing::info!("building DirectedCCH from scratch");
    let cch = CCH::fix_order_and_build(&borrowed, order);
    let directed_cch = cch.to_directed_cch();
    if let Err(e) = cache.save(&directed_cch, &source_refs) {
        tracing::warn!("failed to write DirectedCCH cache: {e}");
    }
    directed_cch
});
```

The `validate_edge_mappings` function lives in `hanoi-core` (not
rust_road_router) because it requires knowledge of the source graph's edge count
— information the `DirectedCCHReconstructor` inside rust_road_router does not
have.

> **Import note:** `line_graph.rs` currently imports `DirectedCCH` and `CCH` but
> not `CCHT` or `EdgeIdT`. Implementation must add:
>
> ```rust
> use rust_road_router::algo::customizable_contraction_hierarchy::CCHT;
> use rust_road_router::datastr::graph::EdgeIdT;
> ```

```rust
/// Validate that all edge IDs in the CCH→original-arc mappings are valid
/// indices into the source graph. Called after cache load, before customization.
fn validate_edge_mappings(cch: &DirectedCCH, num_metric_edges: usize) -> std::io::Result<()> {
    let check = |cond: bool, msg: String| -> std::io::Result<()> {
        if cond { Ok(()) } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
        }
    };
    for (i, arcs) in cch.forward_cch_edge_to_orig_arc().iter().enumerate() {
        for &EdgeIdT(arc) in arcs {
            check((arc as usize) < num_metric_edges,
                format!("fw edge_to_orig[{}] contains arc {} >= num_metric_edges {}",
                    i, arc, num_metric_edges))?;
        }
    }
    for (i, arcs) in cch.backward_cch_edge_to_orig_arc().iter().enumerate() {
        for &EdgeIdT(arc) in arcs {
            check((arc as usize) < num_metric_edges,
                format!("bw edge_to_orig[{}] contains arc {} >= num_metric_edges {}",
                    i, arc, num_metric_edges))?;
        }
    }
    Ok(())
}
```

> **Why not in the reconstructor?** The `ReconstructPrepared` trait signature is
> `reconstruct_with(self, loader: Loader) -> io::Result<DirectedCCH>` — it
> cannot accept extra parameters. The metric graph (the graph used to build the
> DirectedCCH) is only known at the hanoi-core layer where both the graph and
> the CCH are loaded. This is a two-layer validation design: structural
> invariants (CSR shape, ranges, permutations) are checked inside
> rust_road_router during reconstruction; semantic invariants (edge ID validity
> against the actual metric graph) are checked in hanoi-core after
> reconstruction.

**Normal-graph caching (`CchContext` in `cch.rs`):** The undirected `CCH`
already has `Deconstruct` + `CCHReconstrctor` in `mod.rs` (lines 43–60).
Normal-graph caching follows the same cache-or-build pattern, but uses the
existing `CCHReconstrctor` for loading. The `CchCache` module handles both modes
by dispatching to the appropriate reconstructor based on whether the graph dir
contains a line graph (DirectedCCH) or normal graph (CCH).

> **KNOWN GAP:** The existing `CCHReconstrctor` (mod.rs:53–60) does NOT have the
> same hardening as `DirectedCCHReconstructor`:
>
> - `NodeOrder::reconstruct_with` calls `from_ranks()` which uses
>   `debug_assert!` (silent in release on corrupt ranks)
> - `UnweightedOwnedGraph::new()` uses `assert!`/`assert_eq!` (panics instead of
>   returning `io::Error`)
> - No value-range checks on head values
> - No first_out monotonicity check
>
> **Decision:** The strong corruption-handling guarantees (io::Error on corrupt
> cache → rebuild) apply ONLY to DirectedCCH caches in Steps 1–4. Hardening
> `CCHReconstrctor` requires modifying `FirstOutGraph::new()` and
> `NodeOrder::reconstruct_with()` in rust_road_router — additional engine
> changes beyond the approved scope. If normal-graph caching is required with
> the same guarantees, it must be a separate amendment adding
> `from_raw_validated` for `FirstOutGraph` and a validated `NodeOrder` loader.
> For now, normal-graph cache corruption will panic (visible, but not graceful).

### Step 5 (Future): Mmap Backend

**Deferred — requires more invasive changes.**

Once Steps 1–4 are complete, the CCH data is on disk. A future step can replace
`Vec::load_from()` with `memmap2::Mmap` to avoid copying into heap.

This requires either:

- (a) Making CCH/DirectedCCH generic over storage (`Vec<T>` vs `&[T]`), or
- (b) Using `MmapVec<T>` wrapper that implements `Deref<Target=[T]>` and can be
  used wherever `Vec<T>` is currently used.

Option (b) is less invasive. The wrapper:

```rust
pub enum Storage<T> {
    Owned(Vec<T>),
    Mapped { mmap: Mmap, _phantom: PhantomData<T> },
}
impl<T> Deref for Storage<T> {
    type Target = [T];
    fn deref(&self) -> &[T] { ... }
}
```

CCH fields would change from `Vec<T>` to `Storage<T>`. Since all CCHT methods
return `&[T]`, this is transparent to consumers.

#### Scope clarification for 400–600 MB target

The savings table (§2) lists base graph vectors (~63 MB) as mmap-backed. To
reach the 400–600 MB target, Step 5 must cover **all** large Vec owners, not
just CCH/DirectedCCH fields:

| Owner                  | Vectors                                                                             | mmap-able directly?                                                                          | Est. RAM     | Location                   |
| ---------------------- | ----------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | ------------ | -------------------------- |
| DirectedCCH fields     | topology, mappings, inverted, elim tree                                             | Yes (cached flat files)                                                                      | ~900–1500 MB | rust_road_router           |
| GraphData              | first_out, head, travel_time, lat, lng                                              | Yes (raw RoutingKit files)                                                                   | ~63 MB       | hanoi-core `graph.rs`      |
| LineGraphCchContext    | original_first_out, original_head, original_travel_time, original_lat, original_lng | Yes (raw RoutingKit files)                                                                   | ~40–60 MB    | hanoi-core `line_graph.rs` |
| LineGraphCchContext    | original_tail                                                                       | **No — reconstructed** from first_out CSR loop (`line_graph.rs:93-98`)                       | ~10–15 MB    | hanoi-core `line_graph.rs` |
| LineGraphCchContext    | original_arc_id_of_lg_node                                                          | **No — synthesized** as `0..num_edges` then extended with split_map (`line_graph.rs:86-125`) | ~5–10 MB     | hanoi-core `line_graph.rs` |
| baseline_weights       | Vec\<Weight\>                                                                       | Yes (share travel_time mmap)                                                                 | ~17 MB       | hanoi-core                 |
| NodeOrder (Arc\<[T]\>) | ranks, node_order                                                                   | Yes (cached flat file)                                                                       | ~15 MB       | rust_road_router           |

`GraphData` and the directly-loaded `LineGraphCchContext.original_*` arrays can
use `Storage<T>` with no serialization step. However, `original_tail` and
`original_arc_id_of_lg_node` are **derived at load time** — `original_tail` is
reconstructed via a CSR expansion loop, and `original_arc_id_of_lg_node` is
synthesized as `0..num_edges` then extended per split node. These two cannot be
directly mmap'd from existing files. Options:

1. **Cache them as flat files** alongside the DirectedCCH cache — serialize
   after `load_and_build`, load as `Storage<T>` on subsequent runs. Invalidation
   follows the same checksum as the DirectedCCH cache.
2. **Recompute at startup** and accept their ~15–25 MB heap cost — they are
   small relative to the DirectedCCH savings and the reconstruction is fast.

**Decision: Option 2 — recompute at startup.** These two arrays total ~15–25 MB,
which is negligible against the ~1–1.5 GB saved by mmap'ing DirectedCCH. The
reconstruction is O(num_edges) with no I/O — under 50 ms. Caching them would add
two more files to the invalidation surface for minimal gain. `baseline_weights`
is a clone of `travel_time` and can share the same mmap. `NodeOrder` is
persisted as a flat file and loaded into the cache dir alongside DirectedCCH.

Without covering GraphData + LineGraphCchContext, steady-state RAM would be
~480–680 MB instead of 400–600 MB.

---

## 5. Potential Problems & Verification Needed

### P1: `InRangeOption<NodeId>` and `EdgeIdT` Binary Layout — NOT SOUND

~~Previous claim: "compiler guarantees same layout as T" for single-field tuple
structs.~~

**Correction:** Neither `InRangeOption<T>` nor `EdgeIdT` has
`#[repr(transparent)]`. Rust's default repr makes **no** layout guarantees for
structs — even single-field wrappers. Writing `Vec<InRangeOption<u32>>` as raw
bytes and reading it back as `Vec<u32>` (or vice versa) is technically UB.

**In practice** rustc does lay out single-field structs identically to the inner
type today, but this is an implementation detail, not a guarantee.

**Resolution:** The Deconstruct/Reconstruct path must serialize through the
inner `u32` value, not through raw byte reinterpretation of the wrapper types.
For `elimination_tree: Vec<InRangeOption<NodeId>>`, serialize as `Vec<u32>` by
extracting `.value()` (or raw inner for NONE = `u32::MAX`). For `Vecs<EdgeIdT>`,
serialize the `.data` slice by mapping `EdgeIdT(x) → x`. On load, reconstruct
wrappers from `u32` values. This adds trivial O(n) map passes but makes the
contract explicit and repr-independent.

### P2: `SeparatorTree` Cannot Be Serialized Flat

Recursive tree with `Vec<SeparatorTree>` children. **Plan: don't serialize it.**
Rebuild from `elimination_tree` in ~5 ms. The elimination tree IS serialized.

After `validate_for_parallelization()`, all `SeparatorNodes` are
`Consecutive(Range)` — so the Random(Vec) variant is never used in practice for
valid CCH orders. This means SeparatorTree memory is small (just Ranges + tree
pointers). Not worth mmap'ing.

### P3: `to_directed_cch()` Clones `elimination_tree` and `separator_tree`

Lines 211–217 of mod.rs. When loading from cache we skip `to_directed_cch()`
entirely, so this clone doesn't happen. The DirectedCCH is reconstructed
directly. **No issue.**

### P4: `Vecs<T>` Uses `usize` for `first_idx` — RESOLVED (REVISED)

**Original decision:** Convert `first_idx` in-memory from `Vec<usize>` to
`Vec<u32>`.

**Revised decision:** Keep `first_idx` as `Vec<usize>` internally. Convert
on-the-fly during serialization (`first_idx_as_u32()`) and deserialization
(`from_raw()` takes `Vec<u32>`, converts to `Vec<usize>`). This avoids touching
all `Vecs<T>` call sites while still writing portable u32 to disk.

Cross-platform safety is preserved: disk format is always `Vec<u32>`. The
`cache_meta` header additionally records `pointer_width` to catch any mismatch.

### P5: `NodeOrder` Uses `Arc<[T]>` Internally

`from_ranks(ranks: Vec<Rank>)` converts to `Arc<[NodeId]>`. When loading from
cache, we load `ranks` as `Vec<u32>` then call `from_ranks()` which allocates
both `node_order` and `ranks` as Arc.

With mmap (Step 5), we'd need `from_ranks_borrowed(ranks: &[Rank])` that stores
references instead of Arc. This is a Step 5 concern, not Steps 1–4.

### P6: Checksum Invalidation — RESOLVED (CORRECTED)

**Decision:** Use SHA-256 content hash of source files.

Checksum covers:

- `first_out` file content hash
- `head` file content hash
- `cch_perm` file content hash

~~Previously included `travel_time` with rationale "affects `to_directed_cch()`
via `always_infinity`".~~

**Correction:** `always_infinity()` calls `prepare_zero_weights()`, which only
checks `!up_arcs.is_empty()` — it never reads travel_time values. The edge→orig
mapping emptiness is determined solely by graph topology (first_out, head) and
CCH ordering (cch_perm). `travel_time` does NOT affect DirectedCCH construction.
Removed from checksum to avoid spurious cache invalidation when only weights
change (e.g. traffic overlay updates the travel_time file but topology is
unchanged).

Combined into a single SHA-256 digest stored in `cch_cache/cache_meta`. Content
hash is slower than mtime but bulletproof — cache is validated once at startup,
so the ~100 ms hash time is negligible against the ~30–60 s build it replaces.

### P7: Cache Disk Usage

For motorcycle line graph, cache would add:

- 6 directed topology files: ~6 × 17 MB avg ≈ 100 MB
- 4 Vecs files: ~100–200 MB (depends on edge→orig mapping density)
- 6 inverted graph files: ~6 × 17 MB avg ≈ 100 MB
- Elim tree + ranks: ~15 MB + ~8 MB

**Total: ~350–450 MB additional disk per profile.**

### P8: `customize_directed()` Access Pattern — VERIFIED SAFE (CORRECTED)

~~Previous claim: uses direct field access `cch.forward_head.len()`.~~

**Correction:** `customization/directed.rs:5` actually uses the CCHT trait
method: `cch.forward_head().len()`. The `.len()` call goes through `&[NodeId]`
returned by the trait, not the private field directly.

`customize_directed.rs:55` does use `cch.backward_inverted.link_iter()` — this
IS a direct field access on `backward_inverted`. However, this is within the
same crate (`rust_road_router`), so private field access is valid.

**No issue** for Steps 1–4. For Step 5 (`Storage<T>`), the inverted graph fields
must still impl `LinkIterable` — which works because
`Storage<T>: Deref<Target=[T]>` makes it transparent.

### P9: First Build Peak RAM — RESOLVED

The contraction process creates an undirected CCH, then `to_directed_cch()`
prunes infinity edges. First run peaks at ~3–4 GB.

**Decision:** Acceptable as one-time cost. Add explicit cleanup between phases:

```rust
// Build & serialize, then drop intermediate structures
let directed_cch = {
    let cch = CCH::fix_order_and_build(&borrowed, order);
    let directed = cch.to_directed_cch();
    if let Err(e) = cache.save(&directed, &source_refs) {
        tracing::warn!("failed to write DirectedCCH cache: {e}");
    }
    // `cch` dropped here — frees ~1 GB of undirected CCH structures
    directed
};
```

The undirected `CCH` (~1 GB) is dropped immediately after `to_directed_cch()`
produces the directed variant, and before customization allocates its own
buffers. This reduces first-run peak from ~3–4 GB to ~2.5–3 GB.

For production: build cache on a build machine (or the server itself with swap),
then deploy only the cache files. Production servers never run Phase 1.

### P10: `Vecs<T>` `par_iter()` Requires `T: Sync`

`Vecs::par_iter()` is used during `prepare_weights` (customization.rs:76). If
`data` becomes mmap-backed `&[T]`, `T: Sync` is already satisfied since all
element types are `u32`. **No issue.**

---

## 6. Implementation Order

```
Step 1: Add accessors to Vecs<T>, ReversedGraphWithEdgeIds     [rust_road_router] *
         ↓
Step 2: Add Deconstruct + ReconstructPrepared for DirectedCCH  [rust_road_router] *
         ↓
Step 3: CchCache module                                        [CCH-Hanoi]
         ↓
Step 4: Integrate cache into load paths + explicit drop        [CCH-Hanoi]
         ↓
Step 5: (future) Storage<T> wrapper + mmap backend             [both]
```

> \* **rust_road_router exception approved.** Steps 1–2 add only accessors
> (read-only getters), validated constructors (`from_raw`,
> `from_raw_validated`), and `Deconstruct` / `ReconstructPrepared` trait impls.
> Zero algorithm changes.

Steps 1–2 are minimal accessor + trait additions in rust_road_router. No
algorithm logic changes, no field type changes (`Vecs.first_idx` stays
`Vec<usize>`). Steps 3–4 are CCH-Hanoi only.

**Steps 1–4 reduce startup time** by caching CCH structures to disk and skipping
re-contraction on subsequent runs. Step 4 also adds explicit drop of the
undirected CCH to reduce first-run peak RAM.

**Step 5 is the only step that reduces steady-state RAM** — replaces heap
allocations with file-backed mmap pages.

---

## 7. Risk Assessment

| Risk                                                            | Severity | Mitigation                                                                        | Status         |
| --------------------------------------------------------------- | -------- | --------------------------------------------------------------------------------- | -------------- |
| Cache corruption → bad routes                                   | High     | SHA-256 content hash + full cross-structure validation                            | Resolved (P6)  |
| Corrupt cache aborts startup instead of rebuilding              | High     | Step 4 catches io::Error from cache.load(), falls back to rebuild                 | Resolved (R5)  |
| Edge mapping IDs out-of-range → panic in metric.link()          | High     | Post-load validate_edge_mappings() in hanoi-core checks < num_metric_edges        | Resolved (R5)  |
| Silent zip truncation on size mismatch                          | High     | Reconstructor checks mapping/inverted/tree/order vs topology                      | Resolved (R2)  |
| Out-of-range node/edge IDs → UB via get_unchecked_mut           | High     | Value-range checks: head/tail < num_nodes, inv edge_ids < num_edges               | Resolved (R3)  |
| Non-monotonic first_out → panic at SlcsIdx::range()             | High     | Monotonicity check on all 4 first_out arrays + inverted first_outs                | Resolved (R4)  |
| Cyclic elimination tree → infinite loop in SeparatorTree::new() | High     | Pre-construction check: parent rank > child rank for all entries                  | Resolved (R4)  |
| NodeOrder serialize/load file name mismatch                     | High     | Unified on "ranks" — matches save_each() output and from_ranks() load             | Resolved (R4)  |
| NodeOrder permutation not validated in release                  | Medium   | Manual bijection check with seen[] before from_ranks()                            | Resolved (R3)  |
| Normal-graph CCHReconstrctor uses assert!                       | Medium   | Documented as known gap; DirectedCCH guarantees only; requires separate amendment | Acknowledged   |
| assert! crash on corrupt DirectedCCH cache                      | Medium   | All validation uses io::ErrorKind::InvalidData, not panic                         | Resolved (R2)  |
| Platform/ABI mismatch on cache load                             | High     | `cache_meta` header with version, endianness, pointer_width                       | Resolved (R1)  |
| Wrapper type repr not guaranteed                                | Medium   | Serialize through inner u32, not raw byte reinterpret                             | Resolved (P1)  |
| Crate boundary: DirectedCCH private fields                      | High     | Deconstruct + ReconstructPrepared inside rust_road_router                         | Resolved (R1)  |
| Platform-dependent serialization (usize)                        | Medium   | Serialize on-the-fly as u32; keep usize internally                                | Resolved (P4)  |
| First-run peak RAM                                              | Low      | Explicit drop of undirected CCH before customization; one-time cost               | Resolved (P9)  |
| Step 5 scope gap (GraphData, LG context)                        | Medium   | Explicit scope table added; derived arrays (original_tail, arc_id) noted          | Clarified (R3) |
| Storage\<T\> → Deref complexity                                 | Medium   | Thorough testing; fallback to Vec                                                 | Step 5 concern |
| mmap page faults → query latency spikes                         | Medium   | `madvise(MADV_WILLNEED)` pre-fault on startup                                     | Step 5 concern |
| SeparatorTree rebuild cost                                      | Low      | ~5 ms from elimination tree                                                       | Accepted       |
| Cache dir conflicts between profiles                            | Medium   | Include profile in cache path (see §3)                                            | Resolved       |
| rust_road_router modification policy                            | High     | Minimal, accessor-only changes; explicit approval granted                         | Resolved       |

---

## 8. Summary

| Metric              | Before   | After (Step 4) | After (Step 5)         |
| ------------------- | -------- | -------------- | ---------------------- |
| Startup time (cold) | ~30–60 s | ~2–5 s         | ~1–2 s                 |
| Startup time (warm) | ~30–60 s | ~2–5 s         | ~0.5 s                 |
| Steady-state RAM    | ~2.2 GB  | ~2.2 GB        | ~400–600 MB            |
| Disk usage          | ~230 MB  | ~600–700 MB    | ~600–700 MB            |
| Query latency (p50) | ~1 ms    | ~1 ms          | ~1 ms                  |
| Query latency (p99) | ~3 ms    | ~3 ms          | ~5–10 ms (page faults) |
