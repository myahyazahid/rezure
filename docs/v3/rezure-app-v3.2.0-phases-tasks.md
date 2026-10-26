# Rezure (Desktop App) — v3.2.0: Framework Registry

Rilis ini mengganti deteksi framework project dari rantai `if` di kode menjadi **registry berbasis
data**: menambah framework berarti menambah satu entri definisi, bukan mengubah mesin deteksi. User
juga selalu bisa mengoreksi hasil deteksi, dan bisa menambah definisi framework sendiri tanpa
menunggu rilis.

Dipicu oleh bug yang ditemukan maintainer lewat pemakaian nyata (lihat Latar belakang). Fase 1 saja
sudah memperbaiki bug itu; Fase 2–4 yang membuat solusinya scalable.

Penomoran fase di dokumen ini (Fase 1–4) lokal untuk rilis v3.2.0, supaya tidak tertukar dengan
nomor versi patch (3.2.1, 3.2.2, …).

---

## Latar belakang: bug "Detected as Node" di project CodeIgniter 4

**Gejala.** Add existing folder → `C:\repository\ORDO\melisa-topup` (CodeIgniter 4) → dialog
menampilkan "Detected as **Node**". `sisgamer` (juga CI4) sama.

**Akar masalah 1 — urutan pengecekan.** `services::projects::detect_stack` memeriksa `package.json`
sebelum `composer.json`. `package.json` milik melisa-topup cuma berisi `sass` untuk compile CSS,
tapi itu sudah cukup untuk jatuh ke cabang "Node". Project Laravel selamat hanya karena `artisan`
dicek paling awal — project PHP lain yang memakai npm untuk aset (sass, Vite, Mix) akan salah label
dengan cara yang sama.

**Akar masalah 2 — docroot, dan ini yang membuat situsnya benar-benar rusak.**
`vhosts::vhost_config` menghasilkan template yang sama persis untuk semua stack; satu-satunya yang
dipengaruhi stack adalah `root`, lewat `projects::docroot`, yang hanya memberi `public/` ke Laravel.
Jadi label "Node" vs "PHP" tidak mengubah config nginx sama sekali — tapi CI4 disajikan dari root
project yang tidak punya `index.php` (front controller-nya ada di `public/index.php`), sehingga 404.
Andai terdeteksi "PHP" pun hasilnya sama.

**Workaround yang sudah terlanjur dipakai.** `C:\rezure\etc\links.json` maintainer berisi
`melisa-topup` → `...\melisa-topup\public` dan `sisgamer` → `...\sisgamer\public`: folder `public/`
di-link langsung supaya situsnya jalan. Efek sampingnya, tombol Open folder/terminal membuka
`public/`, bukan root project, dan domain `melisa-topup.test` sudah terpakai — makanya link baru ke
root mendapat `melisa-topup-2.test`.

**Gambaran nyata dari satu workspace** (`C:\repository\ORDO`, 17 project): 11 Laravel (aman, karena
`artisan`), 2 CodeIgniter 4 (rusak), 4 Flutter (`pubspec.yaml` + `web/`, di luar cakupan). Semua
project PHP di situ punya `package.json` juga — "PHP + npm untuk aset" itu kasus normal, bukan
pengecualian.

---

## Desain

### Tiga konsep yang dipisah

| Konsep | Isi | Siapa yang menambah |
|---|---|---|
| **Framework** | id, label, aturan deteksi, prioritas, docroot, serve profile | data — kontributor (built-in) atau user (`etc/frameworks.json`) |
| **Serve profile** | template config nginx | kode Rust saja |
| **Docroot** | subfolder yang disajikan (`public`, `web`, `webroot`) | data, divalidasi |

Serve profile sengaja tidak bisa didefinisikan lewat data. Semua vhost di-include oleh satu
`nginx.conf`, jadi satu definisi yang typo atau berbahaya bisa menghentikan *semua* situs — alasan
yang sama dengan `projects::is_safe_domain`. Definisi framework hanya bisa **memilih** profile yang
ada, tidak bisa menyisipkan directive nginx.

Di v3.2.0 hanya ada satu profile: **`php`** — template yang dipakai sekarang
(`try_files $uri $uri/ /index.php?$query_string` + FastCGI ke pool PHP). Semua framework built-in
memakainya, jadi perilaku nginx untuk project yang sudah ada tidak berubah. `static`, `spa`, dan
`proxy` dicatat di Backlog.

### Bentuk definisi

```json
{
  "id": "codeigniter4",
  "label": "CodeIgniter 4",
  "priority": 90,
  "detect": { "any": [{ "file": "spark" }, { "composer": "codeigniter4/framework" }] },
  "docroot": "public",
  "serve": "php"
}
```

| Field | Aturan |
|---|---|
| `id` | `[a-z0-9-]`, unik, stabil — dipakai override per project dan telemetry, jadi tidak boleh diganti setelah rilis |
| `label` | teks bebas untuk UI |
| `priority` | 0–100, yang tertinggi menang (lihat tabel Prioritas) |
| `detect` | pohon aturan, lihat di bawah |
| `docroot` | opsional, subfolder relatif; kosong = root project |
| `serve` | opsional, id serve profile; v3.2.0 hanya `php` (default) |
| `category` | opsional, judul kelompok di picker (`PHP`, `JavaScript`, `Generic`); hanya tampilan, deteksi tidak membacanya. Definisi user tanpa kategori masuk "Your frameworks" di urutan teratas |

**Aturan deteksi** — hanya membaca file, tidak pernah menjalankan apa pun (scan berjalan untuk semua
project setiap kali halaman Projects dibuka):

| Aturan | Cocok kalau |
|---|---|
| `{ "file": "spark" }` | file ada (path relatif, boleh bersarang: `system/core/CodeIgniter.php`) |
| `{ "dir": "wp-admin" }` | folder ada |
| `{ "composer": "laravel/framework" }` | paket ada di `require`/`require-dev` `composer.json` |
| `{ "npm": "next" }` | paket ada di `dependencies`/`devDependencies` `package.json` |
| `{ "any": [ … ] }` / `{ "all": [ … ] }` | salah satu / semua aturan di dalamnya cocok |
| `{ "not": { … } }` | aturan di dalamnya tidak cocok |

`composer.json` dan `package.json` di-parse **sekali per project per scan**, lalu semua definisi
dievaluasi terhadap hasil parse itu. Kedalaman pohon aturan dibatasi (8 level) supaya definisi user
yang aneh tidak bisa membuat scan lambat.

**Validasi path** (berlaku untuk `docroot` dan aturan `file`/`dir`): harus relatif, tidak boleh
mengandung `..`, tidak boleh absolut (`C:\…`, `\\server\…`), tidak boleh ada segmen kosong. Docroot
yang valid tapi foldernya tidak ada di project tertentu → jatuh ke root project (perilaku
`docroot()` untuk Laravel sekarang), jadi satu definisi bisa melayani dua layout — mis. Drupal
berbasis Composer dengan `web/` dan Drupal tarball tanpa `web/`.

### Prioritas

Yang menang adalah definisi cocok dengan prioritas tertinggi, bukan yang kebetulan dicek duluan.
Prioritas sama → urutan di registry (deterministik).

| Prioritas | Kelompok | Contoh |
|---|---|---|
| 90 | Framework backend dengan penanda khas | Laravel (`artisan`), CI4 (`spark`), WordPress (`wp-config.php`) |
| 80 | Framework backend yang dikenali dari Composer saja | Symfony, Yii2, CakePHP, Slim, Laminas |
| 55 | Meta-framework frontend | Next.js, Nuxt (mengalahkan React/Vue yang jadi dependency-nya) |
| 50 | Framework frontend | Vue, React, Svelte, Angular |
| 30 | PHP generik dengan front controller di `public/` | `public/index.php` ada, `index.php` di root tidak |
| 20 | PHP generik | `composer.json` atau `index.php` |
| 15 | Node generik | `package.json` |
| 10 | Static | `index.html` |
| — | Unknown | tidak ada yang cocok → root project |

Ini yang menyelesaikan bug di atas secara umum, bukan khusus CI4: framework PHP apa pun (90/80/30/20)
selalu mengalahkan Node generik (15), dan Laravel + Inertia/Vue tetap Laravel. Baris 30 menangkap
framework PHP yang belum punya definisi tapi memakai pola `public/index.php` yang umum.

### Tiga lapis sumber

1. **Built-in** — `src-tauri/resources/frameworks.json`, di-embed lewat `include_str!`, di-parse
   sekali (`OnceLock`). Satu test memastikan file ini selalu valid, jadi kegagalan parse tidak
   pernah terjadi di runtime.
2. **Milik user** — `C:\rezure\etc\frameworks.json`, format sama (`{ "frameworks": [ … ] }`).
   Digabung di atas built-in: `id` sama = menggantikan built-in, `id` baru = ditambahkan. Entri yang
   tidak valid dilewati satu per satu dengan alasannya; JSON yang rusak total → seluruh file
   diabaikan. Daftar project tetap jalan dalam kedua kasus (sama seperti `config::links`
   memperlakukan `links.json` yang rusak), dan masalahnya ditampilkan di UI.
3. **Override per project** — `C:\rezure\etc\project-frameworks.json`, dikunci dengan id project:

   ```json
   {
     "overrides": {
       "melisa-topup-3dff0a": { "framework": "codeigniter4" },
       "legacy-app": { "docroot": "htdocs" }
     }
   }
   ```

   `framework` = pakai definisi itu; `docroot` saja = **Custom** (serve profile `php`, label
   "Custom"). Override selalu menang atas deteksi. Override yang menunjuk ke `id` yang sudah tidak
   ada (mis. definisi user dihapus) diabaikan, project kembali ke hasil deteksi, dan masalahnya
   dilaporkan di banner halaman Projects.

Registry (lapis 1 + 2) dimuat sekali ke cache saat pertama dipakai (sinkronisasi vhost waktu
startup) dan dimuat ulang hanya lewat command `reload_frameworks` — sesuai prinsip config dimuat
sekali, bukan dibaca ulang tiap aksi. File
override (lapis 3) dibaca di dalam `scan_projects`, sama seperti `links.json`, karena fungsi itu
sinkron dan tidak punya akses ke state Tauri (alasannya sama dengan yang tertulis di
`config::links`).

**Kenapa override di JSON, bukan SQLite.** `docs/v1/projects.md` menyebut SQLite sebagai tempat
riwayat yang *derived and disposable*. Override framework adalah deklarasi user: kalau `rezure.db`
di-reset, project tidak boleh diam-diam kembali disajikan dari folder yang salah. Satu file untuk
project scanned **dan** linked juga berarti satu jalur kode di `scan_projects`, dan semua konsumen
(vhost, daftar project, launcher, `AGENTS.md`) langsung mendapat hasil yang sudah dikoreksi tanpa
plumbing tambahan seperti `php_versions` di `sync_vhosts`.

Konsekuensi yang perlu didokumentasikan: id project scanned adalah nama foldernya, jadi me-rename
folder di `www` melepas override-nya. Unlink **ikut menghapus** override project itu, dan pilihan
di dialog Add selalu jadi kebenaran untuk id baru (termasuk Auto, yang menghapus sisa override).
*Berubah dari rencana awal* ("unlink tidak menghapus, link ulang mengembalikan"): id linked project
diturunkan dari path **dan nama**, sementara preview di dialog Add belum tahu id-nya — override lama
yang muncul lagi diam-diam setelah user melihat "Detected as X" akan membingungkan.

### Bentuk data yang berubah

`ProjectInfo` (`db::projects`):
- `stack: String` **tetap** — label efektif, supaya badge di UI dan kolom `stack` di SQLite tidak
  berubah (tidak perlu migrasi).
- `framework: String` — id efektif.
- `detected_framework: String` — id hasil deteksi, untuk opsi "Auto (detected: X)" di UI.
- `framework_overridden: bool`.
- `docroot: String` — path absolut yang disajikan nginx. `vhosts::sync_vhosts` memakai field ini
  langsung, tidak lagi menghitung ulang dari `stack`.

Linked project yang foldernya hilang: `stack` tetap "Missing" seperti sekarang, `framework` dan
`detected_framework` bernilai `unknown`.

`LinkPreview` mendapat `framework` dan `detected_framework`, plus `suggestion` (lihat Fase 2).
`docroot` sudah ada.

---

## Fase 1 — Mesin registry + perbaikan deteksi

**Tujuan:** Bug melisa-topup/sisgamer beres, dan deteksi sudah berjalan di atas registry. Belum ada
UI baru; yang berubah dari sisi user hanya label dan docroot yang benar.

### Tasks
- [x] Modul baru `services::frameworks`: tipe `FrameworkDef`, `Rule`, `ServeProfile` (enum, v3.2.0
      hanya `Php`), `Registry`, `Serving`
- [x] Validasi definisi: format `id`, `id` unik, rentang `priority`, path `docroot`/`file`/`dir`
      (relatif, tanpa `..`, tidak absolut, tanpa segmen kosong, tanpa `$`/`"`), kedalaman aturan
      ≤ 8, `serve` dikenal, field tak dikenal ditolak (`deny_unknown_fields`, supaya typo seperti
      `docRoot` dilaporkan). Mengembalikan alasan yang bisa dibaca manusia — dipakai ulang di Fase 3
- [x] `src-tauri/resources/frameworks.json` berisi 23 definisi awal:
  - PHP (90): Laravel (`public`), CodeIgniter 4 (`public`), CodeIgniter 3 (root), WordPress (root),
    Bedrock (`web`), Drupal (`web`, jatuh ke root untuk layout tarball), Joomla (root)
  - PHP lewat Composer (80): Symfony (`public`), Yii 2 basic (`web`), CakePHP (`webroot`), Slim
    (`public`), Laminas/Mezzio (`public`)
  - JS: Next.js, Nuxt, Astro (55); Vue, React, Svelte, Angular (50)
  - Generik: `php-public` (30), `php` (20), `node` (15), `static` (10)
- [x] Engine deteksi: kumpulkan fakta project sekali (paket Composer, paket npm), evaluasi semua
      definisi, pilih prioritas tertinggi; tidak ada yang cocok → `unknown`. Definisi yang tidak
      mungkin menang (prioritas ≤ yang sudah cocok) tidak dievaluasi sama sekali
- [x] Ganti `projects::detect_stack`/`detect_from_package_json` dengan engine ini; ganti
      `projects::docroot(path, stack)` dengan `frameworks::resolve_docroot` (termasuk fallback ke
      root kalau subfolder tidak ada)
- [x] Tambah `framework`, `detected_framework`, `framework_overridden`, `docroot` ke `ProjectInfo`
      dan `framework`/`detected_framework` ke `LinkPreview`
- [x] `vhosts::sync_vhosts`/`vhost_config` memakai `project.docroot` (`vhost_config` tidak lagi
      menerima `stack`)
- [x] `src/types/project.ts` ikut diperbarui; peringatan "No framework markers found" di
      `LinkProjectModal.vue` membandingkan `preview.framework === 'unknown'`, bukan label
- [x] Test `#[ignore]` `print_detection`: deteksi lawan folder project nyata tanpa me-link apa pun
      (`REZURE_DETECT_DIR`)

### Test
- [x] `builtin_definitions_are_all_valid` — menjamin parse built-in tidak pernah gagal di runtime
- [x] Fixture per framework di `src-tauri/tests/fixtures/frameworks/<id-yang-diharapkan>[--varian]/`
      (32 folder); `every_fixture_is_detected_as_its_folder_name` mengiterasi semuanya.
      `every_builtin_framework_has_a_fixture` memastikan arah sebaliknya: entri built-in tanpa
      fixture menggagalkan test
- [x] Fixture kasus prioritas: `codeigniter4--with-sass` (bentuk persis melisa-topup: `spark` +
      `composer.json` + `package.json` berisi `sass`), `laravel--with-vue`, `nextjs--with-react`,
      `nuxt--with-vue`, `php--with-npm`, `php-public--no-root-index`, `drupal--tarball` (tanpa
      `web/` → docroot root)
- [x] Validasi path menolak `..`, `C:\x`, `\\server\x`, `/abs`, `a//b`, `./public`, `"`, `$`
- [x] Test lama di `services::projects` (`detects_laravel_via_artisan`,
      `falls_back_to_node_for_unrecognized_package_json`, dst.) tetap lulus, plus
      `a_php_project_that_also_uses_npm_stays_php`
- [x] `vhosts`: `vhost_config_roots_codeigniter4_projects_at_public` (dari folder sampai baris
      `root`)

### Verifikasi
- [x] **Vhost project yang sudah ada tidak berubah**: `root` di kelima vhost yang ada di
      `C:\rezure\data\nginx\vhosts` dibandingkan dengan docroot hasil scan baru (`print_real_scan`)
      — identik semua, dan template-nya sendiri tidak berubah, jadi `VhostSync::changed` false dan
      nginx tidak di-reload saat upgrade
- [x] Deteksi lawan `C:\repository\ORDO` (`print_detection`): `melisa-topup` dan `sisgamer` →
      CodeIgniter 4, docroot `public`; 11 project Laravel tetap Laravel + `public`; project Flutter
      → Unknown (sesuai, di luar cakupan)
- [ ] Di app sungguhan: Add existing folder → `melisa-topup` (root) → "Detected as CodeIgniter 4 ·
      served from public/", situs terbuka. Sama untuk `sisgamer`
- [x] `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test --lib` (586 lulus),
      `npm run lint`, `npm run type-check`, `npm run build-only`

---

## Fase 2 — Koreksi manual (override per project)

**Tujuan:** Deteksi yang salah selalu bisa dikoreksi user, baik saat menambah folder maupun
sesudahnya, untuk project scanned maupun linked.

### Tasks
- [x] `config::project_frameworks` (baru): load/save `etc/project-frameworks.json`, pola sama dengan
      `config::links` (file rusak → kosong + `log::warn`, tidak menggagalkan daftar project). Dibaca
      per entri, jadi satu entri salah hanya menggugurkan entri itu; BOM dari Notepad diterima
- [x] `scan_projects` menerapkan override setelah deteksi; override ke `id` yang tidak dikenal
      diabaikan (project kembali ke deteksi) dan dilaporkan lewat `list_frameworks`
- [x] Command `list_frameworks` → `{ frameworks: [{ id, label, docroot, source }], problems }`,
      urut abjad — UI tidak pernah meng-hardcode daftar framework
- [x] `preview_project_link(path, framework?, docroot?)` — preview ulang dengan pilihan user,
      supaya docroot di dialog selalu dihitung di Rust
- [x] `link_project(path, name, domain, framework?, docroot?)` — pilihan yang sama dengan hasil
      deteksi tidak ditulis, supaya perbaikan deteksi di rilis berikutnya tetap berlaku. Override
      ditulis **sebelum** link-nya, jadi kegagalan tidak meninggalkan setengah tambah
- [x] `unlink_project` ikut menghapus override project itu (lihat bagian Desain)
- [x] Command `set_project_framework(id, framework?, docroot?)` — keduanya kosong = kembali ke
      Auto; lalu `sync_vhosts_and_reload`
- [x] Error baru di `AppError`: `UnknownFramework(String)` dan `InvalidDocroot { docroot, reason }`,
      lengkap dengan `code()`
- [x] **Saran folder induk** (`projects::parent_suggestion`): memilih `melisa-topup\public`
      sementara `melisa-topup` terdeteksi CI4 dengan docroot `public` → preview membawa
      `suggestion`, dialog menampilkan "This looks like the public/ folder of a CodeIgniter 4
      project" dengan tombol "Add melisa-topup instead". Tidak muncul kalau folder induknya sudah
      di-link, ada di `www`, atau folder sistem
- [x] `LinkProjectModal.vue`: "Detected as X · served from public/" + tombol **Change** →
      `FrameworkPicker`; setiap ganti pilihan memanggil ulang preview, nama/domain yang sudah diedit
      user tidak ditimpa
- [x] `ProjectFrameworkModal.vue` (baru): status sekarang ("Served as X from public/ · set
      manually, detection says Y"), picker, Save. *Beda dari rencana:* pakai dropdown
      `FrameworkPicker.vue` yang sama dengan dialog Add, bukan daftar tombol ala modal PHP — dengan
      23+ framework, daftar tombol terlalu panjang, dan dua dialog jadi menawarkan pilihan yang
      persis sama. Dropdown-nya kustom (`FrameworkPickerMenu.vue`), bukan `<select>` native yang
      daftarnya digambar Windows: panel glass dengan pencarian, judul kelompok per `category`,
      docroot sebagai pill, centang di pilihan aktif, navigasi panah/Enter/Esc, dan membuka ke atas
      kalau ruang di bawah sempit
- [x] Badge di baris dan kartu project (`ProjectsView.vue`) jadi tombol yang membuka modal itu
      (tooltip menyebut folder yang disajikan); project yang di-override diberi titik aksen. Kolom
      "Stack" di tampilan list jadi "Framework"
- [x] Store `projects`: `frameworkCatalog`, `fetchFrameworks`, `reloadFrameworks`,
      `frameworkLabel`, `setFramework`, `frameworkModalFor`, `open/closeFrameworkModal`

### Test
- [x] Override menang atas deteksi; Custom docroot disajikan sebagai `custom`
- [x] Override ke id yang tidak dikenal → hasil deteksi dipakai, tidak error
- [x] `choice()` menolak framework + docroot sekaligus; `validate_choice` menolak id tak dikenal,
      `..`, path absolut, dan folder yang tidak ada
- [x] File override yang rusak → dibaca kosong, tidak panic; entri rusak dilewati satu per satu
- [x] Saran folder induk muncul untuk `<project CI4>\public` dan tidak muncul untuk folder biasa
      maupun project itu sendiri

### Verifikasi manual (di app sungguhan, belum dilakukan)
- [ ] Ganti framework melisa-topup ke "PHP" → 404 (docroot root); kembali ke Auto → jalan lagi,
      tanpa restart nginx manual
- [ ] Override project scanned di `www`, rename foldernya → override lepas (sesuai dokumentasi)
- [ ] Pilih `melisa-topup\public` di Add existing folder → saran folder induk muncul

---

## Fase 3 — Framework milik user + Custom docroot

**Tujuan:** User bisa menambah framework yang belum dikenal Rezure (framework internal kantor, CMS
lokal) tanpa menunggu rilis, dan project yang strukturnya unik tetap bisa disajikan dengan benar
lewat Custom docroot.

### Tasks
- [x] Muat `etc/frameworks.json` dan gabungkan di atas built-in (id sama = mengganti di tempat, id
      baru = ditambah), memakai validasi dari Fase 1. `unknown` dan `custom` adalah id cadangan
- [x] `list_frameworks` ikut mengembalikan `problems` (mis. `frameworks.json: "escape" was skipped
      — docroot it can't contain "." or ".."`), termasuk project yang di-set ke framework yang sudah
      tidak ada
- [x] Command `reload_frameworks` → muat ulang registry, lalu `sync_vhosts_and_reload`
- [x] Banner di halaman Projects kalau `problems` tidak kosong, dengan tombol Reload; tombol
      "Reload frameworks.json" juga ada di dialog framework
- [x] Opsi **Custom folder…** di picker (dialog Add dan dialog framework): input subfolder,
      divalidasi di Rust (aturan path yang sama + foldernya harus ada), disimpan sebagai
      `{ "docroot": "…" }`
- [x] `services::agent_docs`: kolom "Framework (served folder)" di tabel project `AGENTS.md`, aturan
      tentang badge dan `etc/frameworks.json`; panduan `agent-docs/how-rezure-works.md` dapat bagian
      "Frameworks and document roots" (format + contoh), dan `frameworks.json` dikecualikan dari
      larangan "jangan edit JSON di `etc\`"
- [x] Doc comment layout di `utils/paths.rs` ditambah `frameworks.json` dan
      `project-frameworks.json`

### Test
- [x] Definisi user mengganti built-in dengan id sama, dan menambah yang baru
- [x] Satu entri tidak valid dilewati (docroot keluar folder, field typo, serve profile tak
      dikenal), entri lain di file yang sama tetap dipakai
- [x] JSON rusak total → hanya built-in yang dipakai, `problems` berisi alasannya
- [x] Id cadangan dan `any`/`all` kosong ditolak
- [x] Custom docroot ditolak untuk `..`/path absolut, diterima untuk `htdocs`/`src/web`

### Verifikasi manual (di app sungguhan, belum dilakukan)
- [ ] Tambah definisi percobaan di `etc/frameworks.json`, klik Reload → muncul di dropdown tanpa
      restart app
- [ ] Rusak file itu dengan sengaja → banner muncul, daftar project tetap tampil

---

## Fase 4 — Feedback loop lewat telemetry

**Tujuan:** Maintainer tahu framework apa yang dipakai user dan di mana deteksi salah, sehingga
daftar built-in bisa diperkaya berdasarkan data, bukan tebakan.

Fase ini mengubah data yang dikirim keluar dari mesin user. Disetujui maintainer bersama
rekomendasi lain di dokumen ini; kontraknya ada di `docs/telemetry-contract.md`. Tetap tunduk pada
`shareUsageData` seperti semua event lain.

### Tasks
- [x] Event `project.linked` (`event_name` = id framework efektif), payload
      `{ "detected": "<id>", "chosen": "<id>" | null }`, dari command `link_project`
- [x] Event `project.framework_changed`, payload sama, dari command `set_project_framework`
      (`chosen: null` = kembali ke Auto)
- [x] Hanya id built-in yang dikirim apa adanya (`Registry::reportable_id`); definisi user dikirim
      sebagai `"user-defined"` (termasuk yang menimpa id built-in) dan Custom docroot sebagai
      `"custom"`. Tidak ada nama project, path, domain, folder Custom, atau isi `composer.json`.
      Dibentuk oleh fungsi murni `telemetry::framework_context`, dengan test untuk ketiga kasusnya
- [x] Perbarui `docs/telemetry-contract.md`: tabel "What's recorded", bentuk payload, dan
      "What's deliberately not sent"

### Dependency ke proyek lain
- `laravel-api`: sudah dicek ke kodenya — `EventRequest` memvalidasi `event_type` sebagai string
  bebas maks. 64 karakter dan `payload` sebagai `nullable|array`, jadi kedua event diterima dan
  disimpan tanpa perubahan backend. `api-documentation/telemetry-api.md` juga menyatakan
  `event_type`/`event_name` bebas dan merujuk ke `docs/telemetry-contract.md`, jadi tidak perlu
  diubah
- `rezure-dashboard`: dua tampilan baru — framework yang paling sering dikoreksi (`detected` ≠
  `chosen` → aturan deteksinya salah) dan porsi `unknown`/`user-defined`/`custom` (→ framework apa
  yang perlu dijadikan built-in)

---

## Dokumentasi
- [x] `docs/v1/projects.md`: bagian baru "Frameworks" (cara deteksi, tabel prioritas, cara
      mengoreksi, menambah framework), "Stack detection" di bagian internal ditulis ulang jadi
      "Framework detection", troubleshooting "situs 404" dan "Some framework settings aren't being
      used", serta lokasi `links.json` yang sudah usang (`%APPDATA%` → `C:\rezure\etc`)
- [x] `docs/v1/frameworks.md` (baru): referensi format definisi, aturan deteksi, aturan path, band
      prioritas, serve profile, dan dua jalur menambah framework — PR ke built-in (entri + folder
      fixture) atau `etc/frameworks.json` lokal
- [x] `contributing.md`: bagian "Adding a Framework" yang menunjuk ke `docs/v1/frameworks.md`
- [x] `docs/telemetry-contract.md` (lihat Fase 4)
- [ ] Changelog v3.2.0 — changelog di app diambil dari API (`commands::changelog`), bukan dari file
      di repo ini, jadi ditulis bersama rilisnya

---

## Keputusan

Diputuskan maintainer, sesuai rekomendasi:

1. **Serve profile di v3.2.0 hanya `php`.** Perilaku nginx untuk project yang sudah ada tidak
   berubah, dan project Vue/Node tetap disajikan seperti sekarang.
2. **Format JSON, bukan TOML.** Konsisten dengan `settings.json`, `profiles.json`, `links.json` di
   `etc/`, dan tidak menambah dependency. JSON tidak bisa diberi komentar; itu ditutup oleh
   `docs/v1/frameworks.md`.
3. **Override untuk project scanned juga, bukan hanya linked**, lewat satu file
   `project-frameworks.json` untuk keduanya.
4. **Fase 4 (telemetry) ikut dikerjakan.**

---

## Status

Fase 1–4 dan dokumentasinya sudah diimplementasikan; semua pengecekan otomatis lulus (lihat
Verifikasi di Fase 1). Yang tersisa hanya verifikasi manual di jendela app sungguhan (daftar
"Verifikasi manual" di Fase 1–3) dan changelog rilis. Belum ada bump versi — `Cargo.toml` dan
`package.json` masih 3.1.1.

---

## Backlog (bukan v3.2.0)

- **Serve profile tambahan:** `static` (tanpa PHP), `spa` (`try_files … /index.html` untuk hasil
  build Vue/React), `proxy` (nginx meneruskan ke dev server Node/Python/Go di port tertentu). Format
  definisi sudah punya field `serve`, jadi ini tinggal menambah template di Rust. `proxy`
  bertentangan dengan keputusan v3.5 bahwa Node tidak di-proxy nginx, jadi butuh keputusan desain
  tersendiri
- **Katalog remote:** definisi built-in diperbarui dari server tanpa rilis app. Formatnya sudah
  data; yang dibutuhkan adalah tanda tangan seperti manifest updater (`docs/version-contract.md`)
- Yii2 advanced template (`frontend/web` + `backend/web` — dua docroot untuk satu project) dan
  Flutter web (`pubspec.yaml`, hasil build di `build/web`)

---

## Setelah rilis — data maintainer sendiri

Dua link lama yang menunjuk ke `\public` (melisa-topup, sisgamer) tetap jalan setelah upgrade:
folder `public/` itu berisi `index.php`, jadi terdeteksi sebagai PHP dengan docroot root — yang
memang folder `public/`. Untuk merapikannya: hapus keduanya dari Rezure, lalu tambahkan ulang folder
root-nya. Domain `melisa-topup.test`/`sisgamer.test` kembali, dan Open folder/terminal membuka root
project. Ini tidak dilakukan otomatis; Rezure tidak mengubah `links.json` tanpa aksi user.
