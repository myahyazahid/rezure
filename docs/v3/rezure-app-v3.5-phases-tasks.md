# Rezure (Desktop App) — v3.5: Additional Language/Service Support

Fase tambahan di antara v3 dan v4, fokus menambah dukungan Node.js version switching serta Mailpit sebagai service baru — memanfaatkan fondasi `Service` trait yang sudah ada, tanpa perlu sistem plugin dinamis. Redis (bekas Fase 3.5.2 di file ini) dipindah ke v4 atas permintaan maintainer — lihat `docs/v4/rezure-app-v4-phases-tasks.md`.

---

## Fase 3.5.1 — Node.js Version Switching

**Tujuan:** User bisa switch versi Node.js aktif, mirip cara kerja PHP version switcher yang sudah ada.

**Dikerjakan lebih dulu, di luar urutan yang disarankan** di bagian bawah dokumen ini (harusnya nomor 3, setelah Redis/Mailpit) — permintaan langsung maintainer, bukan karena ketergantungan teknis ke Fase 3.5.2/3.5.3.

### Cara kerja (ringkas)

> **Diubah di v3.3.0.** Project JavaScript/Node sekarang disajikan nginx lewat dev server-nya
> (serve profile `proxy`), dan Rezure bisa menjalankan dev server itu sebagai `Service` yang
> opt-in per project — tidak di Dashboard, tidak ikut Start all, tidak di-restart otomatis. Pin
> versi Node per project dari fase ini ikut dipakai dev server. Lihat
> `docs/v3/rezure-app-v3.3.0-phases-tasks.md`. Paragraf di bawah dibiarkan apa adanya sebagai
> catatan keputusan saat itu.

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

**Sudah diverifikasi ulang oleh maintainer setelah fix ini** — laporan bug awal datang dari testing manual sebelum fix ditulis; fix-nya lolos `cargo test`/`cargo clippy`/`cargo fmt`, lalu dicoba ulang manual di app sungguhan dan berjalan dengan baik.

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
- [x] **Tambahan atas permintaan maintainer:** Mailpit juga bisa di-install dari halaman **Switch**
      (baris sendiri di bawah MariaDB, pola yang sama dengan Nginx: satu versi yang di-pin, jadi
      baris itu melaporkan terinstall/tidak, bukan dropdown pilih versi) dan dari modal **Install
      version** (ubin Mailpit). Langkah Nginx di modal dijadikan generik untuk semua paket pinned
      (`PINNED = ['nginx', 'mailpit']`) alih-alih diduplikasi. `binariesStore.install` sekarang
      me-refresh daftar service setelah berhasil — sebelumnya install dari Switch/modal tidak
      mengubah kartu Services (tetap "Not installed") sampai app dibuka ulang, karena daftar service
      cuma di-fetch saat startup dan saat ada event supervisor
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
- Tombol Install diklik maintainer di jendela app sungguhan (unduhan nyata lewat `AppHandle`) —
  berjalan dengan baik

---

## Perbaikan UI (di luar fase, atas permintaan maintainer)

Murni frontend — tidak ada perubahan command, store logic, atau skema.

- [x] **Add connection jadi dialog dua panel.** Kiri: Server type (logo asli), Server engine,
      preview baris koneksi yang live; kanan: form (Host / IP + Port terpisah, SSH, Read-only,
      More options). `TechIcon` dapat logo asli MySQL (dolphin) dan SQL Server dari devicon (MIT)
- [x] **Changelog bisa di-collapse**; status buka/tutup disimpan di store `changelog` selama app
      jalan, hanya rilis yang baru muncul yang terbuka otomatis
- Verifikasi: `eslint`, `prettier --check`, `vue-tsc` bersih. Tampilan Add connection sudah dilihat
  maintainer di app; collapse changelog yang tetap tertutup belum dicek di app
- [x] **Tombol "Install version" di halaman Switch.** Label sempat jadi "Install" saja dengan ikon
      "+", yang terlihat gemuk dan tidak cocok maknanya ("+" di app ini berarti "New …"). Sekarang
      "Install version" dengan ikon download yang sama seperti tombol Install di `ServiceRow.vue`,
      sejajar dengan "New project"/"New database". Layout halaman Switch tetap satu kolom (opsi
      dua kolom, grid kartu, master-detail, dan chip versi sudah dicoba di canvas desain dan tidak
      dipakai)

---

## Tema Appearance tambahan (atas permintaan maintainer)

Lanjutan Fase 3.13 (`docs/v3/rezure-app-v3-phases-tasks.md`). Sebelas tema baru dari rekomendasi
yang disetujui maintainer, total jadi 25. Kategori tetap hanya catatan di komentar kode (tab
kategori sudah dihapus di v3.5), dengan satu kategori baru, **Developer**:

| Kategori | Tema | Aksen | Latar (mesh) & dekorasi |
|---|---|---|---|
| Default | **High Contrast** | biru tua (`500` = `blue-700`) | permukaan solid tanpa glass, tepi 1px tegas, tanpa warna latar, pola, dan blur. Teks redup (`neutral-400/500`) dinaikkan dua tingkat, seperti Sky Glass |
| Default | **Aurora** | teal, ujung gradien violet | mint dan violet dengan pita cahaya; versi dark berupa aurora di langit malam dengan bintang |
| Girls | **Honey** | amber (`600` ke atas satu tingkat lebih gelap supaya teks aksen terbaca di krem) | krem dan kuning madu, sarang lebah kecil |
| Girls | **Berry** | raspberry (fuchsia dicampur rose) | plum dan berry, lebih tua dari pink Blossom, buah berry kecil |
| Mens | **Synthwave** | fuchsia, ujung gradien oranye | pink, violet, sedikit cyan; grid retro |
| Mens | **Arctic** | cyan (`500` = `cyan-600`, dikembalikan di mode dark) | putih salju dan biru es, kepingan salju |
| Developer | **Nord** | Frost `#5e81ac` | Snow Storm (light) / Polar Night (dark), grid titik |
| Developer | **Catppuccin** | Mauve | Latte (light) / Mocha (dark), jejak kaki kucing |
| Developer | **Dracula** | ungu, ujung gradien pink | lavender pucat dari warna Dracula (light) / latar Dracula `#282a36` (dark), kelelawar |
| Developer | **Gruvbox** | oranye, ujung gradien kuning | pasir Gruvbox light / arang hangat Gruvbox dark, garis diagonal tipis |
| Developer | **Tokyo Night** | biru, ujung gradien magenta | Tokyo Night Day / Night, rintik hujan |

- [x] Rust: varian baru di `ThemePreset` (`highcontrast`, `aurora`, `honey`, `berry`, `synthwave`,
      `arctic`, `nord`, `catppuccin`, `dracula`, `gruvbox`, `tokyonight`), dan test "setiap nama
      tema terbaca sebagai dirinya sendiri" ikut diperluas
- [x] Frontend: `ThemePreset` di `src/types/settings.ts`, daftar `THEMES` di `stores/appearance.ts`
      (diurutkan dekat tema sejenis, lima tema Developer di akhir), blok light dan dark di
      `main.css`
- [x] Lima palet Developer hanya menyebut satu warna aksen, jadi skala 50–950 diturunkan sekali
      di `main.css` dari `--accent-base` lewat `color-mix` (ke putih untuk tingkat terang, ke hitam
      untuk tingkat gelap). Tiap tema cukup mengisi `--accent-base`. Di mode dark, aksen Nord,
      Catppuccin, Dracula, dan Tokyo Night digeser ke tengah antara warna light dan dark palet
      aslinya, karena warna pastel versi dark terlalu pucat untuk isian dengan teks putih
- [x] High Contrast mengganti token glass, jadi blok dark-nya meng-override setiap token yang sama
      (aturan yang sama dengan Soft Pink/Navy/Full Glass)
- Verifikasi: `cargo fmt --check`, `cargo clippy`, test `config::settings`, `prettier --check`,
  `eslint`, `vue-tsc`, dan `vite build` bersih, dan kesebelas selector ada di CSS hasil build.
  **Belum dilihat di app sungguhan**, baik light maupun dark. Kontras teks kecil di tema baru
  belum diukur, kecuali High Contrast yang memang dirancang untuk lolos 4.5:1
- Nama palet Developer (Nord, Catppuccin, Dracula, Gruvbox, Tokyo Night) dipakai sebagai nama
  tema. Paletnya open source; aturan pemakaian nama dari masing-masing proyek belum dicek

---

## Stiker Live tambahan (atas permintaan maintainer)

Lanjutan Fase 3.13d (`docs/v3/rezure-app-v3-phases-tasks.md`). Empat belas stiker baru di tab
Live, total jadi 36. Gaya dan aturannya sama dengan 22 yang lama: SVG original `viewBox 0 0 100
100`, outline `#6b2d45`, mata berkedip, pipi pink, animasi CSS di dalam SVG hanya `transform`/
`opacity`, dan mati saat `prefers-reduced-motion`.

| Stiker | Gerakan khas | Catatan |
|---|---|---|
| `baby-bee` | sayap bergetar cepat, antena berkedut, melayang | pasangan tema Honey |
| `baby-bat` | sayap mengepak dari bahu, taring kecil | pasangan tema Dracula. Sayap kanan = cerminan sayap kiri supaya selalu seirama |
| `baby-owl` | kepala dan badan miring pelan, sayap bergoyang | pasangan tema Tokyo Night |
| `baby-arctic-fox` | ekor tebal bergoyang, telinga berkedut | pasangan tema Arctic. Putih polos dengan bayangan biru es dan satu ekor, supaya beda dari kitsune |
| `baby-crab` | capit membuka-menutup dua kali, jalan ke samping | anggukan ke Rust, tapi kepiting generik, bukan Ferris |
| `baby-snail` | antena bergoyang, merayap pelan | |
| `baby-octopus` | enam tentakel melambai tidak serempak | sengaja tidak mirip Octocat GitHub (bukan kucing, warna coral) |
| `baby-red-panda` | ekor belang bergoyang, telinga berkedut | |
| `baby-turtle` | kepala masuk ke cangkang lalu keluar lagi | |
| `baby-elephant` | belalai mengayun, telinga mengipas | |
| `baby-jellyfish` | kepala berdenyut sambil melayang, tentakel melambai | |
| `baby-lamb` | bulu wol mengembang pelan, telinga berkedut | |
| `baby-unicorn` | surai bergoyang, kilau muncul di dekat tanduk | |
| `baby-kitty` | lompat kecil dengan squash & stretch, kepala bergoyang, mata melirik kiri-kanan dan kedip dua kali, sesekali "meow" (mulut terbuka), kumis dan kedua telinga berkedut, pita bergoyang, kaki depan melambai, tiga hati muncul dan melayang | dibuat lebih ramai dari stiker lain atas permintaan maintainer ("kurang hidup"). Mulut terbuka/tertutup ditukar lewat `opacity`, jadi tetap hanya `transform`/`opacity`; versi diam menampilkan mulut tertutup | ditambahkan belakangan: versi Live dari stiker Classic "Kitty" (`cat.svg`), wajah dan pita yang sama. Di palet tepat setelah "Sleepy kitten" |

- [x] Rust: varian baru di `StickerKind` (kebab-case otomatis, jadi `BabyArcticFox` =
      `"baby-arctic-fox"`). Test `live_kinds_are_kebab_case_and_animate_defaults_on` ditambah
      cek untuk nama dua kata, karena nama itu harus sama dengan nama file SVG-nya
- [x] Frontend: `LiveStickerKind` di `src/types/settings.ts` dan `CATALOG` di
      `stores/decorations.ts` (urutan di palet: setelah kitsune). File SVG di
      `src/assets/stickers/live/` otomatis ikut lewat `import.meta.glob`
- [x] **Dua belas stiker Classic bekas kategori Mens digambar ulang** (permintaan maintainer,
      "classic jelek"): `bolt`, `coffee`, `flame`, `football`, `gamepad`, `headphones`, `planet`,
      `robot`, `rocket`, `shield`, `sunglasses`, `terminal`. Sebelumnya outline navy `#1e293b`
      dan warna clip-art yang bentrok dengan stiker lain; sekarang gayanya sama dengan Live dan
      12 Classic lainnya: outline `#6b2d45`, warna pastel, kilau putih, dan wajah kawaii (petir,
      cangkir, api, bola, gamepad, planet, robot, roket lewat jendelanya, tameng). Kacamata jadi
      lensa hati, headphone dapat not musik, terminal menampilkan wajah `>_<`. Nama file dan
      `StickerKind` tidak berubah, jadi stiker yang sudah ditempel ikut berganti gambar. Masih
      gambar diam (Classic). Dirender di headless Chrome; belum dilihat di app sungguhan
- Verifikasi: keempat belas SVG dirender di headless Chrome, diam dan dibekukan di 50% dan 76%
  animasinya, untuk cek bentuk dan poros putar. **Belum dilihat di app sungguhan** (palet
  Decorations, overlay, switch Animate)

---

## Port dipegang Windows service: stop lewat UAC (atas permintaan maintainer)

Kasus nyata: PostgreSQL Rezure gagal start karena 5432 dipegang `postgres.exe` milik service
`PostgreSQL_For_Odoo` (jalan sebagai akun `openpgsvc`, start type Automatic). Path exe-nya tidak
terbaca dari akun user, jadi `ports::holder` menganggapnya `System` dan kartu hanya bilang
"protected system process" tanpa tombol. Sekarang alurnya sama dengan **Sync hosts file**: pesan
spesifik, lalu satu prompt UAC.

- [x] Modul baru `services::windows_services`. `owning(pid, parent)` membaca daftar service aktif
      dari Service Control Manager (`EnumServicesStatusExW`, tanpa admin) dan mencocokkan pid
      proses pemegang port **atau parent langsungnya** (`pg_ctl.exe` adalah service-nya,
      `postgres.exe` anaknya yang listen). Hanya satu tingkat ke atas, supaya program biasa yang
      dijalankan dari shell di sesi SSH tidak dikira milik `sshd`. Proses `svchost` bersama
      (`SERVICE_WIN32_SHARE_PROCESS`) sengaja tidak dijawab: menghentikan salah satunya cuma
      tebakan dan ikut mematikan service lain. Start type dibaca lewat `QueryServiceConfigW`
- [x] `HolderKind::Service` baru, dan `PortHolder.service` (`name`, `displayName`,
      `startsWithWindows`). Pesannya jadi: *port 5432 is held by the Windows service
      "PostgreSQL_For_Odoo" (postgres.exe, pid 41144)*. Proses milik Rezure tetap dicek lebih
      dulu dan tidak pernah dianggap service
- [x] `ports::reclaim(port, disable_autostart)` menghentikan service lewat
      `utils::elevation::run_script` (`Stop-Service`, opsional `Set-Service -StartupType
      Manual`). Nama service diambil ulang dari SCM di backend, bukan dari frontend, dan masuk ke
      script lewat `quote_ps`. Tanpa `-Force`, supaya service lain yang bergantung padanya tidak
      ikut mati diam-diam; kalau ada, pesan error PowerShell yang ditampilkan. Command
      `free_port` dapat argumen opsional `disableAutostart`
- [x] Error baru: `WindowsServiceStopCancelled` (UAC ditolak) dan `WindowsServiceStopFailed`
- [x] `ServiceRow.vue`: untuk holder service, panel menjelaskan bahwa Windows akan minta izin
      admin, checkbox **Don't start it with Windows** (hanya kalau start type Automatic, default
      mati), dan tombol **Stop the service and start …**. `DevServersModal.vue` memakai jalur
      yang sama dengan label yang menyebut admin
- [x] `docs/v1/databases.md` diperbarui
- Verifikasi: `cargo fmt`, `cargo clippy --all-targets`, test `windows_services`,
  `services::ports`, `utils::error` lolos; `prettier`, `eslint`, `vue-tsc` bersih. Deteksi dicek
  ke service sungguhan lewat `print_real_holders`: Redis (6379), AnyDesk (7070), Print Spooler
  terbaca sebagai `Service`; port `svchost` bersama tetap `System`. **Belum dicek:** jalur parent
  (`pg_ctl` → `postgres`) dan klik tombol + UAC di app sungguhan, karena service Odoo sudah
  dimatikan manual sebelum fitur ini selesai
- Port 80 yang dipegang `System` (http.sys/IIS) tetap tanpa tombol: pid 4 tidak bisa ditelusuri
  ke satu service tertentu

---

## Pesan gagal start dev server yang spesifik (atas permintaan maintainer)

Kasus nyata: Open di `company-profile` (Astro) gagal dengan satu paragraf merah berisi output
mentah: `[astro] Unable to load your Astro config / Cannot find module '@astrojs/node' imported
from '…' / Location: / C:\…\vite\dist\node\chunks\node.js:36285:32`, lalu saran generik "Its
dependencies look incomplete". Nama paketnya tidak disebut di saran, lokasi kode di dalam Vite
ikut tampil, dan "run Install" juga disarankan untuk paket yang tidak ada di `package.json`, yang
tidak akan terbantu oleh Install.

- [x] Modul baru `services::dev_server_failure` menggantikan `dev_servers::summarize_failure`.
      `diagnose()` menghasilkan `StartFailure { what, cause, fix, output }`:
  - **Modul tidak ditemukan** (format Vite, ESM, CommonJS): nama paket diambil dari specifier
    (`@astrojs/node/server` jadi `@astrojs/node`), dan file yang meng-import ikut disebut. Paket
    dicocokkan ke `package.json`: kalau tercantum, fix `install`; kalau tidak, fix
    `addDependency` dengan perintah sesuai package manager (`npm install x`, `pnpm add x`, …).
    Perintah ini **hanya ditampilkan untuk disalin**, tidak pernah dijalankan Rezure, karena
    namanya berasal dari output dev server. Nama yang bukan nama paket npm valid tidak dipakai.
    Path relatif (`./routes/home`) disebut sebagai file yang hilang, tanpa saran Install
  - **Perintah tidak dikenal** (`'vite' is not recognized`): fix `install`
  - **Copy lain sudah jalan**: fix `stopOther`
  - **Port salah**: fix `usePort` (dulu string bebas di `process.rs`)
  - Output: mulai dari baris error pertama, maksimal 12 baris. Stack frame, banner npm, baris
    `Location:`, dan baris lokasi kode (`C:\…\x.js:1:2`, `file:///…`) dibuang
- [x] Pesan `DevServerFailed` jadi pendek: *company-profile's dev server didn't come up -
      @astrojs/node isn't installed (imported by astro.config.mjs). Run Install in Dev servers,
      then start it again*. Detailnya disimpan di `ProcessService`, dibaca lewat method trait
      baru `Service::start_failure()` (default `None`, hanya dev server yang mengisinya), lalu
      dikirim sebagai `DevServerRow.lastFailure`. Dibersihkan saat start berikutnya
- [x] Komponen baru `DevServerFailureCard.vue`: judul, penyebab, kotak **Recommended fix**
      berisi penjelasan singkat dan tombol aksinya (**npm install & open**, **Stop the other
      copy & open**, perintah + **Copy** + **Open terminal here**, **Set the port in Dev
      servers**, atau **Try again**), lalu **What it printed** yang bisa dilipat (terbuka otomatis
      kalau penyebabnya tidak dikenali). Dipakai di halaman Projects menggantikan teks merah (bisa
      ditutup), dan versi ringkas di baris dialog Dev servers. Error yang sama tidak diulang di
      bagian bawah dialog
- [x] `docs/v1/projects.md` diperbarui
- Verifikasi: `cargo fmt`, `cargo clippy --all-targets` bersih; 9 test baru di
  `dev_server_failure` (termasuk output asli kasus Astro di atas, paket yang tidak tercantum,
  file lokal yang hilang, nama paket berbahaya ditolak) dan test `dev_servers`/`process` lolos.
  `npm run type-check`, `npm run lint`, `prettier` bersih. **Belum dilihat di app sungguhan**
  (tampilan kartu light/dark, alur tombol Install & open)

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
