# Rezure (Desktop App) — v2: Telemetry Client

Breakdown fase dan task untuk menambahkan kemampuan pengiriman telemetry di sisi **Rezure desktop app**. Proyek ini terpisah dari `rezure-dashboard`, berkomunikasi hanya lewat kontrak API (lihat `docs/telemetry-contract.md`).

---

## Fase 2.1 — Feedback / Bug Report Menu

**Tujuan:** User bisa mengirim feedback atau laporan error dengan jelas, termasuk lampiran file, langsung dari dalam app.

> Catatan: menu ini di-rename dari "Support" menjadi **"Feedback"** agar tidak bertabrakan dengan menu **"Support Developer"** (donasi) yang direncanakan di v3.

### Tasks

- [ ] Tambahkan menu "Feedback" di sidebar
- [ ] Buat form pengiriman ticket: judul, deskripsi, kategori (Bug Report / Feature Request / General Feedback)
- [ ] Implementasi upload lampiran file (screenshot, log file) — validasi tipe & ukuran file di sisi client sebelum kirim
- [ ] Auto-attach info sistem opsional (versi Rezure, versi OS) supaya laporan bug lebih informatif tanpa user perlu ketik manual
- [ ] Tombol "Lampirkan log terbaru" — ambil log dari service yang error (integrasi dengan log viewer yang sudah ada di v1)
- [ ] Kirim ticket via `POST /api/v1/support/tickets` (multipart form-data) ke `rezure-dashboard`
- [ ] Tampilkan status pengiriman jelas (loading, sukses, gagal) — ini aksi yang di-inisiasi user secara langsung, jadi feedback harus real-time, beda dari telemetry yang silent di background
- [ ] Tangani kegagalan kirim dengan baik (retry manual oleh user, atau simpan draft lokal supaya tidak hilang)
- [ ] (Opsional) Halaman riwayat ticket yang pernah dikirim user beserta statusnya (Open/In Progress/Resolved), diambil dari `GET /api/v1/support/tickets?device_id=...`

---

## Fase 2.2 — Device Identity & Local Foundation

**Tujuan:** Rezure bisa mengidentifikasi dirinya sendiri secara unik dan siap mencatat data (belum terhubung ke server).

### Tasks

- [ ] Implementasi generate `device_id` (UUID v4) saat pertama kali app dibuka
- [ ] Simpan `device_id` di local config (persist, tidak berubah selama app tidak di-reset/uninstall)
- [ ] Buat tabel lokal SQLite `pending_events` (kolom: `id`, `payload`, `type` [event/heartbeat], `created_at`, `sent_at`)
- [ ] Buat modul `TelemetryClient` di Rust sebagai titik masuk tunggal untuk mencatat data (`record_event()`, `record_heartbeat()`)
- [ ] Tambahkan toggle "Share anonymous usage data" di halaman Settings
- [ ] Pastikan saat toggle off, `TelemetryClient` tidak mencatat apapun ke antrian lokal

---

## Fase 2.3 — Sending Logic

**Tujuan:** Data dari antrian lokal terkirim ke API Laravel dengan aman, tanpa mengganggu UX.

### Tasks

- [ ] Implementasi heartbeat berkala (interval 5-10 menit) selama app aktif
- [ ] Implementasi pengiriman batch: ambil beberapa event dari `pending_events`, kirim sebagai satu request
- [ ] Jalankan pengiriman di background task (`tokio::spawn`), non-blocking terhadap UI
- [ ] Set timeout pendek untuk request (misal 5 detik)
- [ ] Tandai event sebagai `sent_at` setelah sukses terkirim; bersihkan/retensi terbatas untuk event yang sudah terkirim
- [ ] Retry logic sederhana untuk event yang gagal terkirim (exponential backoff atau retry di siklus berikutnya)
- [ ] Implementasi HTTP client (`reqwest`) untuk memanggil endpoint ingest sesuai kontrak API

---

## Fase 2.4 — Event Instrumentation (Scope Terbatas Dulu)

**Tujuan:** Mulai mencatat event nyata dari fitur yang paling penting, bukan semua fitur sekaligus.

### Tasks

- [ ] Instrumentasi event `app_opened`
- [ ] Instrumentasi event `service_started` (metadata: nama service, versi)
- [ ] Instrumentasi event `service_stopped`
- [ ] Uji manual: pastikan event tercatat di `pending_events` saat aksi dilakukan
- [ ] Uji skenario offline → online: pastikan event tetap terkirim setelah koneksi kembali

---

## Fase 2.5 — Hardening (Sisi Client)

**Tujuan:** Pastikan fitur telemetry tidak mengganggu pengalaman inti Rezure.

### Tasks

- [ ] Review: pastikan tidak ada data selain yang direncanakan yang ikut tercatat/terkirim
- [ ] Pastikan opt-out benar-benar menghentikan seluruh pencatatan, bukan hanya pengiriman
- [ ] Pastikan kegagalan pengiriman (API down, tidak ada internet) tidak memunculkan error yang mengganggu user
- [ ] Dokumentasikan payload yang dikirim di `docs/telemetry-contract.md`

---

## Fase 2.6 — Changelog Menu

**Tujuan:** User bisa lihat riwayat rilis Rezure langsung dari dalam app.

### Tasks

- [ ] Tambahkan menu "Changelog" di sidebar
- [ ] Buat UI list changelog: versi, tanggal, ringkasan perubahan (dikelompokkan per versi)
- [ ] Implementasi fetch data dari endpoint `GET /api/v1/changelog` (`rezure-dashboard`)
- [ ] Cache hasil fetch secara lokal (SQLite/file) supaya tetap bisa dibuka saat offline
- [ ] Tambahkan badge notifikasi kecil di menu jika ada entry baru yang belum dibaca (bandingkan versi terakhir dibaca vs data terbaru dari API)
- [ ] Handle graceful saat API tidak bisa diakses (tampilkan data cache terakhir, jangan error mengganggu)

---

## Dependency ke Proyek Lain

Fase 2.1 (Support/Ticket) membutuhkan endpoint `POST /api/v1/support/tickets` sudah tersedia di `rezure-dashboard`. Fase 2.3 dan seterusnya membutuhkan endpoint `POST /api/v1/telemetry/ingest` sudah tersedia di `rezure-dashboard`. Fase 2.2 bisa dikerjakan independen tanpa menunggu backend siap. Fase 2.6 membutuhkan endpoint `GET /api/v1/changelog` sudah tersedia.

## Urutan Pengerjaan yang Disarankan

1. Fase 2.1 (Support/Ticket — bisa duluan karena tidak bergantung pada telemetry, hanya butuh endpoint ticket dari `rezure-dashboard`)
2. Fase 2.2 (independen, tidak perlu menunggu backend)
3. Fase 2.3 (butuh endpoint telemetry dari `rezure-dashboard` sudah siap untuk diuji end-to-end)
4. Fase 2.4
5. Fase 2.5
6. Fase 2.6 (butuh endpoint changelog dari `rezure-dashboard` sudah siap)
