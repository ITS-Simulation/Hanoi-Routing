#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import unicodedata
from collections import defaultdict
from dataclasses import dataclass
from http import HTTPStatus
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

import numpy as np
import pyarrow.ipc as ipc
import yaml


EARTH_RADIUS_M = 6_371_000.0
DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8765
GRID_CELL_DEG = 0.001
INITIAL_SEARCH_RING = 4
MAX_SEARCH_RING = 12
BEARING_WARN_THRESHOLD_DEG = 90.0


def normalize_bearing(bearing_deg: float) -> float:
    if not math.isfinite(bearing_deg):
        raise ValueError(f"flow_bearing_deg must be finite, got {bearing_deg}")
    normalized = math.fmod(bearing_deg, 360.0)
    if normalized < 0.0:
        normalized += 360.0
    return normalized


def normalize_hour(hour: float) -> float:
    if not math.isfinite(hour):
        raise ValueError(f"hour must be finite, got {hour}")
    normalized = math.fmod(hour, 24.0)
    if normalized < 0.0:
        normalized += 24.0
    return normalized


def circular_angle_diff_deg(left_deg: float, right_deg: float) -> float:
    raw = abs(normalize_bearing(left_deg) - normalize_bearing(right_deg))
    return min(raw, 360.0 - raw)


def normalize_search_text(value: str) -> str:
    decomposed = unicodedata.normalize("NFKD", value)
    stripped = "".join(char for char in decomposed if not unicodedata.combining(char))
    return stripped.casefold()


def point_to_segment_distance_m(
    point_lat: float,
    point_lon: float,
    tail_lat: float,
    tail_lon: float,
    head_lat: float,
    head_lon: float,
) -> float:
    ref_lat_rad = math.radians(point_lat)

    def project(lat: float, lon: float) -> tuple[float, float]:
        x = math.radians(lon - point_lon) * EARTH_RADIUS_M * math.cos(ref_lat_rad)
        y = math.radians(lat - point_lat) * EARTH_RADIUS_M
        return x, y

    ax, ay = project(tail_lat, tail_lon)
    bx, by = project(head_lat, head_lon)
    abx = bx - ax
    aby = by - ay
    ab_len_sq = abx * abx + aby * aby

    if ab_len_sq == 0.0:
        return math.hypot(ax, ay)

    t = ((-ax) * abx + (-ay) * aby) / ab_len_sq
    clamped_t = min(max(t, 0.0), 1.0)
    closest_x = ax + clamped_t * abx
    closest_y = ay + clamped_t * aby
    return math.hypot(closest_x, closest_y)


@dataclass(frozen=True)
class DatasetBounds:
    min_lat: float
    max_lat: float
    min_lon: float
    max_lon: float


class ManifestIndex:
    def __init__(self, manifest_path: Path):
        self.manifest_path = manifest_path
        self._load_manifest()

    def _load_manifest(self) -> None:
        with self.manifest_path.open("rb") as fh:
            table = ipc.open_file(fh).read_all()

        columns = {name: table[name].combine_chunks() for name in table.column_names}
        required = {
            "arc_id",
            "routing_way_id",
            "osm_way_id",
            "name",
            "highway",
            "tail_lat",
            "tail_lon",
            "head_lat",
            "head_lon",
            "bearing_deg",
            "is_antiparallel_to_way",
        }
        missing = sorted(required.difference(columns))
        if missing:
            raise RuntimeError(f"road_arc_manifest.arrow is missing required columns: {missing}")

        arc_ids = columns["arc_id"].to_numpy(zero_copy_only=False).astype(np.int64, copy=False)
        order = np.argsort(arc_ids)
        arc_ids = arc_ids[order]
        expected = np.arange(arc_ids.size, dtype=np.int64)
        if not np.array_equal(arc_ids, expected):
            raise RuntimeError("road_arc_manifest.arrow must contain dense arc_id values 0..N-1")

        self.arc_count = int(arc_ids.size)
        self.routing_way_ids = columns["routing_way_id"].to_numpy(zero_copy_only=False).astype(np.int32, copy=False)[order]
        self.osm_way_ids = columns["osm_way_id"].to_numpy(zero_copy_only=False)[order]
        self.tail_lat = columns["tail_lat"].to_numpy(zero_copy_only=False).astype(np.float64, copy=False)[order]
        self.tail_lon = columns["tail_lon"].to_numpy(zero_copy_only=False).astype(np.float64, copy=False)[order]
        self.head_lat = columns["head_lat"].to_numpy(zero_copy_only=False).astype(np.float64, copy=False)[order]
        self.head_lon = columns["head_lon"].to_numpy(zero_copy_only=False).astype(np.float64, copy=False)[order]
        self.bearing_deg = columns["bearing_deg"].to_numpy(zero_copy_only=False).astype(np.float64, copy=False)[order]
        self.antiparallel = columns["is_antiparallel_to_way"].to_numpy(zero_copy_only=False).astype(np.bool_, copy=False)[order]

        name_encoded = columns["name"].dictionary_encode()
        highway_encoded = columns["highway"].dictionary_encode()
        self.name_codes = name_encoded.indices.to_numpy(zero_copy_only=False).astype(np.int32, copy=False)[order]
        self.highway_codes = highway_encoded.indices.to_numpy(zero_copy_only=False).astype(np.int16, copy=False)[order]
        self.name_values = name_encoded.dictionary.to_pylist()
        self.highway_values = highway_encoded.dictionary.to_pylist()

        self.mid_lat = (self.tail_lat + self.head_lat) / 2.0
        self.mid_lon = (self.tail_lon + self.head_lon) / 2.0
        self.bounds = DatasetBounds(
            min_lat=float(np.minimum(self.tail_lat, self.head_lat).min()),
            max_lat=float(np.maximum(self.tail_lat, self.head_lat).max()),
            min_lon=float(np.minimum(self.tail_lon, self.head_lon).min()),
            max_lon=float(np.maximum(self.tail_lon, self.head_lon).max()),
        )

        self._build_name_index()
        self._build_way_index()
        self._build_spatial_grid()

    def _build_name_index(self) -> None:
        name_count = len(self.name_values)
        self.name_arc_counts = np.bincount(self.name_codes, minlength=name_count)
        self.sample_arc_by_name = np.full(name_count, -1, dtype=np.int64)
        for arc_id, code in enumerate(self.name_codes):
            if self.sample_arc_by_name[code] == -1:
                self.sample_arc_by_name[code] = arc_id
        self.searchable_names: list[tuple[str, str, int, int]] = []
        for code, name in enumerate(self.name_values):
            if not name:
                continue
            sample_arc = int(self.sample_arc_by_name[code])
            if sample_arc < 0:
                continue
            self.searchable_names.append(
                (normalize_search_text(name), name, int(self.name_arc_counts[code]), sample_arc)
            )

    def _build_spatial_grid(self) -> None:
        self.grid: dict[tuple[int, int], list[int]] = defaultdict(list)
        for arc_id in range(self.arc_count):
            cell = self._grid_cell(float(self.mid_lat[arc_id]), float(self.mid_lon[arc_id]))
            self.grid[cell].append(arc_id)

    def _build_way_index(self) -> None:
        self.way_count = int(self.routing_way_ids.max()) + 1 if self.arc_count else 0
        counts = np.bincount(self.routing_way_ids, minlength=self.way_count)
        self.first_arc_offset_by_way = np.zeros(self.way_count + 1, dtype=np.int64)
        self.first_arc_offset_by_way[1:] = np.cumsum(counts, dtype=np.int64)
        self.arc_ids_by_way = np.empty(self.arc_count, dtype=np.int64)
        cursor = self.first_arc_offset_by_way.copy()
        for arc_id, way_id in enumerate(self.routing_way_ids):
            position = int(cursor[way_id])
            self.arc_ids_by_way[position] = arc_id
            cursor[way_id] = position + 1

    @staticmethod
    def _grid_cell(lat: float, lon: float) -> tuple[int, int]:
        return math.floor(lon / GRID_CELL_DEG), math.floor(lat / GRID_CELL_DEG)

    def arc_summary(self, arc_id: int) -> dict[str, Any]:
        if arc_id < 0 or arc_id >= self.arc_count:
            raise KeyError(f"arc_id {arc_id} is outside 0..{self.arc_count - 1}")
        return {
            "arc_id": arc_id,
            "routing_way_id": int(self.routing_way_ids[arc_id]),
            "osm_way_id": int(self.osm_way_ids[arc_id]),
            "name": self.name_values[int(self.name_codes[arc_id])],
            "highway": self.highway_values[int(self.highway_codes[arc_id])],
            "tail_lat": float(self.tail_lat[arc_id]),
            "tail_lon": float(self.tail_lon[arc_id]),
            "head_lat": float(self.head_lat[arc_id]),
            "head_lon": float(self.head_lon[arc_id]),
            "mid_lat": float(self.mid_lat[arc_id]),
            "mid_lon": float(self.mid_lon[arc_id]),
            "bearing_deg": float(self.bearing_deg[arc_id]),
            "is_antiparallel_to_way": bool(self.antiparallel[arc_id]),
        }

    def propagation_preview(self, arc_id: int) -> dict[str, Any]:
        anchor = self.arc_summary(arc_id)
        way_id = anchor["routing_way_id"]
        anchor_bearing = anchor["bearing_deg"]
        anchor_antiparallel = anchor["is_antiparallel_to_way"]

        start_offset = int(self.first_arc_offset_by_way[way_id])
        end_offset = int(self.first_arc_offset_by_way[way_id + 1])
        covered_arcs: list[dict[str, Any]] = []
        warnings: list[dict[str, Any]] = []

        min_lat = math.inf
        max_lat = -math.inf
        min_lon = math.inf
        max_lon = -math.inf

        for offset in range(start_offset, end_offset):
            sibling_arc_id = int(self.arc_ids_by_way[offset])
            if bool(self.antiparallel[sibling_arc_id]) != anchor_antiparallel:
                continue

            sibling = self.arc_summary(sibling_arc_id)
            bearing_diff = circular_angle_diff_deg(anchor_bearing, sibling["bearing_deg"])
            sibling["bearing_diff_from_anchor_deg"] = round(bearing_diff, 1)
            covered_arcs.append(sibling)

            min_lat = min(min_lat, sibling["tail_lat"], sibling["head_lat"])
            max_lat = max(max_lat, sibling["tail_lat"], sibling["head_lat"])
            min_lon = min(min_lon, sibling["tail_lon"], sibling["head_lon"])
            max_lon = max(max_lon, sibling["tail_lon"], sibling["head_lon"])

            if bearing_diff > BEARING_WARN_THRESHOLD_DEG:
                warnings.append(
                    {
                        "arc_id": sibling_arc_id,
                        "bearing_diff_deg": round(bearing_diff, 1),
                    }
                )

        if not covered_arcs:
            raise RuntimeError(
                f"Propagation preview for arc_id={arc_id} produced an empty same-direction group on routing_way_id={way_id}"
            )

        covered_arcs.sort(key=lambda sib: (0 if sib["arc_id"] == arc_id else 1, sib["arc_id"]))

        return {
            "anchor_arc": anchor,
            "routing_way_id": way_id,
            "osm_way_id": anchor["osm_way_id"],
            "name": anchor["name"],
            "highway": anchor["highway"],
            "direction_label": "against OSM way" if anchor_antiparallel else "with OSM way",
            "is_antiparallel_to_way": anchor_antiparallel,
            "anchor_bearing_deg": anchor_bearing,
            "covered_arc_count": len(covered_arcs),
            "bearing_warn_threshold_deg": BEARING_WARN_THRESHOLD_DEG,
            "warning_count": len(warnings),
            "warnings": warnings,
            "covered_arcs": covered_arcs,
            "bounds": {
                "minLat": min_lat,
                "maxLat": max_lat,
                "minLon": min_lon,
                "maxLon": max_lon,
            },
        }

    def nearest_arcs(self, lat: float, lon: float, limit: int = 8) -> list[dict[str, Any]]:
        center_x, center_y = self._grid_cell(lat, lon)
        candidate_ids: list[int] = []
        seen: set[int] = set()

        for ring in range(INITIAL_SEARCH_RING + 1):
            self._collect_ring(center_x, center_y, ring, seen, candidate_ids)

        ring = INITIAL_SEARCH_RING + 1
        while len(candidate_ids) < max(limit * 4, 20) and ring <= MAX_SEARCH_RING:
            self._collect_ring(center_x, center_y, ring, seen, candidate_ids)
            ring += 1

        if not candidate_ids:
            return []

        scored: list[tuple[float, int]] = []
        for arc_id in candidate_ids:
            distance_m = point_to_segment_distance_m(
                lat,
                lon,
                float(self.tail_lat[arc_id]),
                float(self.tail_lon[arc_id]),
                float(self.head_lat[arc_id]),
                float(self.head_lon[arc_id]),
            )
            scored.append((distance_m, arc_id))

        scored.sort(key=lambda item: item[0])
        result: list[dict[str, Any]] = []
        for distance_m, arc_id in scored[:limit]:
            summary = self.arc_summary(arc_id)
            summary["distance_m"] = round(distance_m, 2)
            result.append(summary)
        return result

    def _collect_ring(self, center_x: int, center_y: int, ring: int, seen: set[int], out: list[int]) -> None:
        if ring == 0:
            self._collect_cell(center_x, center_y, seen, out)
            return

        for dx in range(-ring, ring + 1):
            self._collect_cell(center_x + dx, center_y - ring, seen, out)
            self._collect_cell(center_x + dx, center_y + ring, seen, out)
        for dy in range(-ring + 1, ring):
            self._collect_cell(center_x - ring, center_y + dy, seen, out)
            self._collect_cell(center_x + ring, center_y + dy, seen, out)

    def _collect_cell(self, cell_x: int, cell_y: int, seen: set[int], out: list[int]) -> None:
        for arc_id in self.grid.get((cell_x, cell_y), ()):
            if arc_id not in seen:
                seen.add(arc_id)
                out.append(arc_id)

    def search_roads(self, query: str, limit: int = 20) -> list[dict[str, Any]]:
        normalized = normalize_search_text(query.strip())
        if len(normalized) < 2:
            return []

        matches: list[tuple[int, int, str, int]] = []
        for lower_name, original_name, arc_count, sample_arc in self.searchable_names:
            if normalized in lower_name:
                prefix_rank = 0 if lower_name.startswith(normalized) else 1
                matches.append((prefix_rank, -arc_count, original_name, sample_arc))

        matches.sort()
        results: list[dict[str, Any]] = []
        for _, neg_count, name, sample_arc in matches[:limit]:
            sample = self.arc_summary(sample_arc)
            results.append(
                {
                    "name": name,
                    "arc_count": -neg_count,
                    "sample_arc": sample,
                }
            )
        return results


class ConfigValidationError(ValueError):
    pass


class UniqueKeyLoader(yaml.SafeLoader):
    pass


def _construct_unique_mapping(loader: UniqueKeyLoader, node: yaml.nodes.MappingNode, deep: bool = False) -> dict[Any, Any]:
    mapping: dict[Any, Any] = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise ConfigValidationError(f"Duplicate YAML key '{key}'")
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


UniqueKeyLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _construct_unique_mapping)


def _require_mapping(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ConfigValidationError(f"{context} must be an object")
    return value


def _require_list(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise ConfigValidationError(f"{context} must be a list")
    return value


def _require_string(value: Any, context: str) -> str:
    if not isinstance(value, str):
        raise ConfigValidationError(f"{context} must be a string")
    return value


def _require_int(value: Any, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ConfigValidationError(f"{context} must be an integer")
    int_value = int(value)
    if int_value != value:
        raise ConfigValidationError(f"{context} must be an integer")
    return int_value


def _require_float(value: Any, context: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ConfigValidationError(f"{context} must be numeric")
    float_value = float(value)
    if not math.isfinite(float_value):
        raise ConfigValidationError(f"{context} must be finite, got {float_value}")
    return float_value


def parse_yaml_document(yaml_text: str, manifest: ManifestIndex) -> dict[str, Any]:
    try:
        loaded = yaml.load(yaml_text, Loader=UniqueKeyLoader)
    except ConfigValidationError:
        raise
    except yaml.YAMLError as exc:
        raise ConfigValidationError(f"Invalid YAML: {exc}") from exc

    if loaded is None:
        return {"profiles": [], "cameras": []}

    root = _require_mapping(loaded, "root YAML document")
    profiles, known_profile_names = _parse_profiles_document(root.get("profiles"))
    cameras = _parse_cameras_document(root.get("cameras"), known_profile_names, manifest)
    _validate_imported_camera_overlaps(cameras)
    return {"profiles": profiles, "cameras": cameras}


def _parse_profiles_document(node: Any) -> tuple[list[dict[str, Any]], set[str]]:
    if node is None:
        return [], set()

    profiles_map = _require_mapping(node, "profiles")
    profiles: list[dict[str, Any]] = []
    names: set[str] = set()

    for raw_name, raw_profile in profiles_map.items():
        profile_name = _require_string(raw_name, "profile name").strip()
        if not profile_name:
            raise ConfigValidationError("profile name must not be blank")
        if profile_name in names:
            raise ConfigValidationError(f"duplicate profile name '{profile_name}'")
        names.add(profile_name)

        profile_map = _require_mapping(raw_profile, f"profile '{profile_name}'")
        free_flow_kmh = _require_float(profile_map.get("free_flow_kmh"), f"profile '{profile_name}'.free_flow_kmh")
        if free_flow_kmh <= 0.0:
            raise ConfigValidationError(f"profile '{profile_name}'.free_flow_kmh must be > 0")

        free_flow_occupancy = _require_float(
            profile_map.get("free_flow_occupancy"),
            f"profile '{profile_name}'.free_flow_occupancy",
        )
        if not (0.0 <= free_flow_occupancy <= 1.0):
            raise ConfigValidationError(f"profile '{profile_name}'.free_flow_occupancy must be in [0,1]")

        peaks: list[dict[str, float]] = []
        for peak_idx, raw_peak in enumerate(_require_list(profile_map.get("peaks", []), f"profile '{profile_name}'.peaks")):
            peak_map = _require_mapping(raw_peak, f"profile '{profile_name}'.peaks[{peak_idx}]")
            hour = normalize_hour(_require_float(peak_map.get("hour"), f"profile '{profile_name}'.peaks[{peak_idx}].hour"))
            speed_kmh = _require_float(
                peak_map.get("speed_kmh"),
                f"profile '{profile_name}'.peaks[{peak_idx}].speed_kmh",
            )
            occupancy = _require_float(
                peak_map.get("occupancy"),
                f"profile '{profile_name}'.peaks[{peak_idx}].occupancy",
            )
            if speed_kmh <= 0.0:
                raise ConfigValidationError(f"profile '{profile_name}'.peaks[{peak_idx}].speed_kmh must be > 0")
            if not (0.0 <= occupancy <= 1.0):
                raise ConfigValidationError(f"profile '{profile_name}'.peaks[{peak_idx}].occupancy must be in [0,1]")
            peaks.append({"hour": hour, "speedKmh": speed_kmh, "occupancy": occupancy})

        profiles.append(
            {
                "name": profile_name,
                "freeFlowKmh": free_flow_kmh,
                "freeFlowOccupancy": free_flow_occupancy,
                "peaks": peaks,
            }
        )

    return profiles, names


def _parse_cameras_document(node: Any, known_profile_names: set[str], manifest: ManifestIndex) -> list[dict[str, Any]]:
    if node is None:
        return []

    cameras = _require_list(node, "cameras")
    imported: list[dict[str, Any]] = []
    camera_ids: set[int] = set()

    for idx, raw_camera in enumerate(cameras):
        camera_map = _require_mapping(raw_camera, f"cameras[{idx}]")
        camera_id = _require_int(camera_map.get("id"), f"cameras[{idx}].id")
        if camera_id in camera_ids:
            raise ConfigValidationError(f"duplicate camera id {camera_id}")
        camera_ids.add(camera_id)

        label = _require_string(camera_map.get("label"), f"cameras[{idx}].label").strip()
        if not label:
            raise ConfigValidationError(f"cameras[{idx}].label must not be blank")

        profile_name = _require_string(camera_map.get("profile"), f"cameras[{idx}].profile").strip()
        if not profile_name:
            raise ConfigValidationError(f"cameras[{idx}].profile must not be blank")
        if profile_name not in known_profile_names:
            raise ConfigValidationError(
                f"camera {camera_id} ('{label}') references unknown profile '{profile_name}'"
            )

        arc_id_raw = camera_map.get("arc_id")
        lat_raw = camera_map.get("lat")
        lon_raw = camera_map.get("lon")
        flow_bearing_raw = camera_map.get("flow_bearing_deg")

        has_explicit_arc = arc_id_raw is not None
        has_coordinate_mode = lat_raw is not None or lon_raw is not None or flow_bearing_raw is not None
        if has_explicit_arc == has_coordinate_mode:
            raise ConfigValidationError(
                f"cameras[{idx}] must provide exactly one placement mode: either arc_id or lat/lon/flow_bearing_deg"
            )

        if has_explicit_arc:
            arc_id = _require_int(arc_id_raw, f"cameras[{idx}].arc_id")
            if arc_id < 0:
                raise ConfigValidationError(f"cameras[{idx}].arc_id must be >= 0")
            try:
                selected_arc = manifest.arc_summary(arc_id)
            except KeyError as exc:
                raise ConfigValidationError(str(exc)) from exc
            imported.append(
                _build_imported_camera(
                    manifest=manifest,
                    camera_id=camera_id,
                    label=label,
                    profile_name=profile_name,
                    placement_mode="arc",
                    selected_arc=selected_arc,
                    display_lat=float(selected_arc["mid_lat"]),
                    display_lon=float(selected_arc["mid_lon"]),
                    placement_fields={"arcId": arc_id},
                )
            )
            continue

        lat = _require_float(lat_raw, f"cameras[{idx}].lat")
        lon = _require_float(lon_raw, f"cameras[{idx}].lon")
        flow_bearing_deg = normalize_bearing(_require_float(flow_bearing_raw, f"cameras[{idx}].flow_bearing_deg"))
        if not (-90.0 <= lat <= 90.0):
            raise ConfigValidationError(f"cameras[{idx}].lat must be in [-90,90]")
        if not (-180.0 <= lon <= 180.0):
            raise ConfigValidationError(f"cameras[{idx}].lon must be in [-180,180]")

        selected_arc = _resolve_coordinate_camera(manifest, lat, lon, flow_bearing_deg, camera_id, label)
        imported.append(
            _build_imported_camera(
                manifest=manifest,
                camera_id=camera_id,
                label=label,
                profile_name=profile_name,
                placement_mode="coordinate",
                selected_arc=selected_arc,
                display_lat=lat,
                display_lon=lon,
                placement_fields={
                    "lat": lat,
                    "lon": lon,
                    "flowBearingDeg": flow_bearing_deg,
                },
            )
        )

    return imported


def _resolve_coordinate_camera(
    manifest: ManifestIndex,
    lat: float,
    lon: float,
    flow_bearing_deg: float,
    camera_id: int,
    label: str,
) -> dict[str, Any]:
    candidates = manifest.nearest_arcs(lat, lon, limit=8)
    if not candidates:
        raise ConfigValidationError(
            f"camera {camera_id} ('{label}') could not be resolved to any nearby directed arc"
        )

    ranked = []
    for candidate in candidates:
        bearing_diff = circular_angle_diff_deg(float(candidate["bearing_deg"]), flow_bearing_deg)
        distance_m = float(candidate.get("distance_m", 0.0))
        score = distance_m + bearing_diff * 2.0
        ranked.append((score, bearing_diff, distance_m, int(candidate["arc_id"]), candidate))

    ranked.sort(key=lambda item: (item[0], item[1], item[2], item[3]))
    return ranked[0][4]


def _build_imported_camera(
    manifest: ManifestIndex,
    camera_id: int,
    label: str,
    profile_name: str,
    placement_mode: str,
    selected_arc: dict[str, Any],
    display_lat: float,
    display_lon: float,
    placement_fields: dict[str, Any],
) -> dict[str, Any]:
    preview = manifest.propagation_preview(int(selected_arc["arc_id"]))
    return {
        "id": camera_id,
        "label": label,
        "profile": profile_name,
        "placementMode": placement_mode,
        "selectedArc": selected_arc,
        "displayLat": display_lat,
        "displayLon": display_lon,
        "representedWay": {
            "routingWayId": preview["routing_way_id"],
            "name": preview["name"],
            "directionLabel": preview["direction_label"],
            "coveredArcCount": preview["covered_arc_count"],
            "warningCount": preview["warning_count"],
        },
        "propagatedArcIds": [int(arc["arc_id"]) for arc in preview["covered_arcs"]],
        **placement_fields,
    }


def _validate_imported_camera_overlaps(cameras: list[dict[str, Any]]) -> None:
    owner_by_arc: dict[int, dict[str, Any]] = {}
    for camera in cameras:
        for raw_arc_id in camera.get("propagatedArcIds", []):
            arc_id = int(raw_arc_id)
            previous = owner_by_arc.get(arc_id)
            if previous is not None and previous["id"] != camera["id"]:
                raise ConfigValidationError(
                    f"camera {camera['id']} ('{camera['label']}') overlaps propagated arc {arc_id} already represented by camera {previous['id']} ('{previous['label']}')"
                )
            owner_by_arc[arc_id] = camera


def build_yaml_document(payload: dict[str, Any]) -> str:
    profiles_raw = _require_list(payload.get("profiles", []), "profiles")
    cameras_raw = _require_list(payload.get("cameras", []), "cameras")

    profiles_yaml: dict[str, dict[str, Any]] = {}
    for idx, raw_profile in enumerate(profiles_raw):
        profile = _require_mapping(raw_profile, f"profiles[{idx}]")
        name = _require_string(profile.get("name"), f"profiles[{idx}].name").strip()
        if not name:
            raise ConfigValidationError(f"profiles[{idx}].name must not be blank")
        if name in profiles_yaml:
            raise ConfigValidationError(f"duplicate profile name '{name}'")

        free_flow_kmh = _require_float(profile.get("freeFlowKmh"), f"profiles[{idx}].freeFlowKmh")
        if free_flow_kmh <= 0.0:
            raise ConfigValidationError(f"profiles[{idx}].freeFlowKmh must be > 0")

        free_flow_occupancy = _require_float(
            profile.get("freeFlowOccupancy"),
            f"profiles[{idx}].freeFlowOccupancy",
        )
        if not (0.0 <= free_flow_occupancy <= 1.0):
            raise ConfigValidationError(f"profiles[{idx}].freeFlowOccupancy must be in [0,1]")

        peaks_yaml: list[dict[str, float]] = []
        for peak_idx, raw_peak in enumerate(_require_list(profile.get("peaks", []), f"profiles[{idx}].peaks")):
            peak = _require_mapping(raw_peak, f"profiles[{idx}].peaks[{peak_idx}]")
            hour = normalize_hour(_require_float(peak.get("hour"), f"profiles[{idx}].peaks[{peak_idx}].hour"))
            speed_kmh = _require_float(peak.get("speedKmh"), f"profiles[{idx}].peaks[{peak_idx}].speedKmh")
            occupancy = _require_float(peak.get("occupancy"), f"profiles[{idx}].peaks[{peak_idx}].occupancy")
            if speed_kmh <= 0.0:
                raise ConfigValidationError(f"profiles[{idx}].peaks[{peak_idx}].speedKmh must be > 0")
            if not (0.0 <= occupancy <= 1.0):
                raise ConfigValidationError(f"profiles[{idx}].peaks[{peak_idx}].occupancy must be in [0,1]")
            peaks_yaml.append(
                {
                    "hour": hour,
                    "speed_kmh": speed_kmh,
                    "occupancy": occupancy,
                }
            )

        profiles_yaml[name] = {
            "free_flow_kmh": free_flow_kmh,
            "free_flow_occupancy": free_flow_occupancy,
            "peaks": peaks_yaml,
        }

    camera_ids: set[int] = set()
    cameras_yaml: list[dict[str, Any]] = []
    for idx, raw_camera in enumerate(cameras_raw):
        camera = _require_mapping(raw_camera, f"cameras[{idx}]")
        camera_id = _require_int(camera.get("id"), f"cameras[{idx}].id")
        if camera_id in camera_ids:
            raise ConfigValidationError(f"duplicate camera id {camera_id}")
        camera_ids.add(camera_id)

        label = _require_string(camera.get("label"), f"cameras[{idx}].label").strip()
        if not label:
            raise ConfigValidationError(f"cameras[{idx}].label must not be blank")

        profile_name = _require_string(camera.get("profile"), f"cameras[{idx}].profile").strip()
        if not profile_name:
            raise ConfigValidationError(f"cameras[{idx}].profile must not be blank")
        if profile_name not in profiles_yaml:
            raise ConfigValidationError(
                f"camera {camera_id} ('{label}') references unknown profile '{profile_name}'"
            )

        placement_mode = _require_string(camera.get("placementMode"), f"cameras[{idx}].placementMode")
        if placement_mode not in {"arc", "coordinate"}:
            raise ConfigValidationError(f"cameras[{idx}].placementMode must be 'arc' or 'coordinate'")

        camera_yaml: dict[str, Any] = {"id": camera_id, "label": label, "profile": profile_name}

        if placement_mode == "arc":
            arc_id = _require_int(camera.get("arcId"), f"cameras[{idx}].arcId")
            if arc_id < 0:
                raise ConfigValidationError(f"cameras[{idx}].arcId must be >= 0")
            camera_yaml["arc_id"] = arc_id
        else:
            lat = _require_float(camera.get("lat"), f"cameras[{idx}].lat")
            lon = _require_float(camera.get("lon"), f"cameras[{idx}].lon")
            flow_bearing_deg = normalize_bearing(
                _require_float(camera.get("flowBearingDeg"), f"cameras[{idx}].flowBearingDeg")
            )
            if not (-90.0 <= lat <= 90.0):
                raise ConfigValidationError(f"cameras[{idx}].lat must be in [-90,90]")
            if not (-180.0 <= lon <= 180.0):
                raise ConfigValidationError(f"cameras[{idx}].lon must be in [-180,180]")
            camera_yaml["lat"] = lat
            camera_yaml["lon"] = lon
            camera_yaml["flow_bearing_deg"] = flow_bearing_deg

        cameras_yaml.append(camera_yaml)

    document = {
        "profiles": profiles_yaml,
        "cameras": cameras_yaml,
    }
    return yaml.safe_dump(document, sort_keys=False, allow_unicode=True)


def resolve_graph_dir(input_path: Path) -> Path:
    if input_path.joinpath("road_arc_manifest.arrow").is_file():
        return input_path

    nested = input_path.joinpath("graph")
    if nested.joinpath("road_arc_manifest.arrow").is_file():
        return nested

    raise FileNotFoundError(
        f"Could not resolve graph directory from {input_path}. Expected "
        f"{input_path}/road_arc_manifest.arrow or {input_path}/graph/road_arc_manifest.arrow"
    )


class CameraConfigHandler(SimpleHTTPRequestHandler):
    manifest: ManifestIndex

    def __init__(self, *args: Any, directory: str | None = None, manifest: ManifestIndex | None = None, **kwargs: Any):
        self.manifest = manifest  # type: ignore[assignment]
        super().__init__(*args, directory=directory, **kwargs)

    def do_GET(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)
        if parsed.path == "/api/status":
            self._send_json(
                {
                    "graphDir": str(self.manifest.manifest_path.parent),
                    "manifestPath": str(self.manifest.manifest_path),
                    "arcCount": self.manifest.arc_count,
                    "wayCount": self.manifest.way_count,
                    "bounds": {
                        "minLat": self.manifest.bounds.min_lat,
                        "maxLat": self.manifest.bounds.max_lat,
                        "minLon": self.manifest.bounds.min_lon,
                        "maxLon": self.manifest.bounds.max_lon,
                    },
                }
            )
            return

        if parsed.path == "/api/nearby_arcs":
            try:
                params = parse_qs(parsed.query)
                lat = self._require_query_float(params, "lat")
                lon = self._require_query_float(params, "lon")
                limit = int(params.get("limit", ["8"])[0])
            except ValueError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
                return
            self._send_json({"candidates": self.manifest.nearest_arcs(lat, lon, max(1, min(limit, 20)))})
            return

        if parsed.path == "/api/search_roads":
            try:
                params = parse_qs(parsed.query)
                query = params.get("q", [""])[0]
                limit = int(params.get("limit", ["20"])[0])
            except ValueError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
                return
            self._send_json({"results": self.manifest.search_roads(query, max(1, min(limit, 50)))})
            return

        if parsed.path == "/api/arc":
            try:
                params = parse_qs(parsed.query)
                arc_id = int(params.get("arc_id", ["-1"])[0])
            except ValueError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
                return
            try:
                self._send_json(self.manifest.arc_summary(arc_id))
            except KeyError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.NOT_FOUND)
            return

        if parsed.path == "/api/propagation_preview":
            try:
                params = parse_qs(parsed.query)
                arc_id = int(params.get("arc_id", ["-1"])[0])
            except ValueError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
                return
            try:
                self._send_json(self.manifest.propagation_preview(arc_id))
            except KeyError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.NOT_FOUND)
            except RuntimeError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.INTERNAL_SERVER_ERROR)
            return

        super().do_GET()

    def do_POST(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)
        if parsed.path == "/api/import_yaml":
            try:
                payload = self._read_json()
                yaml_text = payload.get("yaml")
                if not isinstance(yaml_text, str):
                    raise ConfigValidationError("Request payload must include a string field 'yaml'")
                imported_state = parse_yaml_document(yaml_text, self.manifest)
            except ConfigValidationError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
                return
            except ValueError as exc:
                self._send_json({"error": f"Invalid JSON payload: {exc}"}, status=HTTPStatus.BAD_REQUEST)
                return

            self._send_json(imported_state)
            return

        if parsed.path == "/api/export_yaml":
            try:
                payload = self._read_json()
                yaml_text = build_yaml_document(payload)
            except ConfigValidationError as exc:
                self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_REQUEST)
                return
            except ValueError as exc:
                self._send_json({"error": f"Invalid JSON payload: {exc}"}, status=HTTPStatus.BAD_REQUEST)
                return

            self._send_json({"yaml": yaml_text})
            return

        self.send_error(HTTPStatus.NOT_FOUND)

    def log_message(self, fm: str, *args: Any) -> None:
        print(f"[camera-config-web] {self.address_string()} - {fm % args}")

    def _read_json(self) -> dict[str, Any]:
        content_length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(content_length)
        return json.loads(raw.decode("utf-8"))

    def _send_json(self, payload: Any, status: HTTPStatus = HTTPStatus.OK) -> None:
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    @staticmethod
    def _require_query_float(params: dict[str, list[str]], key: str) -> float:
        raw = params.get(key, [None])[0]
        if raw is None:
            raise ValueError(f"Missing query parameter '{key}'")
        value = float(raw)
        if not math.isfinite(value):
            raise ValueError(f"Query parameter '{key}' must be finite")
        return value


def make_handler(static_dir: Path, manifest: ManifestIndex):
    def factory(*args: Any, **kwargs: Any) -> CameraConfigHandler:
        return CameraConfigHandler(*args, directory=str(static_dir), manifest=manifest, **kwargs)

    return factory


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Local camera config web app")
    parser.add_argument(
        "--graph-dir",
        required=True,
        help="Graph directory or dataset root containing graph/road_arc_manifest.arrow",
    )
    parser.add_argument("--host", default=DEFAULT_HOST, help=f"Bind host (default: {DEFAULT_HOST})")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"Bind port (default: {DEFAULT_PORT})")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    root_dir = Path(__file__).resolve().parent
    static_dir = root_dir / "static"
    graph_dir = resolve_graph_dir(Path(args.graph_dir).resolve())
    manifest_path = graph_dir / "road_arc_manifest.arrow"
    manifest = ManifestIndex(manifest_path)

    handler = make_handler(static_dir, manifest)
    server = ThreadingHTTPServer((args.host, args.port), handler)
    print(f"[camera-config-web] graph_dir={graph_dir}")
    print(f"[camera-config-web] manifest={manifest_path}")
    print(f"[camera-config-web] listening on http://{args.host}:{args.port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n[camera-config-web] shutting down")
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
