# Kuro Mute & Force Mic — Rust rewrite (v2) design

Status: draft, menunggu approval Kurohiko. Tanggal: 2026-11-09.

## 1. Intent

Kuro Mute & Force Mic adalah tray app Windows untuk gamer/streamer: satu tombol untuk mute/unmute mic
pilihan user di level device (semua app yang pakai mic itu ikut mute), indikator visual saat mute, dan
"kunci" default input/output supaya Windows/game/app lain tidak seenaknya mengganti device.

Versi sekarang (C# .NET 8, tiga edisi: Full WPF, Lite WinForms, Super Lite PowerShell) secara fungsi
sehat, tapi:
- kadang tidak stabil saat main game (penyebab paling mungkin: hotkey `Ctrl+F12` — Ctrl bocor ke game,
  F12 key reserved, game bisa mematikan hotkey app lain via `RIDEV_NOHOTKEYS`);
- berat untuk ukuran fungsinya (Lite ~58 MB private memory);
- .NET 8 EOL 2026-11-10;
- beberapa bug audit (lihat §7).

User (owner) minta **rewrite total**, bebas pakai Rust, dengan tujuan: **lebih stabil, lebih ringan,
lebih responsif.**

## 2. Goals

1. **Feature parity penuh** dengan app lama (§4). Ini syarat utama dari user.
2. Stabil: tidak crash/hang, recover sendiri dari sleep/resume, cabut-colok device, restart Explorer,
   perubahan monitor/DPI.
3. Ringan: target idle private memory < 10 MB untuk proses tray.
4. Responsif: tekan tombol → border/popup tampil < 16 ms (baseline sekarang p50 11 ms, p95 54 ms).
5. Satu exe menggantikan Full, Lite, dan Super Lite.
6. Trigger mute bisa tombol apa saja termasuk F13–F24 dan tombol multimedia, tanpa wajib modifier —
   supaya nanti tombol mouse NOIR S1 bisa di-assign F13 dll. tanpa kerja ulang.

## 3. Non-goals (v1) dan anti-goals

Non-goals v1 (ditunda, bukan dibuang):
- Plugin Stream Deck.
- Reverse-engineer / program tombol mouse NOIR S1.
- Integrasi Wave Link API (mute per-mix). Tidak dibutuhkan: mute di device sumber sudah memute semuanya.
- Code signing. User memutuskan **tanpa tanda tangan**; README menjelaskan cara melewati SmartScreen.

Anti-goals (gagal walau "jalan"):
- **Ada fitur app lama yang hilang.** Kata user: "klu fiturnya g sebanyak app yg lama" → mending yang lama.
- Hardcode device apa pun. Mic yang di-mute dan device default yang dikunci **dipilih user** di settings
  (setup user sekarang: mute PD400X, kunci Wave Link MicrophoneFX + System — itu data, bukan desain).
- Butuh admin rights, telemetry, atau koneksi network selain cek update GitHub.
- Tidak boleh mengganggu game: tidak ada window yang ambil fokus, tidak ada hook yang bisa bikin input lag.

## 4. Feature inventory (wajib parity)

Dari `AppSettings.cs`, `TrayApp.cs`, `OverlayForm.cs`, `BorderGlowForm.cs`, `DeviceChangeToast.cs`,
`super-lite/ForceAudio.ps1`:

| Fitur | Perilaku lama | v2 |
|---|---|---|
| Pilih mic yang di-mute | dropdown capture device, `MicDeviceId` | sama; tampilkan device yang sedang tidak ada sebagai "(tidak terhubung)" tanpa menghapus pilihan |
| Toggle mute | global hotkey (default Ctrl+Alt+M) toggle mute di device terpilih | sama; key apa saja termasuk F13–F24 / media key, modifier opsional, `MOD_NOREPEAT` |
| Hotkey ditekan tapi mic belum dipilih | buka settings | sama |
| Popup mute (OSD) | 220×100 rounded dark, ikon mic + "Microphone Muted/Unmuted", auto-hide 1.5 s, tidak ambil fokus, click-through | sama |
| Posisi popup | TopLeft/TopCenter/TopRight/BottomLeft/BottomCenter/BottomRight, margin 24 px dari working area | sama |
| Skala popup | 0.5–2.0 | sama |
| Popup tetap tampil selama muted | `OverlayPersistWhileMuted` | sama |
| Border glow | garis di tepi layar saat muted, tebal bisa diatur | sama; range tebal **satu** untuk semua (1–30) |
| Monitor target | semua monitor, atau pilih monitor tertentu (fallback primary) | sama; identitas monitor yang stabil lintas reboot |
| Re-sync saat setting berubah | kalau mic sedang muted, indikator langsung tampil | sama, plus sync saat startup |
| Default input/output | pilih device, set sebagai default **dan** default communications | sama (semua role: console, multimedia, communications) |
| Paksa saat startup | `ForceAudioOnStartup` | sama, tapi **hanya saat app start**, bukan setiap kali setting diubah (bug lama) |
| Jaga device | lama (Full/Lite): toast "changed from your pick" + tombol "Set Default Again"; Super Lite: auto-revert | mode **Off / Notify / AutoRevert**, **default AutoRevert** (keinginan user). Notify = perilaku Full/Lite lama |
| Autostart | HKCU Run "MuteMic" | sama; path selalu ke exe yang terinstall |
| Cek update | GitHub Releases saat start + manual dari menu | sama |
| Tray menu | Settings…, Check for Updates, Report Bug… (buka GitHub issue prefilled versi+OS), Exit | sama |

Tambahan v2 (kecil, bukan fitur baru besar):
- Bunyi pendek saat mute/unmute, opsional (default off), di tab Penanda — satu-satunya indikator yang
  tetap jalan di game fullscreen exclusive. Diputar ke default output, non-blocking. (Keputusan Claude.)
- Tray icon berubah sesuai status mute (muted / unmuted / mic tidak terhubung) + tooltip.
- Klik kiri tray = buka settings.

## 5. Keputusan user

- Bahasa: Rust ("pake rust jg gpp").
- Rewrite total, biaya bukan masalah.
- Mute di level device yang **user pilih**; device default yang dikunci juga **user pilih**.
- Jaga device: **langsung balikin otomatis** (AutoRevert) sebagai default.
- Saklar "Jaga device" on/off di tray (buat saat pakai headset).
- Bahasa: English + Indonesia, bisa dipilih.
- Settings UI: **layout B** — sidebar kategori kiri ala Settings Windows 11: **Mic, Penanda, Device, Umum**.
- Distribusi: installer + update check via GitHub seperti sekarang; **tanpa code signing**.
- Stream Deck plugin & mouse NOIR: nanti, setelah inti stabil.

## 6. Arsitektur (keputusan Claude — boleh diganti user)

### 6.1 Proses
- **Satu exe** `kuromute.exe`, dua mode:
  - default: **core tray process** — hotkey, audio, enforcer, overlay, tray. Tidak memuat UI framework.
  - `--settings`: **settings process** — window settings, dijalankan on-demand oleh core, exit saat ditutup.
- Single instance via named mutex (per user session). Instance kedua tanpa argumen → minta instance
  pertama buka settings, lalu exit.
- Komunikasi core ↔ settings: settings menulis file settings (atomic), lalu memberi sinyal ke core
  (mis. `WM_COPYDATA` / registered window message ke hidden window core); core reload & apply diff.
  Settings juga butuh data live dari core (daftar device, status mute, monitor) — boleh enumerate sendiri
  via Core Audio; tidak perlu IPC kompleks. Mekanisme tepatnya diserahkan ke builder.
- Alasan: proses tray yang jalan saat main game tetap kecil; crash di UI tidak menjatuhkan mute.

### 6.2 Core thread model
- Satu thread UI (message loop Win32) memiliki **semua state**.
- Callback COM Core Audio (`IMMNotificationClient`, `IAudioEndpointVolumeCallback`) **hanya**
  `PostMessage` ke hidden window; tidak menyentuh state.
- Operasi yang bisa blok (set default device, cek update WinHTTP) jalan di worker thread, hasil di-post balik.
- Set mute dilakukan via `IAudioEndpointVolume::SetMute` langsung — tidak menunggu konfirmasi (sumber
  freeze 1 detik di versi lama).

### 6.3 Audio
- `windows` crate: `IMMDeviceEnumerator`, `IMMNotificationClient`, `IAudioEndpointVolume` + callback.
- Set default: `IPolicyConfig` (undocumented; CLSID 870AF99C-171D-4F9E-AF0D-E63DF40C2BC9,
  IID F8679F50-850A-41CF-9C72-430F290290C8 — sudah dipakai di `super-lite/ForceAudio.ps1`) untuk role
  eConsole, eMultimedia, eCommunications.
- Device diidentifikasi dengan endpoint ID string Windows (`{0.0.1.00000000}.{guid}` capture,
  `{0.0.0.00000000}.{guid}` render).
- Mic hilang lalu muncul lagi → re-attach volume callback otomatis, sync indikator.

### 6.4 Enforcer (jaga device)
- Trigger: `OnDefaultDeviceChanged` (+ device added/state changed untuk kasus device kunci baru muncul).
- AutoRevert: debounce (~300 ms) lalu set ulang yang drift; **rate limit** (mis. maks N revert per menit)
  — kalau ada app lain yang terus melawan, berhenti revert sementara dan beri notifikasi sekali, supaya
  tidak terjadi ping-pong. Angka pastinya diserahkan ke builder.
- Notify: notifikasi dengan aksi "Set Default Again".
- **Saklar cepat di tray: "Jaga device" on/off** (keputusan user). Dipakai saat user sendiri mau pindah
  device, mis. pakai headset sesekali. Status tersimpan (tidak nyala sendiri) dan terlihat di tooltip/menu.
  Saat dinyalakan lagi, langsung cek & revert sekali.
- Mengecek semua role, bukan cuma console (bug lama).

### 6.5 Trigger / hotkey
- `RegisterHotKey` dengan `MOD_NOREPEAT`, tanpa modifier diperbolehkan. User bebas ganti kapan saja.
- Input tombol di settings: **rekam** (tekan tombolnya — juga menangkap F13 yang dikirim mouse/macro)
  **dan** dropdown pilih manual (berisi F13–F24, media key, dll.), karena keyboard biasa tidak punya
  tombol F13–F24 fisik.
- Validasi saat simpan: tolak F12 tunggal (reserved debugger); tampilkan error kalau registrasi gagal
  (sudah dipakai app lain) di settings dan tray tooltip — bukan cuma log.
- Fallback yang tahan `RIDEV_NOHOTKEYS` (Raw Input `RIDEV_INPUTSINK` di hidden window) **opsional** —
  builder boleh tambahkan kalau murah; keputusan akhir setelah test di game user.
- Low-level keyboard/mouse hook **tidak** dipakai di v1 (risiko timeout & input lag).

### 6.6 Indikator (overlay & border)
- Layered window per monitor, `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOPMOST`,
  tidak pernah ambil fokus.
- Popup: render sekali per state ke bitmap (Direct2D/DirectWrite atau GDI — builder pilih), tampil via
  `UpdateLayeredWindow`. Rounded corner via alpha, bukan Region.
- Border: 4 strip tipis per monitor (bukan window fullscreen transparan) supaya murah untuk compositor.
- Windows dibuat sekali, show/hide saja saat toggle; rebuild hanya saat setting/monitor/DPI berubah
  (`WM_DISPLAYCHANGE`, `WM_DPICHANGED`).
- Per-Monitor V2 DPI aware (user punya 3 monitor mixed DPI: 2560×1440@125% + 2× 1920×1080@100%).

### 6.7 Tray
- `Shell_NotifyIconW`, icon berubah per state, re-add saat `TaskbarCreated` (Explorer restart).
- Menu: Settings…, Jaga device ✓ (toggle), Check for Updates, Report Bug…, Exit.

### 6.7b Bahasa
- **English + Indonesia** (keputusan user), bisa dipilih di Umum; default ikut bahasa Windows
  (Indonesia kalau UI language `id`, selain itu English). Berlaku untuk tray, popup, notifikasi, settings.
  String disimpan di satu tabel per bahasa yang dipakai kedua proses.

### 6.8 Notifikasi
- Ganti `DeviceChangeToast` custom (TopMost, tidak bisa di-dismiss — bug lama) dengan Windows toast
  notification (menghormati Focus Assist / mode game). Butuh AppUserModelID + shortcut Start Menu
  dari installer. Kalau toast tidak tersedia (portable run), fallback ke balloon tray.

### 6.9 Settings UI (proses `--settings`)
- Layout B: sidebar kiri Mic / Penanda / Device / Umum; tampilan dark, gaya Windows 11.
  - **Mic:** pilih mic, set tombol (rekam key), tes mute.
  - **Penanda:** popup on/off, posisi, skala, persist saat muted; border on/off, tebal; monitor target.
  - **Device:** default input, default output, paksa saat startup, mode jaga (Off/Notify/AutoRevert).
  - **Umum:** bahasa, autostart, cek update, versi, buka folder log, report bug.
- Autosave seperti sekarang (perubahan langsung diterapkan).
- Toolkit: saran Claude **Slint** (fluent style, native Rust, ringan); builder boleh ganti kalau ada
  alasan kuat (mis. ukuran binary), selama layout B dan ringan tetap terpenuhi.

### 6.10 Settings & data
- Lokasi: `%APPDATA%\MuteMic\config.json` (file **baru**). `settings.json` lama hanya **dibaca** sekali
  untuk migrasi dan tidak pernah ditulis — supaya rollback ke app C# lama tetap aman (app lama
  diam-diam reset ke default kalau formatnya asing). (Keputusan Claude.)
- Hotkey lama (mis. `Ctrl+F12`) dimigrasi apa adanya, tapi settings menampilkan peringatan non-blocking
  bahwa kombinasi Ctrl/F12 bisa bocor ke game dan menyarankan F13–F24. (Keputusan Claude.)
- Save atomic (tulis temp → flush → rename). Load gagal → backup file rusak ke `settings.json.bad`,
  log, pakai default, beritahu user sekali.
- **Migrasi** dari format lama (field: `MicDeviceId`, `DefaultInputId`, `DefaultOutputId` = GUID;
  `HotkeyModifiers`/`HotkeyKey` = uint Win32 VK/MOD; `ShowToastOverlay`, `ShowBorderGlow`,
  `OverlayPersistWhileMuted`, `AutoStart`, `ForceAudioOnStartup`, `WatchAudioChanges` = bool;
  `OverlayPosition` = enum string/int; `OverlayScale` = double; `BorderThickness` = int;
  `OverlayAllMonitors` = bool; `OverlayMonitorIds` = list nama `\\.\DISPLAYn`).
  GUID → endpoint ID: capture `{0.0.1.00000000}.{guid}`, render `{0.0.0.00000000}.{guid}`.
  `WatchAudioChanges=false` → mode Off, `true` → AutoRevert (user sekarang `true` dan minta auto);
  user baru → default AutoRevert. Tambah field `version` untuk migrasi berikutnya.

### 6.11 Logging & crash
- Log di `%APPDATA%\MuteMic\logs\`, timestamp ISO 8601 lengkap dengan tanggal, rotasi (mis. 1 MB × 3),
  ditulis di luar thread UI.
- Panic hook: tulis panic + backtrace ke log sebelum exit.
- Tetap catat latency hotkey → indikator (untuk membandingkan dengan baseline).

### 6.12 Update check
- WinHTTP ke GitHub Releases API saat start (non-blocking) dan manual. Hanya memberi tahu + buka halaman
  release; tidak auto-download.

### 6.13 Distribusi
- Installer per-user (tanpa admin) — Inno Setup boleh dipertahankan, **AppId baru** (bukan upgrade
  in-place dari `6A9F4C1E-...`, karena installer lama bisa per-machine). Installer: hentikan proses lama
  (`Kuro MuteMic.exe`, Lite, dan watcher PowerShell Super Lite — yang terakhir wajib, karena kalau jalan
  bareng AutoRevert v2 akan ping-pong), pasang exe, shortcut Start Menu (untuk toast AUMID), ambil alih
  Run key "MuteMic". **Tidak menghapus file** edisi lama (Lite user jalan dari folder dev); selesai
  install, kasih tahu user bahwa versi lama bisa di-uninstall manual dari Apps. (Keputusan Claude.)
- Juga zip portable.
- GitHub Actions: build release x64 (ARM64 opsional), lampirkan installer + zip ke Release.
- README: cara lewati SmartScreen ("More info → Run anyway").

## 7. Bug lama yang harus tidak terulang
- Tidak ada single instance.
- Toast device tidak bisa di-dismiss, Alt+F4 men-dispose-nya.
- Error tidak pernah dicatat; log tanpa tanggal dan tanpa rotasi (10.9 MB).
- Save settings tidak atomic; load gagal diam-diam reset ke default.
- Hotkey tanpa validasi / `MOD_NOREPEAT`; gagal registrasi hanya di log.
- Range tebal border beda antar edisi (16 vs 30).
- Paksa device diulang setiap kali setting berubah.
- Autostart menunjuk folder dev / publish.
- Cek drift hanya role console.
- Super Lite membunuh PID dari file (bisa PID yang sudah dipakai proses lain).

## 8. Acceptance / cara ukur
- Idle private memory proses tray < 10 MB (Task Manager / `Get-Process`), setelah 1 jam.
- Latency tekan tombol → indikator tampil: p95 < 16 ms dari log.
- Tidak ada handle/GDI leak setelah 1000× toggle dan 50× buka-tutup settings.
- Skenario manual: sleep/resume; cabut-colok mic; matikan/nyalakan monitor; ubah DPI; restart Explorer;
  game borderless (popup/border harus terlihat) + fullscreen exclusive (cukup: tombol jalan, mic ke-mute,
  tidak crash; overlay memang tidak bisa tampil di atas exclusive fullscreen tanpa hook ke game, dan itu
  di luar scope — keputusan Claude); app lain mengganti default device (AutoRevert balik, tanpa ping-pong).
- Migrasi: settings.json user sekarang terbaca utuh tanpa set ulang.

## 9. Diserahkan ke builder
- Struktur crate/modul, library helper kecil (serde, logging).
- Renderer overlay (Direct2D vs GDI), mekanisme IPC detail, angka debounce/rate limit.
- Apakah Raw Input fallback dibuat di v1.
- Desain visual detail settings dalam batas layout B dan tema dark.
- Nasib source C# lama di repo (pindah ke folder `legacy/` atau dihapus setelah v2 rilis).
