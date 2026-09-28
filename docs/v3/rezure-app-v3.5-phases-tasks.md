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
- Begitu terminal dibuka dari project card: `services::node::terminal_bin_dir` resolve versi efektifnya (pin project → fallback versi aktif global), ketemu folder isi `node.exe`/`npm`/`npx`-nya lewat `services::node::bin_dir_for`, lalu folder itu di-*prepend* ke `PATH` **hanya untuk proses terminal yang baru di-spawn** — tidak pernah menulis ke PATH sistem atau PATH proses Rezure sendiri. Sejak itu PHP ikut pola yang sama (lihat v3 Fase 3.11, task "Terminal dari project card ikut versi PHP yang dipin").

### Tasks
- [x] Deteksi versi Node.js yang sudah terinstall/terbundle di sistem — `services::node::list()`/`installed()`, delegasi ke `binaries::discover("node", "node.exe")` yang sudah ada dari Fase 3.10
- [x] Bundle Node.js sebagai portable binary, download on-demand — sudah selesai dari Fase 3.10 (`node_catalog.rs`), tidak diulang di sini
- [x] Implementasi switch versi aktif secara global — `services::node::set_active`/`active_id` (mirror `services::php`), dipersist ke `Settings::active_node_version`, direstore saat startup (`lib.rs`). **Beda dari PHP**: tidak ada restart service (Node tidak punya service yang jalan) dan, untuk sekarang, tidak ada link PATH sistem-wide ala "PHP Everywhere" (`php_path.rs`) — switch cuma mengubah apa yang di-resolve `open_terminal` untuk terminal berikutnya
- [x] Implementasi switch versi aktif per-project — migrasi `projects.node_version` (nullable), `db::projects::fetch_node_versions`/`set_node_version`/`node_version_for`, command `set_project_node_version`. **Tidak** melakukan resync vhost/nginx sama sekali (beda dari `set_project_php_version`) — murni tulis SQLite, efeknya baru kepakai begitu terminal berikutnya dibuka
- [x] UI: dropdown pilih versi Node.js di halaman Switch — row `RuntimeSwitchRow` yang sebelumnya cuma label sekarang switch beneran (`@select="nodeStore.setActive"`). Tombol pin per-project baru di `ProjectActionButtons.vue` (ikon hex, sebelah tombol pin PHP), buka `ProjectNodeVersionModal.vue` (clone `ProjectPhpVersionModal.vue`)
- [x] Tampilkan versi npm yang ikut terbundle bersama tiap versi Node.js — dua sumber, tanpa spawn proses: **katalog** baca field `npm` langsung dari `nodejs.org/dist/index.json` (`NodeRelease::npm`), **versi terinstall** baca `node_modules/npm/package.json` di samping `node.exe` lewat `services::node::npm_version_in` (`NodeVersionStatus::npm`) — hasilnya sama dengan `npm -v`, tapi tanpa satu proses per versi tiap kali halaman Switch dibuka. Dua-duanya `Option`: file/field yang hilang cuma menyembunyikan detail, tidak menggagalkan listing. Tampil di modal Install version (`CatalogVersionList`, "Released … · npm …"), row Node.js di halaman Switch (label aktif + dropdown, lewat field generik `RuntimeVersionEntry.detail`), dan modal pin per-project (`ProjectNodeVersionModal`). Diverifikasi lawan data nyata: test `#[ignore]` `print_installed` (disk) dan `fetches_the_real_index` (nodejs.org) sama-sama memberi npm yang cocok, mis. Node 16.20.2 → npm 8.19.4. **Belum** dilihat langsung di UI app sungguhan

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
- [x] Implementasikan Mailpit dengan `Service` trait yang sudah ada — bukan tipe service baru,
      tapi `ProcessService::mailpit` dengan `Launch::Mailpit` (pola yang sama dengan nginx/PHP/
      database), terdaftar di `process::real_services`. Dijalankan dengan
      `--smtp 127.0.0.1:1025 --listen 127.0.0.1:8025 --database data/mailpit/mailpit.db`
      (file, bukan database sementara bawaan Mailpit — email yang tertangkap tetap ada setelah
      restart), `--smtp-auth-accept-any --smtp-auth-allow-insecure` (`.env` yang masih berisi
      kredensial provider asli tetap bisa kirim tanpa diubah selain host/port) dan
      `--disable-version-check` (versi binary dikelola Rezure, bukan banner update Mailpit).
      `restarts_on_crash` sengaja tetap `false`, sama seperti nginx
- [x] Binary portable, download on-demand — entry `mailpit` 1.31.3 di `binaries::MANIFEST`
      (`mailpit-windows-amd64.zip` dari GitHub Releases, SHA-256 = digest yang dipublikasikan
      GitHub untuk asset itu, dicocokkan ulang dengan hash hasil unduhan sendiri). Satu `.exe`
      tanpa DLL. **Tidak** ikut di-bundle ke installer — opsional, beda dengan nginx/PHP default
- [x] Mailpit di list Services — `ServiceInfo` dapat field generik baru (bukan kasus khusus id
      `mailpit`): `installed`, `installId` (paket manifest yang bikin service bisa di-start, untuk
      nginx/Mailpit; `null` untuk PHP/database yang versinya dipilih di halaman lain), `ports`
      (semua port yang di-bind) dan `webUrl`. Kartu yang belum terinstall menampilkan "Not
      installed" + tombol **Install** (dengan progress unduhan) alih-alih Start, dan "Start all"
      melewatinya alih-alih gagal
- [x] Deteksi port conflict untuk kedua port — `ProcessService::ports_for` mengecek SMTP dan web UI
      sebelum spawn; `ServiceRow.vue` mencari pemegang port di `service.ports` (daftar yang sama
      dipakai worker PHP), jadi bentrok di 8025 ditelusuri sama baiknya dengan di 1025
- [x] Log viewer — `mailpit` masuk `LOG_SERVICES`; stdout/stderr-nya sudah lewat `LogSink` yang
      sama dengan service lain
- [x] Tombol **Open** di kartu saat running → command `open_service_ui(id)`, yang mengambil URL dari
      service itu sendiri di Rust (frontend tidak pernah mengirim URL). Error jelas kalau service
      belum jalan (`ServiceNotRunning`) atau tidak punya web UI (`NoWebUi`)
- [x] (Opsional) Requirements check — `doctor::mail_setup` membaca `MAIL_MAILER` (atau
      `MAIL_DRIVER` lama), `MAIL_HOST`, `MAIL_PORT` dari `.env` dengan aturan phpdotenv (definisi
      pertama menang, `export`, kutip, komentar ` #`). Hanya `smtp` ke host lokal yang dianggap;
      `log`/provider asli dibiarkan. Command `diagnose_project` mengisi status Mailpit dari
      `ServiceManager`. Modal menampilkan: tertangkap Mailpit (+ Open inbox) · Mailpit mati (Start /
      Install and start) · port bukan 1025 (mis. `2525` bawaan `.env.example` Laravel → sarankan
      `MAIL_PORT=1025`) · `MAIL_HOST=mailpit` hostname Docker Sail yang tidak resolve di luar
      Docker (→ sarankan `127.0.0.1`)

### Verifikasi

- `cargo test` (368 lolos), `cargo clippy --all-targets -D warnings`, `cargo fmt`, `npm run lint`,
  `npm run type-check` — semua bersih
- Test `#[ignore]` `a_real_mailpit_catches_a_message` lawan binary asli (id dan port sendiri):
  SMTP dengan `AUTH PLAIN` kredensial asal diterima, pesan muncul di `/api/v1/messages`, dan
  `mailpit.db` tertulis di folder data Rezure
- Laravel 13 sungguhan (`laravel-api`, Symfony Mailer, `MAIL_USERNAME`/`MAIL_PASSWORD` terisi)
  mengirim `Mail::raw` ke Mailpit → pesan tertangkap
- `ServiceRow.vue` dan `ProjectDoctorModal.vue` asli dirender di Edge headless dengan `invoke`
  di-mock: kartu Not installed/Install, Installing…, Running + Open (memanggil
  `open_service_ui`), dan empat keadaan bagian mail di requirements check
- App dev (`tauri dev`) maintainer sudah menjalankan Mailpit hasil kode ini dengan argumen di atas
- **Belum**: tombol Install diklik di jendela app sungguhan (unduhan nyata lewat `AppHandle`)

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

1. Fase 3.5.3 (Mailpit) — **selesai**; lihat catatan lengkap di Fase 3.5.3 di atas
2. Fase 3.5.1 (Node.js switcher) — **selesai**, dikerjakan lebih dulu di luar urutan ini; lihat catatan lengkap di Fase 3.5.1 di atas
