# Rezure (Desktop App) — v3: Advanced Features

Roadmap fitur lanjutan Rezure. Beberapa fitur yang sebelumnya direncanakan di v3 (Quick App Installer, Auto HTTPS/mkcert, Integrated Terminal, Embedded Database GUI) **sudah tersedia** di rilis sebelumnya — tidak dimasukkan lagi di sini.

---

## Fase 3.1 — One-click Tunneling

**Tujuan:** User bisa share local project ke internet sementara tanpa setup manual.

### Tasks
- [x] Integrasi ngrok atau cloudflared (pilih salah satu, atau keduanya sebagai opsi) — cloudflared dipilih (`services/share.rs`), ngrok dilewati karena butuh signup/authtoken
- [x] Deteksi apakah binary tunnel tool sudah tersedia; download portable version jika belum ada — `share::ensure_installed`, checksum-verified
- [x] Tombol "Share" di tiap project card, trigger tunnel ke port project tersebut — `ProjectActionButtons.vue`
- [x] Tampilkan public URL yang di-generate langsung di UI, dengan tombol copy — `ProjectShareModal.vue`
- [x] Tombol "Stop sharing" untuk menutup tunnel — `ProjectShareModal.vue` → `stopSharing`

---

## Fase 3.1b — PECL Extension Installer (redis)

**Tujuan:** User bisa memasang ekstensi yang tidak ikut di zip resmi PHP — `redis` lebih dulu —
langsung dari dalam app, tanpa berburu DLL.

> **Catatan:** implementasinya sudah ditulis dan lolos test saat mengerjakan feedback user, lalu
> ditunda ke v3 supaya rilis v2 tetap bersih. Lihat `services/php_ext.rs` beserta tombol Install di
> requirements check. Yang tersisa hanyalah keputusan rilis, bukan pekerjaan implementasi.

### Tasks

- [x] Katalog PECL dengan SHA-256 di-pin per branch PHP (7.4—8.5), karena area PECL php.net tidak
      menyediakan index checksum apa pun
- [x] Unduh lewat jalur terverifikasi yang sama dengan runtime lain (`binaries::install_archive`)
- [x] Ekstrak hanya DLL-nya ke `ext/` versi terkait; arsipnya juga berisi README/LICENSE/`.pdb`
- [x] Aktifkan untuk web (ini generated) dan untuk terminal (baris aditif di ini versi)
- [x] Tombol Install di requirements check project, lalu cek ulang otomatis
- [ ] Uji klik pertama di app sungguhan (unduhan nyata lewat `AppHandle`, tidak bisa headless)
- [ ] Putuskan cakupan katalog berikutnya (`imagick`? `xdebug`?) sebelum daftarnya jadi beban rawat

---

## Fase 3.6 — Bundled PHP Extension Toggle

**Tujuan:** UI buat nyalain/matiin ekstensi PHP yang **sudah ikut** di dalam zip resmi versi
aktif (`bz2`, `sodium`, `exif`, `xsl`, `sockets`, dst) — beda dari Fase 3.1b yang soal ekstensi
di luar zip (PECL, `redis`). Sekarang cuma 12 ekstensi default yang di-auto-enable `php_ini.rs`;
selebihnya harus edit manual `conf.d/*.ini`. Konsepnya terinspirasi dari toggle ekstensi ala
Laragon, tapi tampilannya **mengikuti design system Rezure sendiri** — bukan meniru list
checkbox polos Laragon — konsisten dengan Tailwind styling dan pola card/panel modern yang
sudah dipakai di `PhpConfigCard.vue` dan `RuntimeSwitchRow.vue`.

### Tasks
- [ ] Scan `ext/*.dll` di folder versi PHP aktif, cocokkan ke tabel nama+deskripsi ekstensi
- [ ] Simpan pilihan eksplisit user di file state terpisah per-versi PHP (mis. `data/php/<version>/extensions.json`) — **bukan** di `conf.d`, supaya folder itu tetap murni punya user sesuai yang didokumentasikan di [`php-versions.md`](php-versions.md#configuring-php)
- [ ] `php_ini::render` digabung: default list ∪ user-enabled − user-disabled, tetap difilter ke DLL yang benar-benar ada di build itu (yang tidak ada ditampilkan disabled/abu-abu, bukan seolah tersedia)
- [ ] Command Tauri tipis: `list_php_extensions`, `set_php_extension_enabled` — delegasikan logic ke service, ikuti pola restart-langsung yang sama seperti switch versi
- [ ] UI: card/panel baru dengan gaya visual Rezure sendiri (bukan tabel checkbox mentah) — grouping per kategori (database, network, format, dsb), search/filter ringan, badge "tidak tersedia" untuk DLL yang gak ada di build ini
- [ ] Default off + tooltip penjelasan untuk entry debug-only/environment-dependent (`zend_test`, `phpdbg_webhelper`, `oci8`/`pdo_oci`) — jangan diperlakukan sama seperti ekstensi biasa
- [ ] Notice "restart PHP" saat toggle dilakukan sementara service sedang jalan

---

## Fase 3.3 — Project Health Dashboard

**Tujuan:** Ringkasan kondisi tiap project dalam satu pandangan.

**Prasyarat sekarang sudah ada:** dulu gap terbesar fase ini adalah project belum punya konsep
"pakai service/port yang mana" — PHP versi cuma satu secara global, semua vhost proxy ke port yang
sama. [Fase 3.11](#fase-311--per-project-php-version-concurrent) menutup gap itu: `ProjectInfo`
sekarang punya `php_version` sendiri per-project, dan `services::php_pool` mapping versi ke port.
Konsolidasi status di bawah ini bisa langsung baca dari situ, bukan mulai dari nol.

### Tasks
- [ ] Konsolidasi status semua service terkait per-project dalam satu view
- [ ] Port conflict detector yang lebih menyeluruh (across semua project, bukan cuma saat start service)
- [ ] Tampilkan ukuran log per service, dengan opsi clear log
- [ ] Indikator visual sederhana (misal: sehat/perlu perhatian) berdasarkan status gabungan

---

## Fase 3.4 — Auto-Update Mechanism

**Tujuan:** Distribusi update tidak lagi manual — user diberi tahu dan bisa update langsung dari dalam app.

### Tasks
- [x] Implementasi `tauri-plugin-updater` — dependency + registrasi plugin (`src-tauri/src/lib.rs`), capability `updater:default`, `bundle.createUpdaterArtifacts` di `tauri.conf.json`. Tidak ada command Tauri custom — seluruh alur `check`/`downloadAndInstall` dipanggil langsung dari Pinia store (`src/stores/update.ts`) lewat `@tauri-apps/plugin-updater`, bukan diwrap ulang di Rust
- [x] Cek update via `check()` plugin resmi, terhadap manifest bertanda tangan (ed25519) sesuai kontrak di [`docs/version-contract.md`](../version-contract.md) — **bukan** sekadar `{version, download_url}` seperti sketsa awal: checksum dari server yang sama dengan file yang didownload gak ngasih jaminan integritas apa-apa, signature yang diverifikasi pakai public key yang ketanam di app baru bener-bener independen dari server
- [x] Notifikasi in-app saat ada versi baru tersedia — badge titik merah di item sidebar "Changelog" (`AppSidebar.vue`), sinyal terpisah dari "changelog belum dibaca" (dua hal ini independen: update bisa ada padahal entry changelog-nya udah keklik `seen`, atau sebaliknya)
- [x] Link notifikasi ke halaman Changelog (menu yang sudah ada dari v2) untuk detail perubahan — banner "Update" muncul langsung di `ChangelogView.vue`
- [x] Alur update: **satu tombol "Update"** di halaman Changelog memicu seluruh urutan (download dengan progress terlihat → install → di Windows app keluar sendiri setelah installer jalan) — bukan background pre-download + konfirmasi terpisah, sesuai keputusan produk final (one-click-does-it-all, konsisten sama pola tombol Share/Install PHP version yang udah ada)
- [x] Generate keypair `tauri signer generate` (sekali, manual) — public key sudah masuk `tauri.conf.json` menggantikan placeholder; private key + password disimpan maintainer di luar repo (bukan file, ditampilkan sekali di terminal), tidak pernah masuk git
- [x] `laravel-api` ubah response `GET /api/v1/version/latest` dari `{version, notes, published_at}` ke manifest bertanda tangan (`pub_date` + `platforms.windows-x86_64.{signature,url}`) — `VersionController` (`app/Http/Controllers/Api/V1/VersionController.php`) sudah mengembalikan shape ini persis sesuai `docs/version-contract.md`, kolom `signature`/`download_url` sudah ada di tabel `releases` (migrasi `2026_09_15_042915_add_signature_and_download_url_to_releases_table.php`), dan `204` dibalikin saat client sudah current
- [ ] Uji end-to-end lawan endpoint asli — masih tersisa karena belum ada pipeline rilis nyata: repo ini belum punya workflow CI (`.github/workflows`) yang build+sign installer pakai key barusan, jadi belum ada release sungguhan berisi `signature`/`download_url` untuk diuji. Sementara ini kode sisi app sudah bisa diuji lokal lawan manifest tiruan (lihat prosedur di `docs/version-contract.md`)

---

## Fase 3.5 — Support Developer / Donate Menu

**Tujuan:** User yang mau mendukung pengembangan Rezure bisa donasi dengan mudah, lewat berbagai platform (lokal, global, dan crypto).

### Tasks
- [x] Tambahkan menu "Support Developer" di sidebar (terpisah dari menu "Feedback" di Fase 2.1) — `/donate`, ikon hati, `AppSidebar.vue`
- [x] Section link donasi lokal: Trakteer / Saweria — tombol buka browser eksternal ke halaman donasi
- [x] Section link donasi global: GitHub Sponsors / Ko-fi — tombol buka browser eksternal
- [x] Section donasi crypto: tampilkan wallet address dengan tombol copy address dan QR code per wallet — QR digenerate client-side (`qrcode` package) dari address, gak pernah lewat layanan QR pihak ketiga
- [x] Pesan singkat konteks beserta link ke halaman "About" — `AboutView.vue` baru (`/about`), isi masih placeholder generik, nunggu cerita project asli dari maintainer
- [x] **Keputusan berubah dari sketsa awal roadmap**: link/alamat donasi **diambil dari API** (`GET /api/v1/support/donate`), bukan config statis di app — permintaan eksplisit user saat implementasi, memakai jalur pengecualian yang roadmap ini sendiri udah sediakan ("kecuali suatu saat ingin diubah dari server tanpa update app"). Pola fetch+cache+fallback sama persis kayak `services::changelog` (`services/donate.rs`). Endpoint-nya **sudah live** di `laravel-api` (`Api\V1\DonateController`) — spek lengkap di `api-documentation/telemetry-api.md` (`GET /support/donate`)

---

## Fase 3.7 — Xdebug sebagai Extension Resmi

**Tujuan:** Tutup pertanyaan terbuka di Fase 3.1b ("imagick? xdebug?") dengan menjadikan Xdebug
ekstensi PECL resmi yang bisa dipasang lewat app, plus konfigurasi step-debugging yang biasanya
jadi hambatan tersendiri di luar sekadar "install DLL"-nya.

### Tasks
- [ ] Tambahkan `xdebug` ke katalog `php_ext.rs` (SHA-256 per branch PHP, mengikuti pola `redis`)
- [ ] Xdebug beda dari extension biasa: butuh `zend_extension=xdebug` (bukan `extension=`), jadi `php_ini.rs` perlu jalur khusus buat baris ini
- [ ] UI konfigurasi dasar: `xdebug.mode` (off/debug/develop), `xdebug.client_port`, `xdebug.client_host` — bukan raw ini editor, cukup pilihan umum yang paling sering dipakai
- [ ] Auto-generate `.vscode/launch.json` di root project saat Xdebug diaktifkan untuk project itu (kalau folder `.vscode` belum ada/belum punya konfigurasi PHP debug) — nilai tambah yang gak ditawarkan Laragon maupun kompetitor lain
- [ ] Peringatan performa: aktif tapi `xdebug.mode=off` tetap ada overhead loading modul — jelaskan di UI, jangan nyalain semua mode sekaligus by default

---

## Fase 3.8 — Queue Worker Supervision

**Tujuan:** `php artisan queue:work` gak lagi jadi proses yang ditinggal manual di satu terminal
yang gampang ke-close atau kelupaan — disupervisi persis kayak service lain.

### Tasks
- [ ] Manfaatkan infra process management yang sama dengan `Service` trait (start/stop/restart, log lewat `ServiceLogPanel.vue`)
- [ ] Scope per-project, bukan global — satu project bisa punya worker sendiri, jalan/berhenti independen dari project lain
- [ ] Deteksi otomatis project mana yang punya `artisan` (Laravel) sebagai syarat munculnya opsi ini
- [ ] UI: tombol "Start Queue Worker" di project card/detail (dekat tombol Open/Terminal yang sudah ada), dengan indikator running/stopped
- [ ] Opsi dasar: pilih koneksi queue (`--queue=default`, dst) kalau project punya lebih dari satu — sisanya pakai default `artisan`
- [ ] Worker ikut berhenti kalau PHP di-restart/di-switch versi (proses lama sudah tidak valid), dengan notice ke user — bukan dibiarkan jadi proses PHP versi lama yang nyangkut

---

## Fase 3.9 — Export/Import Workspace Config

**Tujuan:** Manfaatkan filosofi portable Rezure (semua di `C:\rezure`, config berbasis file/SQLite,
bukan registry) — pindah ke laptop baru atau nge-share setup ke tim gak perlu setup ulang manual
satu-satu.

### Tasks
- [ ] Export: kumpulkan project list (`links.json`), vhosts config, `conf.d/*.ini` milik user, dan `settings.json` jadi satu file arsip (`.zip`)
- [ ] **Tidak** menyertakan binary PHP/nginx/MariaDB itu sendiri (terlalu besar, dan sudah bisa di-download ulang) — cukup referensi versi yang dipakai, diunduh ulang di mesin tujuan kalau belum ada
- [ ] Import: baca arsip, tampilkan preview apa yang bakal ditambahkan/ditimpa sebelum eksekusi (terutama kalau ada domain yang bentrok dengan project yang sudah ada di mesin tujuan)
- [ ] Path absolut project (`C:\Users\...`) di mesin lama jelas gak valid di mesin baru — import harus nanya lokasi baru per project atau nawarin re-link manual, bukan gagal diam-diam
- [ ] Tombol Export/Import ditaruh di halaman Settings

---

## Fase 3.10 — Multi-Version Installer untuk Semua Runtime

**Tujuan:** Tombol "Install version" di halaman Switch berlaku untuk semua runtime, bukan cuma PHP.
Pembagian perannya tegas:

- **Dropdown di tiap row = switching saja** — cuma melistkan versi yang sudah terinstall.
- **Tombol "Install version" = satu-satunya jalur memasang versi baru.**

### Kondisi sekarang

- Tombol "Install version" langsung membuka `InstallPhpVersionModal` — PHP-only.
- Nginx/MariaDB/Composer sebenarnya bisa diinstall, tapi **hanya lewat dropdown row-nya**: row mengirim satu entry sintetis berstatus not-installed, lalu `pick()` di `RuntimeSwitchRow.vue` nge-branch ke `install`. Persis peran yang mau dihapus.
- Cuma PHP yang punya katalog multi-versi (`php_catalog.rs`). `binaries::MANIFEST` cuma pin **satu** versi nginx (1.25.3, rilis 2023) dan **satu** MariaDB (11.2.2); Composer cuma entry `'latest'`.
- PHP sendiri sudah patuh aturan di atas: versinya ditemukan dari disk, jadi semua yang dilist dropdown pasti sudah terinstall.

### Urutan yang wajib, bukan preferensi

Modal harus digeneralisasi **lebih dulu**, baru dropdown dibersihkan. Kalau dibalik, Nginx/MariaDB/Composer kehilangan satu-satunya jalur install yang mereka punya.

### Tasks
- [ ] Modal install jadi runtime-aware: pilih runtime dulu, lalu versi — menggantikan `InstallPhpVersionModal` yang PHP-only
- [ ] Angkat `php_catalog.rs` jadi abstraksi katalog per-runtime. Fondasinya sudah generic: `binaries::install_archive()` sudah dipakai bersama oleh php_catalog + php_ext, dan `binaries::discover(family, exe_name)` sudah per-family (malah sudah ada test untuk MariaDB) — yang perlu ditulis tinggal parser per sumber
- [ ] Dropdown jadi switch-only: `pick()` tidak lagi emit `install`, dan entry yang belum terinstall tidak dilistkan sama sekali
- [ ] Row tetap menampilkan progress bar install yang sedang jalan — install dimulai dari modal, modalnya boleh ditutup, progressnya dilaporkan row (pola yang sudah ada untuk PHP, tinggal disamakan untuk runtime lain)
- [ ] Runtime dengan 0 versi terinstall: dropdown-nya disabled dan mengarahkan ke tombol Install, bukan dropdown kosong yang bisa diklik
- [ ] Katalog MariaDB dari REST API `downloads.mariadb.org/rest-api/mariadb/<branch>/` — `sha256sum` sudah inline di response, filter `package_type: "ZIP file"` dan buang varian `-debugsymbols`
- [ ] Katalog Composer dari `getcomposer.org/versions` — checksum ada di sidecar `.sha256sum` per versi (fetch kedua)
- [ ] Katalog Node.js dari `nodejs.org/dist/index.json` + `SHASUMS256.txt` per versi — sekalian jadi fondasi Fase 3.5.1 (Node.js version switching)
- [ ] Nginx: lihat open question di bawah, jangan diikutkan sebelum diputuskan

### Catatan: MariaDB itu stateful

PHP/Nginx/Composer stateless — ganti versi cuma soal ganti binary. MariaDB punya datadir, dan turun versi major sering tidak mungkin sama sekali, jadi "switch versi MariaDB" bukan operasi simetris dengan "switch versi PHP".

Ini justru nyambung ke [`mysql-profile-switcher-spec.md`](../v1/mysql-profile-switcher-spec.md), yang sudah menyebut field `mysql_version` untuk "pick a compatible bundled binary" dan meminta form berisi "dropdown of bundled versions". Artinya multi-versi MariaDB adalah **prasyarat yang spec itu sudah asumsikan ada** — user yang mau mengadopsi datadir Laragon buatan MySQL 8.0.30 tidak akan terlayani oleh MariaDB 11.2.2 yang sekarang jadi satu-satunya.

### Open question: Nginx tidak punya checksum sama sekali

Aturan codebase ini tegas — tidak ada download tanpa SHA-256 terverifikasi (lihat alasannya di `php_catalog.rs` dan `php_ext.rs`). Nginx tidak menerbitkan index rilis yang bisa dibaca mesin **maupun** file checksum apapun: di `nginx.org/download/` tiap `.zip` cuma ditemani `.zip.asc` (signature PGP). Tiga opsi:

- **(a)** Tetap satu versi pinned, tapi rutin di-bump — status quo, tapi 1.25.3 sudah ketinggalan jauh
- **(b)** Tabel pinned berisi beberapa versi — trade-off yang persis sama dengan katalog PECL di `php_ext.rs`, jadi presedennya sudah ada di codebase ini
- **(c)** Implementasi verifikasi PGP — menambah dependency dan urusan manajemen key

Rekomendasi: **(b)**, karena jumlah versi nginx yang relevan untuk local dev sedikit dan polanya sudah dikenal di codebase.

---

## Fase 3.11 — Per-Project PHP Version (Concurrent)

**Tujuan:** Project bisa pin versi PHP sendiri, beda dari versi aktif global di halaman Switch —
dan beberapa project dengan versi berbeda bisa **jalan bersamaan** (mis. project A di PHP 7.4,
project B di 8.0, project C di 8.5, semua serve request di waktu yang sama). Sebelumnya cuma ada
satu versi PHP aktif untuk seluruh app; mengganti versi berarti stop-start satu proses `php-cgi`
yang sama, jadi dua project butuh dua versi berbeda tidak mungkin dijalankan bersamaan.

Bukan item dari roadmap awal — ditambahkan setelah pertanyaan langsung dari maintainer soal
kelayakan isolated-mode ala Laragon Pro. Dikerjakan lebih dulu dari 3.6/3.7/3.10 karena ternyata
jadi fondasi yang juga dibutuhkan [Fase 3.3](#fase-33--project-health-dashboard) (project ↔
service/port mapping yang tadinya belum ada sama sekali).

### Cara kerja (ringkas)

Windows tidak punya PHP-FPM asli — Rezure selalu jalanin `php-cgi -b 127.0.0.1:PORT` sebagai
responder FastCGI stateless per versi (lihat `services/vhosts.rs`). Itu artinya satu proses cuma
bisa satu versi, tapi tidak ada yang menghalangi banyak proses jalan bersamaan — jadi solusinya
murni soal port allocation + service lifecycle, bukan batasan PHP itu sendiri.

- **Service "php" default** (id `"php"`, port 9000 tetap) terus ikutin versi aktif global persis
  seperti sebelumnya — project yang tidak pin apa-apa tidak berubah perilakunya sama sekali.
- **Project yang pin versi berbeda dari default** dapat instance `php-cgi` pooled sendiri
  (`services/php_pool.rs`), id `php-<versi>`, port dialokasikan mulai 9001 — dihitung deterministik
  dari sorted set versi yang lagi dipin, bukan dari urutan scan, supaya alokasi port stabil.
- **`ServiceManager`** yang tadinya list service tetap (`Vec<ServiceHandle>` dikunci sejak start)
  sekarang bisa registrasi/unregister instance pooled saat runtime (`sync_php_pool`), dipanggil
  setiap vhost sync — jadi versi yang baru dipin langsung muncul sebagai service yang bisa
  di-Start/Stop di UI, konsisten dengan cara semua service lain dikontrol manual (bukan
  auto-start/stop mengikuti project) — tidak ada proses tersembunyi yang jalan sendiri.
- **`ProjectInfo.php_version`** (kolom SQLite baru, `NULL` = ikut default) — filesystem-scan tidak
  bisa tahu ini (folder tidak punya opini soal versi PHP), jadi di-merge dari SQLite persis seperti
  `last_opened_at`/`open_count`.

### Tasks
- [x] Migrasi SQLite: kolom `projects.php_version` (nullable, `NULL` = ikut versi aktif global)
- [x] `db::projects`: `fetch_php_versions`/`set_php_version`, tidak ikut ditimpa saat rescan (pola
      sama seperti history)
- [x] `services::php_pool` — alokasi port murni & testable: `wanted()` (versi yang dipin → port,
      deterministik dari sorted set), `port_for_project()` (fallback ke port default kalau tidak
      dipin atau dipin ke versi yang sudah tidak terinstall)
- [x] `services::php::exe_for(version)` — resolusi binary per-versi eksplisit, terpisah dari
      `active_exe()` yang ikut versi global
- [x] `ProcessService` digeneralisasi: `Launch::Php` bawa `Option<String>` (versi pinned), konstruktor
      baru `php_pinned(version, port, sink)` untuk instance pooled — `id`/`name` jadi `String` owned
      (dulu `&'static str`) supaya id dinamis seperti `"php-8.3.0"` bisa dibuat
- [x] `ServiceManager` jadi bisa registrasi dinamis (`Mutex<Vec<ServiceHandle>>` + `PhpPoolFactory`),
      `sync_php_pool()` reconcile instance pooled — tambah yang baru dibutuhkan, stop+buang yang
      sudah tidak dipin siapa pun
- [x] `services::vhosts::sync_vhosts` menghitung port per-project lewat `php_pool` dan menulisnya ke
      `fastcgi_pass`, return `VhostSync.php_pool` buat di-reconcile `commands::projects` setiap kali
      project di-scan/link/unlink/pin
- [x] Command Tauri baru: `set_project_php_version` — tipis, delegasi ke `db::projects` +
      re-sync vhosts/pool
- [x] UI: tombol baru di `ProjectActionButtons.vue` (ikon terisi merah kalau sedang dipin) buka
      `ProjectPhpVersionModal.vue` — pilih "Default" atau salah satu versi terinstall
- [x] `TechIcon.vue` dan notifikasi crash di `real_services()` dikenali `"php-*"` sebagai PHP, bukan
      jatuh ke initial-letter/id mentah
- [x] Unit test: `php_pool` (alokasi port, stabilitas urutan, fallback versi tak terinstall),
      `ProcessService::php_pinned`, `db::projects` (override survive rescan, clear balik ke default)
- [x] `cargo fmt`/`cargo clippy -- -D warnings`/`cargo test --lib`, `npm run lint`/`type-check`
      bersih — satu test gagal (`db_clients::tableplus_gets_a_connection_url_naming_the_database`)
      sudah gagal sebelum perubahan ini, tidak terkait
- [x] **Bug ditemukan lewat pemakaian nyata, sudah diperbaiki**: `vhosts::sync_vhosts()` manggil
      `services::projects::scan_projects()` (scan filesystem mentah) langsung — field `php_version`
      di situ **selalu `None`**, karena override cuma di-merge dari SQLite di `commands::projects::list_projects`,
      pass terpisah yang hasilnya tidak pernah sampai ke penulisan vhost. Akibatnya pin project
      manapun **jadi no-op total**: config nginx selalu resolve ke port default, tidak peduli versi
      apa yang dipilih di modal. Fix: `sync_vhosts()` sekarang menerima `php_versions: &HashMap<String, Option<String>>`
      (dari `db::projects::fetch_php_versions`) dan meng-merge-nya sebelum hitung pool — `commands::projects::sync_vhosts_and_reload`
      yang fetch dari SQLite dan mengoper ke bawah, jadi satu-satunya sumber kebenaran
- [x] **Bug kedua, ditemukan pas maintainer nyoba pin 3+ project ke versi berbeda-beda lewat app
      sungguhan**: alokasi port versi pooled dihitung ulang dari nol tiap `sync_vhosts` jalan —
      posisi dalam sorted-set versi yang lagi dipin **saat itu juga**. Pin project baru ke versi
      lain mengubah himpunan itu, dan port versi yang **sudah jalan** bisa ikut geser di config
      nginx yang baru ditulis — padahal proses `php-cgi`-nya sendiri tetap di port lama. Nginx jadi
      nunjuk ke port kosong → 502 buat project yang sama sekali tidak disentuh. Fix: alokasi port
      dipindah jadi *sticky* dan otoritasnya cuma satu — `ServiceManager::sync_php_pool`
      (`services/mod.rs`), satu-satunya yang tahu port asli tempat service pooled yang sudah
      terdaftar itu bind. `php_pool::assign_ports` (murni, testable) reuse port yang sudah ada,
      cuma alokasi port baru buat versi yang genar-genar baru. `vhosts::sync_vhosts` sekarang minta
      port lewat closure (`resolve_pool`) yang dioper dari `commands::projects`, bukan hitung
      sendiri — satu jalur data, nggak ada lagi dua tempat yang bisa saling beda pendapat soal port
- [x] **Bug ketiga, ketemu dari `openssl_cipher_iv_length()` undefined di project yang dipin ke PHP
      7.4**: `php_ini::ensure_php_ini` nulis **satu `php.ini` bersama** (`data/php/php.ini`) untuk
      semua proses PHP. Aman selama cuma satu versi yang pernah jalan; begitu beberapa versi start
      berdekatan, versi yang start belakangan bisa kebaca `extension_dir` versi lain, gagal load DLL
      beda ABI, dan extension-nya hilang. Fix: satu ini per install
      (`data/php/ini/<folder>-<hash path>/php.ini`), plus tulis ulang hanya kalau isinya berubah
- [x] **Bug keempat, semua project 502 setelah restart app**: service pooled baru didaftarkan saat
      halaman Projects dibuka, jadi "Start all" di halaman Services cuma menyalakan Nginx/PHP
      default/Database — semua project yang dipin nunjuk ke port yang tidak ada prosesnya. Fix:
      `lib.rs` menjalankan `sync_vhosts_and_reload` sekali saat startup, begitu `DbState` siap.
      Sekalian: folder `data/php-<versi>` untuk pid file service pooled tidak pernah dibuat, jadi
      pid-nya gagal ditulis diam-diam dan `reap_orphan` buta terhadap instance itu setelah crash —
      sekarang foldernya dibuat sebelum menulis
- [x] **Bug kelima, ketemu pas maintainer pin `pabrik-baut` ke versi baru dan kartunya nggak muncul
      di halaman Services**: `stores/services.ts`'s `services` cuma di-fetch sekali waktu app dibuka
      (`App.vue`'s `onMounted`) — `commands::projects::list_projects` yang mendaftarkan service
      pooled baru di backend tidak pernah memicu refetch itu di frontend, jadi service-nya beneran
      ada dan bisa di-start lewat command langsung, tapi user nggak lihat kartunya sama sekali
      tanpa restart app. Fix: `stores/projects.ts`'s `fetchAll()` sekarang ikut manggil
      `useServicesStore().fetchAll()` — daftar project dan daftar service selalu disegarkan bareng
- [x] **Verifikasi manual di mesin sungguhan** (bukan lewat UI, langsung cek proses/port/ini):
      3 `php-cgi.exe` (7.4.33, 8.4.25, 8.5.10) kebukti jalan **bersamaan**, masing-masing bind port
      sendiri (9001/9000/9002) dan resolve `openssl_cipher_iv_length()` dengan benar dari
      `ext_dir`-nya masing-masing — mekanisme inti (banyak versi PHP jalan bareng, tiap project
      nyampe ke versi yang benar) terbukti bekerja. **Belum ada** verifikasi `curl` end-to-end lewat
      browser buat tiap kombinasi setelah rentetan fix bug 1-5 di atas — sesi ini berhenti di tahap
      diagnosis manual, bukan konfirmasi "semua project sudah normal lagi" dari maintainer
- [x] Filter log di `stores/logs.ts` (`LOG_SERVICES`) masih daftar statis `['nginx','php','mariadb']`
      — instance pooled (`php-8.0.30`, dst.) tidak muncul sebagai opsi filter eksplisit (log-nya
      sendiri tetap masuk, cuma tidak ada shortcut filter per-versi). Fix: `LogsView.vue` sekarang
      turunan `filterableServices` dari `LOG_SERVICES` digabung id pooled (`id.startsWith('php-')`)
      yang lagi terdaftar di `useServicesStore()` — shortcut filter per-versi muncul otomatis begitu
      project dipin, tanpa nyentuh `stores/logs.ts` sendiri. Diuji manual oleh maintainer, jalan baik
- [ ] Nama tampilan `"PHP 8.3.0"` di kartu service belum dicek langsung di app sungguhan (cuma
      diverifikasi lewat unit test `ServiceInfo`, bukan browser/UI manual)
- [x] **Tambahan UI di luar rencana awal, diminta maintainer selama sesi debugging ini**: tombol
      "Restart all" di halaman Services, di antara "Start all" dan "Stop all" — merestart tiap
      service yang lagi Running (yang Stopped tetap dibiarkan, itu tugas "Start all"), overlay dan
      animasi ikon yang sama gayanya dengan dua tombol lain. Berguna khusus buat alur pool PHP: bug
      kelima di atas bikin kartu service pooled baru nggak nongol tanpa refresh manual, jadi tombol
      restart cepat ini mempercepat siklus pin-versi → cek-hasil selagi bug itu (sekarang sudah
      diperbaiki lewat auto-refresh) belum ada. `stores/services.ts`'s `restartAll()`,
      `src/views/DashboardView.vue`

---

## Dependency ke Proyek Lain

Fase 3.4: `GET /api/v1/version/latest` di `laravel-api` **sudah** mengembalikan manifest bertanda tangan sesuai [`docs/version-contract.md`](../version-contract.md) (`VersionController`, kolom `signature`/`download_url` di `releases`). Yang masih tersisa cuma verifikasi end-to-end lawan rilis nyata — repo ini belum punya pipeline yang build+sign installer, jadi belum ada rilis sungguhan buat diuji; sementara kode sisi app sudah bisa diuji lokal lawan manifest tiruan (prosedur ada di doc kontrak itu). Fase 3.1, 3.3, 3.5–3.10, dan 3.11 sepenuhnya independen, tidak bergantung pada backend.

**Catatan soal analytics lanjutan (v3 `rezure-dashboard`):** fitur traffic by hour, breakdown negara, cohort retention, dll di dashboard **tidak membutuhkan perubahan apapun di app ini** — semua data granular yang dibutuhkan (timestamp, OS version, metadata service) sudah terkirim sejak fondasi telemetry v2. Geolocation negara diproses di sisi server dari IP request yang masuk, bukan dikirim dari client.

## Urutan Pengerjaan yang Disarankan

1. Fase 3.1 (One-click Tunneling) — independen, langsung menambah nilai bagi user
2. **Fase 3.11 (Per-Project PHP Version) — selesai duluan**, di luar urutan aslinya, karena jadi
   fondasi yang dibutuhkan Fase 3.3 (project ↔ service/port mapping) dan nempel ke infrastruktur
   yang sama dengan 3.6/3.7 (`php_ini.rs`/`php_ext.rs`/`vhosts.rs`)
3. Fase 3.6 (Bundled PHP Extension Toggle) — nempel langsung ke infrastruktur `php_ini.rs`/`php_ext.rs` yang sudah ada, scope kecil dan kontributor-friendly
4. Fase 3.10 (Multi-Version Installer) — nyentuh halaman Switch dan `php_catalog.rs`, sebaiknya sebelum Fase 3.5.1 (Node.js switcher) yang bakal butuh katalognya
5. Fase 3.7 (Xdebug sebagai Extension Resmi) — kelanjutan langsung dari 3.1b/3.6, sama-sama nyentuh `php_ext.rs`/`php_ini.rs`, cocok dikerjakan berurutan
6. Fase 3.8 (Queue Worker Supervision) — pain point harian yang nempel di infra process management yang sudah ada
7. Fase 3.3 (Project Health Dashboard) — sekarang bisa langsung mulai dari mapping project↔PHP yang sudah ada dari Fase 3.11
8. Fase 3.9 (Export/Import Workspace Config)
9. Fase 3.4 (Auto-Update) — sisa cuma verifikasi E2E, butuh pipeline rilis nyata dulu