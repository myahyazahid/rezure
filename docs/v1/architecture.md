# Architecture — Rezure

This document describes the technical foundation of Rezure: how core systems are structured and the conventions contributors should follow when extending them.

---

## 1. Process Management

Managing local services (Apache, Nginx, MySQL/MariaDB, PHP-FPM) is the core responsibility of Rezure's backend.

- Every service is implemented behind a shared abstraction (a `Service` trait) exposing `start()`, `stop()`, `status()`, and `restart()`. Adding a new service means implementing this trait — no changes needed elsewhere.
- The PID of every spawned process is tracked. On app exit, all tracked processes are terminated to avoid orphaned ("zombie") processes still consuming system resources.
- Shutdown is graceful by default: processes are given time to exit cleanly before being force-killed.

---

## 1b. Runtime Versions

- PHP versions are **discovered on disk**, not enumerated in a fixed manifest: `binaries::discover` scans Rezure's own download cache and a user-facing drop-in folder, so a version installed from the app and one the user unpacked by hand are equally real and neither needs registering. Deleting a folder is a complete uninstall.
- The installable list comes from php.net's published release index at runtime, which supplies the SHA-256 every download is still verified against — a live catalog is not a reason to verify less than a pinned entry.
- `services::process` resolves the active PHP at spawn time rather than caching a path, so switching versions takes effect on the next start with no extra plumbing.
- Full detail in [`php-versions.md`](php-versions.md).

---

## 1c. Project Stack & Requirements

What a project is built on and needs (frameworks, PHP, Node.js, package manager, database) is
worked out in `services::project_stack` and attached to each project as `ProjectInfo.stack_info`
by `list_projects`, after the version pins are loaded.

- **Files only.** Nothing is executed: `composer.json`/`composer.lock`,
  `vendor/composer/platform_check.php`, `package.json`, `node_modules/<pkg>/package.json` or
  `package-lock.json`, `.nvmrc`/`.node-version`, `.env`, framework config files and Prisma's
  schema. That keeps it cheap enough to run on every listing.
- **Constraints are parsed in Rust** (`services::version_constraint`), in two dialects: Composer's
  for PHP and npm's for Node.js. They differ in what `~` may move and whether a bare partial
  version (`20`) is a range. A constraint that doesn't parse is reported as unreadable, never as
  a mismatch.
- **Compared against what serves the project**: its pin while that version is installed, the
  global active version otherwise (the same fallback `php_pool` uses). Rust also picks the
  suggestion: the newest installed version that fits, or the oldest release line to install.
  The frontend only phrases it (`composables/projectStack.ts`).
- **Extending it:** framework and tool packages are listed in `COMPOSER_FRAMEWORKS`,
  `NODE_FRAMEWORKS` and `NODE_TOOLS` at the top of `project_stack.rs`. Only those packages'
  `engines.node` counts toward a project's Node.js requirement.
- Full detail in [`projects.md`](projects.md#stack--requirements).

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

## 5b. Modals

- A modal never closes on a click outside it: a stray click shouldn't throw away what was typed or
  interrupt an action. The way out is the **X** in the top-right corner, a Cancel/Close button,
  or Escape.
- The X is `components/common/ModalCloseButton.vue`. It's absolutely positioned, so the panel
  needs `relative` and its header needs room on the right (`pr-8`). Wire `@click` to the modal's
  own close function, and pass `:disabled` while closing would abandon a running action (the
  same condition the modal's Cancel button uses).
- Dropdown menus are different: they still close on an outside click.

---

## 6. Databases

Two different things are called "database" in this codebase, and they are unrelated:

**Rezure's own storage (SQLite)** — the app's internal state.

- Schema changes are handled through a migration system (`sqlx migrate` or `refinery`) rather than manual schema edits, so contributors can apply schema updates without resetting their local database.
- Initial schema scope: `projects`, `services_config`, `settings`.

**The managed MariaDB** — the server Rezure starts *for the user's projects*, with its own data directory under `C:\rezure\data\mariadb`.

- Managed through the client binaries that ship in the same archive as the server (`mariadb.exe`, `mariadb-dump.exe`), not through a MySQL driver crate — the archive is already downloaded, so a second way to speak the protocol would be redundant weight.
- Identifiers (schema and collation names) can't be bound as query parameters, so they are validated against a strict allowlist before any statement is built. See `services::database::validate_identifier`.
- Rezure ships no database GUI of its own: `services::db_clients` detects the SQL clients already installed and hands one a ready-made connection.
- Full detail in [`databases.md`](databases.md).

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
