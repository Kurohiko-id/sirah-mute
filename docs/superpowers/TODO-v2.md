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

## Bagian 2: Jendela settings ⏳ (belum mulai, wajib sebelum rilis)

- [ ] Brainstorm detail, lalu tulis plan (`plans/…-v2-settings.md`)
- [ ] Mode `--settings` pakai Slint, layout B: sidebar Mic / Penanda / Device / Umum, tema gelap
- [ ] Mic: pilih device, rekam tombol, dropdown F13–F24 (`hotkey::SELECTABLE_KEYS`), peringatan kalau tombol bisa bocor ke game, tes mute
- [ ] Penanda: popup (posisi, skala, tetap tampil), border (tebal 1–30), pilih monitor, bunyi
- [ ] Device: input/output default, paksa saat startup, mode jaga (Off/Notify/AutoRevert)
- [ ] Umum: bahasa, autostart, cek update, versi, buka folder log, report bug
- [ ] Simpan otomatis lalu `KuroMute.Reload`. Ganti stub Notepad.

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
