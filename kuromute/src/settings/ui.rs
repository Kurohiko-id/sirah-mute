use super::save::{self, CoreReply, SaveOutcome};
use crate::config::{self, Config, LoadOutcome};
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

/// State shared by the settings tabs (extended by Tasks 6-9).
pub struct State {
    pub cfg: Config,
    /// false = config was Corrupt/Unreadable at startup; later tasks must not overwrite blindly.
    pub startup_load_ok: bool,
}

fn load_state() -> State {
    match config::load(&config::config_path()) {
        LoadOutcome::Loaded(cfg) => State {
            cfg,
            startup_load_ok: true,
        },
        LoadOutcome::Missing => State {
            cfg: Config::default(),
            startup_load_ok: true,
        },
        _ => State {
            cfg: Config::default(),
            startup_load_ok: false,
        },
    }
}

/// (Re)applies every UI string for `lang`; Task 9 calls this on language change.
#[allow(dead_code)] // called live by Task 9
pub fn apply_lang(w: &MainWindow, lang: ResolvedLang) {
    let s = w.global::<Strings>();
    s.set_tab_mic(i18n::t(lang, Key::TabMic).into());
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
    s.set_sec_test(i18n::t(lang, Key::SecTest).into());
    s.set_test_label(i18n::t(lang, Key::TestLabel).into());
    s.set_test_button(i18n::t(lang, Key::TestButton).into());
    s.set_sec_popup(i18n::t(lang, Key::SecPopup).into());
    s.set_popup_enabled(i18n::t(lang, Key::PopupEnabled).into());
    s.set_popup_position(i18n::t(lang, Key::PopupPosition).into());
    s.set_popup_scale(i18n::t(lang, Key::PopupScale).into());
    s.set_popup_persist(i18n::t(lang, Key::PopupPersist).into());
    s.set_sec_border(i18n::t(lang, Key::SecBorder).into());
    s.set_border_enabled(i18n::t(lang, Key::BorderEnabled).into());
    s.set_border_thickness(i18n::t(lang, Key::BorderThickness).into());
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

/// The one save path for every control: write config, ask core to reload, show the outcome.
/// Takes the RefCell, not a borrow: the borrow is released before SendMessageTimeoutW, which
/// pumps sent messages and could re-enter other callbacks.
pub fn commit(state: &RefCell<State>, w: &MainWindow) -> SaveOutcome {
    let (cfg, load_ok) = {
        let s = state.borrow();
        (s.cfg.clone(), s.startup_load_ok)
    };
    let lang = i18n::resolve(cfg.language);
    let out = save::save_and_reload(load_ok, &config::config_path(), &cfg, || {
        send_to_core(MSG_RELOAD, 0, 0)
    });
    match &out {
        SaveOutcome::Blocked => w.set_save_error(i18n::t(lang, Key::SaveBlocked).into()),
        SaveOutcome::WriteFailed(e) => {
            crate::log!("settings: save failed: {e}");
            w.set_save_error(i18n::t(lang, Key::SaveFailed).replace("{err}", e).into());
        }
        SaveOutcome::Saved(reply) => {
            w.set_save_error("".into());
            match reply {
                CoreReply::Ok => w.set_hotkey_error("".into()),
                CoreReply::HotkeyFailed => {
                    let key = crate::hotkey::display(&cfg.hotkey);
                    w.set_hotkey_error(
                        i18n::t(lang, Key::HotkeyInUse)
                            .replace("{key}", &key)
                            .into(),
                    );
                }
                // NotRunning info line: Task 10.
                CoreReply::Unknown | CoreReply::NotRunning => {}
            }
        }
    }
    out
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
    let (pen, flush_pen) = super::penanda::setup(&w, &state);
    let dev = super::device::setup(&w, &state, mic.sink);
    let refresh_mic = mic.refresh.clone();
    // Spec §3: re-read devices/monitors whenever a tab that lists them opens.
    w.on_tab_opened(move |t| match t {
        0 => refresh_mic(),
        1 => pen(),
        2 => dev(),
        _ => {}
    });
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
    fn slint_title_matches_single_window_title() {
        let src = include_str!("../../ui/settings.slint");
        let want = format!("title: \"{}\";", crate::settings::single::WINDOW_TITLE);
        assert!(src.contains(&want));
    }
}
