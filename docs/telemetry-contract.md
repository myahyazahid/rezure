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
| A service is started via the Services page | event, `service.start` (name = the service's display name, e.g. `Nginx`, `MariaDB`), with the PHP/database versions in `payload` | `commands::services::start_service` |
| A service is stopped via the Services page | event, `service.stop` | `commands::services::stop_service` |
| Starting, stopping or restarting a service fails | event, `error.report` (name = the error's kind, e.g. `PortInUse`) | `commands::services::{start,stop,restart}_service` |
| Every worker of a service dies on its own | event, `error.report` (name = `ServiceCrashed`) | the crash sink in `services::process::real_services` |
| Every 5 minutes while the app is open (plus once immediately) | heartbeat | `lib.rs`'s heartbeat-recorder loop |
| The app is quitting (`ExitRequested`), if a session is open | heartbeat, with `ended_at` set | `lib.rs`'s `app.run(...)` closure |

`force_stop_service` isn't instrumented, and a successful `restart_service` isn't either:
only its failures are reported.

## Payload shapes (as actually serialized)

**Event** (`EventPayload` in `services/telemetry.rs`):

```json
{
  "device_id": "…",
  "event_id": "…",
  "event_type": "app_opened | service.start | service.stop | error.report",
  "event_name": "Nginx | PortInUse | … | null",
  "app_version": "1.0.0",
  "payload": { "…": "…" },
  "occurred_at": "2026-09-02T06:00:00+00:00"
}
```

`payload` is left off entirely when an event has none (`app_opened`, `service.stop`).
When it's there, it's one of:

- **`service.start`**: the stack in use, which the dashboard's "top stack combos" chart
  pairs up: `{ "php_version": "8.3.33", "mariadb_version": "MariaDB 11.8.9" }`, or
  `mysql_version` / `"MySQL 8.4.2"` when the active database profile is MySQL. A PHP
  service reports its own version (a project's pinned one), anything else the active PHP.
  Keys for what isn't installed are left out (`services::telemetry::stack_context`).
- **`error.report`**: what was being done to which service:
  `{ "action": "service.start | service.stop | service.restart | service.run", "service": "php-8.0.30" }`
  (`service.run` is a crash). `event_name` is `AppError::code()`, the variant's name,
  or `ServiceCrashed`.

**Heartbeat** (`HeartbeatPayload` in `services/telemetry.rs`):

```json
{
  "device_id": "…",
  "session_id": "…",
  "app_version": "1.0.0",
  "os": "Windows 11 Home Single Language",
  "os_version": "11 (26200)",
  "device_name": "Yahya",
  "occurred_at": "2026-09-02T06:00:00+00:00",
  "ended_at": null
}
```

`occurred_at` is stamped at *record* time, not send time — a row queued while offline
and sent later still reports when it actually happened. `os`/`os_version` come from
`sysinfo::System::long_os_version()` / `os_version()`.

`device_name` is the Windows account name the app runs under, `%USERNAME%` (the `Yahya`
in `C:\Users\Yahya`), read fresh on each 5-minute heartbeat by
`services::telemetry::device_name()`. It lets the dashboard's Devices page show a human
name instead of `dev_xxxxxxxx`. Blank becomes `null`, and anything past the API's 64-char
limit is cut client-side, since a `422` would get the whole heartbeat dropped (see
Sending). The closing heartbeat sends `null`, which the backend reads as "keep what you
have". Like every other field, it's never recorded or sent while `shareUsageData` is
`false`. Added after 3.0.0, so 3.0.0 and older clients don't send it.

`session_id` is one UUID generated once per launch (`services::telemetry::SessionIdState`,
in-memory only, never persisted) and reused on every heartbeat for that run.

## Sending

`send_pending` (`services/telemetry.rs`) processes up to 20 unsent rows per tick, oldest
first, POSTing each one individually to `{base_url}/api/v1/telemetry/event` or
`.../heartbeat` depending on its stored `type` — the real backend has no bulk endpoint,
so "batch" here means "several requests per wake-up", not one combined request. On
success the row's `sent_at` is set. A `5xx`, `408` or network failure leaves it alone to
be retried on the next tick (the queue's whole retry strategy — no explicit backoff
timer). A `429` response stops the rest of that tick's batch early. Any other `4xx` (a
`422` validation failure, say) is a verdict on the row itself, so the row is deleted
(`db::telemetry::discard`), as is a row that no longer parses. Left in place, such rows
would be refetched first every tick, and 20 of them would stop everything recorded after
them from ever being sent. Rows already sent are deleted after
7 days (`db::telemetry::delete_sent_before`) — `pending_events` is a bounded local queue,
not a permanent log.

Every failure in this path is `log::warn!`-only. It never surfaces to the UI and never
blocks a user-initiated action — this is the "kegagalan pengiriman tidak boleh
mengganggu user" requirement from Fase 2.5.

## What's deliberately *not* sent

- No file paths, project names, database names, or anything else from the user's local
  filesystem/config beyond the fields listed above. The heartbeat's `device_name` (the
  Windows account name) is the one deliberate exception, and it's the only thing that
  identifies the person rather than the install.
- `event_name` is limited to a service's display name, an error kind, or omitted — never
  free text a user typed (that's what support tickets are for, a separate, explicit,
  user-initiated action documented in `docs/v2/rezure-app-v2-phases-tasks.md`'s Fase 2.1).
- `error.report` never carries the error's message. Messages hold paths, project names
  and hostnames, so only the kind (`AppError::code()`) and the service id go out.
- `payload` holds version numbers and service ids only.
