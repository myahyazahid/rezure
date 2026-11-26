# Rezure (Desktop App) — v3.7: SQL Server LocalDB dan Port Dev Server

Rilis perbaikan setelah v3.6.1. Isinya dua hal: dev server yang port-nya tidak terdeteksi di IPv6,
dan SQL Server LocalDB yang gagal start di laptop user tanpa pesan yang jelas.

**Versi:** dinaikkan dari 3.6.1 ke **3.7.0** di semua tempat yang menyimpan versi app:
`package.json`, `package-lock.json` (root dan `packages[""]`), `src-tauri/Cargo.toml`,
`src-tauri/Cargo.lock` (entri `rezureapp`), dan `src-tauri/tauri.conf.json`.

---

## Fase 3.7.1 — Port dev server di IPv6, dan menghentikan "copy lain"

**Masalah:** Node 17+ me-resolve `localhost` ke `::1` lebih dulu, jadi Vite, Astro, dan sejenisnya
sering listen di IPv6 saja. Rezure hanya melihat IPv4, sehingga port yang sudah dipakai terbaca
bebas, dan start berikutnya gagal dengan error yang membingungkan.

- [x] `ports` membaca semua socket TCP yang listen, IPv4 dan IPv6, dari `netstat -ano` (tanpa
      `-p TCP`, yang hanya menampilkan IPv4).
- [x] Pemilihan port dev server ikut memperhitungkan socket di `::1`, bukan cuma bind ke
      `127.0.0.1`.
- [x] Membebaskan port mengulang untuk holder berikutnya, karena satu port bisa dipegang dua proses
      (`127.0.0.1` dan `::1`). Hasilnya dikonfirmasi lewat `netstat`, tidak cukup dari laporan sukses
      `taskkill`.
- [x] "Stop the other copy": kalau tidak ada copy yang listen (masih starting, atau cuma memegang
      lock file), semua runtime luar di project itu yang dihentikan. Tidak menemukan apa-apa
      dilaporkan sebagai error.
- [x] Shim `cmd.exe /d /s /c` milik npm/npx di antara dua proses node dikenali sebagai bagian dari
      dev server, bukan terminal user.

## Fase 3.7.2 — Diagnosa saat SQL Server LocalDB gagal start

**Masalah (kasus nyata, telemetry):** satu device mencatat `ProcessSpawnFailed` 6× dan SQL Server
tidak pernah muncul di "Services used". Jenis error itu dipakai banyak service, dan pesan aslinya
tidak pernah dikirim, jadi dari dashboard tidak bisa tahu apa yang terjadi. Selain itu
`SqlLocalDB.exe` selalu exit 0, dan kegagalannya hanya dikenali dari teks bahasa Inggris.

- [x] `ensure_running` membaca ulang status instance setelah `start`. Kalau belum Running, dianggap
      gagal. Ini menutup kasus Windows non-Inggris yang kegagalannya terbaca sukses.
- [x] `create` yang gagal dilaporkan sama seperti start yang gagal.
- [x] Jenis error baru di `AppError`, jadi dashboard bisa membedakannya: `LocalDbStartFailed`,
      `LocalDbSectorSize`, `LocalDbRestartPending`, `LocalDbFixIneffective`, `LocalDbFixCancelled`,
      dan `LocalDbFixFailed`.
- [x] Pesan error menyertakan error terakhir dari `error.log` instance (UTF-16), dan hanya kalau file
      itu ditulis sejak start tersebut.
- [x] `services::localdb_health`: cek ukuran sector fisik disk lewat
      `GetFileInformationByHandleEx(FileStorageInfo)`. Tidak butuh admin (`fsutil` butuh) dan tidak
      tergantung bahasa Windows. Hasilnya dibedakan menjadi perlu fix, tinggal restart, atau fix
      tidak berpengaruh (driver NVMe bukan `stornvme`), berdasarkan waktu tulis key registry
      dibanding waktu boot.
- [x] Tombol **Fix disk compatibility** menulis workaround resmi Microsoft
      (`stornvme\Parameters\Device\ForcedPhysicalSectorSizeInBytes = * 4095`) lewat satu UAC. Key
      hanya dibuat kalau belum ada, karena `New-Item -Force` di registry mengosongkan key yang sudah
      ada.
- [x] Tombol **Recreate instance** (dengan konfirmasi): delete lalu create instance. Login `rezure`
      untuk jembatan TCP dibuat ulang (`mssql::forget_localdb_login`).
- [x] Panel di bawah card SQL Server (`LocalDbHealthPanel.vue`) dan peringatan disk di Project
      Doctor untuk project yang `DB_HOST`-nya `(localdb)\Rezure`.
- [x] Docs: troubleshooting di `docs/v1/databases.md`. Action telemetry baru (`localdb.fix_disk`,
      `localdb.recreate`) di `docs/telemetry-contract.md`.

## Fase 3.7.3 — Katalog LocalDB: 2022, 2019, 2017

**Masalah:** hanya LocalDB 2022 RTM (16.0.1000.6) yang ditawarkan. Di mesin yang 2022-nya tidak
mau jalan, tidak ada alternatif dari dalam Rezure.

- [x] `msi::PACKAGES` berisi `sqllocaldb` (2022, tetap jadi default dan id yang di-install card
      Services), `sqllocaldb-2019` (15.0.2000.5), dan `sqllocaldb-2017` (14.0.1000.169). Masing-
      masing dicek 2026-11-26: SHA-256 di-pin, tanda tangan Authenticode Microsoft valid,
      ProductVersion cocok, dan property lisensinya sama (`IACCEPTSQLLOCALDBLICENSETERMS`).
      `every_pinned_installer_matches_its_download` lolos untuk semuanya.
- [x] Deteksi per rilis lewat folder engine (`<NNN>\LocalDB\Binn\sqlservr.exe`), lewat
      `msi::Detect`. Rilis ter-install berdampingan.
- [x] Versi aktif = versi instance itu sendiri (`SqlLocalDB info`), jadi tidak disimpan di settings.
      Pindah rilis = recreate instance di rilis itu (`SqlLocalDB create Rezure 15.0`). Alamatnya
      tetap `(localdb)\Rezure`.
- [x] Command `list_localdb_releases` dan `switch_localdb_release`, plus action telemetry
      `localdb.switch`.
- [x] Install version menampilkan ketiga rilis. Switch menampilkan dropdown rilis dengan konfirmasi
      (database di-detach, dan rilis yang lebih lama tidak bisa attach database dari rilis yang
      lebih baru).
- [x] Label SQL Server di telemetry memakai rilis instance, bukan rilis terbaru yang ter-install,
      tanpa spawn proses tambahan.
- SQL Server 2025 tidak ditawarkan: Microsoft tidak menerbitkan MSI LocalDB standalone untuk 2025
  (hanya ikut di installer Express), jadi tidak ada file yang bisa di-pin. LocalDB 2025 juga
  punya bug `RegExpr.dll` yang tidak ikut ter-install.

## Catatan kasus — LocalDB crash di ASUS TUF (AMD Ryzen 7 170)

Kasus yang memicu Fase 3.7.2 dan 3.7.3 **tidak terselesaikan oleh Rezure**. Crash-nya terjadi di
dalam `sqlservr.exe`: `error.log` mencatat `c00000fd EXCEPTION_STACK_OVERFLOW` sangat awal saat
startup.

| | Client (ASUS TUF) | Pembanding (Lenovo LOQ) |
|---|---|---|
| CPU | AMD Ryzen 7 170, 8C/16T | Intel i5-13450HX, 10C/16T |
| Build Windows | 26300 | 26300 |
| Sector size (`PhysicalBytesPerSectorForAtomicity`) | 4096 | 4096 |
| HVCI/VBS | Aktif | Aktif |
| LocalDB 2022 RTM | Crash | Jalan |

Build Windows, sector size, dan HVCI tereliminasi. Yang tersisa adalah platform hardware. Laporan
serupa di Microsoft Q&A (HP, Lenovo ThinkBook, semuanya Intel) tidak punya solusi, dan di laporan HP
2019 juga ikut crash. Saran ke client: coba rilis 2019, update BIOS/chipset ASUS, atau SQL Server
di Docker lewat Add connection.

---

## Persiapan rilis

- [x] `cargo fmt --check`, `cargo clippy --all-targets`: bersih.
- [x] `cargo test --lib`: 696 passed.
- [x] `npm run lint`, `vue-tsc --build`, `vite build`: lolos.
- [x] Checksum dan tanda tangan keempat installer MSI dicek terhadap download asli.
- [ ] Tes manual di app (`npm run tauri dev`):
  - [ ] Card SQL Server: Start, Stop, dan tidak ada panel disk di disk 4 KB.
  - [ ] Install version → SQL Server LocalDB: ketiga rilis tampil. Install 2019 (UAC).
  - [ ] Switch → SQL Server LocalDB → 2019. Peringatan downgrade muncul. Setelah konfirmasi,
        instance jalan di 15.0, dan Databases masih bisa konek.
  - [ ] Switch kembali ke 2022. Peringatan downgrade tidak muncul, karena ini naik versi.
  - [ ] Dev server Vite/Astro yang listen di `::1`: port terdeteksi, dan "stop the other copy"
        berhasil.
- [ ] `npm run tauri build` dengan kunci signing updater, lalu publish di dashboard Releases
      (versi `3.7.0`, `signature`, `download_url`, catatan di bawah).

### Catatan rilis (untuk dashboard)

```
Rezure 3.7.0

SQL Server LocalDB
- When LocalDB fails to start, Rezure now says why: LocalDB's own reason and the last
  errors from the instance's error.log, instead of a generic failure.
- Detects disks whose sector size SQL Server can't run on (some NVMe drives on
  Windows 11) and applies Microsoft's fix with one click (restart Windows after).
- Recreate instance: rebuilds a LocalDB instance left broken by a failed start.
- LocalDB 2019 and 2017 can now be installed alongside 2022, and the Switch page
  moves Rezure's instance between them. Projects keep using (localdb)\Rezure.
- Fixed: on a non-English Windows a failed start could read as started.

Dev servers
- Dev servers listening on IPv6 only (Vite, Astro on Node 17+) are no longer missed
  when checking ports, and stopping "the other copy" works when it isn't listening yet.
```

## Ditunda

- [ ] Kenali crash dari baris `Exception Code = …` di `error.log` (misalnya `c00000fd`), bukan hanya
      baris `Error: N`. Tampilkan petunjuk khusus dan jenis error sendiri (`LocalDbStackOverflow`).
      Saat ini crash seperti kasus di atas tidak muncul di pesan error Rezure.
- [ ] Baris LocalDB di Switch menampilkan "not installed" kalau instance belum pernah berhasil
      dibuat, padahal LocalDB-nya ter-install.
- [ ] Link lisensi 2019/2017 masih halaman umum `microsoft.com/en-us/useterms`. Link PDF per rilis
      tidak bisa diverifikasi karena situs Microsoft menolak request otomatis (403).
