# Rezure (Desktop App) — v4: Multi-Runtime Architecture

Ditaruh di v4, bukan v3, karena isinya bukan fitur yang tinggal dipecah jadi checklist — tiap
fase di sini mengubah salah satu asumsi paling inti di codebase (Fase 4.1: satu proses PHP untuk
seluruh app; Fase 4.2: database yang dikelola Rezure selalu lokal dan selalu milik Rezure), dan
butuh keputusan desain dulu sebelum ditulis jadi task implementasi.

---

## Fase 4.1 — Per-Project PHP Version (riset arsitektur)

**Tujuan:** Dua project yang jalan bersamaan bisa pakai versi PHP yang beda — project A di PHP
8.1, project B di PHP 8.3 — tanpa perlu gonta-ganti "active version" tiap pindah project. Ini
gap dibanding Herd/Valet, yang jadi salah satu alasan orang pindah dari tool macam Laragon.

### Kenapa ini bukan toggle sederhana

Ditelusuri langsung dari kode yang ada sekarang:

- `services::process::ProcessService::php()` cuma spawn **satu** `php-cgi.exe`, di port tetap
  `vhosts::PHP_FASTCGI_PORT` (`= 9000`) — satu proses, siapapun versi yang lagi aktif.
- **Setiap** vhost yang digenerate `vhosts::vhost_config()` menunjuk ke port yang sama persis
  (`fastcgi_pass 127.0.0.1:9000;`) buat semua project tanpa kecuali.
- "Switch versi" di halaman Switch itu, secara harfiah: matiin `php-cgi` yang lama, nyalain yang
  baru, di port yang sama. PHP version hari ini adalah properti **aplikasi**, bukan properti
  **project**.
- `ProjectInfo` (`db::projects`) dan `links.json` (`config::links`) belum punya field apapun
  yang berhubungan dengan runtime — cuma path, nama, domain, stack.

### Yang harus berubah

1. **PHP jadi pool per-versi, bukan satu proses.** Tiap versi PHP yang sedang dipakai minimal
   satu project = satu `php-cgi.exe` sendiri, di port masing-masing (dialokasikan otomatis, bukan
   hardcode `9000`).
2. **Project butuh field baru**: `phpVersion` opsional — kosong berarti ikut default/global
   (perilaku hari ini, jadi backward compatible). Disimpan di SQLite buat project hasil scan
   `www/`, dan di `links.json` buat project yang di-link — pola yang sama seperti domain custom
   yang sudah ada untuk linked project.
3. **`vhost_config()` gak lagi menerima `PHP_FASTCGI_PORT` konstan** — portnya diturunkan dari
   pool yang cocok dengan versi PHP project itu. `sync_vhosts()` juga perlu tahu pool mana yang
   harus hidup/mati berdasarkan versi apa saja yang sedang benar-benar dipakai project aktif.
4. **Dashboard/Services page kena imbas.** "PHP: Running/Stopped" sekarang satu baris status;
   dengan multi-pool ini jadi daftar (`PHP 8.1 — running, 2 project` / `PHP 8.3 — running, 1
   project`). UI status service perlu redesign kecil, bukan cuma backend.
5. **Composer/scaffolding** (`services::scaffold`) yang sekarang selalu resolve ke "versi aktif
   global" juga perlu tahu project mana yang lagi discaffold, kalau override per-project berlaku
   di sana juga.

### Pertanyaan yang perlu diputuskan sebelum ditulis jadi task

- Alokasi port pool: statis per-versi (misal turunan dari hash versi) atau dicari port bebas
  saat runtime dan disimpan di state in-memory?
- Pool yang tidak dipakai project manapun lagi — dimatikan otomatis, atau dibiarkan hidup sampai
  restart app? (Trade-off resource vs. kecepatan buka project berikutnya.)
- Override per-project ini berlaku juga untuk Composer/`artisan` yang dijalankan dari dalam app
  (scaffolding, tombol-tombol project), atau cuma untuk request web lewat nginx?
- UI: dropdown versi PHP di mana — di halaman Projects (row per project, mirip badge "Stack"
  yang sudah ada), di project detail, atau dua-duanya?
- Apakah "PHP Everywhere" (PATH link di `php_path.rs`) tetap merujuk ke satu versi global, atau
  jadi ambigu begitu override per-project ada? (Kemungkinan besar: tetap satu versi global untuk
  PATH, override per-project cuma memengaruhi request lewat nginx — perlu didokumentasikan biar
  gak jadi kejutan.)

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`, tidak menyentuh `laravel-api` maupun
`rezure_websites`.

---

## Fase 4.2 — Remote Database Connections (list, export, import)

**Tujuan:** Halaman Databases bisa diarahkan ke server MySQL/MariaDB yang bukan milik Rezure —
staging, VPS, atau instance tim — lalu menampilkan daftar database-nya dan export/import dari
dalam Rezure, tanpa perlu buka DBeaver/Workbench cuma buat narik satu dump.

### Kenapa ini masuk v4

Sama seperti Fase 4.1, ini mengubah asumsi inti: **database yang ditampilkan Rezure selalu lokal
dan selalu dijalankan sendiri oleh Rezure**. Hampir semua yang ada sekarang berdiri di atas
asumsi itu — profile switcher, kartu `root · no password`, kolom USED BY, sampai status service
MySQL di Dashboard.

### Yang ternyata sudah mendukung

Ditelusuri langsung dari kode yang ada sekarang:

- `database::export_database()` menjalankan `mysqldump` dan me-redirect **stdout** ke file;
  `database::import_sql()` menjalankan `mysql` dengan **stdin** dari file. Dua-duanya bicara
  lewat TCP dan jalan identik ke host remote — tidak satupun mengasumsikan datadir lokal.
- Semua operasi DB (`query`, `execute`, export, import) melewati `database::base_args()`.
  Host/port/user ngumpul di satu fungsi, jadi titik ubahnya cuma satu.
- Tidak ada driver MySQL di dependency tree — semua lewat client binary yang ikut tiap build
  server. Artinya dukungan remote **tidak** butuh crate protokol baru.

### Yang mengunci semuanya ke lokal

| Tempat | Yang mengunci |
|---|---|
| `database::base_args()` | `HOST` hardcode `127.0.0.1`, port dari profile aktif, user selalu `root`, dan **tidak ada jalur password sama sekali** |
| `database::bin_dir()` | Client binary di-resolve dari server exe milik profile aktif — target remote tidak punya profile, jadi tidak ada binary yang ketemu |
| `database::server_info()` | `has_password` hardcode `false`, DSN dirakit dari konstanta |
| `database::used_by()` | Mencocokkan nama schema ke project lokal — tidak punya arti untuk server remote |

### "Profile" dan "Connection" itu dua hal berbeda

|  | Profile (yang ada sekarang) | Connection (baru) |
|---|---|---|
| Isi | datadir + engine + versi + `my.ini` + binary | host + port + user + kredensial |
| Proses | Rezure yang spawn `mysqld`-nya | bukan milik Rezure, tidak bisa start/stop |
| Bisa switch? | ya, lewat gate `check_can_switch_to` | ya, tapi tidak ada yang perlu dimatikan dulu |
| Risiko utama | korupsi datadir | operasi tulis ke server produksi |

Penting: **jangan** menambah `ProfileSource::Remote` ke `config::profiles`. Profile hari ini
mendeskripsikan datadir, engine, `my.ini`, binary dan port yang dibutuhkan `mysqld` saat spawn;
connection remote tidak punya satupun dari itu, dan memaksakannya berarti menempelkan `Option<>`
di mana-mana sampai tiap pembaca `profile.datadir_path` jadi bohong. Yang aktif harus naik jadi
`Target`:

```
Target
├── Local(Profile)      → punya datadir, punya proses, bisa start/stop  (yang ada sekarang)
└── Remote(Connection)  → cuma endpoint + kredensial, prosesnya bukan milik Rezure
```

### Fase 4.2.1 — Model, penyimpanan kredensial, dan Test connection

Dikerjakan pertama karena semua masalah auth, TLS dan engine skew ketahuan di sini — sebelum
menyentuh fitur lain.

- [x] `config::connections` — `Connection { id, name, host, port, user, engine (hint), tls_mode, read_only, last_used_at }`, disimpan di `%APPDATA%\Rezure\connections.json`, **tanpa password**
- [x] Password disimpan di Windows Credential Manager (crate `keyring`); JSON cuma menyimpan referensinya
- [x] `TempDefaults`: menulis file temp berisi `[client] password=...` dan menghapusnya saat `Drop`. Password **tidak boleh** lewat argv — `-p<pass>` kelihatan di Task Manager, dan itu melanggar aturan "jangan concat user input ke shell command" di `CLAUDE.md`. Satu file cukup untuk `mysql` dan `mysqldump` karena keduanya membaca section `[client]`
- [x] Command `test_connection` yang menjalankan `SELECT VERSION()` dan mengembalikan versi + engine yang terdeteksi
- [x] Dukungan TLS (`--ssl-mode`, CA file opsional) — banyak server remote menolak koneksi non-TLS
- [x] UI `AddConnectionModal.vue`, sejajar dengan `AddDbProfileModal.vue` yang sudah ada; Test connection wajib hijau sebelum connection bisa disimpan
- [x] Error auth/timeout ditampilkan apa adanya dari client, mengikuti pola `database::client_error()`

### Fase 4.2.2 — Refactor jalur koneksi

Murni refactor, tanpa perubahan perilaku untuk target lokal — sengaja dipisah biar gampang
di-review.

- [x] `struct Conn { host, port, user, defaults: Option<TempDefaults>, bin: PathBuf }`
- [x] `active_conn() -> Result<Conn, AppError>` yang me-resolve `Target::Local` maupun `Target::Remote`
- [x] `base_args()` diganti `conn.args()`; `query`, `execute`, `export_database`, `import_sql`, `list_collations` semuanya lewat `Conn`
- [x] `bin_dir()` untuk target remote fallback ke build lokal manapun yang terinstall, **dipilih berdasarkan engine hint connection** — client MariaDB tidak bisa auth `caching_sha2_password` milik MySQL 8, dan itu tidak bisa diakali flag
- [x] Tambahkan `--connect-timeout` ke semua pemanggilan client; default client itu lama, target lokal tidak pernah kena, remote sering

### Fase 4.2.3 — List database dari connection remote (read-only)

- [x] Target aktif jadi `Target`, bukan profile id; switcher di header menampilkan dua grup: **Local profiles** dan **Connections**
- [x] Kartu server (host/user/DSN) menampilkan nilai asli connection, bukan `root · no password` yang hardcode
- [x] Kolom **USED BY** disembunyikan untuk target remote
- [x] ~~Services page menandai MySQL "not managed by Rezure" saat target remote aktif~~ — tidak jadi perlu: memilih connection remote **tidak** menyentuh server lokal, jadi status running/stopped di Services tetap benar apa adanya
- [x] **New database** dan **Delete** default nonaktif untuk remote; dibuka lewat flag `read_only` per-connection
- [ ] Query ukuran ke `information_schema` diberi caching — di server besar query itu mahal, dan tiap read = spawn proses + handshake baru

### Fase 4.2.4 — Export dari server remote

Ini value terbesarnya, dan tetap read-only terhadap server.

- [x] `--single-transaction` untuk target remote — `mysqldump` default pakai `LOCK TABLES`, dan di server bersama itu memblokir tim lain
- [x] `--skip-column-statistics` saat client MySQL 8 nge-dump server MariaDB; tanpa itu: `Unknown table 'COLUMN_STATISTICS'`
- [x] `--set-gtid-purged=OFF` untuk managed DB (RDS/Aiven) — tanpa ini dump-nya tidak bisa di-import balik
- [x] `--max-allowed-packet` / `--net-buffer-length` dinaikkan untuk row besar
- [ ] Progress + cancel: dump remote 2 GB itu menit-menitan, sedangkan UI sekarang cuma spinner. Minimal progress dari ukuran file yang sedang ditulis, plus cancel yang mematikan child process dan menghapus file setengah jadi (pola hapus-file-gagal sudah ada di `export_database()`)
- [x] Hasil export tetap mendarat di folder dumps yang sama, tapi nama file diberi prefix nama connection biar dump staging tidak ketuker dump lokal

### Fase 4.2.5 — Import ke server remote

Ditaruh paling belakang karena ini satu-satunya operasi yang bisa merusak data orang lain.

- [x] Konfirmasi eksplisit (ketik ulang nama database) sebelum import ke target remote — `import_sql()` sekarang diam-diam menjalankan `CREATE DATABASE IF NOT EXISTS` lalu mengeksekusi seluruh isi file
- [x] Hormati flag `read_only`: kalau connection ditandai read-only, import tidak muncul sama sekali
- [x] Pesan error privilege ditampilkan apa adanya — user remote sering tidak punya `CREATE`
- [ ] Progress/cancel mengikuti pola Fase 4.2.4

### Fase 4.2.6 — SSH tunnel

- [x] Pakai `ssh.exe` bawaan Windows 10+ (`ssh -N -L <local>:<dbhost>:<dbport> user@host`), bukan embed crate SSH — key auth, `~/.ssh/config` dan ssh-agent didapat gratis, dan maintenance-nya nol
- [x] Dukung dua mode auth: private key (`-i`), dan **password lewat `SSH_ASKPASS`** — OpenSSH for Windows 9.5p2 memanggil helper askpass, dan helper-nya adalah executable Rezure sendiri yang di-re-enter sebelum Tauri start, jadi tidak ada binary kedua yang harus dibundel. Password dikirim lewat environment proses `ssh` (per-spawn, bukan environment Rezure), bukan argv
- [x] Proses tunnel disupervisi seperti child process service lain, dan mati bersama connection-nya
- [x] Kalau tunnel hidup, seluruh layer DB tetap menembak `127.0.0.1:<port lokal>` — jadi **tidak ada perubahan sama sekali** di kode Fase 4.2.2–4.2.5. Ini alasannya tunnel ditaruh terakhir, bukan pertama

### Pertanyaan yang perlu diputuskan

- Connection remote boleh create/drop database sama sekali, atau read-only + export/import saja? (Saran: read-only sebagai default, tulis dibuka per-connection.)
- Kredensial: `keyring` saja, atau sediakan juga opsi "jangan simpan password, tanya tiap connect"?
- Kalau tidak ada satupun client binary lokal yang cocok dengan engine server remote — tolak connection-nya di awal, atau tetap sambungkan dan biarkan gagal dengan pesan aslinya?
- Folder dumps: dipisah per-connection (`dumps/<connection>/`) atau satu folder dengan prefix nama?
- Target remote ikut muncul di halaman Switch, atau cuma di dropdown halaman Databases?

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`. Dependency baru satu-satunya: crate
`keyring` untuk kredensial. Tidak butuh driver MySQL maupun crate SSH.

### Urutan Pengerjaan yang Disarankan

1. **4.2.1** — semua risiko teknis (auth, TLS, engine skew) muncul di sini, paling murah kalau ketahuan duluan
2. **4.2.2** — refactor mekanis tanpa perubahan perilaku, jadi fondasi tiga fase berikutnya
3. **4.2.3** — read-only, aman, dan sudah langsung kepakai
4. **4.2.4** — export; ini alasan utama fitur ini dibuat
5. **4.2.5** — import; destruktif, dikerjakan setelah semua jalur lain terbukti stabil
6. **4.2.6** — tunnel; nol dampak ke kode sebelumnya, jadi boleh nyusul kapan saja

---

## Fase 4.3 — Xdebug sebagai Extension Resmi

**Beda kelas dari Fase 4.1/4.2 di atas** — bukan riset arsitektur, task-nya sudah siap kerja
langsung. Dipindah ke v4 dari v3 (bekas Fase 3.7) atas permintaan maintainer, bukan karena
mengubah asumsi inti seperti dua fase lain di file ini.

**Tujuan:** Tutup pertanyaan terbuka di [Fase 3.1b](../v3/rezure-app-v3-phases-tasks.md#fase-31b--pecl-extension-installer-redis)
("`imagick`? dst.") dengan menjadikan Xdebug ekstensi PECL resmi yang bisa dipasang lewat app,
plus konfigurasi step-debugging yang biasanya jadi hambatan tersendiri di luar sekadar
"install DLL"-nya.

### Tasks
- [ ] Tambahkan `xdebug` ke katalog `php_ext.rs` (SHA-256 per branch PHP, mengikuti pola `redis`)
- [ ] Xdebug beda dari extension biasa: butuh `zend_extension=xdebug` (bukan `extension=`), jadi
      `php_ini.rs` perlu jalur khusus buat baris ini — pola ini sudah ada presedennya:
      `services::php_ext_toggle` (v3 Fase 3.6) sudah menangani kasus yang sama persis untuk
      `opcache`, lihat `ExtensionMeta::zend_extension` dan lookup di `php_ini::render()`
- [ ] UI konfigurasi dasar: `xdebug.mode` (off/debug/develop), `xdebug.client_port`, `xdebug.client_host` — bukan raw ini editor, cukup pilihan umum yang paling sering dipakai
- [ ] Auto-generate `.vscode/launch.json` di root project saat Xdebug diaktifkan untuk project itu (kalau folder `.vscode` belum ada/belum punya konfigurasi PHP debug) — nilai tambah yang gak ditawarkan Laragon maupun kompetitor lain
- [ ] Peringatan performa: aktif tapi `xdebug.mode=off` tetap ada overhead loading modul — jelaskan di UI, jangan nyalain semua mode sekaligus by default

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`, sama seperti Fase 4.1.

---

## Fase 4.4 — Queue Worker Supervision

**Sama seperti Fase 4.3** — bukan riset arsitektur, task-nya sudah siap kerja langsung. Dipindah
ke v4 dari v3 (bekas Fase 3.8) atas permintaan maintainer.

**Tujuan:** `php artisan queue:work` gak lagi jadi proses yang ditinggal manual di satu terminal
yang gampang ke-close atau kelupaan — disupervisi persis kayak service lain.

### Tasks
- [ ] Manfaatkan infra process management yang sama dengan `Service` trait (start/stop/restart, log lewat `ServiceLogPanel.vue`)
- [ ] Scope per-project, bukan global — satu project bisa punya worker sendiri, jalan/berhenti independen dari project lain
- [ ] Deteksi otomatis project mana yang punya `artisan` (Laravel) sebagai syarat munculnya opsi ini
- [ ] UI: tombol "Start Queue Worker" di project card/detail (dekat tombol Open/Terminal yang sudah ada), dengan indikator running/stopped
- [ ] Opsi dasar: pilih koneksi queue (`--queue=default`, dst) kalau project punya lebih dari satu — sisanya pakai default `artisan`
- [ ] Worker ikut berhenti kalau PHP di-restart/di-switch versi (proses lama sudah tidak valid), dengan notice ke user — bukan dibiarkan jadi proses PHP versi lama yang nyangkut

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`.

---

## Fase 4.5 — Project Health Dashboard

**Sama seperti Fase 4.3/4.4** — bukan riset arsitektur, task-nya sudah siap kerja langsung.
Dipindah ke v4 dari v3 (bekas Fase 3.3) atas permintaan maintainer.

**Tujuan:** Ringkasan kondisi tiap project dalam satu pandangan. Sekarang halaman Services cuma
nunjukin status per-service (PHP jalan/mati, Nginx jalan/mati, dst) secara global — fase ini
nambahin pandangan per-project: "project ini sehat apa nggak", bukan cuma "service X jalan apa
nggak".

**Prasyaratnya sudah ada:** dulu gap terbesar fase ini adalah project belum punya konsep "pakai
service/port yang mana" — PHP versi cuma satu secara global, semua vhost proxy ke port yang sama.
[Fase 3.11](../v3/rezure-app-v3-phases-tasks.md#fase-311--per-project-php-version-concurrent) (v3)
menutup gap itu: `ProjectInfo` sekarang punya `php_version` sendiri per-project, dan
`services::php_pool` mapping versi ke port. Konsolidasi status di bawah ini bisa langsung baca
dari situ, bukan mulai dari nol.

### Tasks
- [ ] Konsolidasi status semua service terkait per-project dalam satu view — tiap project card
      nunjukin PHP versi/port yang dipakai (dari `services::php_pool`), status vhost nginx-nya, dan
      status database yang dia butuhin, sekali liat tanpa harus cek satu-satu ke halaman Services
- [ ] Port conflict detector yang lebih menyeluruh (across semua project, bukan cuma saat start
      service) — deteksi proaktif, bukan cuma pas tombol Start diklik
- [ ] Tampilkan ukuran log per service, dengan opsi clear log
- [ ] Indikator visual sederhana (misal: sehat/perlu perhatian) berdasarkan status gabungan

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`, sama seperti Fase 4.1/4.3/4.4.

---

## Fase 4.6 — Redis Support

**Sama seperti Fase 4.3/4.4/4.5** — bukan riset arsitektur, task-nya sudah siap kerja langsung.
Dipindah ke v4 dari v3.5 (bekas Fase 3.5.2) atas permintaan maintainer.

**Tujuan:** Redis tersedia sebagai service baru di Rezure, konsisten dengan service lain yang
sudah ada — lewat `Service` trait yang sama, tanpa sistem baru.

### Tasks
- [ ] Implementasikan Redis dengan `Service` trait yang sudah ada (`start()`, `stop()`, `status()`,
      `restart()`) — tidak perlu sistem baru
- [ ] Bundle Redis portable binary untuk Windows — Redis resmi sudah tidak menerbitkan binary
      Windows lagi (deprecated dari Redis Inc sendiri), jadi perlu diputuskan dulu fork mana yang
      masih maintained (mis. `tporadowski/redis`, atau Memurai) sebelum jalur download+checksum-verify
      bisa ditulis — mirip masalah checksum Nginx/Python di v3, tapi opsinya lebih jelas di sini
- [ ] Tambahkan Redis ke list service di Dashboard/Services (port default `6379`)
- [ ] Deteksi port conflict untuk Redis, konsisten dengan service lain
- [ ] Log viewer Redis mengikuti pola log viewer service lain yang sudah ada
- [ ] (Opsional, bisa nyusul) Mini Redis viewer — quick-view keys yang tersimpan, tanpa perlu tool
      eksternal (RedisInsight, dll)

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`, sama seperti Fase 4.1/4.3/4.4/4.5.

---

## Fase 4.7 — Nginx Multi-Version Catalog

Dipindah ke v4 dari v3 (bekas open question di Fase 3.10, Multi-Version Installer) atas permintaan
maintainer. Beda dari Fase 4.5/4.6: ini **belum siap kerja langsung** — butuh keputusan dulu
sebelum ditulis jadi task, sama seperti Fase 4.1/4.2.

**Tujuan:** Halaman Switch bisa install lebih dari satu versi Nginx, sama seperti PHP/MariaDB/
Composer/Node.js sejak Fase 3.10 (v3). Sekarang Nginx satu-satunya runtime yang masih dipin ke
**satu** versi (1.25.3, rilis 2023) lewat `binaries::MANIFEST` — sudah lumayan ketinggalan, tapi
modal `InstallVersionModal.vue` di Switch page sudah siap nampung katalog beneran begitu ada.

### Kenapa belum bisa langsung ditulis jadi katalog

Aturan codebase ini tegas — **tidak ada download tanpa SHA-256 terverifikasi** (lihat alasannya di
`php_catalog.rs` dan `php_ext.rs`). Nginx tidak menerbitkan index rilis yang bisa dibaca mesin
**maupun** file checksum apapun: di `nginx.org/download/` tiap `.zip` cuma ditemani `.zip.asc`
(signature PGP), bukan hash. [Python](../v3/rezure-app-v3-phases-tasks.md#fase-310--multi-version-installer-untuk-semua-runtime)
punya masalah yang sama persis (API-nya cuma expose MD5).

### Pertanyaan yang perlu diputuskan sebelum ditulis jadi task

Tiga opsi:

- **(a)** Tetap satu versi pinned, tapi rutin di-bump — status quo, tapi 1.25.3 sudah ketinggalan jauh
- **(b)** Tabel pinned berisi beberapa versi, checksum di-hardcode manual — trade-off yang persis sama dengan katalog PECL di `php_ext.rs`, jadi presedennya sudah ada di codebase ini
- **(c)** Implementasi verifikasi PGP — menambah dependency dan urusan manajemen key

**Rekomendasi: (b)**, karena jumlah versi Nginx yang relevan untuk local dev sedikit dan polanya
sudah dikenal di codebase (`nginx_catalog.rs` bisa langsung mencontek bentuk `php_ext.rs`'s
`PeclExtension`/checksum-per-entry).

### Dependency ke Proyek Lain

Tidak ada — sepenuhnya perubahan internal `rezureapp`.

---

## Status

- **Fase 4.1** — belum ada task checklist resmi. Langkah berikutnya: tulis proposal desain
  singkat (pilihan arsitektur pool + jawaban pertanyaan di atas), baru dipecah jadi fase dengan
  task list seperti fase-fase di `docs/v3/`.
- **Fase 4.2** — Fase 4.2.1 sampai 4.2.6 sudah diimplementasikan (dependency baru: crate
  `keyring`; SSH tunnel pakai `ssh.exe` bawaan Windows, tanpa crate tambahan). Yang belum:
  - **Progress + cancel** untuk export/import remote (4.2.4/4.2.5). Sekarang masih overlay
    "Exporting…" tanpa angka dan tanpa tombol batal — cukup untuk dump kecil, kurang untuk dump
    yang makan menit.
  - **Caching query ukuran** `information_schema` (4.2.3).

  Keputusan yang diambil saat implementasi, menjawab pertanyaan desain di atas: connection
  **read-only secara default** (tulis dibuka per-connection lewat checkbox), password boleh
  tidak disimpan (ditanya sekali per sesi, disimpan di memori saja), client engine yang tidak
  cocok **tidak** ditolak di awal melainkan dipakai dengan fallback ke engine satunya, dan dump
  remote mendarat di folder `dumps` yang sama dengan prefix nama connection.
- **Fase 4.3** — belum dikerjakan, task-nya sudah siap (dipindah apa adanya dari v3 Fase 3.7).
  Fondasi `zend_extension=` untuk directive khusus sudah ada duluan lewat `php_ext_toggle`
  (v3 Fase 3.6, dibuat untuk `opcache`) — tinggal dipakai ulang buat `xdebug`, bukan dibangun
  dari nol.
- **Fase 4.4** — belum dikerjakan, task-nya sudah siap (dipindah apa adanya dari v3 Fase 3.8).
  Pola registrasi/unregistrasi service secara dinamis saat runtime sudah ada duluan lewat
  `ServiceManager::sync_php_pool` (v3 Fase 3.11, dibuat untuk instance PHP pooled per-project) —
  worker per-project bisa ikut pola yang sama, bukan dibangun dari nol.
- **Fase 4.5** — belum dikerjakan, task-nya sudah siap (dipindah apa adanya dari v3 Fase 3.3).
  Prasyaratnya (mapping project↔PHP lewat `services::php_pool`) sudah ada duluan lewat v3 Fase 3.11
  — tinggal dikonsolidasi jadi satu view, bukan dibangun dari nol.
- **Fase 4.6** — belum dikerjakan, task-nya sudah siap (dipindah apa adanya dari v3.5 Fase 3.5.2).
  Satu open question belum diputuskan sebelum implementasi bisa mulai: binary Redis Windows mana
  yang mau dipakai, karena Redis resmi sudah tidak menerbitkan build Windows sendiri.
- **Fase 4.7** — belum ada task checklist resmi, sama seperti Fase 4.1 (dipindah dari open question
  di v3 Fase 3.10). Rekomendasi sudah ada (opsi b: tabel pinned beberapa versi, pola `php_ext.rs`)
  — tinggal dikonfirmasi maintainer, baru ditulis jadi task list.
