//! Umum tab glue: language (live), autostart, update check, log folder, report bug.
use super::model::{UpdateStatus, update_status};
use super::ui::{MainWindow, State, apply_lang, commit};
use crate::config::{self, Lang};
use crate::i18n::{self, Key};
use crate::update;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

pub const SAWERIA_URL: &str = "https://saweria.co/Kurohiko";
pub const TOPUP_URL: &str = "https://kurohikotopup.com";
const LANGS: [Lang; 3] = [Lang::Auto, Lang::En, Lang::Id];

fn selected_lang(w: &MainWindow) -> i18n::ResolvedLang {
    let l = LANGS.get(w.get_gen_lang().max(0) as usize).copied();
    i18n::resolve(l.unwrap_or_default())
}

/// `refreshers` rebuild tab content whose text was baked at build time (device lists, monitors).
pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>, refreshers: Vec<Rc<dyn Fn()>>) {
    {
        let s = state.borrow();
        w.set_gen_lang(
            LANGS
                .iter()
                .position(|l| *l == s.cfg.language)
                .map_or(0, |i| i as i32),
        );
        w.set_gen_autostart(s.cfg.autostart);
        w.set_app_version(crate::app::VERSION.into());
    }

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_gen_changed(move || {
        let Some(w) = ww.upgrade() else { return };
        let (changed, lang_changed) = {
            let mut s = st.borrow_mut();
            let before = s.cfg.clone();
            if let Some(l) = LANGS.get(w.get_gen_lang().max(0) as usize) {
                s.cfg.language = *l;
            }
            // the core applies the autostart registry value when its reload sees the change
            s.cfg.autostart = w.get_gen_autostart();
            (before != s.cfg, before.language != s.cfg.language)
        };
        if lang_changed {
            apply_lang(&w, selected_lang(&w));
            for r in &refreshers {
                r();
            }
            w.set_upd_text("".into());
            w.set_upd_link(false);
        }
        if changed {
            commit(&st, &w);
        }
    });

    let ww = w.as_weak();
    w.on_check_update_clicked(move || {
        let Some(w) = ww.upgrade() else { return };
        w.set_upd_busy(true);
        w.set_upd_link(false);
        w.set_upd_text(i18n::t(selected_lang(&w), Key::UpdChecking).into());
        let ww = w.as_weak();
        std::thread::spawn(move || {
            let rel = update::fetch_latest();
            if let Err(e) = &rel {
                crate::log!("settings: update check failed: {e}");
            }
            let newer = rel
                .as_ref()
                .map(|r| update::is_newer(&r.tag, crate::app::VERSION))
                .map_err(|e| e.clone());
            let status = update_status(newer);
            let (tag, url) = rel.map(|r| (r.tag, r.url)).unwrap_or_default();
            let _ = ww.upgrade_in_event_loop(move |w| {
                // read the language now: it may have changed while the check ran
                let (key, link) = match status {
                    UpdateStatus::Available => (Key::UpdNewer, true),
                    UpdateStatus::UpToDate => (Key::UpdCurrent, false),
                    UpdateStatus::Failed => (Key::UpdFailed, false),
                };
                w.set_upd_text(
                    i18n::t(selected_lang(&w), key)
                        .replace("{tag}", &tag)
                        .into(),
                );
                w.set_upd_url(url.into());
                w.set_upd_link(link);
                w.set_upd_busy(false);
            });
        });
    });

    let ww = w.as_weak();
    w.on_open_release_clicked(move || {
        if let Some(w) = ww.upgrade() {
            let url = w.get_upd_url();
            if crate::app::is_release_url(&url) {
                crate::app::open_url(&url);
            } else {
                crate::log!("settings: refused release url {url}");
            }
        }
    });

    w.on_open_logs_clicked(|| {
        let dir = config::data_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = std::process::Command::new("explorer.exe").arg(&dir).spawn() {
            crate::log!("settings: open logs failed: {e}");
        }
    });

    w.on_report_bug_clicked(|| {
        crate::app::open_url(&crate::app::report_bug_url(&crate::app::os_build()));
    });

    w.on_support_saweria_clicked(|| crate::app::open_url(SAWERIA_URL));
    w.on_support_topup_clicked(|| crate::app::open_url(TOPUP_URL));
}
