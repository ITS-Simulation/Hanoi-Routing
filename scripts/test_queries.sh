#!/usr/bin/env bash
set -euo pipefail

# Test script for CCH-Hanoi routing: runs CLI queries, server queries, and benchmarks
# Usage: bash scripts/test_queries.sh [motorcycle|car]

REPO=/mnt/c/ITS/Routing/Hanoi-Routing
PROFILE="${1:-motorcycle}"
DATA_DIR="$REPO/Maps/data/hanoi_$PROFILE"
GRAPH_DIR="$DATA_DIR/graph"
LINE_GRAPH_DIR="$DATA_DIR/line_graph"
CLI_BIN="$REPO/CCH-Hanoi/target/release/cch-hanoi"
BENCH_CORE="$REPO/CCH-Hanoi/target/release/bench_core"
BENCH_SERVER="$REPO/CCH-Hanoi/target/release/bench_server"
RESULTS_DIR="$DATA_DIR/test_results"
QUERY_PORT=8081

export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"
source ~/.cargo/env 2>/dev/null || true

mkdir -p "$RESULTS_DIR"

passed=0
failed=0
total=0

run_test() {
    local name="$1"
    shift
    total=$((total + 1))
    echo ""
    echo "--- Test $total: $name ---"
    if "$@"; then
        passed=$((passed + 1))
        echo "  PASS"
    else
        failed=$((failed + 1))
        echo "  FAIL (exit code: $?)"
    fi
}

echo "========================================="
echo " CCH-Hanoi Test Suite -- $PROFILE profile"
echo "========================================="
echo "Data dir: $DATA_DIR"
echo ""

# --- Phase 1: CLI Tests (offline, no server needed) ---

echo "=== Phase 1: CLI Tests (line-graph mode) ==="

run_test "CLI info" \
    "$CLI_BIN" info --data-dir "$DATA_DIR"

run_test "CLI: long route (center to south)" \
    "$CLI_BIN" query \
        --data-dir "$DATA_DIR" --line-graph \
        --from-lat 21.03835 --from-lng 105.78310 \
        --to-lat 20.887784 --to-lng 105.775691 \
        --output-format geojson --demo \
        --output-file "$RESULTS_DIR/cli_long_route.geojson"

run_test "CLI: short route (Old Quarter)" \
    "$CLI_BIN" query \
        --data-dir "$DATA_DIR" --line-graph \
        --from-lat 21.03389 --from-lng 105.85127 \
        --to-lat 21.03160 --to-lng 105.85263 \
        --output-format geojson \
        --output-file "$RESULTS_DIR/cli_short_route.geojson"

run_test "CLI: cross-city (west to east)" \
    "$CLI_BIN" query \
        --data-dir "$DATA_DIR" --line-graph \
        --from-lat 21.02940 --from-lng 105.75407 \
        --to-lat 21.01320 --to-lng 105.94399 \
        --output-format geojson \
        --output-file "$RESULTS_DIR/cli_cross_city.geojson"

run_test "CLI: reverse direction (south to north)" \
    "$CLI_BIN" query \
        --data-dir "$DATA_DIR" --line-graph \
        --from-lat 20.887784 --from-lng 105.775691 \
        --to-lat 21.03835 --to-lng 105.78310 \
        --output-format json \
        --output-file "$RESULTS_DIR/cli_reverse.json"

run_test "CLI: JSON output" \
    "$CLI_BIN" query \
        --data-dir "$DATA_DIR" --line-graph \
        --from-lat 21.07692 --from-lng 105.81305 \
        --to-lat 20.989024 --to-lng 105.852828 \
        --output-format json \
        --output-file "$RESULTS_DIR/cli_json_output.json"

# --- Phase 2: Server Tests (requires running server on port 8081) ---

echo ""
echo "=== Phase 2: Server Tests ==="

if ! curl -s "http://localhost:$QUERY_PORT/health" > /dev/null 2>&1; then
    echo "Server not running on port $QUERY_PORT, skipping server tests"
else
    run_test "Server: health check" \
        curl -sf "http://localhost:$QUERY_PORT/health" -o "$RESULTS_DIR/health.json"

    run_test "Server: info" \
        curl -sf "http://localhost:$QUERY_PORT/info" -o "$RESULTS_DIR/info.json"

    run_test "Server: ready" \
        curl -sf "http://localhost:$QUERY_PORT/ready" -o "$RESULTS_DIR/ready.json"

    run_test "Server: coordinate query (long)" \
        curl -sf "http://localhost:$QUERY_PORT/query?colors" \
            -H "Content-Type: application/json" \
            -d "{\"from_lat\":21.03835,\"from_lng\":105.78310,\"to_lat\":20.887784,\"to_lng\":105.775691}" \
            -o "$RESULTS_DIR/server_long.geojson"

    run_test "Server: short route (Old Quarter)" \
        curl -sf "http://localhost:$QUERY_PORT/query" \
            -H "Content-Type: application/json" \
            -d "{\"from_lat\":21.03389,\"from_lng\":105.85127,\"to_lat\":21.03160,\"to_lng\":105.85263}" \
            -o "$RESULTS_DIR/server_short.json"

    run_test "Server: cross-city (west to east)" \
        curl -sf "http://localhost:$QUERY_PORT/query?colors" \
            -H "Content-Type: application/json" \
            -d "{\"from_lat\":21.02940,\"from_lng\":105.75407,\"to_lat\":21.01320,\"to_lng\":105.94399}" \
            -o "$RESULTS_DIR/server_cross_city.geojson"

    run_test "Server: same origin and dest" \
        curl -sf "http://localhost:$QUERY_PORT/query" \
            -H "Content-Type: application/json" \
            -d "{\"from_lat\":21.03835,\"from_lng\":105.78310,\"to_lat\":21.03835,\"to_lng\":105.78310}" \
            -o "$RESULTS_DIR/server_same_point.json"

    # Out-of-boundary test (expect 400)
    echo ""
    echo "--- Test $((total + 1)): Server: out-of-boundary (expect 400) ---"
    total=$((total + 1))
    HTTP_CODE=$(curl -s -o "$RESULTS_DIR/server_oob.json" -w "%{http_code}" \
        "http://localhost:$QUERY_PORT/query" \
        -H "Content-Type: application/json" \
        -d "{\"from_lat\":10.0,\"from_lng\":100.0,\"to_lat\":11.0,\"to_lng\":101.0}")
    if [ "$HTTP_CODE" = "400" ]; then
        passed=$((passed + 1))
        echo "  PASS (got expected 400)"
    else
        failed=$((failed + 1))
        echo "  FAIL (expected 400, got $HTTP_CODE)"
    fi
fi

# --- Phase 3: Benchmark Tests ---

echo ""
echo "=== Phase 3: Benchmark Tests ==="

run_test "Bench: generate 100 random queries" \
    "$BENCH_CORE" \
        --graph-dir "$GRAPH_DIR" \
        --generate-queries 100 \
        --save-queries "$RESULTS_DIR/bench_queries.json"

run_test "Bench: core benchmark (100 queries, 3 iterations)" \
    "$BENCH_CORE" \
        --graph-dir "$GRAPH_DIR" \
        --queries "$RESULTS_DIR/bench_queries.json" \
        --iterations 3 \
        --output "$RESULTS_DIR/bench_core_results.json"

if curl -s "http://localhost:$QUERY_PORT/health" > /dev/null 2>&1; then
    run_test "Bench: server benchmark (100 queries)" \
        "$BENCH_SERVER" \
            --url "http://localhost:$QUERY_PORT" \
            --query-file "$RESULTS_DIR/bench_queries.json" \
            --queries 100
fi

# --- Summary ---

echo ""
echo "========================================="
echo " Test Results: $passed/$total passed"
if [ "$failed" -gt 0 ]; then
    echo " $failed test(s) FAILED"
else
    echo " All tests PASSED"
fi
echo " Results saved to: $RESULTS_DIR/"
echo "========================================="

[ "$failed" -eq 0 ]
