# Hướng Dẫn Bảo Trì CCH-Hanoi

Hướng dẫn thực hành để bảo trì workspace CCH-Hanoi mà không cần hiểu sâu về
lõi chương trình định tuyến (`rust_road_router`).

---

## 1. Kiến Trúc Tổng Quan

```
                           +------------------+
                           | rust_road_router |  <-- KHÔNG ĐƯỢC CHỈNH SỬA
                           |  (engine crate)  |
                           +--------+---------+
                                    |
        Cargo path dependency       |  (kiểu dữ liệu + API thuật toán)
                                    |
+-------------------------------+   |   +-------------------+
|         hanoi-core            |<--+   |   hanoi-gateway   |
| Xây dựng CCH, truy vấn,      |       | Reverse proxy     |
| spatial, hình học, cache     |       | phân phối profile  |
+------+-------+-------+------+       | (không phụ thuộc   |
       |       |       |              |  engine)           |
       v       v       v              +-------------------+
  hanoi-    hanoi-   hanoi-     hanoi-
  server     cli     tools      bench
```

**Quy tắc chính:** `rust_road_router` là dependency bất khả xâm phạm. Chỉ gọi
API công khai của nó; không bao giờ sửa source code.

---

## 2. Chức Năng Các Crate

| Crate | Dòng code | Phụ thuộc engine? | Chức năng |
|-------|----------:|:-----------------:|-----------|
| `hanoi-core` | ~2800 | **Trực tiếp** | Xây dựng/truy vấn CCH, spatial index, hình học, via-way, cache, multi-route |
| `hanoi-server` | ~3000 | Nhẹ | HTTP server (Axum), handlers, traffic overlay, camera overlay, route eval, UI |
| `hanoi-cli` | ~600 | Không | CLI wrapper qua `hanoi-core` (query, multi-query, info) |
| `hanoi-gateway` | ~520 | Không | Reverse proxy phân phối theo profile (car/motorcycle) |
| `hanoi-tools` | ~800 | Vừa | Công cụ offline: `generate_line_graph`, `diagnose_turn` |
| `hanoi-bench` | ~1500 | Nhẹ | Benchmark: CCH build, customize, query, spatial, HTTP |

### "Phụ thuộc nhẹ" nghĩa là gì

`hanoi-server`, `hanoi-bench` chỉ import **type alias** từ engine:

```rust
use rust_road_router::datastr::graph::Weight;    // = u32
use rust_road_router::datastr::graph::INFINITY;  // = u32::MAX
use rust_road_router::datastr::graph::NodeId;    // = u32
use rust_road_router::datastr::graph::EdgeId;    // = u32
```

Tất cả đều là `u32`. Không cần hiểu thuật toán.

---

## 3. Biểu Đồ Phụ Thuộc (Cargo)

```
hanoi-server  ──phụ thuộc──>  hanoi-core  ──phụ thuộc──>  rust_road_router
hanoi-cli     ──phụ thuộc──>  hanoi-core
hanoi-bench   ──phụ thuộc──>  hanoi-core
hanoi-tools   ──phụ thuộc──>  rust_road_router  (trực tiếp, cho I/O + kiểu graph)
hanoi-gateway ──phụ thuộc──>  (không có dependency trong workspace — chỉ là HTTP proxy)
```

Thư viện bên thứ ba đáng chú ý:

| Thư viện | Dùng bởi | Mục đích |
|----------|----------|----------|
| `axum` | server, gateway | HTTP framework |
| `tokio` | server, gateway | Async runtime |
| `kiddo` | core | KD-tree cho spatial snapping |
| `rayon` | core | CCH customization song song |
| `arrow` | server | Road arc manifest (Arrow IPC) |
| `clap` | server, cli, gateway | Phân tích tham số CLI |
| `sha2` | core | Checksum cho CCH cache |
| `memmap2` | core | Memory-mapped file I/O |

---

## 4. Bản Đồ Rủi Ro Theo File

### Vùng An Toàn (không cần hiểu engine)

Các file này có thể sửa tự do. Chúng xử lý HTTP, cấu hình, định dạng I/O,
và logic nghiệp vụ:

| File | Chức năng |
|------|-----------|
| `hanoi-server/src/handlers.rs` | Xử lý HTTP request |
| `hanoi-server/src/types.rs` | Struct JSON request/response |
| `hanoi-server/src/state.rs` | Trạng thái server, mpsc messages |
| `hanoi-server/src/traffic.rs` | Logic overlay lưu lượng giao thông |
| `hanoi-server/src/camera_overlay.rs` | Overlay camera tốc độ |
| `hanoi-server/src/route_eval.rs` | Đánh giá tuyến đường (GeoJSON replay) |
| `hanoi-server/src/ui.rs` | Giao diện web nhúng |
| `hanoi-server/src/engine.rs` | Phân phối truy vấn (mpsc consumer) |
| `hanoi-server/src/main.rs` | Khởi động và ghép nối server |
| `hanoi-cli/src/main.rs` | Điểm vào CLI |
| `hanoi-gateway/src/*` | Toàn bộ crate gateway |
| `hanoi-core/src/bounds.rs` | Kiểm tra bounding box |
| `hanoi-core/src/geometry.rs` | Tính góc rẽ |
| `hanoi-core/src/spatial.rs` | Spatial index KD-tree |
| `hanoi-core/src/multi_route.rs` | Hằng số multi-route |
| `hanoi-core/src/graph.rs` | Wrapper tải dữ liệu graph |
| `hanoi-bench/src/*` | Toàn bộ hạ tầng benchmark |

### Vùng Cẩn Thận (gọi API engine)

Các file này gọi API xây dựng/truy vấn CCH. Thay đổi ở đây cần hiểu vòng đời
CCH ba pha (xem mục 5):

| File | Làm gì với engine |
|------|-------------------|
| `hanoi-core/src/cch.rs` | CCH đồ thị thường: xây dựng, customize, truy vấn |
| `hanoi-core/src/line_graph.rs` | CCH line graph: xây dựng, customize, truy vấn + giải nén đường |
| `hanoi-core/src/cch_cache.rs` | Serialize/deserialize DirectedCCH ra disk cache |
| `hanoi-core/src/via_way_restriction.rs` | Tải hạn chế rẽ via-way |
| `hanoi-tools/src/bin/generate_line_graph.rs` | Tạo line graph từ đồ thị thường |
| `hanoi-tools/src/bin/diagnose_turn.rs` | Chẩn đoán hạn chế rẽ |

---

## 5. Vòng Đời CCH (Kiến Thức Cần Thiết)

API engine theo một vòng đời ba pha cứng nhắc. Đây là khái niệm thuật toán
duy nhất bạn cần hiểu:

```
Pha 1: XÂY DỰNG           Pha 2: TÙY CHỈNH           Pha 3: TRUY VẤN
───────────────            ────────────────            ───────────────
Graph + thứ tự             CCH + trọng số              CCH đã tùy chỉnh
       |                        |                          |
       v                        v                          v
CCH::fix_order_and_build  customize(&cch, &metric)   server.query(Query { from, to })
       |                        |                          |
       v                        v                          v
  CCH / DirectedCCH       CustomizedBasic             QueryResult { distance, path }
  (topo, bất biến)        (trọng số lên/xuống)        (kết quả đường ngắn nhất)
```

- **Pha 1** tốn kém (~30-60 giây). Chạy một lần khi khởi động hoặc tải từ cache.
- **Pha 2** nhanh (~1-3 giây). Chạy lại khi trọng số thay đổi (cập nhật giao thông).
- **Pha 3** tức thì (~0.1-1ms mỗi truy vấn).

Hai struct wrapper trong `hanoi-core`:

| Struct | Kiểu engine bên trong | Pha 1 | Pha 2 | Pha 3 |
|--------|----------------------|-------|-------|-------|
| `CchContext` | `CCH` (vô hướng) | `load_and_build()` | `customize()` | `QueryEngine::query()` |
| `LineGraphCchContext` | `DirectedCCH` | `load_and_build()` | `customize()` | `LineGraphQueryEngine::query()` |

---

## 6. Các Endpoint HTTP API

Tất cả route được định nghĩa tại `hanoi-server/src/main.rs:341-363`.

### Query Router (đồng thời với truy vấn)

| Method | Path | Handler | Mục đích |
|--------|------|---------|----------|
| POST | `/query` | `handle_query` | Truy vấn đường ngắn nhất (tọa độ hoặc node ID) |
| POST | `/evaluate_routes` | `handle_evaluate_routes` | Đánh giá các tuyến GeoJSON nhập vào |
| POST | `/reset_weights` | `handle_reset_weights` | Reset trọng số về baseline |
| GET | `/traffic_overlay` | `handle_traffic_overlay` | Trạng thái trọng số giao thông hiện tại |
| GET | `/camera_overlay` | `handle_camera_overlay` | Dữ liệu camera tốc độ |
| GET | `/info` | `handle_info` | Metadata server (số nút, số cạnh, chế độ) |
| GET | `/health` | `handle_health` | Health check |
| GET | `/ready` | `handle_ready` | Readiness probe |

### Customize Router (tuần tự, chặn truy vấn trong lúc customize)

| Method | Path | Handler | Mục đích |
|--------|------|---------|----------|
| POST | `/customize` | `handle_customize` | Áp dụng vector trọng số mới |

### UI Router (tài nguyên tĩnh)

| Method | Path | Handler |
|--------|------|---------|
| GET | `/` , `/ui` | `handle_index` |
| GET | `/assets/cch-query.css` | `handle_styles` |
| GET | `/assets/cch-query.js` | `handle_script` |

### Gateway API (hanoi-gateway)

| Method | Path | Mục đích |
|--------|------|----------|
| POST | `/query?profile=<tên>` | Chuyển tiếp đến backend theo profile |
| GET | `/info?profile=<tên>` | Metadata backend |
| GET | `/profiles` | Liệt kê các profile khả dụng |

---

## 7. Các Tác Vụ Bảo Trì Thường Gặp

### Thêm endpoint HTTP mới

1. Định nghĩa kiểu request/response trong `hanoi-server/src/types.rs`
2. Thêm hàm handler trong `hanoi-server/src/handlers.rs`
3. Đăng ký route trong `hanoi-server/src/main.rs` (query_router hoặc customize_router)
4. Không cần hiểu engine.

### Thay đổi định dạng response truy vấn

1. Sửa `QueryAnswer` trong `hanoi-core/src/cch.rs:18-40`
2. Cập nhật JSON serialization trong `hanoi-server/src/handlers.rs:handle_query`
3. Struct `QueryAnswer` là dữ liệu thuần — không có kiểu engine trong đó.

### Thêm profile gateway mới

1. Thêm mục vào file cấu hình YAML gateway
2. Khởi động instance `hanoi_server` mới trỏ đến thư mục graph của profile
3. Không cần thay đổi code.

### Cập nhật logic traffic overlay

1. Sửa `hanoi-server/src/traffic.rs`
2. `TrafficOverlay` làm việc với `Vec<Weight>` (chỉ là `Vec<u32>`) — không có kiểu engine.
3. Overlay được áp dụng qua endpoint `/customize` kích hoạt Pha 2.

### Thêm subcommand CLI mới

1. Thêm variant vào enum `Command` trong `hanoi-cli/src/main.rs`
2. Sử dụng API công khai của `hanoi-core` (`CchContext`, `LineGraphCchContext`, v.v.)
3. Không cần import trực tiếp từ engine — CLI đi qua `hanoi-core`.

### Cập nhật logic spatial snapping

1. Sửa `hanoi-core/src/spatial.rs`
2. Sử dụng KD-tree `kiddo` — spatial indexing chuẩn, không phụ thuộc engine.
3. Kiểu engine duy nhất dùng: `NodeId`, `EdgeId` (đều là `u32`).

### Chỉnh sửa camera overlay

1. Sửa `hanoi-server/src/camera_overlay.rs`
2. Xử lý dữ liệu thuần (Arrow IPC + cấu hình YAML). Không phụ thuộc engine.

---

## 8. Build & Test

```bash
# Build toàn bộ
cd CCH-Hanoi
cargo build --release --workspace

# Build binary cụ thể
cargo build --release -p hanoi-server --bin hanoi_server
cargo build --release -p hanoi-cli --bin cch-hanoi
cargo build --release -p hanoi-gateway --bin hanoi_gateway

# Chạy test (nếu có)
cargo test --workspace

# Chạy server
./target/release/hanoi_server \
    --graph-dir Maps/data/hanoi_motorcycle/line_graph \
    --original-graph-dir Maps/data/hanoi_motorcycle/graph \
    --line-graph

# Chạy gateway
./target/release/hanoi_gateway --config gateway.yaml
```

**Yêu cầu Rust nightly** — workspace sử dụng `edition = "2024"`.

---

## 9. Khi Nào Cần Rebuild Engine?

Bạn KHÔNG cần rebuild `rust_road_router` trừ khi:

- Phiên bản toolchain Rust thay đổi (cập nhật nightly)
- Bạn chạy `cargo clean`
- Ai đó sửa file trong `rust_road_router/engine/` (cấm theo quy định)

Phát triển CCH-Hanoi bình thường chỉ compile lại các crate CCH-Hanoi. Engine
được compile một lần và Cargo cache lại.

---

## 10. Xử Lý Sự Cố

| Triệu chứng | Nguyên nhân có thể | Cách xử lý |
|-------------|-------------------|------------|
| Server khởi động mất 30-60 giây | CCH build (Pha 1) chạy từ đầu | Kiểm tra `cch_cache/` có tồn tại không. Lần chạy đầu luôn chậm. |
| Panic "failed to load graph" | Sai đường dẫn `--graph-dir` hoặc thiếu file binary | Xác nhận `first_out`, `head`, `travel_time` tồn tại trong thư mục |
| Thiếu "via_way_split_map" | Chưa tạo line graph | Chạy lại `generate_line_graph` trên thư mục graph |
| Truy vấn trả về INFINITY | Không có đường đi, hoặc điểm snap rơi vào thành phần mất liên thông | Kiểm tra tọa độ hợp lệ, bounding box, số lượng snap candidates |
| `/customize` chặn truy vấn | Theo thiết kế — customization là tuần tự | Giữ vector trọng số nhỏ; customization mất ~1-3 giây |
| Gateway trả về 400 | Tên profile không biết | Kiểm tra file cấu hình YAML gateway để biết các profile khả dụng |
| Cache rebuild mỗi lần khởi động | File nguồn đã thay đổi (checksum không khớp) | Bình thường sau khi chạy lại data pipeline |

---

## 11. Các File Không Bao Giờ Cần Chỉnh Sửa

| Đường dẫn | Lý do |
|-----------|-------|
| `rust_road_router/` | Lõi thuật toán — cấm chỉnh sửa |
| `RoutingKit/` | Dependency C++ — cấm chỉnh sửa |
| `InertialFlowCutter/` | Công cụ sắp xếp CCH — cấm chỉnh sửa |
| `hanoi-core/src/cch_cache.rs` | Serialize nội bộ engine — chỉ thay đổi khi struct engine thay đổi |
| `hanoi-core/src/cch.rs:67-88` | Hàm `load_and_build` — ổn định, mẫu build-customize-query |
| `hanoi-core/src/line_graph.rs:69-198` | Tương tự ở trên cho biến thể line graph |

---

## 12. Bảng Tra Cứu Nhanh Kiểu Engine

Khi đọc code CCH-Hanoi, các kiểu engine này xuất hiện thường xuyên. Tất cả
đều là wrapper đơn giản quanh số nguyên:

| Kiểu engine | Kiểu thực tế | Ý nghĩa |
|-------------|-------------|---------|
| `Weight` | `u32` | Thời gian di chuyển (mili giây) |
| `NodeId` | `u32` | Chỉ số nút trong graph |
| `EdgeId` | `u32` | Chỉ số cạnh trong graph |
| `EdgeIdT` | `u32` (newtype) | Edge ID an toàn kiểu |
| `INFINITY` | `u32::MAX` | Giá trị sentinel "không có đường" |
| `NodeOrder` | `Arc<[u32]>` x 2 | Hoán vị nút CCH (ranks + order) |
| `CCH` | struct | Topo CCH vô hướng |
| `DirectedCCH` | struct | Topo CCH có hướng (cho line graph) |
| `CustomizedBasic` | struct | CCH + trọng số (kết quả Pha 2) |
| `FirstOutGraph` | struct | Graph CSR (first_out + head + weight slices) |

---

## 13. Nhật Ký Quyết Định

| Quyết định | Lý do |
|-----------|-------|
| `rust_road_router` là chỉ-đọc | Đội thiếu kinh nghiệm Rust thuật toán sâu; engine đã được kiểm chứng |
| `hanoi-gateway` không phụ thuộc engine | HTTP proxy thuần; có thể bảo trì/viết lại độc lập |
| `hanoi-cli` chỉ import từ `hanoi-core` | Tách CLI khỏi engine; truy cập thuật toán qua core |
| CCH cache nằm trong `hanoi-core`, không phải engine | Logic cache là đặc thù triển khai, không phải đặc thù thuật toán |
| Type alias (`Weight`, `NodeId`) import từ engine | Tránh phân kỳ; chi phí là Cargo dependency, không phải gánh nặng nhận thức |
