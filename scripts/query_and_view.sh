#!/usr/bin/env bash
set -euo pipefail

# query_and_view.sh — Chạy truy vấn CCH-Hanoi rồi tự động mở kết quả trên bản đồ
#
# Sử dụng:
#   bash scripts/query_and_view.sh --from-lat 21.0283 --from-lng 105.8542 \
#                                  --to-lat 20.998  --to-lng 105.829
#
# Tất cả đối số được truyền thẳng cho `cch-hanoi query`. Script tự thêm:
#   --data-dir, --line-graph, --output-format geojson, --demo, --output-file
#
# Biến môi trường tùy chỉnh:
#   PROFILE     — motorcycle (mặc định) hoặc car
#   DATA_DIR    — ghi đè thư mục dữ liệu (bỏ qua PROFILE)
#   NO_VIEW     — đặt =1 để chỉ query, không mở visualizer
#   CLI_BIN     — đường dẫn tới binary cch-hanoi

REPO=/mnt/c/ITS/Routing/Hanoi-Routing
PROFILE="${PROFILE:-motorcycle}"
DATA_DIR="${DATA_DIR:-$REPO/Maps/data/hanoi_$PROFILE}"
CLI_BIN="${CLI_BIN:-$REPO/CCH-Hanoi/target/release/cch-hanoi}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VISUALIZER="$SCRIPT_DIR/visualize_route.sh"

export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"
source ~/.cargo/env 2>/dev/null || true

# Generate output file name
TIMESTAMP=$(date +%Y-%m-%dT%H%M%S)
OUTPUT_FILE="$REPO/query_${TIMESTAMP}.geojson"

echo "=== CCH-Hanoi Query + Visualize ==="
echo "Profile:  $PROFILE"
echo "Data dir: $DATA_DIR"
echo ""

# Run query — pass through all user args, add our defaults
"$CLI_BIN" query \
    --data-dir "$DATA_DIR" \
    --line-graph \
    --output-format geojson \
    --demo \
    --output-file "$OUTPUT_FILE" \
    "$@"

QUERY_EXIT=$?
if [ $QUERY_EXIT -ne 0 ]; then
    echo "ERROR: query failed (exit $QUERY_EXIT)" >&2
    exit $QUERY_EXIT
fi

echo ""
echo "Output: $OUTPUT_FILE"

# Open visualizer unless NO_VIEW=1
if [ "${NO_VIEW:-0}" = "1" ]; then
    echo "Skipping visualization (NO_VIEW=1)"
else
    echo "Opening visualizer..."
    bash "$VISUALIZER" "$OUTPUT_FILE"
fi
