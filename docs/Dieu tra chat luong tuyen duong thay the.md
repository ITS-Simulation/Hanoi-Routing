# Dieu tra chat luong tuyen duong thay the

> **Pham vi**: Phan tich nguyen nhan goc re cua hanh vi re bat thuong trong
> thuat toan tao K tuyen duong thay the — quay dau (U-turn), keo dai khong can
> thiet, bo qua cac tuyen tot hon.
>
> **Cac module lien quan**:
>
> - `CCH-Hanoi/crates/hanoi-core/src/multi_route.rs` — thuat toan K-alternatives
>   qua via-node
> - `CCH-Hanoi/crates/hanoi-core/src/cch.rs` — wrapper multi-query cho do thi
>   thuong
> - `CCH-Hanoi/crates/hanoi-core/src/line_graph.rs` — multi-query cho line graph
> - `CCH-Hanoi/crates/hanoi-tools/src/bin/generate_line_graph.rs` — xay dung
>   line graph & gan chi phi re
> - `rust_road_router/engine/src/algo/customizable_contraction_hierarchy/query.rs`
>   — CCH query goc (tham chieu)
> - `rust_road_router/engine/src/datastr/graph.rs:181` — ham `line_graph()`
>   (cong thuc trong so)
>
> **Tai lieu tham chieu chinh**: `docs/walkthrough/separator-based-alternative-paths-cch.md`
> — Bai bao SeArCCH (Bacherle, Blasius, Zundorf — ATMOS 2025)

---

## Muc luc

1. [Boi canh](#1-boi-canh)
2. [Cach thuat toan hien tai hoat dong](#2-cach-thuat-toan-hien-tai-hoat-dong)
3. [Van de 1 — Via-Node thieu kiem tra tinh hop le](#3-van-de-1--via-node-thieu-kiem-tra-tinh-hop-le)
4. [Van de 2 — Chi phi re bang 0 trong Line Graph](#4-van-de-2--chi-phi-re-bang-0-trong-line-graph)
5. [Van de 3 — Xep hang thuan theo thoi gian di chuyen](#5-van-de-3--xep-hang-thuan-theo-thoi-gian-di-chuyen)
6. [Ba van de ket hop nhu the nao](#6-ba-van-de-ket-hop-nhu-the-nao)
7. [De xuat sua chua](#7-de-xuat-sua-chua)
8. [Ma tran uu tien](#8-ma-tran-uu-tien)

---

## 1. Boi canh

Tinh nang tuyen duong thay the su dung phuong phap **via-node** tren nen CCH.
Trong CCH query chuan, **ca hai** luot duyet elimination tree (forward va
backward) deu di **len den goc (root)** — khong co diem dung som. Moi to tien
chung (common ancestor) cua nguon va dich trong elimination tree la mot meeting
node tiem nang. Query chuan tim mot meeting node tot nhat duy nhat (nho nhat
`d(s,v) + d(v,t)`). Phan mo rong multi-route thu thap *tat ca* cac to tien
chung trong pham vi stretch factor va dung moi meeting node lam via-vertex de
tai tao mot tuyen ung vien thay the.

Trieu chung quan sat duoc:

- Tuyen duong co U-turn khong can thiet (di thang roi quay dau roi re sang
  duong khac).
- Duong vong dai bat thuong khi co tuyen ngan hon, tu nhien hon.
- Tuyen thay the bam sat tuyen toi uu nhung co nhung lech huong ky la tai
  cac nga tu.

Hai gia thuyet ban dau:

1. Qua trinh unpack shortcut CCH gay ra viec mot so doan duong duoc mo rong
   khong toi uu.
2. Trong so mac dinh (travel time) gay ra hanh vi re bat thuong.

**Ket luan**: Gia thuyet 2 duoc xac nhan truc tiep (chi phi re bang 0). Gia
thuyet 1 duoc xac nhan mot phan — khong phai do loi unpack, ma do van de co ban
hon: duong di qua via-node khong toi uu duoc unpack thanh duong **dung nhung
khong hop ly**, vi khong co kiem tra tinh hop le (bounded stretch, local
optimality) tren cac doan con. Mot yeu to thu ba — xep hang thuan theo thoi
gian di chuyen khong co nhan thuc dia ly — cung duoc xac dinh.

---

## 2. Cach thuat toan hien tai hoat dong

### Co che duyet Elimination Tree

Duyet elimination tree trong CCH **khong** giong Dijkstra hai chieu. Moi luot
duyet di theo mot duong don, tat dinh, tu nguon (hoac dich) di len qua
elimination tree den goc. Khong co frontier, khong co priority queue, khong co
tieu chi dung. Ca hai luot duyet **luon den goc**.

Tai moi node tren duong di len, luot duyet relax cac canh trong do thi CCH
huong len — cac canh nay den cac node co rank cao hon (cac to tien khac hoac
node tren cac nhanh ke). Qua trinh nay lan truyen khoang cach duong ngan nhat
len tren de moi to tien chung `v` co gia tri `d(s,v)` va `d(v,t)` hop le sau
khi ca hai luot duyet hoan tat.

Cac lenh `reset_distance()` trong query chuan (`query.rs:77, 82, 102–103`) la
**don dep de tai su dung** mang khoang cach cho cac query tiep theo — khong
anh huong den tinh dung cua luot duyet. Multi-route walk dung khi bo qua cac
reset nay vi can giu lai khoang cach de tai tao duong.

### Pha 1 — Thu thap Meeting Nodes

`multi_route.rs:collect_meeting_nodes()` (dong 148–222) chay ca hai luot duyet
elimination tree den hoan tat va ghi lai moi to tien chung noi ma ca
`fw_dist < INFINITY` va `bw_dist < INFINITY`.

| Khia canh | Query chuan (`query.rs`) | Multi-route (`multi_route.rs`) |
|-----------|--------------------------|-------------------------------|
| Reset khoang cach sau settle | **Co** — don dep cho query tiep | **Khong** — giu lai de tai tao duong |
| Cat tia tai meeting nodes | **Co** — `skip_next()` toi uu hoa | **Khong** — luon `next()` de lan truyen tat ca khoang cach |
| Theo doi meeting node | Mot node tot nhat duy nhat | Tat ca to tien chung |
| Duyet den goc? | **Co** | **Co** |

**Con tro parent la dung**: Sau khi luot duyet hoan tat, `fw_parents[v]` ghi
lai duong di len ngan nhat tu nguon den `v`. Truy vet `fw_parents` tu bat ky
meeting node nao ve nguon deu cho duong `source → v` ngan nhat thuc su trong
do thi CCH huong len. Tuong tu cho `bw_parents`.

### Pha 2 — Tai tao & Mo rong (Unpack)

Voi moi meeting node, `reconstruct_path()` (dong 230–293) truy vet `fw_parents`
nguoc tu meeting node ve nguon, va `bw_parents` xuoi tu meeting node den dich.
Moi canh shortcut CCH duoc de quy mo rong qua `unpack_edge_recursive()` (dong
305–324).

Viec unpack la dung — no phan chieu logic `query.rs::unpack_path()` goc.

### Pha 3 — Loc

- **Loc stretch**: loai bo ung vien > `1.3x` khoang cach toi uu.
- **Loc da dang**: he so Jaccard trung lap tap canh > `0.85` → bi loai vi
  qua giong nhau.
- **Loc dia ly** (do caller thuc hien): loai bo neu khoang cach dia ly > `2.0x`
  khoang cach dia ly cua tuyen ngan nhat.

---

## 3. Van de 1 — Via-Node thieu kiem tra tinh hop le

**Muc nghiem trong: Cao** — Day la nguyen nhan cau truc chinh gay ra cac
duong di bat thuong.

### Co che

Cach tiep can hien tai coi moi to tien chung trong elimination tree la ung vien
via-vertex va tai tao duong di qua chung. Con tro parent va unpack deu **dung**
— moi duong di thuc su la duong `s → v → t` ngan nhat qua meeting node `v` do.
Tuy nhien, **la duong ngan nhat qua `v` khong co nghia la mot tuyen thay the
tot**.

Van de la khi cac duong nay duoc unpack ra do thi goc. Mot via-vertex `v` o
vi tri cao trong elimination tree co the khien duong di:

1. **Chia se phan lon chieu dai voi duong toi uu**, chi lech mot chut gan `v`
   roi nhap lai — tao ra tuyen gan nhu giong het voi mot duong vong ky la nho
   tai mot nga tu.

2. **Chua cac doan con khong toi uu cuc bo**: doan `s → v` la duong ngan nhat
   tu `s` den `v`, nhung mot doan con `a → b` ben trong no co the dai hon nhieu
   so voi `d(a,b)`. Dieu nay xay ra vi duong di duoc toi uu dau-cuoi de den
   `v`, khong phai cho moi doan trung gian.

3. **Chua U-turn hoac di nguoc**: khi unpack, duong di qua vertex separator
   rank cao co the di qua nga tu, vuot qua, quay dau, va quay lai — vi day
   thuc su la duong ngan nhat de den `v` cu the do, du nguoi lai xe thuc te
   se khong bao gio di nhu vay.

Tai lieu (Bacherle, Blasius, Zundorf — ATMOS 2025, tai
`docs/walkthrough/separator-based-alternative-paths-cch.md`) xac dinh ba tieu
chi hop le ma implementation hien tai **khong** kiem tra:

| Tieu chi | Mo ta | Trang thai hien tai |
|----------|-------|---------------------|
| **Bounded stretch** | Moi doan lech `a → v → b` phai thoa man `c(a→v→b) <= (1+e) * d(a,b)` | **Khong kiem tra** |
| **Limited sharing** | Trung lap voi duong ngan nhat va tuyen khac phai <= g * d(s,t) | **Xap xi bang Jaccard (dem canh), khong theo chi phi** |
| **Local optimality** | Moi doan con <= a*d(s,t) phai la duong ngan nhat (T-test) | **Khong kiem tra** |

Khong co bounded stretch, duong di lech lon gan `v` nhung van du ngan toan cuc
se vuot qua bo loc stretch. Khong co local optimality, cac doan con co U-turn
duoc chap nhan.

---

## 4. Van de 2 — Chi phi re bang 0 trong Line Graph

**Muc nghiem trong: Cao** — Truc tiep cho phep lam dung U-turn.

### Ma nguon

Trong `generate_line_graph.rs:217–232`:

```rust
let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
    // ... kiem tra re cam (return None neu cam) ...

    if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
        return Some(0); // Phat U-turn: 0 ms (MIEN PHI)
    }
    Some(0) // Tat ca cac re khac: cung 0 ms (MIEN PHI)
});
```

### Cach trong so Line Graph hoat dong

Ham `line_graph()` trong `rust_road_router/engine/src/datastr/graph.rs:194`
tinh trong so moi canh line-graph:

```rust
weight.push(next_link.weight + turn_cost);
//          ^^^^^^^^^^^^^^^^   ^^^^^^^^^
//          travel_time cua    chi phi re tu callback
//          canh DICH          (luon = 0 hien tai)
```

Vay moi trong so canh line-graph = `travel_time[canh_dich] + 0`. Chi phi re
la **dong nhat bang khong** cho moi luot re khong bi cam, ke ca U-turn.

### Tai sao day la van de

- **U-turn mien phi**: Thuat toan khong thay khac biet giua di thang qua nga
  tu va quay dau.

- **Khong phan biet theo goc re**: Moi luot re (re nhe trai, re gap phai,
  U-turn) co chi phi bang khong giong het nhau.

- **Khuech dai Van de 1**: Khi duong via-node unpack ra tuyen chua U-turn,
  chi phi re bang 0 khien duong di do khong bi phat so voi cac tuyen sach
  hon, nen no vuot qua bo loc stretch va da dang.

---

## 5. Van de 3 — Xep hang thuan theo thoi gian di chuyen

**Muc nghiem trong: Trung binh** — Gop phan chon tuyen thay the kem chat
luong.

### Ma nguon

Trong `multi_route.rs:216`:

```rust
meeting_candidates.sort_unstable_by_key(|&(_, dist)| dist);
```

Ung vien duoc xep hang thuan theo khoang cach thoi gian di chuyen CCH. Bo loc
khoang cach dia ly chi chay *sau khi* tai tao day du duong di, o caller:

```rust
// cch.rs:270–276
if distance_m > base * MAX_GEO_RATIO {
    continue;  // loai bo duong vong
}
```

### Tai sao day la van de

Hai ung vien co thoi gian di chuyen gan giong nhau nhung hinh dang dia ly
khac biet lon duoc coi la tot nhu nhau. Bo loc `MAX_GEO_RATIO = 2.0` rat
long — tuyen gap doi khoang cach dia ly van duoc chap nhan.

---

## 6. Ba van de ket hop nhu the nao

```
Van de 1: Khong co kiem tra tinh hop le tren duong via-node
    → Cac doan con co duong vong va U-turn ton tai neu toan cuc du ngan
    → Tuyen thay the giong tuyen toi uu voi cac lech cuc bo ky la

Van de 2: Chi phi re bang 0
    → U-turn va re gap khong co penalty
    → Duong via-node co U-turn van canh tranh ve chi phi

Van de 3: Xep hang thuan theo thoi gian di chuyen
    → Khong kiem tra hop ly dia ly khi chon ung vien
    → Tuyen bat thuong vuot qua bo loc stretch
    → Bo loc dia ly (2x) ap dung qua muon va qua long
```

Vi du tinh huong khop voi anh chup quan sat duoc:

1. Tuyen toi uu di thang doc Duong Lang.
2. Multi-route thu thap meeting node `M2` (separator cao hon trong cay) voi
   khoang cach via-path dai hon mot chut.
3. Duong `s → M2 → t` ngan nhat, khi unpack, di xuong phia nam Duong Lang,
   vong qua vung separator gan Cau Hua Muc, roi quay lai phia bac — day la
   duong ngan nhat thuc su qua `M2`, nhung co U-turn.
4. Vi U-turn chi phi bang 0, duong vong nay chi them thoi gian di chuyen cua
   cac canh phu — de dang nam trong stretch factor 1.3x.
5. Khong co kiem tra bounded-stretch de bat doan lech cuc bo khong toi uu.
6. Bo loc khoang cach dia ly (2x) khong bat duoc vi tong chieu dai dia ly
   van duoi nguong.

---

## 7. De xuat sua chua

### Sua 1 — Tich hop kiem tra tinh hop le SeArCCH

**Khong thay doi `rust_road_router`. Tang cuong phuong phap via-node hien tai
voi pipeline chat luong tu bai bao SeArCCH.**

Implementation hien tai da lam dung phan kho: thu thap to tien chung
(= separator vertices) va tai tao duong di qua chung. Cai thieu la **pipeline
kiem tra tinh hop le 4 buoc** de loc bo cac ung vien kem chat luong.

#### Nhung gi da co

Luong `multi_query()` hien tai:

```
collect_meeting_nodes() → sap xep theo d(s,v)+d(v,t)
  → voi moi ung vien:
      reconstruct_path()  → bo loc da dang Jaccard → chap nhan/tu choi
```

#### Can them: Kiem tra Bounded Stretch (tac dong lon nhat)

Chen mot **kiem tra bounded stretch** giua tai tao duong va bo loc Jaccard.
Voi moi via-vertex `v` co duong da unpack `P(s,v,t)`:

1. **Tim diem lech `a` va `b`** — noi duong via-path lech khoi va nhap lai
   duong ngan nhat.

   Cach don gian (du cho quy mo cua chung ta): unpack hoan toan ca `P(s,v,t)`
   va `P(s,t)`, roi di tu `s` ve phia truoc de tim `a` (diem dau tien noi
   hai duong lech nhau) va tu `t` di nguoc de tim `b`.

2. **Tinh `d(a,b)` bang CCH query rieng.**

   Day la buoc then chot. `MultiRouteServer` muon `&'a C` (tham chieu bat
   bien den `Customized`). Co the chay mot elimination tree walk doc lap
   bang co so ha tang da import trong `multi_route.rs` — chi can cap phat
   mang distance/parent tam, chay hai walk, va tim khoang cach meeting-node
   nho nhat. Khong thay doi trang thai chung nao ca.

3. **Kiem tra**: loai bo `v` neu `c(a→v→b) > (1 + e) * d(a,b)`, voi `e = 0.25`.

#### Can them: Chia se theo chi phi (tac dong vua)

Thay the Jaccard dem canh bang chia se theo chi phi:

```rust
// Hien tai: Jaccard tren tap canh (khong co trong so)
let dominated = jaccard_overlap(&edge_set, &accepted_set) > 0.85;

// De xuat: chia se theo chi phi
let shared_cost: Weight = via_path_edges.iter()
    .filter(|e| shortest_path_edges.contains(e))
    .map(|&(u, v)| edge_weight(u, v))
    .sum();
let dominated = shared_cost as f64 > 0.8 * optimal_distance as f64;
```

#### Can them: T-test (uu tien thap hon)

T-test xac minh tinh toi uu cuc bo. Co the hoan lai — kiem tra bounded stretch
da bat hau het cac van de tuong tu.

#### Luong nang cap de xuat

```
collect_meeting_nodes()  → sap xep theo d(s,v)+d(v,t)
  → tai tao duong ngan nhat P(s,t) mot lan
  → voi moi ung vien v:
      1. Cat tia stretch tong: d(s,v)+d(v,t) > (1+e)*d(s,t)? → dung
      2. reconstruct_path(s, v, t)
      3. Tim diem lech a, b
      4. Bounded stretch: c(a→v→b) > (1+e)*d(a,b)? → bo qua
      5. Chia se theo chi phi: shared_cost > g*d(s,t)? → bo qua
      6. (Tuy chon) T-test: doan con toi uu cuc bo? → bo qua neu khong
      7. Chap nhan lam tuyen thay the
```

#### Tuy chon: Mo rong de quy hai buoc

Neu phuong phap co ban tao ra qua it tuyen thay the, co the them phuong phap
hai buoc:

1. Tim vertex rank cao nhat `v` tren duong ngan nhat.
2. De quy tim tuyen thay the trong `s → v_s` va `v_t → t`.
3. Ket hop cac tuyen thay the con thanh duong day du.

Dieu nay nang ty le thanh cong tu 65% len 84–90%. Xem tai lieu tham chieu
de biet cac cong thuc dieu chinh tham so.

### Sua 2 — K duong ngan nhat bang penalty (An toan dong thoi)

**Khong thay doi `rust_road_router`. Hoat dong ben canh phuong phap via-node
hien tai hoac nhu phuong thuc thay the.**

Phuong phap penalty ban dau duoc coi la co van de race condition: thay doi
trong so customized chung trong khi nguoi dung khac dang query se lam sai ket
qua cua ho. Tuy nhien, **van de nay co the giai quyet duoc** vi kien truc da
ho tro tao customization doc lap.

#### Tai sao an toan dong thoi

`CchContext` giu CCH topology (bat bien) va baseline weights. Ham then chot:

```rust
// cch.rs — CchContext
pub fn customize_with(&self, weights: &[Weight]) -> CustomizedBasic<'_, CCH> {
    let metric = FirstOutGraph::new(&self.graph.first_out[..], &self.graph.head[..], weights);
    customize(&self.cch, &metric)  // tra ve CustomizedBasic MOI, DOC LAP
}
```

`customize_with()` nhan `&self` (muon bat bien) va tra ve **`CustomizedBasic`
moi, so huu cuc bo**. No khong cham vao `self.server` chung ma cac query khac
su dung. Topology CCH duoc chia se chi doc; chi cac mang trong so trong
`CustomizedBasic` la cap phat moi.

Luong penalty-based:

```
multi_query(&self, from, to, max_alternatives, stretch):
  1. Dung self.server (chung) de tim duong toi uu P1 qua query chuan.
  2. Voi k = 2..K:
     a. Clone self.context.baseline_weights → penalty_weights cuc bo.
     b. Voi moi canh trong cac duong da chap nhan: penalty_weights[e] *= 2.
     c. let penalized = self.context.customize_with(&penalty_weights);
        // ↑ Tao CustomizedBasic MOI — self.server KHONG BI THAY DOI.
     d. let mut penalty_server = CchQueryServer::new(penalized);
     e. let result = penalty_server.query(Query { from, to });
     f. Loc da dang so voi cac duong da chap nhan.
     g. Neu du da dang, chap nhan voi khoang cach GOC (tu baseline).
  3. Tra ve tat ca cac duong da chap nhan.
```

Tai khong thoi diem nao `self.server` (customization chung) bi thay doi. Moi
lan lap penalty tao `CustomizedBasic` va `CchQueryServer` rieng tren stack,
query no, roi drop. Nguoi dung khac query dong thoi qua cung `self.server`
thay trong so goc, khong bi penalty.

#### Phan tich chi phi

- `customize_with()` la buoc ton kem nhat — chay toan bo pha relaxation tam
  giac. Voi mang Hanoi (~100–300K node), thong thuong mat 100–300 ms.
- Voi K=3 tuyen thay the, tong overhead customization khoang 300–900 ms.
- Chap nhan duoc cho multi-route query (nguoi dung ky vong tuyen thay the mat
  nhieu thoi gian hon), nhung qua cham cho single-route query.

#### Khi nao dung cai nay vs Sua 1

| Khia canh | Sua 1 (kiem tra SeArCCH) | Sua 2 (Penalty K-paths) |
|-----------|------------------------|------------------------|
| Nguon ung vien | To tien chung tu walk | Query duong ngan nhat doc lap |
| Chat luong duong | Xac minh bang kiem tra hop le | Tu nhien tot (moi duong la duong ngan nhat thuc) |
| Da dang | Dam bao bang kiem tra chia se | Dam bao bang penalty hoa + Jaccard |
| Toc do | Nhanh (~1–5 ms tong) | Cham hon (~300–900 ms voi K=3) |
| Cai dat | Trung binh (tim diem lech, query them) | Don gian hon (clone weights, re-customize, query) |
| Han che | Co the khong tim duoc tuyen neu tap separator nho | Luon tim duoc tuyen thay the neu ton tai |

Hai cach tiep can nay **bo sung cho nhau**:
- Dung Sua 1 lam phuong phap chinh (nhanh).
- Du phong Sua 2 khi Sua 1 tao ra it hon `max_alternatives`
  (separator qua nho, tat ca ung vien truot kiem tra hop le).

### Sua 3 — Them penalty re (Sua Van de 2)

**Thay doi mot file trong `generate_line_graph.rs`. Can chay lai pipeline
tao line graph.**

Thay doi callback chi phi re:

```rust
let exp_graph = line_graph(&graph, |edge1_idx, edge2_idx| {
    // ... kiem tra re cam ...

    // Cam U-turn (hoac phat nang)
    if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
        return None; // CAM
        // Hoac: return Some(30_000); // phat 30 giay
    }

    // Tuy chon: phat theo goc re cho re gap
    // let angle = compute_turn_angle(edge1_idx, edge2_idx, &lat, &lng, &tail, &head);
    // Some(angle_penalty(angle))

    Some(0) // di thang va re nhe: khong phat
});
```

**Thay doi toi thieu** (chi cam U-turn):

```rust
if tail[edge1_idx as usize] == graph.head()[edge2_idx as usize] {
    return None; // truoc do: Some(0)
}
```

**Phien ban day du** (phat theo goc — co so ha tang da co trong
`hanoi-core/src/geometry.rs` voi ham tinh huong re):

| Loai re | Pham vi goc | Penalty de xuat |
|---------|-------------|-----------------|
| U-turn | > 160 do | Cam (`None`) hoac 30 000 ms |
| Re gap | 120–160 do | 10 000 ms (10 s) |
| Re vua | 60–120 do | 5 000 ms (5 s) |
| Re nhe | 30–60 do | 2 000 ms (2 s) |
| Di thang | < 30 do | 0 ms |

### Sua 4 — Cham diem ung vien ket hop (Sua Van de 3)

**Thay doi trong `cch.rs` va `line_graph.rs` multi-query wrappers.**

Sau khi tai tao moi duong ung vien va tinh khoang cach dia ly, ap dung
diem tong hop thay vi dung thoi gian di chuyen thuan de sap xep:

```rust
let direct_dist = haversine_m(from_lat, from_lng, to_lat, to_lng);
let detour_ratio = distance_m / direct_dist;
let score = distance_ms as f64 * detour_ratio.sqrt();
```

Sap xep ung vien theo diem tong hop. Tuyen nhanh nhung uon luon ve dia ly
duoc diem cao hon (te hon) so voi tuyen nhanh tuong duong nhung truc tiep hon.

Dong thoi siet `MAX_GEO_RATIO` tu `2.0` xuong `1.5` — tuyen dai hon 50%
khoang cach dia ly da la duong vong dang ke trong mang do thi.

---

## 8. Ma tran uu tien

| Uu tien | Sua chua | No luc | Tac dong | Thay doi `rust_road_router` |
|---------|----------|--------|----------|----------------------------|
| **1** | Sua 3: Penalty U-turn / re | Nho | Ngan chan lam dung U-turn trong moi query | Khong |
| **2** | Sua 1: Kiem tra bounded stretch SeArCCH | Trung binh | Loai bo duong vong cuc bo khong toi uu | Khong |
| **3** | Sua 4: Cham diem ket hop + siet bo loc dia ly | Nho | Cai thien chat luong xep hang tuyen thay the | Khong |
| **4** | Sua 2: Penalty K-paths (du phong) | Trung binh | Du phong khi Sua 1 tim qua it tuyen thay the | Khong |
| Sau | Mo rong Sua 1: T-test, de quy hai buoc | Trung binh–Lon | Tang ty le thanh cong tim tuyen thay the | Khong |

Tat ca cac ban sua deu nam trong code `CCH-Hanoi` va pipeline
(`generate_line_graph`). Khong can sua doi `rust_road_router` hoac `RoutingKit`.

**Thu tu cai dat de xuat**:

1. **Sua 3** (penalty re) — thay doi don gian nhat, cai thien ngay lap tuc
   cho tat ca cac query (don le + nhieu tuyen), chi can chay lai
   `generate_line_graph`.

2. **Sua 1 bounded stretch** — cai thien chat luong cot loi. Them phat hien
   diem lech va query `d(a,b)` cho moi ung vien. Day la tich hop SeArCCH toi
   thieu kha thi va giai quyet van de cau truc goc.

3. **Sua 4** (cham diem ket hop) — tinh chinh nhanh sau khi Sua 1 da co.

4. **Sua 2** (penalty K-paths) — them lam duong du phong khi Sua 1 khong tao
   du tuyen thay the da dang. An toan dong thoi vi moi lan lap penalty tao
   `CustomizedBasic` doc lap qua `context.customize_with()` — server chung
   khong bao gio bi thay doi.

5. **Mo rong Sua 1** (T-test, chia se theo chi phi, de quy hai buoc) —
   nang cap dan khi phuong phap co ban khong tao du tuyen thay the tot cho
   mot so cap query nhat dinh.
