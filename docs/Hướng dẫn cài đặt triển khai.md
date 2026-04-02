# Hướng dẫn Cài đặt & Triển khai

Tài liệu hướng dẫn cài đặt, build, triển khai toàn bộ hệ thống định tuyến
CCH-Hanoi từ mã nguồn trên máy mới.

---

## Mục lục

1. [Yêu cầu hệ thống](#1-yêu-cầu-hệ-thống)
2. [Cài đặt môi trường (WSL Ubuntu)](#2-cài-đặt-môi-trường-wsl-ubuntu)
3. [Build toàn bộ hệ thống](#3-build-toàn-bộ-hệ-thống)
4. [Sinh dữ liệu đồ thị (Pipeline)](#4-sinh-dữ-liệu-đồ-thị-pipeline)
5. [Khởi động server](#5-khởi-chạy-server)
6. [Kiểm tra hệ thống](#6-kiểm-tra-hệ-thống)
7. [Triển khai Gateway đa profile](#7-triển-khai-gateway-đa-profile)
8. [Triển khai production](#8-triển-khai-production)
9. [Cấu trúc thư mục dữ liệu](#9-cấu-trúc-thư-mục-dữ-liệu)
10. [Xử lý sự cố](#10-xử-lý-sự-cố)

---

## 1. Yêu cầu hệ thống

### Phần cứng tối thiểu

| Tài nguyên | Yêu cầu tối thiểu    | Khuyến nghị          |
| ---------- | --------------------- | -------------------- |
| CPU        | 4 cores               | 8+ cores             |
| RAM        | 8 GB                  | 16+ GB               |
| Disk       | 10 GB trống           | 20+ GB (SSD)         |
| OS         | Ubuntu 22.04+ / WSL 2 | Ubuntu 22.04 native  |

### Phần mềm cần thiết

| Phần mềm     | Phiên bản tối thiểu | Mục đích                           |
| ------------- | -------------------- | ---------------------------------- |
| GCC / G++     | 11+                  | Build RoutingKit, CCH-Generator, IFC |
| CMake         | 3.16+                | Build CCH-Generator, IFC           |
| Python 3      | 3.8+                 | Sinh Makefile cho RoutingKit       |
| Rust nightly  | 1.85+                | Build CCH-Hanoi, rust_road_router  |
| zlib          | —                    | Đọc file .osm.pbf (nén gzip)      |
| protobuf-dev  | —                    | Parse protobuf trong OSM           |
| TBB           | —                    | Parallel cho InertialFlowCutter    |
| readline      | —                    | Console của InertialFlowCutter     |
| OpenSSL       | —                    | HTTP client trong CCH-Hanoi        |
| dos2unix      | —                    | Sửa line endings (trên WSL/Windows)|
| curl          | —                    | Test HTTP endpoint                 |

---

## 2. Cài đặt môi trường (WSL Ubuntu)

### 2.1 Cài đặt build tools

```bash
sudo apt update && sudo apt install -y \
  build-essential cmake python3 pkg-config \
  zlib1g-dev libprotobuf-dev protobuf-compiler \
  libtbb-dev libreadline-dev libssl-dev \
  dos2unix curl
```

### 2.2 Cài đặt Rust nightly

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup default nightly
```

Kiểm tra:

```bash
rustc --version    # rustc 1.85.0-nightly (hoặc cao hơn)
cargo --version
```

### 2.3 Sửa line endings (nếu source nằm trên ổ Windows/NTFS)

```bash
REPO=/mnt/c/ITS/Routing/Hanoi-Routing  # sửa lại đường dẫn phù hợp

# Sửa tất cả scripts
find "$REPO/scripts" -type f -exec dos2unix {} \;
find "$REPO/CCH-Generator/scripts" -type f -exec dos2unix {} \;
dos2unix "$REPO/RoutingKit/generate_make_file"
dos2unix "$REPO/rust_road_router/flow_cutter_cch_order.sh"
dos2unix "$REPO/rust_road_router/flow_cutter_cch_cut_order.sh"
dos2unix "$REPO/rust_road_router/flow_cutter_cch_cut_reorder.sh"
```

> **Lưu ý**: Bước này bắt buộc nếu repo nằm trên ổ NTFS mount qua WSL. Bỏ qua
> nếu build trên Linux native.

---

## 3. Build toàn bộ hệ thống

Thứ tự build quan trọng vì có dependency chain:  
**RoutingKit → CCH-Generator → InertialFlowCutter → CCH-Hanoi**

### 3.1 Build RoutingKit (C++)

```bash
cd "$REPO/RoutingKit"
python3 generate_make_file
make -j"$(nproc)"
```

Kiểm tra: thư mục `lib/` có `libroutingkit.so` và `libroutingkit.a`.  
Binary quan trọng: `bin/conditional_turn_extract`.

### 3.2 Build CCH-Generator (C++)

```bash
cd "$REPO/CCH-Generator"
mkdir -p build lib
cmake -S . -B build \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_RUNTIME_OUTPUT_DIRECTORY="$(pwd)/lib"
cmake --build build -j"$(nproc)"
```

Kiểm tra: `lib/cch_generator` và `lib/validate_graph` tồn tại.

### 3.3 Build InertialFlowCutter (C++)

```bash
cd "$REPO/rust_road_router/lib/InertialFlowCutter"
mkdir -p build && cd build
cmake -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF ..
make -j"$(nproc)" console
```

Kiểm tra: `build/console` tồn tại. File này được các script
`flow_cutter_cch_order.sh` gọi tự động.

### 3.4 Build CCH-Hanoi (Rust)

```bash
export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"
source ~/.cargo/env
cd "$REPO/CCH-Hanoi"
cargo build --release --workspace
```

Kiểm tra: các binary sau tồn tại trong `target/release/`:

| Binary                | Chức năng                                  |
| --------------------- | ------------------------------------------ |
| `hanoi_server`        | HTTP routing server (query + customize)    |
| `hanoi_gateway`       | API gateway proxy đa profile               |
| `cch-hanoi`           | CLI offline (query, info)                  |
| `generate_line_graph` | Sinh đồ thị turn-expanded                  |
| `bench_core`          | Benchmark thuật toán core                  |
| `bench_server`        | Benchmark HTTP server                      |
| `bench_report`        | So sánh & phân tích benchmark              |

### 3.5 Script tự động build (tùy chọn)

Có thể chạy script tự động toàn bộ các bước trên:

```bash
bash "$REPO/scripts/wsl_setup.sh"
```

---

## 4. Sinh dữ liệu đồ thị (Pipeline)

Pipeline 5 pha chuyển file `.osm.pbf` thành dữ liệu đồ thị sẵn sàng cho server.

### 4.1 Chuẩn bị

Đặt file bản đồ OSM PBF vào thư mục `Maps/`:

```
Maps/
└── hanoi.osm.pbf      # Bản đồ Hà Nội
```

### 4.2 Chạy pipeline tự động

```bash
export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"
bash "$REPO/scripts/run_pipeline.sh" hanoi.osm.pbf motorcycle
```

Cú pháp:
```
run_pipeline.sh <tên_file_pbf> <profile>
```

- `<tên_file_pbf>`: tên file trong `Maps/` (ví dụ: `hanoi.osm.pbf`)
- `<profile>`: `car` hoặc `motorcycle`

### 4.3 Chi tiết 5 pha

```
Pha 1: Sinh đồ thị cơ sở
  cch_generator <pbf> <output> --profile motorcycle
  → first_out, head, travel_time, latitude, longitude, way, ...
  → validate_graph kiểm tra tính toàn vẹn

Pha 2: Trích xuất turn restrictions có điều kiện
  conditional_turn_extract <pbf> <graph_dir> <output> --profile motorcycle
  → conditional_turn_from_arc, conditional_turn_to_arc, conditional_turn_time_windows
  → forbidden_turn_from_arc, forbidden_turn_to_arc (cập nhật)

Pha 3: Sinh đồ thị turn-expanded (line graph)
  generate_line_graph <graph_dir> <output>/line_graph
  → line_graph/first_out, head, travel_time, latitude, longitude
  → validate_graph --turn-expanded kiểm tra

Pha 4: Sinh node ordering cho đồ thị chính
  flow_cutter_cch_order.sh      → perms/cch_perm
  flow_cutter_cch_cut_order.sh  → perms/cch_perm_cuts
  flow_cutter_cch_cut_reorder.sh → perms/cch_perm_cuts_reorder

Pha 5: Sinh node ordering cho line graph
  flow_cutter_cch_order.sh <line_graph_dir>
  → line_graph/perms/cch_perm
```

### 4.4 Thời gian tham khảo (Hanoi, ~929K nodes)

| Pha | Thời gian xấp xỉ |
| --- | ----------------- |
| 1   | 1-2 phút          |
| 2   | 1-2 phút          |
| 3   | 30 giây           |
| 4   | 3-5 phút          |
| 5   | 3-5 phút          |
| **Tổng** | **~10-15 phút** |

### 4.5 Kết quả

Thư mục output sinh ra:

```
Maps/data/hanoi_motorcycle/
├── graph/
│   ├── first_out                # CSR offsets (Vec<u32>)
│   ├── head                     # CSR targets (Vec<u32>)
│   ├── travel_time              # Trọng số ms (Vec<u32>)
│   ├── latitude                 # Vĩ độ (Vec<f32>)
│   ├── longitude                # Kinh độ (Vec<f32>)
│   ├── way                      # Way ID (Vec<u32>)
│   ├── forbidden_turn_from_arc  # Turn restrictions
│   ├── forbidden_turn_to_arc
│   ├── conditional_turn_*       # Conditional turns
│   └── perms/
│       ├── cch_perm             # Node ordering
│       ├── cch_perm_cuts
│       └── cch_perm_cuts_reorder
└── line_graph/
    ├── first_out
    ├── head
    ├── travel_time
    ├── latitude
    ├── longitude
    └── perms/
        └── cch_perm
```

---

## 5. Khởi chạy server

### 5.1 Chế độ Line Graph (khuyến nghị)

```bash
export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"

$REPO/CCH-Hanoi/target/release/hanoi_server \
  --graph-dir     Maps/data/hanoi_motorcycle/line_graph \
  --original-graph-dir Maps/data/hanoi_motorcycle/graph \
  --line-graph \
  --query-port     8081 \
  --customize-port 9081 \
  --log-format     pretty
```

Hoặc dùng script có sẵn:

```bash
bash "$REPO/scripts/start_server.sh" motorcycle
```

### 5.2 Chế độ Normal (không turn restrictions)

```bash
$REPO/CCH-Hanoi/target/release/hanoi_server \
  --graph-dir     Maps/data/hanoi_motorcycle/graph \
  --query-port     8080 \
  --customize-port 9080
```

### 5.3 Tham số CLI của hanoi_server

| Tham số                | Mặc định | Mô tả                                          |
| ---------------------- | -------- | ----------------------------------------------- |
| `--graph-dir`          | _(bắt buộc)_ | Đường dẫn tới thư mục đồ thị                |
| `--original-graph-dir` | _(không)_ | Thư mục đồ thị gốc (bắt buộc khi `--line-graph`) |
| `--line-graph`         | `false`  | Bật chế độ line graph (DirectedCCH)             |
| `--query-port`         | `8080`   | Cổng REST API cho query                         |
| `--customize-port`     | `9080`   | Cổng cho upload trọng số tùy chỉnh             |
| `--log-format`         | `pretty` | Định dạng log: `pretty`, `full`, `compact`, `tree`, `json` |
| `--log-dir`            | _(không)_ | Thư mục ghi log file (rotation hàng ngày, JSON) |

### 5.4 Biến môi trường

| Biến              | Mô tả                                              |
| ----------------- | --------------------------------------------------- |
| `LD_LIBRARY_PATH` | Phải chứa `RoutingKit/lib` để load `libroutingkit.so` |
| `RUST_LOG`        | Điều chỉnh mức log, ví dụ: `info`, `debug`, `warn,tower_http=debug` |

### 5.5 Xác minh server đã sẵn sàng

```bash
# Health check
curl -s http://localhost:8081/health | python3 -m json.tool

# Thông tin đồ thị
curl -s http://localhost:8081/info | python3 -m json.tool

# Readiness
curl -s http://localhost:8081/ready
```

---

## 6. Kiểm tra hệ thống

### 6.1 Chạy bộ test tự động (17 test cases)

```bash
bash "$REPO/scripts/test_queries.sh" motorcycle
```

Bộ test gồm 3 pha:

- **Pha 1: CLI Tests** (6 tests) — Chạy offline, không cần server
  - Info, long/short/cross-city/reverse routes, JSON output
- **Pha 2: Server Tests** (8 tests) — Cần server đang chạy
  - Health, info, ready, coordinate queries, edge cases (same point, out-of-boundary)
- **Pha 3: Benchmark Tests** (3 tests) — Sinh query ngẫu nhiên, benchmark core + server

Kết quả lưu tại `Maps/data/hanoi_motorcycle/test_results/`.

### 6.2 Test thủ công qua CLI

```bash
CLI=$REPO/CCH-Hanoi/target/release/cch-hanoi
export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"

# Xem thông tin đồ thị
$CLI info --data-dir Maps/data/hanoi_motorcycle

# Query bằng tọa độ (line-graph mode)
$CLI query \
  --data-dir Maps/data/hanoi_motorcycle --line-graph \
  --from-lat 21.0283  --from-lng 105.8542 \
  --to-lat   20.99809 --to-lng   105.8286 \
  --output-format geojson --demo

# Query bằng node ID
$CLI query \
  --data-dir Maps/data/hanoi_motorcycle --line-graph \
  --from-node 12345 --to-node 67890
```

### 6.3 Test thủ công qua HTTP

```bash
# Query tọa độ (GeoJSON + colors)
curl -s http://localhost:8081/query?colors \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.0283,"from_lng":105.8542,"to_lat":20.99809,"to_lng":105.8286}' \
  | python3 -m json.tool

# Query node ID
curl -s http://localhost:8081/query \
  -H "Content-Type: application/json" \
  -d '{"from_node":12345,"to_node":67890}'

# Query JSON format (thay vì GeoJSON mặc định)
curl -s "http://localhost:8081/query?format=json" \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.0283,"from_lng":105.8542,"to_lat":20.99809,"to_lng":105.8286}'
```

### 6.4 Benchmark hiệu năng

```bash
BENCH_CORE=$REPO/CCH-Hanoi/target/release/bench_core
BENCH_SERVER=$REPO/CCH-Hanoi/target/release/bench_server
BENCH_REPORT=$REPO/CCH-Hanoi/target/release/bench_report

# Sinh query ngẫu nhiên
$BENCH_CORE --graph-dir Maps/data/hanoi_motorcycle/graph \
  --generate-queries 10000 --save-queries queries.json

# Benchmark core (CCH build, customize, KD-tree, query)
$BENCH_CORE --graph-dir Maps/data/hanoi_motorcycle/graph \
  --queries queries.json --iterations 10 \
  --output core_results.json

# Benchmark server (cần server đang chạy)
$BENCH_SERVER --url http://localhost:8081 \
  --query-file queries.json --queries 1000

# Benchmark server với concurrent load
$BENCH_SERVER --url http://localhost:8081 \
  --query-file queries.json --queries 1000 --concurrency 8

# So sánh 2 bản benchmark (phát hiện regression)
$BENCH_REPORT --baseline old_results.json --current new_results.json --threshold 10
```

---

## 7. Triển khai Gateway đa profile

Gateway cho phép expose nhiều profile (car, motorcycle) qua 1 endpoint duy nhất.

### 7.1 Tạo file cấu hình gateway.yaml

```yaml
port: 50051
backend_timeout_secs: 30
log_format: pretty

profiles:
  car:
    backend_url: "http://localhost:8080"
  motorcycle:
    backend_url: "http://localhost:8081"
```

### 7.2 Khởi động

Khởi động 2 server backend trước:

```bash
# Terminal 1: car server (normal mode)
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --query-port 8080 --customize-port 9080

# Terminal 2: motorcycle server (line-graph mode)
hanoi_server --graph-dir Maps/data/hanoi_motorcycle/line_graph \
  --original-graph-dir Maps/data/hanoi_motorcycle/graph \
  --line-graph --query-port 8081 --customize-port 9081

# Terminal 3: gateway
hanoi_gateway --config gateway.yaml
```

### 7.3 Sử dụng Gateway

```bash
# Liệt kê profiles
curl -s http://localhost:50051/profiles

# Query qua gateway (thêm field "profile")
curl -s http://localhost:50051/query \
  -H "Content-Type: application/json" \
  -d '{"profile":"motorcycle","from_lat":21.028,"from_lng":105.854,"to_lat":20.998,"to_lng":105.828}'

# Info về 1 profile
curl -s "http://localhost:50051/info?profile=motorcycle"
```

### 7.4 Tham số CLI của hanoi_gateway

| Tham số    | Mặc định       | Mô tả                       |
| ---------- | -------------- | ---------------------------- |
| `--config` | `gateway.yaml` | Đường dẫn tới file cấu hình |
| `--port`   | _(từ config)_  | Override port trong config   |

---

## 8. Triển khai production

### 8.1 Systemd service (Linux)

Tạo file `/etc/systemd/system/hanoi-routing.service`:

```ini
[Unit]
Description=Hanoi CCH Routing Server
After=network.target

[Service]
Type=simple
User=routing
WorkingDirectory=/opt/routing
Environment=LD_LIBRARY_PATH=/opt/routing/RoutingKit/lib
Environment=RUST_LOG=info,tower_http=debug
ExecStart=/opt/routing/CCH-Hanoi/target/release/hanoi_server \
  --graph-dir /opt/routing/Maps/data/hanoi_motorcycle/line_graph \
  --original-graph-dir /opt/routing/Maps/data/hanoi_motorcycle/graph \
  --line-graph \
  --query-port 8081 \
  --customize-port 9081 \
  --log-dir /var/log/hanoi-routing \
  --log-format compact
Restart=on-failure
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

Kích hoạt:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now hanoi-routing
sudo systemctl status hanoi-routing
journalctl -u hanoi-routing -f
```

### 8.2 Reverse proxy (nginx)

```nginx
upstream routing_query {
    server 127.0.0.1:8081;
}

server {
    listen 443 ssl;
    server_name routing.example.com;

    ssl_certificate     /etc/ssl/certs/routing.pem;
    ssl_certificate_key /etc/ssl/private/routing.key;

    location /api/route/ {
        proxy_pass http://routing_query/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_read_timeout 30s;
    }
}
```

### 8.3 Health check & monitoring

Server expose 3 endpoint giám sát:

| Endpoint  | Method | Mô tả                                | HTTP Code       |
| --------- | ------ | ------------------------------------- | --------------- |
| `/health` | GET    | Uptime, số query đã xử lý            | Luôn 200        |
| `/ready`  | GET    | Engine thread còn sống không          | 200 hoặc 503    |
| `/info`   | GET    | Metadata đồ thị (nodes, edges, bbox) | 200             |

Dùng cho Kubernetes liveness/readiness probes:

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 8081
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /ready
    port: 8081
  initialDelaySeconds: 30
  periodSeconds: 5
```

### 8.4 Graceful shutdown

Server xử lý `SIGTERM` và `SIGINT`:
- Ngừng nhận connection mới
- Đợi request đang xử lý hoàn tất (tối đa 30 giây)
- Tự tắt sau 30 giây nếu vẫn còn request đang chạy

---

## 9. Cấu trúc thư mục dữ liệu

### 9.1 File format

Tất cả file đồ thị đều là **raw binary, không header**. Kích thước element suy
ra từ file size:

| File          | Kiểu dữ liệu | Kích thước/element | Nội dung            |
| ------------- | ------------- | ------------------ | ------------------- |
| `first_out`   | `u32`         | 4 bytes            | CSR offsets (n+1)   |
| `head`        | `u32`         | 4 bytes            | CSR targets (m)     |
| `travel_time` | `u32`         | 4 bytes            | Trọng số ms (m)     |
| `latitude`    | `f32`         | 4 bytes            | Vĩ độ node (n)      |
| `longitude`   | `f32`         | 4 bytes            | Kinh độ node (n)     |
| `cch_perm`    | `u32`         | 4 bytes            | Node ordering (n)   |

### 9.2 Đơn vị trọng số

- `travel_time` luôn là **milliseconds** (ms)
- Công thức chuyển đổi OSM: `geo_distance[m] × 18000 / speed[km/h] / 5`
- Metadata: `tt_units_per_s = 1000`
- Giá trị INFINITY: `u32::MAX / 2 = 2,147,483,647` — không được dùng làm trọng số

### 9.3 Kiểm tra kích thước đồ thị

```bash
# Nhanh: đếm nodes và edges
python3 -c "
import os, sys
d = sys.argv[1]
n = os.path.getsize(f'{d}/first_out') // 4 - 1
m = os.path.getsize(f'{d}/head') // 4
print(f'Nodes: {n:,}  Edges: {m:,}')
" Maps/data/hanoi_motorcycle/graph

# Kết quả mong đợi (Hanoi motorcycle):
# Nodes: 929,366  Edges: 1,942,872
```

---

## 10. Xử lý sự cố

### Lỗi thường gặp

| Triệu chứng | Nguyên nhân | Giải pháp |
| ------------ | ----------- | --------- |
| `libroutingkit.so: cannot open shared object` | Thiếu `LD_LIBRARY_PATH` | `export LD_LIBRARY_PATH=$REPO/RoutingKit/lib` |
| `--original-graph-dir required for --line-graph mode` | Chạy line-graph mode thiếu tham số | Thêm `--original-graph-dir` trỏ tới thư mục graph gốc |
| `failed to load graph: No such file` | Chưa chạy pipeline hoặc sai đường dẫn | Kiểm tra `Maps/data/` có đủ files, chạy lại pipeline |
| `bad interpreter: No such file` | Line endings CRLF trên WSL | `dos2unix <script_file>` |
| Script không chạy | Thiếu permission | `chmod +x scripts/*.sh` |
| Pipeline pha 4-5 treo | InertialFlowCutter chưa build | Build theo mục 3.3 |
| `coordinate_validation_failed` (HTTP 400) | Tọa độ ngoài bounding box đồ thị | Kiểm tra tọa độ nằm trong bbox từ `/info` |
| `weight[i] exceeds maximum` (HTTP 400) | Trọng số customize >= INFINITY | Đảm bảo mọi giá trị < 2,147,483,647 |
| Server khởi động chậm | Build CCH mất thời gian | Bình thường: ~1-3 giây cho Hanoi (~929K nodes) |
| `failed to bind port` | Port đang bị chiếm | Kiểm tra `lsof -i :8081` hoặc đổi port |

### Log debug

```bash
# Bật debug log chi tiết
RUST_LOG=debug hanoi_server --graph-dir ... --log-format full

# Log vào file
hanoi_server --graph-dir ... --log-dir /tmp/routing-logs

# Chỉ debug module cụ thể
RUST_LOG=hanoi_server=debug,hanoi_core=info,tower_http=debug hanoi_server ...
```

### Validate dữ liệu đồ thị

```bash
# Validate đồ thị cơ sở
$REPO/CCH-Generator/lib/validate_graph Maps/data/hanoi_motorcycle/graph

# Validate đồ thị + line graph
$REPO/CCH-Generator/lib/validate_graph Maps/data/hanoi_motorcycle/graph \
  --turn-expanded Maps/data/hanoi_motorcycle/line_graph
```
