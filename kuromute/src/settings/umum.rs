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
        // ShellExecuteExW returned after the UAC decision, so 30 s covers only the helper run
        // (its own waits are a few seconds)
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

/// Setup switch that keeps the current install mode, or None for a portable copy (no
/// uninstaller next to the exe): updating that would install a second copy elsewhere.
fn install_mode() -> Option<&'static str> {
    let exe = std::env::current_exe().ok()?;
    if !exe.with_file_name("unins000.exe").exists() {
        return None;
    }
    let all_users = crate::admin::in_program_files(&exe, &crate::admin::program_files_dirs());
    Some(if all_users {
        "/ALLUSERS"
    } else {
        "/CURRENTUSER"
    })
}

/// Downloads and checks the setup exe off the UI thread, then runs it silently. Setup closes
/// Sirah Mute (this window too) and starts the new version when it is done.
fn install_update(w: &MainWindow) {
    let Some(mode) = install_mode() else { return };
    let asset = update::Asset {
        url: w.get_upd_asset_url().into(),
        sha256: w.get_upd_asset_sha().into(),
        size: w.get_upd_asset_size().max(0) as u64,
    };
    w.set_upd_installing(true);
    w.set_upd_install_error("".into());
    show_progress(w, 0, asset.size);
    let ww = w.as_weak();
    std::thread::spawn(move || {
        let mut shown = 0usize;
        let mut on_bytes = |n: usize| {
            if n - shown >= 128 << 10 {
                shown = n;
                let size = asset.size;
                let _ = ww.upgrade_in_event_loop(move |w| show_progress(&w, n as u64, size));
            }
        };
        let res = update::download_installer(&asset, &mut on_bytes);
        let res = res.and_then(|p| {
            // ponytail: no progress from setup itself; its own /SILENT window shows the install
            let _ = ww.upgrade_in_event_loop(|w| {
                w.set_upd_progress(1.0);
                w.set_upd_progress_text(i18n::t(selected_lang(&w), Key::UpdInstalling).into());
            });
            std::process::Command::new(&p)
                .args([mode, "/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
                .spawn()
                .map(drop)
                .map_err(|e| e.to_string())
        });
        if let Err(e) = &res {
            crate::log!("settings: update install failed: {e}");
            let _ = ww.upgrade_in_event_loop(|w| {
                w.set_upd_installing(false);
                w.set_upd_install_error(i18n::t(selected_lang(&w), Key::UpdInstallFailed).into());
            });
        } // on success the button stays disabled until setup closes this window
    });
}

/// "Downloading 3.2 / 7.1 MB" and the bar.
fn show_progress(w: &MainWindow, done: u64, total: u64) {
    let mb = |b: u64| format!("{:.1}", b as f64 / 1_048_576.0);
    w.set_upd_progress(if total > 0 {
        done as f32 / total as f32
    } else {
        0.0
    });
    w.set_upd_progress_text(
        i18n::t(selected_lang(w), Key::UpdDownloading)
            .replace("{done}", &mb(done))
            .replace("{total}", &mb(total))
            .into(),
    );
}

/// Fetches the latest release off the UI thread and fills the Updates section, the sidebar
/// chip and the "what's new" popup; `popup` shows it when a newer version is found.
fn check_update(w: &MainWindow, popup: bool) {
    if w.get_upd_busy() {
        return; // a check is running; it reads the language when it finishes
    }
    w.set_upd_busy(true);
    w.set_upd_link(false);
    w.set_upd_text(i18n::t(selected_lang(w), Key::UpdChecking).into());
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
        let (tag, url, highlights, installer) = rel
            .map(|r| (r.tag, r.url, r.highlights, r.installer))
            .unwrap_or_default();
        let _ = ww.upgrade_in_event_loop(move |w| {
            // read the language now: it may have changed while the check ran
            let lang = selected_lang(&w);
            let (key, link) = match status {
                UpdateStatus::Available => (Key::UpdNewer, true),
                UpdateStatus::UpToDate => (Key::UpdCurrent, false),
                UpdateStatus::Failed => (Key::UpdFailed, false),
            };
            w.set_upd_text(i18n::t(lang, key).replace("{tag}", &tag).into());
            w.set_upd_chip(i18n::t(lang, Key::UpdChip).replace("{tag}", &tag).into());
            w.set_upd_title(i18n::t(lang, Key::UpdTitle).into());
            w.set_upd_whats_new(i18n::t(lang, Key::UpdWhatsNew).into());
            w.set_upd_later(i18n::t(lang, Key::UpdLater).into());
            let items: Vec<slint::SharedString> = highlights.into_iter().map(Into::into).collect();
            w.set_upd_highlights(slint::ModelRc::new(slint::VecModel::from(items)));
            w.set_upd_tag(tag.into());
            w.set_upd_can_install(installer.is_some() && install_mode().is_some());
            w.set_upd_install(i18n::t(lang, Key::UpdInstall).into());
            let a = installer.unwrap_or_default();
            w.set_upd_asset_url(a.url.into());
            w.set_upd_asset_sha(a.sha256.into());
            w.set_upd_asset_size(a.size.min(i32::MAX as u64) as i32);
            w.set_upd_url(url.into());
            w.set_upd_link(link);
            w.set_upd_busy(false);
            if link && popup {
                w.invoke_show_update();
            }
        });
    });
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
            check_update(&w, false); // texts are in the old language
            w.set_gen_admin_error("".into());
            set_admin_note(&w);
        }
        if changed {
            commit(&st, &w);
        }
    });

    let ww = w.as_weak();
    w.on_check_update_clicked(move || {
        if let Some(w) = ww.upgrade() {
            check_update(&w, true);
        }
    });
    check_update(w, true); // on open, so a new version shows without a click

    let ww = w.as_weak();
    w.on_install_update_clicked(move || {
        if let Some(w) = ww.upgrade() {
            install_update(&w);
        }
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
