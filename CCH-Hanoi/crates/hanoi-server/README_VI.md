# hanoi-server

Server định tuyến CCH cho mạng lưới đường bộ Hà Nội. Cung cấp API truy vấn
JSON và API tùy chỉnh trọng số (binary) trên hai cổng riêng biệt.

## Kiến trúc

```
                    ┌──────────────────────────────────────────┐
  HTTP clients ───▶ │  cổng truy vấn (:8080)                   │
                    │    POST /query          truy vấn tuyến   │
                    │    POST /reset_weights  khôi phục gốc    │
                    │    GET  /info           metadata đồ thị  │
                    │    GET  /health         uptime + thống kê│
                    │    GET  /ready          engine còn sống? │
                    │  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
                    │  Chỉ UI (feature = "ui", --serve-ui):    │
                    │    POST /evaluate_routes                  │
                    │    GET  /traffic_overlay                  │
                    │    GET  /camera_overlay                   │
                    │    GET  /  /ui  /assets/*                 │
                    ├──────────────────────────────────────────┤
  Pipeline tools ─▶ │  cổng tùy chỉnh (:9080)                  │
                    │    POST /customize      upload trọng số  │
                    └──────────┬───────────────────────────────┘
                               │ mpsc (truy vấn)
                               │ watch (trọng số)
                               ▼
                    ┌──────────────────────┐
                    │  Background engine   │
                    │  thread (1 thread)   │
                    │  - CCH customization │
                    │  - Dijkstra queries  │
                    └──────────────────────┘
```

Server chạy hai listener Axum trên hai cổng TCP riêng. Handler truy vấn gửi
request tới background engine thread qua `mpsc`. Cập nhật trọng số đi qua
kênh `watch` — ngữ nghĩa "bản mới nhất thắng", không có hàng đợi.

## Chế độ đồ thị

| Cờ | Engine | Mô tả |
| --- | --- | --- |
| *(mặc định)* | `CchContext` / `QueryEngine` | CCH chuẩn theo node |
| `--line-graph` | `LineGraphCchContext` / `LineGraphQueryEngine` | CCH có hướng mở rộng theo lượt rẽ (cần `--original-graph-dir`) |

## CLI

```
hanoi_server --graph-dir Maps/data/hanoi_car/graph [OPTIONS]
```

| Tuỳ chọn | Mặc định | Mô tả |
| --- | --- | --- |
| `--graph-dir` | *(bắt buộc)* | Đường dẫn thư mục đồ thị định dạng RoutingKit |
| `--original-graph-dir` | *(không)* | Thư mục đồ thị gốc (bắt buộc với `--line-graph`) |
| `--query-port` | `8080` | Cổng API truy vấn |
| `--customize-port` | `9080` | Cổng API tùy chỉnh |
| `--line-graph` | `false` | Bật chế độ line-graph |
| `--serve-ui` | `false` | Phục vụ UI xem tuyến đường (cần feature `ui`) |
| `--camera-config` | `CCH_Data_Pipeline/config/mvp_camera.yaml` | YAML camera (cần feature `ui`) |
| `--log-format` | `pretty` | `pretty` / `full` / `compact` / `tree` / `json` |
| `--log-dir` | *(không)* | Bật ghi log JSON xoay vòng hàng ngày |

## Tham chiếu API

### Cổng truy vấn (mặc định 8080)

#### `POST /query`

Tìm tuyến đường giữa hai điểm. Hỗ trợ truy vấn theo tọa độ hoặc theo ID node.

**Request body:**
```json
{
  "from_lat": 21.028, "from_lng": 105.854,
  "to_lat": 21.007,   "to_lng": 105.820
}
```

Hoặc theo node ID:
```json
{ "from_node": 12345, "to_node": 67890 }
```

**Query parameters:**

| Tham số | Hiệu quả |
| --- | --- |
| `format=json` | Phản hồi JSON thuần (mặc định: GeoJSON FeatureCollection) |
| `colors` | Thêm simplestyle-spec stroke/fill vào kết quả GeoJSON |
| `alternatives=N` | Số tuyến thay thế cần trả về (0 = chỉ tuyến ngắn nhất) |
| `stretch=F` | Hệ số giãn địa lý tối đa cho tuyến thay thế (mặc định: `1.25` = dài hơn 25% so với tuyến ngắn nhất). Tuyến ứng viên có chiều dài địa lý vượt quá `khoảng_cách_ngắn_nhất * stretch` sẽ bị loại |

##### Định dạng phản hồi

Phản hồi mặc định là **GeoJSON FeatureCollection**. Dùng `?format=json`
để nhận JSON thuần.

> **Quy ước tọa độ:** Tọa độ GeoJSON theo RFC 7946 `[lng, lat]`.
> JSON thuần dùng `[lat, lng]` (khớp với format request).

**Phản hồi GeoJSON** (mặc định):

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[105.854, 21.028], [105.840, 21.015], [105.820, 21.007]]
    },
    "properties": {
      "source": "hanoi_server",
      "export_version": 1,
      "graph_type": "normal",
      "distance_ms": 324000,
      "distance_m": 5400.0,
      "path_nodes": [1, 42, 99],
      "route_arc_ids": [10, 20],
      "weight_path_ids": [10, 20],
      "origin": [21.028, 105.854],
      "destination": [21.007, 105.820],
      "turns": [
        {
          "direction": "Left",
          "angle_degrees": 87.3,
          "distance_to_next_m": 420.5
        }
      ]
    }
  }]
}
```

Với `?colors`, các thuộc tính simplestyle-spec được thêm vào mỗi feature:

```json
{
  "stroke": "#ff5500",
  "stroke-width": 10,
  "fill": "#ffaa00",
  "fill-opacity": 0.4
}
```

**Phản hồi không tìm thấy đường** (GeoJSON):

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": null,
    "properties": { "distance_ms": null, "distance_m": null }
  }]
}
```

**Phản hồi JSON thuần** (`?format=json`):

```json
{
  "graph_type": "normal",
  "distance_ms": 324000,
  "distance_m": 5400.0,
  "path_nodes": [1, 42, 99],
  "route_arc_ids": [10, 20],
  "weight_path_ids": [10, 20],
  "coordinates": [[21.028, 105.854], [21.015, 105.840], [21.007, 105.820]],
  "turns": [
    {
      "direction": "Left",
      "angle_degrees": 87.3,
      "distance_to_next_m": 420.5
    }
  ],
  "origin": [21.028, 105.854],
  "destination": [21.007, 105.820]
}
```

Các trường `route_arc_ids`, `weight_path_ids`, và `turns` được bỏ qua khi
rỗng. `origin` và `destination` được bỏ qua cho truy vấn theo node ID.

##### Phản hồi đa tuyến (`?alternatives=N`)

Khi `alternatives` > 0, phản hồi chứa nhiều feature (GeoJSON) hoặc mảng
các đối tượng route (JSON thuần).

**GeoJSON đa tuyến** — mỗi feature có thêm thuộc tính `route_index`
(0 = ngắn nhất). Với `?colors`, mỗi tuyến nhận một màu nét riêng từ bảng
10 màu (`#ff5500`, `#0055ff`, `#00aa44`, ...), tuyến chính có
`stroke-width: 10`, tuyến thay thế có `6`.

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [[105.854, 21.028], [105.840, 21.015], [105.820, 21.007]]
      },
      "properties": {
        "source": "hanoi_server",
        "export_version": 1,
        "graph_type": "normal",
        "route_index": 0,
        "distance_ms": 324000,
        "distance_m": 5400.0,
        "path_nodes": [1, 42, 99],
        "route_arc_ids": [10, 20],
        "weight_path_ids": [10, 20],
        "origin": [21.028, 105.854],
        "destination": [21.007, 105.820],
        "stroke": "#ff5500",
        "stroke-width": 10,
        "fill": "#ff5500",
        "fill-opacity": 0.3
      }
    },
    {
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [[105.854, 21.028], [105.850, 21.020], [105.820, 21.007]]
      },
      "properties": {
        "source": "hanoi_server",
        "export_version": 1,
        "graph_type": "normal",
        "route_index": 1,
        "distance_ms": 340000,
        "distance_m": 5800.0,
        "path_nodes": [1, 55, 99],
        "route_arc_ids": [10, 30],
        "weight_path_ids": [10, 30],
        "origin": [21.028, 105.854],
        "destination": [21.007, 105.820],
        "stroke": "#0055ff",
        "stroke-width": 6,
        "fill": "#0055ff",
        "fill-opacity": 0.3
      }
    }
  ]
}
```

**JSON thuần đa tuyến** (`?format=json&alternatives=2`) — trả về mảng JSON
các đối tượng route:

```json
[
  {
    "graph_type": "normal",
    "distance_ms": 324000,
    "distance_m": 5400.0,
    "path_nodes": [1, 42, 99],
    "route_arc_ids": [10, 20],
    "weight_path_ids": [10, 20],
    "coordinates": [[21.028, 105.854], [21.015, 105.840], [21.007, 105.820]],
    "origin": [21.028, 105.854],
    "destination": [21.007, 105.820]
  },
  {
    "graph_type": "normal",
    "distance_ms": 340000,
    "distance_m": 5800.0,
    "path_nodes": [1, 55, 99],
    "route_arc_ids": [10, 30],
    "weight_path_ids": [10, 30],
    "coordinates": [[21.028, 105.854], [21.020, 105.850], [21.007, 105.820]],
    "origin": [21.028, 105.854],
    "destination": [21.007, 105.820]
  }
]
```

##### Chú thích lượt rẽ (Turn annotations)

Mỗi chú thích trong mảng `turns` chứa:

| Trường | Kiểu | Mô tả |
| --- | --- | --- |
| `direction` | string | `Straight`, `SlightLeft`, `SlightRight`, `Left`, `Right`, `SharpLeft`, `SharpRight`, `UTurn`, `RoundaboutEnter`, `RoundaboutExitStraight`, `RoundaboutExitRight`, `RoundaboutExitLeft` |
| `angle_degrees` | f64 | Góc rẽ có dấu [-180, 180]. Dương = trái, âm = phải |
| `distance_to_next_m` | f64 | Khoảng cách (mét) tới điểm rẽ tiếp theo (hoặc cuối tuyến) |

##### Phản hồi lỗi

**400 Bad Request** — xác thực tọa độ thất bại:

```json
{
  "error": "coordinate_validation_failed",
  "message": "origin is outside the graph bounding box",
  "details": {
    "reason": "out_of_bounds",
    "label": "origin",
    "lat": 22.5,
    "lng": 105.8,
    "bbox": { "min_lat": 20.9, "max_lat": 21.1, "min_lng": 105.7, "max_lng": 105.9 },
    "padding_m": 5000.0
  }
}
```

Các lý do từ chối: `non_finite`, `invalid_range`, `out_of_bounds`, `snap_too_far`.

#### `POST /reset_weights`

Khôi phục trọng số travel-time gốc. Không cần request body.

#### `GET /info`

```json
{
  "graph_type": "normal",
  "num_nodes": 123456,
  "num_edges": 456789,
  "customization_active": false,
  "bbox": { "min_lat": 20.9, "max_lat": 21.1, "min_lng": 105.7, "max_lng": 105.9 }
}
```

#### `GET /health`

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "total_queries_processed": 42,
  "customization_active": false
}
```

#### `GET /ready`

Trả về `200 {"ready": true}` hoặc `503 {"ready": false}` nếu engine thread
đã chết.

### Cổng tùy chỉnh (mặc định 9080)

#### `POST /customize`

Upload vector trọng số mới. Body: raw little-endian `[u32; num_edges]`,
có thể nén gzip (`RequestDecompressionLayer` tự giải nén).
Kích thước body tối đa: 64 MiB.

**Kiểm tra hợp lệ:**
- Số byte phải bằng `num_edges * 4`
- Tất cả trọng số phải <= `INFINITY` (2^31 - 1)

**Phản hồi:** `200 {"accepted": true, "message": "customization queued"}`

Trả về **trước khi** quá trình customization hoàn tất. Poll `GET /info` và
theo dõi `customization_active` để biết khi nào xong.

### Endpoint chỉ dành cho UI (feature = "ui", --serve-ui)

| Endpoint | Method | Mô tả |
| --- | --- | --- |
| `/evaluate_routes` | POST | Đánh giá tuyến GeoJSON nhập vào với trọng số hiện tại |
| `/traffic_overlay` | GET | Các đoạn heatmap giao thông lọc theo viewport |
| `/camera_overlay` | GET | Điểm đánh dấu camera lọc theo viewport |
| `/` `/ui` `/assets/*` | GET | UI xem tuyến đường đi kèm |

## Ngữ nghĩa kênh watch

Vòng lặp engine dùng `borrow_and_update()` trên watch receiver, luôn thấy
vector trọng số mới nhất. Nếu `/customize` được gọi hai lần trong khi
customization đang chạy, vector cũ bị thay thế âm thầm. Đây là thiết kế
có chủ đích — engine nên dùng trọng số mới nhất, không phải phát lại trạng
thái trung gian cũ.

## Tắt server an toàn (Graceful shutdown)

SIGINT hoặc SIGTERM kích hoạt graceful shutdown qua broadcast channel. Cả hai
listener chờ xử lý xong các request đang bay. Giới hạn cứng 30 giây sẽ
force-kill process nếu shutdown bị kẹt.

## Build

```bash
# Chỉ API (mặc định)
cargo build --release -p hanoi-server

# Có UI đi kèm
cargo build --release -p hanoi-server --features ui
```
