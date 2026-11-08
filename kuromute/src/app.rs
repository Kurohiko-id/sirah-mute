//! Core app: hidden window, message loop and all runtime state.

use crate::config::{self, Config, LoadOutcome, config_path, data_dir};
use crate::enforcer::{Action, Enforcer};
use crate::i18n::{self, Key, ResolvedLang, t};
use crate::msg::*;
use crate::overlay::{Indicators, TIMER_POPUP_HIDE};
use crate::tray::{MenuCmd, Tray, TrayState};
use crate::update::{self, Release};
use crate::{audio, autostart, hotkey, log, migrate, monitors, sound};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::process::{Child, Command};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{NIN_BALLOONUSERCLICK, NIN_SELECT, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, KillTimer, MSG,
    PBT_APMRESUMEAUTOMATIC, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SPI_SETWORKAREA, SW_SHOWNORMAL, SetTimer, TranslateMessage,
    WINDOW_EX_STYLE, WM_CLOSE, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ENDSESSION,
    WM_HOTKEY, WM_LBUTTONUP, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW,
    WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, w};

const HOTKEY_ID: i32 = 1;
const TIMER_DEVICES: usize = 2;
const TIMER_DISPLAY: usize = 3;
const TIMER_GUARD_RESUME: usize = 4;
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HotkeyAction {
    OpenSettings,
    Toggle,
    MicMissing,
}

pub fn hotkey_action(mic_id: Option<&str>, mic_present: bool) -> HotkeyAction {
    match mic_id {
        None => HotkeyAction::OpenSettings,
        Some(_) if !mic_present => HotkeyAction::MicMissing,
        Some(_) => HotkeyAction::Toggle,
    }
}

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub struct Changes {
    pub hotkey: bool,
    pub indicators: bool,
    pub mic: bool,
    pub guard: bool,
    pub autostart: bool,
}

pub fn diff(o: &Config, n: &Config) -> Changes {
    Changes {
        hotkey: o.hotkey != n.hotkey,
        indicators: o.popup_enabled != n.popup_enabled
            || o.popup_position != n.popup_position
            || o.popup_scale != n.popup_scale
            || o.popup_persist_while_muted != n.popup_persist_while_muted
            || o.border_enabled != n.border_enabled
            || o.border_thickness != n.border_thickness
            || o.all_monitors != n.all_monitors
            || o.monitor_ids != n.monitor_ids
            || o.language != n.language,
        mic: o.mic_id != n.mic_id,
        guard: o.default_input_id != n.default_input_id
            || o.default_output_id != n.default_output_id
            || o.guard_mode != n.guard_mode
            || o.guard_enabled != n.guard_enabled,
        autostart: o.autostart != n.autostart,
    }
}

/// Indicator state to show after a re-watch, or None if nothing should be shown.
/// A missing mic (new = None) never shows anything (the caller hides); a replug
/// that comes back unmuted also shows nothing.
fn indicator_change(old: Option<bool>, new: Option<bool>) -> Option<bool> {
    let n = new?;
    ((old == Some(true)) != n).then_some(n)
}

/// Only work-area changes move overlays; other WM_SETTINGCHANGE broadcasts are noise.
fn settingchange_affects_layout(wparam: usize) -> bool {
    wparam == SPI_SETWORKAREA.0 as usize
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn report_bug_url(build: &str) -> String {
    let body =
        format!("**App version:** {VERSION}\n**OS build:** {build}\n\nDescribe the bug here:\n");
    format!(
        "https://github.com/Kurohiko-id/Kuro-Mute-Force-Mic-Input/issues/new?labels=bug&title=Bug%3A%20&body={}",
        pct(&body)
    )
}

fn os_build() -> String {
    let mut buf = [0u16; 64];
    let mut len = size_of_val(&buf) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if r != ERROR_SUCCESS {
        return "unknown".into();
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

struct Registered {
    reload: u32,
    show: u32,
    toggle: u32,
    taskbar: u32,
}

fn reg() -> &'static Registered {
    static R: OnceLock<Registered> = OnceLock::new();
    R.get_or_init(|| unsafe {
        Registered {
            reload: RegisterWindowMessageW(MSG_RELOAD),
            show: RegisterWindowMessageW(MSG_SHOW_SETTINGS),
            toggle: RegisterWindowMessageW(MSG_TOGGLE),
            taskbar: RegisterWindowMessageW(w!("TaskbarCreated")),
        }
    })
}

/// Posts a registered message (by name) to the running core. False if no core is running.
pub fn post_to_core(name: PCWSTR) -> bool {
    unsafe {
        let Ok(h) = FindWindowW(WINDOW_CLASS, PCWSTR::null()) else {
            return false;
        };
        !h.is_invalid()
            && PostMessageW(Some(h), RegisterWindowMessageW(name), WPARAM(0), LPARAM(0)).is_ok()
    }
}

/// Result of the latest update check; the worker fills it, then posts `WM_APP_UPDATE`.
/// Kept here rather than in lparam so a foreign `WM_APP_UPDATE` can't hand us a pointer.
static UPDATE: Mutex<Option<Release>> = Mutex::new(None);

fn spawn_update_check(hwnd: HWND, manual: bool) {
    let h = hwnd.0 as isize;
    std::thread::spawn(move || {
        if !manual {
            std::thread::sleep(Duration::from_secs(10));
        }
        match update::fetch_latest() {
            Ok(r) if update::is_newer(&r.tag, VERSION) => {
                log!("update: {} available", r.tag);
                *UPDATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
            }
            Ok(r) if manual => log!("update: up to date ({})", r.tag),
            Ok(_) => return,
            Err(e) => {
                log!("update check failed: {e}");
                return;
            }
        }
        unsafe {
            let _ = PostMessageW(
                Some(HWND(h as *mut _)),
                WM_APP_UPDATE,
                WPARAM(manual as usize),
                LPARAM(0),
            );
        }
    });
}

fn spawn_set_defaults(ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for id in ids {
            match audio::set_default(&id) {
                Ok(()) => log!("set_default {id}"),
                Err(e) => log!("set_default {id} failed: {e}"),
            }
        }
    });
}

enum Pending {
    ReapplyDefaults,
    OpenUrl(String),
}

struct State {
    hwnd: HWND,
    cfg: Config,
    /// False only when config.json was unreadable at startup (never overwrite it).
    can_save: bool,
    lang: ResolvedLang,
    audio: audio::Audio,
    tray: Tray,
    ind: Indicators,
    enforcer: Enforcer,
    /// Last known mute state of the watched mic; None = no mic / not present.
    last_muted: Option<bool>,
    pending_toggle_at: Option<Instant>,
    pending_balloon: Option<Pending>,
    hotkey_err: Option<String>,
    devices_armed: bool,
    settings_child: Option<Child>,
}

impl State {
    fn balloon(&self, body: &str) {
        self.tray.balloon(t(self.lang, Key::AppName), body);
    }

    fn update_tray(&mut self) {
        let (state, key) = match (&self.cfg.mic_id, self.last_muted) {
            (None, _) => (TrayState::NoMic, Key::NoMicChosen),
            (Some(_), None) => (TrayState::Disconnected, Key::MicDisconnected),
            (Some(_), Some(true)) => (TrayState::Muted, Key::Muted),
            (Some(_), Some(false)) => (TrayState::Unmuted, Key::Unmuted),
        };
        let tip = match &self.hotkey_err {
            Some(e) => e.clone(),
            None => format!("{} - {}", t(self.lang, Key::AppName), t(self.lang, key)),
        };
        self.tray.set(state, &tip);
    }

    fn arm(&self, id: usize, ms: u32) {
        unsafe {
            SetTimer(Some(self.hwnd), id, ms, None);
        }
    }

    /// Debounce: arm only if not already pending, so a steady event stream can't starve it.
    fn arm_devices(&mut self) {
        if !self.devices_armed {
            self.devices_armed = true;
            self.arm(TIMER_DEVICES, 300);
        }
    }

    fn save(&self) {
        if !self.can_save {
            log!("config: not saved (file was unreadable at startup)");
        } else if let Err(e) = config::save(&config_path(), &self.cfg) {
            log!("config: save failed: {e}");
        }
    }

    fn register_hotkey(&mut self) {
        let h = self.cfg.hotkey;
        let (res, key) = match hotkey::validate(&h) {
            Ok(()) => (
                hotkey::register(self.hwnd, HOTKEY_ID, &h).map_err(|e| e.to_string()),
                Key::HotkeyFailed,
            ),
            Err(e) => (Err(format!("{e:?}")), Key::HotkeyInvalid),
        };
        self.hotkey_err = match res {
            Ok(()) => None,
            Err(e) => {
                log!("hotkey {} failed: {e}", hotkey::display(&h));
                let msg = t(self.lang, key).replace("{key}", &hotkey::display(&h));
                self.balloon(&msg);
                Some(msg)
            }
        };
    }

    /// Re-targets the mute watch (replug / mic change) and syncs indicators + tray.
    fn rewatch(&mut self) {
        let new = self.audio.watch_mute(self.cfg.mic_id.as_deref());
        let old = std::mem::replace(&mut self.last_muted, new);
        if new.is_none() {
            self.ind.hide_all();
        } else if let Some(muted) = indicator_change(old, new) {
            self.ind.show(self.hwnd, muted);
        }
        self.update_tray();
    }

    fn rebuild_indicators(&mut self) {
        let all = monitors::enumerate();
        let sel = monitors::select(&all, self.cfg.all_monitors, &self.cfg.monitor_ids);
        self.ind.rebuild(&self.cfg, &sel, self.lang);
        if self.last_muted == Some(true) {
            self.ind.show(self.hwnd, true);
        }
    }

    fn configured_defaults(&self, input: bool, output: bool) -> Vec<String> {
        [
            (input, &self.cfg.default_input_id),
            (output, &self.cfg.default_output_id),
        ]
        .into_iter()
        .filter_map(|(on, id)| if on { id.clone() } else { None })
        .collect()
    }

    fn guard_check(&mut self) {
        let d = self.audio.defaults(
            self.cfg.default_input_id.as_deref(),
            self.cfg.default_output_id.as_deref(),
        );
        let drift_pending = matches!(self.pending_balloon, Some(Pending::ReapplyDefaults));
        match self.enforcer.evaluate(&self.cfg, &d, Instant::now()) {
            Action::None => {
                if drift_pending {
                    self.pending_balloon = None;
                }
            }
            Action::Revert { input, output } => {
                log!("guard: revert input={input} output={output}");
                spawn_set_defaults(self.configured_defaults(input, output));
            }
            // ponytail: one balloon per drift episode; re-notifies only after defaults match again
            Action::Notify { .. } if !drift_pending => {
                log!("guard: drift, notifying");
                self.balloon(t(self.lang, Key::DriftNotify));
                self.pending_balloon = Some(Pending::ReapplyDefaults);
            }
            Action::Notify { .. } => {}
            Action::GiveUp => {
                log!("guard: giving up for 5 minutes");
                self.balloon(t(self.lang, Key::GuardPaused));
                self.arm(TIMER_GUARD_RESUME, 300_000);
            }
        }
    }

    fn apply_autostart(&self) {
        if let Err(e) = autostart::apply(self.cfg.autostart) {
            log!("autostart: {e}");
        }
    }

    fn open_settings(&mut self) {
        if let Some(c) = self.settings_child.as_mut()
            && matches!(c.try_wait(), Ok(None))
        {
            return;
        }
        match std::env::current_exe().and_then(|exe| Command::new(exe).arg("--settings").spawn()) {
            Ok(c) => self.settings_child = Some(c),
            Err(e) => log!("settings: spawn failed: {e}"),
        }
    }

    fn startup(&mut self, note: Option<String>) {
        self.update_tray();
        if let Some(n) = note {
            self.balloon(&n);
        }
        self.register_hotkey();
        self.rebuild_indicators();
        self.rewatch();
        if self.cfg.force_on_startup {
            spawn_set_defaults(self.configured_defaults(true, true));
        }
        self.apply_autostart();
        spawn_update_check(self.hwnd, false);
    }

    fn on_toggle(&mut self) {
        match hotkey_action(self.cfg.mic_id.as_deref(), self.last_muted.is_some()) {
            HotkeyAction::OpenSettings => self.open_settings(),
            HotkeyAction::MicMissing => self.balloon(t(self.lang, Key::MicDisconnected)),
            HotkeyAction::Toggle => {
                self.pending_toggle_at = Some(Instant::now());
                if let Err(e) = self.audio.toggle_mute() {
                    log!("toggle failed: {e}");
                    self.pending_toggle_at = None;
                    self.balloon(t(self.lang, Key::MicDisconnected));
                }
            }
        }
    }

    fn log_latency(&mut self) {
        if let Some(t0) = self.pending_toggle_at.take() {
            log!("latency_ms={}", t0.elapsed().as_millis());
        }
    }

    fn on_mute(&mut self, muted: bool) {
        // Volume changes post this too; only a real state change counts.
        // A pending toggle stays pending until the real change arrives.
        if self.last_muted == Some(muted) {
            return;
        }
        self.last_muted = Some(muted);
        self.ind.show(self.hwnd, muted);
        self.log_latency();
        self.update_tray();
        if self.cfg.sound_enabled {
            sound::play(muted);
        }
    }

    fn on_tray(&mut self, lp: LPARAM) {
        match lp.0 as u32 & 0xFFFF {
            NIN_SELECT | WM_LBUTTONUP => self.open_settings(),
            WM_CONTEXTMENU => {
                if let Some(cmd) = self.tray.menu(self.hwnd, self.cfg.guard_enabled, self.lang) {
                    self.on_menu(cmd);
                }
            }
            NIN_BALLOONUSERCLICK => match self.pending_balloon.take() {
                Some(Pending::ReapplyDefaults) => {
                    spawn_set_defaults(self.configured_defaults(true, true))
                }
                Some(Pending::OpenUrl(u)) => open_url(&u),
                None => {}
            },
            _ => {}
        }
    }

    fn on_menu(&mut self, cmd: MenuCmd) {
        match cmd {
            MenuCmd::Settings => self.open_settings(),
            MenuCmd::ToggleGuard => {
                self.cfg.guard_enabled = !self.cfg.guard_enabled;
                log!("guard_enabled={}", self.cfg.guard_enabled);
                self.save();
                if self.cfg.guard_enabled {
                    self.enforcer.reset();
                    self.rewatch();
                    self.guard_check();
                }
            }
            MenuCmd::CheckUpdate => spawn_update_check(self.hwnd, true),
            MenuCmd::ReportBug => open_url(&report_bug_url(&os_build())),
            MenuCmd::Exit => self.exit(),
        }
    }

    fn exit(&mut self) {
        hotkey::unregister(self.hwnd, HOTKEY_ID);
        self.ind.destroy();
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
    }

    fn on_update(&mut self, manual: bool) {
        match UPDATE.lock().unwrap_or_else(|e| e.into_inner()).take() {
            // Only ever a GitHub URL; refuse anything else before handing it to the shell.
            Some(r) if r.url.starts_with("https://github.com/") => {
                let body = t(self.lang, Key::UpdateAvailable).replace("{ver}", &r.tag);
                self.balloon(&body);
                self.pending_balloon = Some(Pending::OpenUrl(r.url));
            }
            Some(r) => log!("update: unexpected url {}", r.url),
            None if manual => self.balloon(t(self.lang, Key::UpToDate)),
            None => {}
        }
    }

    fn reload(&mut self) {
        let new = match config::load(&config_path()) {
            LoadOutcome::Loaded(c) => c,
            LoadOutcome::Corrupt { backup } => {
                log!(
                    "reload: corrupt, backup {}; keeping current",
                    backup.display()
                );
                self.balloon(t(self.lang, Key::ConfigCorrupt));
                self.save(); // the bad file was renamed away; recreate config.json
                return;
            }
            LoadOutcome::Missing => {
                log!("reload: config missing; keeping current");
                return;
            }
            LoadOutcome::Unreadable(e) => {
                log!("reload: unreadable ({e}); keeping current");
                return;
            }
        };
        let ch = diff(&self.cfg, &new);
        log!("reload: {ch:?}");
        self.cfg = new;
        self.can_save = true;
        self.lang = i18n::resolve(self.cfg.language);
        if ch.hotkey || self.hotkey_err.is_some() {
            hotkey::unregister(self.hwnd, HOTKEY_ID);
            self.register_hotkey();
        }
        if ch.mic {
            self.rewatch();
        }
        if ch.indicators {
            self.rebuild_indicators();
        }
        if ch.guard {
            self.enforcer.reset();
            self.guard_check();
        }
        if ch.autostart {
            self.apply_autostart();
        }
        self.update_tray();
    }

    fn handle(&mut self, m: u32, wp: WPARAM, lp: LPARAM) {
        let r = reg();
        match m {
            WM_HOTKEY => self.on_toggle(),
            WM_APP_MUTE => self.on_mute(wp.0 != 0),
            WM_APP_DEVICES => self.arm_devices(),
            WM_CLOSE => self.exit(),
            WM_ENDSESSION if wp.0 != 0 => self.exit(),
            WM_DISPLAYCHANGE | WM_DPICHANGED => self.arm(TIMER_DISPLAY, 500),
            WM_SETTINGCHANGE if settingchange_affects_layout(wp.0) => self.arm(TIMER_DISPLAY, 500),
            WM_POWERBROADCAST if wp.0 == PBT_APMRESUMEAUTOMATIC as usize => {
                self.arm(TIMER_DISPLAY, 500);
                self.arm_devices();
            }
            WM_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), wp.0);
                }
                match wp.0 {
                    TIMER_POPUP_HIDE => self.ind.hide_popup(),
                    TIMER_DEVICES => {
                        self.devices_armed = false;
                        self.rewatch();
                        self.guard_check();
                    }
                    TIMER_GUARD_RESUME => self.guard_check(),
                    TIMER_DISPLAY => self.rebuild_indicators(),
                    _ => {}
                }
            }
            WM_APP_TRAY => self.on_tray(lp),
            WM_APP_UPDATE => self.on_update(wp.0 != 0),
            _ if m == r.toggle => self.on_toggle(),
            _ if m == r.reload => self.reload(),
            _ if m == r.show => self.open_settings(),
            _ if m == r.taskbar => self.tray.readd(),
            _ => {}
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// Messages that arrived re-entrantly (e.g. inside the tray menu's modal loop).
    static DEFERRED: RefCell<VecDeque<(u32, WPARAM, LPARAM)>> = const { RefCell::new(VecDeque::new()) };
}

/// Runs `f` on the state, then any messages deferred while it was borrowed.
/// Returns false if the state doesn't exist yet. If already borrowed, queues `m` instead.
fn with_state(m: Option<(u32, WPARAM, LPARAM)>, f: impl FnOnce(&mut State)) -> bool {
    STATE.with(|cell| {
        let Ok(mut g) = cell.try_borrow_mut() else {
            if let Some(m) = m {
                DEFERRED.with(|d| d.borrow_mut().push_back(m));
            }
            return true;
        };
        let Some(s) = g.as_mut() else { return false };
        f(s);
        while let Some((m, wp, lp)) = DEFERRED.with(|d| d.borrow_mut().pop_front()) {
            s.handle(m, wp, lp);
        }
        true
    })
}

fn is_ours(m: u32) -> bool {
    let r = reg();
    matches!(
        m,
        WM_HOTKEY
            | WM_CLOSE
            | WM_ENDSESSION
            | WM_TIMER
            | WM_DISPLAYCHANGE
            | WM_SETTINGCHANGE
            | WM_DPICHANGED
            | WM_POWERBROADCAST
            | WM_APP_MUTE
            | WM_APP_DEVICES
            | WM_APP_UPDATE
            | WM_APP_TRAY
    ) || (m != 0 && [r.reload, r.show, r.toggle, r.taskbar].contains(&m))
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if is_ours(m) && with_state(Some((m, wp, lp)), |s| s.handle(m, wp, lp)) {
        // WM_CLOSE / WM_ENDSESSION return 0 (handled) so DefWindowProc doesn't destroy first.
        return LRESULT((m == WM_POWERBROADCAST) as isize);
    }
    unsafe { DefWindowProcW(h, m, wp, lp) }
}

fn create_window() -> windows::core::Result<HWND> {
    unsafe {
        let hinst = HINSTANCE(GetModuleHandleW(None)?.0);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        // Top-level (never shown) so it receives TaskbarCreated; message-only windows don't.
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            w!("KuroMute"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )
    }
}

/// Startup config: returns (config, can_save, balloon text for the user).
fn load_startup() -> (Config, bool, Option<String>) {
    let path = config_path();
    match config::load(&path) {
        LoadOutcome::Loaded(c) => (c, true, None),
        LoadOutcome::Missing => {
            let legacy = data_dir().join("settings.json");
            let cfg = match std::fs::read_to_string(&legacy) {
                Ok(s) => match migrate::from_legacy(&s) {
                    Ok(c) => Some(c),
                    Err(e) => {
                        log!("config: legacy migrate failed: {e}");
                        None
                    }
                },
                Err(_) => None,
            };
            let migrated = cfg.is_some();
            let cfg = cfg.unwrap_or_default();
            match config::save(&path, &cfg) {
                Ok(()) if migrated => log!("config: migrated"),
                Ok(()) => log!("config: created defaults"),
                Err(e) => log!("config: save failed: {e}"),
            }
            (cfg, true, None)
        }
        LoadOutcome::Corrupt { backup } => {
            log!(
                "config: corrupt, backup {}; using defaults",
                backup.display()
            );
            let cfg = Config::default();
            let body = t(i18n::resolve(cfg.language), Key::ConfigCorrupt).to_string();
            (cfg, true, Some(body))
        }
        LoadOutcome::Unreadable(e) => {
            log!("config: unreadable ({e}); running on defaults, will not save");
            (Config::default(), false, Some(e))
        }
    }
}

/// Runs the tray core until Exit. Returns the process exit code.
pub fn run() -> i32 {
    unsafe {
        // Handle intentionally kept open for the life of the process.
        let _ = CreateMutexW(None, false, w!("Local\\KuroMute.Core"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            post_to_core(MSG_SHOW_SETTINGS);
            return 0;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    log!("start v{VERSION}");
    let (cfg, can_save, note) = load_startup();
    let hwnd = match create_window() {
        Ok(h) => h,
        Err(e) => {
            log!("window create failed: {e}");
            return 1;
        }
    };
    let audio = match audio::Audio::new(hwnd) {
        Ok(a) => a,
        Err(e) => {
            log!("audio init failed: {e}");
            return 1;
        }
    };
    let tray = Tray::add(hwnd);
    let state = State {
        hwnd,
        lang: i18n::resolve(cfg.language),
        cfg,
        can_save,
        audio,
        tray,
        ind: Indicators::new(),
        enforcer: Enforcer::new(),
        last_muted: None,
        pending_toggle_at: None,
        pending_balloon: None,
        hotkey_err: None,
        devices_armed: false,
        settings_child: None,
    };
    STATE.with(|c| *c.borrow_mut() = Some(state));
    with_state(None, |s| s.startup(note));

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    STATE.with(|c| c.borrow_mut().take());
    log!("exit");
    // ponytail: log writer thread isn't joined; give it a moment to flush.
    std::thread::sleep(Duration::from_millis(50));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_actions() {
        assert_eq!(hotkey_action(None, false), HotkeyAction::OpenSettings);
        assert_eq!(hotkey_action(Some("x"), false), HotkeyAction::MicMissing);
        assert_eq!(hotkey_action(Some("x"), true), HotkeyAction::Toggle);
    }

    #[test]
    fn diff_border_thickness_only_indicators() {
        let a = Config::default();
        let b = Config {
            border_thickness: 9,
            ..a.clone()
        };
        assert_eq!(
            diff(&a, &b),
            Changes {
                indicators: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn diff_mic_only_mic() {
        let a = Config::default();
        let b = Config {
            mic_id: Some("m".into()),
            ..a.clone()
        };
        assert_eq!(
            diff(&a, &b),
            Changes {
                mic: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn diff_guard_enabled_flags_guard() {
        let a = Config::default();
        let b = Config {
            guard_enabled: !a.guard_enabled,
            ..a.clone()
        };
        assert!(diff(&a, &b).guard);
    }

    #[test]
    fn indicator_change_cases() {
        assert_eq!(indicator_change(None, Some(true)), Some(true));
        assert_eq!(indicator_change(Some(true), None), None);
        assert_eq!(indicator_change(None, None), None);
        assert_eq!(indicator_change(Some(false), None), None);
        assert_eq!(indicator_change(Some(true), Some(false)), Some(false));
        assert_eq!(indicator_change(Some(false), Some(true)), Some(true));
        assert_eq!(indicator_change(None, Some(false)), None);
        assert_eq!(indicator_change(Some(true), Some(true)), None);
    }

    #[test]
    fn settingchange_only_workarea() {
        assert!(settingchange_affects_layout(0x2F));
        assert!(!settingchange_affects_layout(0));
        assert!(!settingchange_affects_layout(0x14)); // SPI_SETDESKWALLPAPER
    }

    #[test]
    fn pct_encodes_reserved_and_utf8() {
        assert_eq!(pct("a-Z_0.~ *:\n"), "a-Z_0.~%20%2A%3A%0A");
        assert_eq!(pct("é"), "%C3%A9");
    }

    #[test]
    fn diff_identical_nothing() {
        let a = Config::default();
        assert_eq!(diff(&a, &a.clone()), Changes::default());
    }
}
