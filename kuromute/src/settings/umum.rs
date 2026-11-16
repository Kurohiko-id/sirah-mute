//! Umum tab glue: language (live), autostart, update check, log folder, report bug.
use super::model::{UpdateStatus, update_status};
use super::save::SaveOutcome;
use super::ui::{MainWindow, State, apply_lang, commit};
use crate::config::{self, Lang};
use crate::i18n::{self, Key};
use crate::update;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::os::windows::ffi::OsStrExt;
use std::rc::Rc;

/// (checkbox enabled, hint override). Only a Program Files install can own the elevated task,
/// and only an administrator account can elevate. A checked box stays enabled so it can always
/// be turned off.
fn admin_row(in_pf: bool, on: bool, is_admin: bool) -> (bool, Option<Key>) {
    if !is_admin && !on {
        (false, Some(Key::AdminNeedsAdminAccount))
    } else if in_pf {
        (true, None)
    } else {
        (on, Some(Key::AdminNeedsAllUsers))
    }
}

/// (checkbox and config value, error) once the helper has finished.
fn after_helper(want: bool, exit_ok: bool) -> (bool, Option<Key>) {
    if exit_ok {
        (want, None)
    } else {
        (!want, Some(Key::AdminFailed))
    }
}

fn set_admin_note(w: &MainWindow) {
    let on = w.get_gen_admin();
    let in_pf = std::env::current_exe()
        .is_ok_and(|e| crate::admin::in_program_files(&e, &crate::admin::program_files_dirs()));
    let (enabled, hint) = admin_row(in_pf, on, crate::admin::user_is_admin());
    w.set_gen_admin_enabled(enabled);
    let key = hint.unwrap_or(Key::AdminHint);
    w.set_gen_admin_note(i18n::t(selected_lang(w), key).into());
}

/// Runs `sirahmute.exe --admin-on/--admin-off` elevated (UAC prompt unless already elevated)
/// and waits for it. A declined prompt or any launch error counts as failure.
fn run_helper(on: bool) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{PCWSTR, w};
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let args = if on {
        w!("--admin-on")
    } else {
        w!("--admin-off")
    };
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: args,
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if let Err(e) = ShellExecuteExW(&mut info) {
            crate::log!("settings: admin helper launch failed: {e}");
            return false;
        }
        if info.hProcess.is_invalid() {
            return false;
        }
        // UAC prompt plus the helper's own few-second waits fit in 30 s
        if WaitForSingleObject(info.hProcess, 30_000) != WAIT_OBJECT_0 {
            crate::log!("settings: admin helper timed out");
            let _ = CloseHandle(info.hProcess);
            return false;
        }
        let mut code = 1u32;
        let ok = GetExitCodeProcess(info.hProcess, &mut code).is_ok();
        let _ = CloseHandle(info.hProcess);
        ok && code == 0
    }
}

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
        w.set_gen_admin(s.cfg.run_as_admin);
        w.set_app_version(crate::app::VERSION.into());
    }

    set_admin_note(w);

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_gen_admin_toggled(move || {
        let Some(w) = ww.upgrade() else { return };
        let want = w.get_gen_admin();
        w.set_gen_admin_error("".into());
        st.borrow_mut().cfg.run_as_admin = want;
        if !matches!(commit(&st, &w), SaveOutcome::Saved(_)) {
            // not saved: the helper must not run
            st.borrow_mut().cfg.run_as_admin = !want;
            w.set_gen_admin(!want);
            w.set_gen_admin_error(i18n::t(selected_lang(&w), Key::AdminFailed).into());
            return;
        }
        w.set_gen_admin_busy(true);
        let ww = w.as_weak();
        std::thread::spawn(move || {
            let ok = run_helper(want);
            let _ = ww.upgrade_in_event_loop(move |w| w.invoke_gen_admin_finished(ok));
        });
    });

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_gen_admin_finished(move |ok| {
        let Some(w) = ww.upgrade() else { return };
        let want = st.borrow().cfg.run_as_admin;
        let (value, err) = after_helper(want, ok);
        w.set_gen_admin_busy(false);
        w.set_gen_admin(value);
        if let Some(k) = err {
            w.set_gen_admin_error(i18n::t(selected_lang(&w), k).into());
            st.borrow_mut().cfg.run_as_admin = value;
            if !matches!(commit(&st, &w), SaveOutcome::Saved(_)) {
                crate::log!("settings: could not save reverted run_as_admin={value}");
                w.set_gen_admin_error(i18n::t(selected_lang(&w), Key::AdminFailed).into());
            }
        }
    });

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
            w.set_gen_admin_error("".into());
            set_admin_note(&w);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_row_state() {
        use Key::{AdminNeedsAdminAccount as NoAdmin, AdminNeedsAllUsers as NoPf};
        assert_eq!(admin_row(true, false, true), (true, None));
        assert_eq!(admin_row(true, true, true), (true, None));
        assert_eq!(admin_row(false, false, true), (false, Some(NoPf)));
        assert_eq!(admin_row(false, true, true), (true, Some(NoPf)));
        // a standard user can't elevate as themselves
        assert_eq!(admin_row(true, false, false), (false, Some(NoAdmin)));
        assert_eq!(admin_row(false, false, false), (false, Some(NoAdmin)));
        // a checked box stays enabled so it can be turned off
        assert_eq!(admin_row(true, true, false), (true, None));
        assert_eq!(admin_row(false, true, false), (true, Some(NoPf)));
    }

    #[test]
    fn admin_toggle_reverts_on_failure() {
        assert_eq!(after_helper(true, true), (true, None));
        assert_eq!(after_helper(false, true), (false, None));
        assert_eq!(after_helper(true, false), (false, Some(Key::AdminFailed)));
        assert_eq!(after_helper(false, false), (true, Some(Key::AdminFailed)));
    }
}
