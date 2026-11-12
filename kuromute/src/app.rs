//! Core app: hidden window, message loop and all runtime state.

use crate::config::{self, Config, LoadOutcome, config_path};
use crate::enforcer::{Action, Enforcer};
use crate::i18n::{self, Key, ResolvedLang, t};
use crate::msg::*;
use crate::others::OtherPills;
use crate::overlay::{Indicators, TIMER_MUTE_CLOCK, TIMER_POPUP_HIDE, TIMER_PULSE};
use crate::prompt::{self, Choice, Prompt, Row};
use crate::talk::TalkDetector;
use crate::tray::{MenuCmd, Tray, TrayState};
use crate::update::{self, Release};
use crate::{audio, autostart, hotkey, log, migrate, monitors, sound};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE, HINSTANCE, HWND,
    LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{NIN_BALLOONUSERCLICK, NIN_SELECT, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, KillTimer, MSG,
    PBT_APMRESUMEAUTOMATIC, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SMTO_ABORTIFHUNG, SPI_SETWORKAREA, SW_SHOWNORMAL, SendMessageTimeoutW,
    SetTimer, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WM_CONTEXTMENU, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP, WM_POWERBROADCAST, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, w};

const HOTKEY_ID: i32 = 1;
const TIMER_DEVICES: usize = 2;
const TIMER_DISPLAY: usize = 3;
const TIMER_GUARD_RESUME: usize = 4;
const TIMER_SETTINGS_WATCH: usize = 5;
pub(crate) const TIMER_TALK: usize = 8;
const TIMER_DRIFT_SNOOZE: usize = 9;
const TIMER_COUGH: usize = 10;
const COUGH_POLL_MS: u32 = 20;
const DRIFT_SNOOZE_MS: u32 = 15 * 60 * 1000;
const TALK_POLL_MS: u32 = 100;
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    pub talk: bool,
    pub others: bool,
    pub cough: bool,
}

pub fn diff(o: &Config, n: &Config) -> Changes {
    Changes {
        hotkey: o.hotkey != n.hotkey,
        indicators: o.popup_enabled != n.popup_enabled
            || o.popup_position != n.popup_position
            || o.popup_scale != n.popup_scale
            || o.popup_persist_while_muted != n.popup_persist_while_muted
            || o.popup_style != n.popup_style
            || o.border_enabled != n.border_enabled
            || o.border_thickness != n.border_thickness
            || o.border_style != n.border_style
            || o.all_monitors != n.all_monitors
            || o.monitor_ids != n.monitor_ids
            || o.language != n.language,
        mic: o.mic_id != n.mic_id,
        guard: o.default_input_id != n.default_input_id
            || o.default_output_id != n.default_output_id
            || o.guard_mode != n.guard_mode
            || o.guard_enabled != n.guard_enabled,
        autostart: o.autostart != n.autostart,
        talk: o.talk_warn_enabled != n.talk_warn_enabled
            || o.talk_warn_mic_id != n.talk_warn_mic_id,
        others: o.other_mics != n.other_mics || o.other_mics_position != n.other_mics_position,
        cough: o.cough_hold != n.cough_hold || o.cough_hold_ms != n.cough_hold_ms,
    }
}

/// Mic id behind other-pill `i`; None for an index from a stale layout.
fn other_click_target(shown: &[String], i: usize) -> Option<&str> {
    shown.get(i).map(String::as_str)
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

pub(crate) fn report_bug_url(build: &str) -> String {
    let body =
        format!("**App version:** {VERSION}\n**OS build:** {build}\n\nDescribe the bug here:\n");
    format!(
        "https://github.com/Kurohiko-id/sirah-mute/issues/new?labels=bug&title=Bug%3A%20&body={}",
        pct(&body)
    )
}

pub(crate) fn os_build() -> String {
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

/// Only GitHub URLs are ever handed to the shell.
pub(crate) fn is_release_url(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

pub(crate) fn open_url(url: &str) {
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

pub fn reload_reply_code(hotkey_failed: bool) -> isize {
    hotkey_failed as isize
}

struct Registered {
    reload: u32,
    show: u32,
    toggle: u32,
    pause: u32,
    remote: u32,
    taskbar: u32,
}

fn reg() -> &'static Registered {
    static R: OnceLock<Registered> = OnceLock::new();
    R.get_or_init(|| unsafe {
        Registered {
            reload: RegisterWindowMessageW(MSG_RELOAD),
            show: RegisterWindowMessageW(MSG_SHOW_SETTINGS),
            toggle: RegisterWindowMessageW(MSG_TOGGLE),
            pause: RegisterWindowMessageW(MSG_PAUSE_HOTKEY),
            remote: RegisterWindowMessageW(MSG_REMOTE),
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

pub const ST_LIVE: isize = 0;
pub const ST_MUTED: isize = 1;
pub const ST_NO_MIC: isize = 2;
/// Remote replies are `REPLY_BASE + code`, so an older core that doesn't know
/// MSG_REMOTE (DefWindowProc returns 0) can't pass for a live one.
pub const REPLY_BASE: isize = 0x5300;

/// Mirror of `State::last_muted` so `--status` answers even while `State` is borrowed.
static LAST_STATUS: AtomicIsize = AtomicIsize::new(ST_NO_MIC);

pub fn status_code(last_muted: Option<bool>) -> isize {
    match last_muted {
        None => ST_NO_MIC,
        Some(true) => ST_MUTED,
        Some(false) => ST_LIVE,
    }
}

/// Raw reply -> (stdout word, exit code). No reply / unknown reply = core not running.
pub fn status_word(reply: Option<isize>) -> (&'static str, i32) {
    match reply.map(|r| r.wrapping_sub(REPLY_BASE)) {
        Some(ST_LIVE) => ("live", 0),
        Some(ST_MUTED) => ("muted", 1),
        Some(ST_NO_MIC) => ("no_mic", 2),
        _ => ("not_running", 3),
    }
}

/// CLI side of `--status` (0) / `--mute` (1) / `--unmute` (2): asks the core, prints the word,
/// returns the exit code.
pub fn remote(action: usize) -> i32 {
    let reply = unsafe {
        FindWindowW(WINDOW_CLASS, PCWSTR::null())
            .ok()
            .and_then(|h| {
                let mut res = 0usize;
                let r = SendMessageTimeoutW(
                    h,
                    RegisterWindowMessageW(MSG_REMOTE),
                    WPARAM(action),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    2000,
                    Some(&mut res),
                );
                (r.0 != 0).then_some(res as isize)
            })
    };
    let (word, code) = status_word(reply);
    crate::print_stdout(word);
    code
}

/// MSG_REMOTE wparam -> Some(mute) for 1/2; 0 (status) and anything unknown -> None.
fn remote_action(wp: usize) -> Option<bool> {
    match wp {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Answers MSG_REMOTE. Mute/unmute go through `with_state` so a borrowed State defers them.
fn on_remote(m: u32, wp: WPARAM, lp: LPARAM) -> isize {
    REPLY_BASE + remote_code(m, wp, lp)
}

fn remote_code(m: u32, wp: WPARAM, lp: LPARAM) -> isize {
    let cur = LAST_STATUS.load(Ordering::Relaxed);
    let Some(mute) = remote_action(wp.0) else {
        return cur;
    };
    if cur == ST_NO_MIC {
        return cur;
    }
    let want = if mute { ST_MUTED } else { ST_LIVE };
    let mut ok = true;
    if !with_state(Some((m, wp, lp)), |s| ok = s.remote_mute(mute)) {
        return ST_NO_MIC;
    }
    if ok { want } else { ST_NO_MIC }
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

/// Guard Notify episode: no drift, prompt up, user said keep (quiet until defaults match again),
/// or snoozed (asks again when TIMER_DRIFT_SNOOZE fires).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Drift {
    Clear,
    Asking,
    Kept,
    Snoozed,
}

/// Drift state after a guard check; `true` = show the prompt now.
fn drift_after_check(d: Drift, notify: bool) -> (Drift, bool) {
    match (notify, d) {
        (false, _) => (Drift::Clear, false),
        (true, Drift::Clear) => (Drift::Asking, true),
        (true, d) => (d, false),
    }
}

/// Drift state after a prompt button. Switch back also goes quiet: the defaults match again once
/// the switch lands, which clears it; until then a device event must not re-ask.
fn drift_after_choice(c: Choice) -> Drift {
    match c {
        Choice::SwitchBack | Choice::Keep => Drift::Kept,
        Choice::Snooze => Drift::Snoozed,
    }
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
    others: OtherPills,
    /// Ids of the other mics whose pills show, in shown order (click index -> id).
    others_shown: Vec<String>,
    enforcer: Enforcer,
    /// Last known mute state of the watched mic; None = no mic / not present.
    last_muted: Option<bool>,
    pending_toggle_at: Option<Instant>,
    /// Cough hold: when the hotkey press that muted began; Some while TIMER_COUGH runs.
    cough_since: Option<Instant>,
    pending_balloon: Option<Pending>,
    drift: Drift,
    prompt: Option<Prompt>,
    hotkey_err: Option<String>,
    devices_armed: bool,
    settings_child: Option<Child>,
    /// Settings process named by the pause message: Some(None) = open failed (resume at next tick).
    settings_proc: Option<Option<HANDLE>>,
    hotkey_paused: bool,
    /// Reply to MSG_RELOAD: 0 ok, 1 hotkey registration failed.
    last_reload_code: isize,
    talk_meter: Option<IAudioMeterInformation>,
    talk: TalkDetector,
}

impl State {
    /// Every balloon drops the previous click action; callers that want one set it after.
    fn balloon(&mut self, body: &str) {
        self.pending_balloon = None;
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
        if self.hotkey_paused {
            return; // resume registers it
        }
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
    /// Watches the talk-warn mic's meter only while the mic is muted and the
    /// feature is configured. Activation is a COM call: never before `log_latency`.
    fn update_talk_watch(&mut self) {
        let id = self.cfg.talk_warn_mic_id.as_deref();
        let on = self.last_muted == Some(true) && self.cfg.talk_warn_enabled;
        self.talk_meter = id.filter(|_| on).and_then(|id| self.audio.meter(id));
        self.talk.reset();
        unsafe {
            if self.talk_meter.is_some() {
                SetTimer(Some(self.hwnd), TIMER_TALK, TALK_POLL_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_TALK);
            }
        }
    }

    /// TIMER_TALK handler (the app kills the timer first, so this re-arms itself).
    fn tick_talk(&mut self) {
        let Some(m) = &self.talk_meter else { return };
        if self.last_muted != Some(true) {
            self.talk_meter = None;
            return;
        }
        // An error means the device is gone: drop the meter silently; the
        // device-change path re-activates it on replug.
        let Ok(peak) = (unsafe { m.GetPeakValue() }) else {
            self.talk_meter = None;
            return;
        };
        if self.talk.feed(peak, Instant::now()) {
            self.ind.pulse(self.hwnd);
            self.ind.flash_popup(self.hwnd);
            if self.cfg.sound_enabled {
                sound::play(true);
            }
            log!("talk: warned");
        }
        self.arm(TIMER_TALK, TALK_POLL_MS);
    }

    fn rewatch(&mut self) {
        let new = self.audio.watch_mute(self.cfg.mic_id.as_deref());
        let old = std::mem::replace(&mut self.last_muted, new);
        LAST_STATUS.store(status_code(new), Ordering::Relaxed);
        crate::stats::mute_state(new == Some(true));
        if new.is_none() {
            self.ind.hide_all();
        } else if let Some(muted) = indicator_change(old, new) {
            self.ind.show(self.hwnd, muted, true);
        }
        self.update_talk_watch();
        self.update_tray();
    }

    fn rebuild_indicators(&mut self) {
        let all = monitors::enumerate();
        let sel = monitors::select(&all, self.cfg.all_monitors, &self.cfg.monitor_ids);
        self.ind.rebuild(&self.cfg, &sel, self.lang);
        if self.last_muted == Some(true) {
            self.ind.show(self.hwnd, true, false);
        }
        self.rebuild_others();
        if self.drift == Drift::Asking {
            // Monitor layout changed: ask again so the prompt lands in the new work area.
            self.drift = Drift::Clear;
            self.prompt = None;
            self.guard_check();
        }
    }

    fn rebuild_others(&mut self) {
        let all = monitors::enumerate();
        let sel = monitors::select(&all, self.cfg.all_monitors, &self.cfg.monitor_ids);
        self.others.rebuild(&self.cfg, &sel);
        self.others_shown.clear(); // new windows start hidden: force a redraw
        self.rewatch_others();
    }

    /// Watches the enabled other mics (never the main one) and syncs their pills.
    /// Absent mics are skipped, so an unplugged mic's pill goes on the next call.
    fn rewatch_others(&mut self) {
        let main = self.cfg.mic_id.as_deref();
        let ids: Vec<String> = self
            .cfg
            .other_mics
            .iter()
            .filter(|m| m.enabled && Some(m.id.as_str()) != main)
            .map(|m| m.id.clone())
            .collect();
        self.audio.watch_others(&ids);
        self.refresh_others();
    }

    /// Redraws the pill group only when the set of muted other mics changed.
    fn refresh_others(&mut self) {
        let muted: Vec<String> = self
            .audio
            .others_muted()
            .into_iter()
            .filter_map(|(id, m)| m.then_some(id))
            .collect();
        if muted == self.others_shown {
            return;
        }
        let devs = self.audio.list(audio::Flow::Capture);
        let labels: Vec<String> = muted
            .iter()
            .map(|id| {
                let name = self.cfg.other_mics.iter().find(|m| &m.id == id);
                match name.map(|m| m.name.trim()).filter(|n| !n.is_empty()) {
                    Some(n) => n.to_string(),
                    None => devs
                        .iter()
                        .find(|d| &d.id == id)
                        .map(|d| d.name.chars().take(config::OTHER_NAME_MAX).collect())
                        .unwrap_or_else(|| "Mic".to_string()),
                }
            })
            .collect();
        self.others.show(&labels);
        self.others_shown = muted;
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
        let action = self.enforcer.evaluate(&self.cfg, &d, Instant::now());
        let notify = matches!(action, Action::Notify { .. });
        let (next, ask) = drift_after_check(self.drift, notify);
        if next == Drift::Clear {
            self.prompt = None;
        }
        self.drift = next;
        match action {
            Action::None => {}
            Action::Revert { input, output } => {
                log!("guard: revert input={input} output={output}");
                spawn_set_defaults(self.configured_defaults(input, output));
            }
            Action::Notify { input, output } if ask => {
                log!("guard: drift input={input} output={output}, asking");
                self.show_drift(input, output, &d);
            }
            Action::Notify { .. } => {}
            Action::GiveUp => {
                log!("guard: giving up for 5 minutes");
                self.balloon(t(self.lang, Key::GuardPaused));
                self.arm(TIMER_GUARD_RESUME, 300_000);
            }
        }
    }

    /// Shows the drift prompt on the primary monitor; falls back to a balloon if it can't.
    fn show_drift(&mut self, input: bool, output: bool, d: &crate::enforcer::Defaults) {
        let name = |flow: audio::Flow, id: Option<&str>| -> String {
            let id = id.unwrap_or_default();
            self.audio
                .list(flow)
                .into_iter()
                .find(|x| x.id == id)
                .map(|x| x.name)
                .unwrap_or_else(|| "?".into())
        };
        // "Now" = the first role that isn't the pick (drift can be in one role only).
        let now = |roles: &[Option<String>; 3], pick: &Option<String>| {
            roles
                .iter()
                .flatten()
                .find(|r| Some(*r) != pick.as_ref())
                .cloned()
        };
        let mut rows = Vec::new();
        if input {
            let pick = &self.cfg.default_input_id;
            rows.push(Row {
                now_label: Key::DriftMicNow,
                now: name(audio::Flow::Capture, now(&d.capture, pick).as_deref()),
                pick: name(audio::Flow::Capture, pick.as_deref()),
            });
        }
        if output {
            let pick = &self.cfg.default_output_id;
            rows.push(Row {
                now_label: Key::DriftSpeakerNow,
                now: name(audio::Flow::Render, now(&d.render, pick).as_deref()),
                pick: name(audio::Flow::Render, pick.as_deref()),
            });
        }
        let title = match (input, output) {
            (true, true) => Key::DriftTitleBoth,
            (true, false) => Key::DriftTitleMic,
            _ => Key::DriftTitleSpeaker,
        };
        let mons = monitors::enumerate();
        let shown = mons
            .iter()
            .find(|m| m.primary)
            .or(mons.first())
            .map(|m| prompt::show(m, self.lang, title, &rows));
        match shown {
            Some(Ok(p)) => self.prompt = Some(p),
            other => {
                if let Some(Err(e)) = other {
                    log!("drift prompt: {e}");
                }
                self.balloon(t(self.lang, Key::DriftNotify));
                self.pending_balloon = Some(Pending::ReapplyDefaults);
            }
        }
    }

    fn on_drift_choice(&mut self, wp: usize) {
        let Some(c) = Choice::from_wparam(wp) else {
            return;
        };
        if self.drift != Drift::Asking {
            return;
        }
        log!("guard: drift choice {c:?}");
        self.prompt = None;
        self.drift = drift_after_choice(c);
        match c {
            Choice::SwitchBack => spawn_set_defaults(self.configured_defaults(true, true)),
            Choice::Keep => {}
            Choice::Snooze => self.arm(TIMER_DRIFT_SNOOZE, DRIFT_SNOOZE_MS),
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
            crate::settings::single::raise();
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

    fn stop_cough(&mut self) {
        self.cough_since = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_COUGH);
        }
    }

    /// Hold ended: unmute if held long enough. Not a latency measurement.
    fn finish_cough(&mut self) {
        let Some(t0) = self.cough_since.take() else {
            return;
        };
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_COUGH);
        }
        if hotkey::cough_release(t0.elapsed(), self.cfg.cough_hold_ms)
            && let Err(e) = self.audio.set_mute(false)
        {
            log!("cough unmute failed: {e}");
        }
    }

    fn tick_cough(&mut self) {
        if self.cough_since.is_none() {
            return; // stale WM_TIMER queued before stop_cough
        }
        if hotkey::key_down(self.cfg.hotkey.vk) {
            self.arm(TIMER_COUGH, COUGH_POLL_MS);
        } else {
            self.finish_cough();
        }
    }

    fn on_hotkey(&mut self) {
        if !self.cfg.cough_hold
            || hotkey_action(self.cfg.mic_id.as_deref(), self.last_muted.is_some())
                != HotkeyAction::Toggle
        {
            return self.on_toggle();
        }
        self.finish_cough(); // press while a hold is live: treat as release first
        if self.last_muted == Some(false) {
            let now = Instant::now();
            self.pending_toggle_at = Some(now);
            if let Err(e) = self.audio.set_mute(true) {
                log!("cough mute failed: {e}");
                self.pending_toggle_at = None;
                self.balloon(t(self.lang, Key::MicDisconnected));
                return;
            }
            self.cough_since = Some(now);
            self.arm(TIMER_COUGH, COUGH_POLL_MS);
        } else {
            self.on_toggle();
        }
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

    /// Explicit set (not toggle) for the CLI; indicators follow via the normal WM_APP_MUTE path.
    fn remote_mute(&mut self, muted: bool) -> bool {
        self.stop_cough(); // an explicit set wins over a held cough key's release
        match self.audio.set_mute(muted) {
            Ok(()) => true,
            Err(e) => {
                log!("remote: set_mute failed: {e}");
                false
            }
        }
    }

    fn on_mute(&mut self, muted: bool) {
        // Volume changes post this too; only a real state change counts.
        // A pending toggle stays pending until the real change arrives.
        if self.last_muted == Some(muted) {
            return;
        }
        self.last_muted = Some(muted);
        LAST_STATUS.store(status_code(self.last_muted), Ordering::Relaxed);
        self.ind.show(self.hwnd, muted, true);
        self.log_latency();
        if muted {
            self.ind.raise(); // off the measured path; lands above late topmost windows
        }
        self.update_tray();
        if self.cfg.sound_enabled {
            sound::play(muted);
        }
        self.update_talk_watch();
        crate::stats::mute_state(muted);
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
        self.stop_cough();
        hotkey::unregister(self.hwnd, HOTKEY_ID);
        self.ind.destroy();
        self.others.destroy();
        self.prompt = None;
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
    }

    fn on_update(&mut self, manual: bool) {
        match UPDATE.lock().unwrap_or_else(|e| e.into_inner()).take() {
            // Refuse non-GitHub URLs before handing them to the shell.
            Some(r) if is_release_url(&r.url) => {
                let body = t(self.lang, Key::UpdateAvailable).replace("{ver}", &r.tag);
                self.balloon(&body);
                self.pending_balloon = Some(Pending::OpenUrl(r.url));
            }
            Some(r) => log!("update: unexpected url {}", r.url),
            None if manual => self.balloon(t(self.lang, Key::UpToDate)),
            None => {}
        }
    }

    fn set_hotkey_paused(&mut self, paused: bool, pid: u32) {
        if paused == self.hotkey_paused {
            return; // duplicate pause/resume: re-registering would fail
        }
        self.hotkey_paused = paused;
        if paused {
            self.stop_cough();
            hotkey::unregister(self.hwnd, HOTKEY_ID);
            self.settings_proc =
                (pid != 0).then(|| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).ok() });
            self.arm(TIMER_SETTINGS_WATCH, 1000);
        } else {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_SETTINGS_WATCH);
                if let Some(Some(h)) = self.settings_proc.take() {
                    let _ = CloseHandle(h);
                }
            }
            self.settings_proc = None;
            self.register_hotkey();
        }
    }

    fn reload(&mut self) {
        self.last_reload_code = 0;
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
        self.stop_cough();
        self.cfg = new;
        self.can_save = true;
        self.lang = i18n::resolve(self.cfg.language);
        if ch.hotkey || self.hotkey_err.is_some() {
            hotkey::unregister(self.hwnd, HOTKEY_ID);
            self.register_hotkey();
        }
        self.last_reload_code = reload_reply_code(self.hotkey_err.is_some());
        if ch.mic {
            self.rewatch();
        }
        if ch.indicators {
            self.rebuild_indicators(); // also rebuilds the other-mic pills
        } else if ch.others {
            self.rebuild_others();
        } else if ch.mic {
            self.rewatch_others();
        }
        if ch.guard {
            self.enforcer.reset();
            self.guard_check();
        }
        if ch.autostart {
            self.apply_autostart();
        }
        if ch.talk {
            self.update_talk_watch();
        }
        self.update_tray();
    }

    fn handle(&mut self, m: u32, wp: WPARAM, lp: LPARAM) {
        let r = reg();
        match m {
            WM_HOTKEY => self.on_hotkey(),
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
                    TIMER_POPUP_HIDE => {
                        unsafe {
                            let _ = KillTimer(Some(self.hwnd), TIMER_MUTE_CLOCK);
                        }
                        self.ind.hide_popup();
                    }
                    TIMER_TALK => self.tick_talk(),
                    TIMER_COUGH => self.tick_cough(),
                    TIMER_PULSE => self.ind.tick_pulse(self.hwnd),
                    TIMER_MUTE_CLOCK => self.ind.tick_clock(self.hwnd),
                    TIMER_DEVICES => {
                        self.devices_armed = false;
                        self.rewatch(); // also refreshes the talk watch
                        self.rewatch_others();
                        self.guard_check();
                    }
                    TIMER_GUARD_RESUME => self.guard_check(),
                    TIMER_DRIFT_SNOOZE if self.drift == Drift::Snoozed => {
                        self.drift = Drift::Clear;
                        self.guard_check();
                    }
                    TIMER_DISPLAY => self.rebuild_indicators(),
                    TIMER_SETTINGS_WATCH if self.hotkey_paused => {
                        let gone = match (self.settings_proc, self.settings_child.as_mut()) {
                            (Some(None), _) => true,
                            (Some(Some(h)), _) => {
                                (unsafe { WaitForSingleObject(h, 0) } == WAIT_OBJECT_0)
                            }
                            (None, None) => true,
                            (None, Some(c)) => !matches!(c.try_wait(), Ok(None)),
                        };
                        if gone {
                            self.set_hotkey_paused(false, 0);
                        } else {
                            self.arm(TIMER_SETTINGS_WATCH, 1000);
                        }
                    }
                    _ => {}
                }
            }
            WM_APP_TRAY => self.on_tray(lp),
            WM_APP_DRIFT_CHOICE => self.on_drift_choice(wp.0),
            WM_APP_PILL_CLICK if wp.0 == 0 => {
                self.stop_cough(); // the click wins over a held cough key's release
                if self.last_muted == Some(true)
                    && let Err(e) = self.audio.set_mute(false)
                {
                    log!("pill click unmute failed: {e}");
                }
            }
            WM_APP_PILL_CLICK if wp.0 == 1 => {
                if let Some(id) = other_click_target(&self.others_shown, lp.0 as usize)
                    && let Err(e) = self.audio.set_other_mute(id, false)
                {
                    log!("other pill click unmute failed: {e}");
                }
            }
            WM_APP_OTHERS => self.refresh_others(),
            WM_APP_UPDATE => self.on_update(wp.0 != 0),
            _ if m == r.toggle => self.on_toggle(),
            _ if m == r.remote => {
                if let Some(mute) = remote_action(wp.0) {
                    self.remote_mute(mute);
                }
            }
            _ if m == r.reload => self.reload(),
            _ if m == r.pause => self.set_hotkey_paused(wp.0 != 0, lp.0 as u32),
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
            | WM_APP_DRIFT_CHOICE
            | WM_APP_PILL_CLICK
            | WM_APP_OTHERS
    ) || (m != 0 && [r.reload, r.show, r.toggle, r.pause, r.taskbar].contains(&m))
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut code = 0;
    if m != 0 && m == reg().remote {
        return LRESULT(on_remote(m, wp, lp));
    }
    if is_ours(m)
        && with_state(Some((m, wp, lp)), |s| {
            s.handle(m, wp, lp);
            code = s.last_reload_code; // ponytail: deferred reload replies 0 (settings treats it as ok)
        })
    {
        // WM_CLOSE / WM_ENDSESSION return 0 (handled) so DefWindowProc doesn't destroy first.
        if m == reg().reload {
            return LRESULT(code);
        }
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
            w!("SirahMute"),
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
            let legacy = config::legacy_dir().join("settings.json");
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
        let _ = CreateMutexW(None, false, w!("Local\\SirahMute.Core"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            post_to_core(MSG_SHOW_SETTINGS);
            return 0;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    log!("start v{VERSION}");
    crate::stats::start();
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
        others: OtherPills::new(),
        others_shown: Vec::new(),
        enforcer: Enforcer::new(),
        last_muted: None,
        pending_toggle_at: None,
        cough_since: None,
        pending_balloon: None,
        drift: Drift::Clear,
        prompt: None,
        hotkey_err: None,
        devices_armed: false,
        settings_child: None,
        settings_proc: None,
        hotkey_paused: false,
        last_reload_code: 0,
        talk_meter: None,
        talk: TalkDetector::new(),
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
    crate::stats::flush();
    log::write_sync("exit");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_word_maps() {
        assert_eq!(status_word(Some(REPLY_BASE)), ("live", 0));
        assert_eq!(status_word(Some(REPLY_BASE + 1)), ("muted", 1));
        assert_eq!(status_word(Some(REPLY_BASE + 2)), ("no_mic", 2));
        assert_eq!(status_word(None), ("not_running", 3));
        assert_eq!(status_word(Some(REPLY_BASE + 7)), ("not_running", 3));
        // an older core leaves the message to DefWindowProc, which returns 0
        assert_eq!(status_word(Some(0)), ("not_running", 3));
        assert_eq!(status_word(Some(1)), ("not_running", 3));
    }

    #[test]
    fn remote_action_maps() {
        assert_eq!(remote_action(0), None);
        assert_eq!(remote_action(1), Some(true));
        assert_eq!(remote_action(2), Some(false));
        assert_eq!(remote_action(3), None);
        assert_eq!(remote_action(usize::MAX), None);
    }

    #[test]
    fn status_code_from_cache() {
        assert_eq!(status_code(None), 2);
        assert_eq!(status_code(Some(true)), 1);
        assert_eq!(status_code(Some(false)), 0);
    }

    #[test]
    fn drift_asks_once_per_episode() {
        assert_eq!(drift_after_check(Drift::Clear, true), (Drift::Asking, true));
        assert_eq!(
            drift_after_check(Drift::Asking, true),
            (Drift::Asking, false)
        );
        assert_eq!(drift_after_check(Drift::Kept, true), (Drift::Kept, false));
        assert_eq!(
            drift_after_check(Drift::Snoozed, true),
            (Drift::Snoozed, false)
        );
        // Defaults match again (or guard off / AutoRevert): episode over, next drift asks again.
        for d in [Drift::Asking, Drift::Kept, Drift::Snoozed] {
            assert_eq!(drift_after_check(d, false), (Drift::Clear, false));
        }
        assert_eq!(drift_after_choice(Choice::SwitchBack), Drift::Kept);
        assert_eq!(drift_after_choice(Choice::Keep), Drift::Kept);
        assert_eq!(drift_after_choice(Choice::Snooze), Drift::Snoozed);
    }

    #[test]
    fn only_github_release_urls_open() {
        assert!(is_release_url("https://github.com/a/b/releases/tag/v1"));
        assert!(!is_release_url("http://github.com/a"));
        assert!(!is_release_url("https://github.com.evil.com/a"));
        assert!(!is_release_url("file:///c:/x.exe"));
        assert!(!is_release_url(""));
    }

    #[test]
    fn reload_reply_code_maps_hotkey_state() {
        assert_eq!(reload_reply_code(false), 0);
        assert_eq!(reload_reply_code(true), 1);
    }

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
    fn talk_settings_flag_talk_only() {
        let a = Config::default();
        let b = Config {
            talk_warn_mic_id: Some("{x}".into()),
            ..a.clone()
        };
        assert_eq!(
            diff(&a, &b),
            Changes {
                talk: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn diff_border_style_only_indicators() {
        let a = Config::default();
        let b = Config {
            border_style: crate::config::BorderStyle::Solid,
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
    fn diff_other_mics_flags_others() {
        let a = Config::default();
        let b = Config {
            other_mics_position: crate::config::Position::TopRight,
            ..a.clone()
        };
        assert_eq!(
            diff(&a, &b),
            Changes {
                others: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn diff_cough_flags_cough() {
        let a = Config::default();
        let b = Config {
            cough_hold_ms: 900,
            ..a.clone()
        };
        assert_eq!(
            diff(&a, &b),
            Changes {
                cough: true,
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
    fn click_index_out_of_range_is_ignored() {
        let shown = ["a".to_string(), "b".to_string()];
        assert_eq!(other_click_target(&shown, 3), None);
        assert_eq!(other_click_target(&shown, 1), Some("b"));
        assert_eq!(other_click_target(&[], 0), None);
    }

    #[test]
    fn diff_identical_nothing() {
        let a = Config::default();
        assert_eq!(diff(&a, &a.clone()), Changes::default());
    }
}
