# Điều tra chất lượng tuyến đường thay thế

> **Phạm vi**: Phân tích nguyên nhân gốc rễ của hành vi rẽ bất thường trong
> thuật toán tạo K tuyến đường thay thế — quay đầu (U-turn), kéo dài không cần
> thiết, bỏ qua các tuyến tốt hơn.
>
> **Các module liên quan**:
>
> - `CCH-Hanoi/crates/hanoi-core/src/multi_route.rs` — thuật toán K-alternatives
>   qua via-node
> - `CCH-Hanoi/crates/hanoi-core/src/cch.rs` — wrapper multi-query cho đồ thị
>   thường
> - `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` — multi-query cho line graph
> - `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs` — xây dựng
>   line graph & gán chi phí rẽ
> - `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs`
>   — CCH query gốc (tham chiếu)
> - `rust_road_router/engine/src/datastr/graph.rs:181` — hàm `line_graph()`
>   (công thức trọng số)

---

## Mục lục

1. [Bối cảnh](#1-bối-cảnh)
2. [Cách thuật toán hiện tại hoạt động](#2-cách-thuật-toán-hiện-tại-hoạt-động)
3. [Vấn đề 1 — Nhiễm bẩn con trỏ parent](#3-vấn-đề-1--nhiễm-bẩn-con-trỏ-parent)
4. [Vấn đề 2 — Chi phí rẽ bằng 0 trong Line Graph](#4-vấn-đề-2--chi-phí-rẽ-bằng-0-trong-line-graph)
5. [Vấn đề 3 — Xếp hạng thuần theo thời gian di chuyển](#5-vấn-đề-3--xếp-hạng-thuần-theo-thời-gian-di-chuyển)
6. [Ba vấn đề kết hợp như thế nào](#6-ba-vấn-đề-kết-hợp-như-thế-nào)
7. [Đề xuất sửa chữa](#7-đề-xuất-sửa-chữa)
8. [Ma trận ưu tiên](#8-ma-trận-ưu-tiên)

---

## 1. Bối cảnh

Tính năng tuyến đường thay thế sử dụng phương pháp **via-node** trên nền CCH.
Trong CCH query chuẩn, quá trình duyệt hai chiều theo elimination tree gặp nhau
tại một meeting node tối ưu duy nhất. Phần mở rộng multi-route thu thập *tất
cả* meeting nodes trong phạm vi stretch factor, rồi dùng mỗi meeting node để
tái tạo một đường đi khác nhau, cho ra K ứng viên thay thế.

Triệu chứng quan sát được:

- Tuyến đường có U-turn không cần thiết (đi thẳng rồi quay đầu rồi rẽ sang
  đường khác).
- Đường vòng dài bất thường khi có tuyến ngắn hơn, tự nhiên hơn.
- Tuyến thay thế bám sát tuyến tối ưu nhưng có những lệch hướng kỳ lạ tại
  các ngã tư.

Hai giả thuyết ban đầu:

1. Quá trình unpack shortcut CCH gây ra việc một số đoạn đường được mở rộng
   không tối ưu.
2. Trọng số mặc định (travel time) gây ra hành vi rẽ bất thường.

**Kết luận: Cả hai giả thuyết đều được xác nhận**, cùng với một yếu tố thứ ba.

---

## 2. Cách thuật toán hiện tại hoạt động

### Pha 1 — Thu thập Meeting Nodes

`multi_route.rs:collect_meeting_nodes()` (dòng 148–222) chạy quá trình duyệt
hai chiều theo elimination tree giống CCH query chuẩn, nhưng có hai khác biệt
quan trọng:

| Khía cạnh | Query chuẩn (`query.rs`) | Multi-route (`multi_route.rs`) |
|-----------|--------------------------|-------------------------------|
| Reset khoảng cách sau settle | **Có** — `reset_distance(node)` | **Không** — giữ lại để tái tạo đường |
| Cắt tỉa tại meeting nodes | **Có** — `skip_next()` khi distance >= tốt nhất | **Không** — luôn `next()` để tìm meeting nodes phụ |
| Theo dõi meeting node | Một node tốt nhất duy nhất | Tất cả trong phạm vi stretch |
| Sử dụng con trỏ parent | Tái tạo một đường duy nhất | Tái tạo nhiều đường từ mảng chung |

### Pha 2 — Tái tạo & Mở rộng (Unpack)

Với mỗi meeting node, `reconstruct_path()` (dòng 230–293) truy vết `fw_parents`
ngược từ meeting node về nguồn, và `bw_parents` xuôi từ meeting node đến đích.
Mỗi cạnh shortcut CCH được đệ quy mở rộng qua `unpack_edge_recursive()` (dòng
305–324).

### Pha 3 — Lọc

- **Lọc stretch**: loại bỏ ứng viên > `1.3×` khoảng cách tối ưu.
- **Lọc đa dạng**: hệ số Jaccard trùng lặp tập cạnh > `0.85` → bị loại vì
  quá giống nhau.
- **Lọc địa lý** (do caller thực hiện): loại bỏ nếu khoảng cách địa lý > `2.0×`
  khoảng cách địa lý của tuyến ngắn nhất.

---

## 3. Vấn đề 1 — Nhiễm bẩn con trỏ parent

**Mức nghiêm trọng: Nghiêm trọng** — Đây là nguyên nhân chính gây ra các
đường đi bất thường.

### Cơ chế

Trong CCH query chuẩn (`query.rs:72–103`), quá trình duyệt elimination tree
**reset khoảng cách node về INFINITY** sau khi xử lý xong mỗi node:

```rust
// query.rs — query chuẩn
fw_walk.next();
fw_walk.reset_distance(fw_node);  // ← RESET
```

Bước dọn dẹp này rất quan trọng. Quá trình duyệt elimination tree đi từ lá
lên gốc, và `settle_next_node()` relax các cạnh đến các node có rank cao hơn.
Nếu không reset, các giá trị khoảng cách cũ từ các node trước đó **tồn tại**
và can thiệp vào các lần relax sau.

Trong `multi_route.rs:172–213`, khoảng cách cố tình **không được reset**:

```rust
// multi_route.rs — multi-route walk
fw_walk.next();
// Không reset — có chủ đích, xem comment:
// "Do NOT reset: we need parent pointers for path reconstruction."
```

### Tại sao điều này gây ra đường đi sai

Quá trình duyệt elimination tree xử lý node từ rank thấp đến cao (lá → gốc).
Khi `settle_next_node()` được gọi cho node `A`, nó relax các cạnh đến các
neighbor có rank cao hơn:

```rust
// stepped_elimination_tree.rs:86–94
let next_dist = distance + weight;
if next_dist < self.distances[head as usize] {
    self.distances[head as usize] = next_dist;
    self.predecessors.store(head as usize, node, edge_idx);  // ghi đè parent
}
```

Khi không reset khoảng cách, node `X` có thể nhận con trỏ parent từ node `A`
sớm trong quá trình duyệt. Khi node `B` (rank cao hơn, subtree khác) được xử
lý sau, nó cố relax qua `X`, nhưng điều kiện `next_dist < distances[X]` **thất
bại** vì khoảng cách cũ từ `A` vẫn còn đó. Con trỏ parent của `X` vẫn trỏ
đến `A` dù parent đúng cho một đường đi qua meeting node khác phải là `B`.

**Kết quả**: Tái tạo đường đi qua meeting node `M2` (không phải `M1` tối ưu)
sẽ theo các con trỏ parent thuộc về **hỗn hợp subtree từ nhiều meeting node
khác nhau**. Đường đi kết quả là một Frankenstein — các đoạn tối ưu cho `M1`
ghép nối với các đoạn từ `M2`. Điều này tạo ra đường đi uốn lượn, quay lại,
và rẽ không cần thiết.

### Tác động thêm: Thiếu cắt tỉa

Query chuẩn dùng `skip_next()` tại meeting nodes khi khoảng cách tạm thời
đã >= khoảng cách tốt nhất — tránh relax các cạnh không thể cải thiện kết quả.
Multi-route walk **luôn gọi `next()`** (relax cạnh tại mọi meeting node), ghi
đè con trỏ parent ở các node rank cao với giá trị từ meeting nodes không tối
ưu, làm trầm trọng thêm tình trạng nhiễm bẩn.

---

## 4. Vấn đề 2 — Chi phí rẽ bằng 0 trong Line Graph

**Mức nghiêm trọng: Cao** — Trực tiếp cho phép lạm dụng U-turn.

### Mã nguồn

Trong `generate_line_graph.rs:217–232`:

```rust
let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
    // ... kiểm tra rẽ cấm (return None nếu cấm) ...

    if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
        return Some(0); // Phạt U-turn: 0 ms (MIỄN PHÍ)
    }
    Some(0) // Tất cả các rẽ khác: cũng 0 ms (MIỄN PHÍ)
});
```

### Cách trọng số Line Graph hoạt động

Hàm `line_graph()` trong `rust_road_router/engine/src/datastr/graph.rs:194`
tính trọng số mỗi cạnh line-graph:

```rust
weight.push(next_link.weight + turn_cost);
//          ^^^^^^^^^^^^^^^^   ^^^^^^^^^
//          travel_time của    chi phí rẽ từ callback
//          cạnh ĐÍCH          (luôn = 0 hiện tại)
```

Vậy mỗi trọng số cạnh line-graph = `travel_time[cạnh_đích] + 0`. Chi phí rẽ
là **đồng nhất bằng không** cho mọi lượt rẽ không bị cấm, kể cả U-turn.

### Tại sao đây là vấn đề

- **U-turn miễn phí**: Thuật toán không thấy khác biệt giữa đi thẳng qua ngã
  tư và quay đầu. Một tuyến `A → B → A → C` tốn bằng `A → C` (cộng thêm hai
  cạnh), khiến các tuyến thay thế có U-turn gần như cạnh tranh về chi phí.

- **Không phân biệt theo góc rẽ**: Mọi lượt rẽ (rẽ nhẹ trái, rẽ gấp phải,
  U-turn) có chi phí bằng không giống hệt nhau, nên thuật toán không có động
  lực ưu tiên tuyến đi tự nhiên hơn tuyến có thay đổi hướng đột ngột.

- **Tương tác với Vấn đề 1**: Khi con trỏ parent bị nhiễm bẩn và đường đi
  kết quả có U-turn, không có penalty trọng số nào làm đường đi đó tệ đi rõ
  ràng trong bảng xếp hạng, nên nó vượt qua bộ lọc stretch.

---

## 5. Vấn đề 3 — Xếp hạng thuần theo thời gian di chuyển

**Mức nghiêm trọng: Trung bình** — Góp phần chọn tuyến thay thế kém chất lượng.

### Mã nguồn

Trong `multi_route.rs:216`:

```rust
meeting_candidates.sort_unstable_by_key(|&(_, dist)| dist);
```

Ứng viên được xếp hạng thuần theo khoảng cách thời gian di chuyển CCH. Bộ lọc
khoảng cách địa lý chỉ chạy *sau khi* tái tạo đầy đủ đường đi, ở caller:

```rust
// cch.rs:270–276
if distance_m > base * MAX_GEO_RATIO {
    continue;  // loại bỏ đường vòng
}
```

### Tại sao đây là vấn đề

Hai ứng viên có thời gian di chuyển gần giống nhau nhưng hình dạng địa lý
khác biệt lớn (một trực tiếp, một quay đầu) được coi là tốt như nhau. Bộ lọc
đa dạng Jaccard kiểm tra trùng lặp tập cạnh, nhưng:

- Tuyến quay đầu rồi quay lại chia sẻ nhiều cạnh với tuyến trực tiếp — có thể
  bị loại bởi bộ lọc đa dạng (tốt), **hoặc** có thể dùng các cạnh hơi khác
  do nhiễm bẩn từ Vấn đề 1 và vượt qua (xấu).
- Bộ lọc `MAX_GEO_RATIO = 2.0` rất lỏng — tuyến gấp đôi khoảng cách đường
  chim bay vẫn được chấp nhận, bao phủ hầu hết các đường vòng U-turn.

---

## 6. Ba vấn đề kết hợp như thế nào

Ba vấn đề tạo ra vòng phản hồi:

```
Vấn đề 1: Con trỏ parent nhiễm bẩn
    → Đường đi Frankenstein trộn subtree tối ưu và không tối ưu
    → Đường đi uốn lượn và thay đổi hướng không cần thiết

Vấn đề 2: Chi phí rẽ bằng 0
    → U-turn và rẽ gấp không có penalty
    → Đường đi nhiễm bẩn có U-turn vẫn cạnh tranh về chi phí

Vấn đề 3: Xếp hạng thuần theo thời gian di chuyển
    → Không kiểm tra hợp lý địa lý khi chọn ứng viên
    → Tuyến bất thường vượt qua bộ lọc stretch
    → Bộ lọc địa lý (2×) áp dụng quá muộn và quá lỏng
```

Ví dụ tình huống khớp với ảnh chụp quan sát được:

1. Tuyến tối ưu đi thẳng dọc Đường Láng.
2. Multi-route thu thập meeting node `M2` với đường đi dài hơn một chút.
3. Do nhiễm bẩn parent, đường đi qua `M2` sai lầm bao gồm đoạn đi xuống phía
   nam Đường Láng, quay đầu tại Cầu Hoa Mục, rồi quay lại phía bắc.
4. Vì U-turn chi phí bằng 0, đường vòng này chỉ thêm thời gian di chuyển
   của các cạnh phụ — dễ dàng nằm trong phạm vi stretch factor 1.3×.
5. Bộ lọc khoảng cách địa lý (2×) không bắt được vì tổng chiều dài địa lý
   vẫn dưới ngưỡng.

---

## 7. Đề xuất sửa chữa

### Sửa 1 — K đường ngắn nhất bằng penalty (Sửa Vấn đề 1)

**Thay thế hoàn toàn phương pháp via-node. Không thay đổi `rust_road_router`.**

Thay vì thu thập meeting nodes từ quá trình duyệt chung với con trỏ parent
nhiễm bẩn, dùng phương pháp phạt lặp:

```
1. Tìm đường tối ưu P₁ bằng CCH query chuẩn.
2. Với k = 2..K:
   a. Tạo vector trọng số phạt: nhân travel_time × 2 cho tất cả cạnh
      trong các đường đã chấp nhận.
   b. Re-customize CCH với trọng số phạt: customize_with(penalty_weights).
   c. Tìm đường ngắn nhất dưới metric phạt.
   d. Lọc đa dạng so với các đường đã chấp nhận (Jaccard).
   e. Nếu đủ đa dạng, chấp nhận làm tuyến thay thế Pₖ.
3. Trả về tất cả đường đã chấp nhận với khoảng cách GỐC (không phạt).
```

**Tại sao phương pháp này hiệu quả**:

- Mỗi query là một CCH computation độc lập với con trỏ parent sạch.
- Không có trạng thái chung, không nhiễm bẩn, không đường Frankenstein.
- CCH customization nhanh (~100–300 ms cho Hà Nội), nên chạy K lần vẫn
  thực tế (tổng < 2 giây cho 5 tuyến thay thế).
- Hệ số phạt đẩy thuật toán tìm kiếm xa các đường đã tìm mà không cấm
  bất kỳ cạnh nào.

**Vị trí triển khai**: Thay thế `MultiRouteServer::multi_query()` trong
`multi_route.rs` và các caller trong `cch.rs` / `line_graph.rs`.

### Sửa 2 — Thêm penalty rẽ (Sửa Vấn đề 2)

**Thay đổi một file trong `generate_line_graph.rs`. Cần chạy lại pipeline
tạo line graph.**

Thay đổi callback chi phí rẽ:

```rust
let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
    // ... kiểm tra rẽ cấm ...

    // Cấm U-turn (hoặc phạt nặng)
    if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
        return None; // CẤM
        // Hoặc: return Some(30_000); // phạt 30 giây
    }

    // Tuỳ chọn: phạt theo góc rẽ cho rẽ gấp
    // let angle = compute_turn_angle(edge1_idx, edge2_idx, &lat, &lng, &tail, &head);
    // Some(angle_penalty(angle))

    Some(0) // đi thẳng và rẽ nhẹ: không phạt
});
```

**Thay đổi tối thiểu** (chỉ cấm U-turn):

```rust
if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
    return None; // trước đó: Some(0)
}
```

**Phiên bản đầy đủ** (phạt theo góc — cơ sở hạ tầng đã có trong
`hanoi-core/src/geometry.rs` với hàm tính hướng rẽ):

| Loại rẽ | Phạm vi góc | Penalty đề xuất |
|---------|-------------|-----------------|
| U-turn | > 160° | Cấm (`None`) hoặc 30 000 ms |
| Rẽ gấp | 120°–160° | 10 000 ms (10 s) |
| Rẽ vừa | 60°–120° | 5 000 ms (5 s) |
| Rẽ nhẹ | 30°–60° | 2 000 ms (2 s) |
| Đi thẳng | < 30° | 0 ms |

### Sửa 3 — Chấm điểm ứng viên kết hợp (Sửa Vấn đề 3)

**Thay đổi trong `cch.rs` và `line_graph.rs` multi-query wrappers.**

Sau khi tái tạo mỗi đường ứng viên và tính khoảng cách địa lý, áp dụng
điểm tổng hợp thay vì dùng thời gian di chuyển thuần để sắp xếp:

```rust
let direct_dist = haversine_m(from_lat, from_lng, to_lat, to_lng);
let detour_ratio = distance_m / direct_dist;
let score = distance_ms as f64 * detour_ratio.sqrt();
```

Sắp xếp ứng viên theo điểm tổng hợp. Tuyến nhanh nhưng uốn lượn về địa lý
được điểm cao hơn (tệ hơn) so với tuyến nhanh tương đương nhưng trực tiếp hơn.

Đồng thời siết `MAX_GEO_RATIO` từ `2.0` xuống `1.5` — tuyến dài hơn 50%
khoảng cách địa lý đã là đường vòng đáng kể trong mạng đô thị.

---

## 8. Ma trận ưu tiên

| Ưu tiên | Sửa chữa | Nỗ lực | Tác động | Thay đổi `rust_road_router` |
|---------|----------|--------|----------|----------------------------|
| **1** | Sửa 1: K-paths bằng penalty | Trung bình | Loại bỏ hoàn toàn đường Frankenstein | Không |
| **2** | Sửa 2: Penalty U-turn / rẽ | Nhỏ | Ngăn chặn lạm dụng U-turn trong mọi query | Không |
| **3** | Sửa 3: Chấm điểm kết hợp | Nhỏ | Cải thiện chất lượng xếp hạng tuyến thay thế | Không |

Cả ba bản sửa đều nằm trong code `CCH-Hanoi` và pipeline
(`generate_line_graph`). Không cần sửa đổi `rust_road_router` hoặc `RoutingKit`.

Sửa 2 đơn giản nhất để triển khai và kiểm thử (thay đổi một callback + chạy
lại pipeline). Sửa 1 có tác động lớn nhất nhưng cần làm lại phần lõi
multi-route. Sửa 3 là tinh chỉnh, tốt nhất áp dụng sau Sửa 1.
