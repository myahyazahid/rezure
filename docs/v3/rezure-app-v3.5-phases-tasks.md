# Rezure (Desktop App) — v3.5: Additional Language/Service Support

Fase tambahan di antara v3 dan v4, fokus menambah dukungan Node.js version switching serta Mailpit sebagai service baru — memanfaatkan fondasi `Service` trait yang sudah ada, tanpa perlu sistem plugin dinamis. Redis (bekas Fase 3.5.2 di file ini) dipindah ke v4 atas permintaan maintainer — lihat `docs/v4/rezure-app-v4-phases-tasks.md`.

---

## Fase 3.5.1 — Node.js Version Switching

**Tujuan:** User bisa switch versi Node.js aktif, mirip cara kerja PHP version switcher yang sudah ada.

**Dikerjakan lebih dulu, di luar urutan yang disarankan** di bagian bawah dokumen ini (harusnya nomor 3, setelah Redis/Mailpit) — permintaan langsung maintainer, bukan karena ketergantungan teknis ke Fase 3.5.2/3.5.3.

### Cara kerja (ringkas)

Beda dari PHP: Node **tidak** dijalankan Rezure sebagai service yang terus nyala (tidak ada proses yang di-proxy nginx), jadi tidak butuh pool/port ala `services::php_pool` (v3 Fase 3.11). Node dipakai ephemeral — lewat terminal yang dibuka dari project card, `npm install`, dev server yang user start sendiri — jadi "pakai versi tertentu buat satu project" cukup soal PATH resolution di titik `services::launcher::open_terminal` men-spawn terminal baru, bukan soal proses yang harus tetap hidup.

- **Versi aktif global** (`services::node`, `OnceLock<Mutex<String>>` — pola identik `services::php`) dipakai project yang tidak pin apa-apa.
- **`ProjectInfo.node_version`** (kolom SQLite baru, `NULL` = ikut default) — project bisa pin versi sendiri, tidak menyentuh proses/service apapun.
- Begitu terminal dibuka dari project card: `commands::projects::resolve_node_bin_dir` resolve versi efektifnya (pin project → fallback versi aktif global), ketemu folder isi `node.exe`/`npm`/`npx`-nya lewat `services::node::bin_dir_for`, lalu folder itu di-*prepend* ke `PATH` **hanya untuk proses terminal yang baru di-spawn** — tidak pernah menulis ke PATH sistem atau PATH proses Rezure sendiri.

### Tasks
- [x] Deteksi versi Node.js yang sudah terinstall/terbundle di sistem — `services::node::list()`/`installed()`, delegasi ke `binaries::discover("node", "node.exe")` yang sudah ada dari Fase 3.10
- [x] Bundle Node.js sebagai portable binary, download on-demand — sudah selesai dari Fase 3.10 (`node_catalog.rs`), tidak diulang di sini
- [x] Implementasi switch versi aktif secara global — `services::node::set_active`/`active_id` (mirror `services::php`), dipersist ke `Settings::active_node_version`, direstore saat startup (`lib.rs`). **Beda dari PHP**: tidak ada restart service (Node tidak punya service yang jalan) dan, untuk sekarang, tidak ada link PATH sistem-wide ala "PHP Everywhere" (`php_path.rs`) — switch cuma mengubah apa yang di-resolve `open_terminal` untuk terminal berikutnya
- [x] Implementasi switch versi aktif per-project — migrasi `projects.node_version` (nullable), `db::projects::fetch_node_versions`/`set_node_version`/`node_version_for`, command `set_project_node_version`. **Tidak** melakukan resync vhost/nginx sama sekali (beda dari `set_project_php_version`) — murni tulis SQLite, efeknya baru kepakai begitu terminal berikutnya dibuka
- [x] UI: dropdown pilih versi Node.js di halaman Switch — row `RuntimeSwitchRow` yang sebelumnya cuma label sekarang switch beneran (`@select="nodeStore.setActive"`). Tombol pin per-project baru di `ProjectActionButtons.vue` (ikon hex, sebelah tombol pin PHP), buka `ProjectNodeVersionModal.vue` (clone `ProjectPhpVersionModal.vue`)
- [ ] Tampilkan versi npm yang ikut terbundle bersama tiap versi Node.js — **belum dikerjakan**, di luar cakupan sesi ini

### Bug ditemukan lewat pemakaian nyata, sudah diperbaiki

**Windows Terminal mengabaikan PATH override kalau sudah ada window Terminal yang jalan.** Implementasi awal menyetel PATH lewat `Command::env` pada `wt.exe` yang di-spawn `services::launcher::spawn_terminal`. Windows Terminal ternyata punya satu proses "monarch" yang memegang semua window; begitu sudah ada window yang jalan (kasus normal — jarang ada nol window Terminal terbuka), invocation `wt.exe` yang baru cuma jadi "peasant" yang menitip pesan ke monarch lalu keluar, dan tab barunya di-spawn memakai environment proses **monarch** (dari saat dia pertama kali start), bukan environment yang baru kita set di peasant. Efeknya: project dipin ke Node 16, buka terminal, `node -v` malah menunjuk ke Node lain yang kebetulan ada di PATH sistem — override-nya diam-diam tidak kepakai sama sekali. Ditemukan langsung dari testing manual maintainer (screenshot `node -v` menunjukkan versi yang tidak cocok dengan pin maupun versi aktif global).

Fix: `spawn_terminal` sekarang melewati `wt.exe` sepenuhnya kalau ada PATH override yang perlu diterapkan, langsung memakai fallback `cmd`-spawn (proses yang di-spawn Rezure sendiri dari awal sampai akhir, sehingga environment-nya tidak mungkin "tertelan" proses lain). Konsekuensinya: begitu user punya versi Node aktif (otomatis terjadi begitu ada 1+ versi terinstall), terminal dari project card menjadi jendela `cmd` polos, bukan tab Windows Terminal — trade-off yang diambil sadar demi PATH yang benar-benar benar, bukan yang kelihatan benar saja. Tanpa override (belum ada Node terinstall/aktif sama sekali), `wt.exe` tetap dipakai seperti sebelumnya.

**Belum diverifikasi ulang oleh maintainer setelah fix ini** — laporan bug awal datang dari testing manual sebelum fix ditulis; fix-nya sendiri baru lolos `cargo test`/`cargo clippy`/`cargo fmt`, belum dicoba ulang manual di app sungguhan.

### Catatan lain
- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test --lib` (semua test lolos, termasuk test baru untuk `services::node`, `db::projects` bagian Node, dan `services::launcher`), `npm run lint`, `npm run type-check` — semuanya bersih
- Tidak ada perubahan yang menyentuh vhost/nginx sama sekali untuk fitur ini — PHP sepenuhnya tidak tersentuh

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

Fitur ini **tidak membutuhkan sistem plugin dinamis**. Fondasi `Service` trait yang sudah ada di `docs/architecture.md` sudah cukup — menambah service baru cukup dengan mengimplementasikan trait tersebut dan compile ulang. Sama seperti Redis (sekarang v4 Fase 4.6), sistem plugin (dynamic loading tanpa compile ulang) baru relevan jika suatu saat ingin membuka jalur bagi kontributor/komunitas luar menambah service tanpa menyentuh source code inti — itu dipertimbangkan terpisah, bukan prasyarat untuk fase ini.

---

## Dependency ke Proyek Lain

Tidak ada — kedua fase yang tersisa di file ini (3.5.1, 3.5.3) sepenuhnya independen dari `rezure-dashboard` maupun `rezure-website`.

## Urutan Pengerjaan yang Disarankan

Urutan aslinya menaruh Node.js switcher paling belakang karena dianggap paling kompleks di antara
tiga fase yang tadinya ada di file ini. **Fase 3.5.1 sudah selesai duluan** atas permintaan
maintainer, di luar urutan ini. **Fase 3.5.2 (Redis) sudah dipindah ke v4** (jadi Fase 4.6) — lihat
`docs/v4/rezure-app-v4-phases-tasks.md`. Yang tersisa di file ini:

1. Fase 3.5.3 (Mailpit) — pola implementasinya sama persis dengan Redis yang sudah dipindah ke v4; nilai tambah harian buat user paling terasa dari fase-fase yang tersisa di file ini — **belum dikerjakan**
2. Fase 3.5.1 (Node.js switcher) — **selesai**, dikerjakan lebih dulu di luar urutan ini; lihat catatan lengkap di Fase 3.5.1 di atas
