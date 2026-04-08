# K-Alternative Routes — Hướng Dẫn Triển Khai

Cách `alternative.rs` (trong `rust_road_router/engine`) tạo ra K đường đi ngắn
nhất thay thế dựa trên hạ tầng CCH sẵn có, tích hợp vào CCH-Hanoi thông qua
lớp re-export mỏng trong `hanoi-core/src/multi_route.rs`.

---

## 1. Cơ Sở Lý Thuyết

Dựa trên phương pháp đường đi thay thế bằng separator (Bacherle, Bläsius,
Zündorf — ATMOS 2025). Ý tưởng cốt lõi: trong CCH, elimination tree (cây khử)
mã hoá các balanced separator. Với mỗi truy vấn (s, t), các **tổ tiên chung**
của s và t trong elimination tree tạo thành tập hợp ứng viên via-vertex tự nhiên
— và truy vấn CCH chuẩn đã tính sẵn `d(s,v)` và `d(v,t)` cho mọi đỉnh như vậy
mà không tốn thêm chi phí.

Mỗi via-vertex ứng viên tạo ra đường đi `s → v → t`, được tái tạo và kiểm tra
theo các tiêu chí chấp nhận để sinh ra các tuyến thay thế đa dạng, chất lượng
cao.

---

## 2. Lõi Thuật Toán: `AlternativeServer`

**File:** `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query/alternative.rs`

### 2.1 Cấu Trúc Dữ Liệu

```
AlternativeServer<'a, C: Customized>
├── customized: &C           // tham chiếu đến CCH customization (đồ thị thuận/ngược)
├── fw_distances: Vec<Weight> // khoảng cách duyệt thuận (chỉ mục theo rank)
├── bw_distances: Vec<Weight> // khoảng cách duyệt ngược (chỉ mục theo rank)
├── fw_parents: Vec<(NodeId, EdgeId)>  // con trỏ cha hướng thuận để tái tạo đường đi
├── bw_parents: Vec<(NodeId, EdgeId)>  // con trỏ cha hướng ngược
├── ttest_fw_dist: TimestampedVector   // mảng tạm độc lập cho truy vấn khoảng cách T-test
├── ttest_bw_dist: TimestampedVector   // (tránh ghi đè lên dữ liệu duyệt chính)
├── ttest_fw_par, ttest_bw_par         // con trỏ cha của T-test
```

Thiết kế quan trọng: T-test cần truy vấn CCH điểm-tới-điểm giữa các đầu mút
của đoạn đường. Các truy vấn này dùng **mảng tạm riêng biệt**
(`ttest_fw_dist`/`ttest_bw_dist`) để không ghi đè lên
`fw_parents`/`bw_parents` — vốn vẫn cần cho việc tái tạo đường đi của các ứng
viên khác.

`TimestampedVector` cung cấp khả năng reset O(1) phân bổ giữa các truy vấn
T-test liên tiếp — chỉ cần tăng timestamp là "xoá" logic toàn bộ vector mà
không cần chạm vào bộ nhớ.

### 2.2 Giai Đoạn 1 — Duyệt Elimination Tree Hai Chiều

**Phương thức:** `collect_meeting_nodes(from_rank, to_rank)`

Tái hiện thuật toán duyệt elimination tree của `rust_road_router` với một khác
biệt then chốt: `query.rs` gốc cắt tỉa qua `skip_next()` tại các meeting node
(vì chỉ cần một meeting node tối ưu duy nhất). `multi_route.rs` **luôn gọi
`next()`** để relax cạnh tại mọi meeting node, đảm bảo khoảng cách truyền đúng
tới các đỉnh tổ tiên — những đỉnh có thể là via-vertex thay thế.

```
Logic duyệt (đơn giản hoá):
  fw_walk bắt đầu tại from_rank, đi lên elimination tree
  bw_walk bắt đầu tại to_rank, đi lên elimination tree

  Mỗi bước, tiến hành bước nào có đỉnh hiện tại nhỏ hơn.
  Khi cả hai gặp nhau tại cùng một đỉnh (meeting node):
    → luôn relax cạnh (gọi next() cả hai)
    → ghi nhận (đỉnh, fw_dist + bw_dist) làm ứng viên
    → cập nhật tentative_distance nếu đây là giá trị tốt nhất mới

  Sau khi duyệt xong:
    → sắp xếp ứng viên theo tổng khoảng cách (tăng dần)
    → loại trùng lặp theo node ID
```

**Đầu ra:** `Vec<(meeting_node_rank, total_distance)>` sắp xếp tăng dần theo
khoảng cách. Phần tử đầu tiên là meeting node của đường đi ngắn nhất.

**Hiệu ứng phụ:** `fw_parents` và `bw_parents` được điền đầy con trỏ cha trong
không gian rank — cần thiết cho Giai đoạn 2.

### 2.3 Giai Đoạn 2 — Tái Tạo Đường Đi Kèm Chi Phí Cạnh

**Phương thức:** `reconstruct_path_with_costs(from_rank, to_rank, meeting_node)`

Khác với `query.rs` gốc đảo ngược con trỏ thuận vào `bw_parents` (phá huỷ dữ
liệu — chỉ dùng được cho một đường duy nhất), `alternative.rs` truy vết nửa
thuận và nửa ngược độc lập bằng quyền truy cập **chỉ đọc** lên mảng cha. Điều
này cho phép tái tạo đường đi cho nhiều meeting node từ cùng một lần duyệt.

```
Nửa thuận:  meeting_node → ... → from  (theo fw_parents, sau đó đảo ngược)
Nửa ngược:  meeting_node → ... → to    (theo bw_parents, thứ tự tự nhiên)
```

**Bổ sung quan trọng**: chi phí cạnh được thu thập trong quá trình unpack qua
`unpack_edge_with_costs`. Mỗi cạnh đồ thị gốc trả về trọng số từ đồ thị
thuận/ngược đã customized. Điều này tạo ra `Vec<Weight>` chi phí từng cạnh đi
kèm đường đi — thiết yếu cho bộ lọc chia sẻ theo chi phí, kiểm tra bounded
stretch, và T-test.

Mỗi cạnh trên đường đi được unpack đệ quy qua `unpack_edge_with_costs`, phản
ánh cấu trúc contraction của CCH:
- Nếu `tail < head` → `customized.unpack_outgoing(edge)` → trả về `(down_edge, up_edge, middle_node)` hoặc None
- Nếu `tail > head` → `customized.unpack_incoming(edge)`
- `None` = cạnh đồ thị gốc, đệ quy kết thúc; push trọng số cạnh

**Đầu ra**: `(Vec<NodeId>, Vec<Weight>)` — đường đi đã unpack trong node ID gốc
cùng chi phí từng cạnh. Bất biến: `path.len() == edge_costs.len() + 1`.

Bước cuối: chuyển tất cả node ID từ không gian rank về ID gốc qua
`order.node(rank)`.

### 2.4 Giai Đoạn 3 — Lọc Tính Chấp Nhận

Mỗi ứng viên được đánh giá bởi `evaluate_candidate()`, chạy năm bước kiểm tra
theo thứ tự. Từ chối nhanh — bước đầu tiên thất bại sẽ dừng ngay.

**Kiểm tra 1 — Phát Hiện Vòng Lặp:**
`has_repeated_nodes(&path)` — loại bỏ đường đi thăm bất kỳ đỉnh nào hai lần
(vòng U-turn do cấu trúc shortcut của CCH).

**Kiểm tra 2 — Độ Giãn Địa Lý:**
Độ giãn được đánh giá trên **khoảng cách địa lý** (mét, bằng Haversine) thay vì
chi phí thời gian di chuyển. Người gọi cung cấp closure `path_geo_len`.

```
geo_stretch_limit = path_geo_len(main_path) × stretch_factor
loại bỏ nếu path_geo_len(candidate) > geo_stretch_limit
```

`DEFAULT_STRETCH = 1.25` (cho phép vòng 25% về địa lý).

**Kiểm tra 3 — Bounded Stretch Tại Điểm Rẽ:**
Tìm vị trí ứng viên bắt đầu rẽ khỏi đường tham chiếu (điểm A) và nối lại
(điểm B), sau đó xác minh đoạn vòng là gần tối ưu:

```
1. find_deviation_points(candidate, reference):
   → đi thuận: tìm chỉ mục đầu tiên mà candidate[i+1] ≠ reference[i+1] → a_pos
   → đi ngược: tìm chỉ mục cuối cùng mà candidate nối lại reference → b_pos
   → tính cost_s_a (chi phí đoạn chung đầu) và cost_b_t (chi phí đoạn chung cuối)

2. detour_cost = total_candidate_cost - cost_s_a - cost_b_t
3. exact_ab = cch_point_distance(A, B)   // đường ngắn nhất chính xác A→B
4. loại bỏ nếu detour_cost > exact_ab × (1 + BOUNDED_STRETCH_EPS)
   với BOUNDED_STRETCH_EPS = 0.4
```

Điều này bắt các tuyến đi vòng không cần thiết giữa điểm rẽ/nối lại dù độ giãn
tổng thể trông chấp nhận được.

**Kiểm tra 4 — Giới Hạn Chia Sẻ (từng cặp, theo chi phí):**
Xây dựng tập cạnh chi phí `{(tail, head) → cost}` cho ứng viên và tính độ
trùng lặp chi phí với **mọi** tuyến đã được chấp nhận:

```
shared_cost = Σ min(candidate_cost[e], accepted_cost[e]) cho các cạnh chung e
loại bỏ nếu shared_cost > SHARING_THRESHOLD × best_distance
với SHARING_THRESHOLD = 0.80
```

Đảm bảo sự đa dạng từng cặp: mỗi tuyến mới phải đóng góp ít nhất 20% chi phí
độc nhất so với mọi tuyến đã chấp nhận. Dùng chi phí theo trọng số (không chỉ
đếm số cạnh) để tránh trường hợp vài cạnh chung đắt tiền bị áp đảo.

**Kiểm tra 5 — Tối Ưu Cục Bộ (T-test):**
Xác minh đoạn đường quanh via-vertex xấp xỉ là đường ngắn nhất. Bắt các tuyến
trông đa dạng tổng thể nhưng chứa vòng U-turn cục bộ.

```
1. Xác định vị trí via-vertex (meeting node) trong đường đi đã unpack
2. Xây prefix sum tích luỹ từ edge_costs
3. Đi ±T từ via-vertex (T = LOCAL_OPT_T_FRACTION × best_distance, fraction = 0.4)
   để tìm đầu mút khoảng v' và v''
4. Tính subpath_cost = cum_cost[v''] - cum_cost[v']
5. Chạy truy vấn khoảng cách CCH d(v', v'') bằng cch_point_distance()
6. Chấp nhận khi subpath_cost ≤ (1 + LOCAL_OPT_EPSILON) × d(v', v'')
   với LOCAL_OPT_EPSILON = 0.1
```

Nếu khoảng T bao trùm toàn bộ đường đi, bộ lọc stretch đã xử lý trường hợp
tổng thể — T-test được bỏ qua.

Phương thức `cch_point_distance` chạy một lần duyệt elimination tree hai chiều
mới dùng mảng tạm `ttest_*` riêng, trả về khoảng cách ngắn nhất chính xác mà
không làm nhiễu con trỏ cha của lần duyệt chính.

### 2.5 Giai Đoạn 4 — Phân Rã Đệ Quy

Sau giai đoạn chọn lọc cơ bản tạo ra các tuyến thay thế ban đầu, thuật toán cố
tìm thêm tuyến đa dạng hơn bằng cách phân rã đệ quy đường đi chính quanh đỉnh
separator có rank cao nhất.

**Phương thức:** `multi_query_recursive_inner()`

```
1. Xác định separator v: đỉnh có rank cao nhất trên đường đi chính
2. Trích v_s (đỉnh trước v) và v_t (đỉnh sau v)
3. Tính tham số điều chỉnh cho bài toán con trái (S→v_s) và phải (v_t→T):
   - gamma (ngưỡng chia sẻ) = (γ × best_dist - d(v_s,v_t)) / d(sub)
   - alpha (hệ số T-test) = α × best_dist / d(sub)
4. Đệ quy trên bài toán con trái/phải với ngưỡng chặt hơn
5. Kết hợp: ghép left_alt + [v_s, v, v_t] + right_alt cho mọi tổ hợp
6. Sắp xếp ứng viên kết hợp theo khoảng cách, chạy evaluate_candidate() lại
7. Thêm ứng viên đạt yêu cầu cho đến max_alternatives
```

**Điều kiện dừng đệ quy** — `RECURSION_MIN_RATIO = 0.3`:
Bài toán con có khoảng cách tối ưu nhỏ hơn 30% khoảng cách truy vấn gốc sẽ
không phân rã tiếp — chỉ trả về đường ngắn nhất của chính nó.

Cấu trúc đệ quy này phát hiện các tuyến thay thế khác biệt cụ thể ở cách chúng
vòng qua separator cao nhất của đường đi chính — tạo ra các tuyến khác biệt về
cấu trúc mà bước quét meeting-node phẳng có thể bỏ qua.

---

## 3. Lớp Tích Hợp: `cch.rs` / `line_graph.rs`

### 3.1 Đồ Thị Thường — `QueryEngine::multi_query`

**File:** `hanoi-core/src/cch.rs`

`QueryEngine` bọc `AlternativeServer` và cung cấp closure `path_geo_len` cần
thiết:

```
path_geo_len: |path| → tổng Haversine từ mảng latitude/longitude của đồ thị
```

Chi phí cạnh được tạo nội bộ bởi `AlternativeServer` trong
`reconstruct_path_with_costs()` — không cần closure `edge_cost` bên ngoài.

Quy trình hậu xử lý:
1. Yêu cầu thừa ứng viên: `request_count = max_alternatives × GEO_OVER_REQUEST (3)`, tối thiểu `max_alternatives + 12`
2. Với mỗi ứng viên được chấp nhận từ `AlternativeServer::alternatives()`:
   - Bỏ qua đường đi rỗng
   - Ánh xạ node ID → toạ độ
   - Áp dụng ngưỡng `MAX_GEO_RATIO` (2.0×) giới hạn khoảng cách địa lý so với tuyến ngắn nhất
   - Tái tạo arc ID qua `reconstruct_arc_ids` (quét tuyến tính trên CSR adjacency); bỏ ứng viên nếu thất bại
   - Tạo `QueryAnswer` với đầy đủ metadata

### 3.2 Đồ Thị Đường — `LineGraphQueryEngine::multi_query`

**File:** `hanoi-core/src/line_graph.rs`

Cấu trúc tương tự đồ thị thường, nhưng với các điều chỉnh riêng cho line graph:

- **Đỉnh là cạnh:** Node ID của LG tương ứng với arc ID của đồ thị gốc. Truy
  vấn nhận `source_edge` và `target_edge` làm tham số.
- **Độ dài địa lý:** `lg_path_geo_len()` ánh xạ mỗi LG node sang toạ độ đỉnh
  tail của đồ thị gốc, cộng thêm toạ độ đỉnh head cuối cùng.
- **Hiệu chỉnh cạnh nguồn:** `distance_ms = cch_distance + source_edge_cost`
  (khoảng cách CCH bắt đầu từ LG node đại diện cạnh nguồn, nhưng người dùng
  trải nghiệm toàn bộ chi phí duyệt cạnh).
- **Xây dựng kết quả:** `build_answer_from_lg_path` xử lý ánh xạ LG→gốc, ghi
  chú rẽ qua `compute_turns`/`refine_turns`, và cắt đường đi tuỳ chọn cho truy
  vấn theo toạ độ.

### 3.3 Truy Vấn Theo Toạ Độ

Cả hai engine cung cấp `multi_query_coords`:
1. Snap điểm gốc/đích bằng `SpatialIndex::validated_snap_candidates`
   (cùng logic snap như truy vấn một đường)
2. Duyệt các cặp ứng viên snap
3. Chạy `multi_query` trên cặp đầu tiên cho kết quả
4. Gán metadata gốc/đích lên tất cả các kết quả trả về

---

## 4. Tích Hợp Server

### 4.1 Luồng Xử Lý Request

```
HTTP POST /query?alternatives=3&stretch=1.25
  → handlers.rs: parse QueryRequest + FormatParam (alternatives, stretch)
  → mpsc channel → engine thread (QueryMsg với alternatives + stretch)
  → engine.rs: dispatch_normal() hoặc dispatch_line_graph()
    → nếu alternatives > 0:
        engine.multi_query_coords() hoặc engine.multi_query()
        → format_multi_response() → GeoJSON FeatureCollection hoặc JSON array
    → ngược lại:
        truy vấn một đường (luồng hiện tại)
```

### 4.2 Định Dạng Phản Hồi

**GeoJSON (mặc định):** FeatureCollection với một Feature cho mỗi tuyến. Mỗi
Feature có:
- `route_index` (0 = đường ngắn nhất)
- `distance_ms`, `distance_m`
- `path_nodes`, `route_arc_ids`, `weight_path_ids`
- `turns` (chỉ có trong chế độ line-graph)
- Mã màu khi đặt `?colors` (10 màu khác nhau, tuyến chính dày hơn)

**JSON (`?format=json`):** Mảng các đối tượng `QueryResponse`.

### 4.3 Tích Hợp CLI

`hanoi-cli query --alternatives N --stretch F` đi qua cùng các phương thức
engine. Hỗ trợ cả đồ thị thường và line graph.

---

## 5. Các Hằng Số Điều Chỉnh

**Lõi thuật toán** (`alternative.rs`):

| Hằng số | Giá trị | Mục đích |
|---|---|---|
| `DEFAULT_STRETCH` | 1.25 | Hệ số giãn địa lý tối đa (dài hơn 25%) |
| `SHARING_THRESHOLD` | 0.80 | Tỉ lệ trùng lặp chi phí tối đa với mọi tuyến đã chấp nhận |
| `BOUNDED_STRETCH_EPS` | 0.40 | Đoạn vòng phải ≤ tối ưu × (1 + ε) |
| `LOCAL_OPT_T_FRACTION` | 0.40 | Nửa cửa sổ T-test tính theo tỉ lệ khoảng cách tối ưu |
| `LOCAL_OPT_EPSILON` | 0.10 | Dung sai T-test cho tính tối ưu cục bộ |
| `RECURSION_MIN_RATIO` | 0.30 | Bỏ qua đệ quy nếu bài toán con < 30% gốc |
| `TRAVEL_TIME_STRETCH` | 1.50 | Lọc trước: bỏ ứng viên > 1.5× thời gian ngắn nhất |

**Tích hợp Hanoi** (`multi_route.rs`):

| Hằng số | Giá trị | Mục đích |
|---|---|---|
| `MAX_GEO_RATIO` | 2.0 | Lọc sau: loại tuyến > 2× khoảng cách địa lý ngắn nhất |
| `GEO_OVER_REQUEST` | 3 | Hệ số yêu cầu thừa để bù cho lọc sau |

---

## 6. Các Quyết Định Thiết Kế Chính

**Tại sao tái hiện thuật toán duyệt elimination tree thay vì tái sử dụng `Server::query`?**
Truy vấn chuẩn cắt tỉa tại meeting node qua `skip_next()` — đúng cho truy vấn
một đường nhưng mất thông tin ứng viên cần cho tuyến thay thế. Duyệt multi-route
phải relax cạnh tại mọi meeting node để đảm bảo khoảng cách truyền đúng đến tất
cả ứng viên tổ tiên.

**Tại sao dùng mảng tạm T-test riêng biệt?**
T-test chạy truy vấn con `d(v', v'')` giữa các đầu mút đường đi. Nếu các truy
vấn này tái sử dụng `fw_parents`/`bw_parents`, chúng sẽ ghi đè con trỏ cha cần
để tái tạo đường cho các ứng viên sau. Mảng riêng biệt dựa trên
`TimestampedVector` giải quyết vấn đề này với chi phí bộ nhớ tối thiểu.

**Tại sao tái tạo đường đi chỉ đọc?**
`query.rs::path()` gốc đảo ngược con trỏ thuận vào `bw_parents` tại chỗ —
nhanh nhưng phá huỷ dữ liệu. Vì multi-route cần tái tạo nhiều đường từ cùng một
lần duyệt, việc tái tạo truy vết nửa thuận và nửa ngược độc lập mà không thay
đổi mảng cha.

**Tại sao dùng hai chỉ số giãn (địa lý và thời gian)?**
Độ giãn thời gian di chuyển đơn lẻ có thể chấp nhận tuyến vòng địa lý lớn nếu
đường tình cờ nhanh. Độ giãn địa lý đảm bảo tuyến thay thế trông hợp lý trên
bản đồ. Duyệt CCH dùng thời gian làm chỉ số chính; kiểm tra địa lý là bộ lọc
phụ sau.

**Tại sao kiểm tra chia sẻ từng cặp thay vì chỉ so với đường ngắn nhất?**
Kiểm tra trùng lặp chỉ với đường ngắn nhất cho phép nhiều tuyến thay thế đa dạng
với tuyến ngắn nhất nhưng gần giống nhau. Kiểm tra từng cặp đảm bảo mọi cặp
tuyến được chấp nhận đều đủ khác biệt.

**Tại sao chia sẻ theo chi phí thay vì đếm số cạnh?**
Đếm số cạnh coi mọi cạnh như nhau, nên vài cạnh cao tốc chung đắt tiền có thể
bị áp đảo bởi nhiều cạnh đường nội bộ chung rẻ. Dùng chi phí từng cạnh đánh
trọng trùng lặp theo tác động thực tế lên thời gian di chuyển.

**Tại sao bounded stretch tại điểm rẽ?**
Độ giãn địa lý bắt các vòng lớn tổng thể, nhưng một tuyến có thể chung phần lớn
đoạn đầu/cuối với đường ngắn nhất và chỉ rẽ ngắn qua một đoạn dài không cần
thiết. Kiểm tra bounded stretch tại điểm rẽ cô lập chính xác đoạn rẽ và kiểm tra
nó với đường ngắn nhất thực giữa hai điểm đó.

**Tại sao phân rã đệ quy?**
Bước quét meeting-node phẳng tìm các tuyến khác biệt tại các đỉnh separator CCH.
Nhưng separator tốt nhất có thể không chia tuyến thành các vùng địa lý khác biệt
về cấu trúc. Đệ quy chia quanh separator có rank cao nhất và tìm tuyến thay thế
từng nửa độc lập, sau đó ghép lại — phát hiện các tuyến mà bước quét phẳng không
thể đạt tới.
