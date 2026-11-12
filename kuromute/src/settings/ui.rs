use super::save::{self, CoreReply, SaveOutcome, StatusKey};
use crate::config::{self, Config};
use crate::i18n::{self, Key, ResolvedLang};
use crate::msg::{MSG_RELOAD, WINDOW_CLASS};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, RegisterWindowMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
};
use windows::core::PCWSTR;

slint::include_modules!();

/// State shared by the settings tabs.
pub struct State {
    /// What the controls show and edit.
    pub cfg: Config,
    /// The config this process last loaded or saved; `commit` writes only keys where `cfg`
    /// differs from it, so changes the core saved meanwhile survive.
    base: Config,
    /// Re-shows controls after a save pulled in values from disk. The core itself only changes
    /// `guard_enabled` (tray menu), which lives on the Device tab.
    pub reshow: Option<Rc<dyn Fn()>>,
}

/// Read-only read of config.json; never renames a bad file. If it can't be read, asks the core
/// to reload once (the core backs a corrupt file up and re-saves its own config), then retries.
fn read_config() -> Result<Option<Config>, String> {
    let path = config::config_path();
    config::read(&path).or_else(|_| {
        send_to_core(MSG_RELOAD, 0, 0);
        config::read(&path)
    })
}

fn load_state() -> State {
    let cfg = read_config().unwrap_or_else(|e| {
        crate::log!("settings: config not readable at startup ({e}); showing defaults");
        None
    });
    let cfg = cfg.unwrap_or_default();
    State {
        base: cfg.clone(),
        cfg,
        reshow: None,
    }
}

/// (Re)applies every UI string for `lang`; the Umum tab calls this on language change.
pub fn apply_lang(w: &MainWindow, lang: ResolvedLang) {
    let s = w.global::<Strings>();
    s.set_tab_mic(i18n::t(lang, Key::TabMic).into());
    s.set_tab_others(i18n::t(lang, Key::TabOthers).into());
    s.set_others_add(i18n::t(lang, Key::OthersAdd).into());
    s.set_others_remove(i18n::t(lang, Key::OthersRemove).into());
    s.set_others_note(i18n::t(lang, Key::OthersNote).into());
    s.set_others_position(i18n::t(lang, Key::OthersPosition).into());
    s.set_tab_indicator(i18n::t(lang, Key::TabIndicator).into());
    s.set_tab_device(i18n::t(lang, Key::TabDevice).into());
    s.set_tab_general(i18n::t(lang, Key::TabGeneral).into());
    s.set_placeholder(i18n::t(lang, Key::TabPlaceholder).into());
    s.set_sec_mic(i18n::t(lang, Key::SecMic).into());
    s.set_mic_label(i18n::t(lang, Key::MicLabel).into());
    s.set_sec_hotkey(i18n::t(lang, Key::SecHotkey).into());
    s.set_hotkey_label(i18n::t(lang, Key::HotkeyLabel).into());
    s.set_record(i18n::t(lang, Key::Record).into());
    s.set_recording(i18n::t(lang, Key::Recording).into());
    s.set_hotkey_key(i18n::t(lang, Key::PickLabel).into());
    s.set_hotkey_mods(i18n::t(lang, Key::HotkeyMods).into());
    s.set_hotkey_leaks(i18n::t(lang, Key::HotkeyLeaks).into());
    s.set_sec_talk_warn(i18n::t(lang, Key::SecTalkWarn).into());
    s.set_talk_warn_enabled(i18n::t(lang, Key::TalkWarnEnabled).into());
    s.set_talk_warn_source(i18n::t(lang, Key::TalkWarnSource).into());
    s.set_talk_warn_note(i18n::t(lang, Key::TalkWarnNote).into());
    s.set_cough_hold(i18n::t(lang, Key::CoughHold).into());
    s.set_cough_note(i18n::t(lang, Key::CoughNote).into());
    s.set_sec_test(i18n::t(lang, Key::SecTest).into());
    s.set_test_label(i18n::t(lang, Key::TestLabel).into());
    s.set_test_button(i18n::t(lang, Key::TestButton).into());
    s.set_sec_popup(i18n::t(lang, Key::SecPopup).into());
    s.set_popup_enabled(i18n::t(lang, Key::PopupEnabled).into());
    s.set_popup_position(i18n::t(lang, Key::PopupPosition).into());
    s.set_popup_scale(i18n::t(lang, Key::PopupScale).into());
    s.set_popup_persist(i18n::t(lang, Key::PopupPersist).into());
    s.set_pill_click_hint(i18n::t(lang, Key::PillClickHint).into());
    s.set_popup_styles(super::mic::model(
        [Key::PopupCard, Key::PopupPill]
            .iter()
            .map(|&k| i18n::t(lang, k).to_string())
            .collect(),
    ));
    s.set_sec_border(i18n::t(lang, Key::SecBorder).into());
    s.set_border_enabled(i18n::t(lang, Key::BorderEnabled).into());
    s.set_border_thickness(i18n::t(lang, Key::BorderThickness).into());
    s.set_border_style(i18n::t(lang, Key::BorderStyle).into());
    s.set_border_styles(super::mic::model(
        [Key::BorderSolid, Key::BorderGlow]
            .iter()
            .map(|&k| i18n::t(lang, k).to_string())
            .collect(),
    ));
    s.set_sec_monitors(i18n::t(lang, Key::SecMonitors).into());
    s.set_all_monitors(i18n::t(lang, Key::AllMonitors).into());
    s.set_sec_sound(i18n::t(lang, Key::SecSound).into());
    s.set_sound_enabled(i18n::t(lang, Key::SoundEnabled).into());
    s.set_positions(super::mic::model(
        [
            Key::PosTopLeft,
            Key::PosTopCenter,
            Key::PosTopRight,
            Key::PosBottomLeft,
            Key::PosBottomCenter,
            Key::PosBottomRight,
        ]
        .iter()
        .map(|&k| i18n::t(lang, k).to_string())
        .collect(),
    ));
    s.set_sec_defaults(i18n::t(lang, Key::SecDefaults).into());
    s.set_default_input(i18n::t(lang, Key::DefaultInput).into());
    s.set_default_output(i18n::t(lang, Key::DefaultOutput).into());
    s.set_force_on_startup(i18n::t(lang, Key::ForceOnStartup).into());
    s.set_sec_guard(i18n::t(lang, Key::SecGuard).into());
    s.set_guard_enabled(i18n::t(lang, Key::GuardEnabled).into());
    s.set_guard_mode_label(i18n::t(lang, Key::GuardModeLabel).into());
    s.set_sec_general(i18n::t(lang, Key::SecGeneral).into());
    s.set_lang_label(i18n::t(lang, Key::LangLabel).into());
    s.set_lang_options(super::mic::model(vec![
        i18n::t(lang, Key::LangAuto).to_string(),
        "English".into(),
        "Indonesia".into(),
    ]));
    s.set_autostart_label(i18n::t(lang, Key::AutostartLabel).into());
    s.set_version_label(i18n::t(lang, Key::VersionLabel).into());
    s.set_sec_updates(i18n::t(lang, Key::SecUpdates).into());
    s.set_check_update(i18n::t(lang, Key::CheckUpdateBtn).into());
    s.set_open_release(i18n::t(lang, Key::OpenRelease).into());
    s.set_logs_label(i18n::t(lang, Key::LogsLabel).into());
    s.set_logs_btn(i18n::t(lang, Key::LogsBtn).into());
    s.set_bug_label(i18n::t(lang, Key::BugLabel).into());
    s.set_bug_btn(i18n::t(lang, Key::BugBtn).into());
    s.set_sec_support(i18n::t(lang, Key::SecSupport).into());
    s.set_support_saweria(i18n::t(lang, Key::SupportSaweria).into());
    s.set_support_saweria_btn(i18n::t(lang, Key::SupportSaweriaBtn).into());
    s.set_support_topup(i18n::t(lang, Key::SupportTopup).into());
    s.set_support_topup_btn(i18n::t(lang, Key::SupportTopupBtn).into());
    s.set_made_with_slint(i18n::t(lang, Key::MadeWithSlint).into());
    s.set_guard_modes(super::mic::model(
        [Key::GuardOff, Key::GuardNotify, Key::GuardAutoRevert]
            .iter()
            .map(|&k| i18n::t(lang, k).to_string())
            .collect(),
    ));
}

/// `SendMessageTimeoutW` to the core window: `Some((result, timed_out))`, `None` = core not running.
pub fn send_to_core(name: PCWSTR, wp: usize, lp: isize) -> Option<(isize, bool)> {
    unsafe {
        let h = FindWindowW(WINDOW_CLASS, PCWSTR::null())
            .ok()
            .filter(|h| !h.is_invalid())?;
        let mut res = 0usize;
        let r = SendMessageTimeoutW(
            h,
            RegisterWindowMessageW(name),
            WPARAM(wp),
            LPARAM(lp),
            SMTO_ABORTIFHUNG,
            2000,
            Some(&mut res),
        );
        Some((res as isize, r.0 == 0))
    }
}

/// The one save path for every control: merge our changes onto a fresh read of the file, write
/// it, ask core to reload, show the outcome. A file that can't be read is never overwritten.
/// Takes the RefCell, not a borrow: the borrow is released before SendMessageTimeoutW, which
/// pumps sent messages and could re-enter other callbacks.
pub fn commit(state: &RefCell<State>, w: &MainWindow) -> SaveOutcome {
    let (ours, base) = {
        let s = state.borrow();
        (s.cfg.clone(), s.base.clone())
    };
    let lang = i18n::resolve(ours.language);
    let disk = read_config();
    let load_ok = disk.is_ok();
    // missing file: nothing to keep, so merging onto `base` gives `ours`
    let disk = disk.ok().flatten().unwrap_or_else(|| base.clone());
    let cfg = save::merge(&base, &ours, &disk);
    let out = save::save_and_reload(load_ok, &config::config_path(), &cfg, || {
        send_to_core(MSG_RELOAD, 0, 0)
    });
    if matches!(out, SaveOutcome::Saved(_)) {
        let reshow = {
            let mut s = state.borrow_mut();
            s.cfg = cfg.clone();
            s.base = cfg.clone();
            s.reshow.clone().filter(|_| cfg != ours)
        };
        if let Some(f) = reshow {
            f();
        }
    }
    match &out {
        SaveOutcome::Blocked => w.set_save_error(i18n::t(lang, Key::SaveBlocked).into()),
        SaveOutcome::WriteFailed(e) => {
            crate::log!("settings: save failed: {e}");
            w.set_save_error(i18n::t(lang, Key::SaveFailed).replace("{err}", e).into());
        }
        SaveOutcome::Saved(reply) => {
            w.set_save_error("".into());
            if *reply == CoreReply::Ok {
                w.set_hotkey_error("".into());
            }
            let key = save::status_key(*reply);
            // the info line clears on the next save that is not "core not running"
            let info = match key {
                StatusKey::CoreNotRunning => i18n::t(lang, Key::CoreNotRunning),
                _ => "",
            };
            w.set_core_info(info.into());
            if key == StatusKey::HotkeyFailed {
                let key = crate::hotkey::display(&cfg.hotkey);
                w.set_hotkey_error(
                    i18n::t(lang, Key::HotkeyInUse)
                        .replace("{key}", &key)
                        .into(),
                );
            }
        }
    }
    out
}

/// Window size in logical px; must match `MainWindow` in ui/settings.slint.
const WINDOW_SIZE: (f64, f64) = (760.0, 520.0);

thread_local! {
    /// Windows' suggested rect while winit handles a WM_DPICHANGED (see `dpi_subclass`).
    static DPI_RECT: std::cell::Cell<Option<windows::Win32::Foundation::RECT>> =
        const { std::cell::Cell::new(None) };
}

/// While winit handles WM_DPICHANGED, steer its SetWindowPos to the rect Windows suggests.
/// winit computes its own rect (keeping the cursor at the same spot); dragging by the left part of
/// the title bar toward a monitor on the left puts that rect mostly back on the old monitor, Windows
/// fires a second DPI change mid-handler, and the size gets scaled twice (seen as a tiny window on
/// the 100% monitor and a huge one back on the 125% one). Windows' suggested rect avoids that.
unsafe extern "system" fn dpi_subclass(
    h: windows::Win32::Foundation::HWND,
    m: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_DPICHANGED, WM_WINDOWPOSCHANGING,
    };
    match m {
        WM_DPICHANGED => {
            DPI_RECT.with(|c| c.set(Some(unsafe { *(lp.0 as *const RECT) })));
            let res = unsafe { DefSubclassProc(h, m, wp, lp) };
            DPI_RECT.with(|c| c.set(None));
            res
        }
        WM_WINDOWPOSCHANGING => {
            if let Some(r) = DPI_RECT.with(|c| c.get()) {
                let pos = unsafe { &mut *(lp.0 as *mut WINDOWPOS) };
                pos.x = r.left;
                pos.y = r.top;
                pos.cx = r.right - r.left;
                pos.cy = r.bottom - r.top;
                pos.flags &= !(SWP_NOMOVE | SWP_NOSIZE);
            }
            unsafe { DefSubclassProc(h, m, wp, lp) }
        }
        _ => unsafe { DefSubclassProc(h, m, wp, lp) },
    }
}

/// Keeps the window its designed size across monitors with different scaling (e.g. 125% and 100%):
/// moves to Windows' suggested rect on DPI change (see `dpi_subclass`), and after each scale or size
/// change, once Slint has handled it, puts the window back to its logical size if it drifted.
fn keep_size_across_dpi(w: &MainWindow) {
    use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
    use winit::event::WindowEvent as E;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let weak = w.as_weak();
    let mut hooked = false;
    w.window().on_winit_window_event(move |win, ev| {
        if !hooked {
            hooked = true;
            win.with_winit_window(|ww| {
                if let Ok(RawWindowHandle::Win32(h)) = ww.window_handle().map(|x| x.as_raw()) {
                    let hwnd = windows::Win32::Foundation::HWND(h.hwnd.get() as *mut _);
                    let ok = unsafe {
                        windows::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(dpi_subclass), 1, 0)
                    };
                    if !ok.as_bool() {
                        crate::log!("settings: DPI subclass failed");
                    }
                }
            });
        }
        if matches!(ev, E::ScaleFactorChanged { .. } | E::Resized(_)) {
            let weak = weak.clone();
            slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
                let Some(w) = weak.upgrade() else { return };
                w.window().with_winit_window(|ww| {
                    let want = winit::dpi::LogicalSize::new(WINDOW_SIZE.0, WINDOW_SIZE.1)
                        .to_physical::<u32>(ww.scale_factor());
                    let have = ww.inner_size();
                    // 1 px slack for rounding, so a correct window never re-triggers itself.
                    if have.width.abs_diff(want.width) > 1 || have.height.abs_diff(want.height) > 1
                    {
                        crate::log!("settings: size {have:?} -> {want:?} after DPI change");
                        let _ = ww.request_inner_size(want);
                    }
                });
            });
        }
        EventResult::Propagate
    });
}

/// Spec §4: open centered on the monitor under the cursor. Best effort; skipped on any failure.
fn center_on_cursor_monitor(w: &MainWindow) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    if w.show().is_err() {
        return;
    }
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return;
    }
    let mut mi = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let hm = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) };
    if !unsafe { GetMonitorInfoW(hm, &mut mi) }.as_bool() {
        return;
    }
    let win = w.window();
    let size = win.size();
    let r = mi.rcWork;
    let x = r.left + ((r.right - r.left) - size.width as i32) / 2;
    let y = r.top + ((r.bottom - r.top) - size.height as i32) / 2;
    win.set_position(slint::PhysicalPosition::new(x, y));
}

pub fn run() -> i32 {
    let state = Rc::new(RefCell::new(load_state()));
    let w = match MainWindow::new() {
        Ok(w) => w,
        Err(e) => {
            crate::log!("settings: window failed: {e}");
            return 1;
        }
    };
    apply_lang(&w, i18n::resolve(state.borrow().cfg.language));
    let mic = super::mic::setup(&w, &state);
    let others = super::others::setup(&w, &state, mic.sink);
    let (pen, flush_pen) = super::penanda::setup(&w, &state);
    let dev = super::device::setup(&w, &state, mic.sink);
    state.borrow_mut().reshow = Some(dev.clone());
    let refresh_mic = mic.refresh.clone();
    super::umum::setup(
        &w,
        &state,
        vec![
            mic.refresh.clone(),
            others.clone(),
            pen.clone(),
            dev.clone(),
        ],
    );
    // Spec §3: re-read devices/monitors whenever a tab that lists them opens.
    w.on_tab_opened(move |t| match t {
        0 => refresh_mic(),
        1 => others(),
        2 => pen(),
        3 => dev(),
        _ => {}
    });
    center_on_cursor_monitor(&w);
    keep_size_across_dpi(&w);
    let res = w.run();
    flush_pen(); // a debounced change may still be waiting
    mic.finish(); // resume the core hotkey if the window closed mid-recording
    if let Err(e) = res {
        crate::log!("settings: run failed: {e}");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_size_matches_slint() {
        let src = include_str!("../../ui/settings.slint");
        let (w, h) = super::WINDOW_SIZE;
        for key in ["min-width", "max-width"] {
            assert!(src.contains(&format!("{key}: {w}px;")), "{key}");
        }
        for key in ["min-height", "max-height"] {
            assert!(src.contains(&format!("{key}: {h}px;")), "{key}");
        }
    }

    #[test]
    fn slint_title_matches_single_window_title() {
        let src = include_str!("../../ui/settings.slint");
        let want = format!("title: \"{}\";", crate::settings::single::WINDOW_TITLE);
        assert!(src.contains(&want));
    }
}
