# Development Setup Guide

This guide walks through setting up a local development environment for Rezure, including common issues on Windows.

---

## 1. Prerequisites

### Rust
Install via [rustup](https://www.rust-lang.org/tools/install):

```bash
# Verify installation
rustc --version
cargo --version
```

Use the stable toolchain (default with rustup).

### Node.js
Install the LTS version from [nodejs.org](https://nodejs.org/), or via a version manager (e.g. `nvm-windows`).

```bash
node --version
npm --version
```

### Tauri Prerequisites (Windows)

Tauri requires:
- **Microsoft C++ Build Tools** — install via [Visual Studio Installer](https://visualstudio.microsoft.com/visual-cpp-build-tools/), select "Desktop development with C++"
- **WebView2** — usually pre-installed on Windows 10/11; if missing, download from [Microsoft's WebView2 page](https://developer.microsoft.com/microsoft-edge/webview2/)

Full official reference: [Tauri Prerequisites](https://tauri.app/start/prerequisites/)

---

## 2. Clone & Install

```bash
git clone https://github.com/<your-username>/rezure.git
cd rezure
npm install
```

This installs frontend dependencies. Rust dependencies (crates) are fetched automatically on first build via Cargo.

---

## 3. Running in Development Mode

```bash
npm run tauri dev
```

This will:
1. Start the Vite dev server for the Vue frontend (with hot reload)
2. Compile the Rust backend (first run will take longer — subsequent runs use incremental compilation)
3. Launch the Tauri window

---

## 4. Project-Specific Setup

### Portable Binaries
Rezure bundles portable service binaries (PHP, Nginx/Apache, MySQL). For local development:
- Check `src-tauri/binaries/README.md` (once added) for which binaries are expected and where to place them
- These binaries are not committed to the repository (large file size) — a setup script or download instructions will be provided as this matures

### Environment Variables / Config
- Local app config is stored in JSON/TOML — no `.env` file is required for the desktop app itself
- If working on the telemetry/dashboard integration (v2+), refer to the Laravel dashboard repo's own setup guide for its `.env` configuration

---

## 5. Common Issues

**`error: Microsoft Visual C++ 14.0 or greater is required`**
→ Install the C++ Build Tools workload mentioned above, then restart your terminal.

**Tauri window opens blank / white screen**
→ Usually means the Vite dev server didn't start correctly. Check the terminal for frontend build errors, or try deleting `node_modules` and reinstalling.

**`cargo build` fails after pulling new changes**
→ Run `cargo clean` in `src-tauri/` and rebuild — stale build artifacts can occasionally cause issues after dependency changes.

**Port already in use (Vite dev server)**
→ Another process is using the default Vite port. Either stop it or change the port in `vite.config.ts`.

**Admin permission errors when editing `hosts` file**
→ Some features (virtual host setup) require elevated permissions. Run your terminal/IDE as Administrator when testing these features locally.

---

## 6. Before Submitting a PR

```bash
# Rust: format & lint
cd src-tauri
cargo fmt
cargo clippy

# Frontend: lint
cd ..
npm run lint
```

Make sure the app builds and runs cleanly (`npm run tauri dev`) before opening a pull request. See [`CONTRIBUTING.md`](../CONTRIBUTING.md) for the full PR process.

---

## 7. Useful Commands Reference

| Command | Description |
|---|---|
| `npm run tauri dev` | Run app in development mode with hot reload |
| `npm run tauri build` | Build production installer |
| `cargo test` (in `src-tauri/`) | Run Rust unit/integration tests |
| `cargo fmt` | Format Rust code |
| `cargo clippy` | Lint Rust code |
| `npm run lint` | Lint frontend code |

---

If you run into an issue not covered here, please open a [Discussion](../../../discussions) or an issue so we can expand this guide.
