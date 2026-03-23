# hanoi-server

## Architecture

`hanoi-server` exposes two ports:

| Port | Endpoint group | Purpose |
| --- | --- | --- |
| `8080` | `/query`, `/info`, `/health`, `/ready` | Query and status API |
| `9080` | `/customize` | Weight upload / re-customization API |

## `/customize` behavior

`POST /customize` returns `200 OK` before customization completes. The handler
only validates the binary payload and queues the weight vector into the
`watch::channel`:

- `handlers.rs`: `let _ = state.watch_tx.send(Some(weights))`

The background engine thread reads from the watch channel at the top of its
loop and sets `customization_active = true` while re-customizing. This means
the HTTP response confirms acceptance, not completion.

## Watch-channel semantics

The engine loop uses `borrow_and_update()` on the watch receiver, so it always
observes the latest queued weight vector. If `/customize` is called twice in
rapid succession while a customization is already underway, the earlier weight
vector may be silently dropped and replaced by the newer one.

This is intentional for live-traffic updates: the routing engine should move to
the freshest weights rather than replay stale intermediate states.

## Race-condition mitigation strategies

1. Poll `/info` after `POST /customize` and watch `customization_active`.
   Use exponential backoff starting at 10ms and capping at 500ms. A transition
   from `true` to `false` signals completion.
2. Use a fixed sleep of 100-200ms after `/customize` returns.
   `hanoi-bench/src/server.rs` currently uses `tokio::time::sleep(Duration::from_millis(100))`.
   This is acceptable for benchmarks but not reliable under production load.
3. Add a future completion signal.
   A dedicated `GET /customize/status` endpoint or a WebSocket push channel
   would remove the need for polling.

Example polling flow:

```text
POST /customize  ->  {"accepted": true}
GET  /info       ->  {"customization_active": true}   (wait 10ms)
GET  /info       ->  {"customization_active": false}  (done)
```
