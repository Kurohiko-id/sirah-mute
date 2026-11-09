use crate::config::{self, Config, LoadOutcome};
use crate::i18n::{self, Key, ResolvedLang};
use slint::ComponentHandle;

slint::include_modules!();

/// State shared by the settings tabs (extended by Tasks 6-9).
#[allow(dead_code)] // read by Tasks 6-9
pub struct State {
    pub cfg: Config,
    /// false = config was Corrupt/Unreadable at startup; later tasks must not overwrite blindly.
    pub startup_load_ok: bool,
}

fn load_state() -> State {
    match config::load(&config::config_path()) {
        LoadOutcome::Loaded(cfg) => State { cfg, startup_load_ok: true },
        LoadOutcome::Missing => State { cfg: Config::default(), startup_load_ok: true },
        _ => State { cfg: Config::default(), startup_load_ok: false },
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
}

pub fn run() -> i32 {
    let state = load_state();
    let w = match MainWindow::new() {
        Ok(w) => w,
        Err(e) => {
            crate::log!("settings: window failed: {e}");
            return 1;
        }
    };
    apply_lang(&w, i18n::resolve(state.cfg.language));
    if let Err(e) = w.run() {
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
