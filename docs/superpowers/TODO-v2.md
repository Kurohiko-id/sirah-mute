# Kuro Mute v2 (Rust): status & to-do

Spec: [specs/2026-11-09-rust-rewrite-design.md](specs/2026-11-09-rust-rewrite-design.md)
Branch: `main` (bagian 1 udah di-merge) · Crate: `kuromute/`
Terakhir di-update: 2026-11-09

## Bagian 1: Core tray ✅ (selesai, dipakai harian)

Rencana kerja: [plans/2026-11-09-v2-core-tray.md](plans/2026-11-09-v2-core-tray.md)

- [x] Config dan migrasi dari `settings.json` lama. File lama cuma dibaca, nggak pernah ditulis.
- [x] Log yang dirotasi, plus catat crash (panic hook)
- [x] Hotkey bebas, termasuk F13–F24 dan tombol media, dengan `MOD_NOREPEAT`
- [x] Bahasa EN/ID
- [x] Popup dan border per monitor, DPI aware
- [x] Jaga device: AutoRevert dengan batas 5 kali per 60 detik, lalu dijeda 5 menit
- [x] Cek update dari GitHub
- [x] Bunyi beep mute/unmute (opsional)
- [x] Core Audio: mute, set device default untuk 3 role, `--list-devices`
- [x] Tray: ikon dengan badge status, menu, notifikasi balloon, autostart, ikon exe
- [x] Wiring app: satu instance, `--toggle`, `--settings` (sementara pakai Notepad)
- [x] Script pengukur `kuromute/tools/measure.ps1`
- [x] Fix hasil review akhir: app nyangkut pas ditutup, debounce event device, indikator salah pas mic dicabut
- [x] Optimasi latency: border di-cloak lewat DWM, ada fallback
- [x] Fix app nggak keluar waktu `taskkill` (strip border nerusin `WM_CLOSE`)

Hasil tes langsung di PC user (2026-11-09):

| Target | Hasil |
|---|---|
| RAM < 10 MB | 2,3 MB ✅ |
| Hotkey sampai indikator, p95 < 16 ms | p50 6 ms, p95 15 ms ✅ |
| Migrasi settings | ✅ |
| Jaga device dan saklar tray | ✅ |
| Cabut-colok mic | ✅ |
| Exit dari tray | ✅ |
| GDI/USER nggak bocor | ✅ |

- [ ] **Tes di game** (borderless dan fullscreen): cek hotkey nggak bikin stutter dan border kelihatan. Nunggu user.
- [x] Merge `v2-rust` ke `main` (2026-11-09)

## Bagian 2: Jendela settings ✅ (selesai di branch `v2-settings`, dites manual user 2026-11-09)

- [x] Brainstorm detail → desain: [specs/2026-11-09-v2-settings-design.md](specs/2026-11-09-v2-settings-design.md)
- [x] Mockup klik-able disetujui user (2026-11-09): https://claude.ai/code/artifact/05e262d6-77f3-43e4-a208-b3d6f618b5c9
- [x] Tulis plan: [plans/2026-11-09-v2-settings.md](plans/2026-11-09-v2-settings.md) (11 task, belum dieksekusi)
- [x] Spike Raw Input (2026-11-09): F13 dari keyboard dan mouse masuk (`vk=0x7C`), **tapi cuma kalau core lagi nggak daftarin F13** (RegisterHotKey nelen tombolnya), jadi jeda hotkey saat rekam wajib. F24 tidak dites (user nggak punya). Play/Pause nggak masuk lewat koleksi keyboard (jalur consumer HID), jadi tombol media dipilih lewat dropdown, bukan Rekam.
- [x] Mode `--settings` pakai Slint, layout B: sidebar Mic / Penanda / Device / Umum, tema gelap
- [x] Mic: pilih device, rekam tombol, dropdown F13–F24 (`hotkey::SELECTABLE_KEYS`), peringatan kalau tombol bisa bocor ke game, tes mute
- [x] Penanda: popup (posisi, skala, tetap tampil), border (tebal 1–30), pilih monitor, bunyi
- [x] Device: input/output default, paksa saat startup, mode jaga (Off/Notify/AutoRevert)
- [x] Umum: bahasa, autostart, cek update, versi, buka folder log, report bug
- [x] Simpan otomatis lalu `KuroMute.Reload`. Ganti stub Notepad.
- [x] Ukur (2026-11-09, `tools/measure.ps1`, exe release 14 MB): core RAM 2,44 MB, latency p50 4 ms / p95 12 ms. Tetap satu exe.
- [ ] Polish ditunda: error merah tetap bahasa lama sampai simpan berikutnya; tombol media cuma lewat dropdown (Rekam tidak tangkap consumer HID); ukuran exe bisa dikecilkan (profil release lto/strip).

## Bagian 2b: Border glow + pulse ⏳ (sebelum merge `v2-settings`)

Rencana kerja 2b–2e: [plans/2026-11-10-v2-indicators-extras.md](plans/2026-11-10-v2-indicators-extras.md) (7 task)

- [x] Sample gaya disetujui user (2026-11-10): https://claude.ai/code/artifact/c176bc3d-1397-44c1-8ed1-a7b97ac54045 (pilih "Glow + pulse awal")
- [x] Keputusan: jadi pilihan Gaya di tab Penanda (Solid / Glow), default Glow
- [ ] Glow: strip dilebarin ke dalam, bitmap gradasi alpha per-pixel via `UpdateLayeredWindow` (renderer popup), dirender sekali per `rebuild`
- [ ] Pulse: 3 kedip (~1,6 dtk) waktu baru mute, cuma ganti alpha global lewat `UpdateLayeredWindow`, timer mati setelahnya. Jalur show tetap cuma buka cloak (latency nggak berubah)
- [ ] Config `border_style` + default untuk config lama, dropdown di Penanda, i18n EN/ID
- [ ] Ukur ulang latency dan RAM

## Bagian 2c: Peringatan "ngomong pas lagi mute" ⏳

Spike 2026-11-10 (`examples/meter_spike.rs`):
- Mute endpoint mic fisik (PD400X) → meter dan sample capture dua-duanya 0,000. Jadi kalau yang di-mute mic fisik, fitur ini nggak bisa.
- Setup user: yang di-mute **Wave Link MicrophoneFX** (virtual), PD400X tetap hidup. Meter PD400X (`IAudioMeterInformation`, tanpa buka stream sendiri, ngandelin stream Wave Link) tetap kebaca: diam 0,000–0,005, ngomong 0,01–0,13. ✅

Rencana:
- [ ] Setting di tab Mic: nyalain fitur + pilih "mic sumber" (mic fisik yang dibaca meternya), default mati
- [ ] Selama mute: poll `GetPeakValue` ±100 ms; di atas ambang (awal ±0,015) selama ≥300 ms → pulse border + popup lagi, opsional beep; cooldown ±10 dtk. Timer mati waktu unmute
- [ ] Kalau nggak ada app lain yang buka stream di mic sumber, meter 0 → fitur diam (tulis di keterangan UI)
- [ ] Ambang/sensitivitas: slider atau cukup konstanta, putuskan setelah tes harian

## Bagian 2d: Timer durasi mute di popup ⏳

Permintaan user (2026-11-10): popup nampilin udah berapa lama mute, tampil terus selama mute, reset tiap mute baru.
- [ ] Popup: "Mic mute · 0:42" (m:ss, lewat sejam h:mm:ss). Render ulang bitmap popup 1x/detik selama mute dan popup kelihatan; timer mati waktu unmute
- [ ] Tampil terus = opsi "Tetap tampil selama mute" yang udah ada
- [ ] Mulai dihitung dari event mute (termasuk mute dari luar app); unmute lalu mute lagi = reset ke 0:00

## Bagian 2e: Support dev di tab Umum ⏳

- [ ] Bagian "Dukung developer" di tab Umum, 2 tombol lewat `app::open_url` (sama kayak Report Bug):
  - Saweria: https://saweria.co/Kurohiko
  - Top Up Games: https://kurohikotopup.com
- [ ] Teks EN/ID

## Bagian 3: Distribusi ⏳ (belum mulai)

- [ ] Installer per-user (Inno Setup, AppId baru): matikan app lama dan watcher Super Lite, ambil alih Run key, shortcut Start Menu
- [ ] Zip portable
- [ ] GitHub Actions: build x64, upload installer dan zip ke Release
- [ ] README: cara lewati SmartScreen (rilis tanpa code signing)

## Nanti (setelah inti stabil)

- [ ] Plugin Stream Deck
- [ ] Tombol khusus di mouse NOIR S1 (kirim F13 dari firmware atau macro)

## Catatan kecil yang ditunda (polish)

- Pilihan ikon mute: sekarang U+E74F (speaker dicoret) seperti app lama. Bisa ganti ke ikon mic dicoret.
- Pas mic dicabut, timer jeda guard bisa jalan 1 tick lebih awal. Usul: pakai 301000 ms.
- Belum ada guard `exiting`: kalau `TaskbarCreated` datang setelah Exit, ikon tray bisa nongol lagi sebentar.
- Kalau cek update manual gagal, nggak ada notifikasi ke user. Kalau `Audio::new` gagal waktu login, app keluar tanpa pesan.
- Popup muncul ulang tiap layar berubah (ganti resolusi atau resume) waktu lagi mute.
