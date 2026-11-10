# Rezure (Desktop App) — v3.6: Perbaikan untuk Laptop Baru

Rilis perbaikan setelah v3.5, dari kasus nyata user dengan laptop baru: `composer` tidak dikenal di
terminal, dan MariaDB dari katalog tidak bisa di-start.

**Versi:** dinaikkan dari 3.5.0 ke **3.6.0** di semua tempat yang menyimpan versi app:
`package.json`, `package-lock.json` (root dan `packages[""]`), `src-tauri/Cargo.toml`,
`src-tauri/Cargo.lock` (entri `rezureapp`), dan `src-tauri/tauri.conf.json`.

**3.6.1:** dinaikkan dari 3.6.0 di tempat yang sama, berisi dua bagian setelah Fase 3.6.3:
"Perbaikan setelah rilis 3.6.0 — Service yatim setelah update" dan "Node.js bisa dipanggil dari
terminal".

---

## Fase 3.6.1 — Composer bisa dipanggil dari terminal

**Masalah:** Composer dari menu Switch cuma berupa `composer.phar`, tanpa shim, dan foldernya tidak
masuk PATH mana pun. `composer create-project …` hanya jalan di laptop yang kebetulan punya
Composer-Setup global.

- [x] Shim `composer.bat` dan `composer` (untuk Git Bash) ditulis di samping `composer.phar`, sama
      seperti Composer-Setup. Versi lama dapat shim saat dipakai (`services::composer::bin_dir`).
- [x] Terminal project memasukkan folder Composer aktif ke PATH (`services::launcher`).
- [x] Junction `current\composer` dan entri PATH-nya, satu paket dengan "Use Rezure's PHP everywhere"
      (`services::php_path`). Junction ikut pindah saat Composer di-install atau di-switch.
- [x] Kartu PATH di Switch menampilkan baris `composer → <versi>`.

## Fase 3.6.2 — "Use Rezure's PHP everywhere" nyala secara default

**Masalah:** Kebanyakan user tidak tahu toggle ini ada, lalu `php` di terminal berasal dari
Laragon sementara situsnya jalan di PHP Rezure, sehingga versinya bentrok.

- [x] Startup menyalakannya sekali, apa pun kondisinya (termasuk kalau Laragon/XAMPP ada di PATH),
      untuk install baru maupun upgrade. Ditandai dengan `Settings::php_path_auto_enabled`, jadi
      user yang mematikannya tetap mati.
- [x] Tanpa UAC: hanya PATH *user* dan junction di folder Rezure yang ditulis. Konsekuensinya,
      PHP/Composer di PATH *sistem* tetap menang.

## Fase 3.6.3 — MariaDB dari katalog bisa di-start

**Masalah:** Profil default "Rezure" dibuat dengan versi manifest 11.2.2, sedangkan katalog hanya
menawarkan 10.6/10.11/11.4/11.8. Pencocokan `major.minor` di `db_profiles::resolve_server_exe`
tidak pernah cocok, dan errornya tertutup jadi "binary not installed".

- [x] Datadir yang masih kosong memakai binary terbaru yang ter-install, karena belum ada data
      yang perlu dilindungi.
- [x] Versi binary yang mem-bootstrap datadir dicatat ke profil (`db_profiles::record_version`).
- [x] `ProcessService::start_missing` meneruskan error asli dari resolver, tidak lagi
      menyamaratakannya jadi `BinaryNotInstalled`.

## Perbaikan setelah rilis 3.6.0 — Service yatim setelah update

**Masalah (kasus nyata, update 3.5.0 → 3.6.0 saat service jalan):** setelah update, Start all
gagal. Telemetry device itu mencatat `PortInUse` 6× dan `PortInUseBy` 1×, dan PHP 8.5.12 tidak bisa
di-kill dari app.

**Penyebab:** di Windows, plugin updater menutup app dengan `std::process::exit(0)` begitu
installer jalan. Sebelumnya plugin hanya memanggil `cleanup_before_exit()`, jadi
`RunEvent::ExitRequested` (satu-satunya tempat `stop_all()` dipanggil) terlewat. Nginx, php-cgi, dan
database tetap jalan tanpa induk. Saat Rezure baru start, `reap_orphan` membunuhnya lewat file PID,
tapi port langsung dicek tanpa menunggu Windows melepas socket-nya. Bind gagal sementara `netstat`
sudah tidak menunjukkan listener, jadi errornya `PortInUse` tanpa nama pemegang dan tanpa tombol
stop.

- [x] Update dipecah jadi `download()` → `stop_services_for_update` (command baru, `stop_all()`) →
      `install()` di `stores/update.ts`. Service tetap jalan selama download, dan baru dihentikan
      tepat sebelum installer jalan.
- [x] `ProcessService::wait_for_ports_released`: setelah `reap_orphan` membunuh proses sisa, tunggu
      sampai port worker itu bisa di-bind lagi (maksimal ±2,5 detik, sama dengan
      `ports::reclaim`).
- Catatan: perbaikan pertama baru berlaku untuk update *dari* versi yang memuatnya, karena kode
  update yang jalan adalah milik versi lama. Update 3.6.0 → berikutnya masih meninggalkan
  service yatim, tapi versi baru membersihkannya saat start berkat perbaikan kedua.
- **Belum terbukti** bahwa PHP 8.5.12 yang "tidak bisa di-kill" hanya disebabkan race ini. Kalau
  `taskkill` dari user menjawab *Access is denied*, berarti proses lama jalan dengan hak admin, dan
  itu kasus lain.

## Node.js bisa dipanggil dari terminal

**Masalah:** Node.js yang di-install dari Switch hanya tampil versinya. `node`/`npm` cuma jalan di
terminal project Rezure, karena tidak ada link PATH global untuk Node (sama seperti kasus
Composer di Fase 3.6.1).

- [x] Junction `current\node` → folder `node.exe` versi aktif, plus entri PATH-nya setelah PHP dan
      Composer, satu paket dengan "Use Rezure's PHP everywhere". `php_path` sekarang punya enum
      `Tool` (Composer, Node) untuk link selain PHP. Fungsinya digeneralisasi: `sync_tool`,
      `refresh_tool_link`, `ensure_tool_entries`, `with_entries_first(&[..])`.
- [x] Install atau switch Node.js ikut menggeser link-nya (`commands::node`).
- [x] Install yang toggle-nya sudah nyala di 3.6.0 (PATH berisi PHP + Composer) mendapat entri Node
      saat startup lewat `ensure_tool_entries`.
- [x] Kartu PATH di Switch menampilkan baris `node → <versi>`.
- `npm install -g` dari zip Node.js menaruh paketnya di folder versi itu sendiri, jadi CLI global
  ikut jalan lewat junction, tapi terpisah per versi.
- Node dari installer nodejs.org/nvm-windows biasanya ada di PATH *sistem*, jadi tetap menang
  atas milik Rezure.

---

## Ditunda — Ketergantungan pada PowerShell

Kasus user dengan PowerShell rusak. Dibahas, belum dikerjakan:

- [ ] Panggil `powershell.exe` lewat path absolut (`%SystemRoot%\System32\WindowsPowerShell\v1.0`),
      supaya PATH yang rusak tidak mematikan fitur UAC dan toggle PATH.
- [ ] Kartu "System check" di Settings: status PowerShell, fitur yang terdampak, dan langkah
      perbaikan (`sfc /scannow`, `DISM … /RestoreHealth`).
- [ ] Jangka panjang (v4): ganti PowerShell dengan Win32 API native (`ShellExecuteExW` untuk UAC,
      registry, junction, Service Control Manager, `WinVerifyTrust`).
