# Client Protocol Contract

**Interface**: Server ↔ Client (HTTP + SSE)

**Status**: authoritative for the MVP. Replaces the provisional schema
that lived in the archived TypeScript-engine spec; this document is the
single source of truth for the wire protocol implemented by `sim-server`
(PRD §10).

## Purpose

The server is authoritative for the simulation. It precomputes a full
match up front and serves it to a separate frontend over HTTP +
Server-Sent Events (SSE). The client never corrects server state; it
interpolates between frames and renders events as they arrive in the
stream.

## Security (PRD §4)

### `POST /match/{match_id}` — bearer auth

When `api_token` is configured, `POST` requires
`Authorization: Bearer <token>`. Missing or invalid tokens return
**401**. `GET` is never authenticated: browser `EventSource` cannot set
headers, so auth is placed on `POST` only.

Auth and CORS can be disabled for local-only deployments: leave
`api_token` unset to disable auth, and leave `cors_origins` empty to
disable CORS (the default bind is `127.0.0.1`, so same-origin and
direct clients keep working).

### `GET /match/{match_id}/stream` — CORS

`GET` responses carry `Access-Control-Allow-Origin: <origin>` only when
the request `Origin` is in the configured `cors_origins` allowlist. The
default allowlist is empty (`[]`), which disables CORS. `OPTIONS`
preflight is answered with matching CORS headers; an unlisted origin
gets no `Access-Control-Allow-Origin`, so browsers block it.

## Endpoints

### `POST /match/{match_id}`

Starts the match on a dedicated OS thread (never on the async
executor) if it has not already been started; never starts a second
computation for the same id.

| Status | Meaning |
|--------|---------|
| 202    | Simulation started now |
| 200    | Already running or finished (cached status served) |
| 401    | Missing or invalid bearer token (only when `api_token` is set) |
| 404    | Unknown `match_id` |
| 503    | Concurrency cap reached; `Retry-After` header set |

Body: `{ "match_id": "...", "status": "running" \| "finished" \| "failed" }`.

### `GET /match/{match_id}/stream` (`text/event-stream`)

Serves the precomputed SSE stream (gzip optional behind a reverse
proxy). The whole match is simulated up front and sent instantly in
order; resumability is via `Last-Event-ID`.

| Status | Meaning |
|--------|---------|
| 200    | Stream. If the simulation is still running, the connection stays open (keep-alive comment every 15 s) until the first message is ready |
| 204    | `Last-Event-ID` is at or beyond the final message |
| 404    | Unknown `match_id` |
| 409    | Match not started; call `POST` first |
| 410    | Result expired (TTL); `POST` again to regenerate |

## SSE messages

The SSE `id` is the message index; the client sends it back as
`Last-Event-ID` to resume after message *n*. Frames carry no
wall-clock timestamps, so re-simulating the same fixture produces a
byte-identical uncompressed body (PRD §10.4).

| id | `event:` | Content |
|----|----------|---------|
| 0 | `manifest` | Match metadata: `match_id`, `seed`, teams, formations, setups, player roster (`player_id`, name, number, position), pitch dimensions and unit, tick rates, attack direction per team per half, and the exact `from_tick`/`to_tick` of all 10 segments |
| 1–10 | `segment` | `{ segment, half, from_minute, to_minute, from_tick, to_tick, events[], frames[] }` |
| 11 | `full_time` | `{ score, event_count, state_hash }` |
| — | `error` | Replaces the remaining messages on failure: `{ code, message, last_good_tick }` |

A successful match always streams as **1 manifest + 10 segments +
1 `full_time`** (12 messages). A failed match streams a single **id 0
`error`** message that replaces the remaining messages.

### Failure event shape

On a PRD §12 invariant violation or a `tick_cap` breach, the match
status becomes `failed`, the stream sends one `error` event and closes,
and the failure is cached like a success so retries are deterministic:

```json
{
  "code": "tick_cap_exceeded" | "<invariant code>",
  "message": "<human-readable detail>",
  "last_good_tick": 120
}
```

## Segmentation (PRD §10.3)

Segments never cross a half boundary. Each half has exactly 5 segments:

- First half: `0–10`, `10–20`, `20–30`, `30–40`, **`40–45+added`**.
- Second half: `45–55`, `55–65`, `65–75`, `75–85`, **`85–90+added`**.

The last segment of each half runs to the end of that half including
its added time. Message id = `segment + 1`, so segments are ids 1–10.

## Resumability (`Last-Event-ID`)

The client sends the last message `id` it received as
`Last-Event-ID`; the server resumes after message *n* (starts at
message *n* + 1). Reconnecting without `Last-Event-ID` replays from
message 0. If the resume id is at or beyond the final message, the
server returns **204**.

## Status-code summary

`POST`: 202 / 200 / 401 / 404 / 503.
`GET`: 200 / 204 / 404 / 409 / 410.
Failures are emitted as a single `error` SSE message (`{ code,
message, last_good_tick }`) and cached per PRD §10.4.
