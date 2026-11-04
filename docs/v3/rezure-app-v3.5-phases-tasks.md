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

## PostgreSQL Rezure untuk aplikasi lain: user, start otomatis, Odoo (atas permintaan maintainer)

Kasus pemicunya: installer Odoo di laptop teman maintainer tidak bisa mencentang komponen
PostgreSQL, jadi Odoo terpasang tanpa database server. PostgreSQL Rezure sebenarnya cocok (Odoo 20
membawa PostgreSQL 16.14, katalog Rezure punya 16.15; cluster Rezure `UTF8 --no-locale`, sama
dengan yang dipakai Odoo), tapi ada empat celah: tidak ada role selain superuser (Odoo menolak
jalan sebagai `postgres`), PostgreSQL Rezure hanya jalan kalau di-start manual, `odoo.conf` ada
di Program Files (butuh admin), dan `pg_path` harus menunjuk ke `bin` PostgreSQL Rezure.

Arahan maintainer: **sebisa mungkin berguna di luar Odoo juga**. Jadi Odoo hanya kasus pertama
dari fitur yang umum: user database, start otomatis, dan potongan konfigurasi untuk aplikasi apa
pun.

### Fase A — User PostgreSQL

- [x] `services::postgres_roles`: daftar role (tanpa `pg_*`), buat role (`LOGIN`, opsional
      `CREATEDB`, password opsional), ganti password, hapus role. Hanya untuk PostgreSQL milik
      Rezure. `postgres` dan `root` tidak bisa dihapus atau diubah. Nama lewat aturan identifier
      yang sama dengan nama database, tanpa awalan `pg_`
- [x] "Buat juga database miliknya": `CREATE DATABASE … OWNER …`. Pola umum hampir semua
      framework (satu app, satu user, satu database)
- [x] Preset: **App** (user + database miliknya), **Odoo** (user `odoo`, `CREATEDB`, tanpa
      database, karena Odoo membuat database sendiri), **Django/Rails** (user + database,
      `CREATEDB` supaya test runner bisa membuat database test)
- [x] UI: tombol **Users** di halaman Databases saat target-nya PostgreSQL Rezure, membuka modal
      daftar user + form tambah (`PostgresUsersModal.vue`). Password opsional, tombol
      **Generate**, tidak disimpan Rezure. User yang masih punya database tidak bisa dihapus,
      database-nya disebut

### Fase B — Start otomatis

- [x] Setting per service `startOnLaunch`: service itu di-start begitu Rezure terbuka. Berlaku
      untuk semua service (PostgreSQL, MariaDB, Nginx, Mailpit, …), bukan khusus PostgreSQL
- [x] Toggle **Start when Rezure opens** di menu "More actions" tiap baris service (menu jadi
      selalu ada, Force stop tetap hanya saat jalan)
- [x] Digabung dengan **Start with Windows** dan **Keep in tray** yang sudah ada, supaya aplikasi
      seperti Odoo (service Windows yang start saat boot) menemukan database-nya jalan
- [x] `services::start_on_launch`: start dijalankan di akhir setup `lib.rs` (setelah vhost ditulis
      dan PHP pool terdaftar, jadi nginx naik dengan konfigurasi terbaru), di thread terpisah
      supaya jendela tidak menunggu database. Service yang disembunyikan, belum terpasang, atau
      sudah jalan dilewati. Gagal satu tidak menghentikan yang lain
- [x] Teks Settings "Start Rezure with Windows" diperbaiki: sebelumnya bilang "Services boot in
      the background at sign-in", padahal tidak ada yang di-start. Sekarang menyebut service
      mana yang ikut naik

### Fase C — Hubungkan aplikasi

- [x] **Connect an app** untuk setiap user: potongan siap salin, yaitu URL `postgresql://`,
      `.env` Laravel, `DATABASE_URL` (Django/Node/Prisma), `database.yml` Rails, JDBC, dan
      `odoo.conf`
- [x] Odoo (aplikasi pertama yang bisa dikonfigurasi Rezure sendiri, karena terpasang di Windows
      dengan file konfigurasi yang diketahui): deteksi instalasi (`Program Files\Odoo*\server\
      odoo.conf` + service `odoo-server-*`), tampilkan status, lalu satu tombol: buat/perbarui role
      `odoo`, backup `odoo.conf`, tulis `db_host`/`db_port`/`db_user`/`db_password`/`pg_path`,
      restart service Odoo, opsional matikan service PostgreSQL bawaan Odoo. Semua lewat **satu
      UAC** (`utils::elevation`). Bisa dikembalikan ke konfigurasi semula
- [x] Peringatan untuk yang sudah punya data di PostgreSQL bawaan Odoo: database lama tidak
      ikut pindah (Fase D, belum dikerjakan)
- [x] Detail implementasi: `services::odoo` (deteksi, `connect`, `restore`, `open`). Penulisan
      `odoo.conf` hanya mengubah lima key itu; baris lain (admin password, addons path,
      komentar, line ending CRLF) dibiarkan. File hasilnya disiapkan di
      folder data Rezure (`data\odoo\`, biasanya `C:\rezure\data\odoo\`) lalu disalin lewat UAC, dan salinan staging (yang
      berisi password) dihapus setelahnya. Hasilnya dicek dengan membaca `odoo.conf` lagi.
      Password `odoo` dibuat acak dan hanya ada di `odoo.conf`. Service PostgreSQL bawaan yang
      dimatikan dicatat di `data\odoo\state.json`, supaya restore hanya menyalakan yang itu
- [x] Helper bersama `utils::elevation::run_logged` (script `try`/`catch` + log error, satu UAC).
      `windows_services::stop` ikut dipindah ke situ. `windows_services::find` sekarang bisa
      mencari service yang terpasang (jalan maupun mati), bukan hanya yang sedang jalan
- [x] UI: baris "Odoo 20.0 is installed on this PC" di halaman Databases (saat target-nya
      PostgreSQL Rezure), membuka `OdooConnectModal.vue`: setting Odoo sekarang, apa yang akan
      dilakukan, checkbox matikan PostgreSQL bawaan, toggle "After a restart" (Fase B +
      setting yang sudah ada), saran versi (bawaan Odoo vs versi aktif Rezure), **Connect to
      Rezure**, **Open Odoo**, **Put back Odoo's own settings**
- [x] Desain ulang modal Odoo (permintaan maintainer, versi pertama terlalu padat): diagram
      Odoo → PostgreSQL Rezure (menyebut service bawaan yang digantikan), tabel perubahan
      `odoo.conf` per baris (nilai lama dicoret, nilai baru disorot), langkah bernomor dengan
      checkbox "Turn off PostgreSQL_For_Odoo" sebagai langkah terakhir, kolom kanan "After a
      restart" dengan status "All set / x of N on", catatan versi dan peringatan data sebagai
      callout. Tombol Connect berikon perisai (tanda minta admin) dan nonaktif kalau PostgreSQL
      belum terpasang. `OdooInstall` dapat `rezurePgPath` dan `rezurePostgresVersion` dari Rust
      supaya nilai baru `pg_path` di tabel sama persis dengan yang akan ditulis
- [x] Kartu **Odoo's Master Password** di modal (kasus nyata setelah Connect berhasil: database
      manager Odoo menolak dengan "bad admin password"). `OdooInstall.masterPassword` hanya
      menyebut status `admin_passwd` (`missing` → `admin`, `plainText`, `hashed`), **tidak pernah
      nilainya**. Untuk `hashed`: ganti nilainya di `odoo.conf` sebagai administrator lalu restart
      service Odoo. Tombol **Open** membuka `odoo.conf` di Notepad sebagai administrator (UAC,
      `elevation::open_elevated`, tidak menunggu Notepad ditutup), supaya file di Program Files
      itu bisa langsung disimpan. Path-nya diambil ulang dari deteksi, bukan dari frontend. Di
      mesin maintainer terbaca `hashed`
- [x] **Start / Stop / Restart** service Odoo (permintaan maintainer: user awam tidak tahu
      `Restart-Service`). Baris sendiri di bawah baris path + **Open** pada kartu Master
      Password: titik status, nama service, lalu **Stop** + **Restart** saat jalan atau **Start**
      saat mati. **Start**/**Restart** menyalakan PostgreSQL Rezure dulu kalau Odoo terhubung ke
      sana. Link Restart di diagram (versi sebelumnya) dihapus supaya kontrolnya hanya di satu
      tempat
- [x] **Allow without asking**: modul baru `services::windows_service_control`. Satu UAC
      menambahkan ACE `(A;;RPWPLCLO;;;<SID akun ini>)` (start, stop, query status, interrogate,
      tidak ada hak mengubah service) ke security descriptor service itu saja, lewat
      `sc.exe sdset` (exit code native dicek manual). SID diambil dari token proses
      (`GetTokenInformation` + `ConvertSidToStringSidW`), SDDL dibaca dengan `sc.exe sdshow`
      tanpa admin. Setelah itu `control` memanggil SCM langsung (`StartServiceW`,
      `ControlService`, menunggu status sampai 45 detik) tanpa UAC. Sebelum diizinkan, setiap
      aksi tetap jalan lewat UAC. `can_control` mengecek dengan membuka service dengan hak
      start+stop, jadi status "Allow" selalu sesuai kenyataan. **Put back Odoo's own settings**
      ikut mencabut ACE itu
- Verifikasi: test unit `windows_service_control` (sisip ACE ke DACL sekali saja dan sebelum
  SACL, cabut kembali ke SDDL semula, quoting `sdset`, baca SID akun dan SDDL service
  `EventLog` sungguhan) lolos. **Belum dicoba di app:** klik Allow (UAC), lalu Start/Stop/
  Restart tanpa UAC
- [x] Toggle **Keep Rezure in the tray** dihapus dari "After a restart" (keputusan maintainer:
      Rezure memang dimaksudkan tetap terbuka selama Odoo dipakai). Tinggal dua toggle, dan
      hitungannya jadi "x of 2 on". Setting-nya sendiri tetap ada di halaman Settings
- [x] **Bug ditemukan lewat pemakaian nyata:** setelah Connect, Odoo gagal dengan `role "odoo"
      does not exist`. `odoo.conf` sudah ditulis, tapi role `odoo` tidak ada di PostgreSQL Rezure
      (hanya `postgres` dan `root`), padahal modal menampilkan "On Rezure's PostgreSQL" karena
      `uses_rezure` hanya membaca `odoo.conf`. Langkah terakhir Connect (`ensure` role) rupanya
      gagal tanpa terlihat; log sesi itu sudah tidak ada, jadi penyebab pastinya tidak diketahui.
      Role dibuat manual di mesin maintainer. Perbaikan di app:
      - `OdooInstall.loginMissing`: `odoo.conf` menunjuk ke Rezure tapi role `db_user` tidak ada
        (hanya `true` kalau benar-benar terbukti; PostgreSQL yang mati tidak dianggap hilang)
      - Banner merah di atas modal + badge "Login missing" + tombol **Recreate it**
        (`odoo::repair_login`): nyalakan PostgreSQL kalau perlu, lalu `ensure` role dari
        `db_user`/`db_password` di `odoo.conf` apa adanya. Tidak mengubah file di Program Files,
        tidak butuh UAC. Baris Odoo di halaman Databases juga merah dengan tombol **Fix**
      - **Start**/**Restart** Odoo selalu memastikan role-nya ada (idempotent, kegagalannya hanya
        di-log), jadi kasus ini juga sembuh sendiri lewat tombol yang biasa dipakai
- [x] **Delete database di PostgreSQL Rezure** (permintaan maintainer, setelah database `odoo`
      setengah jadi tidak bisa dihapus dari mana pun). `ServerInfo.drop_supported` sekarang
      `true` juga untuk koneksi PostgreSQL yang `managed` dan tidak read-only, dan
      `database::drop_database` mengizinkan koneksi itu. Pakai dialog konfirmasi ketik CONFIRM
      yang sama dengan MariaDB, dan `DROP DATABASE … WITH (FORCE)` yang sudah ada menutup sesi
      Odoo/PHP yang masih terbuka. Koneksi ke server orang lain dan SQL Server tetap tanpa Delete
- [x] Pesan hapus user yang masih punya database diperjelas: *"odoo" still owns the database
      odoo - delete it on the Databases page first, then the user* (sebelumnya "owns odoo -
      delete it first", ambigu antara user dan database yang namanya sama)
- [x] Logo asli Odoo di header modal, diagram, dan baris halaman Databases. Dibaca saat runtime
      dari instalasi Odoo itu sendiri (`server\odoo\addons\web\static\img\odoo-icon-192x192.png`,
      dicek PNG dan ≤ 256 KB) jadi `data:` URL, pola yang sama dengan ikon klien SQL dari `.exe`.
      Tidak ada file logo Odoo yang ikut di repo Rezure. Tanpa file itu, ikon generik tetap dipakai
- [x] Modal dilebarkan (`max-w-4xl`) dan kolomnya diseimbangkan: kartu Master Password
      (`OdooMasterPasswordCard.vue`) ada di kolom kanan sebelum connect, dan pindah ke bawah
      tabel `odoo.conf` di kolom kiri setelah connect, saat kolom kiri tinggal tabelnya saja
- [x] **Catatan di `odoo.conf`** (permintaan maintainer: user bingung membedakan master password
      dengan `db_password` saat membuka file-nya). Connect menulis blok komentar `;` berbahasa
      Indonesia tepat di bawah `[options]` (`odoo::notes`/`set_notes`): arti `db_host`/`db_port`,
      `db_user`/`db_password`, `pg_path`, dan `admin_passwd` (teks biasa, hash `$pbkdf2`, atau
      tidak ada → `admin`), URL database manager sesuai `http_port`, nama service untuk
      restart, dan letak `odoo.conf.rezure-backup`. Ditandai penanda awal/akhir supaya Connect
      ulang mengganti, bukan menumpuk. Hanya ASCII, karena Odoo membaca file ini dengan code
      page ANSI Windows. Hilang kalau Odoo sendiri menulis ulang file-nya (ganti master
      password lewat database manager)
- [x] **Modal tidak lagi tertutup kalau area di luarnya diklik** (permintaan maintainer: klik
      nyasar membuang isian). `@click.self` dihapus dari semua scrim modal. Jalan keluarnya
      sekarang tombol **X** di pojok kanan atas, Cancel/Close, atau Escape. Komponen baru
      `common/ModalCloseButton.vue` (absolute, panel butuh `relative` dan header `pr-8`), dipakai
      di 20 modal. Odoo, Install a version, Browse stickers, dan New connection sudah punya X
      sendiri. X ikut nonaktif selama tombol Close-nya juga menolak (misalnya saat import atau
      create berjalan). Dropdown menu tetap tertutup kalau diklik di luar
- [x] Error baru yang umum (bukan khusus Odoo): `ExternalAppNotFound`,
      `ExternalAppSetupCancelled`, `ExternalAppSetupFailed`
- [x] `docs/v1/databases.md` (Users, Connect an app, Odoo) dan `docs/v1/database-profiles.md`
      (Start when Rezure opens) diperbarui

### Verifikasi

- `cargo fmt --check`, `cargo clippy --all-targets` bersih, seluruh 653 test Rust lolos. Test baru:
  `postgres_roles` (nama yang ditolak, quoting password, escaping tiap format snippet),
  `start_on_launch` (hanya yang ditandai, terpasang, tampil, dan berhenti yang di-start),
  `odoo` (versi dari nama folder, baca/tulis `[options]` tanpa mengubah baris lain),
  `elevation::run_logged` (bentuk script), dan round-trip `settings`
- Test sungguhan ke PostgreSQL Rezure yang sedang jalan (`postgres_roles::round_trip…`,
  `--ignored`): buat user + database miliknya, terdaftar, hapus ditolak sambil menyebut
  database-nya, `ensure` mengubah `CREATEDB`, lalu bersih lagi. Lolos
- Deteksi Odoo di mesin maintainer (`odoo::print_detected`, `--ignored`): Odoo 20.0, service
  `odoo-server-20.0` jalan, `PostgreSQL_For_Odoo` berhenti tapi masih Automatic, versi bawaan
  16.14, `odoo.conf` masih menunjuk ke PostgreSQL bawaan. Benar semua
- `npm run type-check`, `npm run lint`, `prettier` bersih
- **Belum dicoba di app sungguhan:** tombol **Connect to Rezure** (UAC, tulis `odoo.conf`,
  restart service), **Put back Odoo's own settings**, modal Users, dan toggle Start when Rezure
  opens setelah Rezure dibuka ulang. Connect sengaja tidak dijalankan dari sini karena mengubah
  instalasi Odoo milik maintainer

### Fase D — Pindah data (belum dikerjakan)

Dump database dari PostgreSQL bawaan Odoo lalu restore ke Rezure. Hanya untuk yang sudah punya
data; ditunda.

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
