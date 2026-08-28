# Fase Development — Rezure v1 (MVP)

Roadmap pengembangan **Rezure v1**, fokus ke rilis awal yang fungsional sebelum menambahkan fitur lanjutan & dashboard.

> Stack: **Tauri (Rust) + Vue 3 + Tailwind CSS**

---

## Fase 1 — Setup Project

- Inisialisasi project Tauri + Vue 3 (Vite, Tailwind, Pinia)
- Setup struktur folder (frontend `src/`, backend Rust `src-tauri/`)
- Setup `tauri.conf.json` (nama app, ikon, window size, permissions)
- Konfigurasi dasar komunikasi `invoke()` Vue ↔ Rust (test koneksi)

---

## Fase 2 — Service Manager (Core Feature)

- Bundling portable binary: Apache/Nginx, PHP (minimal 1 versi dulu), MySQL/MariaDB
- Fungsi Rust: start/stop proses service (`tokio` + `sysinfo`)
- UI Vue: tombol start/stop per service + indikator status (running/stopped)
- Deteksi port conflict sebelum start service
- Log viewer sederhana per service (real-time via Tauri events)

---

## Fase 3 — Project & Virtual Host

- Auto-detect folder project di direktori kerja (misal `www/`)
- Generate virtual host config otomatis (`.conf` Apache/Nginx)
- Auto-edit file `hosts` sistem operasi (butuh permission admin)
- List project di UI dengan status aktif/tidak

---

## Fase 4 — Konfigurasi & Storage Lokal

- Simpan pengaturan user (config file JSON/TOML)
- Simpan daftar project & histori di SQLite (`rusqlite`/`sqlx`)
- Halaman Settings dasar (path binary, port default, dll)

---

## Fase 5 — UI Polish

- Layout utama (sidebar: services, projects, settings)
- Styling dengan Tailwind, komponen konsisten
- Dark/light mode (opsional, nilai tambah)
- Icon & branding Rezure

---

## Fase 6 — Testing & Packaging

- Testing manual di beberapa versi Windows
- Build installer via Tauri bundler (`.msi`/`.exe`)
- Uji start/stop service tidak bentrok dengan software lain (XAMPP, Laragon, dll)
- Fix bug kritikal sebelum rilis

---

## Fase 7 — Rilis v1

- Siapkan halaman download sederhana / README
- Rilis beta ke lingkup terbatas (diri sendiri, teman, komunitas kecil)
- Kumpulkan feedback awal secara manual (belum via dashboard, karena telemetry masuk di v2)

---

## Catatan

Fitur berikut **sengaja belum masuk di v1**, karena akan dikembangkan di fase berikutnya (v2):

- Integrasi telemetry ke Dashboard Laravel
- Quick app installer (template Laravel/WordPress/Vue)
- Auto HTTPS (mkcert), tunnel (ngrok/cloudflared)
- Docker toggle mode, database GUI embedded
- Auto-update mechanism
