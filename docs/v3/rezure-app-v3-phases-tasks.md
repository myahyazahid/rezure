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
- [x] Implementasi `tauri-plugin-updater` — dependency + registrasi plugin (`src-tauri/src/lib.rs`), capability `updater:default`, `bundle.createUpdaterArtifacts` di `tauri.conf.json`. Tidak ada command Tauri custom — seluruh alur `check`/`downloadAndInstall` dipanggil langsung dari Pinia store (`src/stores/update.ts`) lewat `@tauri-apps/plugin-updater`, bukan diwrap ulang di Rust
- [x] Cek update via `check()` plugin resmi, terhadap manifest bertanda tangan (ed25519) sesuai kontrak di [`docs/version-contract.md`](../version-contract.md) — **bukan** sekadar `{version, download_url}` seperti sketsa awal: checksum dari server yang sama dengan file yang didownload gak ngasih jaminan integritas apa-apa, signature yang diverifikasi pakai public key yang ketanam di app baru bener-bener independen dari server
- [x] Notifikasi in-app saat ada versi baru tersedia — badge titik merah di item sidebar "Changelog" (`AppSidebar.vue`), sinyal terpisah dari "changelog belum dibaca" (dua hal ini independen: update bisa ada padahal entry changelog-nya udah keklik `seen`, atau sebaliknya)
- [x] Link notifikasi ke halaman Changelog (menu yang sudah ada dari v2) untuk detail perubahan — banner "Update" muncul langsung di `ChangelogView.vue`
- [x] Alur update: **satu tombol "Update"** di halaman Changelog memicu seluruh urutan (download dengan progress terlihat → install → di Windows app keluar sendiri setelah installer jalan) — bukan background pre-download + konfirmasi terpisah, sesuai keputusan produk final (one-click-does-it-all, konsisten sama pola tombol Share/Install PHP version yang udah ada)
- [ ] Generate keypair `tauri signer generate` (sekali, manual) — public key masuk `tauri.conf.json` (placeholder `REPLACE_WITH_PUBKEY_FROM_TAURI_SIGNER_GENERATE` sekarang), private key + password jadi secret rilis (CI/lokal), tidak pernah masuk git
- [ ] `laravel-api` ubah response `GET /api/v1/version/latest` dari `{version, notes, published_at}` ke manifest bertanda tangan (`pub_date` + `platforms.windows-x86_64.{signature,url}`), plus tambah kolom `signature`/`url` di tabel `releases` dan bandingkan versi terhadap client yang minta (buat balikin `204` kalau gak ada update) — endpoint-nya **sudah live**, tapi shape-nya belum sesuai; detail di `api-documentation/telemetry-api.md`
- [ ] Uji end-to-end lawan endpoint asli begitu `laravel-api` selesai diubah — sementara ini kode sisi app sudah bisa diuji lokal lawan manifest tiruan (lihat prosedur di `docs/version-contract.md`)

---

## Fase 3.5 — Support Developer / Donate Menu

**Tujuan:** User yang mau mendukung pengembangan Rezure bisa donasi dengan mudah, lewat berbagai platform (lokal, global, dan crypto).

### Tasks
- [x] Tambahkan menu "Support Developer" di sidebar (terpisah dari menu "Feedback" di Fase 2.1) — `/donate`, ikon hati, `AppSidebar.vue`
- [x] Section link donasi lokal: Trakteer / Saweria — tombol buka browser eksternal ke halaman donasi
- [x] Section link donasi global: GitHub Sponsors / Ko-fi — tombol buka browser eksternal
- [x] Section donasi crypto: tampilkan wallet address dengan tombol copy address dan QR code per wallet — QR digenerate client-side (`qrcode` package) dari address, gak pernah lewat layanan QR pihak ketiga
- [x] Pesan singkat konteks beserta link ke halaman "About" — `AboutView.vue` baru (`/about`), isi masih placeholder generik, nunggu cerita project asli dari maintainer
- [x] **Keputusan berubah dari sketsa awal roadmap**: link/alamat donasi **diambil dari API** (`GET /api/v1/support/donate`), bukan config statis di app — permintaan eksplisit user saat implementasi, memakai jalur pengecualian yang roadmap ini sendiri udah sediakan ("kecuali suatu saat ingin diubah dari server tanpa update app"). Pola fetch+cache+fallback sama persis kayak `services::changelog` (`services/donate.rs`). Endpoint-nya **belum ada** di `laravel-api` — spek lengkap di `api-documentation/telemetry-api.md` (`GET /support/donate`). Sampai endpoint itu jadi, halaman nampilin state "nothing configured yet" (fallback `DonateConfig::default()`)

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

## Dependency ke Proyek Lain

Fase 3.4 membutuhkan `GET /api/v1/version/latest` di `laravel-api` diubah dari shape lama (`{version, notes, published_at}`) ke manifest bertanda tangan sesuai [`docs/version-contract.md`](../version-contract.md) — endpoint-nya sendiri **sudah live**, ini soal ganti response shape + tambah kolom `signature`/`url` di `releases`, bukan bikin endpoint baru dari nol (lihat `api-documentation/telemetry-api.md` untuk kontraknya). Kode sisi app untuk fase ini sudah dibangun dan bisa diuji lokal lawan manifest tiruan (prosedur ada di doc kontrak itu); yang masih nunggu backend cuma verifikasi end-to-end lawan endpoint asli setelah shape-nya diubah. Fase 3.1–3.3, dan 3.5–3.10 sepenuhnya independen, tidak bergantung pada backend.

**Catatan soal analytics lanjutan (v3 `rezure-dashboard`):** fitur traffic by hour, breakdown negara, cohort retention, dll di dashboard **tidak membutuhkan perubahan apapun di app ini** — semua data granular yang dibutuhkan (timestamp, OS version, metadata service) sudah terkirim sejak fondasi telemetry v2. Geolocation negara diproses di sisi server dari IP request yang masuk, bukan dikirim dari client.

## Urutan Pengerjaan yang Disarankan

1. Fase 3.1 (One-click Tunneling) — independen, langsung menambah nilai bagi user
2. Fase 3.6 (Bundled PHP Extension Toggle) — nempel langsung ke infrastruktur `php_ini.rs`/`php_ext.rs` yang sudah ada, scope kecil dan kontributor-friendly
3. Fase 3.10 (Multi-Version Installer) — nyentuh halaman Switch dan `php_catalog.rs`, sebaiknya sebelum Fase 3.5.1 (Node.js switcher) yang bakal butuh katalognya
4. Fase 3.7 (Xdebug sebagai Extension Resmi) — kelanjutan langsung dari 3.1b/3.6, sama-sama nyentuh `php_ext.rs`/`php_ini.rs`, cocok dikerjakan berurutan
5. Fase 3.8 (Queue Worker Supervision) — pain point harian yang nempel di infra process management yang sudah ada
6. Fase 3.2 (Docker Toggle Mode)
7. Fase 3.3 (Project Health Dashboard)
8. Fase 3.9 (Export/Import Workspace Config)
9. Fase 3.4 (Auto-Update) — butuh endpoint `rezure-dashboard` siap, cocok dikerjakan setelah backend v2 matang