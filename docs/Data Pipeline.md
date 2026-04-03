# Data Pipeline — Luong du lieu trong he thong CCH Hanoi

> **Pham vi**: Mo ta toan bo luong du lieu tu file ban do goc (OSM PBF)
> den khi server phuc vu query, bao gom cac endpoint tac dong den
> trang thai du lieu tai runtime.
>
> **Cac module lien quan**:
>
> - `CCH-Generator/` — tao do thi CSR tu OSM
> - `RoutingKit/` — trich xuat re co dieu kien, tinh toan IFC ordering
> - `CCH-Hanoi/crates/hanoi-tools/` — tao line graph (do thi mo rong re)
> - `CCH-Hanoi/crates/hanoi-core/` — engine CCH (load, customize, query)
> - `CCH-Hanoi/crates/hanoi-server/` — HTTP server (Axum)
> - `CCH_Data_Pipeline/` — client Kotlin (batch query, phan tich)
> - `scripts/pipeline` — dieu phoi pipeline end-to-end
> - `scripts/start_server.sh` — khoi dong server

---

## Muc luc

1. [Tong quan pipeline](#1-tong-quan-pipeline)
2. [Offline Pipeline — Tu OSM den du lieu do thi](#2-offline-pipeline--tu-osm-den-du-lieu-do-thi)
3. [Server Startup — Load du lieu vao bo nho](#3-server-startup--load-du-lieu-vao-bo-nho)
4. [Runtime — Cac endpoint va tac dong den luong du lieu](#4-runtime--cac-endpoint-va-tac-dong-den-luong-du-lieu)
5. [Mo hinh dong thoi va cach ly du lieu](#5-mo-hinh-dong-thoi-va-cach-ly-du-lieu)
6. [Luong du lieu logic loi — CCH Query](#6-luong-du-lieu-logic-loi--cch-query)
7. [Luong du lieu logic loi — Customization](#7-luong-du-lieu-logic-loi--customization)
8. [Luong du lieu logic loi — Multi-Route](#8-luong-du-lieu-logic-loi--multi-route)
9. [Luong du lieu logic loi — Line Graph Query](#9-luong-du-lieu-logic-loi--line-graph-query)
10. [Luong du lieu logic loi — Spatial Snap & Validation](#10-luong-du-lieu-logic-loi--spatial-snap--validation)
11. [Luong du lieu logic loi — Turn Annotation](#11-luong-du-lieu-logic-loi--turn-annotation)
12. [Dinh dang du lieu RoutingKit](#12-dinh-dang-du-lieu-routingkit)
13. [Cau truc thu muc du lieu](#13-cau-truc-thu-muc-du-lieu)

---

## 1. Tong quan pipeline

He thong gom hai giai doan chinh:

```
┌─────────────────────────────────────────────────────┐
│  OFFLINE PIPELINE (chay 1 lan khi ban do thay doi)  │
│                                                     │
│  OSM PBF ──→ Graph CSR ──→ Conditional Turns        │
│                  ──→ Line Graph ──→ CCH Ordering     │
│                                                     │
│  Ket qua: Maps/data/{map}_{profile}/                │
└─────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────┐
│  RUNTIME (server hoat dong lien tuc)                │
│                                                     │
│  Load du lieu ──→ Build CCH ──→ Customize           │
│                                  ──→ Phuc vu query  │
│                                                     │
│  Endpoint /query      → doc du lieu, tra ket qua    │
│  Endpoint /customize  → thay trong so, re-customize │
└─────────────────────────────────────────────────────┘
```

---

## 2. Offline Pipeline — Tu OSM den du lieu do thi

Dieu phoi boi `scripts/pipeline <map_source> <profile>`.
Moi buoc co validation bang `CCH-Generator/lib/validate_graph`.

### Phase 1 — Tao do thi tu OSM

```
Tool:   CCH-Generator/lib/cch_generator
Input:  Maps/hanoi.osm.pbf
Output: Maps/data/hanoi_{profile}/graph/
```

Doc file OSM PBF, loc theo profile (car/motorcycle), xay do thi CSR:

| File output | Kieu | Mo ta |
|---|---|---|
| `first_out` | u32[] | Node → edge offset (CSR sentinel) |
| `head` | u32[] | Edge → node dich |
| `travel_time` | u32[] | Trong so canh (mili giay) |
| `geo_distance` | u32[] | Khoang cach dia ly (met) |
| `latitude` | f32[] | Vi do node |
| `longitude` | f32[] | Kinh do node |
| `way` | u32[] | Edge → OSM way ID |
| `forbidden_turn_from_arc` | u32[] | Canh nguon cua re cam (da sap xep) |
| `forbidden_turn_to_arc` | u32[] | Canh dich cua re cam (da sap xep) |
| `via_way_chain_*` | u32[] | Chuoi han che via-way |

Cong thuc travel_time: `geo_distance[m] × 18000 / speed[km/h] / 5` → mili giay.

### Phase 2 — Trich xuat re co dieu kien

```
Tool:   RoutingKit/bin/conditional_turn_extract
Input:  OSM PBF + graph/ tu Phase 1
Output: Maps/data/hanoi_{profile}/conditional_turns/
```

| File output | Mo ta |
|---|---|
| `conditional_turn_from_arc` | Canh nguon cua re co dieu kien |
| `conditional_turn_to_arc` | Canh dich cua re co dieu kien |
| `conditional_turn_time_windows` | Khung gio hoat dong (packed binary) |

Vi du: "cam re trai 7h–9h sang" → trich xuat cac khung gio ap dung.

### Phase 3 — Tao Line Graph (do thi mo rong re)

```
Tool:   CCH-Hanoi/target/release/generate_line_graph
Input:  graph/ tu Phase 1
Output: Maps/data/hanoi_{profile}/line_graph/
```

Chuyen do thi duong thanh do thi re:
- Moi **node** trong line graph = mot **canh** trong do thi goc
- Moi **canh** trong line graph = mot **luot re** hop le (2 canh lien tiep)
- Re cam → khong tao canh
- U-turn → hien tai tao canh voi chi phi 0 (xem Van de 2 trong tai lieu
  dieu tra)

| File output | Mo ta |
|---|---|
| `first_out`, `head`, `travel_time` | CSR cua do thi re |
| `latitude`, `longitude` | Toa do (map tu tail node goc) |
| `via_way_split_map` | **Bat buoc** — anh xa split node ve node goc |

### Phase 4 — CCH Ordering cho do thi thuong

```
Tool:   rust_road_router/flow_cutter_cch_order.sh
        rust_road_router/flow_cutter_cch_cut_order.sh
        rust_road_router/flow_cutter_cch_cut_reorder.sh
Input:  graph/
Output: graph/perms/cch_perm, cch_perm_cuts, cch_perm_cuts_reorder
```

Tinh toan nested dissection ordering bang InertialFlowCutter.
`cch_perm` la hoan vi node xac dinh thu tu co (contraction) cua CCH.

### Phase 5 — CCH Ordering cho Line Graph

```
Tool:   rust_road_router/flow_cutter_cch_order.sh
Input:  line_graph/
Output: line_graph/perms/cch_perm
```

Tuong tu Phase 4 nhung cho do thi re (DirectedCCH).

---

## 3. Server Startup — Load du lieu vao bo nho

Khoi dong: `scripts/start_server.sh [profile]`

```
hanoi_server
    --graph-dir      Maps/data/hanoi_{profile}/line_graph
    --original-graph-dir  Maps/data/hanoi_{profile}/graph
    --line-graph
    --query-port     8081
    --customize-port 9081
```

### Trinh tu khoi tao

```
1. Parse CLI args
2. Init tracing (log format, optional file output)
3. Tao channel: mpsc(256) cho query, watch cho customize
4. Load du lieu:
   ├── Normal mode:
   │   CchContext::load_and_build(graph_dir, cch_perm)
   │     → Doc: first_out, head, travel_time, lat, lng, geo_distance
   │     → Build CCH topology (Phase 1 CCH)
   │     → Luu baseline_weights = travel_time.clone()
   │     → Luu geo_distance_weights (hoac clone tu travel_time)
   │
   └── Line-graph mode:
       LineGraphCchContext::load_and_build(lg_dir, original_dir, cch_perm)
         → Doc line graph: first_out, head, travel_time, lat, lng
         → Doc via_way_split_map (bat buoc)
         → Doc original graph: first_out, head, travel_time, lat, lng
         → Tai tao tail array + mo rong cho split nodes
         → Build DirectedCCH (co huong, pruned)
         → Build spatial index tren toa do goc

5. Spawn engine thread (1 thread duy nhat):
   ├── Tao QueryEngine hoac LineGraphQueryEngine
   │     → Customize lan dau (baseline weights)
   │     → Build spatial index (KD-tree)
   └── Vao vong lap xu ly query + customize

6. Bind 2 port: query (8081) + customize (9081)
7. Server san sang
```

### Du lieu trong bo nho sau khoi tao

```
CchContext / LineGraphCchContext (bat bien sau khoi tao)
├── graph: GraphData
│   ├── first_out, head          ← CSR topology
│   ├── travel_time              ← trong so goc (ms)
│   ├── latitude, longitude      ← toa do node
│   └── geo_distance             ← khoang cach dia ly (met)
├── cch: CCH / DirectedCCH       ← topology CCH (bat bien)
├── baseline_weights: Vec<u32>   ← trong so goc (copy tu travel_time)
└── geo_distance_weights: Vec<u32> ← trong so dia ly (met)

QueryEngine / LineGraphQueryEngine (co the cap nhat)
├── server: CchQueryServer<CustomizedBasic>
│   └── customized data          ← THAY DOI khi /customize
├── spatial: SpatialIndex        ← KD-tree (bat bien)
└── context: &CchContext         ← tham chieu den context bat bien
```

---

## 4. Runtime — Cac endpoint va tac dong den luong du lieu

### 4.1 Query Port (mac dinh: 8081)

#### `POST /query` — Tim duong

```
Client                        Server
  │                             │
  │── POST /query ────────────→ │
  │   {from_lat, from_lng,      │
  │    to_lat, to_lng,          │
  │    ?alternatives=3,         │
  │    ?stretch=1.3}            │
  │                             ▼
  │                      handle_query()
  │                             │
  │                      Tao QueryMsg + oneshot channel
  │                             │
  │                      mpsc::send(QueryMsg) ───→ Engine Thread
  │                                                    │
  │                                              ┌─────┴─────┐
  │                                              │alternatives│
  │                                              │   > 0 ?    │
  │                                              └─────┬─────┘
  │                                       Khong /       \ Co
  │                                            ▼         ▼
  │                                     query()    multi_query()
  │                                            │         │
  │                                            │   customize_geo()
  │                                            │   → CustomizedBasic
  │                                            │     (tam, chi doc)
  │                                            │         │
  │                                            ▼         ▼
  │                                      Ket qua (1 hoac nhieu tuyen)
  │                                              │
  │                      oneshot::send(result) ←──┘
  │                             │
  │←── JSON/GeoJSON ───────────│
```

**Tac dong den du lieu**: **KHONG** — chi doc, khong sua doi bat ky
trang thai nao. `customize_geo()` tao `CustomizedBasic` tam thoi,
bi drop sau khi multi_query ket thuc.

| Truong hop | Du lieu doc | Du lieu ghi |
|---|---|---|
| Single query | `server` (customized weights) | Khong |
| Multi query | `baseline_weights` + `geo_distance_weights` | Khong (CustomizedBasic tam thoi) |

#### `GET /info` — Thong tin do thi

```json
{
    "graph_type": "line_graph",
    "num_nodes": 245892,
    "num_edges": 612340,
    "customization_active": false,
    "bbox": { "min_lat": 20.85, "max_lat": 21.15, ... }
}
```

**Tac dong**: Chi doc metadata va flag. Khong anh huong du lieu.

#### `GET /health` — Trang thai hoat dong

```json
{
    "status": "ok",
    "uptime_seconds": 3600,
    "total_queries_processed": 1250,
    "customization_active": false
}
```

**Tac dong**: Chi doc counter va flag. Khong anh huong du lieu.

#### `GET /ready` — Kiem tra san sang

Tra ve 200 neu engine thread con song, 503 neu da chet.

**Tac dong**: Chi doc `engine_alive` flag.

---

### 4.2 Customize Port (mac dinh: 9081)

#### `POST /customize` — Thay doi trong so

Day la **endpoint duy nhat thay doi trang thai du lieu** tai runtime.

```
Client (pipeline/external)           Server
  │                                    │
  │── POST /customize ───────────────→ │
  │   Body: [u32; num_edges]           │
  │   (raw binary, co the gzip)        │
  │                                    ▼
  │                             handle_customize()
  │                                    │
  │                             1. Validate body size
  │                                = num_edges × 4 bytes
  │                                    │
  │                             2. Cast bytes → Vec<u32>
  │                                    │
  │                             3. Validate: moi weight < INFINITY
  │                                (ngan overflow khi relax tam giac)
  │                                    │
  │                             4. watch_tx.send(Some(weights))
  │                                    │
  │←── {"accepted": true} ────────────│
  │                                    │
  │                                    ▼  (bat dong bo)
  │                             Engine Thread
  │                                    │
  │                             watch_rx.has_changed()? → Co
  │                                    │
  │                             customization_active = true
  │                                    │
  │                             engine.update_weights(&weights)
  │                               → context.customize_with(&weights)
  │                               → CustomizedBasic MOI
  │                               → server.update(new_customized)
  │                               → CustomizedBasic CU bi drop
  │                                    │
  │                             customization_active = false
  │                                    │
  │                             query tiep theo dung TRONG SO MOI
```

**Tac dong den du lieu**:

| Thanh phan | Truoc /customize | Sau /customize |
|---|---|---|
| `context.baseline_weights` | Khong doi | **Khong doi** (giu nguyen goc) |
| `context.cch` (topology) | Khong doi | **Khong doi** |
| `engine.server` (CustomizedBasic) | Customized tu baseline | **THAY THE** bang customized tu weights moi |
| Moi query sau do | Dung baseline weights | **Dung weights moi** |

**Dac diem quan trong**:

1. **Bat dong bo**: Handler tra ve ngay `"accepted": true`. Qua trinh
   re-customize dien ra trong engine thread (mat ~100-300ms).

2. **Khong tich luy**: Moi lan `/customize` thay THE TOAN BO vector trong so,
   khong phai cap nhat tang phan. Muon quay ve goc thi gui lai
   `baseline_weights`.

3. **Block query**: Trong luc `customize_with()` dang chay, engine thread
   khong xu ly query nao. Query xep hang trong buffer mpsc(256).

4. **Last-write-wins**: Neu gui 2 lan `/customize` lien tiep, lan gui sau
   se ghi de lan truoc khi engine thread poll `watch_rx`.

---

### 4.3 So do tong hop tac dong endpoint

```
                    ┌──────────────────────────────┐
                    │     Du lieu trong bo nho      │
                    │                              │
                    │  ┌─────────────────────────┐ │
  GET /info ───────►│  │ CchContext (BAT BIEN)   │ │◄─── Khong ai thay doi
  GET /health ─────►│  │  .baseline_weights      │ │
  GET /ready ──────►│  │  .geo_distance_weights  │ │
                    │  │  .cch (topology)         │ │
                    │  │  .graph (CSR)            │ │
                    │  └─────────────────────────┘ │
                    │                              │
                    │  ┌─────────────────────────┐ │
  POST /query ─────►│  │ QueryEngine             │ │
       (DOC)        │  │  .server ◄──────────────│─│── POST /customize
                    │  │    (CustomizedBasic)     │ │      (GHI)
                    │  │                         │ │
                    │  │  .spatial (BAT BIEN)     │ │
                    │  └─────────────────────────┘ │
                    └──────────────────────────────┘

  /query  → Chi DOC server.customized → khong anh huong gi
  /customize → THAY THE server.customized → moi query sau dung data moi
  /info, /health, /ready → doc metadata/flag → khong anh huong gi
```

---

### 4.4 Chi tiet tung HTTP Handler — Du lieu yeu cau va thoi gian giu Engine Thread

#### `POST /query` (single route, `alternatives=0` hoac vang mat)

**HTTP Handler (async, Tokio)**:
1. Parse JSON body → `QueryRequest`
2. Parse URL params → `FormatParam` (format, colors, alternatives, stretch)
3. Tao `QueryMsg` + oneshot channel
4. `query_tx.send(msg)` → xep hang vao mpsc buffer

**Du lieu handler can tu AppState** (doc, khong cho):

| Truong | Kieu | Muc dich |
|---|---|---|
| `query_tx` | `mpsc::Sender<QueryMsg>` | Gui message den engine thread |

**Engine Thread** (blocking, tuan tu):
1. Nhan `QueryMsg` tu `query_rx`
2. Goi `dispatch_normal()` hoac `dispatch_line_graph()`
3. Xac dinh variant: coords hay node IDs
4. Goi `engine.query_coords()` hoac `engine.query()`

**Du lieu engine doc**:

| Du lieu | Nguon | Thao tac |
|---|---|---|
| `SpatialIndex` (KD-tree) | `engine.spatial` | Tim node/canh gan nhat |
| `ValidationConfig` + `BoundingBox` | `engine.validation_config` | Kiem tra toa do |
| `CustomizedBasic.upward/downward` | `engine.server.customized` | Elimination tree walk |
| `CCH.elimination_tree` | `context.cch` | Duyet cay |
| `CustomizedBasic.up/down_unpacking` | `engine.server.customized` | Mo rong shortcut |
| `NodeOrder` | `context.cch.node_order` | rank() va node() |
| `latitude, longitude` | `context.graph` | Toa do ket qua |

**Thoi gian giu engine thread**: **~1–5 ms** (single CCH query).
Bao gom: snap (~0.1ms) + elimination tree walk + unpack (~1ms) + format response.

---

#### `POST /query` (multi-route, `alternatives > 0`)

**HTTP Handler**: Giong single route — cung `QueryMsg` qua `mpsc`.

**Engine Thread**:
1. Goi `engine.multi_query_coords()` hoac `engine.multi_query()`
2. **Customize them mot lan** voi `geo_distance_weights`:
   `context.customize_geo()` → `CustomizedBasic` tam thoi
3. Tao `MultiRouteServer` voi customized tam thoi
4. Chay `collect_meeting_nodes()` (duyet toan bo elimination tree)
5. `reconstruct_path()` × N ung vien
6. Loc stretch + sharing
7. Drop `CustomizedBasic` tam thoi + `MultiRouteServer`

**Du lieu engine doc them** (ngoai du lieu single route):

| Du lieu | Nguon | Thao tac |
|---|---|---|
| `geo_distance_weights` | `context.geo_distance_weights` | Customize tam thoi |
| `CCH.separator_tree` | `context.cch` | Parallel customization |
| `forward/backward_cch_edge_to_orig_arc` | `context.cch` | prepare_weights |
| `baseline_weights` (gian tiep) | `context.baseline_weights` | Khong (multi-route dung geo) |

**Du lieu engine ghi**:

| Du lieu | Pham vi | Bi drop khi nao |
|---|---|---|
| `CustomizedBasic` (geo) | Cuc bo trong ham | Cuoi `multi_query()` |
| `MultiRouteServer.fw/bw_distances` | Cuc bo | Cuoi `multi_query()` |
| `MultiRouteServer.fw/bw_parents` | Cuc bo | Cuoi `multi_query()` |

**Thoi gian giu engine thread**: **~50–300 ms**
- `customize_geo()`: ~100–250 ms (relax tam giac toan bo)
- `collect_meeting_nodes()`: ~1–3 ms
- `reconstruct_path()` × N: ~1–5 ms
- Loc + format: ~1 ms
- **Day la thao tac ton kem nhat**. Moi query multi-route block tat ca
  query khac trong hang doi.

---

#### `POST /customize` (port 9081)

**HTTP Handler (async, Tokio)** — xu ly HOAN TOAN phia handler, **KHONG gui
den engine thread qua mpsc**:

1. Doc body (raw bytes, tu dong decompress gzip/brotli)
2. Validate kich thuoc: `body.len() == num_edges × 4`?
3. Cast `Bytes` → `Vec<u32>` (bytemuck, copy de dam bao alignment)
4. Validate: moi weight `< INFINITY (4294967294)`?
5. `watch_tx.send(Some(weights))` → signal den engine thread

**Du lieu handler can tu AppState**:

| Truong | Kieu | Muc dich |
|---|---|---|
| `num_edges` | `usize` | Validate body size |
| `watch_tx` | `watch::Sender<Option<Vec<Weight>>>` | Gui weights moi |

**Tra ve ngay** — handler **KHONG cho engine thread xu ly xong**.

**Engine Thread** (xu ly bat dong bo, giua cac query):
1. Poll `watch_rx.has_changed()` moi 50ms
2. Neu co: `engine.update_weights(&weights)`
   - `context.customize_with(&weights)` → CustomizedBasic moi
   - `server.update(new)` → thay the cu
3. Dat `customization_active` flag trong luc xu ly

**Du lieu engine doc khi re-customize**:

| Du lieu | Nguon |
|---|---|
| `CCH.forward/backward_cch_edge_to_orig_arc` | `context.cch` |
| `graph.first_out, graph.head` | `context.graph` |
| Weights moi (truyen vao) | Tu handler qua watch channel |

**Du lieu engine ghi**:

| Du lieu | Tac dong |
|---|---|
| `engine.server` (CustomizedBasic) | **THAY THE** — moi query sau dung weights moi |

**Thoi gian giu engine thread**: **~100–300 ms** (customize).
Trong thoi gian nay, **khong xu ly query nao**. Query xep hang trong
mpsc buffer (toi da 256).

---

#### `GET /info` (port 8081)

**HTTP Handler**: Doc truc tiep tu `AppState`, **KHONG gui den engine thread**.

**Du lieu doc tu AppState**:

| Truong | Kieu | Chi phi |
|---|---|---|
| `is_line_graph` | `bool` | Copy |
| `num_nodes` | `usize` | Copy |
| `num_edges` | `usize` | Copy |
| `customization_active` | `Arc<AtomicBool>` | Atomic load |
| `bbox` | `Option<BboxInfo>` | Clone |

**Thoi gian giu engine thread**: **0 ms** — khong tuong tac voi engine thread.

---

#### `GET /health` (port 8081)

**HTTP Handler**: Doc truc tiep tu `AppState`, **KHONG gui den engine thread**.

**Du lieu doc tu AppState**:

| Truong | Kieu | Chi phi |
|---|---|---|
| `startup_time` | `Instant` | `.elapsed()` |
| `queries_processed` | `Arc<AtomicU64>` | Atomic load |
| `customization_active` | `Arc<AtomicBool>` | Atomic load |

**Thoi gian giu engine thread**: **0 ms**.

---

#### `GET /ready` (port 8081)

**HTTP Handler**: Doc truc tiep tu `AppState`, **KHONG gui den engine thread**.

**Du lieu doc tu AppState**:

| Truong | Kieu | Chi phi |
|---|---|---|
| `engine_alive` | `Arc<AtomicBool>` | Atomic load |

**Thoi gian giu engine thread**: **0 ms**. Tra ve 503 neu engine thread da chet.

---

#### Bang tong hop

| Endpoint | Method | Port | Dung engine thread? | Thoi gian giu engine | Thay doi state? |
|---|---|---|---|---|---|
| `/query` (single) | POST | 8081 | **Co** — qua mpsc | ~1–5 ms | Khong |
| `/query` (multi) | POST | 8081 | **Co** — qua mpsc | **~50–300 ms** | Khong (CustomizedBasic tam thoi) |
| `/customize` | POST | 9081 | **Co** — qua watch | ~100–300 ms (bat dong bo) | **Co** — thay CustomizedBasic |
| `/info` | GET | 8081 | **Khong** | 0 ms | Khong |
| `/health` | GET | 8081 | **Khong** | 0 ms | Khong |
| `/ready` | GET | 8081 | **Khong** | 0 ms | Khong |

**Nhan xet**: Chi 3 endpoint (`/query` single, `/query` multi, `/customize`)
can engine thread. Trong do, `/query` multi va `/customize` la 2 thao tac ton
kem nhat — co the block hang doi 100–300 ms moi lan.

---

## 5. Mo hinh dong thoi va cach ly du lieu

### Kien truc single-engine-thread

```
 ┌────────────────────┐
 │ Tokio async runtime│
 │                    │
 │ HTTP Handler 1 ──┐ │
 │ HTTP Handler 2 ──┤ │    mpsc::channel(256)     ┌──────────────────┐
 │ HTTP Handler 3 ──┤ │ ─────────────────────────→ │ Engine Thread    │
 │ ...              ──┤ │                           │ (1 thread duy    │
 │ HTTP Handler N ──┘ │    watch::channel           │  nhat, xu ly    │
 │                    │ ─────────────────────────→ │  tuan tu)        │
 │ Customize Handler ─┘ │                           └──────────────────┘
 └────────────────────┘
```

- **Moi query** duoc xu ly **tuan tu** trong engine thread
- **Khong co race condition** vi chi 1 thread truy cap `QueryEngine`
- **Customize** duoc kiem tra giua cac query (poll `watch_rx` moi 50ms)
- **Buffer**: 256 query xep hang. Neu day, HTTP handler bi back-pressure

### Cach ly giua /query va /customize

| Hoat dong | Anh huong den query dang cho? | Anh huong den query tiep theo? |
|---|---|---|
| `/query` (single) | Khong — chi doc | Khong |
| `/query` (multi, alternatives) | Khong — CustomizedBasic tam thoi | Khong |
| `/customize` | Khong (query cho den khi customize xong) | **Co** — dung weights moi |
| 2x `/customize` lien tiep | Lan 2 ghi de lan 1 (watch last-write) | Dung weights cua lan 2 |

### Customize khong anh huong baseline

`context.customize_with(&weights)` nhan `&self` (borrow bat bien):
- **Khong thay doi** `baseline_weights`, `geo_distance_weights`, hay CCH topology
- Tao `CustomizedBasic` **moi, doc lap** chi tu weights duoc truyen vao
- `engine.server.update(new)` thay the customized cu, drop cu

→ Goi `/customize` roi goi `/query` voi `alternatives` van dung
`geo_distance_weights` goc de tim duong thay the (vi `multi_query`
goi `customize_geo()` tu context bat bien).

---

## 6. Luong du lieu logic loi — CCH Query

Mo ta luong du lieu ben trong mot CCH query don (single-route), tu luc nhan
node nguon/dich den khi tra ve duong di.

### Cau truc du lieu cua Server

`CchQueryServer<CustomizedBasic>` giu trang thai:

```
CchQueryServer
├── customized: CustomizedBasic<CCH>
│   ├── upward: Vec<Weight>         ← trong so CCH huong len
│   ├── downward: Vec<Weight>       ← trong so CCH huong xuong
│   ├── up_unpacking: Vec<(EdgeId?, EdgeId?)>   ← phan huy shortcut
│   ├── down_unpacking: Vec<(EdgeId?, EdgeId?)>
│   └── cch: &CCH                   ← tham chieu topology
│       ├── elimination_tree: Vec<NodeId?>
│       ├── forward_first_out, forward_head
│       ├── backward_first_out, backward_head
│       └── node_order: NodeOrder
│
├── fw_distances: Vec<Weight>  [n]  ← khoang cach tam thoi forward
├── bw_distances: Vec<Weight>  [n]  ← khoang cach tam thoi backward
├── fw_parents: Vec<(NodeId, EdgeId)>  [n]
├── bw_parents: Vec<(NodeId, EdgeId)>  [n]
└── meeting_node: NodeId
```

### Buoc 1 — Khoi tao

```
query(from, to):
  from_rank = cch.node_order.rank(from)   ← DOC node_order
  to_rank   = cch.node_order.rank(to)

  fw_distances[from_rank] = 0             ← GHI (tat ca khac = INFINITY)
  bw_distances[to_rank]   = 0             ← GHI
  fw_parents[from_rank] = (from_rank, 0)  ← GHI
  bw_parents[to_rank]   = (to_rank, 0)    ← GHI

  Tao 2 EliminationTreeWalk:
    fw_walk: duyet forward_graph (upward weights)
    bw_walk: duyet backward_graph (downward weights)
```

### Buoc 2 — Duyet Elimination Tree (Interleaved)

Hai luot duyet xen ke nhau, xu ly node co rank thap truoc:

```
loop:
  fw_node = fw_walk.peek()
  bw_node = bw_walk.peek()

  Truong hop 1: fw_node < bw_node
    fw_walk.next()
    → Voi moi canh (fw_node → v, w) trong forward_graph:  ← DOC upward[]
        neu fw_distances[fw_node] + w < fw_distances[v]:
          fw_distances[v] = fw_distances[fw_node] + w      ← GHI
          fw_parents[v] = (fw_node, edge_id)                ← GHI
    → fw_distances[fw_node] = INFINITY                      ← GHI (reset)
    → Di len: fw_walk.next = elimination_tree[fw_node]      ← DOC

  Truong hop 2: bw_node < fw_node
    (tuong tu, dung backward_graph va bw_distances)

  Truong hop 3: fw_node == bw_node (MEETING NODE)
    dist = fw_distances[node] + bw_distances[node]
    neu dist < tentative_distance:
      meeting_node = node                                    ← GHI
      tentative_distance = dist
    Reset ca hai distances[node] = INFINITY
    Di len tren ca hai walk

  Truong hop 4: Ca hai None → ket thuc
```

**Dac diem**: Moi node chi duoc settle **mot lan** tren moi walk. Sau khi
settle, khoang cach reset ve INFINITY de tranh nhiem ban cho query tiep theo.
Walk luon di len den goc cua elimination tree.

### Buoc 3 — Tai tao duong di

```
1. Lien ket parent pointers thanh chuoi lien tuc:
   node = meeting_node
   while node != from_rank:
     (parent, edge) = fw_parents[node]    ← DOC
     bw_parents[parent] = (node, edge)    ← GHI (ghi de)
     node = parent

   → bw_parents bay gio luu duong: from_rank → meeting → to_rank

2. Mo rong shortcut (Unpack):
   unpack_path(target, source, customized, &mut bw_parents)

   Voi moi canh tren duong:
     (pred, edge) = bw_parents[current]    ← DOC
     unpacked = customized.unpack_outgoing(edge)  ← DOC up_unpacking[]
                hoac unpack_incoming(edge)        ← DOC down_unpacking[]

     Neu la shortcut (co middle node):
       bw_parents[current] = (middle, down_edge)   ← GHI
       bw_parents[middle] = (pred, up_edge)         ← GHI
       (khong tien len, lap lai tu current)

     Neu la canh goc:
       current = pred  (tien len)

3. Xay dung path vector:
   Di theo bw_parents tu from_rank den to_rank
   Chuyen rank → node ID goc: node_order.node(rank)   ← DOC
```

### Ma tran Doc/Ghi cua mot query

| Cau truc | DOC khi | GHI khi |
|---|---|---|
| `upward[]` | Relax canh forward | Khong bao gio |
| `downward[]` | Relax canh backward | Khong bao gio |
| `fw_distances[]` | Relax + meeting check | Relax + reset |
| `bw_distances[]` | Relax + meeting check | Relax + reset |
| `fw_parents[]` | Tai tao duong (forward half) | Relax |
| `bw_parents[]` | Tai tao duong (di theo) | Relax + lien ket + unpack |
| `elimination_tree[]` | Di chuyen len cay | Khong bao gio |
| `up_unpacking[]` | Unpack shortcut | Khong bao gio |
| `down_unpacking[]` | Unpack shortcut | Khong bao gio |
| `node_order` | rank() va node() | Khong bao gio |

---

## 7. Luong du lieu logic loi — Customization

Qua trinh `customize()` chuyen trong so do thi goc thanh trong so CCH
da relax tam giac, san sang cho query.

### Dau vao

```
customize(cch: &CCH, metric: &FirstOutGraph)
  cch: topology CCH (bat bien)
  metric: do thi CSR voi trong so moi (travel_time hoac penalty weights)
```

### Buoc 1 — Khoi tao

```
Cap phat:
  upward_weights:   Vec<Weight>  [m canh CCH]  = INFINITY
  downward_weights: Vec<Weight>  [m canh CCH]  = INFINITY
  up_unpacking:     Vec<(EdgeId?, EdgeId?)>  [m]
  down_unpacking:   Vec<(EdgeId?, EdgeId?)>  [m]
```

### Buoc 2 — Sao chep trong so goc (prepare_weights)

```
Voi moi canh CCH (u → v):
  forward_cch_edge_to_orig_arc[edge]  ← DOC: danh sach canh goc tuong ung
  upward_weights[edge] = min(tat ca orig_arc weights)  ← GHI

Tuong tu cho backward:
  backward_cch_edge_to_orig_arc[edge]  ← DOC
  downward_weights[edge] = min(...)     ← GHI
```

### Buoc 3 — Relax tam giac (customize_basic)

Xu ly node theo thu tu rank giam dan (node cao nhat truoc):

```
Voi moi node u (tu rank cao den thap):

  1. Nap workspace: ghi upward_weights cua cac canh di tu u vao mang tam
     ← DOC upward_weights cho cac canh (u → w)

  2. Voi moi canh xuong (v → u) trong do thi CCH:       ← DOC downward
     Voi moi canh len tu v (v → w):                      ← DOC upward
       chi_phi_qua_v = downward_weights[v→u] + upward_weights[v→w]

       Neu chi_phi_qua_v < workspace[w]:                 ← DOC workspace
         workspace[w] = chi_phi_qua_v                    ← GHI workspace
         up_unpacking[u→w] = (v→u, v→w)                  ← GHI unpacking

  3. Ghi lai workspace vao upward_weights[u→*]           ← GHI upward
```

```
Minh hoa tam giac:
        w (rank cao)
       / \
  (up)/   \(up)         Relax: weight(u→w) = min(weight(u→w),
     /     \                                     weight(u→v) + weight(v→w))
    u ───── v
     (down)
```

### Ket qua

```
CustomizedBasic<CCH>
├── upward: Vec<Weight>       ← trong so da relax (forward query)
├── downward: Vec<Weight>     ← trong so da relax (backward query)
├── up_unpacking              ← thong tin phan huy shortcut
├── down_unpacking
└── cch: &CCH                 ← tham chieu topology (bat bien)
```

### Luong cap nhat weights tai runtime

```
POST /customize (weights moi)
  │
  ▼
handle_customize() → validate → watch_tx.send(weights)
  │
  ▼ (engine thread)
engine.update_weights(&weights)
  │
  ├── context.customize_with(&weights)
  │     └── customize(&cch, &FirstOutGraph(first_out, head, weights))
  │           ├── prepare_weights      ~20ms
  │           └── customize_basic      ~80-250ms
  │           └── → CustomizedBasic MOI
  │
  └── server.update(new_customized)
        └── Drop customized cu, thay bang moi
            → Moi query sau dung weights moi
```

**Quan trong**: `context.baseline_weights` va `context.cch` **khong bao gio
bi thay doi**. Chi `CustomizedBasic` trong `server` duoc thay the.

---

## 8. Luong du lieu logic loi — Multi-Route

Mo ta luong du lieu ben trong thuat toan tim K tuyen thay the (via-node).

### Cau truc MultiRouteServer

```
MultiRouteServer<'a, C: Customized>
├── customized: &'a C           ← tham chieu den CustomizedBasic (chi doc)
├── fw_distances: Vec<Weight>   ← dung lai giua cac pha
├── bw_distances: Vec<Weight>
├── fw_parents: Vec<(NodeId, EdgeId)>
└── bw_parents: Vec<(NodeId, EdgeId)>
```

### Pha 1 — Thu thap meeting nodes

```
multi_query(from, to, max_alternatives, stretch, geo_len):

  collect_meeting_nodes(from, to):
    fw_walk, bw_walk: duyet elimination tree NHU QUERY CHUAN
                      NHUNG:
      - KHONG goi skip_next() → duyet TAT CA node den goc
      - KHONG reset distances → giu lai de tai tao duong
      - Thu thap MOI node chung noi fw_dist < INF va bw_dist < INF

    Ket qua: Vec<(meeting_node, fw_dist + bw_dist)>
    Sap xep theo khoang cach tang dan
    De-duplicate theo node (giu khoang cach nho nhat)
```

```
Khac biet voi query chuan:

  Query chuan              Multi-route
  ────────────             ────────────
  Mot meeting node         TAT CA meeting nodes
  skip_next() toi uu       next() day du
  Reset sau settle         Khong reset (giu distances)
  Tim 1 duong ngan nhat    Tim K ung vien
```

### Pha 2 — Duong chinh (ung vien tot nhat)

```
main_candidate = candidates[0]
main_path = reconstruct_path(from, meeting_node_0, to)
  │
  ├── Trace fw_parents: meeting → from (dao nguoc)  ← DOC fw_parents
  ├── Trace bw_parents: meeting → to                ← DOC bw_parents
  ├── Ghep 2 nua thanh duong CCH rank
  ├── Voi moi canh: unpack_edge_recursive()         ← DOC up/down_unpacking
  │     └── De quy: shortcut → (canh_xuong, canh_len, middle_node)
  │         Tiep tuc unpack cho den canh goc
  └── Chuyen rank → node ID goc                     ← DOC node_order

main_geo = geo_len(&main_path)           ← GOI callback tinh chieu dai dia ly
geo_stretch_limit = main_geo × stretch_factor
main_edges = HashSet cac canh cua duong chinh
```

### Pha 3 — Loc ung vien

```
Voi moi candidate con lai (i = 1, 2, ...):
  path = reconstruct_path(from, meeting_node_i, to)
  candidate_geo = geo_len(&path)

  Kiem tra 1: Bounded stretch (dia ly)
    candidate_geo > geo_stretch_limit?  → BO QUA

  Kiem tra 2: Limited sharing (canh)
    candidate_edges = cac canh cua path
    shared = candidate_edges ∩ (main_edges ∪ accepted_edges)
    sharing_ratio = |shared| / |candidate_edges ∪ main_edges|
    sharing_ratio > 0.80?  → BO QUA (qua giong)

  Chap nhan → them vao ket qua
  Dung khi: accepted.len() >= max_alternatives
```

### Luong du lieu tong hop

```
CchContext (bat bien)
  │
  ├── customize_geo()
  │     └── CustomizedBasic MOI (geo_distance weights)
  │           └── Truyen vao MultiRouteServer
  │
  └── baseline_weights, graph      ← Cho caller tinh khoang cach
                                      va toa do

MultiRouteServer
  │
  ├── collect_meeting_nodes()
  │     ├── DOC: forward_graph, backward_graph (CustomizedBasic)
  │     ├── DOC: elimination_tree
  │     └── GHI: fw/bw_distances, fw/bw_parents
  │
  ├── reconstruct_path() × N lan
  │     ├── DOC: fw/bw_parents (di theo chuoi)
  │     ├── DOC: up/down_unpacking (mo rong shortcut)
  │     └── DOC: node_order (chuyen rank → ID)
  │
  └── Output: Vec<AlternativeRoute>
        ├── distance: Weight (ms)
        ├── geo_distance_m: f64
        └── path: Vec<NodeId>
```

---

## 9. Luong du lieu logic loi — Line Graph Query

Khi server chay che do `--line-graph`, query hoat dong tren do thi re
(turn-expanded graph).

### Cau truc LineGraphCchContext

```
LineGraphCchContext
├── graph: GraphData              ← Line graph CSR
│   ├── first_out, head           (node LG = canh goc, canh LG = luot re)
│   └── travel_time               (gom ca turn penalty)
├── directed_cch: DirectedCCH     ← CCH co huong (pruned)
│
├── original_tail: Vec<NodeId>    ← TAI TAO tu CSR goc + via_way_split_map
├── original_head: Vec<NodeId>    ← Tu do thi goc
├── original_latitude: Vec<f32>   ← Toa do NODE goc (khong phai LG)
├── original_longitude: Vec<f32>
├── original_travel_time: Vec<Weight>  ← Trong so canh goc (ms)
├── original_geo_distance: Vec<Weight>
├── original_first_out: Vec<u32>  ← CSR cua do thi goc
│
├── via_way_split_map: Vec<u32>   ← Anh xa split node → LG node goc
├── geo_distance_weights: Vec<Weight>  ← Cho multi-route
└── baseline_weights: Vec<Weight>
```

### Single query — Node-based

```
query(source_edge, target_edge):    ← Dau vao la EDGE ID goc (= LG node ID)

  1. CCH query tren DirectedCCH:
     server.query(Query { from: source_edge, to: target_edge })
     → cch_distance                                     ← DOC directed_cch

  2. Source-edge correction:
     distance_ms = cch_distance + original_travel_time[source_edge]  ← DOC
     (Ly do: trong so LG = travel_time[canh_DICH] + turn_cost,
      nen canh nguon bi loai khoi tong. Them lai o day.)

  3. Chuyen LG path → intersection path:
     Voi moi lg_node trong path:
       intersection_node = original_tail[lg_node]       ← DOC original_tail
     Them: original_head[lg_path.last()]                ← DOC original_head
     → Vec<NodeId> gom cac node NGA TU goc

  4. Tinh turn annotations:
     compute_turns(lg_path, original_tail, original_head,
                   original_first_out, lat, lng)         ← DOC geometry

  5. Tinh toa do va khoang cach dia ly
```

### Single query — Coordinate-based

```
query_coords(from: (lat, lng), to: (lat, lng)):

  1. Snap toa do vao DO THI GOC (khong phai line graph):
     snap tren original_spatial (KD-tree node GOC)       ← DOC
     → src_snaps: Vec<SnapResult>
       moi snap chua: edge_id (= LG node ID), tail, head, t, distance

  2. Thu tat ca cap snap (toi da 5 × 5 = 25 query):
     Voi moi (src_snap, dst_snap):
       query_trimmed(src_snap.edge_id, dst_snap.edge_id)
       └── query() nhung cat bo LG edge dau va cuoi
           (loai bo phantom turns tu canh snap)

  3. Giu ket qua ngan nhat theo distance_ms

  4. Patch origin/destination metadata vao ket qua
```

### Multi-route — Line graph

```
multi_query(source_edge, target_edge, max_alternatives, stretch):

  1. Customize voi geo_distance_weights:
     geo_customized = context.customize_with(geo_distance_weights)
     → CustomizedBasic MOI (tam thoi)                    ← CAP PHAT

  2. Tao MultiRouteServer(&geo_customized)

  3. Tao closure tinh geo length:
     lg_path_geo_len = |lg_path| {
       Chuyen LG nodes → original_tail coords            ← DOC
       Them original_head[last] coord
       route_distance_m (Haversine sum)
     }

  4. Goi multi.multi_query() nhu do thi thuong
     (xem Muc 8 — cung 3 pha: collect, reconstruct, filter)

  5. Hau xu ly moi ket qua:
     ├── Source-edge correction: + original_travel_time[source_edge]
     ├── Chuyen LG path → intersection nodes
     ├── Tinh turn annotations
     ├── Loc theo MAX_GEO_RATIO (2.0x)
     └── Xay dung QueryAnswer
```

### So sanh doi chieu Normal vs Line Graph

| Khia canh | Normal (CCH) | Line Graph (DirectedCCH) |
|---|---|---|
| Node bieu dien | Nga tu duong | **Canh duong** (luot re) |
| Canh bieu dien | Doan duong | **Luot re** (2 canh lien tiep) |
| CCH kieu | Vo huong (CCH) | **Co huong** (DirectedCCH, pruned) |
| Input query | Node ID | **Edge ID** (= LG node ID) |
| Snap | Node gan nhat | **Canh gan nhat** (trong do thi goc) |
| Source correction | Khong can | **+ travel_time[source_edge]** |
| Turn info | Khong co | Co (compute_turns) |
| Path output | Node IDs | LG nodes → original_tail → **Node IDs** |

---

## 10. Luong du lieu logic loi — Spatial Snap & Validation

### Cau truc SpatialIndex

```
SpatialIndex
├── tree: ImmutableKdTree<f32, 2>   ← KD-tree 2D (lat, lng)
├── first_out: Vec<EdgeId>          ← CSR offset (doc tu do thi)
├── head: Vec<NodeId>               ← Canh (doc tu do thi)
├── lat, lng: Vec<f32>              ← Toa do node
└── bbox: BoundingBox               ← Bao dong (min/max lat/lng)
```

### Luong snap toa do → node/edge

```
snap_candidates(lat, lng, max_results):

  1. KD-tree: tim K=10 node gan nhat (Euclidean tren lat/lng)
     tree.nearest(&[lat, lng], 10)                       ← DOC tree

  2. Voi moi node tim duoc:
     Voi moi canh ke (tu CSR):                           ← DOC first_out, head
       tail_node = node
       head_node = head[edge]
       Tinh khoang cach vuong goc Haversine:
         (snap_distance, t) = haversine_perpendicular_distance_with_t(
           query_lat, query_lng,
           lat[tail], lng[tail],                          ← DOC lat, lng
           lat[head], lng[head]
         )
       Luu vao HashMap<EdgeId, SnapResult>  (de-dup, giu tot nhat)

  3. Sap xep theo snap_distance_m, cat ve max_results

Ket qua: Vec<SnapResult>
  ├── edge_id: EdgeId
  ├── tail, head: NodeId
  ├── t: f64 ∈ [0.0, 1.0]         ← vi tri tren canh
  ├── snap_distance_m: f64
  └── nearest_node() = tail neu t < 0.5, khong thi head
```

### Luong validation (truoc snap)

```
validated_snap_candidates("origin", lat, lng, config, max):

  1. is_finite(lat) va is_finite(lng)?
     Khong → Err(CoordRejection::NonFinite)

  2. -90 <= lat <= 90 va -180 <= lng <= 180?
     Khong → Err(CoordRejection::InvalidRange)

  3. Trong bbox + padding (config.bbox_padding_m = 1500m)?
     padding_lat = 1500 / 111320 ≈ 0.0135°
     padding_lng = 1500 / (111320 × cos(center_lat))
     Khong → Err(CoordRejection::OutOfBounds)

  4. snap_candidates() → snaps
     snaps[0].snap_distance_m > config.max_snap_distance_m (2000m)?
     Khong snap nao → Err(CoordRejection::SnapTooFar)

  5. Ok(snaps)
```

---

## 11. Luong du lieu logic loi — Turn Annotation

Tinh goc re va phan loai huong re tai moi nga tu tren duong di
(chi kha dung trong che do line-graph).

### Luong tinh toan

```
compute_turns(lg_path, original_tail, original_head,
              original_first_out, lat, lng):

  Voi moi cap lien tiep (lg_path[i], lg_path[i+1]):

    edge_a = lg_path[i]
    edge_b = lg_path[i+1]

    tail_a = original_tail[edge_a]         ← DOC
    head_a = original_head[edge_a]         ← DOC (= nga tu re)
    head_b = original_head[edge_b]         ← DOC

    assert: head_a == original_tail[edge_b]  (tinh nhat quan line graph)

    1. Phep chieu equirectangular tai head_a:
       cos_lat = cos(lat[head_a])
       ax = (lng[head_a] - lng[tail_a]) × cos_lat      ← DOC lat, lng
       ay = lat[head_a] - lat[tail_a]
       bx = (lng[head_b] - lng[head_a]) × cos_lat
       by = lat[head_b] - lat[head_a]

    2. Tinh goc:
       dot   = ax·bx + ay·by
       cross = ax·by - ay·bx
       angle = atan2(cross, dot) → do

    3. Phan loai:
       |angle| < 25°              → Straight (di thang)
       25° <= |angle| < 155°:
         cross > 0                → Left (re trai)
         cross < 0                → Right (re phai)
       |angle| >= 155°            → UTurn (quay dau)

    4. Bac cua nga tu:
       degree = first_out[head_a + 1] - first_out[head_a]  ← DOC

    → TurnAnnotation {
        direction, angle_degrees, coordinate_index: i+1,
        distance_to_next_m, intersection_degree: degree
      }

Ket qua: Vec<TurnAnnotation>
```

### Luu y do chinh xac

- Toa do f32 co sai so ~12m o kinh do 105° (Ha Noi)
- Code promote f32 → f64 truoc khi tru de giam sai so
- Voi doan duong ngan (< 20m), goc tinh duoc co the sai vai do

---

## 12. Dinh dang du lieu RoutingKit

Tat ca file du lieu la **raw binary, khong co header**, little-endian.
Kieu du lieu xac dinh boi ten file va convention:

| Kieu | Kich thuoc | Dung cho |
|---|---|---|
| u32 | 4 bytes | first_out, head, travel_time, geo_distance, way, cch_perm |
| f32 | 4 bytes | latitude, longitude |

Doc trong Rust: `Vec::<u32>::load_from(path)` hoac `Vec::<f32>::load_from(path)`.

Quy uoc thoi gian:
- `travel_time` luon la **mili giay** (u32)
- `tt_units_per_s = 1000` (metadata)
- `INFINITY = u32::MAX - 1 = 4294967294` → gia tri dac biet "khong co duong"

---

## 13. Cau truc thu muc du lieu

```
Maps/data/hanoi_motorcycle/
│
├── graph/                              ← Phase 1: Do thi goc
│   ├── first_out                       (u32, n+1 phan tu)
│   ├── head                            (u32, m phan tu)
│   ├── travel_time                     (u32, m phan tu, ms)
│   ├── geo_distance                    (u32, m phan tu, met)
│   ├── latitude                        (f32, n phan tu)
│   ├── longitude                       (f32, n phan tu)
│   ├── way                             (u32, m phan tu)
│   ├── forbidden_turn_from_arc         (u32, sap xep)
│   ├── forbidden_turn_to_arc           (u32, sap xep)
│   ├── via_way_chain_arcs
│   ├── via_way_chain_mandatory
│   ├── via_way_chain_offsets
│   └── perms/
│       ├── cch_perm                    ← Phase 4a: Node ordering
│       ├── cch_perm_cuts               ← Phase 4b: Arc cuts
│       └── cch_perm_cuts_reorder       ← Phase 4c
│
├── line_graph/                         ← Phase 3: Do thi re
│   ├── first_out                       (u32, node i = edge goc i)
│   ├── head                            (u32)
│   ├── travel_time                     (u32, ms)
│   ├── latitude                        (f32, tu tail node goc)
│   ├── longitude                       (f32, tu tail node goc)
│   ├── via_way_split_map               (u32, BAT BUOC)
│   └── perms/
│       └── cch_perm                    ← Phase 5: Ordering line graph
│
└── conditional_turns/                  ← Phase 2: Re co dieu kien
    ├── conditional_turn_from_arc
    ├── conditional_turn_to_arc
    └── conditional_turn_time_windows
```

---

## Phu luc — Huong dan lenh

### Chay offline pipeline

```bash
# Tao toan bo du lieu tu OSM PBF
scripts/pipeline hanoi.osm.pbf motorcycle

# Chi tao lai line graph (sau khi sua turn penalty)
CCH-Hanoi/target/release/generate_line_graph \
    Maps/data/hanoi_motorcycle/graph \
    Maps/data/hanoi_motorcycle/line_graph

# Chi tao lai CCH ordering cho line graph
rust_road_router/flow_cutter_cch_order.sh \
    Maps/data/hanoi_motorcycle/line_graph
```

### Khoi dong server

```bash
# Line-graph mode (ho tro turn restriction)
scripts/start_server.sh motorcycle
# → query port 8081, customize port 9081

# Normal mode (khong co turn info)
hanoi_server --graph-dir Maps/data/hanoi_motorcycle/graph \
    --query-port 8081 --customize-port 9081
```

### Query

```bash
# Single route
curl -X POST localhost:8081/query?colors \
  -H 'Content-Type: application/json' \
  -d '{"from_lat":21.028,"from_lng":105.834,"to_lat":21.003,"to_lng":105.820}'

# Multi route (3 tuyen thay the, stretch 1.3x)
curl -X POST 'localhost:8081/query?alternatives=3&stretch=1.3&colors' \
  -H 'Content-Type: application/json' \
  -d '{"from_lat":21.028,"from_lng":105.834,"to_lat":21.003,"to_lng":105.820}'
```

### Customize (cap nhat trong so)

```bash
# Gui vector trong so moi (binary, num_edges × 4 bytes)
curl -X POST localhost:9081/customize \
  --data-binary @new_weights.bin

# Gui voi gzip compression
curl -X POST localhost:9081/customize \
  -H 'Content-Encoding: gzip' \
  --data-binary @new_weights.bin.gz
```
