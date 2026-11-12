use crate::config::{self, Config};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CoreReply {
    Ok,
    HotkeyFailed,
    Unknown,
    NotRunning,
}

#[derive(PartialEq, Debug)]
pub enum SaveOutcome {
    Saved(CoreReply),
    Blocked,
    WriteFailed(String),
}

pub fn map_reply(code: Option<isize>, timed_out: bool) -> CoreReply {
    match (code, timed_out) {
        (None, false) => CoreReply::NotRunning,
        (Some(0), false) => CoreReply::Ok,
        (Some(1), false) => CoreReply::HotkeyFailed,
        _ => CoreReply::Unknown,
    }
}

#[derive(PartialEq, Debug)]
pub enum StatusKey {
    None,
    CoreNotRunning,
    HotkeyFailed,
}

/// What the window shows for a core reply; a timeout (`Unknown`) is not an error.
pub fn status_key(r: CoreReply) -> StatusKey {
    match r {
        CoreReply::NotRunning => StatusKey::CoreNotRunning,
        CoreReply::HotkeyFailed => StatusKey::HotkeyFailed,
        CoreReply::Ok | CoreReply::Unknown => StatusKey::None,
    }
}

/// Per top-level key: keys this session changed (`ours` != `base`) win, every other key comes
/// from `disk`, so a change the core/tray saved meanwhile survives.
pub fn merge(base: &Config, ours: &Config, disk: &Config) -> Config {
    let (Ok(base), Ok(serde_json::Value::Object(ours_v)), Ok(mut out)) = (
        serde_json::to_value(base),
        serde_json::to_value(ours),
        serde_json::to_value(disk),
    ) else {
        return ours.clone();
    };
    for (k, v) in ours_v {
        if base.get(&k) != Some(&v) {
            out[k.as_str()] = v;
        }
    }
    serde_json::from_value(out).unwrap_or_else(|_| ours.clone())
}

/// `send` returns `Some((code, timed_out))` when the core window was found, else `None`.
/// `load_ok` = false (the file on disk couldn't be read) blocks the save.
pub fn save_and_reload(
    load_ok: bool,
    path: &Path,
    cfg: &Config,
    send: impl FnOnce() -> Option<(isize, bool)>,
) -> SaveOutcome {
    if !load_ok {
        return SaveOutcome::Blocked;
    }
    if let Err(e) = config::save(path, cfg) {
        return SaveOutcome::WriteFailed(e.to_string());
    }
    SaveOutcome::Saved(match send() {
        Some((code, timed_out)) => map_reply(Some(code), timed_out),
        None => CoreReply::NotRunning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::Path;

    #[test]
    fn blocked_when_startup_load_failed() {
        let out = save_and_reload(false, Path::new("unused.json"), &Config::default(), || None);
        assert_eq!(out, SaveOutcome::Blocked);
    }

    #[test]
    fn merge_keeps_disk_changes_to_keys_we_did_not_touch() {
        let base = Config::default();
        let ours = Config {
            sound_enabled: true,
            ..base.clone()
        };
        let disk = Config {
            guard_enabled: false, // flipped from the tray while Settings was open
            ..base.clone()
        };
        let m = merge(&base, &ours, &disk);
        assert!(m.sound_enabled && !m.guard_enabled);
        // a key we changed overrides disk; the rest still comes from disk
        let disk = Config {
            sound_enabled: false,
            border_thickness: 12,
            popup_scale: 1.5,
            ..base.clone()
        };
        let m = merge(&base, &ours, &disk);
        assert!(m.sound_enabled);
        assert_eq!((m.border_thickness, m.popup_scale), (12, 1.5));
        // nothing on disk changed: ours as is
        assert_eq!(merge(&base, &ours, &base), ours);
    }

    #[test]
    fn not_running_is_info_not_error() {
        assert_eq!(status_key(CoreReply::NotRunning), StatusKey::CoreNotRunning);
        assert_eq!(status_key(CoreReply::Unknown), StatusKey::None);
        assert_eq!(status_key(CoreReply::HotkeyFailed), StatusKey::HotkeyFailed);
    }

    #[test]
    fn reply_mapping() {
        assert_eq!(map_reply(Some(0), false), CoreReply::Ok);
        assert_eq!(map_reply(Some(1), false), CoreReply::HotkeyFailed);
        assert_eq!(map_reply(Some(0), true), CoreReply::Unknown);
        assert_eq!(map_reply(None, false), CoreReply::NotRunning);
    }
}
