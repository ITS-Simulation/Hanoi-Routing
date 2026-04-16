# Hướng Dẫn Sử Dụng CCH-Hanoi

Tài liệu tham khảo đầy đủ về vận hành, truy vấn và kiểm thử hệ thống định
tuyến CCH-Hanoi — từ build workspace cho đến chạy server production, thực hiện
truy vấn, upload trọng số tuỳ chỉnh và xác minh kết quả.

---

## Mục Lục

1. [Tổng Quan Hệ Thống](#1-tổng-quan-hệ-thống)
2. [Kiến Trúc Workspace](#2-kiến-trúc-workspace)
3. [Build Workspace](#3-build-workspace)
4. [Dữ Liệu Đầu Vào](#4-dữ-liệu-đầu-vào)
5. [hanoi-core — Tham Chiếu API Thư Viện](#5-hanoi-core--tham-chiếu-api-thư-viện)
6. [hanoi-server — HTTP Routing Server](#6-hanoi-server--http-routing-server)
7. [hanoi-gateway — API Gateway](#7-hanoi-gateway--api-gateway)
8. [hanoi-cli — Giao Diện Dòng Lệnh](#8-hanoi-cli--giao-diện-dòng-lệnh)
9. [hanoi-tools — Công Cụ Pipeline](#9-hanoi-tools--công-cụ-pipeline)
10. [hanoi-bench — Đo Hiệu Năng](#10-hanoi-bench--đo-hiệu-năng)
11. [Tham Chiếu HTTP API](#11-tham-chiếu-http-api)
12. [Hướng Dẫn Tuỳ Chỉnh Trọng Số](#12-hướng-dẫn-tuỳ-chỉnh-trọng-số)
13. [Hướng Dẫn Kiểm Thử](#13-hướng-dẫn-kiểm-thử)
14. [Sơ Đồ Vận Hành](#14-sơ-đồ-vận-hành)
15. [Xử Lý Sự Cố](#15-xử-lý-sự-cố)
16. [Hướng Dẫn Cấu Hình Logging](#16-hướng-dẫn-cấu-hình-logging)

---

## 1. Tổng Quan Hệ Thống

CCH-Hanoi là tầng tích hợp riêng cho Hà Nội của thuật toán Customizable
Contraction Hierarchies (CCH). Nó nằm trên `rust_road_router` (thuật toán
chung) và cung cấp:

- **Tải đồ thị** từ định dạng nhị phân RoutingKit
- **Xây dựng CCH** (contraction hierarchy độc lập metric)
- **Tuỳ chỉnh trọng số** (gán trọng số lên CCH, có thể tuỳ chỉnh lại khi đang chạy)
- **Truy vấn đường ngắn nhất** theo node ID hoặc toạ độ GPS
- **Định tuyến có xét rẽ** qua chế độ line graph (đồ thị mở rộng theo lượt rẽ)
- **HTTP server** với kiến trúc dual-port (truy vấn + tuỳ chỉnh)
- **API gateway** để truy cập thống nhất cả hai chế độ đồ thị
- **Đo hiệu năng** với phân tích thống kê và phát hiện suy giảm
- **K tuyến thay thế** qua separator-based elimination tree walk

### Hai Chế Độ Định Tuyến


| Chế độ         | Loại đồ thị         | Loại CCH               | Hạn chế rẽ                   | Trường hợp sử dụng                     |
| -------------- | -------------------- | ---------------------- | ---------------------------- | --------------------------------------- |
| **Normal**     | Đồ thị đường tiêu chuẩn | `CCH` (vô hướng)     | Không áp dụng                | Định tuyến nhanh, không mô hình hoá rẽ |
| **Line Graph** | Đồ thị mở rộng theo rẽ | `DirectedCCH` (pruned) | Áp dụng qua cấu trúc đồ thị | Định tuyến chính xác với hạn chế rẽ     |


**Chế độ normal**: Node = nút giao, edge = đoạn đường. Đơn giản, nhanh hơn,
nhưng bỏ qua hạn chế rẽ.

**Chế độ line graph**: Node = đoạn đường gốc (edge), edge = lượt rẽ hợp lệ
giữa các đoạn liên tiếp. Lượt rẽ cấm bị loại bỏ cấu trúc. Cần cả dữ liệu
line graph *và* metadata đồ thị gốc (để ánh xạ đường đi và hiệu chỉnh cạnh
cuối).

### Khái Niệm Thuật Toán Chính

**Pipeline CCH ba giai đoạn**:

```
Giai đoạn 1: Contraction (chạy một lần khi khởi động)
  Đầu vào:  topology đồ thị + thứ tự node (cch_perm)
  Đầu ra:   Cấu trúc CCH hierarchy (độc lập metric)

Giai đoạn 2: Customization (mỗi khi thay đổi trọng số)
  Đầu vào:  Cấu trúc CCH + vector trọng số cạnh (travel_time)
  Đầu ra:   CustomizedBasic (trọng số shortcut lên/xuống)

Giai đoạn 3: Query (cho mỗi request)
  Đầu vào:  node nguồn + node đích + CustomizedBasic
  Đầu ra:   khoảng cách đường ngắn nhất + chuỗi node
```

**Giá trị sentinel INFINITY**: `u32::MAX / 2 = 2,147,483,647`. Dùng làm giá
trị "không có cạnh" trong shortcut CCH. Phép giãn tam giác dùng phép cộng
thường (`a + b`, không phải `saturating_add`), nên bất kỳ trọng số đầu vào nào
>= INFINITY sẽ làm sai kết quả. Server từ chối các trọng số như vậy tại
endpoint `/customize`.

---

## 2. Kiến Trúc Workspace

```
CCH-Hanoi/
├── Cargo.toml                     # Workspace root: members = ["crates/*"]
├── rust-toolchain.toml            # Ghim vào nightly (yêu cầu bởi rust_road_router)
└── crates/
    ├── hanoi-core/                # Thư viện — tải đồ thị, CCH, spatial indexing, truy vấn
    ├── hanoi-server/              # Binary — HTTP server dual-port (Axum)
    ├── hanoi-gateway/             # Binary — API gateway proxy
    ├── hanoi-cli/                 # Binary — CLI offline cho truy vấn và thông tin
    ├── hanoi-tools/               # Binaries — công cụ pipeline (generate_line_graph)
    └── hanoi-bench/               # Library + Binaries — benchmark và phân tích
```

### Sơ Đồ Phụ Thuộc Giữa Các Crate

```
rust_road_router/engine     (upstream — thuật toán chung, KHÔNG BAO GIỜ sửa đổi)
        ↑
   hanoi-core               (triển khai CCH cho Hà Nội)
        ↑
   hanoi-cli                (giao diện CLI)
   hanoi-server             (HTTP server)
   hanoi-bench              (benchmark)

   hanoi-tools              (độc lập — phụ thuộc trực tiếp vào rust_road_router)
   hanoi-gateway            (độc lập — HTTP proxy, không phụ thuộc core)
```

### Cấu Trúc Nội Bộ Đã Tinh Gọn

`hanoi-core` nay được tổ chức theo domain thay vì để toàn bộ crate phẳng dưới
`src/`:

```text
hanoi-core/src/
├── lib.rs
├── graph/{mod.rs,data.rs,cache.rs}
├── geo/
│   ├── mod.rs
│   ├── bounds.rs
│   └── spatial/{mod.rs,index.rs,snap.rs,metric.rs}
├── routing/
│   ├── mod.rs
│   ├── alternatives.rs
│   ├── normal/{mod.rs,answer.rs,context.rs,engine.rs,snap.rs}
│   └── line_graph/{mod.rs,context.rs,engine.rs,mapping.rs,coordinate_patch.rs}
├── guidance/{mod.rs,turn_annotation.rs,turn_classify.rs,turn_refine.rs}
└── restrictions/{mod.rs,via_way.rs}
```

`hanoi-server` giờ dùng `lib.rs + thin main.rs`, tách riêng phần khởi động,
API, runtime và UI thành các cây module rõ ràng:

```text
hanoi-server/src/
├── lib.rs
├── main.rs
├── app/{mod.rs,args.rs,tracing.rs,bootstrap.rs,routes.rs}
├── api/
│   ├── mod.rs
│   ├── state.rs
│   ├── dto/{mod.rs,query.rs,customize.rs,status.rs,ui.rs}
│   └── handlers/{mod.rs,query.rs,customize.rs,status.rs,ui.rs}
├── runtime/{mod.rs,worker.rs,dispatch.rs,response.rs}
└── ui/
    ├── mod.rs
    ├── static_assets.rs
    ├── traffic/{mod.rs,overlay.rs,road_flags.rs}
    ├── camera/{mod.rs,loader.rs,overlay.rs}
    └── route_eval/{mod.rs,parser.rs,normal.rs,line_graph.rs}
```

Các đường dẫn import chuẩn sau refactor:

- `hanoi_core::routing::normal::{CchContext, QueryAnswer, QueryEngine}`
- `hanoi_core::routing::line_graph::{LineGraphCchContext, LineGraphQueryEngine}`
- `hanoi_core::geo::spatial::{SpatialIndex, SnapResult, haversine_m}`
- `hanoi_core::restrictions::via_way::{apply_node_splits, load_via_way_chains}`
- `hanoi_server::app::bootstrap::run`
- `hanoi_server::runtime::worker::{run_normal, run_line_graph}`

### Edition và Toolchain

Tất cả các crate dùng **Rust edition 2024** trên toolchain **nightly**. Nightly
là bắt buộc vì `rust_road_router/engine` sử dụng
`#![feature(impl_trait_in_assoc_type)]`.

---

## 3. Build Workspace

### Build Toàn Bộ

```bash
cd CCH-Hanoi
cargo build --release --workspace
```

### Build Từng Crate

```bash
# Server (bản build headless mặc định)
cargo build --release -p hanoi-server

# Server có UI + các endpoint overlay
cargo build --release -p hanoi-server --features ui

# Gateway
cargo build --release -p hanoi-gateway

# CLI
cargo build --release -p hanoi-cli

# Công cụ tạo line graph
cargo build --release -p hanoi-tools --bin generate_line_graph

# Benchmark (tất cả runner)
cargo build --release -p hanoi-bench
```

### Kết Quả Build


| Binary                | Crate         | Đường dẫn                            |
| --------------------- | ------------- | ------------------------------------ |
| `hanoi_server`        | hanoi-server  | `target/release/hanoi_server`        |
| `hanoi_gateway`       | hanoi-gateway | `target/release/hanoi_gateway`       |
| `cch-hanoi`           | hanoi-cli     | `target/release/cch-hanoi`           |
| `generate_line_graph` | hanoi-tools   | `target/release/generate_line_graph` |
| `bench_core`          | hanoi-bench   | `target/release/bench_core`          |
| `bench_server`        | hanoi-bench   | `target/release/bench_server`        |
| `bench_report`        | hanoi-bench   | `target/release/bench_report`        |


### Chạy Test

```bash
cargo test --workspace
```

---

## 4. Dữ Liệu Đầu Vào

Trước khi chạy bất kỳ binary CCH-Hanoi nào, bạn cần dữ liệu đồ thị được tạo
bởi pipeline upstream. Xem `docs/walkthrough/Manual Pipeline Guide.md` để biết
toàn bộ pipeline từ PBF đến đồ thị.

### Cấu Trúc Thư Mục Cần Thiết

**Chế độ normal** — cần một thư mục đồ thị duy nhất:

```
Maps/data/hanoi_car/
└── graph/
    ├── first_out                  # CSR offsets (Vec<u32>, n+1 phần tử)
    ├── head                       # CSR targets (Vec<u32>, m phần tử)
    ├── travel_time                # Trọng số cạnh tính bằng mili giây (Vec<u32>, m phần tử)
    ├── latitude                   # Vĩ độ node (Vec<f32>, n phần tử)
    ├── longitude                  # Kinh độ node (Vec<f32>, n phần tử)
    └── perms/
        └── cch_perm               # Thứ tự node cho CCH (Vec<u32>, n phần tử)
```

**Chế độ line graph** — cần *cả* line graph và đồ thị gốc:

```
Maps/data/hanoi_car/
├── graph/                         # Đồ thị gốc (để ánh xạ đường đi + hiệu chỉnh cạnh cuối)
│   ├── first_out
│   ├── head
│   ├── travel_time
│   ├── latitude
│   └── longitude
└── line_graph/                    # Đồ thị mở rộng theo rẽ
    ├── first_out                  # CSR line graph (LG node = cạnh gốc)
    ├── head
    ├── travel_time
    ├── latitude                   # Toạ độ LG node (= toạ độ node đầu cạnh gốc)
    ├── longitude
    └── perms/
        └── cch_perm               # Thứ tự node line graph
```

### Định Dạng File

Tất cả các file đều là **vector nhị phân thô không có header** — không magic
number, không tiền tố kích thước. Số phần tử được suy ra từ
`file_size / element_size`:


| File                                           | Kiểu phần tử | Kích thước phần tử |
| ---------------------------------------------- | ------------- | ------------------- |
| `first_out`, `head`, `travel_time`, `cch_perm` | `u32`         | 4 bytes             |
| `latitude`, `longitude`                        | `f32`         | 4 bytes             |


### Tham Chiếu Nhanh CSR (Compressed Sparse Row)

```
Các cạnh đi ra của node v:   head[first_out[v] .. first_out[v+1]]
Trọng số cạnh e:             travel_time[e]
Đích cạnh e:                 head[e]
Số node:                     first_out.len() - 1
Số cạnh:                     head.len()
```

Bất biến: `first_out[0] == 0`, `first_out[n] == m`,
`head.len() == travel_time.len()`.

### Kiểm Tra Nhanh Kích Thước

```python
import os

def graph_dims(graph_dir):
    n = os.path.getsize(f'{graph_dir}/first_out') // 4 - 1
    m = os.path.getsize(f'{graph_dir}/head') // 4
    print(f"Nodes: {n:,}  Edges: {m:,}")

graph_dims("Maps/data/hanoi_car/graph")        # ~276K nodes, ~655K edges
graph_dims("Maps/data/hanoi_car/line_graph")   # ~655K nodes, ~1.3M edges
```

---

## 5. hanoi-core — Tham Chiếu API Thư Viện

Thư viện core cung cấp toàn bộ logic định tuyến. Các crate khác (server, CLI,
bench) là consumer của API này.

### 5.1 GraphData — Tải Đồ Thị

```rust
use hanoi_core::GraphData;

// Tải đồ thị từ file nhị phân RoutingKit
let graph = GraphData::load(Path::new("Maps/data/hanoi_car/graph"))?;

println!("Nodes: {}", graph.num_nodes());
println!("Edges: {}", graph.num_edges());

// Lấy CSR view zero-copy cho CCH builder
let borrowed = graph.as_borrowed_graph();

// Lấy CSR view với trọng số tuỳ chỉnh (cho re-customization)
let custom = graph.as_borrowed_graph_with_weights(&custom_weights);
```

**Kiểm tra khi tải**: Kiểm tra `first_out[0] == 0`, tính đơn điệu,
`head.len() == travel_time.len()`, tất cả giá trị trong phạm vi. Trả về
`std::io::Error` với `ErrorKind::InvalidData` nếu thất bại.

### 5.2 CchContext — CCH Đồ Thị Thường

```rust
use hanoi_core::CchContext;

// Giai đoạn 1: Tải đồ thị + xây dựng topology CCH (độc lập metric)
let context = CchContext::load_and_build(
    Path::new("Maps/data/hanoi_car/graph"),
    Path::new("Maps/data/hanoi_car/graph/perms/cch_perm"),
)?;

// Giai đoạn 2: Tuỳ chỉnh với trọng số mặc định
let customized = context.customize();

// Giai đoạn 2 (cách khác): Tuỳ chỉnh với trọng số do caller cung cấp
let customized = context.customize_with(&my_weights);
```

### 5.3 QueryEngine — Truy Vấn Đồ Thị Thường

```rust
use hanoi_core::QueryEngine;

// Tạo engine (customization ban đầu + xây dựng spatial index)
let mut engine = QueryEngine::new(&context);

// Giai đoạn 3: Truy vấn theo node ID
if let Some(answer) = engine.query(source_node, target_node) {
    println!("Khoảng cách: {} ms ({:.1} m)", answer.distance_ms, answer.distance_m);
    println!("Đường đi: {:?}", answer.path);         // ID node giao lộ
    println!("Toạ độ: {:?}", answer.coordinates);     // Các cặp (lat, lng)
}

// Truy vấn theo toạ độ GPS (snap-to-edge + fallback)
match engine.query_coords((21.028, 105.834), (21.006, 105.843)) {
    Ok(Some(answer)) => { /* tìm thấy tuyến */ }
    Ok(None)         => { /* không có đường đi giữa hai điểm */ }
    Err(rejection)   => { /* xác thực toạ độ thất bại */ }
}

// Cập nhật trọng số trực tiếp (re-customize CCH)
engine.update_weights(&new_weights);
```

**Luồng truy vấn toạ độ**:

1. Snap điểm nguồn/đích tới cạnh đồ thị gần nhất (KD-tree + Haversine)
2. Chọn endpoint gần nhất dựa trên tham số projection `t`
3. Chạy truy vấn CCH
4. Nếu không có đường: thử tất cả 4 tổ hợp endpoint (tail/head của cả hai snap)
5. Gắn toạ độ gốc của người dùng vào kết quả

### 5.4 LineGraphCchContext — CCH Line Graph

```rust
use hanoi_core::LineGraphCchContext;

// Tải line graph + metadata đồ thị gốc, xây dựng DirectedCCH
let context = LineGraphCchContext::load_and_build(
    Path::new("Maps/data/hanoi_car/line_graph"),     // CSR line graph
    Path::new("Maps/data/hanoi_car/graph"),           // đồ thị gốc (để ánh xạ đường đi)
    Path::new("Maps/data/hanoi_car/line_graph/perms/cch_perm"),
)?;
```

### 5.5 LineGraphQueryEngine — Truy Vấn Line Graph

```rust
use hanoi_core::LineGraphQueryEngine;

let mut engine = LineGraphQueryEngine::new(&context);

// Truy vấn theo node ID line graph (= chỉ số cạnh gốc)
if let Some(answer) = engine.query(source_edge_id, target_edge_id) {
    // answer.path chứa ID node giao lộ gốc (không phải node LG)
    // answer.distance_ms bao gồm hiệu chỉnh cạnh cuối
}

// Truy vấn theo toạ độ GPS (cùng interface với chế độ normal)
let result = engine.query_coords((21.028, 105.834), (21.006, 105.843))?;
```

**Chi tiết bên trong truy vấn line graph**:

- Truy vấn CCH trả về ID node line graph (= chỉ số cạnh gốc)
- Ánh xạ đường đi: `original_tail[lg_node]` cho mỗi node, cộng thêm
`original_head[last_edge]` cho điểm đích
- **Hiệu chỉnh cạnh cuối**: cộng thêm `original_travel_time[target_edge]` vào
khoảng cách (khoảng cách CCH chỉ tính đến đoạn đường đích chứ không tính
xuyên qua nó)
- Định dạng đầu ra giống chế độ normal: ID node giao lộ + toạ độ

### 5.6 K Tuyến Thay Thế

Cả `QueryEngine` và `LineGraphQueryEngine` đều hỗ trợ truy vấn đa tuyến qua
phương thức `multi_query` / `multi_query_coords`. Các phương thức này tạo tối
đa K tuyến thay thế đa dạng bằng cách dùng các via-vertex ứng cử từ
separator-based elimination tree của CCH.

```rust
// Đồ thị thường — theo node ID
let alternatives = engine.multi_query(from, to, 3, 1.25);
for (i, answer) in alternatives.iter().enumerate() {
    println!("Tuyến {}: {} ms, {:.0} m", i, answer.distance_ms, answer.distance_m);
}

// Đồ thị thường — theo toạ độ
let alternatives = engine.multi_query_coords(
    (21.028, 105.834), (21.006, 105.843),
    3,    // max_alternatives
    1.25, // stretch_factor (theo địa lý)
)?;

// Line graph — cùng interface
let alternatives = lg_engine.multi_query(source_edge, target_edge, 3, 1.25);
let alternatives = lg_engine.multi_query_coords(
    (21.028, 105.834), (21.006, 105.843), 3, 1.25,
)?;
```

**Tham số**:

| Tham số            | Kiểu    | Mô tả                                                       |
| ------------------ | ------- | ------------------------------------------------------------ |
| `max_alternatives` | `usize` | Số tuyến tối đa trả về (bao gồm đường ngắn nhất)           |
| `stretch_factor`   | `f64`   | Tỷ lệ đi vòng địa lý tối đa (vd: 1.25 = dài hơn 25%)     |

**Giá trị trả về**: `Vec<QueryAnswer>` — tuyến chỉ số 0 luôn là đường ngắn nhất.
Mỗi kết quả bao gồm đường đi đầy đủ, toạ độ, arc ID và khoảng cách.

**Pipeline lọc** (cho mỗi ứng cử viên):
1. Phát hiện vòng lặp — loại đường đi có node lặp lại
2. Bounded stretch địa lý — loại nếu khoảng cách geo > đường ngắn nhất × stretch
3. Kiểm tra bounded stretch — đoạn đi vòng A->B phải <= optimal(A->B) × (1 + epsilon)
4. Chia sẻ chi phí — loại nếu >80% chi phí trùng với bất kỳ tuyến đã chấp nhận
5. Local optimality (T-test) — đoạn quanh via-node phải gần tối ưu
6. Phân rã đệ quy — tách tại separator có rank cao nhất để tăng đa dạng

Xem [README_ALTERNATIVE.md](README_ALTERNATIVE.md) để biết chi tiết thuật toán đầy đủ.

### 5.7 SpatialIndex — Snap Toạ Độ

```rust
use hanoi_core::SpatialIndex;

let spatial = SpatialIndex::build(&lat, &lng, &first_out, &head);

// Snap toạ độ GPS tới cạnh đồ thị gần nhất
let snap = spatial.snap_to_edge(21.028, 105.834);
// snap.edge_id  — chỉ số cạnh CSR của cạnh gần nhất
// snap.tail     — ID node nguồn
// snap.head     — ID node đích
// snap.t        — tham số projection [0, 1]: <0.5 gần tail hơn, >=0.5 gần head hơn
// snap.snap_distance_m — khoảng cách Haversine từ điểm truy vấn đến điểm snap

// Có xác thực (kiểm tra bbox + khoảng cách snap)
let validated = spatial.validated_snap("source", 21.028, 105.834, &config)?;
```

**Thuật toán**: Hybrid KD-tree (k=10 node gần nhất) + khoảng cách Haversine
vuông góc tới tất cả cạnh đi ra của các node đó.

### 5.8 BoundingBox và Xác Thực Toạ Độ

```rust
use hanoi_core::{BoundingBox, ValidationConfig, CoordRejection};
use hanoi_core::bounds::validate_coordinate;

let bbox = BoundingBox::from_coords(&lat, &lng);

let config = ValidationConfig {
    bbox_padding_m: 1000.0,       // đệm 1 km quanh bbox đồ thị
    max_snap_distance_m: 1000.0,  // từ chối snap xa hơn 1 km
};

// Xác thực: hữu hạn, trong phạm vi địa lý, trong bbox có đệm
validate_coordinate("source", 21.028, 105.834, &bbox, &config)?;
```

**Lý do từ chối** (enum `CoordRejection`):

- `NonFinite` — NaN hoặc Infinity
- `InvalidRange` — vĩ độ ngoài [-90, 90] hoặc kinh độ ngoài [-180, 180]
- `OutOfBounds` — ngoài bounding box đồ thị có đệm
- `SnapTooFar` — cạnh gần nhất xa hơn `max_snap_distance_m`

### 5.9 QueryAnswer — Kiểu Kết Quả

```rust
pub struct QueryAnswer {
    pub distance_ms: u32,              // Tổng thời gian đi tính bằng mili giây
    pub distance_m: f64,               // Khoảng cách tuyến đường qua tổng Haversine (mét)
    pub path: Vec<u32>,                // Danh sách ID node giao lộ theo thứ tự
    pub coordinates: Vec<(f32, f32)>,  // (lat, lng) cho mỗi node trên đường đi
}
```

Với truy vấn toạ độ, `coordinates` bao gồm toạ độ gốc của người dùng được
thêm vào đầu/cuối, nên `coordinates.len() == path.len() + 2`.

---

## 6. hanoi-server — HTTP Routing Server

### 6.1 Khởi Động Server

`hanoi-server` hiện có hai cấu hình build:

- **Bản build headless mặc định**: API query/status + `/reset_weights` trên
  query port, cùng `/customize` trên customize port.
- **Bản build UI** (`--features ui`): thêm `/evaluate_routes`,
  `/traffic_overlay`, `/camera_overlay`, và cho phép bật route-viewer tích hợp
  sẵn.

Lệnh build:

```bash
cd CCH-Hanoi

# Headless mặc định
cargo build --release -p hanoi-server

# Bản build có UI
cargo build --release -p hanoi-server --features ui
```

**Chế độ normal (headless mặc định)**:

```bash
hanoi_server \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-port 8080 \
  --customize-port 9080
```

**Chế độ line graph (headless mặc định)**:

```bash
hanoi_server \
  --graph-dir Maps/data/hanoi_car/line_graph \
  --original-graph-dir Maps/data/hanoi_car/graph \
  --query-port 8081 \
  --customize-port 9081 \
  --line-graph
```

**Bản build UI với route-viewer tích hợp**:

```bash
hanoi_server \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-port 8080 \
  --customize-port 9080 \
  --serve-ui
```

Trong bản build headless mặc định, CLI sẽ **không** chấp nhận `--serve-ui` hoặc
`--camera-config`. Hai cờ này chỉ có khi binary được build với `--features ui`.

### 6.2 Tham Số CLI


| Tham số                | Mặc định   | Mô tả                                                  |
| ---------------------- | ---------- | ------------------------------------------------------- |
| `--graph-dir`          | (bắt buộc) | Đường dẫn thư mục đồ thị                               |
| `--original-graph-dir` | (không)    | Bắt buộc cho chế độ `--line-graph`                      |
| `--camera-config`      | `CCH_Data_Pipeline/config/mvp_camera.yaml` | Đường dẫn YAML camera cho overlay camera (chỉ có trong bản build `--features ui`) |
| `--query-port`         | `8080`     | Port cho API truy vấn/info/health/ready                 |
| `--customize-port`     | `9080`     | Port cho API upload trọng số                            |
| `--serve-ui`           | `false`    | Phục vụ `/`, `/ui`, `/assets/*` từ query port (chỉ có trong bản build `--features ui`) |
| `--line-graph`         | `false`    | Bật định tuyến mở rộng theo lượt rẽ                     |
| `--log-format`         | `pretty`   | Định dạng log: `pretty`, `full`, `compact`, `tree`, `json` |
| `--log-dir`            | (không)    | Thư mục cho file log JSON xoay vòng theo ngày           |


### 6.3 Kiến Trúc Dual-Port

```
┌─────────────────────────────────────────────────────────────┐
│                       hanoi_server                          │
│                                                             │
│  Query Port (:8080)          Customize Port (:9080)         │
│  ┌──────────────────┐        ┌──────────────────────┐       │
│  │ POST /query      │        │ POST /customize      │       │
│  │ GET  /info       │        │   (binary body,      │       │
│  │ GET  /health     │        │    gzip tuỳ chọn)    │       │
│  │ GET  /ready      │        └──────────┬───────────┘       │
│  └───────┬──────────┘                   │                   │
│          │ mpsc channel                 │ watch channel      │
│          ▼                              ▼                   │
│  ┌─────────────────────────────────────────────────┐        │
│  │            Background Engine Thread              │        │
│  │  ┌─────────────────────────────────────────┐    │        │
│  │  │ QueryEngine / LineGraphQueryEngine       │    │        │
│  │  │   → CCH customization                   │    │        │
│  │  │   → truy vấn đường ngắn nhất            │    │        │
│  │  └─────────────────────────────────────────┘    │        │
│  └─────────────────────────────────────────────────┘        │
└─────────────────────────────────────────────────────────────┘
```

**Tại sao hai port**: Query port phục vụ consumer JSON API bên ngoài. Customize
port nhận vector trọng số nhị phân thô từ pipeline dữ liệu nội bộ. Tách riêng
cho phép kiểm soát truy cập và giới hạn kích thước body độc lập (64 MB cho
customize, giới hạn tiêu chuẩn cho query).

**Ma trận endpoint trên query port**:

- Bản build headless mặc định: `/query`, `/reset_weights`, `/info`,
  `/health`, `/ready`
- Bản build `--features ui`: thêm `/evaluate_routes`, `/traffic_overlay`,
  `/camera_overlay`
- Bản build `--features ui` + `--serve-ui`: gắn thêm `/`, `/ui`, `/assets/*`

### 6.4 Engine Background Thread

Engine chạy trong một OS thread riêng (không phải Tokio task) để tránh chặn
async runtime trong các thao tác CCH tốn CPU.

**Vòng lặp engine**:

```
loop:
  1. Kiểm tra watch channel (non-blocking):
     → nếu có trọng số mới: đặt customization_active=true, re-customize, đặt false
  2. Đợi message truy vấn (timeout 50ms):
     → nhận message: dispatch truy vấn, gửi kết quả qua oneshot
     → hết thời gian: quay lại vòng lặp (cho phép kiểm tra customization định kỳ)
     → channel đóng: thoát vòng lặp, đặt engine_alive=false
```

**Cơ chế watch channel**: Người ghi cuối cùng thắng. Nếu nhiều request
`/customize` đến trong khi customization đang chạy, chỉ vector trọng số mới
nhất được áp dụng. Đây là thiết kế có chủ đích cho cập nhật giao thông
thời gian thực.

### 6.5 Tắt Server Nhẹ Nhàng

Server xử lý SIGINT và SIGTERM:

1. Phát tín hiệu shutdown tới cả hai listener
2. Cả hai port ngừng nhận kết nối mới
3. Các request đang xử lý được drain nhẹ nhàng
4. Timeout 30 giây — nếu drain bị treo, buộc thoát với mã 1

### 6.6 Logging

Server có khả năng logging toàn diện nhất trong tất cả binary CCH-Hanoi. Hỗ trợ
cả năm định dạng đầu ra, lưu log vào file với xoay vòng hàng ngày, và tracing
HTTP request qua `tower-http`.

Để biết đầy đủ chi tiết về các tuỳ chọn logging, định dạng, biến môi trường và
khác biệt giữa các binary, xem [Phần 16: Hướng Dẫn Cấu Hình Logging](#16-hướng-dẫn-cấu-hình-logging).

**Ví dụ khởi động nhanh**:

```bash
# Mặc định: info level, HTTP debug, pretty output ra stderr
hanoi_server --graph-dir Maps/data/hanoi_car/graph

# JSON log ra stderr + file log xoay vòng theo ngày
hanoi_server --graph-dir Maps/data/hanoi_car/graph --log-format json --log-dir /var/log/hanoi/

# Debug mọi thứ
RUST_LOG=debug hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Hiển thị dạng cây phân cấp (chỉ server)
hanoi_server --graph-dir Maps/data/hanoi_car/graph --log-format tree
```

---

## 7. hanoi-gateway — API Gateway

Gateway cung cấp một điểm truy cập thống nhất, định tuyến truy vấn tới
backend phù hợp dựa trên **routing profile** của request (vd: `car`,
`motorcycle`). Toàn bộ cấu hình backend được định nghĩa trong file YAML.

### 7.1 Khởi Động Gateway

```bash
hanoi_gateway --config gateway.yaml
hanoi_gateway --config gateway.yaml --port 9000   # ghi đè port
```

### 7.2 Tham Số CLI

| Tham số    | Mặc định         | Mô tả                                       |
| ---------- | ---------------- | -------------------------------------------- |
| `--config` | `gateway.yaml`   | Đường dẫn tới file cấu hình YAML            |
| `--port`   | (từ config)      | Ghi đè port được định nghĩa trong file config |

### 7.3 Cấu Hình YAML

File cấu hình YAML là **nguồn sự thật duy nhất** cho gateway. Nó kiểm soát
port lắng nghe, timeout backend, logging, và — quan trọng nhất — tập hợp các
routing profile cùng URL backend tương ứng.

```yaml
port: 50051
backend_timeout_secs: 30       # 0 để tắt; mặc định 30
log_format: pretty             # pretty | full | compact | tree | json
# log_file: /var/log/gw.json  # bỏ trống để tắt ghi log file

profiles:
  car:
    backend_url: "http://localhost:8080"
  motorcycle:
    backend_url: "http://localhost:8081"
```

**Các trường config:**

| Trường                | Bắt buộc | Mặc định | Mô tả                                              |
| --------------------- | -------- | -------- | --------------------------------------------------- |
| `port`                | có       | —        | Port lắng nghe của gateway                          |
| `backend_timeout_secs`| không    | `30`     | Timeout HTTP client cho request tới backend (0 = không) |
| `log_format`          | không    | `pretty` | Định dạng log: pretty, full, compact, tree, json    |
| `log_file`            | không    | (không)  | Ghi log ra file song song ở định dạng JSON          |
| `profiles`            | có       | —        | Map tên profile -> cấu hình backend (>= 1 mục)     |
| `profiles.<name>.backend_url` | có | —    | URL gốc của backend routing server                  |

Gateway không quan tâm backend dùng đồ thị thường hay line graph — đó là việc
của backend. Nhờ vậy API gateway được tách biệt khỏi chi tiết topology đồ thị.
Thêm profile mới (vd: `truck`, `bicycle`) chỉ cần thêm mục trong YAML và có
backend server đang chạy.

**Ghi log ra file** — khi `log_file` được đặt, log được ghi ra **cả** stderr
và file đồng thời. Stderr dùng `log_format`; file luôn là JSON phân cách
dòng:

```bash
# Parse file JSON log bằng jq
jq 'select(.fields.message | contains("ready"))' /var/log/gw.json
```

### 7.4 Các Endpoint Gateway

| Phương thức | Đường dẫn   | Mô tả                                                      |
| ----------- | ----------- | ----------------------------------------------------------- |
| POST        | `/query`    | Truy vấn tuyến — `?profile=<name>` chọn backend            |
| GET         | `/info`     | Metadata backend — `?profile=<name>` (tuỳ chọn)            |
| GET         | `/profiles` | Liệt kê tất cả routing profile có sẵn                      |

Phản hồi `GET /profiles`:

```json
{ "profiles": ["car", "motorcycle"] }
```

### 7.5 Kiến Trúc Định Tuyến

```
             Client
               │
       POST /query?profile=car  {...}
               │
               ▼
        ┌─────────────┐
        │   Gateway    │ :50051
        │   (:50051)   │
        └──┬───────┬───┘
           │       │
   profile         profile
   ="car"          ="motorcycle"
           │       │
           ▼       ▼
     ┌─────────┐ ┌─────────┐
     │ Server  │ │ Server  │
     │ :8080   │ │ :8081   │
     │  (car)  │ │ (moto)  │
     └─────────┘ └─────────┘
```

**Gateway proxy những gì**: `/query` (POST) và `/info` (GET).

**Không proxy**: `/customize`, `/reset_weights`, `/health`, `/ready`.
Customization và thao tác reset về baseline được gửi trực tiếp tới API của
từng server. Health/ready check được công cụ orchestration thực hiện trực tiếp
với từng backend.

### 7.6 Truyền Lỗi

Gateway giữ nguyên mã HTTP status của backend. Nếu backend trả về 400 (vd:
xác thực toạ độ thất bại) hoặc 500, gateway chuyển tiếp nguyên mã status và
body JSON đó cho client.

Profile không xác định trả về HTTP 400 kèm danh sách các lựa chọn hợp lệ:

```json
{
  "error": "unknown profile: truck",
  "available_profiles": ["car", "motorcycle"]
}
```

---

## 8. hanoi-cli — Giao Diện Dòng Lệnh

CLI chạy truy vấn và tra cứu thông tin hoàn toàn trong process — không cần
server. Hữu ích cho truy vấn đơn lẻ, xác thực và scripting.

### 8.1 Lệnh Query

**Sử dụng cơ bản — Theo node ID**:

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-node 1000 \
  --to-node 5000
```

**Theo toạ độ**:

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

**Chế độ line graph**:

```bash
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --line-graph \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

**Tuyến thay thế**:

```bash
# Yêu cầu 3 tuyến thay thế với stretch 25%
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --alternatives 3 --stretch 1.25

# Chế độ line graph với tuyến thay thế
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --line-graph \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --alternatives 5
```

| Tham số          | Mặc định | Mô tả                                            |
| ---------------- | -------- | ------------------------------------------------- |
| `--alternatives` | `0`      | Số tuyến thay thế (0 = chỉ đường ngắn nhất)      |
| `--stretch`      | `1.25`   | Hệ số stretch địa lý tối đa cho tuyến thay thế   |

Khi `--alternatives > 0`, đầu ra chứa nhiều feature (GeoJSON) hoặc mảng
các đối tượng tuyến (JSON). Tuyến chỉ số 0 luôn là đường ngắn nhất.

**Định dạng đầu ra và tuỳ chọn file**:

Flag `--output-format` điều khiển định dạng đầu ra (mặc định: `geojson`):

```bash
# Định dạng GeoJSON (RFC 7946) — mặc định, phù hợp cho thư viện bản đồ
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-format geojson

# Định dạng JSON — toạ độ dạng [lat, lng]
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-format json
```

Kết quả luôn được ghi ra file. Nếu bỏ qua `--output-file`, file có tên tự
động theo timestamp sẽ được tạo trong thư mục hiện tại (vd:
`query_2026-03-19T143052.geojson`). Phần mở rộng file khớp với định dạng đầu
ra. Tóm tắt (khoảng cách, số node, đường dẫn file) được ghi ra stderr.

```bash
# File đầu ra chỉ định rõ
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --output-file result.geojson

# File tự động tạo tên (tạo query_<timestamp>.geojson)
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

**Ví dụ đầu ra GeoJSON** (định dạng mặc định):

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": {
        "type": "LineString",
        "coordinates": [[105.834, 21.028], [105.836, 21.025], ...]
      },
      "properties": {
        "distance_ms": 142300,
        "distance_m": 3842.7,
        "path_nodes": [1523, 1524, 1530, 1547, 1563]
      }
    }
  ]
}
```

**Ví dụ đầu ra JSON** (định dạng cũ):

```json
{
  "distance_ms": 142300,
  "distance_m": 3842.7,
  "path_nodes": [1523, 1524, 1530, 1547, 1563],
  "coordinates": [[21.028, 105.834], [21.025, 105.836], ...]
}
```

**Ghi log ra file** — đầu ra đồng thời ra stderr và file:

Mặc định, log ra stderr với định dạng màu. Khi chỉ định `--log-file`, log được
ghi ra **cả** stderr và file đồng thời. File luôn dùng định dạng **JSON** (phân
cách dòng), dễ đọc bằng máy và không bị ảnh hưởng bởi mã escape ANSI. Flag
`--log-format` chỉ ảnh hưởng đầu ra stderr.

Flag `--log-file` là flag cấp cao nhất — phải đặt **trước** subcommand:

```bash
# Log ra cả stderr (pretty, có màu) và query.log (JSON)
cch-hanoi --log-file query.log query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# --log-format chỉ ảnh hưởng stderr; file luôn là JSON
cch-hanoi --log-format compact --log-file query.log query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# Parse file JSON log bằng jq
jq '.fields.message' query.log
```

**Mã thoát**: 0 = thành công, 1 = không tìm thấy đường, 2 = xác thực toạ độ thất bại.

### 8.2 Lệnh Info

```bash
cch-hanoi info --data-dir Maps/data/hanoi_car
cch-hanoi info --data-dir Maps/data/hanoi_car --line-graph
```

**Đầu ra**:

```json
{
  "graph_type": "normal",
  "graph_dir": "Maps/data/hanoi_car/graph",
  "num_nodes": 276372,
  "num_edges": 654787
}
```

### 8.3 Quy Ước Thư Mục

Flag `--data-dir` trỏ tới thư mục dữ liệu **cha**. CLI tự động nối thêm
`/graph` cho chế độ normal, `/line_graph` và `/graph` cho chế độ line graph:

```
--data-dir Maps/data/hanoi_car
  Normal:     tải Maps/data/hanoi_car/graph/
  Line graph: tải Maps/data/hanoi_car/line_graph/ + Maps/data/hanoi_car/graph/
```

---

## 9. hanoi-tools — Công Cụ Pipeline

### 9.1 generate_line_graph

Chuyển đổi đồ thị đường gốc thành đồ thị mở rộng theo lượt rẽ (line graph).

```bash
# Xuất ra <graph_dir>/line_graph/ (mặc định)
generate_line_graph Maps/data/hanoi_car/graph

# Xuất ra thư mục chỉ định
generate_line_graph Maps/data/hanoi_car/graph Maps/data/hanoi_car/line_graph
```

**Tham số CLI**:


| Tham số        | Bắt buộc | Mô tả                                                |
| -------------- | -------- | ---------------------------------------------------- |
| `<graph_dir>`  | Có       | Thư mục đồ thị đầu vào (tham số vị trí)             |
| `<output_dir>` | Không    | Thư mục đầu ra (mặc định: `<graph_dir>/line_graph`) |
| `--log-format` | Không    | Định dạng log (mặc định: `pretty`)                   |


**File đầu vào** (từ `<graph_dir>/`):


| File                      | Bắt buộc | Mô tả                         |
| ------------------------- | -------- | ------------------------------ |
| `first_out`               | Có       | CSR offsets                    |
| `head`                    | Có       | CSR targets                    |
| `travel_time`             | Có       | Trọng số cạnh (mili giây)     |
| `latitude`                | Có       | Vĩ độ node                    |
| `longitude`               | Có       | Kinh độ node                   |
| `forbidden_turn_from_arc` | Có       | Nguồn lượt rẽ cấm (đã sắp xếp) |
| `forbidden_turn_to_arc`   | Có       | Đích lượt rẽ cấm (đã sắp xếp) |


**File đầu ra** (tới thư mục output):


| File          | Mô tả                                                             |
| ------------- | ----------------------------------------------------------------- |
| `first_out`   | CSR offsets line graph                                            |
| `head`        | Edge targets line graph                                           |
| `travel_time` | Trọng số line graph: `original_travel_time[e1] + turn_cost(e1, e2)` |
| `latitude`    | Toạ độ node line graph (= node đầu cạnh gốc)                     |
| `longitude`   | Toạ độ node line graph                                            |


**Hoạt động**:

1. Tải đồ thị gốc và lượt rẽ cấm
2. Xây dựng mảng tail (tra ngược cạnh -> node nguồn)
3. Liệt kê tất cả lượt rẽ có thể tại mỗi giao lộ
4. Lọc lượt rẽ cấm (sorted merge-scan, O(1) trung bình) và U-turn
5. Ghi đồ thị mở rộng

**Kích thước dự kiến** (đồ thị ô tô Hà Nội):

- Đầu vào: ~276K node, ~655K cạnh, ~403 lượt rẽ cấm
- Đầu ra: ~655K node (= cạnh gốc), ~1.3M cạnh (lượt rẽ hợp lệ)

---

## 10. hanoi-bench — Đo Hiệu Năng

### 10.1 Benchmark Core (Không Cần Server)

```bash
bench_core \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-count 1000 \
  --iterations 10 \
  --output core_results.json
```

**Đo những gì** (theo thứ tự):

1. Xây dựng CCH (`CchContext::load_and_build`)
2. Customization (`customize()`)
3. Xây dựng KD-tree (`SpatialIndex::build`)
4. Truy vấn theo node ID (`query()`)
5. Truy vấn theo toạ độ (`query_coords()`)
6. Snap-to-edge (`snap_to_edge()`)

**Tham số CLI**:


| Tham số              | Mặc định                     | Mô tả                        |
| -------------------- | ---------------------------- | ----------------------------- |
| `--graph-dir`        | (bắt buộc)                   | Đường dẫn thư mục đồ thị     |
| `--perm-path`        | `<graph_dir>/perms/cch_perm` | File thứ tự CCH              |
| `--query-count`      | `1000`                       | Số truy vấn mỗi vòng lặp     |
| `--iterations`       | `10`                         | Số vòng lặp đo                |
| `--warmup`           | `3`                          | Số vòng lặp khởi động         |
| `--seed`             | `42`                         | Seed RNG cho tái lập          |
| `--generate-queries` | (không)                      | Tạo N truy vấn ngẫu nhiên    |
| `--save-queries`     | (không)                      | Lưu truy vấn ra file JSON    |
| `--queries`          | (không)                      | Tải truy vấn từ file JSON    |
| `--output`           | `core_results.json`          | File kết quả đầu ra          |
| `--log-name`         | `bench_core`                 | Tiền tố tên file log tuỳ chỉnh |


### 10.2 Benchmark Server (Cần Server Đang Chạy)

```bash
# Truy vấn tuần tự
bench_server \
  --url http://localhost:8080 \
  --queries 1000 \
  --graph-dir Maps/data/hanoi_car/graph

# Test tải đồng thời
bench_server \
  --url http://localhost:8080 \
  --queries 1000 \
  --concurrency 10 \
  --graph-dir Maps/data/hanoi_car/graph
```

**Đo những gì**:

1. Độ trễ `GET /info`
2. Độ trễ `POST /query` tuần tự
3. Throughput `POST /query` đồng thời (với N client)

**Tham số CLI**:


| Tham số         | Mặc định                | Mô tả                         |
| --------------- | ----------------------- | ------------------------------ |
| `--url`         | `http://localhost:8080` | URL server                     |
| `--queries`     | `1000`                  | Số truy vấn                    |
| `--concurrency` | `1`                     | Số client đồng thời            |
| `--query-file`  | (không)                 | Tải tập truy vấn từ JSON      |
| `--graph-dir`   | (không)                 | Thư mục đồ thị để tạo truy vấn |
| `--seed`        | `42`                    | Seed RNG                       |
| `--output`      | `bench_results.json`    | File kết quả                   |
| `--log-name`    | `bench_server`          | Tiền tố tên file log tuỳ chỉnh |


### 10.3 Tạo Báo Cáo và So Sánh

```bash
# Tạo báo cáo từ kết quả
bench_report --input core_results.json --format table

# So sánh hai lần chạy để phát hiện suy giảm
bench_report \
  --baseline previous_results.json \
  --current current_results.json \
  --threshold 10
```

Mã thoát 1 nếu bất kỳ benchmark nào suy giảm quá `--threshold` phần trăm.
Cả ba binary cũng chấp nhận `--log-name <PREFIX>` để tuỳ chỉnh tiền tố tên
file log (mặc định: tên binary).

**Thống kê tính toán**: min, max, mean, median (p50), p95, p99, std_dev,
throughput (QPS), tỷ lệ thành công.

**Định dạng đầu ra**: `table` (dễ đọc), `json` (tích hợp CI), `csv`
(bảng tính).

### 10.4 Logging Benchmark

Tất cả binary bench tự động tạo file log trong thư mục hiện tại mỗi lần chạy.
Không cần flag CLI để bật — luôn bật mặc định.

- **Stderr**: Định dạng compact để theo dõi tiến trình
- **File**: Định dạng JSON để phân tích bằng máy
- **Tên file**: `{binary_name}_{timestamp}.log` (vd: `bench_core_2026-03-19T143052.log`)
- **Tiền tố tuỳ chỉnh**: `--log-name my_run` -> `my_run_2026-03-19T143052.log`
- **Bộ lọc**: Điều khiển qua biến môi trường `RUST_LOG` (mặc định: `info`)

Parse file log bằng `jq`:

```bash
# Xem tất cả sự kiện
cat bench_core_2026-03-19T143052.log | jq .

# Trích xuất thời gian các giai đoạn benchmark
cat bench_core_2026-03-19T143052.log | jq 'select(.fields.message != null) | .fields.message'
```

### 10.5 Micro-Benchmark Criterion

```bash
# Benchmark CCH
cargo bench --bench cch_bench -p hanoi-bench

# Benchmark spatial
cargo bench --bench spatial_bench -p hanoi-bench
```

### 10.6 Tập Truy Vấn Tái Lập

```bash
# Tạo và lưu tập truy vấn
bench_core --graph-dir ... --generate-queries 5000 --save-queries queries.json

# Tái sử dụng giữa các lần chạy
bench_core --graph-dir ... --queries queries.json --output run1.json
bench_core --graph-dir ... --queries queries.json --output run2.json
bench_report --baseline run1.json --current run2.json
```

---

## 11. Tham Chiếu HTTP API

### 11.1 POST /query — Truy Vấn Đường Ngắn Nhất

**Port**: Query port (mặc định 8080, hoặc gateway 50051)

**Request (theo toạ độ)**:

`POST /query` (mặc định: GeoJSON) | `POST /query?format=json` (JSON thuần) | `POST /query?colors` (GeoJSON với màu simplestyle-spec).

```json
{
  "from_lat": 21.028,
  "from_lng": 105.834,
  "to_lat": 21.006,
  "to_lng": 105.843
}
```

**Request (theo node ID)**:

```json
{
  "from_node": 1000,
  "to_node": 5000
}
```

**Request (tuyến thay thế)** — qua query parameter:

`POST /query?alternatives=3&stretch=1.25`

```json
{
  "from_lat": 21.028,
  "from_lng": 105.834,
  "to_lat": 21.006,
  "to_lng": 105.843
}
```

| Query Param    | Mặc định | Mô tả                                             |
| -------------- | -------- | -------------------------------------------------- |
| `alternatives` | `0`      | Số tuyến (0 = chỉ đường ngắn nhất)                |
| `stretch`      | `1.25`   | Hệ số stretch địa lý tối đa cho tuyến thay thế    |

Khi `alternatives > 0`, phản hồi là GeoJSON FeatureCollection với một Feature
mỗi tuyến (hoặc mảng JSON khi dùng `?format=json`). Mỗi feature có
`route_index` trong properties — chỉ số 0 luôn là đường ngắn nhất.

**Phản hồi (GeoJSON đa tuyến)** — `POST /query?alternatives=3&colors`:

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "geometry": { "type": "LineString", "coordinates": [...] },
      "properties": {
        "route_index": 0,
        "distance_ms": 142300,
        "distance_m": 3842.7,
        "stroke": "#e6194b",
        "stroke-width": 5
      }
    },
    {
      "type": "Feature",
      "geometry": { "type": "LineString", "coordinates": [...] },
      "properties": {
        "route_index": 1,
        "distance_ms": 158700,
        "distance_m": 4210.3,
        "stroke": "#3cb44b",
        "stroke-width": 3
      }
    }
  ]
}
```

**Request qua gateway** (profile trong query param):

```
POST /query?profile=car
Content-Type: application/json

{
  "from_lat": 21.028,
  "from_lng": 105.834,
  "to_lat": 21.006,
  "to_lng": 105.843
}
```

**Phản hồi (mặc định — GeoJSON)** — 200 OK:

```json
{
  "type": "FeatureCollection",
  "features": [{
    "type": "Feature",
    "geometry": {
      "type": "LineString",
      "coordinates": [[105.834, 21.028], [105.836, 21.025], ...]
    },
    "properties": {
      "distance_ms": 142300,
      "distance_m": 3842.7
    }
  }]
}
```

Lưu ý: Toạ độ GeoJSON theo thứ tự `[longitude, latitude]` theo RFC 7946
(ngược so với quy ước nội bộ). Khi không tìm thấy đường, `"geometry": null`.

**Phản hồi (định dạng JSON)** — `POST /query?format=json`:

```json
{
  "distance_ms": 142300,
  "distance_m": 3842.7,
  "path_nodes": [1523, 1524, 1530, 1547, 1563],
  "coordinates": [[21.028, 105.834], [21.025, 105.836], ...]
}
```

Khi không tìm thấy đường: `distance_ms`/`distance_m` là `null`, các mảng rỗng.

**Phản hồi lỗi** — 400 Bad Request (xác thực toạ độ):

```json
{
  "error": "coordinate_validation_failed",
  "message": "source coordinate (91.0, 105.8) is outside valid geographic range",
  "details": {
    "label": "source",
    "lat": 91.0,
    "lng": 105.8,
    "reason": "InvalidRange"
  }
}
```

### 11.2 POST /customize — Upload Vector Trọng Số

**Port**: Customize port (mặc định 9080)

**Request**: Body nhị phân thô — little-endian `[u32; num_edges]`

```bash
# Upload trọng số bằng curl
curl -X POST http://localhost:9080/customize \
  --data-binary @travel_time \
  -H "Content-Type: application/octet-stream"

# Với nén gzip
gzip -c travel_time | curl -X POST http://localhost:9080/customize \
  --data-binary @- \
  -H "Content-Type: application/octet-stream" \
  -H "Content-Encoding: gzip"
```

**Xác thực**:

- Kích thước body phải bằng `num_edges * 4` bytes
- Tất cả giá trị trọng số phải `< INFINITY` (< 2,147,483,647)
- Kích thước body tối đa: 64 MB

**Phản hồi thành công** — 200 OK:

```json
{
  "accepted": true,
  "message": "customization queued"
}
```

**Phản hồi lỗi** — 400 Bad Request:

```json
{
  "accepted": false,
  "message": "expected 2619148 bytes (654787 edges x 4), got 1000"
}
```

```json
{
  "accepted": false,
  "message": "weight[42] = 2147483647 exceeds maximum allowed value (2147483646)"
}
```

**Quan trọng**: `/customize` trả về 200 trước khi customization hoàn tất.
Handler xác thực và đưa vào hàng đợi; engine thread áp dụng bất đồng bộ. Để
xác nhận hoàn tất, poll `GET /info` và theo dõi `customization_active` chuyển
từ `true` sang `false`.

### 11.3 GET /info — Metadata Đồ Thị

**Port**: Query port (mặc định 8080)

```json
{
  "graph_type": "normal",
  "num_nodes": 276372,
  "num_edges": 654787,
  "customization_active": false,
  "bbox": {
    "min_lat": 20.899,
    "max_lat": 21.098,
    "min_lng": 105.701,
    "max_lng": 105.952
  }
}
```

Qua gateway: `GET /info?profile=car`

### 11.4 GET /health — Chỉ Số Vận Hành

**Port**: Query port (mặc định 8080). Luôn trả về 200.

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "total_queries_processed": 15230,
  "customization_active": false
}
```

### 11.5 GET /ready — Kiểm Tra Sẵn Sàng

**Port**: Query port (mặc định 8080).

- **200 OK**: `{"ready": true}` — engine thread còn sống
- **503 Service Unavailable**: `{"ready": false}` — engine thread đã chết

### 11.6 Bảng Tổng Hợp Endpoint


| Endpoint     | Phương thức | Port      | Mục đích            | Mã status  |
| ------------ | ----------- | --------- | ------------------- | ---------- |
| `/query`     | POST        | Query     | Truy vấn tuyến      | 200, 400   |
| `/reset_weights` | POST    | Query     | Đưa lại trọng số baseline vào hàng đợi | 200, 503 |
| `/info`      | GET         | Query     | Metadata đồ thị     | 200        |
| `/health`    | GET         | Query     | Chỉ số vận hành     | 200        |
| `/ready`     | GET         | Query     | Probe sẵn sàng      | 200, 503   |
| `/customize` | POST        | Customize | Upload trọng số      | 200, 400   |

**Các route chỉ dành cho UI** — chỉ có khi `hanoi-server` được build với
`--features ui`:

| Endpoint            | Phương thức | Port  | Mục đích                            | Mã status |
| ------------------- | ----------- | ----- | ----------------------------------- | --------- |
| `/evaluate_routes`  | POST        | Query | Đánh giá các tuyến GeoJSON nhập vào | 200, 400  |
| `/traffic_overlay`  | GET         | Query | Overlay giao thông theo viewport    | 200, 400  |
| `/camera_overlay`   | GET         | Query | Overlay camera theo viewport        | 200, 400  |

Các route frontend tĩnh `/`, `/ui`, và `/assets/*` chỉ được mount khi bản build
`--features ui` được chạy cùng `--serve-ui`.


---

## 12. Hướng Dẫn Tuỳ Chỉnh Trọng Số

### 12.1 Cơ Chế Customization

```
travel_time trên ổ đĩa: [1000, 2000, 3000, 5000, 8000]    ← không bao giờ thay đổi
                          │
                          │ tải một lần khi khởi động (baseline)
                          ▼
Trọng số baseline:   [1000, 2000, 3000, 5000, 8000]    ← không bao giờ bị sửa
                          │
                          │ POST /customize với vector mới
                          ▼
Vector thay thế:     [1000, 9999, 3000, 5000, 8000]    ← thay thế toàn bộ
                          │
                          │ CCH customize() (Giai đoạn 2)
                          ▼
CustomizedBasic:     upward_weights[...]                 ← truy vấn dùng cái này
                     downward_weights[...]
```

**Điểm chính**:

- Mỗi lần gọi `/customize` gửi một vector trọng số thay thế **hoàn chỉnh**
- Cập nhật KHÔNG tích luỹ — mỗi lần gọi bắt đầu từ đầu
- `travel_time` baseline (trên ổ đĩa) không bao giờ bị sửa đổi
- Độ dài vector trọng số phải chính xác bằng `num_edges` (kiểm tra qua `GET /info`)

### 12.2 Tạo Trọng Số Test

**Python — tạo nhanh**:

```python
import numpy as np

def read_u32(path):
    return np.fromfile(path, dtype=np.uint32)

def write_u32(path, data):
    data.astype(np.uint32).tofile(path)

# Đọc kích thước đồ thị
graph_dir = "Maps/data/hanoi_car/graph"
head = read_u32(f"{graph_dir}/head")
m = len(head)
print(f"Đồ thị có {m:,} cạnh")

# Chiến lược 1: Trọng số đồng nhất (10 giây mỗi cạnh)
weights = np.full(m, 10_000, dtype=np.uint32)

# Chiến lược 2: Ngẫu nhiên có biên (1s–60s mỗi cạnh, seed=42)
rng = np.random.default_rng(seed=42)
weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)

# Chiến lược 3: Dựa trên khoảng cách (từ toạ độ)
lat = np.fromfile(f"{graph_dir}/latitude", dtype=np.float32)
lng = np.fromfile(f"{graph_dir}/longitude", dtype=np.float32)
first_out = read_u32(f"{graph_dir}/first_out")
# (tính haversine cho mỗi cạnh, quy đổi sang mili giây)

# Ghi ra file
write_u32(f"{graph_dir}/travel_time", weights)
```

**Rust — tạo bằng code**:

```rust
use std::fs::File;
use std::io::Write;

fn write_u32_vec(path: &str, data: &[u32]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    let bytes = bytemuck::cast_slice(data);
    file.write_all(bytes)
}

// Trọng số đồng nhất 10 giây
let weights = vec![10_000u32; num_edges];
write_u32_vec("travel_time", &weights)?;
```

### 12.3 Upload Trọng Số Lên Server Đang Chạy

```bash
# Tạo trọng số ngẫu nhiên
python3 -c "
import numpy as np
m = $(python3 -c "import os; print(os.path.getsize('Maps/data/hanoi_car/graph/head') // 4)")
rng = np.random.default_rng(seed=42)
weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)
weights.tofile('test_weights.bin')
print(f'Đã ghi {m} trọng số ra test_weights.bin')
"

# Upload lên server
curl -X POST http://localhost:9080/customize \
  --data-binary @test_weights.bin \
  -H "Content-Type: application/octet-stream"

# Đợi customization hoàn tất
sleep 0.2
curl -s http://localhost:8080/info | python3 -m json.tool
```

### 12.4 Ràng Buộc Trọng Số


| Ràng buộc         | Giá trị                      | Lý do                                              |
| ----------------- | ---------------------------- | -------------------------------------------------- |
| Kiểu              | `u32` (little-endian)        | Định dạng nhị phân RoutingKit                      |
| Giá trị tối thiểu | 0 (dùng cẩn thận)           | Cạnh trọng số 0 có thể gây trường hợp biên thuật toán |
| Giá trị tối đa   | 2,147,483,646 (INFINITY - 1) | Server từ chối >= INFINITY                         |
| Phạm vi khuyến nghị | 1,000 – 10,000,000        | 1 giây – 2.7 giờ mỗi cạnh                         |
| Đơn vị            | Mili giây                    | `tt_units_per_s = 1000`                            |
| Độ dài vector     | Chính xác `num_edges`        | Phải khớp topology đồ thị                          |


### 12.5 Lưu Ý Trọng Số Line Graph

Với trọng số line graph, có hai cách tiếp cận:

**Cách A: Tạo từ đồ thị thường** (khuyến nghị để đảm bảo nhất quán)

1. Ghi `travel_time` test vào thư mục đồ thị thường
2. Chạy `generate_line_graph` — nó tự sinh trọng số line graph
3. Công thức trọng số line graph: `travel_time[turn_edge] = original_travel_time[e1]`

**Cách B: Trọng số line graph trực tiếp**

1. Ghi `travel_time` trực tiếp vào thư mục line graph
2. Phải có đúng `num_line_graph_edges` phần tử
3. **Quy tắc nhất quán**: `line_weight(e1 → e2) = original_travel_time[e1]` nếu
  muốn kết quả normal và line graph trùng nhau

---

## 13. Hướng Dẫn Kiểm Thử

### 13.1 Tổng Quan Chiến Lược Kiểm Thử

Quy trình kiểm thử trải qua ba giai đoạn:

```
Giai đoạn 1: Trọng Số Mặc Định
  → Dùng travel_time trên ổ đĩa (từ pipeline OSM)
  → Xác thực toàn bộ stack hoạt động end-to-end
  → Kiểm tra hành vi baseline

Giai đoạn 2: Trọng Số Ngẫu Nhiên Seed Cố Định
  → Tạo trọng số ngẫu nhiên xác định (seed=42)
  → Cùng đầu vào mỗi lần chạy = kết quả tái lập
  → Stress-test sự đa dạng trọng số

Giai đoạn 3: Nhiều Bộ Trọng Số
  → Tạo nhiều bộ trọng số seed cố định (seed=1, 2, 3, ...)
  → So sánh kết quả giữa các chu kỳ customization
  → Xác thực tính đúng đắn re-customization
```

### 13.2 Giai Đoạn 1: Test Với Trọng Số Mặc Định

**CLI (không cần server)**:

```bash
# Chế độ normal — truy vấn theo toạ độ
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# Chế độ line graph
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --line-graph \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843

# So sánh: cả hai chế độ nên cho khoảng cách tương tự
# (line graph có thể khác do áp dụng hạn chế rẽ)
```

**Server**:

```bash
# Khởi động server với trọng số mặc định
hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Truy vấn
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool

# Kiểm tra thông tin
curl -s http://localhost:8080/info | python3 -m json.tool

# Kiểm tra sức khoẻ
curl -s http://localhost:8080/health | python3 -m json.tool
```

**Những gì cần xác minh**:

- Phản hồi là GeoJSON FeatureCollection với một Feature duy nhất
- `features[0].properties.distance_ms` > 0 (tuyến tồn tại)
- `features[0].geometry.coordinates` không rỗng với các cặp `[lng, lat]` (thứ tự RFC 7946)
- Tất cả toạ độ nằm trong bounding box Hà Nội
- `GET /info` trả về `num_nodes` và `num_edges` đúng
- `GET /ready` trả về `{"ready": true}`

### 13.3 Giai Đoạn 2: Test Với Trọng Số Ngẫu Nhiên Seed Cố Định

**Tạo một bộ trọng số tái lập**:

```python
import numpy as np
import os

graph_dir = "Maps/data/hanoi_car/graph"
m = os.path.getsize(f"{graph_dir}/head") // 4

# Seed cố định = tái lập giữa các lần chạy
rng = np.random.default_rng(seed=42)
weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)
weights.tofile("test_weights_seed42.bin")
print(f"Đã tạo {m:,} trọng số, phạm vi [{weights.min():,}, {weights.max():,}]")
```

**Upload và test**:

```bash
# Upload trọng số ngẫu nhiên
curl -X POST http://localhost:9080/customize \
  --data-binary @test_weights_seed42.bin \
  -H "Content-Type: application/octet-stream"

# Đợi customization
sleep 0.3

# Chạy cùng truy vấn — khoảng cách phải khác Giai đoạn 1
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool
```

**Những gì cần xác minh**:

- Vẫn tìm thấy tuyến (không bị nhiễm INFINITY)
- Khoảng cách khác với baseline (customization đã được áp dụng)
- Đường đi có thể khác (trọng số khác -> đường ngắn nhất khác)
- `GET /info` hiện `customization_active: false` sau khi ổn định

### 13.4 Giai Đoạn 3: Nhiều Bộ Trọng Số (Xác Thực Re-Customization)

```python
import numpy as np
import requests
import time

graph_dir = "Maps/data/hanoi_car/graph"
m = os.path.getsize(f"{graph_dir}/head") // 4

# Truy vấn test
query = {
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
}

results = {}

for seed in [1, 2, 3, 42, 100]:
    # Tạo trọng số
    rng = np.random.default_rng(seed=seed)
    weights = rng.integers(1_000, 60_000, size=m, dtype=np.uint32)

    # Upload
    resp = requests.post(
        "http://localhost:9080/customize",
        data=weights.tobytes(),
        headers={"Content-Type": "application/octet-stream"}
    )
    assert resp.json()["accepted"]

    # Đợi customization
    time.sleep(0.3)

    # Truy vấn
    resp = requests.post("http://localhost:8080/query", json=query)
    result = resp.json()
    results[seed] = result["distance_ms"]
    print(f"Seed {seed:>3}: distance_ms = {result['distance_ms']}")

# Xác minh: các seed khác nhau phải cho khoảng cách khác nhau
assert len(set(results.values())) > 1, "Tất cả seed cho cùng khoảng cách!"
print("Xác thực re-customization thành công: trọng số khác → tuyến khác")
```

**Những gì cần xác minh**:

- Mỗi seed cho khoảng cách khác nhau (customization thực sự có hiệu lực)
- Không có kết quả cũ (watch channel cập nhật được áp dụng)
- Không crash hoặc panic trong quá trình re-customization liên tục
- Server health vẫn OK suốt quá trình

### 13.5 Test Gateway

```bash
# Khởi động cả hai server
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --query-port 8080 --customize-port 9080 &

hanoi_server --graph-dir Maps/data/hanoi_motorcycle/graph \
  --query-port 8081 --customize-port 9081 &

# Khởi động gateway với config
hanoi_gateway --config gateway.yaml &

# Liệt kê các profile có sẵn
curl -s http://localhost:50051/profiles | python3 -m json.tool

# Truy vấn qua gateway — profile ô tô
curl -s -X POST "http://localhost:50051/query?profile=car" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# Truy vấn qua gateway — profile xe máy
curl -s -X POST "http://localhost:50051/query?profile=motorcycle" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# Info qua gateway
curl -s "http://localhost:50051/info?profile=car" | python3 -m json.tool
curl -s "http://localhost:50051/info?profile=motorcycle" | python3 -m json.tool
```

### 13.6 Test Tuyến Thay Thế

**CLI**:

```bash
# Chế độ normal — 3 tuyến thay thế
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --alternatives 3

# Chế độ line graph — 5 tuyến thay thế với stretch tuỳ chỉnh
cch-hanoi query \
  --data-dir Maps/data/hanoi_car \
  --line-graph \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843 \
  --alternatives 5 --stretch 1.4
```

**Server**:

```bash
# GeoJSON đa tuyến có màu
curl -s -X POST "http://localhost:8080/query?alternatives=3&colors" \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool

# JSON đa tuyến
curl -s -X POST "http://localhost:8080/query?alternatives=3&format=json" \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool

# Qua gateway
curl -s -X POST "http://localhost:50051/query?profile=car&alternatives=3&colors" \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}' \
  | python3 -m json.tool
```

**Những gì cần xác minh**:

- Phản hồi có nhiều feature (GeoJSON) hoặc phần tử mảng (JSON)
- `route_index` 0 là khoảng cách ngắn nhất
- Mỗi tuyến có toạ độ khác biệt (các tuyến thay thế đa dạng về hình học)
- Với `?colors`, mỗi tuyến có màu `stroke` khác nhau
- Tất cả tuyến nằm trong giới hạn stretch địa lý

### 13.7 Test Định Dạng Phản Hồi

```bash
# Phản hồi mặc định là GeoJSON (không cần query param)
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# Định dạng JSON rõ ràng qua query parameter
curl -s -X POST "http://localhost:8080/query?format=json" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool

# GeoJSON với thuộc tính màu simplestyle-spec
curl -s -X POST "http://localhost:8080/query?colors" \
  -H "Content-Type: application/json" \
  -d '{
    "from_lat": 21.028, "from_lng": 105.834,
    "to_lat": 21.006, "to_lng": 105.843
  }' | python3 -m json.tool
```

**Xác minh**: Phản hồi mặc định có `geometry.coordinates` theo thứ tự `[longitude, latitude]`
(RFC 7946). `?format=json` trả về các trường `distance_ms`/`coordinates` phẳng.
`?colors` thêm `stroke`, `stroke-width`, `fill`, `fill-opacity` vào properties GeoJSON.

### 13.8 Test Trường Hợp Lỗi

```bash
# Toạ độ không hợp lệ (ngoài phạm vi)
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 91.0, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}'
# → 400 với coordinate_validation_failed

# Toạ độ xa đồ thị
curl -s -X POST http://localhost:8080/query \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 10.0, "from_lng": 100.0, "to_lat": 21.006, "to_lng": 105.843}'
# → 400 với OutOfBounds rejection

# Kích thước vector trọng số sai
echo "invalid" | curl -X POST http://localhost:9080/customize \
  --data-binary @- -H "Content-Type: application/octet-stream"
# → 400 với lỗi kích thước không khớp

# Profile không xác định qua gateway
curl -s -X POST "http://localhost:50051/query?profile=unknown" \
  -H "Content-Type: application/json" \
  -d '{"from_lat": 21.028, "from_lng": 105.834, "to_lat": 21.006, "to_lng": 105.843}'
# → 400 với lỗi profile không xác định và danh sách available_profiles
```

### 13.9 Benchmark Hiệu Năng Trong Quá Trình Test

```bash
# Benchmark core (không cần server)
bench_core \
  --graph-dir Maps/data/hanoi_car/graph \
  --query-count 1000 \
  --output baseline.json

# Benchmark server
bench_server \
  --url http://localhost:8080 \
  --queries 1000 \
  --concurrency 10 \
  --graph-dir Maps/data/hanoi_car/graph \
  --output server_baseline.json

# Sau khi thay đổi, so sánh để phát hiện suy giảm
bench_core --graph-dir Maps/data/hanoi_car/graph --output current.json
bench_report --baseline baseline.json --current current.json --threshold 10
```

### 13.10 Bảng Kiểm Tra Xác Thực


| Kiểm tra                              | Lệnh / Phương pháp                                                       | Kỳ vọng                   |
| ------------------------------------- | ------------------------------------------------------------------------- | -------------------------- |
| Đồ thị tải không lỗi                 | `cch-hanoi info --data-dir ...`                                           | JSON với số node/edge      |
| Truy vấn normal trả về tuyến         | `cch-hanoi query --data-dir ... --from-lat ... --to-lat ...`              | Thoát 0, khoảng cách > 0  |
| Truy vấn line graph trả về tuyến     | `cch-hanoi query --data-dir ... --line-graph --from-lat ... --to-lat ...` | Thoát 0, khoảng cách > 0  |
| Server khởi động và phục vụ          | `curl /health`                                                            | `{"status": "ok"}`         |
| Kiểm tra server sẵn sàng             | `curl /ready`                                                             | `{"ready": true}` (200)    |
| Customization được chấp nhận         | `curl POST /customize`                                                    | `{"accepted": true}`       |
| Customization thay đổi định tuyến    | So sánh khoảng cách trước/sau                                             | `distance_ms` khác nhau    |
| Re-customization hoạt động           | Upload nhiều bộ trọng số                                                  | Kết quả khác nhau mỗi bộ  |
| Xác thực toạ độ từ chối không hợp lệ | `curl` với toạ độ sai                                                     | Lỗi 400                   |
| Xác thực trọng số từ chối INFINITY   | Upload trọng số >= 2^31/2                                                 | Lỗi 400                   |
| Đa tuyến trả về các lựa chọn         | `curl POST /query?alternatives=3`                                         | Nhiều feature được trả về  |
| Gateway định tuyến đúng              | `curl POST /query?profile=car`                                            | Chuyển tới backend đúng    |
| Định dạng GeoJSON đúng (mặc định)    | `curl POST /query` (không query param)                                    | GeoJSON Feature hợp lệ    |
| Định dạng JSON qua query param       | `curl POST /query?format=json`                                            | Phản hồi JSON phẳng       |
| Tắt nhẹ nhàng                        | Gửi SIGTERM tới server                                                    | Thoát sạch trong 30s      |


---

## 14. Sơ Đồ Vận Hành

### 14.1 Luồng Khởi Động Server

```
Phân tích tham số CLI
        │
        ▼
  ┌─────────────────┐
  │ Tải dữ liệu     │  GraphData::load() — xác thực bất biến CSR
  │ đồ thị           │
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Xây dựng CCH    │  Giai đoạn 1 contraction (độc lập metric)
  │ (hoặc Directed  │  Chi phí một lần: vài giây đến vài phút
  │  CCH)            │
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Khởi tạo engine │  OS thread nền với channel truy vấn + customization
  │ thread           │  Customization ban đầu với trọng số baseline
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Bind TCP port    │  Query port + Customize port
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Cài đặt signal  │  SIGINT / SIGTERM → tắt nhẹ nhàng
  │ handler          │  Timeout buộc tắt 30 giây
  └────────┬────────┘
           │
           ▼
     Server sẵn sàng
     (chấp nhận request)
```

### 14.2 Luồng Xử Lý Truy Vấn

```
POST /query  {"from_lat": 21.028, "from_lng": 105.834, ...}
        │
        ▼
  ┌─────────────────┐
  │ Parse JSON       │  Deserialize QueryRequest
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Gửi tới engine  │  Qua mpsc channel (buffer 256)
  │ thread           │  Oneshot reply channel cho phản hồi
  └────────┬────────┘
           │
           ▼   (trong engine thread)
  ┌─────────────────┐
  │ Phát hiện dạng  │  Toạ độ → query_coords()
  │                  │  Node ID → query()
  └────────┬────────┘
           │ (nếu truy vấn toạ độ)
           ▼
  ┌─────────────────┐
  │ Xác thực toạ độ │  Phạm vi địa lý, bbox, khoảng cách snap
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Snap vào cạnh   │  KD-tree (k=10) + Haversine vuông góc
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Truy vấn CCH    │  Tìm kiếm elimination-tree hai chiều
  │ (Giai đoạn 3)   │  trên CustomizedBasic
  └────────┬────────┘
           │ nếu không có đường
           ▼
  ┌─────────────────┐
  │ Fallback: thử   │  Tất cả 4 tổ hợp endpoint
  │ node thay thế   │  (tail/head × nguồn/đích)
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Format phản hồi │  GeoJSON mặc định hoặc ?format=json
  └────────┬────────┘
           │
           ▼
     200 OK  {"distance_ms": ..., "path_nodes": [...], ...}
```

### 14.3 Luồng Customization

```
POST /customize  (binary body: [u32; num_edges])
        │
        ▼
  ┌─────────────────┐
  │ Xác thực kích   │  body.len() == num_edges * 4
  │ thước            │
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Copy vào        │  bytemuck: Bytes → Vec<u32>
  │ Vec<u32> aligned │  (Bytes không đảm bảo alignment 4 byte)
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Xác thực giá trị│  Tất cả trọng số < INFINITY (2,147,483,647)
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ Đưa vào hàng    │  watch_tx.send(Some(weights))
  │ đợi qua watch   │  Người ghi cuối cùng thắng nếu nhiều bản chờ
  │ channel          │
  └────────┬────────┘
           │
           ▼
     200 OK  {"accepted": true, "message": "customization queued"}
           │
           │  (bất đồng bộ, trong engine thread)
           ▼
  ┌─────────────────┐
  │ Re-customize     │  CCH Giai đoạn 2 với trọng số mới
  │ CCH              │  customization_active = true → false
  └─────────────────┘
```

### 14.4 Kiến Trúc Triển Khai Đầy Đủ (Hình T)

Hệ thống theo **kiến trúc hình T** — thanh dọc là lưu lượng truy vấn bên
ngoài chảy qua gateway, và thanh ngang là pipeline dữ liệu nội bộ đẩy trọng
số trực tiếp tới customize port của mỗi server.

```
                    Client Bên Ngoài
                    (ứng dụng, dashboard)
                         │
                    POST /query
                    GET  /info
                         │
                         ▼
              ┌─────────────────────┐
              │   API Gateway       │  :50051         ─┐
              │   (hanoi_gateway)   │                   │
              └──────┬─────┬────────┘                   │ Thanh dọc:
                     │     │                            │ lưu lượng truy vấn
        profile      │     │  profile                   │ bên ngoài
        ="car"       │     │  ="motorcycle"             │
                     │     │                            │
                     ▼     ▼                           ─┘
           ┌──────────┐  ┌──────────┐
           │ Server A  │  │ Server B  │
           │  (car)    │  │  (moto)   │
           │ :8080     │  │ :8081     │
           │ :9080     │  │ :9081     │
           └─────┬─────┘  └─────┬─────┘
                 ▲              ▲
                 │              │
       POST /customize   POST /customize              ── Thanh ngang:
                 │              │                         pipeline trọng số
                 └──────┬───────┘                         nội bộ
                        │
        ┌───────────────┴────────────────┐
        │   Pipeline Xử Lý Dữ Liệu       │
        │   (tạo vector trọng số          │
        │    từ dữ liệu giao thông thực) │
        └────────────────────────────────┘
```

**Đặc tính thiết kế quan trọng**: Gateway **không bao giờ** proxy `/customize`.
Cập nhật trọng số chảy trực tiếp từ pipeline tới customize port của mỗi server.
Nhờ vậy gateway stateless và thuần tuý phục vụ consumer, trong khi lưu lượng
upload trọng số (lên tới 8 MB mỗi lần cập nhật) ở trên mạng nội bộ.

### 14.5 Pipeline Dữ Liệu Tích Hợp (Dự Kiến)

Hệ thống end-to-end đầy đủ mở rộng ra ngoài các server định tuyến CCH.
Pipeline xử lý dữ liệu — chịu trách nhiệm chuyển đổi dữ liệu giao thông thô
thành vector trọng số `travel_time` khả dụng — nằm ở upstream của endpoint
`/customize`. Phần này mô tả kiến trúc tích hợp dự kiến.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        PIPELINE DỮ LIỆU                                │
│                                                                         │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────────────┐    │
│  │ Nguồn Dữ     │     │ Thu Thập     │     │   Xử Lý Dữ Liệu     │    │
│  │ Liệu Giao    │────▶│ Dữ Liệu     │────▶│                      │    │
│  │ Thông         │     │              │     │ Huber-robust Double  │    │
│  │               │     │ Thu thập,    │     │ Exponential          │    │
│  │ • Probe data  │     │ xác thực,   │     │ Smoothing (DES)      │    │
│  │ • Loop detectors    │ khử trùng   │     │                      │    │
│  │ • Floating car│     │              │     │ Lọc nhiễu,          │    │
│  │ • API feeds   │     │              │     │ xử lý ngoại lệ,     │    │
│  └──────────────┘     └──────────────┘     │ tạo ước lượng tốc    │    │
│                                             │ độ đã làm trơn       │    │
│                                             └──────────┬───────────┘    │
│                                                        │               │
│                                                        ▼               │
│                        ┌──────────────────────────────────────────┐    │
│                        │          Mô Hình Trọng Số                │    │
│                        │                                          │    │
│                        │  Tốc độ đã làm trơn + khoảng cách tuyến  │    │
│                        │        → travel_time (mili giây)         │    │
│                        │                                          │    │
│                        │  Mô hình tuỳ chỉnh: ánh xạ tốc độ      │    │
│                        │  giao thông sang thời gian đi mỗi cạnh, │    │
│                        │  có tính khoảng cách địa lý (haversine)  │    │
│                        │                                          │    │
│                        │  Khái niệm công thức:                    │    │
│                        │    travel_time[e] = f(speed[e],          │    │
│                        │                      distance[e])        │    │
│                        │                                          │    │
│                        │  Đầu ra: Vec<u32> [num_edges]            │    │
│                        │          (mili giây, < INFINITY)         │    │
│                        └──────────────────┬───────────────────────┘    │
│                                           │                            │
└───────────────────────────────────────────┼────────────────────────────┘
                                            │
                              Vector trọng số khả dụng
                              (nhị phân thô, little-endian u32)
                                            │
                         ┌──────────────────┴──────────────────┐
                         │                                     │
                         ▼                                     ▼
           POST /customize :9080                 POST /customize :9081
           ┌──────────────────┐                  ┌──────────────────┐
           │   Server A       │                  │   Server B       │
           │   (car)          │                  │   (motorcycle)   │
           │                  │                  │                  │
           │   CCH Giai đoạn  │                  │   CCH Giai đoạn  │
           │   2: re-customize│                  │   2: re-customize│
           │   với trọng số   │                  │   với trọng số   │
           │   mới            │                  │   mới            │
           └──────────────────┘                  └──────────────────┘
                  │                                     │
                  │         ┌─────────────┐             │
                  └────────▶│ API Gateway │◀────────────┘
                            │  (:50051)   │
                            └──────┬──────┘
                                   │
                                   ▼
                            Client Bên Ngoài
                            POST /query
                            GET  /info
```

#### Mô Tả Các Giai Đoạn Pipeline

**Giai đoạn 1 — Nguồn Dữ Liệu Giao Thông**: Quan sát giao thông thô từ bất
kỳ tổ hợp nào của probe vehicle, loop detector, floating car data, API feed bên
thứ ba, hoặc tập dữ liệu lịch sử. Định dạng và nguồn là việc của pipeline —
các server định tuyến không cần biết.

**Giai đoạn 2 — Thu Thập Dữ Liệu**: Thu thập, xác thực, khử trùng lặp và
chuẩn hoá quan sát giao thông thô. Đảm bảo chất lượng dữ liệu trước khi xử
lý thống kê. Xử lý dữ liệu thiếu, căn chỉnh timestamp, và đặc thù từng
nguồn.

**Giai đoạn 3 — Xử Lý Dữ Liệu (Huber-robust Double Exponential Smoothing)**:
Làm trơn thống kê quan sát tốc độ thô thành ước lượng tốc độ ổn định.

- **Double Exponential Smoothing (DES)**: Nắm bắt cả mức và xu hướng trong
dữ liệu chuỗi thời gian tốc độ, thích ứng với thay đổi mẫu giao thông dần
dần (vd: khởi đầu giờ cao điểm buổi sáng, giảm tải buổi tối)
- **Robustification bằng Huber loss**: Thay thế hàm mất mát bình phương sai
số chuẩn bằng hàm Huber loss — bậc hai cho sai số nhỏ nhưng tuyến tính cho
sai số lớn. Nhờ vậy phép làm trơn **kháng ngoại lệ** (vd: probe GPS báo
200 km/h trên đường dân sinh, hoặc đọc tốc độ 0 tạm thời từ xe đang dừng)
- **Đầu ra**: Ước lượng tốc độ đã làm trơn theo từng đoạn đường (km/h hoặc
m/s), theo sát điều kiện giao thông thực mà không bị giật bởi quan sát
nhiễu đơn lẻ

**Giai đoạn 4 — Mô Hình Trọng Số**: Chuyển đổi ước lượng tốc độ đã làm trơn
thành giá trị `travel_time` (mili giây) phù hợp cho engine định tuyến CCH.

- **Đầu vào**: Tốc độ đã làm trơn mỗi đoạn đường + khoảng cách địa lý đoạn
- **Mô hình**: Hàm ánh xạ tuỳ chỉnh có tính khoảng cách cạnh (Haversine
giữa toạ độ đầu cuối) và tốc độ đã làm trơn. Công thức baseline là
`travel_time[e] = distance_m[e] / speed_m_per_s[e] * 1000`, nhưng mô hình
có thể bổ sung hiệu chỉnh cho delay giao lộ, điều chỉnh theo loại đường,
hoặc phi tuyến tính tắc nghẽn
- **Ràng buộc**: Đầu ra phải là `u32`, trong phạm vi `[1, INFINITY)` với
`INFINITY = 2,147,483,647`. Tránh trọng số 0 (trường hợp biên thuật toán).
Giá trị tính bằng mili giây
- **Đầu ra**: `Vec<u32>` đầy đủ có độ dài `num_edges` — một trọng số cho mỗi
cạnh có hướng trong đồ thị, sẵn sàng upload

**Giai đoạn 5 — POST /customize**: Vector trọng số khả dụng được upload dưới
dạng nhị phân thô (`application/octet-stream`, little-endian `[u32; num_edges]`)
tới customize port của mỗi server. Server xác thực, đưa vào hàng đợi, và
engine thread re-customize CCH bất đồng bộ (Giai đoạn 2).

#### Nhịp Cập Nhật

Pipeline được thiết kế cho **cập nhật snapshot định kỳ**: mỗi X giây (cấu
hình được), pipeline tạo vector trọng số hoàn chỉnh mới phản ánh điều kiện
giao thông hiện tại. Mỗi lần upload là **thay thế toàn bộ** — không tích luỹ,
không thưa. Phù hợp cho dữ liệu giao thông dạng snapshot trong đó mỗi
cửa sổ quan sát tạo ra bức tranh hoàn chỉnh.

#### Hợp Đồng Biên

Giao diện giữa pipeline dữ liệu và server định tuyến được thiết kế hẹp có chủ
đích:


| Phạm vi            | Trách nhiệm pipeline                            | Trách nhiệm server                              |
| ------------------- | ------------------------------------------------ | ------------------------------------------------ |
| Nguồn dữ liệu      | Thu thập, xác thực, chuẩn hoá                   | Không cần biết                                   |
| Ước lượng tốc độ    | Huber-robust DES smoothing                       | Không cần biết                                   |
| Tính trọng số       | Mô hình: tốc độ × khoảng cách → ms              | Không cần biết                                   |
| Định dạng trọng số  | `Vec<u32>`, độ dài = `num_edges`, tất cả < INFINITY | Xác thực kích thước + giá trị                |
| Vận chuyển          | HTTP POST nhị phân thô                           | Nhận, giải nén (gzip tuỳ chọn)                  |
| Lịch trình          | Quyết định khi nào đẩy                          | Nhận bất kỳ lúc nào; watch-channel người ghi cuối thắng |
| CCH customization   | Không cần biết                                   | Re-customization Giai đoạn 2                     |
| Phục vụ truy vấn    | Không cần biết                                   | Truy vấn Giai đoạn 3 với trọng số mới nhất      |


Sự tách biệt này cho phép thay thế, nâng cấp, hoặc mở rộng mỗi bên một cách
độc lập. Pipeline không cần biết về CCH, và server không cần biết về định dạng
dữ liệu giao thông hay thuật toán làm trơn.

---

## 15. Xử Lý Sự Cố

### 15.1 Các Vấn Đề Thường Gặp

**"failed to load graph"**

- Kiểm tra tất cả file cần thiết có tồn tại trong thư mục đồ thị
- Xác minh kích thước file: `first_out` phải có `(n+1) * 4` bytes, `head` và
`travel_time` phải có `m * 4` bytes
- Kiểm tra bất biến CSR: `python3 -c "import struct; fo = struct.unpack(...); assert fo[0] == 0"`

**"failed to bind query port"**

- Port đang bị chiếm: `lsof -i :8080`
- Dùng port khác: `--query-port 8082 --customize-port 9082`

**"--original-graph-dir required for --line-graph mode"**

- Chế độ line graph cần truy cập đồ thị gốc để ánh xạ đường đi và hiệu chỉnh
cạnh cuối

**Customization dường như không có tác dụng**

- `/customize` bất đồng bộ — đợi hoàn tất
- Poll `GET /info` cho đến khi `customization_active: false`
- Xác minh độ dài vector trọng số khớp `num_edges` từ `/info`

**Truy vấn trả về kết quả rỗng (không có đường)**

- Xác minh toạ độ nằm trong bounding box đồ thị (kiểm tra `GET /info`)
- Thử truy vấn theo node ID để loại trừ vấn đề snap
- Kiểm tra nguồn và đích có cùng thành phần liên thông

**Gateway trả về 502 Bad Gateway**

- Backend server không chạy hoặc không truy cập được
- Kiểm tra URL backend trong tham số gateway
- Xác minh health backend: `curl http://localhost:8080/health`

### 15.2 Lệnh Debug Hữu Ích

```bash
# Kiểm tra kích thước đồ thị
python3 -c "
import os
d = 'Maps/data/hanoi_car/graph'
print(f'Nodes: {os.path.getsize(f\"{d}/first_out\") // 4 - 1:,}')
print(f'Edges: {os.path.getsize(f\"{d}/head\") // 4:,}')
print(f'Perm:  {os.path.getsize(f\"{d}/perms/cch_perm\") // 4:,}')
"

# Xác minh kích thước perm khớp số node
python3 -c "
import os
d = 'Maps/data/hanoi_car/graph'
n = os.path.getsize(f'{d}/first_out') // 4 - 1
p = os.path.getsize(f'{d}/perms/cch_perm') // 4
assert n == p, f'KHÔNG KHỚP: {n} nodes vs {p} perm entries'
print(f'OK: {n:,} nodes = {p:,} perm entries')
"

# Bật debug logging
RUST_LOG=debug hanoi_server --graph-dir ...

# Kiểm tra trạng thái server
curl -s http://localhost:8080/health | python3 -m json.tool
curl -s http://localhost:8080/ready  | python3 -m json.tool
curl -s http://localhost:8080/info   | python3 -m json.tool
```

### 15.3 Kỳ Vọng Hiệu Năng


| Thao tác                | Đồ thị thường | Line Graph | Ghi chú             |
| ----------------------- | ------------- | ---------- | ------------------- |
| Tải đồ thị              | < 1s          | < 2s       | Phụ thuộc I/O ổ đĩa |
| Xây dựng CCH (GĐ 1)    | 2–10s         | 5–20s      | Phụ thuộc CPU, một lần |
| Customization (GĐ 2)    | 50–200ms      | 100–500ms  | Mỗi lần cập nhật trọng số |
| Truy vấn (GĐ 3)         | < 1ms         | < 2ms      | Mỗi truy vấn       |
| Đa tuyến (K=3)          | 5–50ms        | 10–100ms   | Mỗi truy vấn       |
| Xây dựng KD-tree         | < 1s          | < 2s       | Một lần             |
| Snap-to-edge             | < 0.1ms       | < 0.1ms    | Mỗi toạ độ         |


Đo trên đồ thị ô tô Hà Nội (~276K node / ~655K cạnh cho normal, ~655K node /
~1.3M cạnh cho line graph). Hiệu năng thực tế thay đổi tuỳ phần cứng.

---

## 16. Hướng Dẫn Cấu Hình Logging

Tất cả binary CCH-Hanoi sử dụng hệ sinh thái [tracing](https://docs.rs/tracing)
cho structured logging. Phần này bao quát mọi tuỳ chọn, định dạng và cấu hình
logging có sẵn trong workspace.

### 16.1 Stack Logging

```
┌─────────────────────────────────────────────────────────────────────┐
│                     Application Code                                │
│  tracing::info!(), tracing::debug!(), #[tracing::instrument]       │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ tracing events & spans
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  tracing-subscriber registry                        │
│                                                                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │  EnvFilter   │  │  fmt layer   │  │  file layer (tuỳ chọn)   │  │
│  │  (RUST_LOG)  │  │  (stderr)    │  │  (JSON, xoay vòng ngày)  │  │
│  └──────────────┘  └──────────────┘  └──────────────────────────┘  │
│                                                                     │
│  ┌──────────────────────────────────┐                               │
│  │  tower-http TraceLayer          │  (hanoi-server & gateway)      │
│  │  (tự động tạo span HTTP request)│                                │
│  └──────────────────────────────────┘                               │
└─────────────────────────────────────────────────────────────────────┘
```

**Thành phần chính**:

- **EnvFilter**: Parse biến môi trường `RUST_LOG` để điều khiển mức log theo
từng module. Dùng giá trị mặc định riêng mỗi binary nếu `RUST_LOG` không được đặt.
- **fmt layer**: Format tracing event ra stderr với một trong năm định dạng đầu
ra có thể chọn.
- **file layer** (chỉ hanoi-server): Ghi log JSON vào file xoay vòng hàng ngày
qua background writer non-blocking.
- **TraceLayer** (hanoi-server và hanoi-gateway): Tự động tạo span cho HTTP
request, ghi lại method, path, status code và latency.

### 16.2 Khả Năng Logging Theo Từng Binary

| Khả năng                    | `hanoi_server` | `cch-hanoi` | `hanoi_gateway` | `generate_line_graph` | `hanoi-bench` |
| --------------------------- | -------------- | ----------- | --------------- | --------------------- | ------------- |
| Flag `--log-format`         | Có             | Có          | Có              | Có                    | Không         |
| Ghi đè bằng `RUST_LOG`     | Có             | Có          | Có              | Có                    | Có            |
| `--log-dir` ghi file        | Có             | Không       | Không           | Không                 | Không         |
| Ghi file tự động            | Không          | Không       | Không           | Không                 | Có            |
| Định dạng pretty            | Có             | Có          | Có              | Có                    | —             |
| Định dạng full              | Có             | Có          | Có              | Có                    | —             |
| Định dạng compact           | Có             | Có          | Có              | Có                    | Stderr        |
| Định dạng tree              | Có (native)    | Fallback¹   | Fallback¹       | Fallback¹             | —             |
| Định dạng json              | Có             | Có          | Có              | Có                    | File          |
| HTTP request tracing        | Có             | Không       | Có              | Không                 | Không         |
| Bộ lọc mặc định            | `info,tower_http=debug` | `info` | `info` | `info`           | `info`        |

¹ Fallback sang định dạng Full vì `tracing-tree` chỉ là dependency của
`hanoi-server`.

**Lưu ý về `hanoi-bench`**: Cả ba binary bench (`bench_core`, `bench_server`,
`bench_report`) dùng tracing subscriber dual-output. Stderr nhận định dạng
compact cho tiến trình; file log JSON luôn được tạo trong thư mục hiện tại
(xem Phần 10.4). Dùng `--log-name <PREFIX>` để tuỳ chỉnh tiền tố tên file.

### 16.3 Flag `--log-format`

Tất cả binary (trừ hanoi-bench) chấp nhận `--log-format <FORMAT>` để chọn định
dạng đầu ra stderr. Mặc định là `pretty`.

**Quan trọng**: Với `cch-hanoi`, `--log-format` là flag **cấp cao nhất** phải
đặt trước subcommand:

```bash
# Đúng:
cch-hanoi --log-format json query --data-dir Maps/data/hanoi_car ...

# Sai (sẽ báo lỗi):
cch-hanoi query --log-format json --data-dir Maps/data/hanoi_car ...
```

Với các binary khác, `--log-format` đặt cùng các flag khác:

```bash
hanoi_server --log-format compact --graph-dir Maps/data/hanoi_car/graph
hanoi_gateway --log-format json --port 50051
generate_line_graph --log-format full Maps/data/hanoi_car/graph
```

### 16.4 Tham Chiếu Định Dạng Đầu Ra

#### Pretty (mặc định)

Đầu ra nhiều dòng, có màu với vị trí file nguồn. Định dạng dễ đọc nhất cho
phát triển và sử dụng tương tác.

```bash
hanoi_server --log-format pretty --graph-dir Maps/data/hanoi_car/graph
```

```
  2026-03-19T10:30:00.123456Z  INFO hanoi_core::routing::normal::context: preparing CCH
    at crates/hanoi-core/src/routing/normal/context.rs:32
    in hanoi_core::routing::normal::context::load_and_build with graph_dir: Maps/data/hanoi_car/graph

  2026-03-19T10:30:05.456789Z  INFO hanoi_server: server ready
    query_addr: 0.0.0.0:8080
    customize_addr: 0.0.0.0:9080
    mode: normal
    ui_enabled: false
```

**Đặc điểm**:
- Nhiều dòng với trường thụt lề
- Mã màu ANSI (tắt bằng `NO_COLOR=1`)
- Hiện đầy đủ file nguồn + số dòng
- Context span hiện inline
- Phù hợp nhất cho: phát triển, debug, terminal tương tác

#### Full

Đầu ra một dòng với đầy đủ span context và thread ID. Cân bằng tốt giữa
khả năng đọc và mật độ.

```bash
hanoi_server --log-format full --graph-dir Maps/data/hanoi_car/graph
```

```
2026-03-19T10:30:00.123Z  INFO hanoi_core::routing::normal::context: preparing CCH num_nodes=276372 num_edges=654787
2026-03-19T10:30:05.456Z  INFO ThreadId(01) hanoi_server: server ready query_addr=0.0.0.0:8080 customize_addr=0.0.0.0:9080 mode=normal ui_enabled=false
```

**Đặc điểm**:
- Một dòng mỗi event
- Hiện target module (`hanoi_core::routing::normal::context`)
- Hiện thread ID
- Hiển thị trường gọn (`key=value`)
- Phù hợp nhất cho: terminal production, theo dõi log

#### Compact

Định dạng một dòng rút gọn với target module. Định dạng text ngắn gọn nhất.

```bash
hanoi_server --log-format compact --graph-dir Maps/data/hanoi_car/graph
```

```
2026-03-19T10:30:00.123Z  INFO hanoi_core::routing::normal::context: preparing CCH
2026-03-19T10:30:05.456Z  INFO hanoi_server: server ready
```

**Đặc điểm**:
- Định dạng một dòng ngắn nhất
- Hiện target module
- Các trường có thể bị rút gọn hoặc bỏ qua
- Phù hợp nhất cho: log khối lượng lớn cần ngắn gọn

#### Tree (chỉ hanoi-server)

Đầu ra phân cấp thụt lề, hiển thị trực quan span lồng nhau và event của chúng.
Dùng `tracing-tree` với deferred span và span retrace.

```bash
hanoi_server --log-format tree --graph-dir Maps/data/hanoi_car/graph
```

```
hanoi_core::routing::normal::context::load_and_build
  graph_dir: Maps/data/hanoi_car/graph
  0ms  INFO preparing CCH
  ┌ hanoi_core::routing::normal::context::customize
  │ 150ms  INFO customization complete
  └
5012ms  INFO hanoi_server: server ready
```

**Đặc điểm**:
- Cấu trúc cây thụt lề với ký tự indent (`│`, `┌`, `└`)
- Hiện target module
- Deferred span (render khi event con đầu tiên xảy ra)
- Span retrace (render lại context cha khi cần)
- Độ rộng thụt lề: 2 dấu cách
- Phù hợp nhất cho: hiểu luồng thực thi, debug thao tác lồng nhau

**Phạm vi hỗ trợ**: Chỉ `hanoi-server` có dependency `tracing-tree`. Các binary
khác (`cch-hanoi`, `hanoi_gateway`, `generate_line_graph`) chấp nhận `--log-format tree`
nhưng tự động fallback sang định dạng `full`.

#### Json

JSON phân cách dòng. Mỗi event là một đối tượng JSON. Không có mã màu ANSI.

```bash
hanoi_server --log-format json --graph-dir Maps/data/hanoi_car/graph
```

```json
{"timestamp":"2026-03-19T10:30:00.123456Z","level":"INFO","target":"hanoi_core::routing::normal::context","fields":{"message":"preparing CCH","num_nodes":276372,"num_edges":654787},"spans":[{"name":"load_and_build","graph_dir":"Maps/data/hanoi_car/graph"}]}
{"timestamp":"2026-03-19T10:30:05.456789Z","level":"INFO","target":"hanoi_server","fields":{"message":"server ready","query_addr":"0.0.0.0:8080","customize_addr":"0.0.0.0:9080","mode":"normal","ui_enabled":false}}
```

**Đặc điểm**:
- Một đối tượng JSON mỗi dòng (NDJSON / JSON Lines)
- Không có mã escape ANSI
- Trường có cấu trúc được giữ nguyên dạng key JSON
- Span context nằm trong mảng `spans`
- Đọc được bằng máy
- Phù hợp nhất cho: log aggregation (ELK, Loki, Splunk, Datadog), pipeline CI,
phân tích bằng chương trình

### 16.5 Bảng So Sánh Định Dạng

| Định dạng | Dòng/Event | Màu | Vị trí nguồn | Thread ID | Span Context | Đọc bằng máy |
| --------- | ---------- | --- | ------------- | --------- | ------------ | ------------- |
| `pretty`  | Nhiều      | Có  | Có            | Không     | Inline       | Không         |
| `full`    | 1          | Có  | Không         | Có        | Inline       | Không         |
| `compact` | 1          | Có  | Không         | Không     | Rút gọn      | Không         |
| `tree`    | Nhiều      | Có  | Không         | Không     | Phân cấp     | Không         |
| `json`    | 1          | Không | Không       | Không     | Mảng JSON    | Có            |

### 16.6 Biến Môi Trường `RUST_LOG`

`RUST_LOG` điều khiển event log nào được cho qua bộ lọc. Nó ghi đè giá trị
mặc định của binary. Cú pháp tuân theo định dạng
[`tracing-subscriber` EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html).

#### Bộ Lọc Mặc Định (khi `RUST_LOG` không được đặt)

| Binary                | Bộ lọc mặc định             | Lý do                                              |
| --------------------- | --------------------------- | -------------------------------------------------- |
| `hanoi_server`        | `info,tower_http=debug`     | Hiện chi tiết HTTP request để debug                |
| `cch-hanoi`           | `info`                      | Công cụ chạy một lần; ít nhiễu                     |
| `hanoi_gateway`       | `info`                      | Proxy; HTTP tracing qua tower-http ở info là đủ    |
| `generate_line_graph` | `info`                      | Công cụ pipeline; logging mức tiến trình           |

#### Cú Pháp Bộ Lọc

```bash
# Mức toàn cục
RUST_LOG=debug                    # Mọi thứ từ debug trở lên
RUST_LOG=warn                     # Chỉ warning và error
RUST_LOG=trace                    # Mức chi tiết tối đa (rất nhiều)

# Mức theo module (phân cách bằng dấu phẩy)
RUST_LOG=info,hanoi_core=debug    # Info toàn cục, debug cho hanoi_core
RUST_LOG=warn,hanoi_server=info   # Warn toàn cục, info cho server crate
RUST_LOG=info,tower_http=trace    # Info toàn cục, trace cho HTTP layer

# Chi tiết theo target
RUST_LOG=hanoi_core::routing::normal::context=trace    # Trace build/customize CCH normal-graph
RUST_LOG=hanoi_core::geo::spatial::index=debug         # Debug spatial indexing
RUST_LOG=hanoi_server::runtime::worker=trace           # Trace engine thread
RUST_LOG=hanoi_server::api::handlers=debug             # Debug HTTP handler

# Kết hợp nhiều target
RUST_LOG="info,hanoi_core::routing::normal::context=debug,hanoi_core::geo::spatial::index=debug,tower_http=debug"
```

#### Công Thức Thường Dùng

```bash
# Phát triển: xem mọi thứ bao gồm chi tiết CCH
RUST_LOG=debug hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Production: chỉ info, không nhiễu HTTP
RUST_LOG=info hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Debug vấn đề snap toạ độ
RUST_LOG="info,hanoi_core::geo::spatial::index=debug" cch-hanoi --log-format full \
  query --data-dir Maps/data/hanoi_car --from-lat 21.028 --from-lng 105.834 ...

# Debug hiệu năng CCH customization
RUST_LOG="info,hanoi_core::routing::normal::context=trace,hanoi_server::runtime::worker=trace" \
  hanoi_server --log-format full --graph-dir Maps/data/hanoi_car/graph

# Debug hành vi proxy gateway
RUST_LOG="debug,hyper=info" hanoi_gateway --log-format full

# Tắt mọi thứ trừ error
RUST_LOG=error hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Debug tải đồ thị (I/O file dữ liệu)
RUST_LOG="info,hanoi_core::graph::data=debug" cch-hanoi query --data-dir Maps/data/hanoi_car ...
```

### 16.7 Ghi Log Vào File (Chỉ hanoi-server)

Flag `--log-dir` bật ghi log file bền vững song song với stderr. Tính năng này
chỉ có cho `hanoi_server`.

#### Cách Hoạt Động

```
hanoi_server --log-dir /var/log/hanoi/ --log-format pretty --graph-dir ...
                  │                            │
                  │                            └─ điều khiển định dạng stderr
                  │
                  ▼
         /var/log/hanoi/
         └── hanoi-server.log.2026-03-19   ← luôn JSON, bất kể --log-format
         └── hanoi-server.log.2026-03-20   ← file mới lúc nửa đêm
         └── hanoi-server.log.2026-03-21
```

**Hành vi chính**:

- **Định dạng file luôn là JSON** — bất kể `--log-format`. Nhờ vậy file log
luôn đọc được bằng máy cho công cụ aggregation, dù người vận hành chọn pretty
hoặc tree trên stderr.
- **Xoay vòng hàng ngày** — file mới được tạo lúc nửa đêm (giờ địa phương).
Tên file là `hanoi-server.log.YYYY-MM-DD`.
- **Ghi non-blocking** — event log được ghi qua background thread bằng
`tracing-appender::non_blocking`. Ứng dụng chính không bao giờ bị chặn bởi
file I/O.
- **Vòng đời WorkerGuard** — writer non-blocking trả về `WorkerGuard` phải
được giữ suốt vòng đời chương trình. Drop sẽ flush và đóng writer. Server
giữ guard này trong `app::bootstrap::run()`.
- **Đầu ra kép** — layer stderr và file chạy đồng thời. Cả hai nhận cùng
event (lọc bởi cùng `RUST_LOG` / bộ lọc mặc định).

#### Cách Dùng

```bash
# Pretty stderr + JSON file log
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --log-format pretty --log-dir /var/log/hanoi/

# JSON khắp nơi (stderr + file)
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --log-format json --log-dir /var/log/hanoi/

# Compact stderr cho monitoring + JSON file cho aggregation
hanoi_server --graph-dir Maps/data/hanoi_car/graph \
  --log-format compact --log-dir /var/log/hanoi/
```

#### Parse File Log

```bash
# Theo dõi log ngày hiện tại
tail -f /var/log/hanoi/hanoi-server.log.$(date +%Y-%m-%d)

# Lọc error bằng jq
cat /var/log/hanoi/hanoi-server.log.2026-03-19 | jq 'select(.level == "ERROR")'

# Trích xuất event customization
cat /var/log/hanoi/hanoi-server.log.2026-03-19 | jq 'select(.fields.message | contains("customiz"))'

# Đếm truy vấn theo giờ
cat /var/log/hanoi/hanoi-server.log.2026-03-19 \
  | jq -r 'select(.fields.message == "query") | .timestamp[:13]' \
  | sort | uniq -c
```

### 16.8 HTTP Request Tracing

`hanoi-server` và `hanoi-gateway` có `TraceLayer` của `tower-http`, tự động
tạo tracing span cho mọi HTTP request.

**Ghi lại những gì** (ở mức `tower_http=debug`):

- Request: method, URI, version, header
- Response: status code, latency
- Body: kích thước (nếu biết)

**Điều khiển qua RUST_LOG**:

```bash
# Bao gồm chi tiết HTTP request (mặc định server)
RUST_LOG="info,tower_http=debug" hanoi_server --graph-dir ...

# Tracing HTTP đầy đủ (rất chi tiết — bao gồm header, thông tin body)
RUST_LOG="info,tower_http=trace" hanoi_server --graph-dir ...

# Tắt HTTP tracing (chỉ log ứng dụng)
RUST_LOG="info,tower_http=warn" hanoi_server --graph-dir ...
```

### 16.9 Các Điểm Code Được Instrument

Thư viện `hanoi-core` và server crate phát tracing event có cấu trúc tại các
điểm chính trong pipeline định tuyến. Đây là các log message bạn sẽ thấy ở các
mức khác nhau:

#### Event Mức Info (hiện mặc định)

| Module nguồn               | Message                              | Trường                              | Khi nào                             |
| -------------------------- | ------------------------------------ | ----------------------------------- | ----------------------------------- |
| `hanoi_core::routing::normal::context`     | preparing CCH                        | `num_nodes`, `num_edges`            | Bắt đầu chuẩn bị normal-graph Phase 1 |
| `hanoi_core::routing::line_graph::context` | preparing DirectedCCH for line graph | `num_nodes`, `num_edges`            | Bắt đầu chuẩn bị line-graph Phase 1 |
| `hanoi_core::geo::spatial::index`          | bounding box computed                | `min_lat`, `max_lat`, `min_lng`, `max_lng` | Thiết lập spatial index      |
| `hanoi_server::runtime::worker`            | re-customizing                       | `num_weights`                       | Bắt đầu customization Giai đoạn 2  |
| `hanoi_server::runtime::worker`            | customization complete               | —                                   | Kết thúc customization Giai đoạn 2 |
| `hanoi_server::runtime::worker`            | re-customizing line graph            | `num_weights`                       | Bắt đầu Giai đoạn 2 line graph     |
| `hanoi_server::runtime::worker`            | line graph customization complete    | —                                   | Kết thúc Giai đoạn 2 line graph    |
| `hanoi_server::api::handlers::customize`   | customization weights accepted, queued for engine thread | —            | Xác thực `/customize` thành công   |
| `hanoi_server::app::bootstrap`             | server ready                         | `query_addr`, `customize_addr`, `mode`, `ui_enabled` | Cả hai port đã bind và phục vụ |

#### Event Mức Warning

| Module nguồn               | Message                              | Trường                              | Khi nào                             |
| -------------------------- | ------------------------------------ | ----------------------------------- | ----------------------------------- |
| `hanoi_server::api::handlers::query` | coordinate validation failed | `rejection`                         | Toạ độ không hợp lệ trong `/query` |

#### Event Mức Debug (cần `RUST_LOG=debug` hoặc chỉ định module)

| Module nguồn               | Message                              | Trường                              | Khi nào                             |
| -------------------------- | ------------------------------------ | ----------------------------------- | ----------------------------------- |
| `hanoi_core::graph::data`  | loading graph data from disk         | `dir`                               | Bắt đầu GraphData::load()          |
| `hanoi_core::graph::data`  | graph data loaded                    | `num_nodes`, `num_edges`            | Hoàn tất GraphData::load()         |

#### Span Được Instrument (cho timing và lồng nhau)

| Module nguồn               | Tên span             | Trường                              | Bao bọc                            |
| -------------------------- | -------------------- | ----------------------------------- | ----------------------------------- |
| `hanoi_core::routing::normal::context`     | `load_and_build`     | `graph_dir`                         | Toàn bộ pipeline normal-graph Phase 1 |
| `hanoi_core::routing::normal::context`     | `customize`          | —                                   | Customization normal-graph Phase 2 |
| `hanoi_core::routing::normal::context`     | `customize_with`     | `num_weights`                       | Phase 2 normal-graph với trọng số tuỳ chỉnh |
| `hanoi_core::routing::normal::engine`      | `query`              | `from`, `to`                        | Truy vấn CCH đơn lẻ               |
| `hanoi_core::routing::normal::engine`      | `query_coords`       | `from`, `to`                        | Truy vấn theo toạ độ              |
| `hanoi_core::routing::line_graph::context` | `load_and_build`     | `line_graph_dir`, `original_graph_dir` | Toàn bộ pipeline line-graph Phase 1 |
| `hanoi_core::routing::line_graph::context` | `customize`          | —                                   | Giai đoạn 2 line graph            |
| `hanoi_core::routing::line_graph::context` | `customize_with`     | `num_weights`                       | Giai đoạn 2 line graph với trọng số tuỳ chỉnh |
| `hanoi_core::geo::spatial::index`          | `build`              | `num_nodes`                         | Xây dựng KD-tree                  |
| `hanoi_core::geo::spatial::index`          | `snap_to_edge`       | `lat`, `lng`                        | Thao tác snap-to-edge đơn lẻ     |
| `hanoi_core::geo::spatial::index`          | `validated_snap`     | `label`, `lat`, `lng`               | Snap có xác thực bbox/khoảng cách |
| `hanoi_server::runtime::worker`            | `customization`      | `num_weights`                       | Khối customization engine thread  |

### 16.10 Tắt Màu ANSI

Các định dạng text (`pretty`, `full`, `compact`, `tree`) phát mã escape màu
ANSI mặc định. Để tắt màu (hữu ích khi pipe ra file hoặc môi trường không
phải terminal):

```bash
# Biến môi trường chuẩn (được tracing-subscriber tôn trọng)
NO_COLOR=1 hanoi_server --graph-dir Maps/data/hanoi_car/graph

# Hoặc dùng định dạng json (không bao giờ phát mã ANSI)
hanoi_server --log-format json --graph-dir Maps/data/hanoi_car/graph
```

### 16.11 Cấu Hình Khuyến Nghị

#### Phát Triển Nội Bộ

```bash
# Pretty output, debug level cho lĩnh vực quan tâm
RUST_LOG="info,hanoi_core::routing::normal::context=debug" hanoi_server \
  --log-format pretty \
  --graph-dir Maps/data/hanoi_car/graph
```

#### Triển Khai Production

```bash
# Compact stderr cho dashboard monitoring + JSON file cho aggregation
RUST_LOG="info,tower_http=info" hanoi_server \
  --log-format compact \
  --log-dir /var/log/hanoi/ \
  --graph-dir Maps/data/hanoi_car/graph
```

#### CI / Test Tự Động

```bash
# JSON ra stderr để parse có cấu trúc, chỉ warning trở lên
RUST_LOG=warn hanoi_server \
  --log-format json \
  --graph-dir Maps/data/hanoi_car/graph
```

#### Debug CLI Nhanh

```bash
# Định dạng full với debug để xem chi tiết truy vấn
RUST_LOG="debug" cch-hanoi --log-format full \
  query --data-dir Maps/data/hanoi_car \
  --from-lat 21.028 --from-lng 105.834 \
  --to-lat 21.006 --to-lng 105.843
```

#### Debug Công Cụ Pipeline

```bash
# Trace quá trình tạo line graph
RUST_LOG="debug" generate_line_graph --log-format full Maps/data/hanoi_car/graph
```
