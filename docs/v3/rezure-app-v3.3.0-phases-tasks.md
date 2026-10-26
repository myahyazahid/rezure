# Rezure (Desktop App) — v3.3.0: Dev Servers

Rilis ini membuat project yang butuh proses sendiri — dev server Vite/Astro/Next/Nuxt, app Node
seperti 9router, backend Node lain — bisa dibuka lewat domain `.test`-nya seperti project PHP.
Nginx meneruskan domain itu ke port proses tersebut (serve profile **`proxy`**), dan Rezure bisa
menyalakan serta mematikan prosesnya sendiri dari halaman Projects.

Dibangun di atas registry framework v3.2.0: field `serve` di definisi framework sudah ada dan
sengaja dibiarkan berisi satu profile (`php`) sampai rilis ini. **v3.2.0 harus rilis dulu.**

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
server {
    listen 80;
    server_name 9router.test;

    location / {
        proxy_pass http://127.0.0.1:20128;
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

    error_page 502 504 /__rezure_dev_server_down.html;
    location = /__rezure_dev_server_down.html {
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
- [ ] `ServeProfile::Proxy` di `services::frameworks`; validasi menolak `proxy` + `docroot`
- [ ] Field `dev` di `FrameworkDef` (`script`, `port: PortMode`), divalidasi seperti field lain
      (`deny_unknown_fields`, nama script `[A-Za-z0-9:_-]`, maks. 64)
- [ ] `ProjectInfo` mendapat `serve: ServeProfile` dan `dev_port: Option<u16>`; `src/types/project.ts`
      ikut
- [ ] `config::dev_servers` (baru): load/save `etc/dev-servers.json`, pola `config::project_frameworks`
      (file rusak → kosong + warn, entri rusak dilewati satu per satu, BOM diterima)
- [ ] Alokasi port: project `proxy` tanpa entri dapat port bebas pertama dari `DEV_PORT_RANGE`
      (4400–4499), langsung disimpan. Dipanggil dari jalur sync vhost, bukan dari scan (scan tetap
      hanya membaca)
- [ ] `services::dev_ports` (baru): validasi port (rentang, unik antar project, daftar port Rezure,
      peringatan excluded range Windows) — fungsi murni + satu fungsi yang membaca `netsh`
- [ ] `vhosts`: template `proxy`, `map $connection_upgrade` di `ensure_main_config`, halaman
      `__rezure_dev_server_down.html` ditulis ke `<runtime>/nginx/pages/`
- [ ] Command `set_dev_server_config(id, port?, port_mode?, script?, auto_start?)` → validasi, simpan,
      `sync_vhosts_and_reload`; `list_dev_servers` → baris modal (tanpa status proses dulu)
- [ ] `unlink_project` ikut menghapus entri `dev-servers.json`
- [ ] Error baru di `AppError` dengan `code()`: `InvalidDevPort { port, reason }`,
      `DevPortTaken { port, by }`
- [ ] Ubah `frameworks.json` built-in sesuai tabel default di atas
- [ ] UI minimal: tombol **Dev servers** + `DevServersModal.vue` berisi kolom Project/Domain/Port
      (bisa diedit) — store baru `stores/devServers.ts`, tipe `src/types/devServer.ts`

### Test
- [ ] `vhost_config` untuk `proxy`: berisi `proxy_pass http://127.0.0.1:<p>`, header Upgrade, tidak
      ada `fastcgi_pass` dan `root`
- [ ] Main config berisi `map $http_upgrade $connection_upgrade`, kurung seimbang
- [ ] Validasi definisi: `proxy` + `docroot` ditolak, `portMode` tak dikenal ditolak, script dengan
      spasi/`&`/`"` ditolak
- [ ] Validasi port: <1024, port Rezure, duplikat antar project ditolak
- [ ] Alokasi port deterministik dan melewati port yang sudah dipakai project lain
- [ ] `dev-servers.json` rusak → kosong, tidak panic
- [ ] Fixture `nextjs`, `astro`, `vue`, dst. masih terdeteksi sama (hanya `serve` yang berubah)

### Verifikasi
- [ ] 9router: set port 20128, jalankan manual → `9router.test` terbuka, respons streaming tidak
      tertahan (bukan muncul sekaligus di akhir)
- [ ] 9router dimatikan → halaman 502 Rezure, bukan 502 bawaan nginx
- [ ] Project PHP yang sudah ada: vhost-nya **tidak berubah** (`VhostSync::changed` false untuk
      file mereka). Hanya `nginx.conf` utama yang berubah karena `map`
- [ ] `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run lint`,
      `npm run type-check`

---

## Fase 2 — Dev server dikelola Rezure

**Tujuan:** Rezure bisa menyalakan dan mematikan dev server per project, dengan port yang
dipaksakan, log yang terlihat, dan tidak ada proses yatim.

### Tasks
- [ ] `Launch::DevServer { project_id, dir, script, port, port_mode, node_pin }` di
      `services::process`, memakai `ProcessService` yang sama (log, pid file, CPU sampling)
- [ ] Pembentukan perintah sebagai fungsi murni: package manager dari lockfile, argumen per
      `portMode`, env `PORT` dan `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS`. Argumen ke `.cmd`
      (npm/pnpm/yarn di Windows dijalankan lewat `cmd.exe`) hanya dari himpunan karakter aman —
      nama script dan port sudah divalidasi di Fase 1, jadi tidak ada string user yang mentah
- [ ] `PATH` dengan folder Node yang di-pin di depan (`node::terminal_bin_dir`); tidak ada Node sama
      sekali → error yang jelas, bukan "program not found"
- [ ] Readiness: `wait_until_accepting` dengan timeout yang di-reset oleh output. Proses keluar
      sebelum siap → `DevServerExited { tail }` berisi baris log terakhir
- [ ] `portMode: fixed`: kalau port tidak listen dalam batas waktu tapi pohon prosesnya listen di
      port lain → error menyebut port itu
- [ ] Stop lewat `kill_process_tree`; pid file supaya sisa dari Rezure yang crash dibereskan saat
      launch berikutnya
- [ ] `ServiceManager::sync_dev_servers`, dipanggil di jalur yang sama dengan `sync_php_pool`;
      category `dev-server` difilter dari `list` Dashboard dan dari Start/Restart/Stop all;
      `stop_all()` saat exit tetap menghentikannya
- [ ] State baris (`DevServerState`: `NeedsInstall`, `NodeMissing`, `ScriptMissing`,
      `PackageManagerMissing`, `PortHeld { holder }`, `Ready`, `Starting`, `Running`) dihitung di
      Rust dan dikirim lewat `list_dev_servers`
- [ ] Command `start_dev_server`, `stop_dev_server`, `install_dev_dependencies` (`<pm> install` sebagai
      proses sekali jalan, log ke panel yang sama). Semuanya `async` + `spawn_blocking`, tidak
      menahan UI
- [ ] Modal: kolom Script (dropdown dari `package.json`), Open auto, Status, ▶/■, Log
      (`ServiceLogPanel.vue` dipakai ulang), Install, Stop all, dan alur bebaskan port
- [ ] Badge jumlah yang jalan di tombol Dev servers, diperbarui lewat `supervisor::CHANGED_EVENT`

### Test
- [ ] Pembentukan perintah untuk keempat `portMode` × empat package manager
- [ ] Deteksi package manager dari lockfile, termasuk dua lockfile sekaligus (urutan tetap)
- [ ] `sync_dev_servers` mendaftarkan yang baru, melepas (dan menghentikan) yang hilang, tidak
      menyentuh service lain
- [ ] Service `dev-server` tidak ada di daftar Dashboard dan dilewati Start all, tapi dihentikan
      `stop_all()`
- [ ] `#[ignore]` end-to-end: fixture `server.js` kecil (`http.createServer` di `process.env.PORT`)
      → start, port siap, stop, port bebas lagi dan tidak ada `node.exe` sisa dari pohon itu

### Verifikasi
- [ ] Per framework built-in, project nyata dibuat dari template resminya: Vite+Vue, Vite+React,
      Svelte, Astro, Next, Nuxt, Angular — start dari modal, domain terbuka, **HMR jalan lewat
      domain**, port tidak bergeser saat port default-nya sengaja dipakai proses lain. Hasilnya
      menentukan `dev.port` final tiap definisi
- [ ] Vite Host check: `astro-web.test` tidak kena "Blocked request"
- [ ] Rezure ditutup normal → tidak ada `node.exe` sisa. Rezure di-kill dari Task Manager → launch
      berikutnya membereskannya
- [ ] Project dengan compile error → status Stopped, log menampilkan error-nya, tidak ada loop
      restart

---

## Fase 3 — Open menyalakan dev server

**Tujuan:** Untuk project `proxy`, klik Open cukup — tidak perlu membuka modal dulu.

### Tasks
- [ ] `open_project_site`: untuk project `proxy` yang mati dan `autoStart` → start, tunggu siap, baru
      buka browser. Gagal → tidak membuka browser, kembalikan error dengan `code()` supaya UI bisa
      menawarkan "Lihat log"
- [ ] Baris dan kartu project: titik status di tombol Open (mati/starting/jalan), state "Starting…"
      selama menunggu, toast saat gagal
- [ ] Kolom/badge Framework menampilkan port (`Astro · :4400`)
- [ ] Filter "Running" di halaman Projects (opsional, kalau tidak membuat toolbar terlalu padat)

### Test
- [ ] Logika keputusan Open sebagai fungsi murni: profile `php` → langsung buka; `proxy` jalan →
      langsung buka; `proxy` mati + `autoStart` → start dulu; `proxy` mati tanpa `autoStart` →
      langsung buka

### Verifikasi
- [ ] Open project Astro yang mati → "Starting…" → browser terbuka di halaman yang sudah jadi,
      bukan 502
- [ ] Open saat `node_modules` belum ada → tidak start, pesan menyarankan Install
- [ ] Fitur Share pada project `proxy` tetap jalan (tunnel → nginx → dev server)

---

## Fase 4 — Dokumentasi

- [ ] `docs/v1/frameworks.md`: bagian Serve profile ditulis ulang (`php` dan `proxy`), field `dev`,
      tabel `portMode`, aturan `proxy` + `docroot`
- [ ] `docs/v1/projects.md`: bagian "Dev servers" — cara kerja proxy, modal, Open auto-start,
      `dev-servers.json`, troubleshooting (502, Vite "Blocked request", HMR tidak jalan, port
      bergeser, proses sisa)
- [ ] `docs/v3/rezure-app-v3.5-phases-tasks.md`: catatan bahwa keputusan "Node tidak di-proxy nginx"
      diubah di v3.3.0, dengan rujukan ke dokumen ini
- [ ] `services::agent_docs` + `agent-docs/how-rezure-works.md`: project `proxy`, port-nya, dan
      bahwa `dev-servers.json` tidak boleh diedit saat app jalan
- [ ] Doc comment layout di `utils/paths.rs` ditambah `dev-servers.json`
- [ ] Changelog v3.3.0 (diambil dari API, ditulis bersama rilisnya)

---

## Keputusan yang perlu diambil maintainer

Rekomendasi ditulis tebal; dokumen di atas sudah mengikuti rekomendasinya.

1. **Mengubah keputusan v3.5.** v3.5 memutuskan Node tidak dijalankan Rezure sebagai service. Rilis
   ini menjadikan dev server service yang opt-in per project. **Rekomendasi: ubah**, dengan batas
   yang tetap dijaga — tidak di Dashboard, tidak ikut Start all, tidak restart otomatis.
2. **Next/Nuxt/Astro/Vue/React/Svelte/Angular/Node generik pindah ke `proxy` secara default.**
   Project yang sudah terdeteksi sebagai framework itu berubah dari "folder mentah" (yang memang
   tidak pernah jalan) menjadi "502 sampai dev server dinyalakan". **Rekomendasi: ya.** Project yang
   menyajikan `dist/` lewat Custom folder tidak terpengaruh.
3. **Rentang port 4400–4499, konstanta, bukan setting.** Di luar port default tool populer (3000,
   4200, 4321, 5173, 8080) dan di luar rentang dinamis Windows (49152+). **Rekomendasi: konstanta
   dulu**; jadi setting kalau ada yang butuh.
4. **Menolak dotfile di profile `php` juga** (`location ~ /\.(?!well-known/)` → 404). Kebocoran `.env`
   bukan hanya di project Node: CodeIgniter 3, WordPress, dan PHP generik juga disajikan dari root.
   Ini mengubah semua vhost yang ada (nginx reload sekali saat upgrade). **Rekomendasi: ya, di
   Fase 1** — risikonya kecil dibanding `.env` yang bisa diunduh, dan `share` membuat situs lokal
   bisa diakses dari luar.
5. **Telemetry.** **Rekomendasi: tidak menambah event di v3.3.0.** `project.framework_changed` dari
   v3.2.0 sudah cukup untuk melihat siapa yang memakai framework `proxy`; event start/stop dev
   server ditunda sampai ada pertanyaan yang perlu dijawab datanya.

---

## Status

Rencana; belum ada yang diimplementasikan. Menunggu v3.2.0 dirilis dan keputusan di atas.

---

## Backlog (bukan v3.3.0)

- **Profile `static` dan `spa`** untuk hasil build (`dist/`, `build/`, `out/`, `.output/public`,
  `dist/<nama>/browser`) — tanpa proses sama sekali, `spa` dengan `try_files … /index.html`. Butuh
  pilihan "Dev / Build" per project karena folder build baru ada setelah `npm run build`
- Deteksi otomatis port untuk `portMode: fixed` (tanpa user mengisi), dari port yang di-listen pohon
  proses setelah start
- Dev server untuk runtime lain (Python `uvicorn`/`manage.py runserver`, Go) — `portMode` dan
  package manager-nya beda, tapi profile `proxy` dan `Service`-nya sama
- Monorepo (Turborepo/Nx) — beberapa app dalam satu folder, jadi beberapa domain/port per project
- Matikan dev server otomatis setelah lama tidak dipakai, untuk menghemat RAM
