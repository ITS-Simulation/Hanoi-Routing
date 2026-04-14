# hanoi-gateway

Profile-based API gateway for Hanoi routing servers. Routes requests to the
correct `hanoi-server` backend based on a `profile` query parameter
(e.g. `car`, `motorcycle`).

## Architecture

```
                     ┌─────────────────────────────┐
   HTTP clients ───▶ │  gateway (:50051)            │
                     │    POST /query?profile=...   │──▶ hanoi_server (car)     :8080
                     │    POST /reset_weights?...   │──▶ hanoi_server (car)     :8080
                     │    GET  /info?profile=...    │──▶ hanoi_server (moto)    :8081
                     │    GET  /status?profile=...  │
                     │    GET  /health?profile=...  │
                     │    GET  /ready?profile=...   │
                     │    GET  /profiles            │    ...
                     └─────────────────────────────┘
```

The gateway is a stateless reverse proxy. It does **not** parse or transform
request/response bodies — it forwards them verbatim to the backend selected by
`profile`. All backend configuration lives in a single YAML file.

## CLI

```
hanoi_gateway --config gateway.yaml [--port 50051]
```

| Option | Default | Description |
| --- | --- | --- |
| `--config` | `gateway.yaml` | Path to YAML config file |
| `--port` | *(from config)* | Override the listen port |

## Configuration

```yaml
port: 50051
backend_timeout_secs: 30    # 0 to disable; default 30
log_format: pretty          # pretty | full | compact | tree | json
# log_file: /var/log/gw.json

profiles:
  car:
    backend_url: "http://localhost:8080"
  motorcycle:
    backend_url: "http://localhost:8081"
```

The `profiles` map is the single source of truth for which routing profiles
the gateway exposes. Requests with an unknown profile are rejected with
HTTP 400 and a list of valid options.

## API reference

### `POST /query?profile=<name>`

Forward a route query to the selected backend. The JSON body is forwarded
unchanged.

**Query parameters:**

| Param | Required | Description |
| --- | --- | --- |
| `profile` | **yes** | Routing profile (must match a config key) |
| `format` | no | Forwarded to backend (`json` for plain JSON) |
| `colors` | no | Forwarded to backend (enables simplestyle-spec) |
| `alternatives` | no | Forwarded to backend to request alternative routes |
| `stretch` | no | Forwarded to backend to control alternative-route stretch |

**Example:**
```bash
curl -X POST "http://localhost:50051/query?profile=car" \
  -H "Content-Type: application/json" \
  -d '{"from_lat":21.028,"from_lng":105.854,"to_lat":21.007,"to_lng":105.820}'
```

**Errors:**

| Status | Cause |
| --- | --- |
| 400 | Unknown or missing `profile` |
| 502 | Backend unreachable or returned invalid JSON |

### `GET /info?profile=<name>`

Forward a metadata query to the selected backend. When `profile` is omitted,
defaults to the first profile alphabetically.

### `GET /status?profile=<name>`

Gateway alias for `GET /info?profile=<name>`. This is helpful when clients
think of `/info` as a status endpoint.

### `GET /health?profile=<name>`

Forward the selected backend's health response. When `profile` is omitted,
defaults to the first profile alphabetically.

### `GET /ready?profile=<name>`

Forward the selected backend's readiness response. When `profile` is omitted,
defaults to the first profile alphabetically.

### `POST /reset_weights?profile=<name>`

Forward a baseline-weight reset request to the selected backend.

### `GET /profiles`

List all available routing profiles.

```json
{ "profiles": ["car", "motorcycle"] }
```

## Build

```bash
cargo build --release -p hanoi-gateway
```
