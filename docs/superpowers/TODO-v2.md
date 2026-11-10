# Kuro Mute v2 (Rust): status & to-do

Spec: [specs/2026-11-09-rust-rewrite-design.md](specs/2026-11-09-rust-rewrite-design.md)
Branch: `main` (bagian 1–2e udah di-merge) · Crate: `kuromute/`
Terakhir di-update: 2026-11-10

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

## Bagian 2: Jendela settings ✅ (dites manual user 2026-11-09, `v2-settings` di-merge ke `main` 2026-11-10)

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

## Bagian 2b: Border glow + pulse ✅

Rencana kerja 2b–2e: [plans/2026-11-10-v2-indicators-extras.md](plans/2026-11-10-v2-indicators-extras.md) (7 task)

- [x] Sample gaya disetujui user (2026-11-10): https://claude.ai/code/artifact/c176bc3d-1397-44c1-8ed1-a7b97ac54045 (pilih "Glow + pulse awal")
- [x] Keputusan: jadi pilihan Gaya di tab Penanda (Solid / Glow), default Glow
- [x] Glow: strip dilebarin ke dalam, bitmap gradasi alpha per-pixel via `UpdateLayeredWindow` (renderer popup), dirender sekali per `rebuild`
- [x] Pulse: 3 kedip (~1,6 dtk) waktu baru mute, cuma ganti alpha global lewat `UpdateLayeredWindow`, timer mati setelahnya. Jalur show tetap cuma buka cloak (latency nggak berubah)
- [x] Config `border_style` + default untuk config lama, dropdown di Penanda, i18n EN/ID
- [x] Ukur ulang latency dan RAM (2026-11-10, exe release 14 MB, border Glow): core RAM 3,49 MB (baseline 2,44 MB), latency run baru p50 2 ms / p95 3 ms, n=20 (baseline p50 4 ms / p95 12 ms; semua sampel log p50 2 / p95 9, n=280). GDI 14 / USER 21 sebelum dan sesudah 20 toggle, nggak bocor. (popup off di config user; jalur clock belum terukur)

Tes manual user: belum

## Bagian 2c: Peringatan "ngomong pas lagi mute" ✅

Spike 2026-11-10 (`examples/meter_spike.rs`):
- Mute endpoint mic fisik (PD400X) → meter dan sample capture dua-duanya 0,000. Jadi kalau yang di-mute mic fisik, fitur ini nggak bisa.
- Setup user: yang di-mute **Wave Link MicrophoneFX** (virtual), PD400X tetap hidup. Meter PD400X (`IAudioMeterInformation`, tanpa buka stream sendiri, ngandelin stream Wave Link) tetap kebaca: diam 0,000–0,005, ngomong 0,01–0,13. ✅

Rencana:
- [x] Setting di tab Mic: nyalain fitur + pilih "mic sumber" (mic fisik yang dibaca meternya), default mati
- [x] Selama mute: poll `GetPeakValue` ±100 ms; di atas ambang (awal ±0,015) selama ≥300 ms → pulse border + popup lagi, opsional beep; cooldown ±10 dtk. Timer mati waktu unmute
- [x] Kalau nggak ada app lain yang buka stream di mic sumber, meter 0 → fitur diam (tulis di keterangan UI)
- [x] Ambang/sensitivitas: konstanta 0,015, ditinjau lagi setelah dipakai harian

Tes manual user: belum

## Bagian 2d: Timer durasi mute di popup ✅

Permintaan user (2026-11-10): popup nampilin udah berapa lama mute, tampil terus selama mute, reset tiap mute baru.
- [x] Popup: "Mic mute · 0:42" (m:ss, lewat sejam h:mm:ss). Render ulang bitmap popup 1x/detik selama mute dan popup kelihatan; timer mati waktu unmute
- [x] Tampil terus = opsi "Tetap tampil selama mute" yang udah ada
- [x] Mulai dihitung dari event mute (termasuk mute dari luar app); unmute lalu mute lagi = reset ke 0:00

Tes manual user: belum

## Bagian 2e: Support dev di tab Umum ✅

- [x] Bagian "Dukung developer" di tab Umum, 2 tombol lewat `app::open_url` (sama kayak Report Bug):
  - Saweria: https://saweria.co/Kurohiko
  - Top Up Games: https://kurohikotopup.com
- [x] Teks EN/ID

## Bagian 2f: Popup Pill + ganti nama jadi Sirah Mute ✅ (2026-11-10)

- [x] Gaya popup Card / Pill (dropdown Gaya di Popup). Pill: ikon mic dicoret merah, "Mic mute", jam di kanan
- [x] Dukung developer dipindah ke atas Updates & support
- [x] Ganti nama: `sirahmute.exe`, data `%APPDATA%\SirahMute` (config.json disalin sekali dari `MuteMic`), Run value `SirahMute` (`MuteMic` dihapus), link update/issue ke `Kurohiko-id/sirah-mute`
- [x] Logo "Belah" (sirah = merah, bahasa Minang): kapsul mic merah terbelah miring 45°, aset di `kuromute/assets/`. Tray: kapsul abu-abu = unmute, merah = mute, badge kuning = mic hilang
- [ ] Lockup logo + tulisan "Sirah Mute" (butuh font OFL di-outline), dikerjain bareng banner
- [x] Repo baru https://github.com/Kurohiko-id/sirah-mute (2026-11-10): file v1 C# dihapus dari main, README EN + `README.id.md`, banner `assets/banner.png` (sumber `assets/banner.html`), remote `origin` = repo baru, `legacy` = repo lama
- [x] Repo lama: README pindah, rilis v1.3.0 "moved" (app v1 dapat notif update ke sana), di-archive
- [ ] Upload `assets/social-preview.png` manual di Settings → Social preview repo baru (GitHub nggak punya API-nya)

Tes manual user: belum (cek ikon mic dicoret di Pill)

## Bagian 3: Distribusi ✅ (2026-11-10: draft v2.0.0 dibuat CI)

- [x] Installer per-user `installer/sirah-mute.iss` (Inno Setup, AppId baru `B131B4B2-…`, ke `%LOCALAPPDATA%\Programs\SirahMute`): matikan Sirah Mute lama, `Kuro MuteMic.exe`, `Kuro MuteMic Lite.exe`, watcher Super Lite (PID dari `%TEMP%\KuroForceAudio.pid`, difilter `powershell.exe`), shortcut Start Menu (+ Settings), saran uninstall v1. Autostart diurus app sendiri; uninstall hapus Run `SirahMute`, settings di `%APPDATA%\SirahMute` dibiarkan. Compile lokal OK (6,7 MB)
- [x] Zip portable (isi `sirahmute.exe`)
- [x] GitHub Actions `.github/workflows/release.yml`: push tag `v*` → cek tag = versi Cargo, test, build, ISCC (choco), zip, **draft** release
- [x] Tes installer manual user: timpa live install, F13 aman, uninstall bersih (dicek), halaman Finish pakai tombol besar Support/Top Up + panel kiri Sirah
- [x] Tag `v2.0.0` di-push, CI hijau (5 mnt), draft berisi installer + zip, notes EN/ID; README diupdate
- [x] Mode Notify sekarang popup sendiri (tembus Do Not Disturb): Switch back / Keep for now / Remind in 15 min (`src/prompt.rs`), dites user OK; tag v2.0.0 dibuat ulang, CI hijau
- [x] Fix: jendela Settings rusak ukurannya waktu di-drag antar monitor beda skala (125%/100%), terutama ke monitor kiri. winit naruh jendela pakai rumus kursornya sendiri sehingga DPI ganti dua kali; sekarang SetWindowPos winit dibelokkan ke rect saran Windows + koreksi ukuran 50 ms (`settings/ui.rs`). Dites drag otomatis dua arah: ukuran benar. Tag v2.0.0 dibuat ulang di bffbbbb, CI hijau
- [ ] User klik **Publish** di draft, lalu post promo `assets/promo/`
- [x] Gambar promo 1:1 di `assets/promo/` (4 slide EN/ID, slide 4 = guard), **lokal saja** (di-.gitignore, nggak ke GitHub); post setelah rilis ada

## Nanti (setelah inti stabil)

- [ ] Plugin Stream Deck
- [ ] Tombol khusus di mouse NOIR S1 (kirim F13 dari firmware atau macro)

## Catatan kecil yang ditunda (polish)

- Pilihan ikon mute: sekarang U+E74F (speaker dicoret) seperti app lama. Bisa ganti ke ikon mic dicoret.
- Pas mic dicabut, timer jeda guard bisa jalan 1 tick lebih awal. Usul: pakai 301000 ms.
- Belum ada guard `exiting`: kalau `TaskbarCreated` datang setelah Exit, ikon tray bisa nongol lagi sebentar.
- Kalau cek update manual gagal, nggak ada notifikasi ke user. Kalau `Audio::new` gagal waktu login, app keluar tanpa pesan.
- Popup muncul ulang tiap layar berubah (ganti resolusi atau resume) waktu lagi mute.
