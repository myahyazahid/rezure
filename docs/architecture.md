# Architecture — Rezure

This document describes the technical foundation of Rezure: how core systems are structured and the conventions contributors should follow when extending them.

---

## 1. Process Management

Managing local services (Apache, Nginx, MySQL/MariaDB, PHP-FPM) is the core responsibility of Rezure's backend.

- Every service is implemented behind a shared abstraction (a `Service` trait) exposing `start()`, `stop()`, `status()`, and `restart()`. Adding a new service means implementing this trait — no changes needed elsewhere.
- The PID of every spawned process is tracked. On app exit, all tracked processes are terminated to avoid orphaned ("zombie") processes still consuming system resources.
- Shutdown is graceful by default: processes are given time to exit cleanly before being force-killed.

---

## 2. Error Handling

Because Rezure interacts heavily with the OS (files, ports, processes), failure points are numerous — error handling is treated as a first-class concern, not an afterthought.

- A centralized custom error type (via `thiserror`) is used instead of scattering `unwrap()` calls throughout the codebase.
- Every Tauri command (`#[tauri::command]`) exposed to the frontend returns a `Result<T, E>` that serializes cleanly to JSON, so the Vue frontend can surface meaningful error messages to the user rather than a generic failure.

---

## 3. Config & State Management

- App configuration has a single source of truth: one Rust struct, serialized to TOML/JSON, loaded once at startup.
- This config lives in Tauri's managed state (`AppState`) and is shared across commands via `tauri::State`, avoiding repeated file reads on every user action.

---

## 4. Logging

- Structured logging via the `tracing` crate is used instead of `println!`, allowing log level filtering (debug/info/error) and output redirection to a log file.
- This foundation also supports future error-reporting features (e.g. sending crash/error data to a remote dashboard).

---

## 5. IPC Contract (Frontend ↔ Backend)

- Data types are kept consistent between Rust structs and TypeScript interfaces — ideally auto-generated (e.g. via `tauri-specta`) to prevent the API contract from drifting out of sync.
- Business logic stays in Rust. The Vue frontend only invokes commands and renders results — it does not duplicate logic like service management or file editing.

---

## 6. Database (SQLite)

- Schema changes are handled through a migration system (`sqlx migrate` or `refinery`) rather than manual schema edits, so contributors can apply schema updates without resetting their local database.
- Initial schema scope: `projects`, `services_config`, `settings`.

---

## 7. Testing Strategy

- Logic that doesn't depend on OS interaction (e.g. config parsing, port conflict detection) is kept separate from logic that does (e.g. actually spawning a process), so the former can be easily unit tested.
- OS-dependent logic is covered by integration tests or documented manual test steps where automated testing isn't practical.
- `cargo test` is set up from the start, even with minimal coverage, to establish the habit for future contributors.

---

## 8. Continuous Integration

- GitHub Actions runs `cargo fmt --check`, `cargo clippy`, and `npm run lint` on every pull request, keeping code quality consistent without requiring manual review for style issues.

---

## Priority for New Contributors

If you're picking up early-stage work, the foundation that matters most — and that everything else builds on — is:

1. Process management (§1)
2. Error handling (§2)
3. Config & state management (§3)

Getting these right early is what keeps the codebase maintainable as more services and features are added.
