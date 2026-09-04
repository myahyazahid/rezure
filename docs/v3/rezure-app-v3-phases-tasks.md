# Rezure (Desktop App) — v3: Advanced Features

Roadmap fitur lanjutan Rezure. Beberapa fitur yang sebelumnya direncanakan di v3 (Quick App Installer, Auto HTTPS/mkcert, Integrated Terminal, Embedded Database GUI) **sudah tersedia** di rilis sebelumnya — tidak dimasukkan lagi di sini.

---

## Fase 3.1 — One-click Tunneling

**Tujuan:** User bisa share local project ke internet sementara tanpa setup manual.

### Tasks
- [ ] Integrasi ngrok atau cloudflared (pilih salah satu, atau keduanya sebagai opsi)
- [ ] Deteksi apakah binary tunnel tool sudah tersedia; download portable version jika belum ada
- [ ] Tombol "Share" di tiap project card, trigger tunnel ke port project tersebut
- [ ] Tampilkan public URL yang di-generate langsung di UI, dengan tombol copy
- [ ] Tombol "Stop sharing" untuk menutup tunnel

---

## Fase 3.2 — Docker Toggle Mode

**Tujuan:** User bisa pilih menjalankan service via native binary (default) atau via Docker container.

### Tasks
- [ ] Deteksi apakah Docker Desktop terinstall & running di sistem
- [ ] Opsi toggle per-project atau per-service: native vs Docker
- [ ] Generate/kelola `docker-compose.yml` sederhana untuk service yang dipilih mode Docker
- [ ] Start/stop container mengikuti pola yang sama dengan `Service` trait yang sudah ada (agar tetap konsisten dengan arsitektur di `docs/architecture.md`)

---

## Fase 3.3 — Project Health Dashboard

**Tujuan:** Ringkasan kondisi tiap project dalam satu pandangan.

### Tasks
- [ ] Konsolidasi status semua service terkait per-project dalam satu view
- [ ] Port conflict detector yang lebih menyeluruh (across semua project, bukan cuma saat start service)
- [ ] Tampilkan ukuran log per service, dengan opsi clear log
- [ ] Indikator visual sederhana (misal: sehat/perlu perhatian) berdasarkan status gabungan

---

## Fase 3.4 — Auto-Update Mechanism

**Tujuan:** Distribusi update tidak lagi manual — user diberi tahu dan bisa update langsung dari dalam app.

### Tasks
- [ ] Implementasi `tauri-plugin-updater`
- [ ] Cek update via endpoint `GET /api/v1/version/latest` (endpoint yang sama dipakai website & dashboard)
- [ ] Notifikasi in-app saat ada versi baru tersedia
- [ ] Link notifikasi ke halaman Changelog (menu yang sudah ada dari v2) untuk detail perubahan
- [ ] Alur update: download di background, apply saat user konfirmasi (hindari update paksa yang mengganggu kerja user)

---

## Fase 3.5 — Support Developer / Donate Menu

**Tujuan:** User yang mau mendukung pengembangan Rezure bisa donasi dengan mudah, lewat berbagai platform (lokal, global, dan crypto).

### Tasks
- [ ] Tambahkan menu "Support Developer" di sidebar (terpisah dari menu "Feedback" di Fase 2.1)
- [ ] Section link donasi lokal: Trakteer / Saweria — tombol buka browser eksternal ke halaman donasi
- [ ] Section link donasi global: GitHub Sponsors / Ko-fi — tombol buka browser eksternal
- [ ] Section donasi crypto: tampilkan wallet address (misal BTC, ETH, USDT — sesuaikan yang dipakai) dengan tombol copy address dan QR code per wallet
- [ ] Pesan singkat konteks: "Rezure gratis & open-source, dukung pengembangannya" beserta link ke halaman "About" untuk cerita project
- [ ] Semua link/alamat dikelola dari konfigurasi statis di app (bukan dari API) — kecuali suatu saat ingin diubah dari server tanpa update app, baru pertimbangkan pindah ke remote config

---

## Dependency ke Proyek Lain

Fase 3.4 membutuhkan endpoint `GET /api/v1/version/latest` sudah tersedia di `rezure-dashboard`. Fase 3.1–3.3 dan 3.5 sepenuhnya independen, tidak bergantung pada backend.

**Catatan soal analytics lanjutan (v3 `rezure-dashboard`):** fitur traffic by hour, breakdown negara, cohort retention, dll di dashboard **tidak membutuhkan perubahan apapun di app ini** — semua data granular yang dibutuhkan (timestamp, OS version, metadata service) sudah terkirim sejak fondasi telemetry v2. Geolocation negara diproses di sisi server dari IP request yang masuk, bukan dikirim dari client.

## Urutan Pengerjaan yang Disarankan

1. Fase 3.1 (One-click Tunneling) — independen, langsung menambah nilai bagi user
2. Fase 3.2 (Docker Toggle Mode)
3. Fase 3.3 (Project Health Dashboard)
4. Fase 3.4 (Auto-Update) — butuh endpoint `rezure-dashboard` siap, cocok dikerjakan setelah backend v2 matang