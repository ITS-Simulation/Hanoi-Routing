# Hanoi Speed Calibration Plan

## Problem Statement

The current default speed table in `RoutingKit/src/osm_profile.cpp` uses European-calibrated defaults that don't reflect Vietnamese traffic law or Hanoi road conditions. This causes two issues:

1. **Legal inaccuracy**: Vietnam has specific speed limits per vehicle class (Circular 31/2019, now replaced by Circular 38/2024 effective 01/01/2025) that differ from the defaults.
2. **Routing quality**: The car profile assigns 8 km/h to `service`/`track` roads, which is too high — the router will still send cars through physically impassable narrow alleys (ngõ/hẻm) when the detour is modest.

## Vietnamese Speed Limit Law (Current as of 2025)

Source: Circular 38/2024/TT-BGTVT (replacing Circular 31/2019), effective 01/01/2025.

### Urban areas (khu vực đông dân cư) — excludes expressways

| Road type | Car (ô tô) | Motorcycle (xe mô tô) |
|---|---|---|
| Divided road or one-way ≥2 motor lanes | **60 km/h** | **60 km/h** |
| Undivided two-way or one-way 1 motor lane | **50 km/h** | **50 km/h** |
| Moped/electric moped (xe gắn máy) | — | **40 km/h** |

### Outside urban areas (ngoài khu vực đông dân cư) — excludes expressways

| Road type | Car (ô tô ≤30 seats, ≤3.5t) | Motorcycle (xe mô tô) |
|---|---|---|
| Divided road or one-way ≥2 motor lanes | **90 km/h** | **70 km/h** |
| Undivided two-way or one-way 1 motor lane | **80 km/h** | **60 km/h** |

### Expressways (đường cao tốc)

| Vehicle | Max |
|---|---|
| Car | **120 km/h** (min 60 km/h) |
| Motorcycle | **Not permitted on expressways** |

> **Key takeaway**: Motorcycles are capped at **60 km/h in urban**, **70 km/h outside urban**. Cars get up to **60 km/h urban**, **90 km/h rural**. Motorcycles are **banned from expressways** (highway=motorway in OSM).

## Current vs. Proposed Default Speeds

### CAR profile (`get_osm_way_speed`)

The "Realistic Hanoi" column accounts for both law and actual Hanoi traffic flow. Note: `maxspeed` OSM tags override these defaults when present.

| Highway type | Current (km/h) | Legal max (km/h) | Proposed Hanoi default (km/h) | Rationale |
|---|---|---|---|---|
| `motorway` | 90 | 120 | **100** | Hanoi expressways (Vành Đai 3 cao tốc, Pháp Vân–Cầu Giẽ) realistically ~100 |
| `motorway_link` | 45 | — | **40** | Ramp merging, lower |
| `trunk` | 85 | 80–90 | **70** | Trunk in Hanoi (e.g., Vành Đai 2, Vành Đai 3 surface) is urban, congested; 70 is realistic |
| `trunk_link` | 40 | — | **35** | Proportional |
| `primary` | 65 | 50–60 | **50** | Major urban roads (Nguyễn Trãi, Giải Phóng) — divided but signal-heavy; legal limit 60, realistic ~50 |
| `primary_link` | 30 | — | **30** | OK as-is |
| `secondary` | 55 | 50 | **40** | Urban two-lane roads (many in Hanoi core); legal limit 50, realistic ~40 with signals |
| `secondary_link` | 25 | — | **25** | OK as-is |
| `tertiary` | 40 | 50 | **30** | Smaller urban roads, intersections, mixed traffic; realistic ~30 |
| `tertiary_link` | 20 | — | **20** | OK as-is |
| `unclassified` | 25 | 50 | **20** | Narrow, often single-lane; conservative |
| `residential` | 25 | 30–50 | **20** | Residential lanes in Hanoi are narrow with parked motorbikes |
| `living_street` | 10 | 20 | **10** | OK as-is, very slow is correct |
| `service` | **8** | — | **4** | **Key change**: make service roads very unattractive for cars. Many Hanoi service roads are physically too narrow for cars. 4 km/h ≈ walking speed, massive penalty |
| `track` | **8** | — | **4** | Same as service — unpaved or very narrow |
| `ferry` | 5 | — | **5** | OK as-is |
| junction (roundabout) | 20 | — | **15** | Hanoi roundabouts are chaotic, slower |

### MOTORCYCLE profile (`get_osm_motorcycle_way_speed`)

Motorcycles in Hanoi are far more agile than cars in congested traffic — they lane-filter, slip through gaps, and maintain closer to free-flow speed on larger roads. The speed penalty relative to cars should mainly apply on small/narrow roads where physical width is the constraint, not on wide multi-lane roads where motorcycles actually move *faster* than cars in practice.

| Highway type | Current (km/h) | Legal max (km/h) | Proposed Hanoi default (km/h) | Rationale |
|---|---|---|---|---|
| `motorway` | 80 | 70 (max outside urban, divided) | **65** | National roads (QL1A) where motorcycles are allowed; legal max 70, realistic ~65. True expressways excluded by `motorcycle=no` tag. See section below |
| `motorway_link` | 40 | — | **40** | OK as-is; ramp segments, no congestion advantage |
| `trunk` | 70 | 60–70 | **60** | Wide multi-lane roads (Vành Đai 2 surface); motorcycles filter through car congestion easily; near legal max |
| `trunk_link` | 35 | — | **30** | Merge/weave segments, slower |
| `primary` | 60 | 50–60 | **55** | Major urban roads (Nguyễn Trãi, Giải Phóng); motorcycles maintain ~50 even when cars slow to ~35–40 |
| `primary_link` | 30 | — | **35** | OK as-is |
| `secondary` | 50 | 50 | **45** | Urban two-lane; motorcycles still filter well, close to legal limit |
| `secondary_link` | 25 | — | **25** | OK as-is |
| `tertiary` | 40 | 50 | **40** | Narrower but motorcycles still agile; slight discount from legal max |
| `tertiary_link` | 20 | — | **20** | OK as-is |
| `unclassified` | 30 | 40–50 | **30** | Keep default; motorcycles handle these fine |
| `residential` | 25 | 30–40 | **25** | Keep default; motorcycles are agile even on narrow residential |
| `living_street` | 15 | 20 | **15** | Keep default |
| `service` | 15 | — | **15** | Keep default; motorcycles fit and move comfortably |
| `track` | 15 | — | **15** | Keep default |
| `path` | 15 | — | **15** | Keep default; ngõ/hẻm alleys are core motorcycle territory |
| `ferry` | 5 | — | **5** | Keep default |
| junction (roundabout) | 20 | — | **20** | Keep default; motorcycles navigate roundabouts well |

## Motorcycle on Motorway-Tagged Roads

Vietnamese law prohibits motorcycles on **expressways** (đường cao tốc), but this is a per-road ban, not a blanket highway-class ban. Some national roads (quốc lộ) such as QL1A are tagged `highway=motorway` in OSM yet still legally allow motorcycles. The actual ban is road-specific and reflected by the `motorcycle=no` tag in OSM data.

**Current behavior is correct**: The motorcycle profile in `is_osm_way_used_by_motorcycles()` includes `motorway`/`motorway_link`, but checks `motorcycle=no` at the top of the function (line 267). This means:
- Roads with `motorcycle=no` (true expressways like Pháp Vân–Cầu Giẽ) → **excluded** ✓
- Roads tagged `highway=motorway` but without `motorcycle=no` (QL1A segments) → **included** ✓

**No code change needed for the way filter.** The OSM `motorcycle` tag is generally reliable for higher-tier roads in Vietnam.

### Speed defaults for motorway in motorcycle profile

Since motorcycles can legally use some motorway-tagged roads, we still need a reasonable default speed. The current 80 km/h exceeds the legal motorcycle maximum of 70 km/h (outside urban on divided road). Proposed: **60 km/h** — conservative for national roads where motorcycles mix with trucks and buses.

## Implementation

### Files to modify

1. **`RoutingKit/src/osm_profile.cpp`**
   - `get_osm_way_speed()` (line 524): Update car default speed table
   - `get_osm_motorcycle_way_speed()` (line 613): Update motorcycle default speed table

No changes to `is_osm_way_used_by_motorcycles()` — the existing `motorcycle=no` tag check already correctly handles the expressway ban on a per-road basis.

### Steps

1. Update `get_osm_way_speed()` default speeds per the car table above
2. Update `get_osm_motorcycle_way_speed()` default speeds per the motorcycle table above
3. Rebuild RoutingKit: `cd RoutingKit && ./generate_make_file && make`
4. Rebuild CCH-Generator: `cd CCH-Generator/build && cmake .. && make`
5. Re-run pipeline for both profiles to regenerate graph data

### No changes needed to
- `parse_maxspeed_value()` — this handles explicit `maxspeed` tags from OSM which override defaults. The Vietnamese named speed tags (e.g., `vn:urban`) could be added in the future but are rarely tagged in OSM.
- Turn restriction logic — unaffected by speed changes
- Rust engine/server — they consume the binary graph data; speed changes are baked in at import time

## Impact Assessment

- **Car routing quality**: Service/track roads become ~4x more expensive (8→4 km/h). A 50m service road now costs 45 seconds instead of 22.5 seconds. The router will strongly prefer any alternative that doesn't add >22.5 seconds of detour.
- **Motorcycle motorway speed**: Reduced from 80→60 km/h on motorway-tagged roads where motorcycles are allowed (e.g., QL1A segments). True expressways (Vành Đai 3 elevated, Pháp Vân–Cầu Giẽ) are already excluded via `motorcycle=no` OSM tag.
- **Overall speed reduction**: Urban defaults are generally lower, producing slightly longer travel time estimates but more realistic routes that prefer higher-grade roads.
- **OSM maxspeed override**: Any road with an explicit `maxspeed` tag in OSM is unaffected — these defaults only apply when no tag exists. In Hanoi, most major roads lack `maxspeed` tags, so the defaults matter significantly.

## Future Considerations

- **Width-based filtering**: Parse `width` tag to exclude roads narrower than ~2.5m for cars. This would be more robust than speed penalties alone.
- **Vietnam named speed zones**: Add `vn:urban` (50 km/h), `vn:rural` (80 km/h) to `parse_maxspeed_value()` if OSM mappers start using them.
- **Time-of-day speeds**: Hanoi traffic varies wildly by hour. This is already handled by the TD-CCH (time-dependent) system but requires traffic flow data.

## Sources

- [Vietnam speed limits (Circular 38/2024, effective 2025)](https://thuvienphapluat.vn/chinh-sach-phap-luat-moi/vn/ho-tro-phap-luat/chinh-sach-moi/75843/bang-toc-do-toi-da-cua-xe-o-to-trong-va-ngoai-khu-vuc-dong-dan-cu-tu-ngay-01-01-2025)
- [Decree 168/2024 penalty table](https://thuvienphapluat.vn/van-ban/Giao-thong-Van-tai/Nghi-dinh-168-2024-ND-CP-xu-phat-vi-pham-hanh-chinh-an-toan-giao-thong-duong-bo-619502.aspx)
- [Maximum speed limits from October 2019](https://lawnet.vn/thong-tin-phap-luat/en/chinh-sach-moi/vietnam-maximum-speed-limits-for-different-types-of-vehicles-applicable-from-october-15-2019-145446.html)
- [Speed limits overview (Wikivoyage)](https://en.wikivoyage.org/wiki/Driving_in_Vietnam)
- [New traffic law changes 2025](https://baochinhphu.vn/quy-dinh-ve-toc-do-toi-da-cua-xe-co-gioi-ap-dung-tu-01-01-2025-102241127101044466.htm)
