# Rezure (Desktop App) — v3: Advanced Features

Roadmap fitur lanjutan Rezure. Beberapa fitur yang sebelumnya direncanakan di v3 (Quick App Installer, Integrated Terminal, Embedded Database GUI) **sudah tersedia** di rilis sebelumnya — tidak dimasukkan lagi di sini.

> **Koreksi:** dokumen ini dulu juga menyebut **Auto HTTPS/mkcert** sudah tersedia. Itu keliru —
> tidak ada kode HTTPS lokal sama sekali (vhost hanya `listen 80`, tidak ada `ssl_certificate`
> maupun integrasi mkcert). Fitur ini **belum dijadwalkan** ke fase mana pun; kalau diinginkan,
> perlu diputuskan maintainer dan dibuatkan fase sendiri. Jangan tertukar dengan CA bundle di
> bagian hotfix bawah — itu soal PHP memverifikasi HTTPS *keluar*, bukan melayani HTTPS lokal.

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
- [x] Semua build yang di-pin diverifikasi lawan php.net sungguhan — test `#[ignore]`
      `every_pinned_build_matches_its_download` mengunduh ketujuh build `redis` 6.3.0 (PHP
      7.4–8.5): SHA-256 semuanya cocok dengan pin dan `php_redis.dll` ada di tiap arsip. Klik
      Install di jendela app sungguhan (unduhan lewat `AppHandle`) **tidak** diuji; jalur unduhnya
      `binaries::install_archive` yang sama dengan runtime lain
- [x] Cakupan katalog **diputuskan maintainer: tidak diperluas** — tetap `redis` saja. `imagick`
      dan lainnya tidak masuk v3; `xdebug` punya rencana sendiri di [Fase 4.3](../v4/rezure-app-v4-phases-tasks.md#fase-43--xdebug-sebagai-extension-resmi) (v4)

**Status: ditutup.**

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
- [x] Scan `ext/*.dll` di folder versi PHP aktif, cocokkan ke tabel nama+deskripsi ekstensi — `services::php_ext_toggle::CATALOG` (id, label, kategori, deskripsi, `default_on`, `debug_only`, `zend_extension`), 41 entry: 12 default lama, PECL `redis`, ekstensi bundled lain (`bz2`, `sodium`, `exif`, `xsl`, `sockets`, `ldap`, `imap`, `gmp`, `bcmath`, `ftp`, `dba`, `com_dotnet`, dst.), `opcache` (kategori "Performance", off by default karena bisa nyembunyiin perubahan kode kalau cache-nya tidak di-invalidate — satu-satunya entry yang butuh directive `zend_extension=` bukan `extension=`, `php_ini::render` sudah menangani ini lewat lookup `php_ext_toggle::find(id).zend_extension`, termasuk dedup-nya kalau user sudah nulis `zend_extension=` sendiri di `conf.d`), dan 4 entry debug-only. **Catatan:** ini masih tabel kurasi manual, bukan hasil scan otomatis — DLL yang ketemu di `ext/` tapi tidak ada di tabel ini tetap tidak akan muncul di UI
- [x] Simpan pilihan eksplisit user di file state terpisah per-versi PHP (`data/php/<version>/extensions.json`) — **bukan** di `conf.d`. Hanya override yang beda dari `default_on` yang ditulis (memilih ulang nilai default menghapus entrinya), supaya perubahan `default_on` di rilis berikutnya tetap kepakai buat siapa pun yang belum pernah menyentuh extension itu
- [x] `php_ini::render` digabung: `php_ext_toggle::EXTENSIONS` const lama dihapus total, diganti `php_ext_toggle::enabled_ids(version)` (default ∪ user-enabled − user-disabled) sebagai satu-satunya sumber kebenaran — `enabled_extensions`/`render` tetap filter ke DLL yang benar-benar ada di build itu persis seperti sebelumnya. `php_ini::version_for(php_dir)` resolve folder ke id versi lewat `services::php::installed()` supaya `ensure_php_ini`/`ensure_cli_php_ini` tidak perlu ubah signature publiknya sama sekali — proses lain yang manggil (`process.rs`, `doctor.rs`, `scaffold.rs`, `php_path.rs`) tidak tersentuh
- [x] Command Tauri tipis: `list_bundled_php_extensions`/`set_bundled_php_extension` (nama sengaja beda dari `php_extensions`/`install_php_extension` yang sudah dipakai PECL, biar tidak ambigu) — delegasi penuh ke `php_ext_toggle::status_for`/`set_enabled`
- [x] UI: **menu sidebar baru** "PHP Extensions" (`PhpExtensionsView.vue`, route `/php-extensions`), diletakkan tepat di bawah "Switch" di `AppSidebar.vue` — bukan digabung ke halaman Switch sebagai card, atas permintaan maintainer, supaya fitur yang isinya bisa puluhan baris toggle punya halaman sendiri. Dropdown pilih versi (beberapa bisa jalan bersamaan sejak Fase 3.11), search ringan, grouping per kategori (tiap kategori jadi kartu `divide-y` sendiri, gaya sama dengan daftar Runtimes di halaman Switch), badge "not in this build" abu-abu buat DLL yang gak ada, toggle switch bergaya sama dengan `PhpPathLinkCard.vue`
- [x] Default off + tooltip penjelasan untuk entry debug-only/environment-dependent (`zend_test`, `phpdbg_webhelper`, `oci8`/`pdo_oci`) — badge "special" dengan native tooltip, `debug_only: true` di catalog, tidak pernah `default_on`
- [x] Notice "restart PHP" saat toggle dilakukan sementara service sedang jalan — dicek dari `ServiceInfo.version` yang sudah real-time (service `php` default atau instance pooled manapun), bukan field baru
- [x] Diverifikasi lawan PHP terinstall nyata — test `#[ignore]` `a_toggle_changes_what_php_cgi_really_loads` menjalankan jalur backend yang sama dengan klik toggle (`set_enabled` → `php_ini::ensure_php_ini` → `php-cgi -m` dengan env service, `apply_process_env`) di keenam versi terinstall (7.4.33, 8.0.30, 8.1.34, 8.3.33, 8.4.25, 8.5.10): `bz2` dinyalakan → benar-benar termuat tanpa warning, dimatikan → hilang. `extensions.json` user dikembalikan byte-per-byte setelahnya (dicek hash-nya). Yang **tidak** diuji: klik switch di halaman PHP Extensions itu sendiri (UI → command tipis `set_bundled_php_extension`)

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
- [x] **Jalur rilis per major (3.x, 4.x, … dirawat bersamaan)**, diputuskan maintainer: versi
      wajib `MAJOR.MINOR.PATCH` (`3.0.1`, bukan `V.3.0.1`; prefix `v` hanya di git tag, satu
      branch per major). Major baru berarti rilis besar, sedangkan minor/patch berarti perbaikan
      di jalur yang sama. Semuanya di `laravel-api`, app tidak diubah: `VersionController` hanya
      menawarkan rilis dengan major yang sama dengan `current_version` (user 3.x tidak pernah
      auto-update ke 4.0). `Release::current()` sekarang memilih **versi tertinggi**, bukan
      `published_at` terbaru, supaya hotfix 3.0.2 yang di-publish setelah 4.0.0 tidak mengacaukan
      jalur lain. Form publish menolak format versi lain, dan halaman Releases menampilkan rilis
      terbaru per jalur. Detail di `docs/version-contract.md`
- [x] **Upgrade notice ("v4 sudah rilis")** untuk user di jalur lama, karena updater tidak pernah
      lintas major. Diatur dinamis dari dashboard (halaman Releases, satu pengaturan: on/off,
      major, pesan, link) lewat endpoint baru `GET /api/v1/version/upgrade`. Di app:
      `services::upgrade_notice` + command `fetch_upgrade_notice`, ditampilkan sebagai banner di
      `ChangelogView.vue` dengan tombol "Learn more" yang membuka link website lewat
      `open_external_link`. Tidak pernah download/install apa pun. Gagal fetch berarti banner
      tidak muncul, dan hasilnya tidak di-cache. Diuji lewat test Laravel (`UpgradeNoticeTest`,
      `ReleaseManifestTest`, `ReleasesTest`) dan unit test Rust. **Belum** dilihat di app
      sungguhan dengan notice yang dinyalakan di dashboard produksi
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
- [x] Modal install jadi runtime-aware: pilih runtime dulu, lalu versi — `InstallVersionModal.vue` (baru) menggantikan `InstallPhpVersionModal.vue` yang PHP-only (dihapus). Step 1 grid pilih runtime (PHP/Nginx/MariaDB/Composer/Node.js — Python sengaja tidak diikutkan, lihat catatan Python di bawah), step 2 tampilkan katalog runtime itu lewat `CatalogVersionList.vue` (baru, presentational, dipakai bersama oleh 4 dari 5 runtime); folder-add PHP ("Add from folder…") dipindah apa adanya ke dalam modal ini
- [x] Angkat `php_catalog.rs` jadi abstraksi katalog per-runtime — bukan lewat satu trait generik (API tiap sumber beda bentuk: MariaDB per-branch, Composer sidecar checksum, Node butuh filter LTS), tapi lewat modul paralel dengan bentuk struct yang sama (`version/latest/installed` + field opsional) mengikuti pola `php_catalog.rs` sendiri: `mariadb_catalog.rs`, `composer_catalog.rs`, `node_catalog.rs` (baru). `binaries::install_archive()`/`binaries::discover()` dipakai ulang persis seperti dugaan task ini
- [x] Dropdown jadi switch-only: `RuntimeSwitchRow.vue`'s `pick()` tidak lagi emit `install` (emit `install` dihapus total dari komponen), dan `installedVersions` (computed baru) memfilter entry yang belum terinstall — tidak pernah dilistkan sama sekali
- [x] Row tetap menampilkan progress bar install yang sedang jalan — pola yang sudah ada untuk PHP disamakan ke MariaDB/Composer/Node lewat `installingXxxVersion`/`progressFor` di masing-masing store (`stores/binaries.ts`, `stores/composer.ts`, `stores/node.ts` baru)
- [x] Runtime dengan 0 versi terinstall: tombol dropdown di-disable (`installedVersions.length === 0`) dengan title mengarahkan ke tombol "Install version", bukan dropdown kosong yang bisa diklik
- [x] Katalog MariaDB dari REST API `downloads.mariadb.org/rest-api/mariadb/<branch>/` — `services::mariadb_catalog`, daftar branch di-hardcode (`10.6`/`10.11`/`11.4`/`11.8`, perlu di-bump manual kalau MariaDB rilis branch baru) karena API-nya tidak punya endpoint "list semua branch" yang layak diandalkan; checksum SHA-256 tetap dari response live, bukan pinned manual
- [x] Katalog Composer dari `getcomposer.org/versions` — `services::composer_catalog` + `services::composer` (baru, install/active-version tracking, `composer.phar` sekarang per-versi di `bin/composer/<versi>/` bukan satu file flat tanpa checksum seperti sebelumnya). Checksum dicoba dari field JSON dulu, fallback ke sidecar `.sha256sum` kalau field-nya kosong — dua-duanya di-support sekaligus karena skema asli `getcomposer.org/versions` tidak bisa dipastikan tanpa akses live
- [x] Katalog Node.js dari `nodejs.org/dist/index.json` + `SHASUMS256.txt` per versi — `services::node_catalog`, cuma tampilkan versi terbaru tiap LTS line + 1 Current terbaru (bukan semua ratusan rilis). **Instalasi doang** (`bin/node/<versi>/node.exe` muncul di disk, checksum-verified) — belum ada PATH/per-project switching, itu memang fondasi buat Fase 3.5.1 sesuai rencana awal, bukan bagian dari task ini
- [ ] Nginx: **dipindah ke v4** ([Fase 4.7](../v4/rezure-app-v4-phases-tasks.md#fase-47--nginx-multi-version-catalog)) — belum diputuskan/diikutkan di sini, tapi modalnya sudah siap kalau nanti ada katalog: step Nginx di `InstallVersionModal.vue` sekarang menampilkan satu entry pinned dari `binaries::MANIFEST` (jalur yang sudah ada), tinggal diganti ke katalog beneran begitu keputusan di Fase 4.7 diambil

**Sudah diuji lawan API sungguhan** (awalnya ditulis tanpa akses jaringan, parsing dari dokumentasi) — ketiga test `#[ignore]` `fetches_the_real_index` lolos:
- **Node** (`nodejs.org/dist/index.json`): 12 versi (newest patch tiap LTS line + 1 Current), checksum dari `SHASUMS256.txt` per versi terbaca
- **Composer** (`getcomposer.org/versions`): 2.10.3 dan 2.2.30; checksum katalog dicocokkan dengan `composer.phar` yang benar-benar diunduh — cocok, dan sama dengan `composer.phar` 2.10.3 yang sudah terinstall
- **MariaDB** (REST API per branch): 66 versi di 4 branch (10.6, 10.11, 11.4, 11.8), terbaru 11.8.9. Versi terbaru tiap branch: URL unduhan HTTP 200 (83–92 MB), dan checksum dari REST API sama dengan `sha256sums.txt` di `archive.mariadb.org` (sumber independen). Zip-nya sendiri tidak diunduh penuh

**Python sengaja tidak diikutkan** — python.org tidak menerbitkan index rilis dengan SHA-256 yang bisa diambil otomatis (API publiknya cuma expose MD5), persis masalah yang sama dengan Nginx (lihat [Fase 4.7](../v4/rezure-app-v4-phases-tasks.md#fase-47--nginx-multi-version-catalog) di v4). Ditunda sampai ada keputusan serupa opsi (a)/(b)/(c) untuk Python, bukan diselesaikan diam-diam dengan checksum yang lebih lemah.

### Catatan: MariaDB itu stateful

PHP/Nginx/Composer stateless — ganti versi cuma soal ganti binary. MariaDB punya datadir, dan turun versi major sering tidak mungkin sama sekali, jadi "switch versi MariaDB" bukan operasi simetris dengan "switch versi PHP".

Ini justru nyambung ke [`mysql-profile-switcher-spec.md`](../v1/mysql-profile-switcher-spec.md), yang sudah menyebut field `mysql_version` untuk "pick a compatible bundled binary" dan meminta form berisi "dropdown of bundled versions". Artinya multi-versi MariaDB adalah **prasyarat yang spec itu sudah asumsikan ada** — user yang mau mengadopsi datadir Laragon buatan MySQL 8.0.30 tidak akan terlayani oleh MariaDB 11.2.2 yang sekarang jadi satu-satunya.

Pertanyaan terbuka soal Nginx tidak punya checksum sama sekali (dan tiga opsi penyelesaiannya)
sudah dipindah ke [Fase 4.7](../v4/rezure-app-v4-phases-tasks.md#fase-47--nginx-multi-version-catalog)
di v4.

---

## Fase 3.11 — Per-Project PHP Version (Concurrent)

**Tujuan:** Project bisa pin versi PHP sendiri, beda dari versi aktif global di halaman Switch —
dan beberapa project dengan versi berbeda bisa **jalan bersamaan** (mis. project A di PHP 7.4,
project B di 8.0, project C di 8.5, semua serve request di waktu yang sama). Sebelumnya cuma ada
satu versi PHP aktif untuk seluruh app; mengganti versi berarti stop-start satu proses `php-cgi`
yang sama, jadi dua project butuh dua versi berbeda tidak mungkin dijalankan bersamaan.

Bukan item dari roadmap awal — ditambahkan setelah pertanyaan langsung dari maintainer soal
kelayakan isolated-mode ala Laragon Pro. Dikerjakan lebih dulu dari 3.6/3.10 karena ternyata
jadi fondasi yang juga dibutuhkan [Fase 4.5 — Project Health Dashboard](../v4/rezure-app-v4-phases-tasks.md#fase-45--project-health-dashboard)
di v4 (project ↔ service/port mapping yang tadinya belum ada sama sekali).

### Cara kerja (ringkas)

Windows tidak punya PHP-FPM asli — Rezure selalu jalanin `php-cgi -b 127.0.0.1:PORT` sebagai
responder FastCGI stateless per versi (lihat `services/vhosts.rs`). Itu artinya satu proses cuma
bisa satu versi, tapi tidak ada yang menghalangi banyak proses jalan bersamaan — jadi solusinya
murni soal port allocation + service lifecycle, bukan batasan PHP itu sendiri.

- **Service "php" default** (id `"php"`, blok port tetap `9100–9103` sejak Hotfix T4 — dulu satu
  port 9000) terus ikutin versi aktif global persis seperti sebelumnya — project yang tidak pin
  apa-apa tidak berubah perilakunya sama sekali.
- **Project yang pin versi berbeda dari default** dapat instance `php-cgi` pooled sendiri
  (`services/php_pool.rs`), id `php-<versi>`, blok port dialokasikan mulai 9110 (per 10 port; dulu
  satu port mulai 9001) — sticky: versi yang sudah jalan tidak pernah pindah port saat versi lain
  dipin/di-unpin (lihat `php_pool::assign_ports`).
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
- [x] **Terminal dari project card ikut versi PHP yang dipin**, sama seperti Node (v3.5 Fase
      3.5.1). Diminta maintainer: sebelumnya pin cuma berlaku untuk web, jadi `php artisan` di
      terminal project tetap jalan di PHP lain (versi di "PHP Everywhere", Laragon, atau tidak ada
      sama sekali). `services::php::terminal_bin_dir` resolve pin project, lalu fallback ke versi
      aktif kalau pin kosong **atau versinya sudah tidak terinstall**. Fallback ini sama dengan
      `php_pool::port_for_project` di sisi web, jadi terminal dan site tidak mungkin beda versi.
      Beda dengan Node, yang pin basinya jadi `None`. Folder itu di-prepend ke `PATH` terminal
      (`composer` global ikut, karena memanggil `php` dari `PATH`), `PHP_INI_SCAN_DIR` diberi
      `conf.d` (append, bukan replace), dan `php.ini` CLI di folder versi itu di-heal dulu.
      `OPENSSL_CONF` **sengaja tidak** di-set, karena Git di terminal yang sama membaca variabel
      itu juga. `launcher::open_terminal` sekarang menerima `TerminalEnv` (daftar folder + env var)
      pengganti satu `node_bin_dir`, dan resolusi Node dipindah dari `commands::projects` ke
      `services::node::terminal_bin_dir` supaya kedua runtime di-resolve di satu tempat.
      Diverifikasi di mesin nyata: dengan env hasil pin 7.4.33, `where php` menaruh
      `bin\php\7.4.33` di atas junction `current\php` dan Laragon, `php --ini` membaca `php.ini`
      versi itu + `conf.d`, dan `openssl`/`pdo_mysql` ter-load. **Belum** diklik lewat tombol
      terminal di app sungguhan
- [x] **Tambahan UI di luar rencana awal, diminta maintainer selama sesi debugging ini**: tombol
      "Restart all" di halaman Services, di antara "Start all" dan "Stop all" — merestart tiap
      service yang lagi Running (yang Stopped tetap dibiarkan, itu tugas "Start all"), overlay dan
      animasi ikon yang sama gayanya dengan dua tombol lain. Berguna khusus buat alur pool PHP: bug
      kelima di atas bikin kartu service pooled baru nggak nongol tanpa refresh manual, jadi tombol
      restart cepat ini mempercepat siklus pin-versi → cek-hasil selagi bug itu (sekarang sudah
      diperbaiki lewat auto-refresh) belum ada. `stores/services.ts`'s `restartAll()`,
      `src/views/DashboardView.vue`

---

## Hotfix — Stabilitas PHP & Batas Request (feedback client)

**Latar belakang:** log nginx client menunjukkan ~4,6% request berakhir 502 (`connect() failed
(10061) ... fastcgi://127.0.0.1:9000`), plus 413 untuk upload >1 MB dan `upstream timed out`.

### Tasks
- [x] **T1** — `PHP_FCGI_MAX_REQUESTS=0` di setiap spawn `php-cgi` (default maupun pooled).
      Tanpa ini `php-cgi` keluar sendiri setelah 500 request. `services/process.rs`
- [x] **T2** — `client_max_body_size`, `fastcgi_read_timeout` dan `fastcgi_send_timeout` di blok
      `http {}` config utama nginx, diambil dari konstanta yang sama dengan php.ini
      (`php_ini::BODY_SIZE_LIMIT_MB` = 64, `php_ini::MAX_EXECUTION_TIME_SECS` = 300).
      Override user lewat `conf.d` **tidak** ikut dibaca, jadi user yang menaikkan
      `post_max_size` di sana tetap dibatasi 64 MB oleh nginx
- [x] **T3** — Watchdog `services::supervisor`: cek tiap 2 detik tanpa bergantung pada UI, restart
      service yang crash dengan backoff 1/2/5/10/30 detik. Kalau crash 5 kali dalam 5 menit, restart
      otomatis berhenti sampai user start/stop manual. Opt-in lewat
      `Service::restarts_on_crash` (sekarang khusus PHP, bukan nginx/database). Stop manual
      selalu menang karena flag `Service::crashed` direset oleh stop. Frontend refetch lewat event
      `service://changed`
- [x] T2 diverifikasi: nginx + php-cgi 8.5.10 terpisah (port sendiri) dengan baris
      `client_max_body_size`/`fastcgi_*_timeout` disalin apa adanya dari `nginx.conf` hasil
      generate dan `php.ini` hasil generate. Upload 5 MB: **413** tanpa baris itu (default nginx,
      kondisi sebelum fix) → **200**, PHP menerima 5.242.880 byte utuh dengan baris itu. Upload
      70 MB tetap 413 — di atas batas 64 MB, sesuai rancangan
- [x] T3 diuji di app sungguhan oleh maintainer: `php-cgi.exe` di-kill lewat Task Manager →
      thread supervisor di dalam app menghidupkannya lagi sendiri, berjalan dengan baik. Jalur
      heal-nya sendiri sebelumnya sudah terbukti lawan `php-cgi` asli lewat test
      `a_dead_php_worker_is_healed_without_touching_the_rest` (lihat T4)
- [x] **T4** — Worker pool per versi (N `php-cgi` dalam satu service, nginx `upstream`).
      Alasannya: `php-cgi` di Windows cuma bisa melayani satu request sekaligus
      (`PHP_FCGI_CHILDREN` butuh `fork()`), jadi satu request lambat bikin request lain ke versi
      yang sama antre sampai `upstream timed out`.
      - **Alokasi port baru, blok 10 port per versi** (`php_pool::PORT_BLOCK`), worker pakai
        4 port pertama (`WORKERS_PER_VERSION`): default `9100–9103`, versi pin pertama
        `9110–9113`, berikutnya `9120…`. Sisa 6 port per blok = ruang buat menaikkan jumlah worker
        nanti tanpa renumber. Sengaja **tidak** mulai dari 9000: blok 9000 akan menaruh worker di
        **9003**, port tempat IDE listen untuk Xdebug 3 (v4 Fase 4.3), dan 9000 sendiri biasa
        dipegang php-cgi/FPM Laragon/XAMPP. Port pool tidak pernah disimpan ke disk (hidup di
        `ServiceManager`), jadi tidak ada migrasi — vhost ditulis ulang saat sync pertama begitu
        app start. Tetap sticky seperti sebelumnya, cuma satuannya blok, bukan port
      - **`ProcessService`**: satu `child` → slot per worker (`children: Vec<Option<Child>>`),
        PID file per worker (worker 0 tetap `service.pid` supaya orphan dari build lama masih
        dikenali). `start()` cuma spawn slot yang kosong — worker yang masih hidup tidak disentuh,
        jadi supervisor (T3) menyembuhkan satu worker mati tanpa memutus request yang sedang
        jalan di worker lain. Semua port dicek dulu sebelum ada yang di-spawn; kalau spawn gagal di
        tengah, worker yang baru di-spawn di panggilan itu dibunuh lagi. Notifikasi crash cuma
        muncul kalau **semua** worker mati; worker mati sebagian cukup ditulis ke log (service
        masih melayani)
      - **nginx**: `php-upstreams.conf` (di luar folder `vhosts/`, karena folder itu dipangkas ke
        satu file per project) berisi satu `upstream rezure_php_<port>` per versi dengan
        `least_conn` — bukan round-robin, karena `php-cgi` yang sibuk tetap *menerima* koneksi
        (antre di listen backlog OS) sehingga round-robin tetap mengirim request ke worker yang
        sedang sibuk. `fastcgi_next_upstream error` (tanpa `timeout`): worker yang mati dilewati,
        tapi request yang kena timeout 300 detik tidak diulang ke worker lain (itu akan mengikat
        semua worker). Vhost sekarang `fastcgi_pass rezure_php_<port>;`
      - **UI**: `ServiceInfo.workers` (`{ running, total }`, `null` untuk nginx/database) — kartu
        service menampilkan `:9100–9103` dan `4/4 workers` (kuning kalau ada yang mati);
        penanganan "port in use" mencari pemegang port di semua port worker, bukan cuma yang
        pertama
      - **Diverifikasi di mesin nyata**: bench terpisah (nginx + php-cgi 8.3 di port sendiri,
        config sama dengan yang digenerate) — request cepat saat 2 request `sleep(6)` jalan:
        **11,55 detik** dengan 1 worker vs **0,003 detik** dengan 4 worker; satu worker dibunuh →
        8/8 request tetap 200. Test `#[ignore]`
        `a_dead_php_worker_is_healed_without_touching_the_rest` lawan `php-cgi` asli: worker yang
        dibunuh diganti, 3 lainnya tetap PID yang sama. App dev (`tauri dev`) yang ikut rebuild
        menjalankan 4 versi × 4 worker di blok yang benar, dan `nginx -t` lawan config hasil
        generate-nya lolos. `ServiceRow.vue` asli dirender di Edge headless dengan `invoke` di-mock:
        `:9100–9103`, `4/4 workers`, `3/4 workers` (kuning), nginx tanpa label worker; alur "port
        in use" dengan port ke-3 (9112) yang dipegang proses lain → pencarian berhenti di 9112,
        `free_port` dipanggil dengan **9112** (kode lama akan membebaskan 9110), lalu start
        diulang. Project Laravel 13 sungguhan (`laravel-api`, PHP 8.5.10 + `php.ini` hasil
        generate Rezure) dilayani lewat upstream 4 worker: 12 request `/up` paralel semuanya 200
        (0,68 detik dengan 1 worker vs 0,25 detik dengan 4). **Belum**: diklik langsung di
        jendela app Tauri, dan halaman Laravel yang butuh database/session
      - Jumlah worker masih konstanta (4); kalau nanti perlu jadi setting, batasnya
        `PORT_BLOCK` (10)

### CA bundle & OpenSSL

**Latar belakang:** `php_ini.rs` sudah menulis `curl.cainfo`/`openssl.cafile` kalau
`etc/cacert.pem` ada, tapi tidak ada satu pun kode yang menaruh file itu di disk — instalasi baru
kena `cURL error 60` untuk setiap HTTPS keluar dari PHP. Ditambah lagi, `php.ini` di folder versi
(yang dibaca terminal) ditulis sekali dan tidak pernah dapat baris CA.

- [x] **C1** — `services::ca_bundle`: `seed_bundled` menyalin bundle dari resource installer
      (`bundled-bin/ca/`, di-stage `scripts/stage-bundled-binaries.ps1` dari URL curl.se bertanggal
      dengan SHA-256 di-pin) ke `etc/cacert.pem` — kalau belum ada, atau kalau bundle installer lebih
      baru (tanggal Mozilla di header). Bundle tanpa header curl.se dianggap milik user dan tidak
      disentuh. Tombol **Download/Update** di halaman Switch (`PhpCaBundleCard.vue`) mengunduh
      `cacert.pem` terbaru dari curl.se, diverifikasi lewat sidecar `.sha256`
- [x] **C2** — `php_ini::repair_ca_directives`: menambahkan/memperbaiki `curl.cainfo` dan
      `openssl.cafile` di `php.ini` folder versi, hanya dua baris itu. Directive yang sudah menunjuk
      file yang ada (bundle korporat pilihan user) dibiarkan. Dijalankan saat startup untuk semua
      versi, di `ensure_cli_php_ini`, dan setelah update bundle
- [x] **C3** — Requirements check (`ProjectDoctorModal.vue`) sekarang juga menguji HTTPS dari PHP
      yang melayani (`doctor::check_active_tls`, command `check_php_tls`, terpisah dari cek ekstensi
      karena menunggu jaringan). Membedakan `untrusted` (error 60/77 → tawarkan download bundle)
      dari `unreachable` (offline — bukan salah bundle)
- [x] **C4** — `OPENSSL_CONF` → `extras/ssl/openssl.cnf` versi terkait, lewat helper baru
      `php_ini::apply_process_env` yang sekarang dipakai ketiga tempat spawn PHP (FastCGI,
      doctor, scaffold Composer). Terbukti di mesin nyata: `openssl_pkey_new()` `false` tanpa, `true`
      dengan. **Sengaja tidak di-set machine-wide** untuk terminal user — Git dan tool OpenSSL lain
      membaca variabel yang sama
- [ ] Belum diuji manual di app sungguhan: installer hasil `npm run stage:binaries` + build di mesin
      bersih (bundle ter-seed, `php artisan tinker` → `Http::get('https://…')` tidak error 60)

---

## Fase 3.12 — Dokumen untuk AI Agent (`AGENTS.md` di Rezure home)

**Latar belakang:** AI coding agent (Claude Code, Codex, Cursor, …) mengenal Laragon dari data
training tapi tidak mengenal Rezure. Di mesin yang punya keduanya, agent yang membaca `where php`
atau isi `C:\` malah mencari Laragon. Tidak ada di roadmap, **ditambahkan atas permintaan
maintainer**. Sengaja hanya menulis ke Rezure home, tidak ke config agent milik user
(`~/.claude/…`) maupun ke folder project.

### Tasks
- [x] `services::agent_docs`: saat startup lalu dicek tiap 60 detik (ditulis ulang hanya kalau
      isinya berubah), menulis tiga file di Rezure home:
      - `AGENTS.md`: apa itu Rezure ("bukan Laragon/XAMPP") dan keadaan mesin saat ini: PHP aktif,
        profil database (klaim "root tanpa password" hanya untuk profil bawaan Rezure; datadir
        adopsi disebut memakai user-nya sendiri), serta tabel project berisi URL, folder dan versi
        PHP yang benar-benar melayani (pin dihitung hanya kalau versinya terinstall, sama seperti
        `php_pool`). Juga aturan: jangan menjalankan atau mematikan proses sendiri, override PHP di
        `etc\php\conf.d`, dan `data\` itu hasil generate
      - `CLAUDE.md`: berisi `@AGENTS.md`. Claude Code membaca `CLAUDE.md` dari setiap folder
        induk, jadi project di `www\` otomatis memuatnya
      - `docs\how-rezure-works.md`: panduan lengkap (layanan dan port, layout folder, project dan
        hosts, Nginx, tiga lapis php.ini, database, Mailpit, `.env` Laravel, Node/Composer, share,
        hidup berdampingan dengan Laragon, aturan untuk agent), dari
        `src-tauri/agent-docs/how-rezure-works.md` yang di-embed saat build (`{{home}}` dan
        `{{version}}` diisi saat ditulis)
- [x] File yang sudah ada tanpa penanda `<!-- Generated by Rezure` dianggap milik orang lain dan
      tidak disentuh (aturan yang sama dengan CA bundle)
- [x] Unit test untuk render, resolusi pin, file bukan milik Rezure, dan escape nama project. Hasil
      render dengan data asli mesin maintainer sudah dicek (10 project, profil Laragon MySQL 8.4.3)
- [ ] Belum dilihat berjalan di app sungguhan (`tauri dev`), dan belum dicoba apakah Claude Code
      benar-benar memuatnya di sesi project di bawah `www\`
- **Batasan:** project yang di-link dari luar Rezure home (misalnya `C:\repository\…`) tidak punya
  `C:\rezure` sebagai folder induk, jadi `CLAUDE.md` tidak termuat otomatis. Solusinya satu baris
  `@C:\rezure\AGENTS.md` di `%USERPROFILE%\.claude\CLAUDE.md` milik user. Ini pilihan user, app
  tidak menulisnya (dijelaskan di akhir panduan)
- **Perawatan:** `src-tauri/agent-docs/how-rezure-works.md` harus ikut diperbarui setiap kali
  perilaku yang dilihat user berubah (port, folder, cara kerja php.ini, dan seterusnya). Kalau
  tidak, agent akan diberi informasi yang salah

---

## Fase 3.13 — Appearance (Tema & Dekorasi)

**Tujuan:** User bisa memilih tampilan Rezure: mode terang/gelap/ikut sistem, dan tema dekorasi
dari tiga kategori (Default, Girls, Mens), dari menu sendiri di sidebar. Tidak ada di roadmap awal,
**ditambahkan atas permintaan maintainer**.

### Keputusan yang sudah diambil
- **Nama kategori:** `Default`, `Girls`, `Mens`. Nama ini ditampilkan apa adanya di UI
- **Tema ada empat belas:** awalnya tiga (satu per kategori). Atas permintaan maintainer ditambah
  **Soft Pink** (Girls) dan **Navy** (Mens), lalu enam lagi dari rekomendasi yang disetujui
  maintainer: **Lavender Dream**, **Peach**, **Matcha** (Girls) serta **Carbon**, **Forest**,
  **Terminal** (Mens), lalu **Full Glass**, **Clear Glass**, dan **Sky Glass** (Default):

  | Kategori | Tema | Aksen | Latar (mesh) & dekorasi |
  |---|---|---|---|
  | Default | **Rezure** | merah coral (yang sekarang) | mesh yang sekarang, tanpa pola |
  | Default | **Full Glass** | sky/cyan | glass paling bening: panel hampir transparan, tepi dan highlight putih terang, blur `glass-strong` 48px; latar iridescent (aqua, violet, pink, biru langit); kilau cahaya diagonal |
  | Default | **Clear Glass** | slate netral (tanpa warna) | Full Glass tanpa warna, kaca bening di atas putih: panel dibentuk hanya oleh cahaya (tepi dan highlight putih, garis tepi tipis, bayangan abu lembut); latar putih ke perak; versi dark berupa kaca asap di atas arang; lengkung refraksi cahaya samar |
  | Default | **Sky Glass** | indigo muda | dari referensi maintainer (gaya kartu kaca di atas foto langit senja): panel kaca bertint biru-lavender, teks putih, latar langit senja biru ke pink, awan lembut. Tema mid-tone, **selalu memakai warna teks mode dark** |
  | Girls | **Blossom** | rose/pink | pink, lavender, peach, pola kelopak tipis |
  | Girls | **Soft Pink** | pink lembut (satu tingkat lebih terang) | glass paling bening: lebih transparan, tepi dan highlight lebih terang, bayangan pink, blur `glass-strong` 40px; latar pink pekat supaya transparansinya terlihat; pola gelembung |
  | Mens | **Midnight** | biru/teal | navy, slate, teal, pola grid tipis |
  | Mens | **Navy** | navy pekat (`blue-800`/`900`; versi dark lebih terang supaya terlihat) | navy dengan sentuhan emas: gradien navy → emas, latar navy dengan cahaya emas hangat, glass sedikit lebih solid supaya teks tetap terbaca; pola gelombang tipis |
  | Girls | **Lavender Dream** | violet | lilac dan biru muda (ungu malam di dark), bulan sabit dan bintang |
  | Girls | **Peach** | peach (oranye dicampur rose) | peach, krem, kuning lembut, hati kecil |
  | Girls | **Matcha** | hijau kalem (hijau dicampur stone) | mint, krem, sage, daun kecil |
  | Mens | **Carbon** | oranye | grafit hampir monokrom, anyaman serat karbon |
  | Mens | **Forest** | olive (lime dicampur stone) | hijau tua dan tanah, garis kontur peta |
  | Mens | **Terminal** | hijau neon | hitam pekat dengan cahaya hijau tipis (versi light putih kehijauan), scanline samar |

- **Posisi menu:** `/appearance`, di sidebar tepat **di bawah Support Developer** dan di atas
  Settings. Settings tetap paling bawah (lihat komentar di `AppSidebar.vue`). Ikon palet warna
- **Kustomisasi lanjutan (Fase 3.13c)** awalnya ditunda, lalu sebagian dikerjakan atas permintaan
  maintainer (kecerahan, intensitas warna, opacity glass, ukuran UI, kurangi animasi). Yang masih
  ditunda: warna aksen kustom, yang sudah disepakati berupa **palet pilihan yang kontrasnya sudah
  aman**, bukan color picker bebas
- **Pilihan mode (Light / Dark / System) dihapus dari halaman Appearance** atas permintaan
  maintainer. Tombol Light/Dark di title bar jadi satu-satunya kontrolnya. Nilai `system` tetap
  didukung di data, jadi user yang sudah memilihnya tidak terganggu

### Cara kerja (ringkas)
- **Tema = satu set nilai variabel CSS.** Semua permukaan glass sudah memakai token
  (`--app-mesh`, `--glass-*`) di `src/assets/main.css`, jadi tema tidak menyentuh komponen.
  Nilainya di-override lewat atribut `data-theme` di `<html>`:
  `[data-theme='blossom']` dan `.dark [data-theme='blossom']`. Atribut ini terpisah dari class
  `.dark`, jadi setiap tema punya varian light dan dark
- **Selector tema tidak boleh memakai `:root`.** Dengan `[data-theme='…']` biasa, elemen mana pun
  bisa memakai tema lain untuk isinya. Itulah yang membuat kartu preview di halaman Appearance
  murah: miniatur sidebar dan kartu cukup dibungkus `<div data-theme="midnight">`
- **Accent dipisah dari danger.** Saat ini merah dipakai untuk dua hal, yaitu brand dan
  error/destruktif, di sekitar 33 file. Brand pindah ke token `accent` (didaftarkan di `@theme`
  Tailwind v4, jadi `bg-accent-500` dan seterusnya) yang ikut tema. Error, tombol destruktif,
  badge alert, dan hover tombol close jendela tetap merah di semua tema
- **Warna status tidak ikut tema:** hijau (running), kuning (warning), merah (error) sama di
  ketiga tema
- **Dekorasi** berupa pola SVG statis dengan opacity rendah, dilapis di atas mesh lewat variabel
  (`--app-pattern`). **Tidak ada animasi.** Latar yang bergerak membuat setiap lapisan
  `backdrop-filter` digambar ulang tiap frame (lihat catatan di `main.css`)
- **Penyimpanan:** objek `appearance` di `settings.json` lewat `config::settings::Settings`
  (`#[serde(default)]`, jadi file lama tetap terbaca), sehingga config tetap satu sumber kebenaran
  (prinsip 4 di `CLAUDE.md`). Mode (`light`/`dark`/`system`) dan tema (`rezure`/`blossom`/
  `midnight`) berupa enum di Rust, dan nilai yang tidak dikenal jatuh ke default
- **localStorage hanya cache tampilan pertama.** Pengaturan dari `settings.json` baru datang
  setelah `invoke` selesai, jadi tanpa cache layar sempat berkedip ke tema default. Cache dibaca
  sinkron sebelum app di-mount, lalu dicocokkan dengan nilai dari Rust
- **Mode System hanya kalau dipilih.** Default tetap Light. Keputusan lama di `useTheme.ts` (sudah
  dihapus, digantikan `stores/appearance.ts`) tetap berlaku: tidak mengikuti OS supaya peluncuran
  pertama terlihat sama untuk semua orang

### Fase 3.13a — Token accent (prasyarat, tanpa perubahan visual)
- [x] Token `accent` (skala 50–950 + `accent-alt` untuk ujung gradien) di `main.css` lewat
      `@theme inline`, nilai default = merah/oranye yang dipakai sebelumnya
- [x] Semua penggunaan merah sebagai brand diganti ke `accent`: toggle, titik/indikator aktif,
      progress bar (`from-accent-500 to-accent-alt`), step indicator `NewProjectModal.vue`, badge
      aktif sidebar, tombol mode di title bar, label "Current"/"Active"/"Latest", link, tepi fokus
      input, checkbox (`accent-accent-600`). Bayangan glass yang berwarna merah sekarang diturunkan
      dari `--accent-900`/`--accent-950`, `--glass-accent-icon` dan `--glass-selected-border` dari
      accent
- [x] Merah sebagai danger dibiarkan: pesan error, input invalid, tombol destruktif (Unlink, Force
      stop, Free port), hover tombol hapus, ikon Stop, badge alert sidebar, hover tombol close
      jendela. Wordmark "Redscale" di title bar juga tetap merah (nama perusahaan, bukan aksen)
- [x] Build, `vue-tsc`, ESLint dan Prettier bersih. Di app sungguhan tema Rezure terlihat seperti
      sebelumnya. Satu perbedaan kecil yang disengaja: bayangan glass memakai `red-900`/`red-950`
      Tailwind, bukan nilai RGB tulisan tangan yang sebelumnya, jadi rona bayangannya sedikit
      bergeser

### Fase 3.13b — Halaman Appearance & tiga tema
- [x] Rust: `AppearanceSettings { mode, theme, show_decoration }` di `config::settings::Settings`
      sebagai `Option` (`None` = belum pernah disimpan), enum `ThemeMode`/`ThemePreset`, ikut di
      `SettingsPatch`/`update_settings` (mengganti seluruh blok). Dibaca lewat `lenient`: nilai tak
      dikenal jatuh ke default untuk field itu saja, tanpa membuat seluruh `settings.json` gagal
      dibaca. Unit test: belum disimpan, round trip bentuk JSON, nilai tak dikenal
- [x] Store Pinia `appearance` (`stores/appearance.ts`) menggantikan `useTheme.ts` (dihapus).
      Tombol Light/Dark di `AppTitleBar.vue` tetap ada dan membaca store yang sama. Dari mode
      System, tombol itu memilih kebalikan dari yang sedang tampil. Mode System mendengarkan
      `prefers-color-scheme`
- [x] Migrasi satu kali: kalau `appearance` di `settings.json` masih `null`, pilihan dari cache
      (atau key lama `rezure-theme`) disimpan ke sana, lalu key lama dihapus
- [x] Cache `rezure-appearance` di localStorage untuk tampilan pertama, divalidasi per field dan
      dibungkus try/catch. Store dibuat di `main.ts` sebelum `mount`, jadi `<html>` sudah memakai
      tema yang benar di frame pertama
- [x] CSS tema (`rezure`, `blossom`, `midnight`), masing-masing varian light dan dark: skala
      aksen, `--app-mesh`, `--app-pattern` (kelopak untuk Blossom, grid untuk Midnight). Class
      `no-decoration` di `<html>` mematikan pola
- [x] Tema keempat `softpink` (Soft Pink, kategori Girls). Selain warna, tema ini juga mengganti
      token glass (`--glass-bg`, border, highlight, bayangan, `--glass-blur`). Blur `glass-strong`
      sekarang memakai token `--glass-blur` (default 24px). Karena itu blok dark-nya harus
      meng-override setiap token yang di-override blok light-nya (alasannya di komentar
      `main.css`). Aksennya satu tingkat lebih terang (`accent-600` = `pink-500`), jadi kontras
      teks kecilnya lebih rendah lagi dari tema lain. Itu konsekuensi dari "soft"
- [ ] Soft Pink belum dilihat di app sungguhan, baik light maupun dark
- [x] Tema kelima `navy` (Navy, kategori Mens). Awalnya dibuat sebagai "Soft Navy" (pasangan
      Soft Pink), lalu diganti atas permintaan maintainer karena terlalu mirip Midnight. Bedanya
      sekarang: aksen navy pekat (bukan biru terang), ujung gradien emas (bukan teal), latar navy
      dengan cahaya emas, dan pola gelombang (bukan grid). Tema ini juga mengganti token glass,
      jadi blok dark-nya meng-override semua token yang sama; di mode dark aksen 500–700 dinaikkan
      satu tingkat supaya toggle tidak hilang di latar navy. Dirender di light dan dark lewat build
      + headless Chrome; belum dilihat di app sungguhan
- [x] Enam tema warna: `lavender`, `peach`, `matcha` (Girls), `carbon`, `forest`, `terminal`
      (Mens). Seperti Blossom dan Midnight, keenamnya hanya mengganti skala aksen, mesh, dan pola
      (glass tetap default). Test Rust memastikan kesebelas nama tema terbaca sebagai dirinya
      sendiri. Galeri dan beberapa tema yang diterapkan ke seluruh app (Carbon light, Terminal dan
      Forest dark) dirender lewat build + headless Chrome; belum dilihat di app sungguhan
- [x] Tema `fullglass` (Full Glass, kategori Default): versi paling ekstrem dari pendekatan Soft
      Pink. Mengganti token glass, termasuk `glass-accent`/`glass-raised`/`glass-ghost` supaya
      tombol ikut bening, dan blok dark meng-override token yang sama. Panel biasa tetap tanpa
      `backdrop-filter` (lihat catatan di atas `main.css`), jadi kesan beningnya datang dari
      transparansi, tepi, dan highlight, bukan dari frosting. Dirender di light dan dark lewat
      build + headless Chrome; belum dilihat di app sungguhan
- [x] Tema `clearglass` (Clear Glass, kategori Default): permintaan maintainer untuk kaca "bening
      putih" yang benar-benar seperti kaca. Tanpa warna di belakangnya, kaca hanya terlihat lewat
      cahaya, jadi bayangan diberi garis tepi tipis (`0 0 0 1px`) dan highlight bawah selain
      highlight atas. Aksen slate netral; di mode dark aksen 500/600 dinaikkan supaya toggle
      terlihat di latar arang. Dirender di light dan dark lewat build + headless Chrome; belum
      dilihat di app sungguhan
- [x] Tema `skyglass` (Sky Glass, kategori Default), dibuat dari gambar referensi maintainer.
      Latar langit senja mid-tone tidak cocok dengan teks gelap maupun teks terang mode light, jadi:
      - `ThemeInfo.forcesDark` di `stores/appearance.ts` memaksa `.dark` selama tema ini aktif
        (`isDarkForced`). Tombol Light/Dark di title bar dinonaktifkan dan diberi keterangan. Pilihan
        mode user tetap tersimpan dan berlaku lagi saat ganti tema
      - Teks redup aplikasi (`text-neutral-400/500`) di tema ini diangkat jadi putih transparan
        dengan meng-override `--color-neutral-400/500` di dalam `[data-theme='skyglass']`. Ini
        satu-satunya tema yang menyentuh palet Tailwind, dan sengaja dibatasi ke dua tingkat itu
      - Ikon aksen dibuat putih, dan wordmark "Redscale" diberi halo terang (`.brand-wordmark`)
        supaya tetap terbaca di latar biru tanpa mengubah warnanya
      - Pola awan berupa elips yang di-blur di dalam SVG. Elips dijaga jauh dari tepi tile, karena
        blur yang melewati tepi terpotong lurus dan terlihat sebagai garis
      - Dirender lewat build + headless Chrome; belum dilihat di app sungguhan
- [x] Menu `Appearance` di `AppSidebar.vue` (di bawah Support Developer, ikon palet), route
      `/appearance`, `AppearanceView.vue`
- [x] Isi halaman: ~~mode (Light / Dark / System)~~ (dihapus belakangan, lihat keputusan), filter kategori (All / Default / Girls / Mens),
      grid kartu tema dengan miniatur preview memakai `data-theme`, toggle "Show background
      decoration". Perubahan langsung diterapkan dan disimpan di belakang layar
- [ ] Kontras WCAG AA: **belum lolos untuk teks kecil**, termasuk di tema default sejak sebelum
      fase ini. `*-600` di atas glass terang punya kontras sekitar 4.2–4.3:1 (hitungan perkiraan,
      bukan pengukuran di app), sedikit di bawah 4.5:1. Blossom dan Midnight setara dengan Rezure.
      Kalau mau lolos, link dan label kecil perlu naik ke `accent-700`; ini keputusan desain
      untuk semua tema sekaligus
- [x] Dilihat di app sungguhan (`tauri dev`): Blossom dan Midnight di mode light tampil benar,
      preview tiap kartu memakai temanya sendiri
- [ ] Belum dicek: mode dark untuk Blossom/Midnight, restart app (pilihan bertahan dan tidak ada
      kedipan saat start), dan migrasi dari key `rezure-theme` pada instalasi lama

### Fase 3.13d — Menu Decorations (stiker)
Ditambahkan atas permintaan maintainer: stiker lucu (pita pink dan lainnya) yang bisa ditempel di
mana saja di window, dengan preview Rezure untuk memilih posisinya.

**Cara kerja (ringkas)**
- Stiker berupa 24 SVG buatan sendiri di `src/assets/stickers/`, dibagi dua kategori seperti tema.
  **Girls**: bow, heart, sparkle, star, sakura, cloud, strawberry, cat, butterfly, rainbow, crown,
  cherry. **Mens** (outline lebih gelap, warna lebih dingin): gamepad, rocket, bolt, flame, coffee,
  terminal, football, headphones, shield, robot, planet, sunglasses. Tidak ada download dan tidak
  ada aset pihak ketiga. Tepi putih ala stiker dan bayangan dibuat lewat class `.sticker` (filter
  `drop-shadow`) di `main.css`, bukan di SVG-nya, supaya tebalnya sama di semua ukuran
- Posisi (`x`, `y`) dan ukuran disimpan dalam **persen dari window**, jadi satu rumus
  (`stickerStyle` di `stores/decorations.ts`) dipakai baik untuk window asli maupun preview
- Preview di halaman Decorations memakai rasio dan tata letak window yang sedang dipakai (ukuran
  title bar dan sidebar dalam persen) dan ikut berubah saat window di-resize, jadi posisi di preview
  sama dengan posisi di window
- `StickerOverlay.vue` (di `App.vue`) menggambar stiker di layer `fixed inset-0 z-40
  pointer-events-none`: di atas halaman, di bawah modal (`z-50`), dan tidak pernah menghalangi
  klik. Layer ini disembunyikan di halaman Decorations sendiri supaya tidak menutupi editor
- Disimpan di `settings.json` sebagai `decorations { visible, stickers[] }`
  (`config::stickers`). Jenis stiker berupa enum tertutup. Stiker yang tidak dikenal dibuang satu
  per satu tanpa menghilangkan yang lain, nilai dibatasi ke rentangnya, dan jumlah maksimal 40.
  Semua ini juga berlaku untuk data dari frontend (`update_settings`)

**Tasks**
- [x] `config::stickers`: `StickerKind`, `Sticker`, `Decorations::sanitized`, pembacaan lenient.
      Unit test: bentuk JSON, stiker tak dikenal, blok rusak, clamp dan batas jumlah. Round trip
      di test settings
- [x] `Settings::decorations` dan `SettingsPatch::decorations` (mengganti seluruh blok, disanitasi)
- [x] 24 aset SVG stiker (12 Girls, 12 Mens; kategori Mens ditambahkan atas permintaan maintainer)
- [x] Tab kategori di palet (All / Girls / Mens)
- [x] Feedback saat stiker ditambahkan (permintaan maintainer): notifikasi singkat di atas preview
      ("Pink bow added — drag it into place", hilang setelah ~2 detik, juga dibacakan screen reader
      lewat `aria-live`), stiker baru muncul dengan animasi "pop", dan langsung terpilih. Saat
      sudah 40 stiker, notifikasinya memberi tahu batasnya. Duplikat juga diberi notifikasi
- [x] Store Pinia `decorations` (tambah, geser, ubah ukuran dan rotasi, flip, duplikat, bawa ke
      depan, hapus, hapus semua, tampil/sembunyi). Saat drag dan geser slider, perubahan hanya di
      layar; penyimpanan terjadi saat dilepas
- [x] `DecorationsView.vue` (`/decorations`, di sidebar di bawah Appearance): palet stiker, preview
      dengan drag, panah untuk geser (Shift untuk langkah besar), Delete untuk hapus, panel kontrol
      stiker terpilih, "Remove all" dengan konfirmasi klik kedua
- [x] `StickerOverlay.vue` di `App.vue`
- [x] Dicek lewat build + headless Chrome dengan backend tiruan: posisi di preview cocok dengan
      window asli
- [ ] Belum dicoba di app sungguhan (`tauri dev`): drag dengan mouse, penyimpanan lewat restart

### Fase 3.13c — Kustomisasi lanjutan (sebagian dikerjakan)
Bagian "Adjustments" di halaman Appearance. Semua nilainya ikut `AppearanceSettings` di
`settings.json` (`brightness`, `saturation`, `glassSolidity`, `uiScale`, `reduceMotion`), dibatasi
rentangnya di Rust (`AppearanceSettings::sanitized`, dipanggil saat load dan saat patch), dan ikut
cache tampilan pertama.

- [x] **Brightness** (70–120%) dan **Colour intensity** (50–150%): `filter: brightness() saturate()`
      di `<html>`. Di elemen root, filter tidak menjadikan halaman containing block untuk
      `position: fixed`, jadi modal dan layer stiker tidak terganggu. Di nilai 100% filternya
      tidak dipasang sama sekali, supaya tampilan default tidak menambah beban compositing
- [x] **Glass opacity** (0–60%): lapisan warna solid di atas `.glass`, `.glass-strong`,
      `.glass-btn` lewat `--glass-fill`/`--glass-solidity`. 0 = tema apa adanya
- [x] **Interface size** (90 / 100 / 110 / 125%): zoom webview asli (`setZoom`, permission
      `core:webview:allow-set-webview-zoom`), bukan CSS `zoom` yang membuat layout `100vh`
      ikut membesar dan meluber. Preview di halaman Decorations tetap pas karena ukuran window
      dalam CSS px ikut berubah
- [x] **Reduce motion**: class `reduce-motion` di `<html>` mematikan transition dan animation
- [x] Tombol "Reset to default" (hanya bagian Adjustments, tema dan dekorasi tidak ikut)
- [x] Slider memakai warna aksen (`accent-color` untuk `input[type=range]`)
- [x] Unit test: nilai di luar rentang dibatasi. Dirender lewat build + headless Chrome dengan backend
      tiruan
- [ ] Belum dicoba di app sungguhan: zoom webview (termasuk izinnya) dan bertahan setelah restart
- [ ] Masih ditunda: override warna aksen dari palet pilihan

### Di luar v3
Gambar latar sendiri (perlu copy file ke Rezure home dan asset protocol Tauri), export/import tema
sebagai JSON, dan tema dari komunitas.

---

## Dependency ke Proyek Lain

Fase 3.4: `GET /api/v1/version/latest` di `laravel-api` **sudah** mengembalikan manifest bertanda tangan sesuai [`docs/version-contract.md`](../version-contract.md) (`VersionController`, kolom `signature`/`download_url` di `releases`). Yang masih tersisa cuma verifikasi end-to-end lawan rilis nyata — repo ini belum punya pipeline yang build+sign installer, jadi belum ada rilis sungguhan buat diuji; sementara kode sisi app sudah bisa diuji lokal lawan manifest tiruan (prosedur ada di doc kontrak itu). Fase 3.1, 3.5, 3.6, 3.10, 3.11, dan 3.13 sepenuhnya independen, tidak bergantung pada backend.

**Catatan soal analytics lanjutan (v3 `rezure-dashboard`):** fitur traffic by hour, breakdown negara, cohort retention, dll di dashboard **tidak membutuhkan perubahan apapun di app ini** — semua data granular yang dibutuhkan (timestamp, OS version, metadata service) sudah terkirim sejak fondasi telemetry v2. Geolocation negara diproses di sisi server dari IP request yang masuk, bukan dikirim dari client.

## Urutan Pengerjaan yang Disarankan

1. Fase 3.1 (One-click Tunneling) — independen, langsung menambah nilai bagi user
2. **Fase 3.11 (Per-Project PHP Version) — selesai duluan**, di luar urutan aslinya, karena jadi
   fondasi yang dibutuhkan Fase 4.5 di v4 (Project Health Dashboard — project ↔ service/port
   mapping) dan nempel ke infrastruktur yang sama dengan 3.6 (`php_ini.rs`/`php_ext.rs`/`vhosts.rs`)
3. Fase 3.6 (Bundled PHP Extension Toggle) — nempel langsung ke infrastruktur `php_ini.rs`/`php_ext.rs` yang sudah ada, scope kecil dan kontributor-friendly
4. Fase 3.10 (Multi-Version Installer) — nyentuh halaman Switch dan `php_catalog.rs`, sebaiknya sebelum Fase 3.5.1 (Node.js switcher) yang bakal butuh katalognya
5. Fase 3.4 (Auto-Update) — sisa cuma verifikasi E2E, butuh pipeline rilis nyata dulu
6. Fase 3.13 (Appearance) — 3.13a dan 3.13b sudah dikerjakan di branch `glassmorph` (bersama
   redesain glassmorphism, karena 3.13a menyentuh file yang sama). Sisa verifikasi manual ada di
   daftar task 3.13b. 3.13c ditunda

**Fase 3.3 (Project Health Dashboard) dipindah ke v4** (jadi Fase 4.5) atas permintaan maintainer
— lihat `docs/v4/rezure-app-v4-phases-tasks.md`.