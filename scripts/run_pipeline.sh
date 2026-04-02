#!/usr/bin/env bash
set -euo pipefail

REPO=/mnt/c/ITS/Routing/Hanoi-Routing
MAP_SOURCE="${1:-hanoi.osm.pbf}"
PROFILE="${2:-motorcycle}"

INPUT_PBF="$REPO/Maps/$MAP_SOURCE"
MAP_BASENAME="$(basename "$MAP_SOURCE")"
MAP_NAME="${MAP_BASENAME%.osm.pbf}"
OUTPUT_DIR="$REPO/Maps/data/${MAP_NAME}_$PROFILE"
GRAPH_DIR="$OUTPUT_DIR/graph"

CCH_GEN="$REPO/CCH-Generator/lib/cch_generator"
VALIDATOR="$REPO/CCH-Generator/lib/validate_graph"
COND_EXTRACT="$REPO/RoutingKit/bin/conditional_turn_extract"
LINE_GRAPH_GEN="$REPO/CCH-Hanoi/target/release/generate_line_graph"

export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"
source ~/.cargo/env 2>/dev/null || true

if [ ! -f "$INPUT_PBF" ]; then
    echo "Error: Input PBF not found: $INPUT_PBF"
    exit 1
fi

SECONDS=0

echo "=== Phase 1/5: Generate graph ==="
mkdir -p "$GRAPH_DIR"
"$CCH_GEN" "$INPUT_PBF" "$GRAPH_DIR" --profile "$PROFILE"
"$VALIDATOR" "$GRAPH_DIR"
echo "Phase 1 done (${SECONDS}s elapsed)"

echo ""
echo "=== Phase 2/5: Extract conditional turns ==="
"$COND_EXTRACT" "$INPUT_PBF" "$GRAPH_DIR" "$OUTPUT_DIR" --profile "$PROFILE"
"$VALIDATOR" "$GRAPH_DIR"
echo "Phase 2 done (${SECONDS}s elapsed)"

echo ""
echo "=== Phase 3/5: Generate line graph ==="
"$LINE_GRAPH_GEN" "$GRAPH_DIR" "$OUTPUT_DIR/line_graph"
"$VALIDATOR" "$GRAPH_DIR" --turn-expanded "$OUTPUT_DIR/line_graph"
echo "Phase 3 done (${SECONDS}s elapsed)"

echo ""
echo "=== Phase 4/5: CCH ordering for primary graph ==="
bash "$REPO/rust_road_router/flow_cutter_cch_order.sh" "$GRAPH_DIR"
bash "$REPO/rust_road_router/flow_cutter_cch_cut_order.sh" "$GRAPH_DIR"
bash "$REPO/rust_road_router/flow_cutter_cch_cut_reorder.sh" "$GRAPH_DIR"
echo "Phase 4 done (${SECONDS}s elapsed)"

echo ""
echo "=== Phase 5/5: CCH ordering for line graph ==="
bash "$REPO/rust_road_router/flow_cutter_cch_order.sh" "$OUTPUT_DIR/line_graph"
echo "Phase 5 done (${SECONDS}s elapsed)"

echo ""
echo "========================================="
echo " Pipeline completed in ${SECONDS}s"
echo " Output: $OUTPUT_DIR"
echo "========================================="
