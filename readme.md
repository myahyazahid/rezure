# Rezure

> A modern, lightweight local development environment manager for Windows — inspired by Laragon, built with Tauri + Rust + Vue 3.

Rezure lets you spin up local web development environments (Apache/Nginx, PHP, MySQL, and more) with one click — fast, portable, and resource-friendly.

---

## ✨ Features

- One-click start/stop for local services (Apache, Nginx, MySQL/MariaDB, PHP-FPM)
- Automatic virtual host & `hosts` file configuration
- Multi-version PHP switching
- Database management for the bundled MariaDB — create, export, import, and open a database in whichever SQL client you already have installed (see [`docs/databases.md`](docs/databases.md))
- Portable bundled binaries — no manual installation required
- Lightweight footprint thanks to Tauri (native webview, small binary size)

> See [`docs/roadmap.md`](docs/roadmap.md) for planned features (quick app installer, HTTPS via mkcert, tunneling, Docker mode, usage dashboard, etc.)

---

## 🧱 Tech Stack

| Layer | Technology |
|---|---|
| App Framework | [Tauri](https://tauri.app/) |
| System / Backend | Rust |
| Frontend | Vue 3 + Vite |
| Styling | Tailwind CSS |
| State Management | Pinia |
| Local Storage | SQLite (`rusqlite`/`sqlx`) + JSON/TOML config |
| IPC | Tauri `invoke()` & events |

---

## 📁 Project Structure

```
rezure/
├── src/                      # Vue 3 frontend
│   ├── assets/                # Static assets (images, icons, fonts)
│   ├── components/            # Reusable Vue components
│   │   ├── common/              # Generic UI components (buttons, modals, etc.)
│   │   └── services/            # Components specific to service management UI
│   ├── views/                 # Page-level views (Dashboard, Projects, Settings)
│   ├── stores/                # Pinia stores (service state, project state, settings)
│   ├── composables/           # Reusable composition functions (e.g. useService, useProject)
│   ├── router/                # Vue Router configuration
│   ├── types/                 # Shared TypeScript types/interfaces
│   └── main.ts                # Frontend entry point
│
├── src-tauri/                # Rust backend (Tauri)
│   ├── src/
│   │   ├── commands/           # Tauri commands exposed to frontend (invoke handlers)
│   │   ├── services/            # Core logic: process management, port scanning
│   │   ├── config/              # App config read/write (JSON/TOML)
│   │   ├── db/                  # SQLite models & queries
│   │   ├── utils/                # Shared helper functions
│   │   └── main.rs              # Backend entry point
│   ├── binaries/                # Bundled portable binaries (PHP, Nginx, MySQL, etc.)
│   ├── icons/                   # App icons
│   └── tauri.conf.json          # Tauri configuration
│
├── docs/                     # Project documentation
│   ├── architecture.md         # High-level architecture overview
│   ├── roadmap.md              # Development phases & planned features
│   ├── setup-dev.md            # Local development setup guide
│   └── contributing-notes/     # Additional notes for contributors
│
├── .github/                  # GitHub-specific files
│   ├── ISSUE_TEMPLATE/
│   └── workflows/               # CI/CD pipelines
│
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── LICENSE
└── README.md
```

**Folder principles:**
- Frontend and backend are cleanly separated (`src/` vs `src-tauri/`) — contributors can work on UI without touching Rust, and vice versa.
- Rust backend is organized by responsibility (`commands`, `services`, `config`, `db`), not by feature — keeps system-level logic easy to audit.
- Vue frontend follows a standard composition-API structure (`components`, `views`, `stores`, `composables`) for familiarity to any Vue contributor.

---

## 🚀 Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- [Node.js](https://nodejs.org/) (LTS recommended) + npm/pnpm
- [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/)

### Setup

```bash
# Clone the repository
git clone https://github.com/<your-username>/rezure.git
cd rezure

# Install frontend dependencies
npm install

# Run in development mode
npm run tauri dev
```

### Build for production

```bash
npm run tauri build
```

For a more detailed walkthrough, see [`docs/setup-dev.md`](docs/setup-dev.md).

---

## 🤝 Contributing

Rezure is open source and contributions are welcome — bug reports, feature requests, documentation improvements, and pull requests.

Before contributing, please read:
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to set up your dev environment, coding conventions, and PR process
- [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) — community guidelines

**Reference docs:**
- [`docs/architecture.md`](docs/architecture.md) — technical foundation & design decisions
- [`docs/databases.md`](docs/databases.md) — the bundled MariaDB, SQL-client detection, export/import
- [`docs/roadmap.md`](docs/roadmap.md) — what's in scope per version

**Coding standards (summary):**
- Rust code formatted with `cargo fmt`, linted with `cargo clippy`
- Frontend code formatted with Prettier, linted with ESLint
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) (e.g. `feat:`, `fix:`, `docs:`)
- Every new feature/module should include a short doc comment or entry in `docs/`

---

## 🗺️ Roadmap

Development is organized into phases (MVP → advanced features → telemetry dashboard). See [`docs/roadmap.md`](docs/roadmap.md) for the full breakdown.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).

---

## 🙏 Acknowledgements

Rezure is inspired by [Laragon](https://laragon.org/), reimagined with a modern stack and community-driven development.