# Kuro Mute v2 — Bagian 2: Jendela settings

Induk: [2026-11-09-rust-rewrite-design.md](2026-11-09-rust-rewrite-design.md) (§6.9 Settings UI, §6.10 data,
§6.5 hotkey). Dokumen ini nambah detail; kalau bentrok, dokumen ini yang menang untuk settings.

## 1. Intent

Ganti stub Notepad (`--settings` di `main.rs`) dengan jendela settings beneran supaya v2 punya fitur
setara app lama dan bisa dirilis. User: "jarang buka" — sekali setup (pilih mic, set F13) lalu lupa.
Settings lama tidak ada yang bikin kesel; targetnya **pindahin semua fitur ke tampilan baru**, bukan
redesign.

Prioritas: ringan dan jelas > cepat diakses atau fancy.

## 2. Goals / non-goals / anti-goals

**Goals**
- Semua field `Config` bisa diatur dari UI (lihat §5), plus aksi: tes mute, cek update, buka folder log,
  report bug.
- Perubahan langsung kepake (auto-save + reload core), tanpa tombol Save.
- Rekam tombol hotkey jalan untuk F13 yang dikirim mouse/macro.

**Non-goals** (keputusan Claude, disetujui user di chat: "gas")
- Tema terang, animasi, preview border live, ikon tray di settings.
- Resize jendela.

**Anti-goals**
- Core (proses tray) jadi lebih berat saat main game. Target spec induk tetap: core RAM < 10 MB,
  hotkey→indikator p95 < 16 ms. Ukur ulang setelah Slint masuk binary.
  - Core **tidak boleh** inisialisasi Slint; semua kode UI di modul `settings` yang cuma dipanggil
    dari cabang `--settings`.
  - Kalau satu exe ternyata bikin core lewat target, fallback yang boleh: pecah jadi exe settings
    terpisah (`kuromute-settings.exe`) di folder yang sama. (Keputusan Claude.)
- Settings crash/hang bikin mute ikut mati — settings tetap proses terpisah.
- Menulis `settings.json` lama (tetap read-only).

## 3. Proses & IPC

- Satu exe. `kuromute.exe --settings` = proses settings; keluar saat jendela ditutup. Core sudah spawn
  ini (`App::open_settings`) dan sudah cegah spawn dobel dari core.
- **Satu jendela saja:** kalau `--settings` dijalankan saat settings lain sudah buka (mis. dari
  shortcut manual), bawa jendela yang ada ke depan lalu keluar. (Keputusan Claude.)
- **Simpan:** settings tulis `config.json` lewat `config::save` (atomic, sudah ada), lalu kirim
  `KuroMute.Reload` ke window core (`KuroMuteCore`) pakai **`SendMessageTimeoutW`** (timeout ~2 s),
  bukan post. Core balikin hasil: `0` = ok, `1` = registrasi hotkey gagal / invalid. Settings tampilkan
  error merah di tab Mic kalau `1`. (Butuh perubahan kecil di `App::handle`/`reload` supaya return
  value-nya diisi dari `hotkey_err`.)
- Core tidak jalan (window core tidak ketemu) → tetap simpan, tampilkan baris info "Kuro Mute tidak
  jalan; perubahan dipakai saat start berikutnya". (Keputusan Claude.)
- Core ada tapi timeout / sibuk (mis. menu tray lagi terbuka dan pesan ditunda) → **bukan error**:
  config sudah tersimpan, core tetap reload saat pesannya diproses. Settings tidak tampilkan apa-apa
  (status hotkey dianggap belum diketahui). Kode balasan: `0` ok, `1` hotkey gagal; selain itu/timeout
  = tidak diketahui. (Keputusan Claude.)
- **Rekam tombol:** pesan baru `KuroMute.PauseHotkey` (wparam 1 = jeda/unregister, 0 = daftar ulang).
  Settings kirim 1 saat mode rekam mulai, 0 saat selesai/batal/jendela ditutup. Core juga daftar ulang
  sendiri kalau proses settings keluar (anak proses `settings_child` sudah dilacak) supaya hotkey tidak
  hilang kalau settings crash di tengah rekam.
- **Tes mute:** settings post `KuroMute.Toggle` (sudah ada).
- Data live (daftar device, monitor) di-enumerate sendiri oleh proses settings pakai modul yang sudah
  ada (`audio::Audio::list`, `monitors::enumerate`). Refresh saat tab dibuka; tidak perlu live-update.

## 4. Tampilan

- Slint, tema gelap gaya Windows 11 (fluent dark). Sidebar kiri: **Mic / Penanda / Device / Umum**.
- Layout B (mockup yang dipilih user): sidebar sempit (~92 px kalau di mockup, boleh lebih lebar) berisi
  ikon + nama kategori, item aktif di-highlight. Konten kanan: judul section kecil (huruf kapital abu),
  lalu **satu kolom baris kartu** — label di kiri, kontrol (dropdown/toggle/tombol/badge tombol hotkey)
  rata kanan. Tab panjang dipecah jadi beberapa section dengan judul. Urutan kontrol = urutan di §5.
  Tidak ada kartu dua kolom.
- Lisensi Slint: pakai **Royalty-free License** (gratis untuk app desktop, syarat atribusi). Atribusi
  ditaruh di tab Umum (widget `AboutSlint` atau teks "Made with Slint") + README. (Keputusan Claude;
  repo belum punya file LICENSE.)
- Ukuran tetap ~760×520 logical px, DPI-aware, tidak bisa resize, bisa digeser. Ikon jendela = ikon exe.
- Bahasa ikut `Config.language` (EN/ID, `Auto` = bahasa Windows), ganti langsung saat diubah. String
  settings masuk tabel `i18n` yang sama dengan core.
- Jendela muncul di tengah monitor tempat kursor berada, dapat fokus normal (ini satu-satunya jendela
  v2 yang boleh ambil fokus).

## 5. Isi tab

Semua field merujuk `config::Config`.

**Mic**
- Dropdown mic (`mic_id`), daftar input aktif. Mic tersimpan tapi tidak terpasang → tampil sebagai
  "(tidak terhubung)" dan tetap terpilih.
- Rekam di level Win32, bukan event keyboard Slint (Slint tidak kasih VK code dan tidak andal untuk
  F13–F24/media/Win). Selama mode rekam saja, proses settings pasang **Raw Input**
  (`RIDEV_INPUTSINK`) di hidden window miliknya dan ambil VK + modifier dari situ; dilepas setelah
  selesai. Low-level hook (`WH_KEYBOARD_LL`) tetap dilarang, juga di proses settings. Tombol yang tidak
  bisa direkam tetap bisa dipilih lewat dropdown. Builder: buktikan dulu (spike kecil) bahwa F13 dari
  macro dan satu media key masuk lewat Raw Input sebelum bikin UI lainnya. (Keputusan Claude.)
- Hotkey (`hotkey`): tombol **Rekam** (tekan kombinasi; Esc = batal) **dan** dropdown manual berisi
  `hotkey::SELECTABLE_KEYS` (+ modifier Ctrl/Alt/Shift/Win sebagai checkbox).
  Tampil pakai `hotkey::display`.
- Validasi lokal `hotkey::validate` sebelum simpan (tolak F12 tunggal dll.); invalid → error merah,
  tidak disimpan. Lolos lokal tapi core balas gagal → error merah "dipakai app lain", config tetap
  tersimpan (core sudah pegang `hotkey_err` + balloon, perilaku sama dengan bagian 1).
- `hotkey::leaks_into_games` true → peringatan kuning non-blocking + saran F13–F24.
- Tombol **Tes mute**.

**Penanda**
- Popup: on/off (`popup_enabled`), posisi 6 pilihan (`popup_position`), skala 0.5–2.0
  (`popup_scale`, slider step 0.1), tetap tampil selama mute (`popup_persist_while_muted`).
- Border: on/off (`border_enabled`), tebal 1–30 px (`border_thickness`).
- Monitor: "Semua monitor" (`all_monitors`) atau checklist monitor (`monitor_ids`). ID monitor yang
  tersimpan tapi sudah tidak ada **tetap disimpan** (monitor bisa dicolok lagi). List kosong boleh —
  `monitors::select` sudah punya fallback. (Keputusan Claude.)
- Bunyi mute/unmute (`sound_enabled`).

**Device**
- Default input (`default_input_id`) dan output (`default_output_id`): dropdown, ada pilihan
  "Jangan diubah" (= `None`).
- Paksa saat startup (`force_on_startup`).
- Jaga device: aktif (`guard_enabled`) + mode Off/Notify/AutoRevert (`guard_mode`).

**Umum**
- Bahasa (`language`), autostart (`autostart`).
- Versi app, tombol **Cek update** (pakai `update::fetch_latest`; hasil ditampilkan inline di settings),
  **Buka folder log**, **Report bug** (URL GitHub issue prefilled sama dengan tray).

## 6. Auto-save

- Tiap kontrol berubah → simpan + reload. Slider/angka di-debounce ~300 ms supaya tidak spam reload.
- Hotkey hanya disimpan setelah rekam/pilih selesai dan lolos validasi lokal.
- Settings baca config sekali saat buka. Kalau core menulis config saat settings buka (mis. toggle "Jaga
  device" dari tray), tulisan settings berikutnya menang — diterima, karena jarang. (Keputusan Claude.)
- Gagal tulis file → error inline, jangan tutup jendela.

## 7. Yang diserahkan ke builder

- Backend/renderer Slint (saran: software renderer kalau cukup ringan), struktur file `.slint`.
- Cara bridging Slint ↔ `Config` (callback per field atau model tunggal).
- Detail visual (spasi, ikon sidebar), selama gaya Win11 dark dan layout B.
- Cara bring-to-front jendela settings yang sudah ada.
- Test: logika non-UI (validasi, mapping config ↔ state UI, debounce) wajib punya unit test; UI dites
  manual + checklist.

## 8. Selesai kalau

- Semua field §5 bisa diubah dari UI dan core langsung pakai tanpa restart.
- Rekam F13 dari mouse/macro berhasil; hotkey core kembali aktif setelah rekam/tutup/crash settings.
- Stub Notepad dihapus.
- Core RAM & latency masih memenuhi target bagian 1 (ukur pakai `tools/measure.ps1`).
