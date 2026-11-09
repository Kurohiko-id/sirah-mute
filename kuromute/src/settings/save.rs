use crate::config::{self, Config};
use std::path::Path;

#[derive(PartialEq, Debug)]
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

/// `send` returns `Some((code, timed_out))` when the core window was found, else `None`.
pub fn save_and_reload(
    startup_load_ok: bool,
    path: &Path,
    cfg: &Config,
    send: impl FnOnce() -> Option<(isize, bool)>,
) -> SaveOutcome {
    if !startup_load_ok {
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
    fn reply_mapping() {
        assert_eq!(map_reply(Some(0), false), CoreReply::Ok);
        assert_eq!(map_reply(Some(1), false), CoreReply::HotkeyFailed);
        assert_eq!(map_reply(Some(0), true), CoreReply::Unknown);
        assert_eq!(map_reply(None, false), CoreReply::NotRunning);
    }
}
