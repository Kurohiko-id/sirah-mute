# Kuro Mute v2 Indicator Extras Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Before merging `v2-settings`, add four features:
- 2b: a Glow border style with an opening pulse
- 2d: a mute duration clock in the popup
- 2c: a talk-while-muted warning
- 2e: support-the-developer links

**Architecture:** Every indicator change stays inside `overlay.rs` (`Indicators`). Core timers on the existing hidden window drive them. The latency-critical show path stays as it is today: un-cloak the strips, then `UpdateLayeredWindow` with a pre-rendered popup bitmap. Pulses, clock ticks and meter polling run on `WM_TIMER`, after the measured path, and only while muted. Pure logic lives in plain functions with unit tests: glow falloff, pulse curve, clock text and talk detection.

**Tech Stack:** Rust 2024, `windows` 0.62.2, Slint 1.18.1 (settings only). No new dependencies.

**Spec:** [../TODO-v2.md](../TODO-v2.md), sections "Bagian 2b" to "Bagian 2e" (decisions and spike results). Approved style sample: https://claude.ai/code/artifact/c176bc3d-1397-44c1-8ed1-a7b97ac54045 ("Glow + pulse awal").

## Global Constraints

- Work on branch `v2-settings` and run all commands from `kuromute/`.
- Core RAM < 10 MB and hotkey-to-indicator p95 < 16 ms, measured with `tools/measure.ps1`. Baseline: 2.44 MB, p50 4 ms / p95 12 ms.
- Add nothing to the mute hot path (`on_mute` → `Indicators::show` → `log_latency`): no rendering, no COM calls, no allocations bigger than today's. Start timers there and do the work in `WM_TIMER`.
- Old `config.json` files must still load. Every new field gets a default through the struct's `#[serde(default)]`, and new enums use `deserialize_with = "or_default"`.
- All UI and popup text goes through `i18n::Key` with EN and ID. Update `Key::ALL` and its length.
- Settings code stays under `src/settings/` and `ui/`. The core never touches Slint.
- One commit per task, with no AI attribution lines.

## Review Focus

1. **Unmute in the middle of a pulse.** The strips hide and stay hidden. The next mute starts at full alpha, not wherever the pulse stopped. Test: `stop_pulse_resets_alpha` (Task 3).
2. **Display change or settings reload while muted.** The popup clock keeps counting from the original mute and does not reset to 0:00. Test: `clock_survives_non_fresh_show` (Task 4).
3. **The warning's source mic is unplugged, unset, or the same device as the muted mic.** No crash, no warning and no log spam. Test: `silence_never_fires` on a constant 0.0 meter (Task 5).
4. **Constant background noise just above the threshold.** At most one warning per 10 s, not one per 300 ms. Test: `talk_fires_once_per_cooldown` (Task 5).
5. **Mutes longer than an hour.** The clock text switches to `h:mm:ss` and still fits the popup. Test: `fmt_elapsed(3600) == "1:00:00"` (Task 4).

---

### Task 1: Support links in the Umum tab (2e)

**Files:**
- Modify: `src/i18n.rs` (keys, strings, `Key::ALL`)
- Modify: `src/settings/ui.rs` (`apply_lang`)
- Modify: `src/settings/umum.rs`
- Modify: `ui/settings.slint` (`Strings` global, tab 3, callbacks)

**Interfaces:**
- Produces:
  - `umum::SAWERIA_URL: &str`
  - `umum::TOPUP_URL: &str`
  - Slint callbacks `support-saweria-clicked()` and `support-topup-clicked()`

- [ ] **Step 1: Add the keys.** Add these `Key` variants with this exact copy:

| Key | EN | ID |
|---|---|---|
| `SecSupport` | Support the developer | Dukung developer |
| `SupportSaweria` | Buy me a coffee | Traktir kopi |
| `SupportSaweriaBtn` | Saweria | Saweria |
| `SupportTopup` | Top up your games | Top up game kamu |
| `SupportTopupBtn` | KurohikoTopUp.com | KurohikoTopUp.com |

- [ ] **Step 2: Run** `cargo test all_keys_non_empty`. Expected: PASS. It will not compile until `Key::ALL` lists the new keys.

- [ ] **Step 3: Wire up the UI.**
  - In `umum.rs`, add `pub const SAWERIA_URL: &str = "https://saweria.co/Kurohiko";` and `pub const TOPUP_URL: &str = "https://kurohikotopup.com";`.
  - Each button calls `crate::app::open_url` with its constant.
  - In the slint file, add `SectionTitle { text: Strings.sec-support; }` and two `Row`s (label plus `Button`). Place them after the Report Bug row and before the "Made with Slint" note.

- [ ] **Step 4: Check it by hand.** Run `cargo build`, open `--settings` and click both buttons. Expected: each one opens the right page in the browser. Switching language updates all five texts.

- [ ] **Step 5: Commit** `feat(settings): support links in general tab`

---

### Task 2: Glow border style (2b, part 1)

**Files:**
- Modify: `src/config.rs` (enum, field, tests)
- Modify: `src/app.rs:68` (`diff`)
- Modify: `src/overlay.rs` (border creation in `rebuild`)
- Modify: `src/i18n.rs`, `src/settings/ui.rs`, `src/settings/penanda.rs`, `ui/settings.slint`

**Interfaces:**
- Produces:
  - `config::BorderStyle { Solid, #[default] Glow }`
  - `Config.border_style: BorderStyle`, using `or_default`
  - `overlay::glow_extent(t: u32) -> u32`
  - `overlay::glow_alpha(d: u32, t: u32) -> u8`
  - `Indicators.glow: bool`, true when the strips were built with per-pixel alpha

- [ ] **Step 1: Write the failing tests**

```rust
// config.rs
#[test] fn border_style_defaults_to_glow_for_old_configs() {
    let c: Config = serde_json::from_str(r#"{"border_thickness":6}"#).unwrap();
    assert_eq!(c.border_style, BorderStyle::Glow);
    let c: Config = serde_json::from_str(r#"{"border_style":"Neon"}"#).unwrap();
    assert_eq!(c.border_style, BorderStyle::Glow);
}
// app.rs
#[test] fn diff_border_style_only_indicators() { /* same shape as diff_border_thickness_only_indicators */ }
// overlay.rs
#[test] fn glow_falloff() {
    assert_eq!(glow_extent(6), 30);   // 5t
    assert_eq!(glow_extent(30), 96);  // capped
    assert_eq!(glow_alpha(0, 6), 255);
    assert_eq!(glow_alpha(5, 6), 255);
    assert!((110..=140).contains(&glow_alpha(6, 6)));
    assert_eq!(glow_alpha(glow_extent(6), 6), 0);
    for d in 6..30 { assert!(glow_alpha(d + 1, 6) <= glow_alpha(d, 6)); }
}
```

- [ ] **Step 2: Run** `cargo test glow` and `cargo test border_style`. Expected: compile errors or FAIL.

- [ ] **Step 3: Implement the glow.**
  - `glow_extent(t) = (5 * t).min(96).max(t)`.
  - `glow_alpha(d, t)` returns 255 while `d < t` (the solid core).
  - Past the core it falls off quadratically from 128 to 0 over the remaining `extent - t` px: `128 * (1 - x)^2` with `x = (d - t) / (extent - t)`.
  - In `rebuild`, when `border_style == Glow`, build the strips with `border_strips(m.rect, glow_extent(t))`.
  - For each strip, create a 32-bit top-down DIB, using the same `BITMAPINFO` pattern as `Renderer::popup`.
  - Fill every pixel with premultiplied `#E5484D` at `glow_alpha(d, t)`. Here `d` is the pixel's distance to the nearest monitor edge in screen coordinates: `min(x - mon.left, mon.right - 1 - x, y - mon.top, mon.bottom - 1 - y)`. This makes the corners join with no seam.
  - Push the bitmap once with `UpdateLayeredWindow(…, ULW_ALPHA)`, then delete the DIB and the DC.
  - Solid keeps today's `SetLayeredWindowAttributes` path. Cloak, show and hide logic does not change.

- [ ] **Step 4: Add the settings UI.**
  - New keys: `BorderStyle` ("Style" / "Gaya"), `BorderSolid` ("Solid" / "Solid"), `BorderGlow` ("Glow" / "Glow").
  - Add a `Strings.border-styles` model.
  - Add a ComboBox row under the thickness row, visible when `pen-border-enabled`. Bind it to `pen-border-style-index` and fire `pen-changed()`.
  - In `penanda.rs`, use `const STYLES: [BorderStyle; 2] = [Solid, Glow]` in both `show` and `on_pen_changed`.

- [ ] **Step 5: Test.**
  - Run `cargo test`. Expected: all green.
  - By hand: enable the border, mute, and compare Glow and Solid at thickness 6 and 30.
  - Expected for Glow: it looks like the sample, with a solid red edge, a soft fade inward and seamless corners. It stays click-through.
  - Expected for Solid: unchanged from today.

- [ ] **Step 6: Commit** `feat(overlay): glow border style`

---

### Task 3: Opening pulse (2b, part 2)

**Files:**
- Modify: `src/overlay.rs`
- Modify: `src/app.rs` (timer dispatch, `on_mute`, `rewatch`, `rebuild_indicators`)

**Interfaces:**
- Consumes: `Indicators.glow` (Task 2).
- Produces:
  - `overlay::TIMER_PULSE: usize = 6`
  - `overlay::pulse_alpha(elapsed_ms: u32) -> Option<u8>`, where `None` means finished
  - `Indicators::show(&mut self, owner: HWND, muted: bool, fresh: bool)`. The new `fresh` parameter means "a new mute or unmute event". `rebuild_indicators` passes `false`.
  - `Indicators::pulse(&mut self, owner: HWND)`, which starts or restarts a pulse. Task 5 uses it.
  - `Indicators::tick_pulse(&mut self, owner: HWND)`
  - `Indicators.pulse_start: Option<Instant>` and `Indicators.alpha: u8` (the current strip alpha, 255 when idle)

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn pulse_curve() {
    assert_eq!(pulse_alpha(0), Some(255));
    assert!(pulse_alpha(275).unwrap() < 90);   // trough of blink 1
    assert_eq!(pulse_alpha(550), Some(255));
    assert!(pulse_alpha(1375).unwrap() < 90);  // trough of blink 3
    assert_eq!(pulse_alpha(1650), None);
}
#[test] fn stop_pulse_resets_alpha() {
    // Indicators::new() with no windows: start a pulse, run the hide path,
    // assert pulse_start == None and the stored alpha == 255.
}
```

- [ ] **Step 2: Run** `cargo test pulse`. Expected: FAIL.

- [ ] **Step 3: Implement the pulse.**
  - The curve is 3 blinks of 550 ms each: `255 - 191 * (1 - cos(2π·t/550)) / 2`. That puts the floor at 64.
  - `pulse()` stores `pulse_start: Option<Instant>` and calls `SetTimer(owner, TIMER_PULSE, 33)`.
  - The app kills each timer on `WM_TIMER`, so `tick_pulse` re-arms it every tick. Each tick sets every strip's global alpha from the curve. When the curve returns `None`, it sets 255 and stops.
  - Glow strips set alpha with a blend-only update: `UpdateLayeredWindow(h, None, None, None, None, None, COLORREF(0), Some(&blend), ULW_ALPHA)` with `SourceConstantAlpha = a`.
  - Solid strips use `SetLayeredWindowAttributes(h, COLORREF(0), a, LWA_ALPHA)`.
  - `show(owner, true, fresh = true)` calls `pulse()`, but only when `glow` is true.
  - Hiding the strips (`set_strips(false)`) clears `pulse_start`, kills the timer and restores alpha 255.
  - In `app.rs`, `on_mute` and `rewatch` pass `fresh = true`, and `rebuild_indicators` passes `false`. Dispatch `TIMER_PULSE => self.ind.tick_pulse(self.hwnd)`.

- [ ] **Step 4: Test.**
  - Run `cargo test`. Expected: all green.
  - By hand, with Glow: a mute gives 3 blinks in about 1.6 s, then the border stays solid.
  - Unmute in the middle of a pulse. Expected: the border hides cleanly and the next mute starts bright.

- [ ] **Step 5: Commit** `feat(overlay): pulse glow border on mute`

---

### Task 4: Mute duration clock in the popup (2d)

**Files:**
- Modify: `src/overlay.rs` (`Popup`, `Renderer`, `Indicators`)
- Modify: `src/app.rs` (timer dispatch, `TIMER_POPUP_HIDE` handler)

**Interfaces:**
- Consumes: `Indicators::show(owner, muted, fresh)` (Task 3).
- Produces:
  - `overlay::TIMER_MUTE_CLOCK: usize = 7`
  - `overlay::fmt_elapsed(secs: u64) -> String`
  - `Indicators::tick_clock(&mut self, owner: HWND)`
  - `Indicators.muted_since: Option<Instant>`. It is not cleared by `rebuild` or `destroy`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn elapsed_text() {
    assert_eq!(fmt_elapsed(0), "0:00");
    assert_eq!(fmt_elapsed(42), "0:42");
    assert_eq!(fmt_elapsed(3599), "59:59");
    assert_eq!(fmt_elapsed(3600), "1:00:00");
    assert_eq!(fmt_elapsed(36125), "10:02:05");
}
#[test] fn clock_survives_non_fresh_show() {
    // Indicators with no windows:
    // show(HWND::default(), true, true) sets muted_since;
    // show(.., true, false) keeps the same value;
    // show(.., false, true) clears it.
}
```

- [ ] **Step 2: Run** `cargo test elapsed_text` and `cargo test clock_survives`. Expected: FAIL.

- [ ] **Step 3: Implement the clock.**
  - Muted popup text: `format!("{} · {}", t(lang, Key::Muted), fmt_elapsed(s))`. The unmuted text stays `t(lang, Key::Unmuted)`.
  - Keep the `Renderer` (`Option<Renderer>`) and `lang` in `Indicators` after `rebuild`.
  - Each `Popup` keeps what it needs to redraw: both text formats, `icon_h`, `w`/`h`, the corner radius and `bmi`.
  - Split the draw loop into `Popup::draw(&mut self, r: &Renderer, slot: usize, glyph: &str, text: &str) -> Result<()>`.
  - Bitmaps become `[HBITMAP; 3]`:

    | Slot | Content | Drawn |
    |---|---|---|
    | 0 | unmuted | in `rebuild` |
    | 1 | muted "· 0:00" | in `rebuild` |
    | 2 | muted, live time | on every tick |

  - Hot path: `show(…, true, fresh = true)` sets `muted_since = Some(now)` and shows slot 1, so nothing is rendered.
  - A non-fresh show (after a rebuild) draws slot 2 with the current elapsed time and shows it.
  - While muted with the popup visible, call `SetTimer(owner, TIMER_MUTE_CLOCK, 1000)`.
  - `tick_clock` draws slot 2, re-shows each popup and re-arms. It stops once the mic is unmuted or the popup is hidden.
  - The `TIMER_POPUP_HIDE` handler also kills the clock. With "Tetap tampil selama mute" on, the popup stays up and keeps ticking.
  - `show(…, false, _)` clears `muted_since`.

- [ ] **Step 4: Test.**
  - Run `cargo test`. Expected: all green.
  - By hand with "Tetap tampil selama mute" on:
    - The popup counts 0:00, 0:01, and so on.
    - Unmute and mute again. Expected: the count restarts at 0:00.
    - While muted, change the popup scale in settings. Expected: the count continues from where it was.
  - With it off, the popup shows "· 0:00" for 1.5 s and then hides.

- [ ] **Step 5: Commit** `feat(overlay): mute duration clock in popup`

---

### Task 5: Talk-while-muted detection in the core (2c, part 1)

**Files:**
- Create: `src/talk.rs` (the detector, pure logic)
- Modify: `src/main.rs` (`mod talk;`), `src/config.rs`, `src/audio.rs`, `src/app.rs`

**Interfaces:**
- Consumes:
  - `Indicators::pulse(owner)` from Task 3.
  - A popup re-show. Add `Indicators::flash_popup(&mut self, owner: HWND)`: it draws slot 2 now, shows the muted popup, and re-arms the hide timer unless persist is on.
- Produces:
  - `Config.talk_warn_enabled: bool`, default false
  - `Config.talk_warn_mic_id: Option<String>`, default None
  - `Changes.talk: bool`
  - `talk::TalkDetector::new() -> Self`
  - `TalkDetector::feed(&mut self, peak: f32, now: Instant) -> bool`, where `true` means warn now
  - `TalkDetector::reset(&mut self)`
  - `audio::Audio::meter(&self, id: &str) -> Option<IAudioMeterInformation>`, `None` if the device is absent or inactive
  - `app::TIMER_TALK: usize = 8`

- [ ] **Step 1: Write the failing tests.** They go in `talk.rs`, except the config and diff tests, which go in `config.rs` and `app.rs`. Constants: `THRESHOLD = 0.015`, `HOLD_MS = 300`, `COOLDOWN_MS = 10_000`. Tests feed one sample every 100 ms.

```rust
// helper: feeds `peak` every 100 ms from from_ms to to_ms, returns the ms values where feed() == true
fn run(d: &mut TalkDetector, t0: Instant, from_ms: u64, to_ms: u64, peak: f32) -> Vec<u64>
#[test] fn silence_never_fires() { /* 0.0 and 0.005 for 60 s -> [] */ }
#[test] fn short_blip_ignored() { /* 0.2 for 200 ms, then 0.0 -> [] */ }
#[test] fn sustained_talk_fires_after_hold() { /* 0.05 from 0 -> first fire at 300 ms */ }
#[test] fn talk_fires_once_per_cooldown() { /* 0.05 for 25 s -> fires at 300, ~10300, ~20300 */ }
#[test] fn reset_clears_hold_and_cooldown() { /* fire, reset, talk again -> fires after 300 ms */ }
#[test] fn talk_settings_flag_talk_only() { /* app.rs diff: talk_warn_mic_id change -> Changes { talk: true, .. } */ }
#[test] fn old_config_has_talk_warn_off() { /* config.rs: {} -> enabled false, mic None */ }
```

- [ ] **Step 2: Run** `cargo test talk`. Expected: FAIL.

- [ ] **Step 3: Implement the detector.**
  - It tracks `above_since: Option<Instant>` and `last_fire: Option<Instant>`.
  - A sample below `THRESHOLD` clears `above_since`.
  - It fires once the level has stayed above the threshold for at least `HOLD_MS` and the cooldown has passed. Firing also clears `above_since`.

- [ ] **Step 4: Add `Audio::meter`.** Get the device with `self.device(id)`, check it is `DEVICE_STATE_ACTIVE`, then call `Activate::<IAudioMeterInformation>`.

- [ ] **Step 5: Wire it into `app.rs`.**
  - `State` gains `talk_meter: Option<IAudioMeterInformation>` and `talk: TalkDetector`.
  - Add `fn update_talk_watch(&mut self)`. The watch is on only when the mic is muted, `talk_warn_enabled` is true and `talk_warn_mic_id` is set. Then it activates the meter, resets the detector and arms `TIMER_TALK` at 100 ms. Otherwise it drops the meter and kills the timer.
  - Call `update_talk_watch` in these places:
    - at the end of `on_mute`, after `log_latency`
    - in `rewatch`
    - on a reload where `ch.talk` is true
    - in the `TIMER_DEVICES` handler
  - The `TIMER_TALK` handler reads `GetPeakValue()`. An error means the device is gone, so drop the meter without logging.
  - When `feed` returns true, do these in order:
    1. `ind.pulse`
    2. `ind.flash_popup`
    3. `sound::play(true)` if `sound_enabled` is on
    4. log `talk: warned`
  - Re-arm the timer while the mic is still muted.

- [ ] **Step 6: Run** `cargo test`. Expected: all green.

- [ ] **Step 7: Commit** `feat(core): warn when talking while muted`

---

### Task 6: Talk warning settings in the Mic tab (2c, part 2)

**Files:**
- Modify: `src/i18n.rs`, `src/settings/ui.rs`, `src/settings/mic.rs`, `ui/settings.slint`

**Interfaces:**
- Consumes:
  - `Config.talk_warn_enabled` and `Config.talk_warn_mic_id` (Task 5)
  - the existing `mic::device_choices`
- Produces:
  - Slint properties `talk-enabled: bool`, `talk-mic-names: [string]`, `talk-mic-index: int`
  - Slint callbacks `talk-changed()` and `talk-mic-selected(int)`

- [ ] **Step 1: Add the keys and copy**

| Key | EN | ID |
|---|---|---|
| `SecTalkWarn` | Talking while muted | Ngomong pas lagi mute |
| `TalkWarnEnabled` | Warn me when I talk while muted | Ingatkan kalau ngomong pas mute |
| `TalkWarnSource` | Physical mic to listen to | Mic fisik yang dipantau |
| `TalkWarnNote` | Pick your real mic, not the one you mute. Works only while another app (e.g. Wave Link) is using that mic. | Pilih mic asli, bukan mic yang di-mute. Cuma jalan kalau ada app lain (mis. Wave Link) yang lagi pakai mic itu. |

- [ ] **Step 2: Build the UI.**
  - In the slint file, add the new section between Hotkey and Test:
    - a CheckBox row
    - a ComboBox row for the source mic, shown only when enabled
    - a dim `Note` with `TalkWarnNote`, also shown only when enabled
  - In `mic.rs`, build a second list with `device_choices(sink, Flow::Capture, cfg.talk_warn_mic_id.as_ref(), lang)`. Give it its own `ids` cell and have the existing `refresh` closure re-read it too.
  - Picking a device sets `talk_warn_mic_id` and commits. Toggling the checkbox sets `talk_warn_enabled` and commits.

- [ ] **Step 3: Run** `cargo test all_keys_non_empty`, then `cargo build`.

- [ ] **Step 4: Test by hand on the user's setup.**
  - Settings: muted mic = Wave Link MicrophoneFX, source = PD400X, warning on.
  - Mute and stay silent for 30 s. Expected: no warning.
  - Talk for 1 s. Expected: a pulse, the popup, and a beep if sound is on.
  - Keep talking. Expected: the next warning comes about 10 s later.
  - Unmute. Expected: no more warnings, and the log has no `talk:` lines while unmuted.
  - Set the source to the muted mic itself. Expected: no warnings.

- [ ] **Step 5: Commit** `feat(settings): talk warning options in mic tab`

---

### Task 7: Measure and update status

**Files:**
- Modify: `docs/superpowers/TODO-v2.md`

- [ ] **Step 1: Measure.**
  - Run `cargo build --release`.
  - Deploy the same way as v2-settings Task 11, backing up the live exe first.
  - With the border on in Glow style, run `powershell -File tools/measure.ps1`.
  - Expected: RAM < 10 MB and p95 < 16 ms. Record both next to the baseline (2.44 MB, p95 12 ms).

- [ ] **Step 2: Check memory if it grew.** If RAM rose by more than about 2 MB per monitor, make sure the glow DIBs are freed after `UpdateLayeredWindow` and that only one `Renderer` exists.

- [ ] **Step 3: Update TODO-v2.md.** Tick 2b, 2c, 2d and 2e and add the measured numbers. For 2c, record the threshold decision as "constant 0.015, revisit after daily use".

- [ ] **Step 4: Commit** `docs: mark indicator extras done with measurements`
