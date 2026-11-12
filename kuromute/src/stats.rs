//! Anonymous usage stats to dash.kurohiko.id (see "Privacy" in README).
//! Same contract as Kuro Stats' `client/kuro-stats.cjs`: install / update / daily ping,
//! features in use (once a day each), panics. Installed builds only, always on, no toggle.

use crate::config::{BorderStyle, Config, GuardMode, Hotkey, PopupStyle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::core::{GUID, w};

const HOST: &str = "dash.kurohiko.id";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    uid: String,
    installed: u8,
    ver: Option<String>,
    day: Option<String>,
    /// Mute totals for `uday`, sent as buckets once the day is over.
    uday: Option<String>,
    mutes: u32,
    muted_s: u64,
    /// UTC day of `fsent`: features already sent that day (kept across restarts).
    fday: Option<String>,
    fsent: HashSet<String>,
    #[serde(skip)]
    since: Option<Instant>,
}

struct Base {
    uid: String,
    os: String,
    arch: &'static str,
    lang: String,
}

static BASE: OnceLock<Base> = OnceLock::new();
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static ST: OnceLock<Mutex<State>> = OnceLock::new();

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let mut g = ST.get()?.lock().ok()?;
    Some(f(&mut g))
}
/// Serialises under the lock, writes outside it (the mute path must not wait on disk).
/// Atomic, and one writer at a time (stats thread and core exit share the tmp file).
fn persist() {
    static DISK: Mutex<()> = Mutex::new(());
    let Ok(_g) = DISK.lock() else { return };
    if let Some(b) = with_state(|st| serde_json::to_vec(st).unwrap_or_default()) {
        let _ = crate::config::write_atomic(&state_path(), &b);
    }
}

fn state_path() -> PathBuf {
    crate::config::data_dir().join("kuro-stats.json")
}
fn load_state() -> State {
    std::fs::read(state_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Release builds outside a cargo `target` folder (installer / portable zip).
fn enabled() -> bool {
    !cfg!(debug_assertions)
        && std::env::current_exe().is_ok_and(|p| {
            !p.components()
                .any(|c| c.as_os_str().to_string_lossy().ends_with("target"))
        })
}

fn os_name(build: u32) -> &'static str {
    if build >= 22000 {
        "Windows 11"
    } else {
        "Windows 10"
    }
}

fn windows_build() -> u32 {
    let mut buf = [0u16; 32];
    let mut len = std::mem::size_of_val(&buf) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    }
    .is_ok();
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).min(buf.len())]);
    if ok {
        s.trim_end_matches('\0').parse().unwrap_or(0)
    } else {
        0
    }
}

fn node_arch(rust: &'static str) -> &'static str {
    match rust {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        a => a,
    }
}

fn locale() -> String {
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    let s = String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1).min(buf.len())]);
    s.chars().take(16).collect()
}

fn today() -> String {
    let t = unsafe { GetSystemTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

fn new_uid() -> String {
    GUID::new()
        .map(|g| format!("{g:?}").to_lowercase())
        .unwrap_or_default()
}

/// Creates the random id once. No-op (and no file written) in dev builds.
pub fn init() {
    if !enabled() {
        return;
    }
    let mut st = load_state();
    let fresh = st.uid.is_empty();
    if fresh {
        st.uid = new_uid();
        if st.uid.is_empty() {
            return;
        }
    }
    let _ = BASE.set(Base {
        uid: st.uid.clone(),
        os: os_name(windows_build()).into(),
        arch: node_arch(std::env::consts::ARCH),
        lang: locale(),
    });
    let _ = ST.set(Mutex::new(st));
    if fresh {
        persist();
    }
}

/// POSTs one event; true on 2xx. Silent on every failure.
fn send(ev: &str, props: Value) -> bool {
    let Some(b) = BASE.get() else { return false };
    let body = json!({
        "app": "sirah-mute", "ev": ev, "uid": b.uid, "ver": VERSION,
        "os": b.os, "arch": b.arch, "lang": b.lang, "props": props,
    });
    let body = serde_json::to_vec(&body).unwrap_or_default();
    matches!(
        crate::http::request(
            "POST",
            HOST,
            "/e",
            "content-type: application/json",
            Some(&body),
            8000
        ),
        Ok((200..=299, _))
    )
}

/// Core only: install/update once, then ping + features every hour on a background thread.
pub fn start() {
    if BASE.get().is_none() {
        return;
    }
    std::thread::spawn(|| {
        let Some((fresh, old)) = with_state(|st| {
            let old = st.ver.clone().filter(|v| v != VERSION);
            (st.installed == 0, old)
        }) else {
            return;
        };
        let (mut installed, mut ok) = (false, true);
        if fresh {
            installed = send("install", json!({}));
        } else if let Some(old) = old {
            ok = send("update", json!({ "from": old })); // offline: retried next launch
        }
        with_state(|st| {
            if installed {
                st.installed = 1;
            }
            if ok {
                st.ver = Some(VERSION.into());
            }
        });
        persist();
        loop {
            let day = today();
            let need_ping = with_state(|st| st.day.as_deref() != Some(&day)) == Some(true);
            if need_ping && send("ping", json!({})) {
                with_state(|st| st.day = Some(day.clone()));
                persist();
            }
            let done = with_state(|st| roll_usage(st, &day, Instant::now())).flatten();
            persist();
            if let Some((mutes, secs)) = done {
                for e in usage_events(mutes, secs) {
                    send(&e, json!({}));
                }
            }
            if let Some(cfg) = read_config() {
                // once per feature per day; a failed send is not retried that day
                let new = with_state(|st| unsent_features(st, &day, features(&cfg)));
                persist();
                for f in new.unwrap_or_default() {
                    send(&f, json!({}));
                }
            }
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

/// Core: the watched mic's mute state (repeats are ignored). Counts mutes and muted time.
pub fn mute_state(muted: bool) {
    with_state(|st| match (muted, st.since) {
        (true, None) => {
            st.since = Some(Instant::now());
            st.mutes += 1;
        }
        (false, Some(t)) => {
            st.muted_s += t.elapsed().as_secs();
            st.since = None;
        }
        _ => {}
    });
}

/// Core exit: keep today's totals for the next launch.
pub fn flush() {
    with_state(|st| {
        if let Some(t) = st.since.replace(Instant::now()) {
            st.muted_s += t.elapsed().as_secs();
        }
    });
    persist();
}

/// Features in `feats` not yet sent on `day`, marked as sent. The set starts over each day.
fn unsent_features(st: &mut State, day: &str, feats: Vec<String>) -> Vec<String> {
    if st.fday.as_deref() != Some(day) {
        st.fday = Some(day.into());
        st.fsent.clear();
    }
    feats
        .into_iter()
        .filter(|f| st.fsent.insert(f.clone()))
        .collect()
}

/// Folds the running mute into the total; on a new (UTC) day returns the finished day's
/// (mutes, seconds) when there was at least one mute, and starts the new day.
// ponytail: hourly tick, so up to an hour after UTC midnight still counts for the old day
fn roll_usage(st: &mut State, day: &str, now: Instant) -> Option<(u32, u64)> {
    if let Some(t) = st.since {
        st.muted_s += now.saturating_duration_since(t).as_secs();
        st.since = Some(now);
    }
    let prev = st.uday.replace(day.into())?;
    if prev == day {
        return None;
    }
    let done = (st.mutes, st.muted_s);
    st.mutes = st.since.is_some() as u32; // still muted: carries into the new day
    st.muted_s = 0;
    Some(done).filter(|(m, _)| *m > 0)
}

/// Daily totals as coarse buckets, never raw numbers.
fn usage_events(mutes: u32, secs: u64) -> [String; 2] {
    let m = match mutes {
        0..=5 => "1-5",
        6..=20 => "6-20",
        21..=50 => "21-50",
        _ => "51plus",
    };
    let d = match secs {
        0..300 => "0-5m",
        300..1800 => "5-30m",
        1800..7200 => "30-120m",
        _ => "2hplus",
    };
    [format!("f_mutes_{m}"), format!("f_muted_{d}")]
}

/// Read-only: never renames a corrupt file like `config::load` does.
fn read_config() -> Option<Config> {
    crate::config::read(&crate::config::config_path())
        .ok()
        .flatten()
}

/// Features currently switched on. Only setting names, never ids or names from the config.
pub fn features(c: &Config) -> Vec<String> {
    let mut f = Vec::new();
    let mut on = |b: bool, n: &str| {
        if b {
            f.push(format!("f_{n}"));
        }
    };
    on(
        c.popup_enabled && c.popup_style == PopupStyle::Card,
        "popup_card",
    );
    on(
        c.popup_enabled && c.popup_style == PopupStyle::Pill,
        "popup_pill",
    );
    on(
        c.popup_enabled && c.popup_persist_while_muted,
        "popup_persist",
    );
    let pos = format!("pos_{:?}", c.popup_position).to_lowercase();
    on(c.popup_enabled, &pos);
    on(
        c.border_enabled && c.border_style == BorderStyle::Glow,
        "border_glow",
    );
    on(
        c.border_enabled && c.border_style == BorderStyle::Solid,
        "border_solid",
    );
    on(c.border_enabled && !c.all_monitors, "border_some_monitors");
    on(c.sound_enabled, "sound");
    on(c.force_on_startup, "force_on_startup");
    on(
        c.guard_enabled && c.guard_mode == GuardMode::Notify,
        "guard_notify",
    );
    on(
        c.guard_enabled && c.guard_mode == GuardMode::AutoRevert,
        "guard_revert",
    );
    on(c.autostart, "autostart");
    on(c.talk_warn_enabled, "talk_warn");
    on(c.cough_hold, "cough_hold");
    on(c.other_mics.iter().any(|m| m.enabled), "other_mics");
    on(c.hotkey != Hotkey::default(), "hotkey_custom");
    on(c.mic_id.is_some(), "mic_pinned");
    let lang = format!("lang_{:?}", c.language).to_lowercase();
    on(true, &lang);
    f
}

fn scrub(s: &str, home: &str, user: &str) -> String {
    let mut s = s.to_string();
    if !home.is_empty() {
        s = s.replace(home, "~");
    }
    if !user.is_empty() {
        s = s.replace(user, "<user>");
    }
    s
}

/// Panic report: message + source location, user folder masked, same message once, max 10 per session.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        prev(info);
        if BASE.get().is_none() {
            return;
        }
        let p = info.payload();
        let msg = p
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        let at = info.location().map(|l| l.to_string()).unwrap_or_default();
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        let user = std::env::var("USERNAME").unwrap_or_default();
        let msg: String = scrub(&msg, &home, &user).chars().take(300).collect();
        let stack: String = scrub(&at, &home, &user).chars().take(600).collect();
        {
            let Ok(mut seen) = ERRORS.lock() else { return };
            let key: String = msg.chars().take(100).collect();
            if seen.len() >= 10 || seen.contains(&key) {
                return;
            }
            seen.push(key);
        }
        send("error", json!({ "msg": msg, "stack": stack }));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_ev(s: &str) -> bool {
        let mut c = s.chars();
        c.next().is_some_and(|c| c.is_ascii_lowercase())
            && s.len() <= 32
            && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c))
    }

    #[test]
    fn features_never_carry_user_data() {
        use crate::config::{Lang, Position};
        let secret = "secret-device-abc";
        for (border, style, guard, pos, lang) in [
            (
                true,
                PopupStyle::Pill,
                GuardMode::Notify,
                Position::TopLeft,
                Lang::Id,
            ),
            (
                true,
                PopupStyle::Card,
                GuardMode::AutoRevert,
                Position::BottomRight,
                Lang::Auto,
            ),
        ] {
            let c = Config {
                mic_id: Some(secret.into()),
                default_input_id: Some(secret.into()),
                talk_warn_mic_id: Some(secret.into()),
                monitor_ids: vec![secret.into()],
                border_enabled: border,
                all_monitors: false,
                popup_style: style,
                popup_persist_while_muted: true,
                guard_mode: guard,
                popup_position: pos,
                language: lang,
                sound_enabled: true,
                talk_warn_enabled: true,
                hotkey: Hotkey { mods: 0, vk: 124 },
                cough_hold: true,
                other_mics: vec![crate::config::OtherMic {
                    id: "{0.0.1}.{secret}".into(),
                    name: "PD400X".into(),
                    enabled: true,
                }],
                ..Config::default()
            };
            let f = features(&c);
            assert!(f.len() > 10, "{f:?}");
            assert!(
                f.contains(&"f_other_mics".to_string()) && f.contains(&"f_cough_hold".to_string())
            );
            for e in &f {
                assert!(e.starts_with("f_") && valid_ev(e), "bad event name {e}");
                assert!(
                    !e.contains("secret") && !e.contains("124") && !e.contains("PD400X"),
                    "user data in {e}"
                );
            }
        }
        assert!(features(&Config::default()).contains(&"f_pos_bottomcenter".to_string()));
    }

    #[test]
    fn contract_fields() {
        assert_eq!(os_name(22631), "Windows 11");
        assert_eq!(os_name(19045), "Windows 10");
        assert_eq!(node_arch("x86_64"), "x64");
        assert_eq!(node_arch("aarch64"), "arm64");
        assert_eq!(node_arch("x86"), "ia32");
        assert!(windows_build() >= 10240);
        let l = locale();
        assert!(l.contains('-') && !l.contains('_') && l.len() <= 16, "{l}");
        assert_eq!(today().len(), 10);
        let u = new_uid();
        assert!(
            u.len() == 36 && u.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "{u}"
        );
    }

    #[test]
    fn scrub_masks_home_then_user() {
        let s = scrub(
            r"panicked at C:\Users\kaito\src\a.rs (kaito)",
            r"C:\Users\kaito",
            "kaito",
        );
        assert_eq!(s, r"panicked at ~\src\a.rs (<user>)");
    }

    #[test]
    fn usage_buckets() {
        let ev = |m, s| usage_events(m, s).to_vec();
        assert_eq!(ev(1, 0), ["f_mutes_1-5", "f_muted_0-5m"]);
        assert_eq!(ev(6, 300), ["f_mutes_6-20", "f_muted_5-30m"]);
        assert_eq!(ev(50, 7199), ["f_mutes_21-50", "f_muted_30-120m"]);
        assert_eq!(ev(51, 7200), ["f_mutes_51plus", "f_muted_2hplus"]);
        for e in ev(999, 99999) {
            assert!(valid_ev(&e), "{e}");
        }
    }

    #[test]
    fn usage_rolls_over_per_day() {
        let t0 = Instant::now();
        let mut st = State {
            mutes: 2,
            ..Default::default()
        };
        // first ever tick: adopts the day, keeps what was counted before it
        assert_eq!(roll_usage(&mut st, "2026-11-12", t0), None);
        assert_eq!(st.mutes, 2);
        // muted for 90 s across two ticks, then midnight
        st.since = Some(t0);
        st.mutes = 3;
        roll_usage(&mut st, "2026-11-12", t0 + Duration::from_secs(60));
        let done = roll_usage(&mut st, "2026-11-13", t0 + Duration::from_secs(90));
        assert_eq!(done, Some((3, 90)));
        assert_eq!((st.mutes, st.muted_s), (1, 0)); // still muted -> counts for the new day
        // a day without any mute sends nothing
        st.since = None;
        st.mutes = 0;
        assert_eq!(roll_usage(&mut st, "2026-11-14", t0), None);
    }

    #[test]
    fn features_sent_once_per_day() {
        let mut st = State::default();
        let f = || vec!["f_sound".to_string(), "f_autostart".to_string()];
        assert_eq!(unsent_features(&mut st, "2026-11-12", f()).len(), 2);
        // a restart reloads the saved state: nothing again the same day
        let mut st: State = serde_json::from_slice(&serde_json::to_vec(&st).unwrap()).unwrap();
        assert!(unsent_features(&mut st, "2026-11-12", f()).is_empty());
        // next day: sent again, and the old day's entries are dropped
        assert_eq!(unsent_features(&mut st, "2026-11-13", f()).len(), 2);
        assert_eq!(st.fsent.len(), 2);
    }

    #[test]
    fn dev_build_is_disabled() {
        assert!(!enabled());
    }
}
