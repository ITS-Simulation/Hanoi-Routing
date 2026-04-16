# hanoi-gateway

API gateway chọn profile cho các server định tuyến Hà Nội. Chuyển tiếp request
tới backend `hanoi-server` phù hợp dựa trên tham số `profile`
(ví dụ: `car`, `motorcycle`).

## Kiến trúc

```
                     ┌─────────────────────────────┐
   HTTP clients ───▶ │  gateway (:50051)            │
                     │    POST /query?profile=...   │──▶ hanoi_server (ô tô)   :8080
                     │    POST /reset_weights?...   │──▶ hanoi_server (ô tô)   :8080
                     │    GET  /info?profile=...    │──▶ hanoi_server (xe máy)  :8081
                     │    GET  /status?profile=...  │
                     │    GET  /health?profile=...  │
                     │    GET  /ready?profile=...   │
                     │    GET  /profiles            │    ...
                     └─────────────────────────────┘
```

Gateway là một reverse proxy không trạng thái (stateless). Nó **không** phân
tích hay biến đổi request/response body — chuyển tiếp nguyên vẹn tới backend
được chọn bởi `profile`. Toàn bộ cấu hình backend nằm trong một file YAML duy
nhất.

## CLI

```
hanoi_gateway --config gateway.yaml [--port 50051]
```

| Tuỳ chọn | Mặc định | Mô tả |
| --- | --- | --- |
| `--config` | `gateway.yaml` | Đường dẫn file cấu hình YAML |
| `--port` | *(từ config)* | Ghi đè cổng lắng nghe |

## Cấu hình

```yaml
port: 50051
backend_timeout_secs: 30    # 0 để tắt; mặc định 30
log_format: pretty          # pretty | full | compact | tree | json
# log_file: /var/log/gw.json

profiles:
  car:
    backend_url: "http://localhost:8080"
  motorcycle:
    backend_url: "http://localhost:8081"
```

Map `profiles` là nguồn sự thật duy nhất cho các routing profile mà gateway
cung cấp. Request với profile không hợp lệ bị từ chối với HTTP 400 kèm danh
sách các profile khả dụng.

## Tham chiếu API

### `POST /query?profile=<tên>`

Chuyển tiếp truy vấn tuyến đường tới backend được chọn. JSON body được
chuyển tiếp nguyên vẹn.

**Query parameters:**

| Tham số | Bắt buộc | Mô tả |
| --- | --- | --- |
| `profile` | **có** | Profile định tuyến (phải khớp key trong config) |
| `format` | không | Chuyển tiếp tới backend (`json` cho JSON thuần) |
| `colors` | không | Chuyển tiếp tới backend (bật simplestyle-spec) |
| `alternatives` | không | Chuyển tiếp tới backend để yêu cầu tuyến thay thế |
| `stretch` | không | Chuyển tiếp tới backend để điều chỉnh ngưỡng độ vòng |

**Ví dụ:**
```bash
curl -X POST "http://localhost:50051/query?profile=car" \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.028,"from_lng":105.854,"to_lat":21.007,"to_lng":105.820}'
```

**Lỗi:**

| Status | Nguyên nhân |
| --- | --- |
| 400 | Profile không xác định hoặc thiếu |
| 502 | Backend không thể kết nối hoặc trả về JSON không hợp lệ |

### `GET /info?profile=<tên>`

Chuyển tiếp truy vấn metadata tới backend được chọn. Khi không có `profile`,
mặc định dùng profile đầu tiên theo thứ tự bảng chữ cái.

### `GET /status?profile=<tên>`

Alias của gateway cho `GET /info?profile=<tên>`. Hữu ích khi client coi
`/info` như một endpoint trạng thái.

### `GET /health?profile=<tên>`

Chuyển tiếp phản hồi health của backend được chọn. Khi không có `profile`,
mặc định dùng profile đầu tiên theo thứ tự bảng chữ cái.

### `GET /ready?profile=<tên>`

Chuyển tiếp phản hồi readiness của backend được chọn. Khi không có `profile`,
mặc định dùng profile đầu tiên theo thứ tự bảng chữ cái.

### `POST /reset_weights?profile=<tên>`

Chuyển tiếp yêu cầu khôi phục trọng số nền tới backend được chọn.

### `GET /profiles`

Liệt kê tất cả routing profile khả dụng.

```json
{ "profiles": ["car", "motorcycle"] }
```

## Build

```bash
cargo build --release -p hanoi-gateway
```
