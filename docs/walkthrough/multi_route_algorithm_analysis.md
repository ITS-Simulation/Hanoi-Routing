# Phân tích chi tiết luồng xử lý tìm nhiều đường đi thay thế (Multi-Route)

> Module: `CCH-Hanoi/crates/hanoi-core/src/multi_route.rs`
> Upstream reference: `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs`

## Tổng quan

Module multi-route tái hiện thuật toán CCH query (bidirectional elimination tree walk) nhưng thay vì chỉ giữ **một** meeting node tốt nhất, nó thu thập **tất cả** meeting nodes trong phạm vi stretch factor. Mỗi meeting node tạo ra một đường đi ứng viên, sau đó được reconstruct → unpack → diversity-filter để trả về K đường đi thay thế.

### Pipeline tổng thể

```
multi_query(from, to, max_alternatives, stretch_factor)
    │
    ├── Phase 1: collect_meeting_nodes(from_rank, to_rank)
    │       ├── Forward EliminationTreeWalk (from → root)
    │       ├── Backward EliminationTreeWalk (to → root)
    │       └── Thu thập Vec<(meeting_node_rank, total_distance)>
    │
    ├── Phase 2: Với mỗi meeting node ứng viên
    │       ├── reconstruct_path(from_rank, to_rank, meeting_node)
    │       │       ├── Clone bw_parents
    │       │       ├── Đảo forward parents → backward parents
    │       │       ├── unpack_path() — mở rộng shortcut
    │       │       └── Trace path from → to
    │       │
    │       └── Diversity filter (Jaccard overlap)
    │
    └── Trả về Vec<AlternativeRoute>
```

---

## Phase 1: Tìm Meeting Nodes — `collect_meeting_nodes()`

### 1.1 Cơ chế Elimination Tree Walk

#### Elimination Tree là gì?

Elimination tree là cây bao trùm biểu diễn quan hệ cha–con giữa các node trong CCH ordering. Mỗi node có đúng một parent (trừ root). Walk đi từ node nguồn **ngược lên root** theo elimination tree, dọc đường relax các cạnh upward.

#### Struct `EliminationTreeWalk`

```
File: rust_road_router/engine/.../stepped_elimination_tree.rs

Trường:
- graph        : &Graph         — forward hoặc backward CCH graph
- distances    : &mut [Weight]  — mảng khoảng cách, dùng chung (reuse qua query)
- predecessors : &mut [(NodeId, EdgeId)] — parent pointers (node_cha, edge_id)
- elimination_tree : &[InRangeOption<NodeId>] — mảng parent trong elim. tree
- next         : Option<NodeId> — node tiếp theo sẽ xử lý
- relaxed_edges: usize
```

#### Các method quan trọng

| Method | Hành vi |
|--------|---------|
| `query_with_resetted(graph, elim_tree, dists, parents, start)` | Khởi tạo: `dists[start] = 0`, `next = Some(start)` |
| `peek()` → `Option<NodeId>` | Trả về node tiếp theo **không** advance |
| `next()` / `settle_next_node()` | **(1)** Lấy `node = self.next`; **(2)** `self.next = elim_tree[node].parent`; **(3)** Relax tất cả cạnh outgoing: nếu `dist[node] + weight < dist[head]` → cập nhật `dist[head]`, `parents[head] = (node, edge)` |
| `skip_next()` | Advance `self.next = parent` **KHÔNG relax cạnh** — pruning khi đã chắc chắn khoảng cách tạm thời ≥ tentative_distance tốt nhất |
| `tentative_distance(node)` | Trả về `distances[node]` |
| `reset_distance(node)` | Đặt `distances[node] = INFINITY` — dọn dẹp để tái sử dụng mảng |

### 1.2 Vòng lặp chính: Interleaved bidirectional walk

```
Forward walk: from_rank → ... → root (qua elimination tree parents)
Backward walk: to_rank → ... → root

Cả hai walk đi "lên" theo thứ tự rank tăng dần.
```

#### So sánh: Original query.rs vs multi_route.rs

| Bước | Original (`query.rs`) | Multi-route (`multi_route.rs`) |
|------|----------------------|-------------------------------|
| fw < bw | `fw.next(); fw.reset_distance(fw_node)` | `fw.next()` — **KHÔNG reset** |
| fw > bw | `bw.next(); bw.reset_distance(bw_node)` | `bw.next()` — **KHÔNG reset** |
| fw == bw (meeting) | Relax/skip + check dist → cập nhật **1** meeting node + **reset cả hai** | Relax/skip + push vào `meeting_candidates` + **KHÔNG reset** |
| fw done, bw chưa | `bw.next(); bw.reset_distance()` | `bw.next()` — **KHÔNG reset** |

**Điểm khác biệt then chốt**: Multi-route **KHÔNG gọi `reset_distance()`** ở bất kỳ đâu. Lý do:

1. **Bảo toàn parent pointers**: Reset distance sẽ vô hiệu hóa thông tin khoảng cách/parent cần thiết cho reconstruct path sau này. Trong original query, chỉ cần 1 meeting node nên reset được. Multi-route cần parent pointers cho TẤT CẢ meeting nodes.

2. **Hệ quả**: Mảng `fw_distances` và `bw_distances` sau walk vẫn chứa khoảng cách hợp lệ cho tất cả node trên đường lên root → cho phép reconstruct bất kỳ meeting node nào.

3. **Trade-off**: Mảng distances bị "bẩn" sau mỗi query — `MultiRouteServer::new()` khởi tạo mảng mới mỗi lần, thay vì tái sử dụng như original.

### 1.3 Logic tại meeting node (fw == bw)

```rust
// Cả fw và bw đều ở cùng 1 node
(Some(node), Some(_node)) => {
    // Pruning: chỉ relax nếu khoảng cách tạm thời < best known
    if fw_walk.tentative_distance(node) < tentative_distance {
        fw_walk.next();   // relax edges
    } else {
        fw_walk.skip_next();  // chỉ advance, không relax
    }
    // Tương tự cho backward
    if bw_walk.tentative_distance(node) < tentative_distance {
        bw_walk.next();
    } else {
        bw_walk.skip_next();
    }

    let fw_dist = fw_walk.tentative_distance(node);  // sau khi relax
    let bw_dist = bw_walk.tentative_distance(node);

    if fw_dist < INFINITY && bw_dist < INFINITY {
        let dist = fw_dist + bw_dist;
        meeting_candidates.push((node, dist));
        if dist < tentative_distance {
            tentative_distance = dist;  // cập nhật best
        }
    }
}
```

**Lưu ý quan trọng về pruning**:

- Kiểm tra `tentative_distance(node) < tentative_distance` TRƯỚC KHI gọi `next()`.
- Nếu skip, các cạnh từ node này **KHÔNG được relax** → các node phía trên (rank cao hơn) có thể không nhận được khoảng cách tốt nhất qua con đường này.
- **Pruning ảnh hưởng đến chất lượng candidates**: Khi `tentative_distance` giảm dần (tìm được meeting node tốt hơn), nhiều node sẽ bị skip → meeting nodes phát hiện sau có thể có khoảng cách KHÔNG phải tối ưu qua node đó.
- Đây là pruning **đúng** cho shortest-path (vì đường ngắn nhất vẫn đúng), nhưng **có thể bỏ sót** một số alternative candidates.

### 1.4 Sắp xếp và khử trùng

```rust
meeting_candidates.sort_unstable_by_key(|&(_, dist)| dist);
meeting_candidates.dedup_by_key(|&mut (node, _)| node);
```

- Sort theo distance tăng dần → đường ngắn nhất luôn ở đầu
- Dedup theo node ID → giữ bản ghi khoảng cách thấp nhất (vì đã sort)

**Vấn đề tiềm ẩn**: Dedup chỉ xóa phần tử **liền kề** trùng key. Vì đã sort theo distance trước, nếu cùng 1 node xuất hiện với distance khác nhau → chúng sẽ nằm ở vị trí khác nhau → dedup_by_key HOẠT ĐỘNG ĐÚNG vì sort + dedup ≈ unique-by-key giữ phần tử đầu tiên.

---

## Phase 2: Reconstruct Path — `reconstruct_path()`

### 2.1 Tổng quan

Với mỗi meeting node, cần reconstruct đường đi `from → meeting_node → to` từ parent pointers được ghi lại trong Phase 1.

**Vấn đề cốt lõi**: Parent pointers trong `fw_parents` trỏ **ngược** (con → cha), tức là từ meeting_node về from. Còn `bw_parents` trỏ từ meeting_node về to. Cần **hợp nhất** thành một mảng parent trỏ **thuận** (from → to).

### 2.2 Bước 1: Clone parent arrays

```rust
let mut parents = self.bw_parents.clone();
```

Clone `bw_parents` (không phải `fw_parents`) vì bw_parents đã trỏ đúng hướng cho nửa sau (meeting → to). Forward parents cần đảo chiều.

**Tại sao clone**: Mỗi meeting node candidate cần reconstruct riêng. Nếu modify trực tiếp bw_parents, candidate tiếp theo sẽ bị corrupt. Original query.rs KHÔNG clone vì chỉ có 1 meeting node.

### 2.3 Bước 2: Đảo chiều forward parents

```rust
let mut node = meeting_node;
while node != from {
    let (parent, edge) = self.fw_parents[node as usize];
    // fw_parents: parent → node (trỏ "lên" root)
    // Đảo: parents[parent] = (node, edge) (trỏ "xuống" từ parent tới node)
    parents[parent as usize] = (node, edge);
    node = parent;
}
```

**Trước đảo chiều**:
```
fw_parents[meeting_node] = (A, e1)     // A là cha của meeting trong fw walk
fw_parents[A]            = (B, e2)     // B là cha của A
fw_parents[B]            = (from, e3)  // from là cha của B
```

**Sau đảo chiều** (ghi vào `parents`):
```
parents[from] = (B, e3)       // from → B
parents[B]    = (A, e2)       // B → A
parents[A]    = (meeting, e1) // A → meeting_node
```

Kết hợp với bw_parents (đã có sẵn trong `parents`):
```
parents[meeting_node] = (X, e4) // meeting → X (từ bw_parents gốc)
parents[X]            = (Y, e5) // X → Y
parents[Y]            = (to, e6)
```

→ Chuỗi hoàn chỉnh: `from → B → A → meeting → X → Y → to`

### 2.4 Rủi ro khi đảo chiều (parent pointer collision)

**Trường hợp nguy hiểm**: Nếu một node trên forward path đồng thời nằm trên backward path → đảo chiều sẽ **ghi đè** parent pointer của backward, phá hủy nửa sau.

Ví dụ: Node `A` vừa nằm trên đường `from → ... → meeting` vừa trên đường `meeting → ... → to`.

```
parents[A] đang = (X, e_bw)  // backward: A → X → ... → to
Sau đảo:
parents[A] = (meeting, e_fw) // forward: A → meeting ← WRONG cho backward!
```

→ Khi trace path, đi qua `A` sẽ đi sai hướng.

**Multi-route xử lý vấn đề này** bằng kiểm tra cycle detection ở bước trace cuối:
```rust
if next == current || steps >= max_steps {
    return Vec::new();  // Bỏ candidate này
}
```

→ Candidate bị loại nhưng KHÔNG crash.

### 2.5 Bước 3: Unpack shortcuts — `unpack_path()`

CCH graph chứa **shortcut edges** — cạnh ảo biểu diễn đường đi qua nhiều cạnh gốc. Unpack mở rộng mỗi shortcut thành 2 sub-edges, lặp lại đến khi toàn bộ path chỉ còn cạnh gốc.

#### Thuật toán chi tiết

```rust
fn unpack_path(
    origin: NodeId,    // = to_rank (đích trong backward direction)
    target: NodeId,    // = from_rank (nguồn, nơi trace bắt đầu)
    customized: &C,
    parents: &mut [(NodeId, EdgeId)],
) -> bool {
    let mut current = target;  // Bắt đầu từ from_rank
    while current != origin {  // Đi đến to_rank
        let (pred, edge) = parents[current as usize];

        // Xác định hướng của edge trong CCH hierarchy
        let unpacked = if pred > current {
            // pred có rank cao hơn → đây là cạnh "upward" (outgoing)
            customized.unpack_outgoing(EdgeIdT(edge))
        } else {
            // pred có rank thấp hơn → đây là cạnh "downward" (incoming)
            customized.unpack_incoming(EdgeIdT(edge))
        };

        if let Some((EdgeIdT(down), EdgeIdT(up), NodeIdT(middle))) = unpacked {
            // edge là shortcut: thay bằng 2 cạnh qua middle node
            //
            // Trước: parents[current] = (pred, edge)
            // Sau:   parents[current] = (middle, down)
            //        parents[middle]  = (pred, up)
            //
            // Đường: ... → pred --up--> middle --down--> current → ...
            parents[current as usize] = (middle, down);
            parents[middle as usize] = (pred, up);
            // KHÔNG advance current → tiếp tục unpack (middle, down) nếu nó cũng là shortcut
        } else {
            // edge là cạnh gốc → advance
            current = pred;
        }
    }
    true
}
```

#### Minh họa quá trình unpack

```
Ban đầu:
  parents[from] = (B, shortcut_1)
  parents[B]    = (to, shortcut_2)

Iteration 1: current = from
  parents[from] = (B, shortcut_1) → unpack → Some(down_1, up_1, M1)
  → parents[from] = (M1, down_1)
    parents[M1]   = (B, up_1)
  current vẫn = from (tiếp tục unpack)

Iteration 2: current = from
  parents[from] = (M1, down_1) → unpack → None (cạnh gốc)
  → current = M1

Iteration 3: current = M1
  parents[M1] = (B, up_1) → unpack → None
  → current = B

Iteration 4: current = B
  parents[B] = (to, shortcut_2) → unpack → Some(down_2, up_2, M2)
  → parents[B]  = (M2, down_2)
    parents[M2]  = (to, up_2)

... tiếp tục cho đến khi current == origin (to)

Kết quả: from → M1 → B → M2 → to (tất cả cạnh gốc)
```

#### Phân biệt `origin` và `target` trong unpack_path

**CHÚ Ý**: Naming **ngược** với trực giác:

| Tham số | `query.rs` gốc | `multi_route.rs` | Ý nghĩa thực |
|---------|----------------|-------------------|---------------|
| `origin` | `to` (rank) | `to` (rank) | Điều kiện dừng (endpoint) |
| `target` | `from` (rank) | `from` (rank) | Nơi bắt đầu trace (start) |

Trace đi theo parents: `from → ... → to`, nhưng tham số gọi là `target` cho `from` và `origin` cho `to`.

#### Phát hiện cycle trong unpack

Multi-route thêm `max_steps` guard:

```rust
let max_steps = parents.len() * 2;
let mut steps: usize = 0;
// ...
if steps >= max_steps { return false; }
```

Original query.rs **KHÔNG** có guard này vì chỉ unpack meeting node tối ưu (luôn đúng). Multi-route cần guard vì meeting node không tối ưu có thể có parent pointers bị collision (xem 2.4).

### 2.6 Bước 4: Trace path

```rust
let mut path = vec![from];
let mut current = from;
while current != to {
    let next = parents[current as usize].0;
    if next == current || steps >= max_steps {
        return Vec::new();  // Cycle → bỏ candidate
    }
    path.push(next);
    current = next;
    steps += 1;
}
```

### 2.7 Bước 5: Convert rank → original node ID

```rust
let order = self.customized.cch().node_order();
for node in &mut path {
    *node = order.node(*node);  // rank → original ID
}
```

---

## Phase 3: Diversity Filter

### 3.1 Jaccard Overlap

```rust
fn jaccard_overlap(a: &HashSet<(NodeId, NodeId)>, b: &HashSet<(NodeId, NodeId)>) -> f64 {
    intersection(a, b).count() / union(a, b).count()
}
```

- Mỗi route được biểu diễn bằng tập edge `{(u,v)}` (cặp node liên tiếp)
- Overlap = |A ∩ B| / |A ∪ B|
- Ngưỡng: `OVERLAP_THRESHOLD = 0.85` → reject nếu trùng > 85%

### 3.2 Luồng filter

```
Với mỗi candidate (đã sort theo distance):
  1. Nếu đã đủ max_alternatives → dừng
  2. Nếu distance > stretch_limit → dừng
  3. reconstruct_path → nếu empty (cycle) → skip
  4. Tính Jaccard overlap với TẤT CẢ route đã accept
  5. Nếu bất kỳ overlap > 0.85 → skip (dominated)
  6. Otherwise → accept
```

### 3.3 Tham số điều khiển

| Tham số | Giá trị mặc định | Ý nghĩa |
|---------|-------------------|----------|
| `DEFAULT_STRETCH` | 1.3 | Candidate dài ≤ 130% shortest path |
| `OVERLAP_THRESHOLD` | 0.85 | Reject nếu Jaccard > 85% |
| `EXPLORE_MULTIPLIER` | 100 | Duyệt tối đa `100 × max_alternatives` candidates |

---

## Sơ đồ dữ liệu (Data Flow)

```
                    ┌──────────────────────────────────┐
                    │  Input: from (NodeId), to (NodeId) │
                    └────────────────┬─────────────────┘
                                     │
                              rank conversion
                                     │
                    ┌────────────────▼─────────────────┐
                    │     Bidirectional Elim. Tree Walk  │
                    │                                    │
                    │  fw_distances[n]  bw_distances[n]  │
                    │  fw_parents[n]    bw_parents[n]    │
                    │                                    │
                    │  → meeting_candidates: Vec<(rank, dist)>
                    └────────────────┬─────────────────┘
                                     │
                              sort + dedup
                                     │
               ┌─────────────────────▼──────────────────────┐
               │  For each candidate (within stretch limit): │
               │                                             │
               │  ┌─── clone bw_parents ───┐                 │
               │  │                        │                 │
               │  │  reverse fw_parents    │                 │
               │  │  into cloned parents   │                 │
               │  │                        │                 │
               │  │  unpack_path()         │                 │
               │  │  (expand shortcuts)    │                 │
               │  │                        │                 │
               │  │  trace from→to         │                 │
               │  │                        │                 │
               │  │  rank → original IDs   │                 │
               │  └────────┬───────────────┘                 │
               │           │                                 │
               │    Jaccard diversity check                  │
               │    vs all accepted routes                   │
               │           │                                 │
               │     accept or reject                        │
               └─────────────────────┬──────────────────────┘
                                     │
                    ┌────────────────▼─────────────────┐
                    │  Output: Vec<AlternativeRoute>    │
                    │    .distance (ms)                 │
                    │    .path (original node IDs)      │
                    └──────────────────────────────────┘
```

---

## Các điểm yếu và hướng cải thiện

### 1. Parent pointer collision khi đảo chiều forward path

**Vấn đề**: Khi forward path và backward path chia sẻ node (ngoài meeting node), đảo chiều ghi đè backward parent → reconstruct fail → candidate bị bỏ.

**Ảnh hưởng**: Mất candidates hợp lệ, đặc biệt trên mạng lưới thưa (ít nhánh rẽ).

**Hướng cải thiện**:
- Dùng 2 mảng parent riêng biệt (fw_trace[], bw_trace[]) thay vì merge vào 1
- Hoặc trace fw và bw path riêng, unpack riêng, rồi nối tại meeting node

### 2. Pruning trong walk ảnh hưởng chất lượng candidates

**Vấn đề**: Skip relaxation khi `tentative_distance(node) >= best` → meeting nodes phát hiện sau có thể có distance cao hơn thực (vì edges không được relax đầy đủ). Khoảng cách ghi nhận có thể KHÔNG phải tối ưu cho meeting node đó.

**Ảnh hưởng**: Một số candidate đáng lẽ tốt (distance thấp) bị miss hoặc bị gán distance quá cao → bị loại bởi stretch filter.

**Hướng cải thiện**:
- Bỏ pruning (luôn gọi `next()`, không `skip_next()`) → chính xác hơn nhưng chậm hơn
- Hoặc dùng stretch_limit thay vì tentative_distance cho ngưỡng pruning

### 3. Clone bw_parents cho mỗi candidate

**Vấn đề**: `self.bw_parents.clone()` copy toàn bộ mảng (n phần tử) cho mỗi meeting node → O(n × k) memory và time.

**Hướng cải thiện**:
- Chỉ clone phần liên quan (nodes trên elimination tree path)
- Hoặc dùng undo-log: ghi lại các cell bị modify, restore sau reconstruct

### 4. Jaccard trên edge set KHÔNG phân biệt đường gần nhau

**Vấn đề**: Hai đường đi qua đường song song (cách nhau 50m) có Jaccard overlap = 0 (hoàn toàn khác edge set) nhưng về mặt hình học rất giống nhau → không đa dạng thực sự.

**Hướng cải thiện**:
- Bổ sung spatial diversity: Fréchet distance hoặc Hausdorff distance
- Hoặc dùng "corridor" overlap (buffer geometry quanh path)

### 5. Không kiểm tra tính hợp lệ topology

**Vấn đề**: Path reconstruct từ non-optimal meeting node có thể chứa cycle hoặc đoạn lặp mà cycle detection đơn giản (check `next == current`) không phát hiện hết.

**Hướng cải thiện**:
- Dùng visited set khi trace path
- Validate topology: kiểm tra mỗi cạnh liên tiếp có thực sự tồn tại trong graph gốc

---

## Phụ lục: Variant cho Line Graph (Turn-Expanded)

File `line_graph.rs` sử dụng cùng `MultiRouteServer` nhưng:

1. **Node ID trong CCH = edge ID trong graph gốc** (do line graph mapping)
2. Sau reconstruct, path chứa "original node IDs" nhưng thực chất là **edge indices**
3. `build_answer_from_lg_path()` chuyển edge indices → intersection node IDs:
   - `original_tail[edge_id]` → node gốc tại đầu cạnh
   - `original_head[last_edge]` → node gốc tại cuối cạnh cuối
4. Source edge cost được cộng thêm vào distance (vì CCH distance trên line graph không bao gồm trọng số cạnh nguồn)
5. Coordinate queries trim cạnh đầu và cuối (`lg_path[1..len-1]`)

---

## Phụ lục: Tham chiếu code

| Component | File | Lines |
|-----------|------|-------|
| MultiRouteServer::multi_query | [multi_route.rs](../../CCH-Hanoi/crates/hanoi-core/src/multi_route.rs) | 67–130 |
| collect_meeting_nodes | [multi_route.rs](../../CCH-Hanoi/crates/hanoi-core/src/multi_route.rs) | 133–218 |
| reconstruct_path | [multi_route.rs](../../CCH-Hanoi/crates/hanoi-core/src/multi_route.rs) | 224–277 |
| unpack_path | [multi_route.rs](../../CCH-Hanoi/crates/hanoi-core/src/multi_route.rs) | 282–316 |
| jaccard_overlap | [multi_route.rs](../../CCH-Hanoi/crates/hanoi-core/src/multi_route.rs) | 319–328 |
| Original query.rs Server::distance | [query.rs](../../rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs) | 44–129 |
| Original query.rs Server::path | [query.rs](../../rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs) | 131–155 |
| Original query.rs unpack_path | [query.rs](../../rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs) | 159–176 |
| EliminationTreeWalk | [stepped_elimination_tree.rs](../../rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query/stepped_elimination_tree.rs) | 26–124 |
| CchEngine::multi_query | [cch.rs](../../CCH-Hanoi/crates/hanoi-core/src/cch.rs) | 236–270 |
| LineGraphEngine::multi_query | [line_graph.rs](../../CCH-Hanoi/crates/hanoi-core/src/line_graph.rs) | 447–477 |
