# Rezure (Desktop App) — v3.3.0: Dev Servers

Rilis ini membuat project yang butuh proses sendiri — dev server Vite/Astro/Next/Nuxt, app Node
seperti 9router, backend Node lain — bisa dibuka lewat domain `.test`-nya seperti project PHP.
Nginx meneruskan domain itu ke port proses tersebut (serve profile **`proxy`**), dan Rezure bisa
menyalakan serta mematikan prosesnya sendiri dari halaman Projects.

Dibangun di atas registry framework v3.2.0: field `serve` di definisi framework sudah ada dan
sengaja dibiarkan berisi satu profile (`php`) sampai rilis ini. v3.2.0 dirilis lebih dulu.

Penomoran fase di dokumen ini (Fase 1–4) lokal untuk rilis v3.3.0.

---

## Latar belakang: 9router tidak bisa dibuka lewat `9router.test`

**Gejala.** Maintainer menambahkan 9router (app Node yang listen di port-nya sendiri) ke Rezure dan
mengganti framework-nya lewat dialog framework v3.2.0. `9router.test` tetap tidak jalan, framework
apa pun yang dipilih.

**Akar masalah.** Semua vhost memakai satu template, profile `php` (`vhosts::vhost_config`):
file disajikan apa adanya, sisanya ke `index.php` lewat FastCGI. Memilih framework hanya mengubah
**folder** yang disajikan, bukan **cara** menyajikannya. Project Node tidak punya `index.php` dan
tidak bisa dijalankan dari file mentah — browser tidak bisa mengeksekusi `.vue`/`.jsx`/`.ts` atau
`import` dari `node_modules`. Yang dibutuhkan adalah `proxy_pass` ke proses yang sedang jalan.

**Efek samping yang ditemukan sekaligus — kebocoran file.** Project Node disajikan dari root
folder-nya, dan template `php` hanya menolak `/\.ht`. Jadi `http://9router.test/.env` mengunduh
`.env` project itu. Ini juga berlaku untuk project PHP yang docroot-nya root project (CodeIgniter 3,
WordPress, PHP generik) — lihat Keputusan 4.

**Ini memang belum didukung, bukan bug di setup maintainer.** v3.2.0 memutuskan profile hanya
`php` dan mencatat `proxy` di Backlog. `proxy` juga bertentangan dengan keputusan v3.5 ("Node
tidak dijalankan Rezure sebagai service, tidak ada proses yang di-proxy nginx"), jadi rilis ini
mengubah keputusan itu secara eksplisit — lihat Keputusan 1.

---

## Desain

### Siapa butuh apa

| Jenis project | Butuh proses? | Profile |
|---|---|---|
| PHP (Laravel, CI, WP, …) | tidak | `php` (tidak berubah) |
| Laravel + Vite | tidak — browser mengambil aset langsung dari Vite lewat `public/hot` | `php` (tidak berubah) |
| SPA Vite/CRA (Vue, React, Svelte, Angular) — mode dev | ya, dev server | `proxy` |
| Astro, Next, Nuxt, SvelteKit — mode dev atau SSR | ya | `proxy` |
| App/backend Node (9router, Express, Nest) | ya, selalu | `proxy` |
| Hasil build statis (`dist/`, `build/`, `out/`) | tidak | `php` lewat Custom folder sekarang; `static`/`spa` di Backlog |

### Dua lapis: proxy dulu, proses kemudian

Fitur ini sengaja dipisah jadi dua lapis yang bisa dipakai sendiri-sendiri:

1. **Profile `proxy`** — nginx meneruskan `<domain>.test` ke `127.0.0.1:<port>`. Rezure tidak peduli
   siapa yang menyalakan prosesnya. Ini saja sudah cukup untuk 9router: user start sendiri, domain
   jalan.
2. **Dev server terkelola** — Rezure menyalakan proses itu sebagai `Service`, memaksakan port-nya,
   menunggu sampai siap, menampilkan log-nya, dan mematikannya saat Rezure ditutup.

Lapis 2 bergantung pada lapis 1, tidak sebaliknya. Project bisa memakai proxy tanpa membiarkan
Rezure mengelola prosesnya (mis. dev server dijalankan dari IDE).

### Template `proxy`

```nginx
# Vite listens on `localhost`, which on most Windows machines is `::1` only;
# Next and most Node apps answer on 127.0.0.1. Both, IPv6 as the backup.
upstream rezure_dev_9router_2da1b2c3 {
    server 127.0.0.1:20128;
    server [::1]:20128 backup;
}

server {
    listen 80;
    server_name 9router.test;

    location / {
        proxy_pass http://rezure_dev_9router_2da1b2c3;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;          # HMR Vite/Next lewat websocket
        proxy_set_header Connection $connection_upgrade; # map di nginx.conf utama
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host $host;
        proxy_buffering off;      # SSE/streaming (9router meneruskan respons LLM secara streaming)
        proxy_read_timeout 1h;    # koneksi HMR dan stream yang lama tidak diputus
    }

    error_page 502 504 /rezure-dev-server-down.html;
    location = /rezure-dev-server-down.html {
        root "<runtime>/nginx/pages";
        internal;
    }
}
```

- `map $http_upgrade $connection_upgrade { default upgrade; '' close; }` ditambahkan sekali di blok
  `http` `ensure_main_config`, supaya request biasa tetap `Connection: close` ke upstream.
- Port selalu `u16` dari Rust, tidak pernah string dari data — tidak ada jalan menyisipkan directive
  lewat port. Sama dengan prinsip v3.2.0: definisi hanya **memilih** profile, tidak menulis nginx.
- **Halaman 502 yang ramah.** Dev server mati adalah keadaan normal untuk profile ini (beda dengan
  PHP yang selalu jalan), jadi 502 bawaan nginx menyesatkan. Rezure menulis satu halaman statis:
  "Dev server untuk project ini belum jalan — nyalakan dari Rezure (Projects → Dev servers)".

### Konfigurasi per project: `etc/dev-servers.json`

```json
{
  "projects": {
    "9router-a1b2c3": { "port": 20128, "portMode": "fixed", "script": "start", "autoStart": true },
    "astro-web": { "port": 4400 }
  }
}
```

| Field | Isi |
|---|---|
| `port` | port proxy. Diisi otomatis dari rentang Rezure saat project pertama kali butuh, lalu **disimpan**, supaya `astro-web.test` selalu ke port yang sama |
| `portMode` | opsional, menimpa default framework (lihat bawah) |
| `script` | opsional, nama script di `package.json` yang dijalankan; default dari framework |
| `autoStart` | opsional (default `true`): klik Open menyalakan dev server kalau belum jalan |

**Kenapa file baru, bukan `project-frameworks.json`.** File override hanya ada untuk project yang
framework-nya dikoreksi, dan memilih Auto menghapus entrinya. Port dev server dibutuhkan juga oleh
project yang terdeteksi benar, dan tidak boleh hilang saat user mengganti framework. Alasan memakai
JSON di `etc/` (bukan SQLite) sama dengan v3.2.0: ini deklarasi user, tidak boleh hilang kalau
`rezure.db` di-reset. Unlink ikut menghapus entri project itu, sama seperti override framework.

**Kenapa `script`, bukan perintah bebas.** Rezure hanya menjalankan script yang **sudah ada** di
`package.json` project (`<pm> run <script>`), tidak pernah string perintah yang diketik user. Tidak
ada shell string di config Rezure, dan app yang butuh perintah khusus (`npx 9router`) cukup diberi
script di `package.json`-nya.

### Memaksakan port: `portMode`

Masalah utamanya: kalau port bentrok, Vite/Astro diam-diam pindah ke port berikutnya, dan proxy ke
port lama jadi 502. Jadi Rezure menentukan port dan memaksakannya, dengan cara yang beda per tool.
Pilihannya himpunan tertutup (enum di Rust), sama seperti serve profile — data hanya memilih:

| `portMode` | Yang ditambahkan Rezure | Untuk |
|---|---|---|
| `vite` | `-- --port <p> --strictPort` + env `PORT` | Vue, React, Svelte (Vite) |
| `flag` | `-- --port <p>` + env `PORT` | Next, Nuxt, Astro, Angular (`ng serve`) |
| `env` | env `PORT` saja | CRA, Express/Node yang membaca `process.env.PORT` |
| `fixed` | tidak ada — app memakai port-nya sendiri, user mengisi port itu | 9router, app dengan port di config sendiri |

Untuk `fixed`, kalau port yang diisi tidak kunjung listen tapi proses itu (atau anaknya) listen di
port lain, error-nya menyebut port itu: "Proses listen di :3001, bukan :20128 — pakai 3001?" (dari
`ports::listening_pid` + pohon proses `sysinfo`, keduanya sudah ada).

### Bentuk definisi framework yang berubah

```json
{
  "id": "astro",
  "label": "Astro",
  "category": "JavaScript",
  "priority": 55,
  "detect": { "npm": "astro" },
  "serve": "proxy",
  "dev": { "script": "dev", "port": "flag" }
}
```

| Field | Aturan |
|---|---|
| `serve` | sekarang `php` (default) atau `proxy` |
| `dev` | opsional, hanya berarti untuk `serve: "proxy"`. `script`: nama script default; `port`: salah satu `portMode`. Tanpa `dev` → script `dev`, lalu `start`; port `env` |

Definisi `proxy` dengan `docroot` ditolak (proxy tidak menyajikan folder). Override **Custom
folder** tetap selalu profile `php` — project yang sudah disajikan dari `dist/` lewat Custom folder
tidak berubah.

Default built-in yang diusulkan (diverifikasi satu per satu di Fase 2):

| Framework | `serve` | `dev.script` | `dev.port` |
|---|---|---|---|
| Next.js, Nuxt, Astro | `proxy` | `dev` | `flag` |
| Vue, React, Svelte | `proxy` | `dev` | `vite` |
| Angular | `proxy` | `start` | `flag` |
| Node generik | `proxy` | `start` | `env` |
| Static | `php` (tidak berubah) | — | — |

### Dev server sebagai `Service`

Sesuai prinsip arsitektur 1, dev server adalah implementasi `Service` (`start`/`stop`/`status`/
`restart`), bukan kasus khusus di luar trait:

- **Didaftarkan dinamis** lewat `ServiceManager::sync_dev_servers(projects)`, pola yang sama dengan
  `sync_php_pool`: project ber-profile `proxy` dapat service `dev:<project-id>`; project yang hilang,
  di-unlink, atau ganti ke profile `php` → service-nya dihentikan dan dilepas.
- **Tidak tampil di Dashboard dan tidak ikut Start all / Restart all / Stop all** (category
  `dev-server` difilter dari daftar Services). Dev server makan 200–500 MB RAM masing-masing, jadi
  selalu opt-in per project. **Tetap ikut `stop_all()` saat Rezure ditutup.**
- **Spawn:** `<pm> run <script> [-- args]` di folder project, dengan folder Node yang di-pin
  (`node::terminal_bin_dir`, sama seperti terminal project) di depan `PATH`. Package manager dari
  lockfile: `pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn, `bun.lockb`/`bun.lock` → bun, selain itu npm.
- **Siap = port listen.** `start()` menunggu port menerima koneksi (`wait_until_accepting`), dengan
  timeout yang di-reset setiap ada output (pola `READY_QUIET_TIMEOUT`), karena compile pertama bisa
  lama. Proses keluar sebelum siap → error berisi baris log terakhir.
- **Stop = seluruh pohon proses.** `npm` menjalankan `node` sebagai anak; membunuh `npm` saja
  meninggalkan `node` yang tetap memegang port. Pakai `process::kill_process_tree` yang sudah ada,
  dan pid file (`process::pid_file_path`) supaya sisa proses dari Rezure yang crash dibereskan di
  launch berikutnya.
- **Tidak di-restart otomatis saat crash** (`restarts_on_crash: false`). Dev server yang mati karena
  compile error akan mati lagi; user perlu melihat log-nya, bukan loop restart.
- **Log** lewat `process::LOG_EVENT` yang sudah ada, jadi `ServiceLogPanel.vue` bisa dipakai ulang.

### Vite menolak Host asing

Vite versi baru menolak request dengan Host yang bukan `localhost` ("Blocked request. This host is
not allowed"). Pilihan di Fase 2, diuji dalam urutan ini:

1. Untuk `portMode: vite` dan `flag` pada framework berbasis Vite (Astro, Nuxt, SvelteKit), set env
   `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS=<domain>` saat spawn. Host tetap `$host`, jadi URL absolut
   yang dibuat app tetap `astro-web.test`.
2. Kalau (1) tidak tersedia di versi Vite project, dokumentasikan `server.allowedHosts: ['.test']`
   di `vite.config`.

`proxy_set_header Host localhost` sengaja **tidak** dipakai: Next/Nuxt membentuk URL redirect dan
link absolut dari Host, jadi user akan terlempar ke `localhost:<port>`.

### UI

**Tombol "Dev servers" di header Projects**, di samping Sync hosts file, dengan badge jumlah yang
sedang jalan. Membuka modal yang hanya berisi project ber-profile `proxy`:

```
Dev servers                                                    [Stop all]
──────────────────────────────────────────────────────────────────────────
Project    Domain          Port    Script   Open      Status
9router    9router.test    20128   start    ☑ auto    ● Running   [■] [Log]
astro-web  astro-web.test  4400    dev      ☑ auto    ○ Stopped   [▶]
fmt_app    fmt-app.test    —  node_modules belum ada              [Install]
──────────────────────────────────────────────────────────────────────────
Port otomatis diambil dari 4400–4499. Semua dev server dimatikan saat Rezure ditutup.
```

**State per baris** (dihitung di Rust, UI hanya menampilkan):

| Kondisi | Yang tampil |
|---|---|
| `node_modules` tidak ada | tombol **Install** (`<pm> install`, log di panel yang sama) |
| Node tidak ditemukan / versi yang di-pin tidak terpasang | link ke halaman Switch |
| `package.json` tidak punya script yang dipilih | dropdown script dari `package.json` |
| Lockfile pnpm/yarn/bun tapi tool-nya tidak ada di `PATH` | pesan + saran `corepack enable` |
| Port dipegang proses lain | alur **bebaskan port** yang sudah ada (`ports::holder`/`reclaim`) |
| Siap | ▶ |
| Jalan | ■ + port + Log |

**Validasi port** (di Rust, saat simpan):
- 1024–65535.
- Tidak dipakai project lain di `dev-servers.json`.
- Tidak bentrok dengan port milik Rezure: 80, MariaDB/MySQL, PostgreSQL, blok pool PHP
  (`php_pool`), Mailpit, bridge LocalDB.
- Peringatan (bukan penolakan) kalau di rentang port yang dicadangkan Windows/Hyper-V
  (`netsh int ipv4 show excludedportrange protocol=tcp`).

**Di baris project.** Baris action sudah padat (Open + 6 ikon + hapus), jadi tidak ada ikon play
baru. Untuk project `proxy`:
- Tombol **Open** dapat titik status kecil: abu-abu = mati, hijau = jalan, berdenyut = starting.
- Klik Open → kalau mati dan `autoStart` → baris menampilkan "Starting…" → tunggu port siap →
  **baru** buka browser. Gagal → browser tidak dibuka; toast dengan tombol "Lihat log". Tanpa
  `autoStart` → browser langsung dibuka (user dapat halaman 502 yang ramah).
- Kolom Framework menampilkan port: `Astro · :4400`.

---

## Fase 1 — Profile `proxy` + port manual

**Tujuan:** 9router bisa dibuka lewat `9router.test` selama prosesnya dinyalakan sendiri oleh user.
Belum ada proses yang dikelola Rezure.

### Tasks
- [x] `ServeProfile::Proxy` di `services::frameworks`; validasi menolak `proxy` + `docroot`.
      `Serving` dan `FrameworkSummary` ikut membawa `serve`; Custom folder selalu `php`
- [x] Field `dev` di `FrameworkDef` (`DevDefaults { script, port: PortMode }`), `deny_unknown_fields`,
      `dev` tanpa `serve: "proxy"` ditolak. Nama script lewat `frameworks::validate_script_name`:
      `[A-Za-z0-9:_.-]`, maks. 64, tidak diawali `-`/`.` (titik ditambahkan dari rencana karena
      script seperti `serve.local` ada di dunia nyata)
- [x] `ProjectInfo` mendapat `serve` dan `dev_port`; `src/types/project.ts` ikut. `LinkPreview`
      juga mendapat `serve`, supaya dialog Add folder bilang "lewat dev server" alih-alih nama folder
- [x] `config::dev_servers` (baru): `etc/dev-servers.json`, pola `config::project_frameworks`
- [x] Alokasi port: `dev_servers::assign_ports`, dipanggil dari `vhosts::sync_vhosts`; scan hanya
      membaca. `list_projects` sekarang sync dulu baru scan, supaya port yang baru dipilih langsung
      tampil
- [x] Validasi port di `services::dev_servers` (bukan modul `dev_ports` terpisah — terlalu kecil):
      ≥ 1024, tidak dipakai project lain, bukan port Rezure (80/443, 3306 + port profil aktif, 5432,
      1025/8025, 14330, 9100–9399 untuk pool PHP)
- [x] `vhosts`: template `proxy` (upstream per project `127.0.0.1` + `[::1]` backup), `map
      $connection_upgrade` di `ensure_main_config`, halaman `rezure-dev-server-down.html` di
      `<runtime>/nginx/pages/` yang me-refresh diri tiap 4 detik
- [x] Command `set_dev_server_config(id, change)` dan `list_dev_servers`
- [x] `unlink` ikut menghapus entri `dev-servers.json`
- [x] Error baru di `AppError` dengan `code()`: `InvalidDevPort`, `DevPortTaken`, `InvalidDevScript`,
      `DevServerNotReady`, `DevServerFailed`, `DevServerRunning`, `NotADevServerProject`
- [x] `frameworks.json` built-in sesuai tabel default
- [x] Tombol **Dev servers** + `DevServersModal.vue`, store `stores/devServers.ts`, tipe
      `src/types/devServer.ts`
- [x] **Keputusan 4:** vhost `php` menjawab 404 untuk semua dotfile kecuali `.well-known`
      (`location ~ /\.(?!well-known(?:/|$))`), menggantikan aturan lama `/\.ht`. Vhost `proxy`
      sengaja tidak: Vite menyajikan modulnya dari `/node_modules/.vite/`

### Test
- [x] `proxy_vhosts_pass_everything_to_the_dev_server`, `php_vhosts_hide_dotfiles_but_not_well_known`,
      `dev_upstream_names_are_distinct_and_plain`
- [x] `the_real_nginx_accepts_both_profiles` (`#[ignore]`): config dengan `map`, upstream PHP, vhost
      `php` dan `proxy` lewat `nginx -t` asli — **lulus** ("syntax is ok", "test is successful")
- [x] `proxy_definitions_are_checked_for_what_cannot_apply_to_them`,
      `script_names_that_mean_something_to_cmd_are_refused`,
      `javascript_frameworks_are_proxied_and_php_ones_are_not`, `a_custom_docroot_is_never_proxied`
- [x] `ports_rezure_or_another_project_uses_are_refused`, `ports_are_picked_from_the_range_and_kept`
- [x] `config::dev_servers`: round trip, entri minimal, file rusak, entri rusak, `owner_of`
- [x] Fixture yang ada tetap terdeteksi sama (`every_fixture_is_detected_as_its_folder_name`)

### Verifikasi
- [ ] 9router: set port 20128 + "The app's own port", jalankan → `9router.test` terbuka, respons
      streaming tidak tertahan
- [ ] Dev server mati → halaman "dev server isn't running", bukan 502 bawaan nginx
- [ ] Upgrade dari v3.2.0: semua vhost `php` berubah sekali (aturan dotfile), nginx reload sekali,
      situs PHP tetap jalan; `/.env` di project CI3/WordPress → 404
- [x] `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib` (616 lulus),
      `npm run lint`, `npm run type-check`, `npm run build-only`

---

## Fase 2 — Dev server dikelola Rezure

**Tujuan:** Rezure bisa menyalakan dan mematikan dev server per project, dengan port yang
dipaksakan, log yang terlihat, dan tidak ada proses yatim.

### Tasks
- [x] `Launch::DevServer(Box<DevSpec>)` di `services::process` dan `ProcessService::dev_server`,
      memakai mesin yang sama (log, pid file, CPU). `DevSpec` membawa semua yang sudah di-resolve:
      port, port mode, script, package manager, pin Node, auto start
- [x] Perintah: `cmd.exe /d /s /c "<pm> run <script> [--] <args>"` lewat `raw_arg`, dirakit hanya dari
      package manager (enum), nama script tervalidasi, dan angka port. npm saja yang pakai `--`
      (pnpm meneruskan `--` mentah ke Vite). Env: `PORT`, `BROWSER=none`,
      `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS=<domain>`, `NO_COLOR`/`FORCE_COLOR=0` untuk log polos
- [x] `PATH` dengan folder Node yang di-pin di depan (`launcher::path_with_first` dibuka ke crate);
      Node/package manager yang tidak ada → `DevServerIssue` yang jelas
- [x] Readiness: loop khusus `wait_for_dev_server` (bukan `wait_until_accepting`): cek `127.0.0.1`
      **dan** `[::1]`, quiet timeout 120 dtk (build pertama `ng serve` bisa lama tanpa output), maks. 300 dtk. Proses keluar → error berisi 3 baris
      output terakhir (buffer 12 baris di `ProcessService::tail`)
- [x] Port salah terdeteksi dari pohon proses (`ports::ports_held_by_tree`) untuk **semua** mode —
      setelah 4 dtk untuk `fixed`, 20 dtk untuk yang diberi port
- [x] Stop lewat `kill_process_tree`. Orphan: pid file + cocokkan `cmd.exe` **dan** cwd = folder
      project (cwd ditambahkan karena `cmd.exe` dipakai semua terminal di mesin)
- [x] Cek port sebelum start juga lewat `ports::holder` — bind `127.0.0.1` saja tidak melihat
      listener `::1`
- [x] `ServiceManager::sync_dev_servers` + `DevServerFactory`, dipanggil dari
      `sync_vhosts_and_reload` (yang sekarang mengembalikan spec). Id `dev-<project>` (bukan
      `dev:`: id juga jadi nama folder di `data\`). Spec berubah → service dibangun ulang hanya saat
      berhenti
- [x] Disaring dari Services/Manage services (`service_visibility`), jadi tidak ikut Start/Restart/
      Stop all; tetap kena `stop_all()` saat exit. `restarts_on_crash` false
- [x] Masalah per baris (`DevServerIssue`: `NoPackageJson`, `NodeMissing`, `PackageManagerMissing`,
      `ScriptMissing`, `NeedsInstall`) dihitung di Rust. *Beda dari rencana:* `PortHeld` tidak
      dihitung saat list (satu `netstat` per baris tiap modal dibuka); port bentrok muncul saat Start
      sebagai `PortInUseBy`, lalu modal menawarkan "Free port N and start" lewat
      `port_holder`/`free_port` yang sudah ada
- [x] Command `start_dev_server`, `stop_dev_server`, `stop_all_dev_servers`,
      `install_dev_dependencies`, `dev_server_statuses` — semuanya off async runtime
- [x] Modal: port, cara memberi port, script (dropdown dari `package.json`), "Open starts it",
      perintah persis, status, Start/Stop/Save/Revert, Log (`ServiceLogPanel.vue`), Install,
      Stop all, bebaskan port. Port/script terkunci selama jalan (Rust juga menolak:
      `DevServerRunning`)
- [x] Badge jumlah yang jalan di tombol Dev servers. *Beda dari rencana:* lewat polling
      `dev_server_statuses` tiap 4 dtk selama halaman Projects terbuka, bukan `CHANGED_EVENT` —
      supervisor tidak mengawasi service yang tidak restart otomatis, jadi event itu tidak pernah
      datang untuk dev server
- [x] Tanpa telemetry: crash sink dev server hanya notifikasi; `start/stop/restart_service` melewati
      telemetry untuk id `dev-`; `docs/telemetry-contract.md` ikut diperbarui

### Test
- [x] `each_port_mode_tells_the_dev_server_its_port_its_own_way` (empat mode × empat PM)
- [x] `the_lockfile_decides_the_package_manager` (termasuk beberapa lockfile sekaligus)
- [x] `the_script_is_the_choice_then_the_framework_then_dev_then_start`,
      `package_scripts_skips_names_cmd_would_misread`
- [x] `dev_servers_come_and_go_with_their_projects`,
      `a_changed_spec_waits_for_a_running_dev_server_to_stop`
- [x] `dev_servers_are_never_on_the_services_page`
- [x] `#[ignore]` end-to-end dengan Node sungguhan, **semua lulus**:
      `a_dev_server_starts_answers_and_stops_cleanly` (port bebas lagi, tak ada `node` sisa),
      `a_dev_server_on_the_wrong_port_says_which_one` (~8 dtk),
      `a_dev_server_that_dies_reports_its_last_words`
- [x] `netstat_rows_are_parsed_and_everything_else_skipped`,
      `a_tree_reports_the_ports_its_processes_listen_on`

### Verifikasi
- [ ] Per framework built-in, project nyata dari template resminya (Vite+Vue, Vite+React, Svelte,
      Astro, Next, Nuxt, Angular): start dari modal, domain terbuka, **HMR jalan lewat domain**, port
      tidak bergeser saat port default-nya dipakai proses lain. Hasilnya menentukan `dev.port` final
- [ ] Vite Host check: `astro-web.test` tidak kena "Blocked request" (env
      `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` belum diuji ke Vite sungguhan)
- [ ] Rezure ditutup normal → tidak ada `node.exe` sisa. Rezure di-kill dari Task Manager → Start
      berikutnya membereskannya
- [ ] Project dengan compile error → log menampilkan error-nya, tidak ada loop restart

---

## Fase 3 — Open menyalakan dev server

**Tujuan:** Untuk project `proxy`, klik Open cukup — tidak perlu membuka modal dulu.

### Tasks
- [x] `open_project_site` jadi async: project `proxy` dengan auto start → `start_and_wait` (juga
      menunggu start yang sedang berjalan dari modal), baru buka browser. Gagal → browser tidak
      dibuka, error dikembalikan
- [x] Tombol Open: titik status (abu-abu/hijau), spinner menggantikan ikon globe saat starting,
      lebar tetap supaya kolom Actions tidak bergeser. Gagal → pesan di bawah header dengan
      **View log** yang membuka modal pada log project itu (pesan, bukan toast — mengikuti pola
      `openError` yang sudah ada)
- [x] Badge Framework menampilkan port (`Astro · :4400`); kolom Framework dilebarkan `w-28` → `w-32`.
      Dialog framework dan picker menyebut "dev server" alih-alih folder
- [ ] Filter "Running" — tidak dikerjakan (opsional); badge di tombol Dev servers sudah menjawabnya

### Test
- [x] `open_starts_only_a_proxy_project_that_asks_for_it` (`dev_servers::starts_on_open`)

### Verifikasi
- [ ] Open project Astro yang mati → spinner → browser terbuka di halaman yang sudah jadi
- [ ] Open saat `node_modules` belum ada → tidak start, pesan menyarankan Install
- [ ] Share pada project `proxy` (tunnel → nginx → dev server)

---

## Fase 4 — Dokumentasi

- [x] `docs/v1/frameworks.md`: tabel field (`serve`, `dev`), bagian "Serve profiles" dan "Dev server
      defaults", penutup "Serving a project some other way" ditulis ulang
- [x] `docs/v1/projects.md`: bagian "Dev servers" (Open, dialog, cara memberi port, batasan), bagian
      internal "Dev servers", troubleshooting (halaman not running, Vite "Blocked request", HMR,
      port salah, Install)
- [x] `docs/v3/rezure-app-v3.5-phases-tasks.md`: catatan keputusan diubah di v3.3.0
- [x] `services::agent_docs`: kolom framework menulis "Astro (dev server on :4400)" + aturan baru;
      `agent-docs/how-rezure-works.md`: bagian "Dev servers", field `serve`/`dev`, Node pin, layout
- [x] Doc comment `utils/paths.rs`, `services::node`, `ProjectInfo::node_version`
- [x] `docs/telemetry-contract.md`: dev server tidak pernah dikirim
- [ ] Changelog v3.3.0 (diambil dari API, ditulis bersama rilisnya)

---

## Keputusan

Diambil maintainer sesuai rekomendasi ("gas kerjakan fase 1–4"):

1. **Keputusan v3.5 diubah.** Dev server jadi service yang opt-in per project — tidak di Dashboard,
   tidak ikut Start all, tidak restart otomatis.
2. **Framework JavaScript dan Node generik pindah ke `proxy`.** Custom folder tetap `php`.
3. **Rentang port 4400–4499, konstanta.**
4. **Dotfile ditolak di profile `php`**, dikerjakan di Fase 1.
5. **Tidak ada event telemetry baru**, dan dev server sengaja tidak pernah tercatat.

---

## Setelah uji coba maintainer (26 Okt 2026)

Dua temuan dari pemakaian nyata, keduanya sudah diperbaiki:

1. **whitescale (Astro) gagal start karena `npm run dev` sudah jalan di terminal.** Astro punya
   lockfile sendiri dan langsung exit 1 ("Another astro dev server is already running"). Pesan itu
   tertutup karena error hanya mengambil 3 baris terakhir, yang ternyata stack trace.
   - [x] `dev_servers::outside_servers`: proses node/bun/deno yang working folder-nya folder project,
         bukan turunan Rezure ini, dan memegang port. Dicek sebelum spawn
         (`DevServerNotReady` dengan pid dan port-nya) dan jadi issue `AlreadyRunning` di modal
   - [x] Tombol **Stop it and start here** (`stop_outside_dev_server` → start): yang dimatikan
         runtime teratas di folder itu, bukan terminal tempat perintahnya diketik; port ditunggu
         sampai lepas
   - [x] `dev_servers::summarize_failure`: buffer output 12 → 40 baris, stack frame dan banner npm
         dibuang, baris error pertama yang ditampilkan, plus langkah perbaikan untuk kegagalan yang
         dikenal (sudah jalan / lock / EADDRINUSE → stop salinan lain; modul tidak ada → Install)
   - [x] Banner gagal Open: "View log" → **Fix in Dev servers** (modal terbuka di baris itu)
2. **9router (Next 16) berhenti di "Loading…".** Next 16 menolak (`403 Unauthorized`) request ke
   `/_next/*` dan `/__nextjs*` yang `Origin`-nya bukan `localhost`/`allowedDevOrigins` — termasuk
   websocket HMR. Dikonfirmasi dari log Next sendiri ("Blocked cross-origin request to Next.js dev
   resource /_next/webpack-hmr from "9router.test"").
   - [x] Vhost `proxy` mendapat `location ~ ^/(_next/|__nextjs)` yang membuang `Origin` hanya bila
         sama dengan domain project; Origin situs lain tetap diteruskan dan tetap ditolak Next
   - [x] Diverifikasi dengan nginx dan Next 16 sungguhan (port uji, bukan 80): langsung ke Next
         dengan Origin `9router.test` → `Unauthorized`; lewat vhost baru → diperlakukan sama seperti
         `localhost`; Origin `evil.test` lewat vhost → tetap `Unauthorized`; `/login` → 200

Test baru: `a_failure_is_explained_by_its_message_not_its_stack_trace` (output Astro asli),
`a_missing_dependency_points_at_install`, `an_unfamiliar_failure_still_shows_its_last_words…`,
`a_server_started_outside_rezure_is_found_and_stopped` (`#[ignore]`, Node asli — lulus), dan
asersi `Origin` di `proxy_vhosts_pass_everything_to_the_dev_server`.

---

## Perbaikan lain di rilis ini (29 Okt 2026)

Di luar dev server, dari pemakaian nyata menjelang rilis.

1. **Modal Dev servers: card terpotong saat log dibuka.** Tiap `<li>` adalah item flex dengan
   `overflow-hidden`, jadi tinggi minimumnya 0 dan card ikut menyusut alih-alih list-nya yang
   scroll.
   - [x] `shrink-0` pada card (`DevServersModal.vue`)
2. **"Open with" ke koneksi remote (SSH) gagal di setiap client: "Access denied … (using password:
   NO)".** Tunnel-nya sudah jalan; yang hilang password MySQL. Handoff sengaja tidak pernah
   mengirim password dengan asumsi client akan menanyakannya, tapi TablePlus, DBeaver, HeidiSQL,
   dan konsol MariaDB langsung connect tanpa bertanya. Port tunnel juga berubah tiap sesi, jadi
   password yang disimpan di client tidak membantu. Ini berlaku sama untuk koneksi SSH dengan
   `.pem`, karena `.pem` hanya untuk lapis SSH, bukan lapis MySQL.
   - [x] Password tersimpan ikut dikirim, sebagai pengecualian yang dijelaskan di doc comment
         `db_clients::tableplus_url`: URL TablePlus (`mysql://`/`postgresql://`, di-percent-encode
         lewat `url`), `|password=` di spec `-con` DBeaver (dilewati bila memuat `|`/`=`), dan
         `--password=` HeidiSQL (dilewati bila diawali `-` atau memuat spasi/`"`)
   - [x] Konsol MariaDB: `MYSQL_PWD` hanya di environment proses konsol itu (cara yang sama
         dengan password SSH untuk `ssh.exe`), menggantikan prompt `-p`.
         `database::endpoint_prompts_for_password` → `endpoint_password`
   - [x] MySQL Workbench tetap menanyakan password: tidak punya opsi command line untuk itu
         (daftar opsi dibaca dari `wbprivate.be.dll` 8.0.47). `--run-python` +
         `mforms.Utilities.store_password` dipertimbangkan dan ditolak: urutannya tidak pasti,
         meninggalkan entri vault per port tunnel, dan memakai API internal
3. **HeidiSQL membuka session manager pada session bernama `b=tb_data`.** Parsernya mencari tiap
   switch dengan regex `\s(-d|--description)\s*=?\s*…`, dengan `=` opsional dan `-d` (nama session)
   dibaca sebelum `-db`. Jadi `-db=tb_data` terbaca `-d` + `b=tb_data`.
   - [x] Semua switch HeidiSQL memakai bentuk panjang (`--host=`, `--port=`, `--user=`,
         `--databases=`), yang tidak bisa tertangkap pola pendek (`db_clients::heidisql_args`)
4. **MySQL Workbench hanya membuka Home.** `wb.log`: `open type:` kosong, lalu `Unknown file type
   rezure@127.0.0.1:…`. 8.0.47 tidak mengenal `-query` (satu strip), jadi connection string dianggap
   file.
   - [x] `--query <user>@<host>:<port>`
5. **Logo di menu Open with.** Diambil dari ikon `.exe` client yang terpasang
   (`utils::exe_icon`: `SHDefExtractIconW` 48px → `.ico` 32-bit, dengan mask diubah jadi alpha untuk
   ikon lama), dikirim sebagai `DbClientInfo.icon` (`data:` URL, di-cache per path). Tidak ada
   artwork produk lain di repo, dan client baru otomatis dapat logonya. Konsol memakai mark
   `mariadb`/`postgres` dari `TechIcon`. `windows-sys` mendapat fitur `Win32_Graphics_Gdi`,
   `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`, tanpa dependency baru.
   - [x] Dicek visual: ikon TablePlus, DBeaver, HeidiSQL, dan Workbench terekstrak dengan alpha utuh
6. **Klik Open terasa tidak terjadi apa-apa.** DBeaver dan Workbench butuh beberapa detik sebelum
   jendelanya muncul.
   - [x] `BusyOverlay` "Opening <client>…" (`databases` store: `opening`). Tampil minimal 3 detik,
         karena splash screen DBeaver/Workbench sudah mengambil fokus dalam satu detik, lalu
         tertutup saat jendela Rezure kehilangan fokus, maksimal 20 detik. Tombol Open nonaktif
         selama itu

Test baru: `tableplus_gets_the_saved_password_encoded_into_its_url`,
`dbeaver_gets_the_saved_password_when_its_spec_can_carry_it`,
`heidisql_gets_the_saved_password_only_when_it_can_read_it_back`,
`workbench_is_told_to_query_with_two_dashes`, serta `exe_icon`: `notepad_has_an_icon` dan dua test
format `.ico`.

Diverifikasi maintainer di app sungguhan, lewat koneksi SSH remote: TablePlus, DBeaver, HeidiSQL,
dan konsol MariaDB langsung connect. Workbench membuka dialog passwordnya sendiri.

---

## Status

Fase 1–4 sudah diimplementasikan. Semua pengecekan otomatis lulus, termasuk test `#[ignore]` dengan
Node dan `nginx -t` sungguhan (lihat Test di Fase 1 dan 2). Yang tersisa: verifikasi manual di app
sungguhan (daftar "Verifikasi" Fase 1–3, terutama HMR dan Vite host check per framework) dan
changelog. v3.2.0 tidak dirilis terpisah — isinya ikut rilis ini, dan versi sudah dinaikkan langsung
dari 3.1.1 ke 3.3.0 (`package.json`, `package-lock.json`, `Cargo.toml`, `Cargo.lock`,
`tauri.conf.json`).

---

## Backlog (bukan v3.3.0)

- **Profile `static` dan `spa`** untuk hasil build (`dist/`, `build/`, `out/`, `.output/public`,
  `dist/<nama>/browser`) — tanpa proses sama sekali, `spa` dengan `try_files … /index.html`. Butuh
  pilihan "Dev / Build" per project karena folder build baru ada setelah `npm run build`
- Peringatan port di rentang yang dicadangkan Windows/Hyper-V
  (`netsh int ipv4 show excludedportrange protocol=tcp`) — dari rencana Fase 1, belum dikerjakan;
  start tetap gagal dengan pesan yang jelas kalau kena
- Deteksi otomatis port untuk `fixed` (tanpa user mengisi) — mesinnya sudah ada
  (`ports::ports_held_by_tree`), yang kurang tinggal menyimpannya
- Filter "Running" di halaman Projects
- Dev server untuk runtime lain (Python `uvicorn`/`manage.py runserver`, Go) — `portMode` dan
  package manager-nya beda, tapi profile `proxy` dan `Service`-nya sama
- Monorepo (Turborepo/Nx) — beberapa app dalam satu folder, jadi beberapa domain/port per project
- Matikan dev server otomatis setelah lama tidak dipakai, untuk menghemat RAM
