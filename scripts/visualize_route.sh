#!/usr/bin/env bash
set -euo pipefail

# visualize_route.sh — Mở bản đồ trực quan hóa kết quả truy vấn route
# Sử dụng:
#   bash scripts/visualize_route.sh                          # Mở visualizer rỗng
#   bash scripts/visualize_route.sh result.geojson           # Mở + tự load file
#   bash scripts/visualize_route.sh *.geojson                # Mở + load nhiều file

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
HTML="$SCRIPT_DIR/visualize_route.html"

if [ ! -f "$HTML" ]; then
    echo "ERROR: visualize_route.html not found at $HTML" >&2
    exit 1
fi

# If files are provided, start a tiny HTTP server so fetch() works
if [ $# -gt 0 ]; then
    # Pick a random port in 8100-8199
    PORT=$((8100 + RANDOM % 100))

    # Copy geojson files to a temp dir alongside the HTML
    TMPDIR=$(mktemp -d)
    cp "$HTML" "$TMPDIR/visualize_route.html"

    FILE_LIST=""
    for f in "$@"; do
        if [ -f "$f" ]; then
            cp "$f" "$TMPDIR/"
            BASENAME=$(basename "$f")
            FILE_LIST="${FILE_LIST}${BASENAME},"
        else
            echo "WARNING: skipping $f (not found)" >&2
        fi
    done

    # Inject auto-load script for multiple files
    FILE_LIST="${FILE_LIST%,}"  # trim trailing comma
    {
        head -n -1 "$TMPDIR/visualize_route.html"
        cat <<INJECT_EOF
<script>
(function() {
  var files = '${FILE_LIST}'.split(',');
  files.forEach(function(f) {
    if (!f) return;
    fetch(f)
      .then(function(r) { return r.json(); })
      .then(function(geojson) { addRoute(geojson, f); })
      .catch(function(e) { console.warn('Auto-load failed:', f, e); });
  });
})();
</script>
</body>
</html>
INJECT_EOF
    } > "$TMPDIR/visualize_route_tmp.html"
    mv "$TMPDIR/visualize_route_tmp.html" "$TMPDIR/visualize_route.html"

    echo "Starting HTTP server on port $PORT ..."
    echo "Open: http://localhost:$PORT/visualize_route.html"
    echo "Press Ctrl+C to stop."
    cd "$TMPDIR"
    python3 -m http.server "$PORT" --bind 127.0.0.1 2>/dev/null &
    SERVER_PID=$!
    trap "kill $SERVER_PID 2>/dev/null; rm -rf $TMPDIR" EXIT

    # Try to open browser
    if command -v xdg-open &>/dev/null; then
        xdg-open "http://localhost:$PORT/visualize_route.html"
    elif command -v open &>/dev/null; then
        open "http://localhost:$PORT/visualize_route.html"
    elif command -v wslview &>/dev/null; then
        wslview "http://localhost:$PORT/visualize_route.html"
    else
        # Windows from WSL
        cmd.exe /c start "http://localhost:$PORT/visualize_route.html" 2>/dev/null || true
    fi

    wait $SERVER_PID
else
    # No files — just open the HTML directly
    if command -v xdg-open &>/dev/null; then
        xdg-open "$HTML"
    elif command -v open &>/dev/null; then
        open "$HTML"
    elif command -v wslview &>/dev/null; then
        wslview "$HTML"
    else
        cmd.exe /c start "" "$(wslpath -w "$HTML" 2>/dev/null || echo "$HTML")" 2>/dev/null || true
    fi
    echo "Opened visualize_route.html"
    echo "Drag & drop .geojson files onto the map to view routes."
fi
