use crate::config::Hotkey;
use crate::hotkey::{self, HotkeyError};

/// Removes duplicates only; unknown IDs (unplugged monitors) stay.
pub fn retain_known(saved: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(saved.len());
    for s in saved {
        if !out.contains(s) {
            out.push(s.clone());
        }
    }
    out
}

pub fn hotkey_choice(mods: u32, vk: u32) -> Result<Hotkey, HotkeyError> {
    let h = Hotkey { mods, vk };
    hotkey::validate(&h).map(|()| h)
}

pub fn hotkey_warning(h: &Hotkey) -> bool {
    hotkey::leaks_into_games(h)
}

/// Dropdown index 0 = "don't change" (None); otherwise the device at `index - 1`.
pub fn default_from_index(index: usize, ids: &[String]) -> Option<String> {
    index.checked_sub(1).and_then(|i| ids.get(i).cloned())
}

/// Ready once `ms` has passed since the last bump; never ready before the first bump.
pub struct Debouncer {
    ms: u64,
    last: Option<u64>,
}

impl Debouncer {
    pub fn new(ms: u64) -> Self {
        Debouncer { ms, last: None }
    }

    pub fn bump(&mut self, now_ms: u64) {
        self.last = Some(now_ms);
    }

    pub fn ready(&self, now_ms: u64) -> bool {
        self.last.is_some_and(|l| now_ms.saturating_sub(l) >= self.ms)
    }
}

/// Outcome of "Cek update"; `Ok(true)` = a newer release exists.
#[derive(PartialEq, Debug)]
pub enum UpdateStatus {
    Available,
    UpToDate,
    Failed,
}

pub fn update_status(r: Result<bool, String>) -> UpdateStatus {
    match r {
        Ok(true) => UpdateStatus::Available,
        Ok(false) => UpdateStatus::UpToDate,
        Err(_) => UpdateStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_status_maps() {
        assert_eq!(update_status(Ok(true)), UpdateStatus::Available);
        assert_eq!(update_status(Ok(false)), UpdateStatus::UpToDate);
        assert_eq!(update_status(Err("x".into())), UpdateStatus::Failed);
    }

    #[test]
    fn unplugged_monitor_is_kept() {
        let saved = vec!["A".to_string(), "GONE".to_string()];
        assert_eq!(retain_known(&saved), saved);
    }

    #[test]
    fn bare_f12_rejected() {
        assert!(hotkey_choice(0, 0x7B).is_err());
        assert!(hotkey_choice(2, 0x7B).is_ok());
    }

    #[test]
    fn debouncer_waits_for_quiet() {
        let mut d = Debouncer::new(300);
        d.bump(0);
        d.bump(100);
        assert!(!d.ready(300));
        assert!(d.ready(400));
    }

    #[test]
    fn thickness_clamped_by_sanitize() {
        let c = crate::config::Config {
            border_thickness: 99,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(c.border_thickness, 30);
    }

    #[test]
    fn scale_value_kept_inside_range() {
        let c = crate::config::Config {
            popup_scale: 1.7,
            ..Default::default()
        }
        .sanitized();
        assert!((c.popup_scale - 1.7).abs() < 1e-6);
    }

    #[test]
    fn dropdown_index_maps_to_none() {
        let ids = vec!["X".to_string()];
        assert_eq!(default_from_index(0, &ids), None);
        assert_eq!(default_from_index(1, &ids), Some("X".to_string()));
    }
}
