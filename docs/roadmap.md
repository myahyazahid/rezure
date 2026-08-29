# Roadmap — Rezure

This document outlines the development phases for Rezure, from the initial MVP release through planned future features.

---

## v1 — MVP (Core Functionality)

Goal: a functional, installable release covering the essential local dev environment features.

### Phase 1 — Project Setup
- Initialize Tauri + Vue 3 project (Vite, Tailwind, Pinia)
- Set up folder structure (frontend `src/`, backend `src-tauri/`)
- Configure `tauri.conf.json` (app name, icon, window size, permissions)
- Verify basic `invoke()` communication between Vue and Rust

### Phase 2 — Service Manager (Core Feature)
- Bundle portable binaries: Apache/Nginx, PHP (at least one version), MySQL/MariaDB
- Implement start/stop process control in Rust (`tokio` + `sysinfo`)
- UI: start/stop buttons per service with status indicators
- Port conflict detection before starting a service
- Basic real-time log viewer per service (via Tauri events)

### Phase 3 — Project & Virtual Host
- Auto-detect project folders in the working directory (e.g. `www/`)
- Auto-generate virtual host config (`.conf` for Apache/Nginx)
- Auto-edit the OS `hosts` file (requires admin permission)
- Project list UI with active/inactive status

### Phase 3.5 — Runtime Version Switching
- Support multiple installed versions per bundled runtime (starting with PHP — the only one with real download/process infrastructure in place)
- "Switch" UI: pick which installed version is active, install additional versions on demand
- Services and scaffolding (Composer/Laravel) resolve to whichever version is currently active
- Node.js and Python rows are shown as not-yet-available rather than faked — no portable binary source exists for either yet; adding them means repeating Phase 2's binary-bundling work for a new runtime
- Active-version selection is in-memory only until Phase 4's settings persistence lands (a restart resets it to the newest installed version)

### Phase 4 — Config & Local Storage
- Persist user settings (JSON/TOML config file)
- Store project list & history in SQLite (`rusqlite`/`sqlx`)
- Basic Settings page (binary paths, default ports, etc.)

### Phase 5 — UI Polish
- Main layout (sidebar: services, projects, settings)
- Consistent Tailwind-based styling
- Dark/light mode (nice-to-have)
- App icon & branding

### Phase 6 — Testing & Packaging
- Manual testing across Windows versions
- Build installer via Tauri bundler (`.msi`/`.exe`)
- Verify no conflicts with other tools (XAMPP, Laragon, etc.)
- Fix critical bugs before release

### Phase 7 — v1 Release
- Simple download page / README instructions
- Beta release to a limited audience
- Collect initial feedback manually (telemetry comes in v2)

---

## v2 — Telemetry & Dashboard Integration

Goal: connect Rezure to a Laravel-based dashboard for usage analytics and remote management.

- Generate a unique `device_id` on first install
- Async, non-blocking event reporting (with local queue for offline use)
- Laravel API endpoints: `POST /api/v1/telemetry/heartbeat`, `POST /api/v1/telemetry/event`
- Lightweight auth: per-device API key + rate limiting
- Laravel Queue setup for event ingestion
- Opt-out toggle for usage data sharing (client-side)

**Dashboard (Laravel) — initial scope:**
- Active users (DAU/WAU/MAU) & retention
- Version adoption tracking
- Feature usage heatmap
- Session duration

---

## v3 — Advanced Features

Goal: expand Rezure beyond core Laragon parity with added value features.

- Quick app installer (Laravel, WordPress, Vue starter templates)
- Auto HTTPS via mkcert
- One-click tunneling (ngrok/cloudflared)
- Integrated terminal
- Docker toggle mode (container vs native binary)
- Embedded lightweight database GUI
- Local project health dashboard (port conflict detector, log viewer)

**Dashboard (Laravel) — expanded scope:**
- Error & crash reporting from client
- Update checker (`GET /api/v1/version/latest`) + auto-notify
- License/key management (if a Pro/freemium model is introduced)
- Feature flags (enable/disable features remotely without a client update)
- Remote config for quick-app templates
- In-app feedback form → dashboard
- Changelog/announcement system

---

## v4 — Distribution & Sustainability

Goal: make Rezure easy to install, update, and maintain long-term.

- Auto-update mechanism (`tauri-plugin-updater`)
- CI/CD pipeline for automated builds & releases
- Public documentation site
- Community contribution health (issue templates, CI checks, changelog discipline)

---

## Guiding Principle

Each version should ship a coherent, usable increment — v1 must be a solid standalone tool even without the dashboard, and each subsequent version builds on a stable foundation rather than expanding scope indefinitely.

See [`docs/architecture.md`](architecture.md) for the technical foundation these phases are built on.
