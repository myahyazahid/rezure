# CLAUDE.md

This file provides guidance to Claude (and other AI coding agents) when working on the Rezure codebase.

---

## Project Overview

**Rezure** is an open-source, Laragon-inspired local development environment manager for Windows, built with **Tauri (Rust) + Vue 3**. It lets developers start/stop local services (Apache/Nginx, PHP, MySQL) and manage local projects with one click.

The project is designed to be **sustainable and community-maintained** — clean code, clear documentation, and consistent architecture matter more here than moving fast and breaking conventions.

Read these before making non-trivial changes:
- [`docs/architecture.md`](docs/architecture.md) — technical foundation & design decisions
- [`docs/roadmap.md`](docs/roadmap.md) — what's in scope for the current version vs future versions
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — coding conventions & PR process

---

## Tech Stack

| Layer | Technology |
|---|---|
| App Framework | Tauri |
| Backend / System | Rust |
| Frontend | Vue 3 + Vite |
| Styling | Tailwind CSS |
| State Management | Pinia |
| Local Storage | SQLite (`rusqlite`/`sqlx`) + JSON/TOML config |
| IPC | Tauri `invoke()` & events |

---

## Project Structure

```
rezure/
├── src/                      # Vue 3 frontend
│   ├── components/            # common/ (generic UI), services/ (service-specific UI)
│   ├── views/                 # Page-level views
│   ├── stores/                # Pinia stores
│   ├── composables/           # Reusable composition functions
│   └── types/                 # Shared TypeScript types
│
├── src-tauri/                # Rust backend
│   ├── src/
│   │   ├── commands/           # #[tauri::command] handlers (thin — delegate to services/)
│   │   ├── services/            # Core logic: process mgmt, port scanning
│   │   ├── config/              # Config read/write
│   │   ├── db/                  # SQLite models & queries
│   │   └── utils/
│   └── binaries/                # Bundled portable service binaries
│
└── docs/                     # architecture.md, roadmap.md, setup-dev.md
```

**Rule of thumb:** `commands/` should stay thin (just glue between frontend and logic). Real logic belongs in `services/`. Don't put business logic in Vue components — components call commands and render results.

---

## Architectural Principles (non-negotiable)

These decisions were made deliberately — don't work around them without discussing first (open an issue/leave a comment explaining why if you think an exception is needed):

1. **Services implement a shared `Service` trait** (`start()`, `stop()`, `status()`, `restart()`). Adding a new service (e.g. Redis, PostgreSQL) means implementing this trait — don't special-case a new service type elsewhere in the codebase.
2. **No unhandled `unwrap()`/`panic!` in fallible paths.** Use the centralized error type (`thiserror`) and `Result<T, E>`. This app touches the filesystem, processes, and ports — assume anything can fail.
3. **Every Tauri command returns a serializable `Result`**, so the frontend can show a real error message, not a generic failure.
4. **Config is a single source of truth**, loaded once into Tauri managed state (`AppState`), not re-read from disk on every action.
5. **Business logic stays in Rust.** Vue components/composables call `invoke()` and render — they do not duplicate service-management or file-editing logic.
6. **Structured logging via `tracing`**, not `println!`/`console.log` for anything beyond quick local debugging.
7. **Schema changes go through migrations** (`sqlx migrate`/`refinery`), never hand-edited on a live schema.

See [`docs/architecture.md`](docs/architecture.md) for the reasoning behind each.

---

## Coding Conventions

### Rust
- Format with `cargo fmt`, lint with `cargo clippy` — run both before finishing a task
- Doc comments (`///`) on public functions/modules
- Errors handled explicitly — no silent `unwrap()` on anything touching OS/user input

### Frontend (Vue 3 + TypeScript)
- Composition API with `<script setup>` for all new components
- No `any` in TypeScript without a comment justifying it
- Shared logic → `composables/`, not copy-pasted across components
- Format with Prettier, lint with ESLint (`npm run lint`)

### Commits
Follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`

---

## What NOT to Do

- Don't add a new service integration without going through the `Service` trait
- Don't bundle new large binaries into the repo — portable binaries are handled via on-demand download (see roadmap v1 Phase 2 notes), not committed to git
- Don't introduce a second state-management pattern on the frontend — Pinia only
- Don't add features out of scope for the current version — check [`docs/roadmap.md`](docs/roadmap.md) before implementing something that belongs in a later phase
- Don't concatenate user input (project names, paths) directly into shell commands or generated config files — sanitize/escape to avoid injection

---

## Useful Commands

```bash
npm run tauri dev      # run app in dev mode
npm run tauri build    # build production installer
cargo fmt               # format Rust (run inside src-tauri/)
cargo clippy            # lint Rust
cargo test              # run Rust tests
npm run lint            # lint frontend
```

---

## When Making Changes

1. Check `docs/roadmap.md` to confirm the work is in scope for the current version
2. Follow the existing folder structure and architectural principles above — don't introduce new patterns without a clear reason
3. Run formatters/linters before considering a task done
4. Update relevant docs (`docs/architecture.md`, `README.md`, or inline doc comments) if the change affects how the app is built, configured, or used
5. If a change requires deviating from a principle listed above, explain why in the PR description rather than silently working around it