# Rezure (Desktop App) — v3.5: Additional Language/Service Support

Fase tambahan di antara v3 dan v4, fokus menambah dukungan Node.js version switching serta Redis dan Mailpit sebagai service baru — memanfaatkan fondasi `Service` trait yang sudah ada, tanpa perlu sistem plugin dinamis.

---

## Fase 3.5.1 — Node.js Version Switching

**Tujuan:** User bisa switch versi Node.js aktif, mirip cara kerja PHP version switcher yang sudah ada.

### Tasks
- [ ] Deteksi versi Node.js yang sudah terinstall/terbundle di sistem
- [ ] Bundle Node.js sebagai portable binary, download on-demand saat user memilih versi tertentu (mengikuti pola yang sama dengan PHP)
- [ ] Implementasi switch versi aktif secara global
- [ ] Implementasi switch versi aktif per-project (opsional, jika arsitektur project settings sudah mendukung override per-project)
- [ ] UI: dropdown pilih versi Node.js di halaman yang sama dengan PHP version switcher
- [ ] Tampilkan versi npm yang ikut terbundle bersama tiap versi Node.js

---

## Fase 3.5.2 — Redis Support

**Tujuan:** Redis tersedia sebagai service baru di Rezure, konsisten dengan service lain yang sudah ada.

### Tasks
- [ ] Implementasikan Redis dengan `Service` trait yang sudah ada (`start()`, `stop()`, `status()`, `restart()`) — tidak perlu sistem baru
- [ ] Bundle Redis portable binary untuk Windows
- [ ] Tambahkan Redis ke list service di Dashboard/Services (port default `6379`)
- [ ] Deteksi port conflict untuk Redis, konsisten dengan service lain
- [ ] Log viewer Redis mengikuti pola log viewer service lain yang sudah ada
- [ ] (Opsional, bisa nyusul) Mini Redis viewer — quick-view keys yang tersimpan, tanpa perlu tool eksternal (RedisInsight, dll)

---

## Fase 3.5.3 — Mailpit (Local Mail Catcher)

**Tujuan:** Setiap project Laravel/PHP bisa nangkep email yang dikirim lewat SMTP secara lokal,
tanpa perlu akun mail provider beneran — gap yang paling kentara dibanding Laragon (Mailpit ada
di sidebar Laragon, port `1025`/`8025` bahkan). Default `.env` Laravel selalu SMTP, dan "email-nya
beneran kekirim gak" itu pertanyaan harian buat siapapun yang develop fitur auth/notifikasi.

### Tasks
- [ ] Implementasikan Mailpit dengan `Service` trait yang sudah ada (`start()`, `stop()`, `status()`, `restart()`) — pola identik dengan Fase 3.5.2 (Redis)
- [ ] Bundle Mailpit portable binary untuk Windows, download on-demand mengikuti pola binary lain
- [ ] Tambahkan Mailpit ke list service di Dashboard/Services — port SMTP default `1025`, web UI default `8025`
- [ ] Deteksi port conflict untuk kedua port itu, konsisten dengan service lain
- [ ] Log viewer Mailpit mengikuti pola log viewer service lain yang sudah ada
- [ ] Tombol/link "Open Mailpit" dari Dashboard yang langsung buka `http://127.0.0.1:8025` di browser
- [ ] (Opsional) Requirements check di `doctor.rs` bisa kasih catatan: kalau project punya `MAIL_MAILER=smtp` dan `MAIL_HOST=127.0.0.1` di `.env`, sarankan aktifin Mailpit kalau belum jalan

---

## Catatan Arsitektur

Kedua fitur ini **tidak membutuhkan sistem plugin dinamis**. Fondasi `Service` trait yang sudah ada di `docs/architecture.md` sudah cukup — menambah service baru cukup dengan mengimplementasikan trait tersebut dan compile ulang. Sistem plugin (dynamic loading tanpa compile ulang) baru relevan jika suatu saat ingin membuka jalur bagi kontributor/komunitas luar menambah service tanpa menyentuh source code inti — itu dipertimbangkan terpisah, bukan prasyarat untuk fase ini.

---

## Dependency ke Proyek Lain

Tidak ada — ketiga fase ini sepenuhnya independen dari `rezure-dashboard` maupun `rezure-website`.

## Urutan Pengerjaan yang Disarankan

1. Fase 3.5.2 (Redis) — lebih sederhana karena polanya sudah sangat mirip service yang sudah ada (Apache/MySQL)
2. Fase 3.5.3 (Mailpit) — pola implementasinya sama persis dengan Redis, tinggal diikuti; nilai tambah harian buat user paling terasa dari ketiga fase ini
3. Fase 3.5.1 (Node.js switcher) — sedikit lebih kompleks karena menyentuh logic version-switching yang perlu konsisten dengan PHP switcher yang sudah ada
