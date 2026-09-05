# Telemetry — what `rezureapp` actually sends

This documents the client side of telemetry: what gets recorded locally, when, and what
goes out over the wire. The canonical wire contract — request/response shapes, auth,
rate limits, idempotency — lives outside this repo at
`api-documentation/telemetry-api.md` (shared with `laravel-api`, since both sides must
agree on it). If the two disagree, that doc wins; update this one to match.

## The setting

Usage data is **on by default**, and there is no switch for it in the UI. The setting still
exists and is still honoured — it lives in `settings.json` (`%REZURE_HOME%\etc\settings.json`,
normally `C:\rezure\etc\settings.json`):

```json
{ "shareUsageData": false }
```

Set that and restart, and nothing is recorded or sent. A file that already says `false` keeps
saying it: the default applies only where the key is absent, so an opt-out made while the UI
still had a toggle is never silently reversed.

## Recording vs. sending

Recording and sending are two separate steps, and each independently respects
`shareUsageData`:

1. **Recording** — `services::telemetry::TelemetryClient::record_event` /
   `record_heartbeat` (`src-tauri/src/services/telemetry.rs`) serialize a payload and
   insert it into the local `pending_events` SQLite table
   (`src-tauri/src/db/telemetry.rs`). If the setting is off, these no-op immediately —
   nothing is written, not even locally.
2. **Sending** — `services::telemetry::send_pending` runs on a 60-second timer
   (`src-tauri/src/lib.rs`'s `setup()`) and drains `pending_events`. If the setting is off
   *at send time* — even for rows queued earlier while it was on — it returns
   immediately and sends nothing. Turning it off always means "stop", not "finish
   what's already queued".

## What's recorded, and when

| Trigger | Kind | Fires from |
|---|---|---|
| App finishes starting (`db::init()` succeeds) | event, `app_opened` | `lib.rs` `setup()` |
| A service is started via the Services page | event, `service.start` (name = the service, e.g. `nginx`) | `commands::services::start_service` |
| A service is stopped via the Services page | event, `service.stop` | `commands::services::stop_service` |
| Every 5 minutes while the app is open (plus once immediately) | heartbeat | `lib.rs`'s heartbeat-recorder loop |
| The app is quitting (`ExitRequested`), if a session is open | heartbeat, with `ended_at` set | `lib.rs`'s `app.run(...)` closure |

`force_stop_service` and `restart_service` are **not** instrumented — only the two
actions above, matching the v2 roadmap's Fase 2.4 scope.

## Payload shapes (as actually serialized)

**Event** (`EventPayload` in `services/telemetry.rs`):

```json
{
  "device_id": "…",
  "event_id": "…",
  "event_type": "app_opened | service.start | service.stop",
  "event_name": "nginx | null",
  "app_version": "1.0.0",
  "occurred_at": "2026-09-02T06:00:00+00:00"
}
```

**Heartbeat** (`HeartbeatPayload` in `services/telemetry.rs`):

```json
{
  "device_id": "…",
  "session_id": "…",
  "app_version": "1.0.0",
  "os": "Windows 11 Home Single Language",
  "os_version": "11 (26200)",
  "occurred_at": "2026-09-02T06:00:00+00:00",
  "ended_at": null
}
```

`occurred_at` is stamped at *record* time, not send time — a row queued while offline
and sent later still reports when it actually happened. `os`/`os_version` come from
`sysinfo::System::long_os_version()` / `os_version()`.

`session_id` is one UUID generated once per launch (`services::telemetry::SessionIdState`,
in-memory only, never persisted) and reused on every heartbeat for that run.

## Sending

`send_pending` (`services/telemetry.rs`) processes up to 20 unsent rows per tick, oldest
first, POSTing each one individually to `{base_url}/api/v1/telemetry/event` or
`.../heartbeat` depending on its stored `type` — the real backend has no bulk endpoint,
so "batch" here means "several requests per wake-up", not one combined request. On
success the row's `sent_at` is set; on any failure it's left alone and retried on the
next tick (the queue's whole retry strategy — no explicit backoff timer). A `429`
response stops the rest of that tick's batch early. Rows already sent are deleted after
7 days (`db::telemetry::delete_sent_before`) — `pending_events` is a bounded local queue,
not a permanent log.

Every failure in this path is `log::warn!`-only. It never surfaces to the UI and never
blocks a user-initiated action — this is the "kegagalan pengiriman tidak boleh
mengganggu user" requirement from Fase 2.5.

## What's deliberately *not* sent

- No file paths, project names, database names, or anything else from the user's local
  filesystem/config beyond the fields listed above.
- `event_name` is limited to the service id (`nginx`, `php`, `mariadb`) or omitted — never
  free text a user typed (that's what support tickets are for, a separate, explicit,
  user-initiated action documented in `docs/v2/rezure-app-v2-phases-tasks.md`'s Fase 2.1).
