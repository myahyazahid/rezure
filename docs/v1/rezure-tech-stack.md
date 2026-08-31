# Tech Stack — Rezure (Tauri)

Dokumen ini merangkum tech stack yang dipilih untuk membangun **Rezure**, aplikasi desktop clone Laragon dengan fitur tambahan.

---

## Core Framework — Tauri

Dipilih karena Rezure adalah tool yang berjalan terus di background untuk mengatur service (Apache/MySQL/dll), sehingga resource usage sangat penting. Tauri menggunakan native webview OS (bukan bundle Chromium seperti Electron), sehingga:

- Binary size jauh lebih kecil (~5-10MB, dibanding Electron yang bisa 100-150MB)
- RAM usage lebih ringan
- Startup time lebih cepat

---

## Breakdown per Layer

### 1. Backend / System Layer — Rust

Rust berperan sebagai "otak" yang mengurus hal-hal low-level:

- Start/stop proses (Apache, Nginx, MySQL, PHP-FPM)
- Baca/tulis file (`hosts` file, config `.conf`)
- Registry Windows (untuk integrasi shell/context menu jika diperlukan)
- Port scanning (deteksi konflik port)

**Crate yang digunakan:**
- `tokio` — async runtime
- `sysinfo` — cek proses & resource sistem
- `reqwest` — untuk mengirim data telemetry ke API Laravel

### 2. Frontend / UI — Vue 3

- Vue 3 + Vite
- Tailwind CSS untuk styling
- Pinia untuk state management (status service running/stopped, list project, dll)
- Komponen UI: custom atau headless library seperti `radix-vue` / `shadcn-vue` untuk tampilan modern

### 3. Komunikasi Frontend ↔ Backend

- Tauri `invoke()` — Vue memanggil fungsi Rust langsung (native IPC, lebih cepat dari REST API)
- Tauri events — untuk push data real-time dari Rust ke Vue (misal: status service berubah, log streaming)

### 4. Storage Lokal

- Config aplikasi: file JSON/TOML (simple, mudah diparse oleh Rust)
- Data lebih kompleks (history, cache): SQLite via `rusqlite` atau `sqlx`

### 5. Bundled Binaries (Portable)

- PHP (multi-versi), Nginx/Apache, MySQL/MariaDB — didownload/dibundle sebagai portable version, tidak perlu instalasi manual (sama seperti cara kerja Laragon)

### 6. Auto-Update

- Menggunakan plugin bawaan Tauri: `tauri-plugin-updater`
- Setup endpoint pengecekan versi terbaru

---

## Ringkasan Stack

| Layer | Teknologi |
|---|---|
| Framework | Tauri |
| Backend / System | Rust |
| Frontend | Vue 3 + Tailwind CSS |
| State Management | Pinia |
| Komunikasi IPC | Tauri `invoke()` & events |
| Storage Lokal | JSON/TOML + SQLite (`rusqlite`/`sqlx`) |
| HTTP Client (ke API Laravel) | `reqwest` |
| Auto-Update | `tauri-plugin-updater` |
