# Hướng dẫn đọc tài liệu và hiểu source code

## Tổng quan dự án

Đây là hệ thống định tuyến đường bộ cho mạng lưới giao thông Hà Nội, sử dụng
thuật toán **Customizable Contraction Hierarchies (CCH)** để tìm đường đi ngắn
nhất. Hệ thống xử lý dữ liệu bản đồ OpenStreetMap (OSM), chuyển đổi thành đồ
thị nhị phân, và cung cấp API truy vấn định tuyến.

### Pipeline tổng thể

```
Bản đồ OSM (.osm.pbf)
  → CCH-Generator (C++): tạo đồ thị nhị phân cơ sở
    → RoutingKit (C++): trích xuất conditional turns
      → CCH-Hanoi (Rust): tạo line graph (đồ thị mở rộng turn)
        → InertialFlowCutter: tính thứ tự nested dissection
          → rust_road_router: CCH server phục vụ truy vấn
```

---

## Cấu trúc thư mục

| Thư mục | Ngôn ngữ | Vai trò |
|---------|----------|---------|
| `RoutingKit/` | C++ | Thư viện routing chuẩn công nghiệp (KIT). Cung cấp CH, CCH, import OSM. **Không được sửa đổi.** |
| `rust_road_router/` | Rust | Workspace routing chính — CCH engine, HTTP server, conversion tools. **Không được sửa đổi.** |
| `CCH-Generator/` | C++17 | Tạo đồ thị nhị phân từ OSM PBF và validate cấu trúc đồ thị |
| `CCH-Hanoi/` | Rust (nightly) | Hub tích hợp Hà Nội — core library, CLI, HTTP server, gateway, tools |
| `Maps/` | — | File bản đồ OSM nguồn và dữ liệu đồ thị đã tạo (`Maps/data/`) |
| `CCH_Data_Pipeline/` | Kotlin | Pipeline xử lý dữ liệu camera giao thông (aggregator, smoother, modeler), camera config |
| `scripts/` | Bash | Script hỗ trợ pipeline và testing |
| `docs/` | — | Tài liệu thiết kế, walkthrough, changelog |

### Quy ước quan trọng

- **Đơn vị travel_time**: milliseconds (`tt_units_per_s = 1000`)
- **Format đồ thị**: RoutingKit binary — mảng nhị phân không header, lưu dạng
  CSR (Compressed Sparse Row)
- **Profile hỗ trợ**: `car`, `motorcycle` (motorcycle được calibrate cho Hà
  Nội)

---

## Lộ trình đọc tài liệu

### Bước 1 — Nền tảng (Bắt đầu từ đây)

Đọc theo thứ tự:

1. **[CCH Walkthrough](walkthrough/CCH%20Walkthrough.md)** — Giải thích 3 pha
   của CCH: Contraction → Customization → Query. Đây là nền tảng lý thuyết cốt
   lõi của toàn bộ hệ thống.

2. **[Manual Pipeline Guide](walkthrough/Manual%20Pipeline%20Guide.md)** — Hướng
   dẫn từng bước chạy pipeline từ file OSM PBF → đồ thị nhị phân → line graph.
   Giúp hiểu luồng dữ liệu thực tế.

3. **[Graph Weight Format and Test Weight Generation Guide](walkthrough/Graph%20Weight%20Format%20and%20Test%20Weight%20Generation%20Guide.md)**
   — Format file nhị phân RoutingKit, cấu trúc CSR, đơn vị millisecond, cách
   tạo test data.

> **Nếu chưa biết Rust:** Đọc thêm
> [Rust Fundamentals for Experienced Programmers](walkthrough/Rust%20Fundamentals%20for%20Experienced%20Programmers.md)
> trước khi đi sâu vào code Rust.

### Bước 2 — Hiểu các component chính

4. **[OSM Loading](walkthrough/OSM%20Loading.md)** — Cách RoutingKit load bản
   đồ OSM qua 2 pass, xử lý profile xe, tag filtering, xác định tốc độ/chiều.

5. **[CCH-Hanoi Hub](walkthrough/CCH-Hanoi%20Hub.md)** — Cấu trúc workspace
   CCH-Hanoi: 6 crate (hanoi-core, hanoi-cli, hanoi-server, hanoi-gateway,
   hanoi-bench, hanoi-tools) và vai trò từng crate.

6. **[CCH-Hanoi Usage Guide](walkthrough/CCH-Hanoi%20Usage%20Guide.md)** —
   Hướng dẫn vận hành: build, chạy server, API endpoints, CLI commands, testing.

### Bước 3 — Chuyên sâu

7. **[CCH Deep Dive](walkthrough/CCH%20Deep%20Dive.md)** — Chi tiết từng bước
   CCH: biểu diễn CSR, nested dissection ordering, elimination tree, triangle
   relaxation, bidirectional query, path unpacking.

8. **[Line Graph Spatial Indexing and Snapping](walkthrough/Line%20Graph%20Spatial%20Indexing%20and%20Snapping.md)**
   — Hệ tọa độ trên line graph, KD-tree indexing, thuật toán snap-to-edge.

9. **[rust_road_router Engine API Reference](walkthrough/rust_road_router%20Engine%20API%20Reference.md)**
   — API reference cho engine crate: type definitions, function signatures, query
   interface.

10. **[rust_road_router Algorithm Families](walkthrough/rust_road_router%20Algorithm%20Families.md)**
    — Tổng quan các thuật toán: Dijkstra, A\*, ALT, Hub Labels, TD variants.

### Bước 4 — InertialFlowCutter & Tối ưu

11. **[IFC Scripts Reference](walkthrough/IFC%20Scripts%20Reference.md)** — 3
    script wrapper IFC (cch_order, cch_cut_order, cch_cut_reorder) và cách chọn
    script phù hợp.

12. **[IFC Ordering Effectiveness Analysis](walkthrough/IFC%20Ordering%20Effectiveness%20Analysis.md)**
    — Phân tích chất lượng separator trên mạng lưới Hà Nội và giới hạn của mesh
    topology dày đặc.

13. **[IFC Parameter Calibration for Hanoi](walkthrough/IFC%20Parameter%20Calibration%20for%20Hanoi.md)**
    — Tinh chỉnh tham số IFC cho đô thị Hà Nội.

---

## Lộ trình đọc source code

### CCH-Generator (C++)

Nơi bắt đầu nếu muốn hiểu cách tạo đồ thị từ OSM:

| File | Vai trò |
|------|---------|
| `CCH-Generator/src/generate_graph.cpp` | Entry point — load OSM, gọi RoutingKit, xuất binary vectors |
| `CCH-Generator/src/validate_graph.cpp` | Kiểm tra tính toàn vẹn đồ thị (CSR invariants, coordinate bounds) |
| `CCH-Generator/include/graph_utils.h` | Utility functions dùng chung |

Tài liệu thiết kế: [CCH-Generator Plan](done/CCH-Generator%20Plan.md)

### CCH-Hanoi (Rust) — Component chính cần đọc

Đọc code theo thứ tự dependency:

```
hanoi-core (thư viện lõi — đọc đầu tiên)
  ├── hanoi-server (HTTP API)
  ├── hanoi-gateway (reverse proxy / multi-profile)
  ├── hanoi-cli (CLI wrapper)
  ├── hanoi-tools (standalone binary tools)
  └── hanoi-bench (benchmarking)
```

#### hanoi-core — Thư viện lõi

| File | Vai trò |
|------|---------|
| `crates/hanoi-core/src/lib.rs` | Module root — xem cấu trúc public API |
| `crates/hanoi-core/src/graph.rs` | Load đồ thị RoutingKit binary, build CCH |
| `crates/hanoi-core/src/cch.rs` | CCH customization & query logic |
| `crates/hanoi-core/src/line_graph.rs` | Line graph (turn-expanded) routing, DirectedCCH |
| `crates/hanoi-core/src/geometry.rs` | Turn direction detection, angle computation |
| `crates/hanoi-core/src/bounds.rs` | BoundingBox, coordinate validation (padding, snap distance) |
| `crates/hanoi-core/src/spatial.rs` | KD-tree spatial indexing, snap-to-edge |
| `crates/hanoi-core/src/multi_route.rs` | Alternative route finding (stretch factor) |
| `crates/hanoi-core/src/cch_cache.rs` | Cache compiled CCH structure |
| `crates/hanoi-core/src/via_way_restriction.rs` | Via-way turn restrictions (node splitting) |

#### hanoi-server — HTTP Server

| File | Vai trò |
|------|---------|
| `crates/hanoi-server/src/main.rs` | CLI args, tracing, server startup, twin ports (query + customize) |
| `crates/hanoi-server/src/handlers.rs` | Tất cả endpoint handlers (query, evaluate, customize, overlays, info, health, ready) |
| `crates/hanoi-server/src/engine.rs` | Background threads (run_normal, run_line_graph) — owns CCH contexts |
| `crates/hanoi-server/src/state.rs` | AppState, QueryMsg |
| `crates/hanoi-server/src/types.rs` | JSON request/response types |
| `crates/hanoi-server/src/camera_overlay.rs` | Camera YAML loader, spatial filtering |
| `crates/hanoi-server/src/traffic.rs` | Traffic segment generator, congestion ratio buckets |
| `crates/hanoi-server/src/route_eval.rs` | GeoJSON route evaluation (NormalRouteEvaluator, LineGraphRouteEvaluator) |
| `crates/hanoi-server/src/ui.rs` | Static asset handlers (route-viewer UI) |

#### hanoi-tools — Standalone tools

| File | Vai trò |
|------|---------|
| `crates/hanoi-tools/src/bin/generate_line_graph.rs` | Tạo line graph từ base graph |
| `crates/hanoi-tools/src/bin/diagnose_turn.rs` | Chẩn đoán turn restrictions tại tọa độ chỉ định |

### rust_road_router (Rust) — Engine upstream

> **Đây là thư viện upstream, không sửa đổi.** Đọc để hiểu API mà CCH-Hanoi gọi
> vào.

Các module quan trọng nhất:

| Module | Vai trò |
|--------|---------|
| `engine/src/datastr/graph/` | Cấu trúc dữ liệu đồ thị (CSR, weight types) |
| `engine/src/algo/customizable_contraction_hierarchy/` | CCH: contraction, customization, query |
| `engine/src/io.rs` | Đọc/ghi file nhị phân RoutingKit format |
| `server/src/main.rs` | HTTP server mẫu (CCH-Hanoi server dựa trên pattern này) |

### RoutingKit (C++)

> **Không sửa đổi.** Đọc để hiểu OSM loading và binary format.

| File | Vai trò |
|------|---------|
| `src/osm_graph_builder.cpp` | 2-pass OSM loader chính |
| `src/customizable_contraction_hierarchy.cpp` | CCH implementation gốc |
| `include/routingkit/osm_graph_builder.h` | API header cho OSM loading |

---

## Tài liệu thiết kế (docs/done/) — theo chủ đề

### Tạo đồ thị & Profile

- [CCH-Generator Plan](done/CCH-Generator%20Plan.md) — Thiết kế CCH-Generator
- [Motorcycle Profile Implementation](done/Motorcycle%20Profile%20Implementation.md) — Profile xe máy cho Hà Nội
- [Hanoi Speed Calibration Plan](done/Hanoi%20Speed%20Calibration%20Plan.md) — Hiệu chỉnh bảng tốc độ cho Việt Nam

### Turn restrictions (Ràng buộc rẽ)

- [Conditional Turns Implementation](done/Conditional%20Turns%20Implementation.md) — Kiến trúc trích xuất conditional turns
- [Conditional Turn Integration Plan](done/Conditional%20Turn%20Integration%20Plan.md) — Tích hợp conditional turns vào pipeline
- [Multi-Via-Way Turn Restrictions](done/Multi-Via-Way%20Turn%20Restrictions.md) — Turn restrictions qua nhiều đoạn đường
- [Via-Way Turn Restrictions](done/Via-Way%20Turn%20Restrictions.md) — Via-way restrictions trong line graph
- [Profile-Aware Conditional Resolver Proposal](done/Profile-Aware%20Conditional%20Resolver%20Proposal.md) — Conditional resolver theo profile

### Navigation & Turn annotations

- [Turn Direction Detection](done/Turn%20Direction%20Detection.md) — Phát hiện hướng rẽ (trái/phải/thẳng/quay đầu)
- [Turn Refinement Pipeline](done/Turn%20Refinement%20Pipeline.md) — Lọc và tinh chỉnh turn annotations
- [Turn Refinement Pipeline v2](done/Turn%20Refinement%20Pipeline%20v2.md) — Sửa phantom turns ở đầu/cuối route
- [Progressive Snapping](done/Progressive%20Snapping.md) — Multi-candidate snap point

### Hạ tầng & Chất lượng

- [CCH-Hanoi Structure Rework](done/CCH-Hanoi%20Structure%20Rework.md) — Tái cấu trúc workspace
- [Performance Benchmarking Module](done/Performance%20Benchmarking%20Module.md) — Module benchmark
- [Structured Logging with Tracing](done/Structured%20Logging%20with%20Tracing.md) — Logging framework
- [Enhanced Health Monitoring](done/Enhanced%20Health%20Monitoring%20&%20Graceful%20Shutdown%20Timeout.md) — Health endpoint & graceful shutdown
- [Coordinate Boundary Validation](done/Coordinate%20Boundary%20Validation.md) — Validate tọa độ đầu vào

### Audit & Sửa lỗi

- [Audit Findings 2026-03-12](done/Audit%20Findings%202026-03-12.md) — Bugs phát hiện từ audit
- [Audit Findings 2026-03-18](done/Audit%20Findings%202026-03-18.md) — Audit toàn workspace
- [Fixes and Design Observations 2026-03-18](done/Fixes%20and%20Design%20Observations%202026-03-18.md) — Sửa lỗi từ audit

---

## Tính năng đang lên kế hoạch (docs/planned/)

| Tài liệu | Mô tả |
|-----------|-------|
| [Turn Refinement Pipeline v3](planned/Turn%20Refinement%20Pipeline%20v3.md) | Thiết kế lại turn post-processing |
| [Distance to Next Maneuver](planned/Distance%20to%20Next%20Maneuver.md) | Thêm khoảng cách đến điểm rẽ tiếp theo |
| [Live Weight Pipeline](planned/Live%20Weight%20Pipeline.md) | Pipeline cập nhật trọng số từ camera giao thông |
| [Smoother Module](planned/Smoother%20Module.md) | Module làm mượt dữ liệu tốc độ/mật độ |

---

## Gợi ý cách tiếp cận theo vai trò

### Nếu bạn là người vận hành (operator)

1. Đọc [Manual Pipeline Guide](walkthrough/Manual%20Pipeline%20Guide.md)
2. Đọc [CCH-Hanoi Usage Guide](walkthrough/CCH-Hanoi%20Usage%20Guide.md)
3. Chạy thử pipeline: `CCH-Generator/scripts/run_pipeline Maps/hanoi.osm.pbf`

### Nếu bạn là developer backend

1. Đọc [CCH Walkthrough](walkthrough/CCH%20Walkthrough.md) để hiểu thuật toán
2. Đọc [CCH-Hanoi Hub](walkthrough/CCH-Hanoi%20Hub.md) để hiểu cấu trúc code
3. Bắt đầu đọc `CCH-Hanoi/crates/hanoi-core/src/lib.rs` → `graph.rs` → `cch.rs`
4. Tham khảo [Engine API Reference](walkthrough/rust_road_router%20Engine%20API%20Reference.md)

### Nếu bạn muốn hiểu sâu thuật toán

1. Đọc [CCH Deep Dive](walkthrough/CCH%20Deep%20Dive.md)
2. Đọc [Algorithm Families](walkthrough/rust_road_router%20Algorithm%20Families.md)
3. Đọc [IFC Ordering Effectiveness Analysis](walkthrough/IFC%20Ordering%20Effectiveness%20Analysis.md)
4. Đọc source `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/`

---

## Changelog

Mọi thay đổi code và tài liệu đều được ghi trong
[docs/CHANGELOGS.md](CHANGELOGS.md), theo format:

```
## YYYY-MM-DD — Tiêu đề ngắn
- **file_path**: Mô tả thay đổi
```

Đọc changelog từ trên xuống để nắm tiến trình phát triển gần nhất.

---

## Hướng dẫn triển khai source

### Yêu cầu hệ thống

| Yêu cầu | Chi tiết |
|----------|----------|
| **Hệ điều hành** | Linux (native hoặc WSL2 trên Windows) |
| **GCC/G++** | 7+ với hỗ trợ C++17 |
| **CMake** | 3.16+ |
| **Make** | GNU Make |
| **Python** | 3.x (cần cho `generate_make_file` của RoutingKit) |
| **Rust** | Nightly — `CCH-Hanoi` dùng nightly mới nhất, `rust_road_router` dùng `nightly-2024-06-01` |
| **Intel TBB** | `libtbb-dev` — cần cho InertialFlowCutter |
| **Readline** | `libreadline-dev` — cần cho IFC console |
| **Protobuf** | `libprotobuf-dev`, `protobuf-compiler` — cần cho OSM PBF parsing |

Cài đặt trên Ubuntu/Debian:

```bash
sudo apt update
sudo apt install -y build-essential cmake python3 libprotobuf-dev \
  protobuf-compiler libtbb-dev libreadline-dev zlib1g-dev pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup install nightly
rustup default nightly
```

### Thứ tự build (quan trọng — có dependency)

```
1. RoutingKit          ← thư viện nền, build trước
2. CCH-Generator       ← link RoutingKit
3. InertialFlowCutter  ← link RoutingKit (qua submodule)
4. CCH-Hanoi           ← Rust workspace, không dependency C++ trực tiếp
```

### Bước 1 — Build RoutingKit

```bash
cd RoutingKit
python3 generate_make_file
make -j"$(nproc)"
cd ..
```

Output: `RoutingKit/bin/conditional_turn_extract`, `RoutingKit/lib/libroutingkit.a`

### Bước 2 — Build CCH-Generator

```bash
cmake -S CCH-Generator -B CCH-Generator/build \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_RUNTIME_OUTPUT_DIRECTORY="${PWD}/CCH-Generator/lib"
cmake --build CCH-Generator/build -j"$(nproc)"
```

Output: `CCH-Generator/lib/cch_generator`, `CCH-Generator/lib/validate_graph`

### Bước 3 — Build InertialFlowCutter

```bash
cd rust_road_router/lib/InertialFlowCutter
mkdir -p build && cd build
cmake -DCMAKE_BUILD_TYPE=Release -DGIT_SUBMODULE=OFF -DUSE_KAHIP=OFF ..
make -j"$(nproc)" console
cd ../../../..
```

Output: `rust_road_router/lib/InertialFlowCutter/build/console`

> Script `flow_cutter_cch_order.sh` gọi binary `console` này — nếu thiếu sẽ
> không tạo được `cch_perm`.

### Bước 4 — Build CCH-Hanoi

```bash
cd CCH-Hanoi
cargo build --release --workspace
cd ..
```

Output (trong `CCH-Hanoi/target/release/`):

| Binary | Vai trò |
|--------|---------|
| `hanoi_server` | HTTP routing server |
| `hanoi_gateway` | API gateway multi-profile |
| `cch-hanoi` | CLI query tool |
| `generate_line_graph` | Tạo line graph từ base graph |
| `hanoi_bench` | Benchmarking tool |

### Build tự động (WSL)

Script `scripts/wsl_setup.sh` tự động hóa toàn bộ quy trình trên trong WSL.

---

## Data Pipeline — Từ bản đồ đến server

### Pha 1: Tạo base graph từ OSM

```bash
CCH-Generator/lib/cch_generator \
  Maps/hanoi.osm.pbf \
  Maps/data/hanoi_motorcycle/graph \
  --profile motorcycle

# Validate
CCH-Generator/lib/validate_graph Maps/data/hanoi_motorcycle/graph
```

Tùy chọn profile: `car`, `motorcycle` (motorcycle calibrate cho Hà Nội).

### Pha 2: Trích xuất conditional turn restrictions

```bash
RoutingKit/bin/conditional_turn_extract \
  Maps/hanoi.osm.pbf \
  Maps/data/hanoi_motorcycle/graph \
  Maps/data/hanoi_motorcycle \
  --profile motorcycle

# Validate lại
CCH-Generator/lib/validate_graph Maps/data/hanoi_motorcycle/graph
```

### Pha 3: Tạo line graph (turn-expanded)

```bash
CCH-Hanoi/target/release/generate_line_graph \
  Maps/data/hanoi_motorcycle/graph \
  Maps/data/hanoi_motorcycle/line_graph

# Validate line graph
CCH-Generator/lib/validate_graph \
  Maps/data/hanoi_motorcycle/line_graph \
  --turn-expanded Maps/data/hanoi_motorcycle/line_graph
```

### Pha 4: Tạo CCH ordering (InertialFlowCutter)

```bash
# Ordering cho line graph (turn-aware routing)
rust_road_router/flow_cutter_cch_order.sh \
  Maps/data/hanoi_motorcycle/line_graph

# Ordering cho base graph (nếu chạy normal mode)
rust_road_router/flow_cutter_cch_order.sh \
  Maps/data/hanoi_motorcycle/graph
```

Output: `<graph_dir>/cch_perm` — file permutation cần cho CCH.

### Pipeline tự động

```bash
# Interactive — chạy từng pha, hỏi xác nhận
scripts/pipeline Maps/hanoi.osm.pbf motorcycle

# Hoặc chạy end-to-end
CCH-Generator/scripts/run_pipeline Maps/hanoi.osm.pbf
```

### Cấu trúc thư mục dữ liệu sau pipeline

```
Maps/data/hanoi_motorcycle/
├── graph/                          ← Base graph
│   ├── first_out                   (u32) CSR offset
│   ├── head                        (u32) target node
│   ├── travel_time                 (u32) milliseconds
│   ├── latitude                    (f32) node latitude
│   ├── longitude                   (f32) node longitude
│   ├── way                         (u32) routing way ID
│   ├── forbidden_turn_from_arc     (u32) turn restriction
│   ├── forbidden_turn_to_arc       (u32) turn restriction
│   └── cch_perm                    (u32) IFC ordering
├── line_graph/                     ← Turn-expanded graph
│   ├── first_out, head, travel_time, latitude, longitude
│   └── cch_perm
├── conditional_turn_from_arc       (u32)
├── conditional_turn_to_arc         (u32)
└── conditional_turn_time_windows   (packed binary)
```

Tất cả file đều là **mảng nhị phân không header** (headerless binary vectors).

---

## Khởi động server

### Chế độ Line graph (turn-aware — khuyến nghị)

```bash
CCH-Hanoi/target/release/hanoi_server \
  --graph-dir Maps/data/hanoi_motorcycle/line_graph \
  --original-graph-dir Maps/data/hanoi_motorcycle/graph \
  --query-port 8081 \
  --customize-port 9081 \
  --line-graph
```

### Chế độ Normal (base graph)

```bash
CCH-Hanoi/target/release/hanoi_server \
  --graph-dir Maps/data/hanoi_motorcycle/graph \
  --query-port 8080 \
  --customize-port 9080
```

### Kiến trúc 2 port

| Port | Endpoints | Mô tả |
|------|-----------|-------|
| Query port (8080/8081) | `GET /health`, `GET /ready`, `GET /info`, `POST /query` | Truy vấn routing |
| Customize port (9080/9081) | `POST /customize` | Cập nhật trọng số (binary body, gzip) |

### Ví dụ truy vấn

```bash
# Health check
curl http://localhost:8081/health

# Query routing (line graph mode)
curl -X POST http://localhost:8081/query \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }'
```

---

## Khởi động Gateway (multi-profile)

Gateway cho phép gọi nhiều profile (car, motorcycle) qua 1 endpoint duy nhất.

### Config file (`gateway.yaml`)

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

### Khởi động

```bash
CCH-Hanoi/target/release/hanoi_gateway \
  --config CCH-Hanoi/crates/hanoi-gateway/gateway.yaml \
  --port 9000
```

---

## CLI — Truy vấn từ command line

```bash
# Query theo tọa độ
CCH-Hanoi/target/release/cch-hanoi query \
  --data-dir Maps/data/hanoi_motorcycle \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-format geojson

# Query theo node ID
CCH-Hanoi/target/release/cch-hanoi query \
  --data-dir Maps/data/hanoi_motorcycle \
  --from-node 1000 --to-node 5000

# Xem thông tin đồ thị
CCH-Hanoi/target/release/cch-hanoi info \
  --data-dir Maps/data/hanoi_motorcycle
```

---

## Biến môi trường

### RUST_LOG — Điều khiển logging

```bash
# Mặc định
RUST_LOG=info

# Debug chi tiết
RUST_LOG=debug

# Mixed — debug chỉ core
RUST_LOG=info,hanoi_core=debug

# Xem HTTP request details (server)
RUST_LOG=info,tower_http=debug
```

---

## Xử lý lỗi thường gặp

| Lỗi | Nguyên nhân | Cách sửa |
|-----|-------------|----------|
| `libtbb.so: cannot open` | Thiếu Intel TBB | `sudo apt install libtbb-dev` |
| `readline/readline.h: No such file` | Thiếu readline | `sudo apt install libreadline-dev` |
| `error: could not find Cargo.toml` | Sai thư mục | `cd` vào đúng workspace root |
| `cch_perm not found` | Chưa chạy IFC ordering (Pha 4) | Chạy `flow_cutter_cch_order.sh` |
| `Permission denied` trên `.sh` | File thiếu execute bit | `chmod +x <script>` |
| `dos2unix: command not found` | Script có line ending Windows | `sudo apt install dos2unix` rồi `dos2unix <file>` |
| Rust nightly mismatch | `CCH-Hanoi` cần nightly mới | `rustup update nightly` |
| IFC console crash / hang | Graph quá lớn cho RAM | Thử giảm `max_balance` hoặc dùng máy có >8GB RAM |
| `validate_graph` báo lỗi CSR | Pipeline pha trước lỗi | Chạy lại từ pha bị lỗi |

---

## Triển khai production (tóm tắt)

```
┌─────────────┐     ┌─────────────────┐     ┌──────────────────┐
│   Client     │────►│  hanoi-gateway  │────►│  hanoi-server    │
│   (HTTP)     │     │  port 9000      │     │  motorcycle:8081 │
└─────────────┘     │                 │     │  car:8080        │
                    └─────────────────┘     └──────────────────┘
```

1. Chạy 1 `hanoi_server` instance cho mỗi profile (car, motorcycle)
2. Chạy 1 `hanoi_gateway` trỏ đến các server instances
3. Client gọi gateway, chỉ định profile trong request
4. Dùng `RUST_LOG=info` cho production, `RUST_LOG=debug` khi debug
5. Dùng `/health` và `/ready` endpoint để health check
