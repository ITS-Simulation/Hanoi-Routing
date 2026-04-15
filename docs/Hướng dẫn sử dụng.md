# Hướng dẫn Sử dụng

Tài liệu hướng dẫn sử dụng hệ thống định tuyến CCH-Hanoi: API server, CLI,
benchmark, và các luồng nghiệp vụ thường gặp.

---

## Mục lục

1. [Tổng quan hệ thống](#1-tổng-quan-hệ-thống)
2. [Giao diện Route Viewer (UI)](#2-giao-diện-route-viewer-ui)
3. [HTTP API Reference](#3-http-api-reference)
4. [CLI Reference (cch-hanoi)](#4-cli-reference-cch-hanoi)
5. [Tùy chỉnh trọng số (Customization)](#5-tùy-chỉnh-trọng-số-customization)
6. [Benchmark & Phân tích hiệu năng](#6-benchmark--phân-tích-hiệu-năng)
7. [Gateway đa profile](#7-gateway-đa-profile)
8. [Định dạng Output](#8-định-dạng-output)
9. [Luồng nghiệp vụ thường gặp](#9-luồng-nghiệp-vụ-thường-gặp)
10. [Hiệu năng tham khảo](#10-hiệu-năng-tham-khảo)
11. [Công cụ chẩn đoán (diagnose_turn)](#11-công-cụ-chẩn-đoán-diagnose_turn)

---

## 1. Tổng quan hệ thống

### Kiến trúc 3 pha

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────┐
│  Pha 1:         │    │  Pha 2:          │    │  Pha 3:     │
│  Contraction    │───▶│  Customization   │───▶│  Query      │
│  (khởi động)    │    │  (khi đổi weight)│    │  (mỗi request)│
└─────────────────┘    └──────────────────┘    └─────────────┘
  Xây cấu trúc CCH      Gán trọng số lên       Tìm đường ngắn
  (1 lần duy nhất)       CCH shortcuts           nhất 2 điểm
```

### Hai chế độ routing

| Chế độ       | Mô tả                                     | Ưu điểm                    | Nhược điểm          |
| ------------ | ------------------------------------------ | --------------------------- | -------------------- |
| **Normal**   | Đồ thị mạng đường bình thường              | Nhanh hơn, ít bộ nhớ hơn   | Bỏ qua turn restrictions |
| **Line Graph** | Đồ thị turn-expanded (node = đoạn đường) | Chính xác, có turn modeling | Đồ thị lớn hơn ~2x  |

> **Khuyến nghị**: Luôn dùng chế độ Line Graph cho routing thực tế để đảm bảo
> các ràng buộc rẽ (cấm rẽ trái, cấm quay đầu, v.v.) được tính toán chính xác.

### Các thành phần

| Thành phần        | Loại    | Mô tả                                      |
| ----------------- | ------- | ------------------------------------------- |
| `hanoi_server`    | Binary  | HTTP server REST API (query + customize + overlays) |
| `hanoi_gateway`   | Binary  | API gateway proxy multiple profiles         |
| `cch-hanoi`       | Binary  | CLI offline                                 |
| `bench_core`      | Binary  | Benchmark thuật toán core                   |
| `bench_server`    | Binary  | Benchmark HTTP server                       |
| `bench_report`    | Binary  | So sánh và phân tích kết quả benchmark      |
| `generate_line_graph` | Binary | Pipeline tool: sinh đồ thị turn-expanded |
| `diagnose_turn`   | Binary  | Chẩn đoán turn restrictions tại tọa độ     |

---

## 2. Giao diện Route Viewer (UI)

Server tích hợp giao diện web để truy vấn, so sánh route, và giám sát giao
thông trực quan trên bản đồ.

### 2.1 Khởi động server với UI

```bash
hanoi_server \
  --graph-dir Maps/data/hanoi_motorcycle/line_graph \
  --original-graph-dir Maps/data/hanoi_motorcycle/graph \
  --line-graph --serve-ui \
  --query-port 8080
```

Truy cập: `http://localhost:8080` hoặc `http://localhost:8080/ui`

### 2.2 Bố cục giao diện

```
┌──────────────────────────────────────────────────┐
│  Sidebar (thu gọn được)           │    Bản đồ    │
│  ▸ Server Context (metadata)      │   (Leaflet)  │
│  ▸ Workspace: Query / Compare     │              │
│  ▸ Map Tools: Route / Measure     │              │
│  ▸ Map Layers: base map, overlays │              │
│  ▸ Query Panel / Compare Panel    │              │
└──────────────────────────────────────────────────┘
```

### 2.3 Luồng truy vấn route (Query)

**Bước 1**: Click lên bản đồ chọn điểm đi (marker xanh cyan) → click tiếp chọn
điểm đến (marker cam). Hoặc nhập tọa độ thủ công.

**Bước 2**: Nhấn **Find Route**.

- **Single mode**: Trả về 1 tuyến tối ưu.
- **Multi mode**: Bật chế độ K-Shortest Paths.
  - `Alternatives`: số tuyến thay thế (1–20, mặc định 5).
  - `Stretch`: hệ số đa dạng (1.0–3.0, mặc định 1.3).

**Bước 3**: Kết quả hiển thị:

- Tuyến đường trên bản đồ (polyline, 8 màu xoay vòng cho multi-route).
- Danh sách route trên sidebar: thời gian (ms), khoảng cách (m), so sánh %.
- Tab **Turns** (chỉ line-graph mode): turn-by-turn — hướng rẽ, góc, khoảng
  cách đến lượt rẽ tiếp theo.

**Bước 4**: Click route trên bản đồ hoặc sidebar để chọn. **Export** GeoJSON.

### 2.4 So sánh route ngoài (Compare)

Chuyển sang tab **Compare** trên sidebar.

1. **Load GeoJSON** — chọn file `.geojson` / `.json` (tối đa 10 tuyến).
2. Server đánh giá từng tuyến qua `/evaluate_routes`: travel_time_ms,
   distance_m, arc_count.
3. Các tuyến hiển thị chồng lên bản đồ với palette 10 màu riêng.

**Chế độ Focus 1-1**: So sánh chi tiết 2 tuyến — hiển thị tuyến nào nhanh hơn,
ngắn hơn, chênh lệch thời gian & khoảng cách.

### 2.5 Đo khoảng cách (Measure)

Chuyển Map Tool sang **Measure**.

- Click đặt waypoint liên tiếp.
- Hiển thị: tổng khoảng cách, số đoạn, số điểm.
- **Undo**: xóa điểm cuối. **Clear**: xóa toàn bộ.

### 2.6 Overlay giao thông

**Traffic Overlay** (bật/tắt trên sidebar):

- Tự động load khi zoom ≥ 14, refresh mỗi 10 giây.
- Phân 3 mức: 🟢 xanh (free flow, ratio ≤ 1.15), 🟡 vàng (moderate, ≤ 1.60),
  🔴 đỏ (congested, > 1.60).
- Lọc chỉ đường từ tertiary trở lên (nếu dataset hỗ trợ).

**Camera Overlay** (bật/tắt trên sidebar):

- Hiển thị marker tròn cam tại vị trí camera.
- Click marker xem: label, ID, profile, arc_id, tọa độ.
- Danh sách camera load từ file YAML cấu hình trên server.

### 2.7 Bản đồ nền

3 style chuyển đổi:

| Style              | Mô tả                                |
| ------------------ | ------------------------------------- |
| **Simple Light**   | CARTO light, ít chi tiết (mặc định)  |
| **Balanced Streets** | CARTO voyager, chi tiết vừa        |
| **Classic OSM**    | OpenStreetMap, chi tiết đầy đủ       |

### 2.8 Thông tin server & Reset

- **Server Context** trên sidebar: graph type, nodes/edges, uptime, tổng queries.
- **Refresh**: cập nhật lại metadata.
- **Reset Weights**: khôi phục trọng số gốc (gọi `/reset_weights`), tự động
  refresh traffic overlay và kết quả compare.

> Trạng thái UI (tab, tọa độ, overlays, chế độ query) được lưu trên trình duyệt
> qua `localStorage` — mở lại trang không mất context.

---

## 3. HTTP API Reference

### 3.1 Endpoints tổng quan

Base URL mặc định: `http://localhost:8080` (query port)

| Endpoint             | Method | Port  | Mô tả                                |
| -------------------- | ------ | ----- | ----------------------------------- |
| `/query`             | POST   | 8080  | Tìm đường ngắn nhất (hỗ trợ alternatives) |
| `/evaluate_routes`   | POST   | 8080  | Đánh giá tuyến đường GeoJSON import    |
| `/reset_weights`     | POST   | 8080  | Khôi phục trọng số gốc               |
| `/traffic_overlay`   | GET    | 8080  | Segment giao thông theo viewport    |
| `/camera_overlay`    | GET    | 8080  | Camera giao thông theo viewport     |
| `/info`              | GET    | 8080  | Metadata đồ thị                    |
| `/health`            | GET    | 8080  | Trạng thái hoạt động              |
| `/ready`             | GET    | 8080  | Kiểm tra sẵn sàng phục vụ            |
| `/customize`         | POST   | 9080  | Upload trọng số tùy chỉnh            |
| `/`, `/ui`           | GET    | 8080  | Route viewer UI (khi `--serve-ui`)  |

### 3.2 POST /query — Tìm đường

**Request Body** (JSON):

```json
{
  "from_lat": 21.0283,
  "from_lng": 105.8542,
  "to_lat": 20.9980,
  "to_lng": 105.8286
}
```

Hoặc dùng node ID:

```json
{
  "from_node": 12345,
  "to_node": 67890
}
```

**Query Parameters**:

| Param          | Mô tả                                              |
| -------------- | --------------------------------------------------- |
| `format`       | `json` → JSON thuần; bỏ trống → GeoJSON (mặc định) |
| `colors`       | Thêm simplestyle-spec properties cho visualization  |
| `alternatives` | Số tuyến đường thay thế (u32, mặc định: 0)         |
| `stretch`      | Hệ số stretch cho alternative routes (f64)          |

**Response — GeoJSON** (mặc định):

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[105.854, 21.028], [105.853, 21.027], ...]
    },
    "properties": {
      "distance_ms": 45230,
      "distance_m": 12567.3
    }
  }]
}
```

> **Lưu ý**: GeoJSON dùng thứ tự `[longitude, latitude]` theo RFC 7946.

**Response — JSON** (`?format=json`):

```json
{
  "distance_ms": 45230,
  "distance_m": 12567.3,
  "path_nodes": [100, 205, 310, ...],
  "coordinates": [[21.028, 105.854], [21.027, 105.853], ...]
}
```

> **Lưu ý**: JSON format dùng thứ tự `[latitude, longitude]` (legacy).

**Response với `?colors`** — thêm vào `properties`:

```json
{
  "stroke": "#ff5500",
  "stroke-width": 10,
  "fill": "#ffaa00",
  "fill-opacity": 0.4
}
```

Dùng để mở trực tiếp trên [geojson.io](https://geojson.io) có hiển thị tuyến
đường với màu sắc.

**Lỗi** — HTTP 400:

```json
{
  "error": "coordinate_validation_failed",
  "message": "source coordinate (10.00, 100.00) is outside graph bounding box",
  "details": { ... }
}
```

**Ví dụ curl**:

```bash
# Tuyến dài: trung tâm → phía Nam
curl -s http://localhost:8080/query?colors \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.03835,"from_lng":105.78310,"to_lat":20.887784,"to_lng":105.775691}' \
  > route.geojson

# Tuyến ngắn: Phố Cổ
curl -s http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.03389,"from_lng":105.85127,"to_lat":21.03160,"to_lng":105.85263}'

# Xuyên thành phố: Tây → Đông
curl -s http://localhost:8080/query?colors \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.02940,"from_lng":105.75407,"to_lat":21.01320,"to_lng":105.94399}'

# Alternative routes (2 tuyến thay thế)
curl -s "http://localhost:8080/query?alternatives=2&colors" \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.03835,"from_lng":105.78310,"to_lat":20.887784,"to_lng":105.775691}'
```

### 3.3 GET /info — Metadata đồ thị

```json
{
  "graph_type": "line_graph",
  "num_nodes": 1943051,
  "num_edges": 4396227,
  "customization_active": false,
  "bbox": {
    "min_lat": 20.5643,
    "max_lat": 21.3886,
    "min_lng": 105.2873,
    "max_lng": 106.1735
  }
}
```

### 3.4 GET /health — Trạng thái hoạt động

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "total_queries_processed": 15234,
  "customization_active": false
}
```

### 3.5 GET /ready — Sẵn sàng phục vụ

```json
{ "ready": true }
```

Trả về HTTP 503 nếu engine thread đã chết:

```json
{ "ready": false }
```

### 3.6 POST /customize — Upload trọng số (port 9080)

Upload vector trọng số mới (little-endian `u32`, mỗi edge 4 bytes):

```bash
# Upload file raw binary
curl -X POST http://localhost:9080/customize \
  --data-binary @custom_weights.bin

# Upload với nén gzip
gzip -c custom_weights.bin | curl -X POST http://localhost:9080/customize \
  -H "Content-Encoding: gzip" \
  --data-binary @-
```

Yêu cầu:
- Body size = `num_edges × 4` bytes (lấy `num_edges` từ `/info`)
- Mọi giá trị phải < 2,147,483,647 (INFINITY sentinel)
- Hỗ trợ gzip decompression tự động
- Body limit: 64 MB

Response:

```json
{ "accepted": true, "message": "customization queued" }
```

Sau khi customize xong, mọi query tiếp theo sẽ dùng trọng số mới.

### 3.7 POST /evaluate_routes — Đánh giá tuyến đường GeoJSON

Upload tối đa 10 tuyến đường GeoJSON để đánh giá trọng số trên đồ thị hiện tại.

**Request Body** (JSON):

```json
{
  "routes": [
    {
      "name": "Route A",
      "geojson": { "type": "FeatureCollection", "features": [...] }
    }
  ]
}
```

**Response**:

```json
{
  "using_customized_weights": false,
  "graph_type": "line_graph",
  "routes": [{
    "name": "Route A",
    "travel_time_ms": 45230,
    "distance_m": 12567.3,
    "geometry_point_count": 120,
    "route_arc_count": 85,
    "travel_time_mode": "sum",
    "distance_mode": "haversine",
    "error": null
  }]
}
```

### 3.8 POST /reset_weights — Khôi phục trọng số gốc

Khôi phục về trọng số baseline (travel_time gốc từ file đồ thị).

```bash
curl -X POST http://localhost:8080/reset_weights
```

Response: `{ "accepted": true, "message": "weights reset to baseline" }`

### 3.9 GET /traffic_overlay — Segment giao thông

Trả về segment đường phân nhóm theo mức độ tắc nghẽn trong viewport.

**Query Parameters**:

| Param                       | Mô tả                                |
| --------------------------- | ------------------------------------- |
| `min_lat`, `max_lat`        | Phạm vi vĩ độ                        |
| `min_lng`, `max_lng`        | Phạm vi kinh độ                      |
| `tertiary_and_above_only`   | Chỉ hiện đường từ tertiary trở lên   |

**Response**:

```json
{
  "using_customized_weights": true,
  "mapping_mode": "line_graph",
  "tertiary_filter_supported": true,
  "tertiary_and_above_only": false,
  "visible_segment_count": 1523,
  "buckets": [
    { "status": "optimal", "color": "green", "segments": [[[21.028, 105.854], [21.027, 105.853]]] },
    { "status": "medium",  "color": "yellow", "segments": [...] },
    { "status": "heavy",   "color": "red",    "segments": [...] }
  ]
}
```

Phân loại congestion ratio:
- **Green** (optimal): ratio ≤ 1.15
- **Yellow** (medium): ratio ≤ 1.60
- **Red** (heavy): ratio > 1.60

### 3.10 GET /camera_overlay — Camera giao thông

Trả về vị trí camera giao thông trong viewport. Dữ liệu camera load từ file
YAML (mặc định: `CCH_Data_Pipeline/config/mvp_camera.yaml`).

**Query Parameters**: `min_lat`, `max_lat`, `min_lng`, `max_lng`

**Response**:

```json
{
  "available": true,
  "visible_camera_count": 12,
  "total_camera_count": 45,
  "cameras": [
    { "id": 1, "label": "Cam Ngã Tư Sở", "profile": "motorcycle", "arc_id": 12345, "lat": 21.003, "lng": 105.821 }
  ]
}
```

---

## 4. CLI Reference (cch-hanoi)

CLI cho phép chạy query offline mà không cần server. Tự load graph, build CCH,
customize, và tìm đường.

### 4.1 Xem thông tin đồ thị

```bash
cch-hanoi info --data-dir Maps/data/hanoi_motorcycle
cch-hanoi info --data-dir Maps/data/hanoi_motorcycle --line-graph
```

Output:

```json
{
  "graph_type": "line_graph",
  "graph_dir": "Maps/data/hanoi_motorcycle/line_graph",
  "num_nodes": 1943051,
  "num_edges": 4396227
}
```

### 4.2 Query bằng tọa độ

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_motorcycle \
  --line-graph \
  --from-lat 21.0283  --from-lng 105.8542 \
  --to-lat   20.9980  --to-lng   105.8286 \
  --output-format geojson \
  --demo \
  --output-file route.geojson
```

### 4.3 Query bằng node ID

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_motorcycle \
  --line-graph \
  --from-node 500000 --to-node 800000 \
  --output-format json
```

### 4.4 Tham số CLI đầy đủ

**Tham số chung:**

| Tham số        | Mặc định | Mô tả                              |
| -------------- | -------- | ----------------------------------- |
| `--log-format` | `pretty` | `pretty`, `full`, `compact`, `tree`, `json` |
| `--log-file`   | _(không)_| Ghi log ra file (JSON format)       |

**Subcommand `query`:**

| Tham số           | Bắt buộc | Mô tả                                    |
| ----------------- | -------- | ----------------------------------------- |
| `--data-dir`      | Có       | Thư mục chứa `graph/` và `line_graph/`    |
| `--line-graph`    | Không    | Dùng đồ thị turn-expanded                 |
| `--from-lat`      | *        | Vĩ độ điểm đi                             |
| `--from-lng`      | *        | Kinh độ điểm đi                            |
| `--to-lat`        | *        | Vĩ độ điểm đến                            |
| `--to-lng`        | *        | Kinh độ điểm đến                           |
| `--from-node`     | *        | Node ID điểm đi                           |
| `--to-node`       | *        | Node ID điểm đến                          |
| `--output-format` | Không    | `geojson` (mặc định) hoặc `json`          |
| `--output-file`   | Không    | Đường dẫn file output (auto-generate nếu bỏ) |
| `--demo`          | Không    | Thêm simplestyle colors vào GeoJSON       |
| `--alternatives`  | Không    | Số tuyến đường thay thế (mặc định: 0)     |
| `--stretch`       | Không    | Hệ số stretch cho alternative routes      |

> \* Phải cung cấp ĐÚng MỘT trong hai bộ: `--from-lat/--from-lng/--to-lat/--to-lng`
> HOẶC `--from-node/--to-node`.

**Subcommand `info`:**

| Tham số        | Bắt buộc | Mô tả                                 |
| -------------- | -------- | -------------------------------------- |
| `--data-dir`   | Có       | Thư mục chứa `graph/` (và `line_graph/`) |
| `--line-graph` | Không    | Hiện info của line graph thay vì normal |

---

## 5. Tùy chỉnh trọng số (Customization)

CCH cho phép **thay đổi trọng số runtime** mà không cần rebuild cấu trúc CCH.
Đây là tính năng quan trọng cho:

- Cập nhật tình trạng giao thông real-time
- Mô phỏng kịch bản khác nhau (ưu tiên đường cao tốc, tránh đường tắc, v.v.)
- A/B testing thuật toán trọng số

### Luồng customization

```
1. Client gọi GET /info → biết num_edges
2. Client tạo vector u32 (little-endian, num_edges phần tử)
3. Client POST /customize (raw binary hoặc gzip)
4. Server validate → queue cho engine thread
5. Engine thread re-customize CCH (~50ms cho Hanoi)
6. Mọi query sau đó dùng trọng số mới
```

### Tạo vector trọng số bằng Python

```python
import struct
import requests

# Lấy num_edges
info = requests.get("http://localhost:8080/info").json()
num_edges = info["num_edges"]

# Tạo trọng số (ví dụ: tăng gấp đôi tất cả)
original = open("Maps/data/hanoi_motorcycle/line_graph/travel_time", "rb").read()
weights = struct.unpack(f"<{num_edges}I", original)
new_weights = [min(w * 2, 2_147_483_646) for w in weights]  # Không vượt INFINITY

# Upload
data = struct.pack(f"<{num_edges}I", *new_weights)
resp = requests.post("http://localhost:9080/customize", data=data)
print(resp.json())  # {"accepted": true, "message": "customization queued"}
```

### Reset về trọng số gốc

Upload lại vector `travel_time` gốc:

```bash
curl -X POST http://localhost:9080/customize \
  --data-binary @Maps/data/hanoi_motorcycle/line_graph/travel_time
```

---

## 6. Benchmark & Phân tích hiệu năng

### 6.1 bench_core — Benchmark thuật toán

Chạy offline, không cần server. Đo:
- **cch_build**: Thời gian xây dựng CCH (load + contraction)
- **customize**: Thời gian customization (gán trọng số lên shortcuts)
- **kd_tree_build**: Thời gian xây KD-tree (spatial index)
- **query_node_id**: Thời gian query by node ID
- **query_coords**: Thời gian query by tọa độ (snap + query + path unpack)
- **snap_to_edge**: Thời gian snap tọa độ GPS vào edge gần nhất

```bash
# Sinh query ngẫu nhiên
bench_core --graph-dir Maps/data/hanoi_motorcycle/graph \
  --generate-queries 10000 --seed 42 \
  --save-queries queries.json

# Chạy benchmark
bench_core --graph-dir Maps/data/hanoi_motorcycle/graph \
  --queries queries.json \
  --iterations 10 --warmup 3 --query-count 1000 \
  --output core_results.json
```

| Tham số              | Mặc định           | Mô tả                        |
| -------------------- | ------------------- | ----------------------------- |
| `--graph-dir`        | _(bắt buộc)_       | Thư mục đồ thị               |
| `--perm-path`        | `<graph>/perms/cch_perm` | Đường dẫn cch_perm       |
| `--generate-queries` | _(không)_           | Số query ngẫu nhiên cần sinh |
| `--save-queries`     | _(không)_           | Lưu queries ra file JSON     |
| `--queries`          | _(không)_           | Load queries từ file JSON    |
| `--iterations`       | `10`                | Số lần đo chính thức         |
| `--warmup`           | `3`                 | Số lần chạy warmup           |
| `--query-count`      | `1000`              | Số queries/iteration          |
| `--output`           | `core_results.json` | File output kết quả          |
| `--seed`             | `42`                | Seed cho RNG                  |

### 6.2 bench_server — Benchmark HTTP

Cần server đang chạy.

```bash
# Sequential
bench_server --url http://localhost:8080 \
  --query-file queries.json --queries 1000

# Concurrent load test
bench_server --url http://localhost:8080 \
  --query-file queries.json --queries 1000 --concurrency 8 \
  --output server_results.json

# Không cần query file (tự sinh random tọa độ trong bbox Hà Nội)
bench_server --url http://localhost:8080 --queries 500
```

| Tham số        | Mặc định               | Mô tả                          |
| -------------- | ----------------------- | ------------------------------- |
| `--url`        | `http://localhost:8080` | Base URL server                 |
| `--queries`    | `1000`                  | Số queries                      |
| `--concurrency`| `1`                     | Concurrent clients (>1 bật load test) |
| `--query-file` | _(không)_               | File query JSON                 |
| `--graph-dir`  | _(không)_               | Dùng để sinh node-ID queries    |
| `--output`     | `bench_results.json`    | File output kết quả             |
| `--seed`       | `42`                    | Seed cho RNG                    |

### 6.3 bench_report — So sánh kết quả

```bash
# Xem report 1 file
bench_report --input core_results.json --format table
bench_report --input core_results.json --format csv
bench_report --input core_results.json --format json

# So sánh 2 bản (phát hiện regression)
bench_report \
  --baseline results_v1.json \
  --current  results_v2.json \
  --threshold 10
```

Nếu bất kỳ metric nào tăng > `--threshold` %, exit code = 1 (regression detected).

| Tham số       | Mặc định | Mô tả                             |
| ------------- | -------- | ---------------------------------- |
| `--input`     | _(không)_| File kết quả cho báo cáo đơn      |
| `--baseline`  | _(không)_| File baseline cho so sánh          |
| `--current`   | _(không)_| File current cho so sánh           |
| `--format`    | `table`  | `table`, `json`, `csv`             |
| `--threshold` | `10.0`   | Ngưỡng % regression                |

---

## 7. Gateway đa profile

Gateway proxy cho phép client truy cập nhiều profile (car, motorcycle) qua 1
endpoint duy nhất, routing request tới đúng backend server.

### 7.1 Cấu hình (gateway.yaml)

```yaml
port: 50051
backend_timeout_secs: 30
log_format: pretty
# log_file: /var/log/gateway.json  # Tùy chọn

profiles:
  car:
    backend_url: "http://localhost:8080"
  motorcycle:
    backend_url: "http://localhost:8081"
```

### 7.2 Endpoint Gateway

| Endpoint    | Method | Mô tả                          |
| ----------- | ------ | ------------------------------- |
| `/query`    | POST   | Forward query tới backend       |
| `/info`     | GET    | Forward info tới backend        |
| `/profiles` | GET    | Liệt kê các profile available  |

### 7.3 Sử dụng

```bash
# Liệt kê profiles
curl -s http://localhost:50051/profiles
# → ["car", "motorcycle"]

# Query với profile
curl -s http://localhost:50051/query \
  -H "Content-Type: application/json" \
  -d '{
    "profile": "motorcycle",
    "from_lat": 21.0283, "from_lng": 105.8542,
    "to_lat": 20.9980, "to_lng": 105.8286
  }'

# Info về 1 profile
curl -s "http://localhost:50051/info?profile=motorcycle"
```

---

## 8. Định dạng Output

### 8.1 GeoJSON (mặc định)

Tuân chuẩn [RFC 7946](https://tools.ietf.org/html/rfc7946). Tọa độ thứ tự
`[longitude, latitude]`.

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[105.8542, 21.0283], [105.8530, 21.0270], ...]
    },
    "properties": {
      "distance_ms": 45230,
      "distance_m": 12567.3,
      "stroke": "#ff5500",
      "stroke-width": 10,
      "fill": "#ffaa00",
      "fill-opacity": 0.4
    }
  }]
}
```

Có thể mở trực tiếp trên [geojson.io](https://geojson.io) để xem tuyến đường
trên bản đồ.

### 8.2 JSON (legacy)

Tọa độ thứ tự `[latitude, longitude]`. Có thêm `path_nodes`.

```json
{
  "distance_ms": 45230,
  "distance_m": 12567.3,
  "path_nodes": [100, 205, 310, 415],
  "coordinates": [[21.0283, 105.8542], [21.0270, 105.8530], ...]
}
```

### 8.3 Ý nghĩa các trường

| Trường         | Kiểu     | Mô tả                                          |
| -------------- | -------- | ----------------------------------------------- |
| `graph_type`   | `string` | Loại đồ thị: `"normal"` hoặc `"line_graph"`       |
| `distance_ms`  | `u32`    | Thời gian di chuyển tính bằng milliseconds      |
| `distance_m`   | `f64`    | Khoảng cách Haversine tính bằng mét             |
| `path_nodes`   | `[u32]`  | Danh sách node ID trên đường đi (chỉ JSON)      |
| `coordinates`  | `[[f32]]`| Danh sách tọa độ các node trên đường đi        |
| `turns`        | `[Turn]` | Chú thích rẽ (line-graph mode, nếu có)          |
| `origin`       | `[f32]`  | Tọa độ snap của điểm đi (nếu query by coords)  |
| `destination`  | `[f32]`  | Tọa độ snap của điểm đến (nếu query by coords) |

---

## 9. Luồng nghiệp vụ thường gặp

### 9.1 Cập nhật bản đồ mới

```bash
# 1. Đặt file PBF mới vào Maps/
cp vietnam-latest.osm.pbf Maps/hanoi_new.osm.pbf

# 2. Chạy pipeline
bash scripts/run_pipeline.sh hanoi_new.osm.pbf motorcycle

# 3. Restart server với dữ liệu mới
# (kill server cũ, start lại trỏ tới hanoi_new_motorcycle/)
```

### 9.2 Thêm profile mới

```bash
# 1. Sinh đồ thị cho profile car
bash scripts/run_pipeline.sh hanoi.osm.pbf car

# 2. Khởi động thêm server
hanoi_server --graph-dir Maps/data/hanoi_car/line_graph \
  --original-graph-dir Maps/data/hanoi_car/graph \
  --line-graph --query-port 8082 --customize-port 9082

# 3. Cập nhật gateway.yaml thêm profile car
# 4. Restart gateway
```

### 9.3 Cập nhật trọng số real-time

```bash
# 1. Hệ thống giám sát giao thông tạo vector trọng số mới
python3 generate_live_weights.py > live_weights.bin

# 2. Upload trọng số
curl -X POST http://localhost:9080/customize --data-binary @live_weights.bin

# 3. Verify
curl -s http://localhost:8080/health
# → customization_active: false (đã xong)
```

### 9.4 Regression test sau khi thay đổi code

```bash
# 1. Benchmark bản hiện tại
bench_core --graph-dir Maps/data/hanoi_motorcycle/graph \
  --queries queries.json --output baseline.json

# 2. Thay đổi code, rebuild
cargo build --release --workspace

# 3. Benchmark bản mới
bench_core --graph-dir Maps/data/hanoi_motorcycle/graph \
  --queries queries.json --output current.json

# 4. So sánh
bench_report --baseline baseline.json --current current.json --threshold 10
```

---

## 10. Hiệu năng tham khảo

Đo trên mạng đường Hà Nội (motorcycle profile, line-graph mode):

### Thông số đồ thị

| Metric                | Đồ thị gốc  | Line graph   |
| --------------------- | ------------ | ------------ |
| Nodes                 | 929,366      | 1,943,051    |
| Edges                 | 1,942,872    | 4,396,227    |
| Turn restrictions     | 345          | —            |

### Hiệu năng thuật toán (bench_core)

| Operation        | Mean       | Throughput    |
| ---------------- | ---------- | ------------- |
| CCH build        | ~984 ms    | 1 lần/startup |
| Customize        | ~51 ms     | 19/s          |
| Query (node ID)  | ~11 µs     | ~91,000 qps   |
| Query (coords)   | ~14.5 ms   | ~6,900 qps    |
| KD-tree build    | ~480 ms    | 1 lần/startup |

### Hiệu năng HTTP (bench_server)

| Operation    | Mean   | p95    | p99     |
| ------------ | ------ | ------ | ------- |
| GET /info    | ~370 µs| ~440 µs| ~440 µs |
| POST /query  | ~4 ms  | ~8.3 ms| ~10 ms  |

> **Ghi chú**: `query_coords` chậm hơn `query_node_id` vì bao gồm snap GPS →
> edge (KD-tree search) + unpack path + tính Haversine distance.

---

## 11. Công cụ chẩn đoán (diagnose_turn)

Binary `diagnose_turn` dùng để debug turn restrictions tại một tọa độ cụ thể.

```bash
DIAG=$REPO/CCH-Hanoi/target/release/diagnose_turn

# Tìm tất cả nodes trong bán kính 50m từ tọa độ
$DIAG Maps/data/hanoi_motorcycle/graph --lat 21.0283 --lng 105.8542

# Mở rộng bán kính, bao gồm cả line graph
$DIAG Maps/data/hanoi_motorcycle/graph --lat 21.0283 --lng 105.8542 --radius 100 --line-graph
```

| Tham số        | Mặc định | Mô tả                                |
| -------------- | -------- | ------------------------------------- |
| `<graph_dir>`  | _(bắt buộc, positional)_ | Thư mục đồ thị          |
| `--lat`        | _(bắt buộc)_ | Vĩ độ tâm tìm kiếm               |
| `--lng`        | _(bắt buộc)_ | Kinh độ tâm tìm kiếm             |
| `--radius`     | `50.0`   | Bán kính tìm kiếm (mét)              |
| `--line-graph` | `false`  | Bao gồm thông tin line graph          |

Output bao gồm:
- Nodes trong bán kính
- Incoming/outgoing edges tại mỗi node
- Forbidden turn pairs liên quan
- Via-way restriction chains liên quan
