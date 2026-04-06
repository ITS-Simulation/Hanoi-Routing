#!/usr/bin/env bash
set -euo pipefail

REPO=C:/ITS/Routing/Hanoi-Routing
PROFILE="${1:-motorcycle}"
DATA_DIR="$REPO/Maps/data/hanoi_$PROFILE"
GRAPH_DIR="$DATA_DIR/graph"
LINE_GRAPH_DIR="$DATA_DIR/line_graph"
SERVER_BIN="$REPO/CCH-Hanoi/target/release/hanoi_server.exe"

export LD_LIBRARY_PATH="$REPO/RoutingKit/lib:${LD_LIBRARY_PATH:-}"

echo "Starting hanoi_server (line-graph mode, profile=$PROFILE)..."
exec "$SERVER_BIN" \
    --graph-dir "$LINE_GRAPH_DIR" \
    --original-graph-dir "$GRAPH_DIR" \
    --line-graph \
    --query-port 8081 \
    --customize-port 9081 \
    --log-format pretty
