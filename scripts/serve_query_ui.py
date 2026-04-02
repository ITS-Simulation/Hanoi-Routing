"""
Tiny dev server: serves query_ui.html AND proxies API calls to hanoi-server.
Eliminates CORS issues without requiring server rebuild.

Usage:
    python scripts/serve_query_ui.py                         # default backend localhost:8081
    python scripts/serve_query_ui.py --backend localhost:8082 # custom backend
    python scripts/serve_query_ui.py --port 8200              # custom local port
"""
import argparse
import http.server
import json
import os
import sys
import urllib.request
import urllib.error
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent


class ProxyHandler(http.server.SimpleHTTPRequestHandler):
    """Serves static files from SCRIPT_DIR; proxies /query, /info, /health, /ready to backend."""

    backend: str = "http://localhost:8081"

    PROXY_PATHS = {"/query", "/info", "/health", "/ready"}

    def end_headers(self):
        # Inject CORS headers on every response so cross-origin requests work
        # (e.g. when the HTML is opened directly via file:// or from another port)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        super().end_headers()

    def _should_proxy(self):
        path = self.path.split("?")[0]
        return path in self.PROXY_PATHS

    def _proxy(self, method):
        qs = ""
        if "?" in self.path:
            qs = "?" + self.path.split("?", 1)[1]
        path = self.path.split("?")[0]
        target = self.backend + path + qs

        # Read request body for POST
        body = None
        if method == "POST":
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length) if length else None

        req = urllib.request.Request(target, data=body, method=method)
        ct = self.headers.get("Content-Type")
        if ct:
            req.add_header("Content-Type", ct)

        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                resp_body = resp.read()
                self.send_response(resp.status)
                for h in ("Content-Type", "Content-Length"):
                    v = resp.getheader(h)
                    if v:
                        self.send_header(h, v)
                self.end_headers()
                self.wfile.write(resp_body)
        except urllib.error.HTTPError as e:
            self.send_response(e.code)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(e.read())
        except urllib.error.URLError as e:
            self.send_response(502)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            reason = str(e.reason)
            if "timed out" in reason:
                msg = f"Backend timeout (120s) — server may be hung: {reason}"
            elif "refused" in reason:
                msg = f"Backend not running at {self.backend}: {reason}"
            else:
                msg = f"Backend unreachable ({self.backend}): {reason}"
            self.wfile.write(json.dumps({"error": msg}).encode())
        except Exception as e:
            self.send_response(502)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"error": f"Proxy error: {e}"}).encode())

    def do_GET(self):
        if self._should_proxy():
            return self._proxy("GET")
        # Serve static files from SCRIPT_DIR
        try:
            return super().do_GET()
        except BrokenPipeError:
            pass  # client disconnected mid-transfer

    def do_POST(self):
        if self._should_proxy():
            return self._proxy("POST")
        self.send_response(404)
        self.end_headers()

    def do_OPTIONS(self):
        # Handle preflight
        self.send_response(204)
        self.end_headers()

    def translate_path(self, path):
        """Override to serve from SCRIPT_DIR instead of cwd."""
        p = path.split("?")[0].split("#")[0]
        if p == "/" or p == "":
            p = "/query_ui.html"
        return str(SCRIPT_DIR / p.lstrip("/"))

    def log_message(self, format, *args):
        proxy_tag = " [proxy]" if self._should_proxy() else ""
        sys.stderr.write(f"  {self.address_string()} - {format % args}{proxy_tag}\n")


class QuietHTTPServer(http.server.HTTPServer):
    """Suppress noisy BrokenPipeError tracebacks from the server layer."""

    def handle_error(self, request, client_address):
        exc = sys.exc_info()[1]
        if isinstance(exc, BrokenPipeError):
            return
        super().handle_error(request, client_address)


def main():
    parser = argparse.ArgumentParser(description="Dev server for query_ui.html with API proxy")
    parser.add_argument("--port", type=int, default=8200, help="Local port (default: 8200)")
    parser.add_argument("--backend", default="http://localhost:8081", help="Backend server URL")
    args = parser.parse_args()

    ProxyHandler.backend = args.backend.rstrip("/")

    os.chdir(SCRIPT_DIR)
    server = QuietHTTPServer(("127.0.0.1", args.port), ProxyHandler)

    print(f"╔══════════════════════════════════════════╗")
    print(f"║  CCH-Hanoi Query UI                      ║")
    print(f"╠══════════════════════════════════════════╣")
    print(f"║  UI:      http://localhost:{args.port:<14}║")
    print(f"║  Backend: {ProxyHandler.backend:<31}║")
    print(f"╠══════════════════════════════════════════╣")
    print(f"║  Press Ctrl+C to stop                    ║")
    print(f"╚══════════════════════════════════════════╝")

    # Auto-open browser
    import webbrowser
    webbrowser.open(f"http://localhost:{args.port}")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nStopped.")
        server.server_close()


if __name__ == "__main__":
    main()
