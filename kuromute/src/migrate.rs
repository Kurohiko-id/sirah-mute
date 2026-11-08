use crate::config::{Config, GuardMode, Hotkey, Position};
use serde_json::Value;

const CAPTURE: &str = "{0.0.1.00000000}.";
const RENDER: &str = "{0.0.0.00000000}.";

fn guid(v: &Value, prefix: &str) -> Option<String> {
    let g = v
        .as_str()?
        .trim()
        .trim_matches(|c| c == '{' || c == '}')
        .to_ascii_lowercase();
    let ok = g.len() == 36
        && g.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    ok.then(|| format!("{prefix}{{{g}}}"))
}

fn position(v: &Value) -> Position {
    use Position::*;
    const ALL: [Position; 6] = [
        TopLeft,
        TopCenter,
        TopRight,
        BottomLeft,
        BottomCenter,
        BottomRight,
    ];
    match v {
        Value::Number(n) => n.as_u64().and_then(|i| ALL.get(i as usize).copied()),
        Value::String(s) => ALL
            .iter()
            .copied()
            .find(|p| format!("{p:?}").eq_ignore_ascii_case(s)),
        _ => None,
    }
    .unwrap_or(BottomCenter)
}

pub fn from_legacy(json: &str) -> Result<Config, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let o = root
        .as_object()
        .ok_or("legacy settings is not a JSON object")?;
    let get = |k: &str| o.get(k).unwrap_or(&Value::Null);
    let b = |k: &str, d: bool| get(k).as_bool().unwrap_or(d);
    let u = |k: &str, d: u64| get(k).as_u64().unwrap_or(d);
    let d = Config::default();
    let mods = (u("HotkeyModifiers", 3) & !0x4000) as u32;
    Ok(Config {
        mic_id: guid(get("MicDeviceId"), CAPTURE),
        default_input_id: guid(get("DefaultInputId"), CAPTURE),
        default_output_id: guid(get("DefaultOutputId"), RENDER),
        hotkey: Hotkey {
            mods,
            vk: u("HotkeyKey", 0x4D) as u32,
        },
        popup_enabled: b("ShowToastOverlay", true),
        popup_position: position(get("OverlayPosition")),
        popup_scale: get("OverlayScale").as_f64().unwrap_or(1.0) as f32,
        popup_persist_while_muted: b("OverlayPersistWhileMuted", false),
        border_enabled: b("ShowBorderGlow", false),
        border_thickness: u("BorderThickness", 6).min(u32::MAX as u64) as u32,
        all_monitors: b("OverlayAllMonitors", true),
        monitor_ids: get("OverlayMonitorIds").as_array().map_or(vec![], |a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        }),
        autostart: b("AutoStart", true),
        force_on_startup: b("ForceAudioOnStartup", true),
        guard_mode: if b("WatchAudioChanges", false) {
            GuardMode::AutoRevert
        } else {
            GuardMode::Off
        },
        ..d
    }
    .sanitized())
}

#[cfg(test)]
mod tests {
    use super::*;
    const USER_LIKE: &str = r#"{"MicDeviceId":"9209C960-E1BA-46A3-AFDA-BAF917ACC5A7","DefaultInputId":"4a0acd44-063c-41d1-b6ee-d6b352787b66",
 "DefaultOutputId":"6da89e19-3fb7-4116-809b-d00295787133","HotkeyModifiers":2,"HotkeyKey":123,"ShowToastOverlay":false,
 "ShowBorderGlow":true,"OverlayPersistWhileMuted":true,"OverlayPosition":4,"OverlayScale":1.24,"AutoStart":true,
 "ForceAudioOnStartup":true,"WatchAudioChanges":true,"BorderThickness":11,"OverlayAllMonitors":true,"OverlayMonitorIds":[]}"#;
    #[test]
    fn migrates_real_shape() {
        let c = from_legacy(USER_LIKE).unwrap();
        assert_eq!(
            c.mic_id.as_deref(),
            Some("{0.0.1.00000000}.{9209c960-e1ba-46a3-afda-baf917acc5a7}")
        );
        assert_eq!(
            c.default_input_id.as_deref(),
            Some("{0.0.1.00000000}.{4a0acd44-063c-41d1-b6ee-d6b352787b66}")
        );
        assert_eq!(
            c.default_output_id.as_deref(),
            Some("{0.0.0.00000000}.{6da89e19-3fb7-4116-809b-d00295787133}")
        );
        assert_eq!(c.hotkey, Hotkey { mods: 2, vk: 123 });
        assert_eq!(
            (c.popup_enabled, c.border_enabled, c.border_thickness),
            (false, true, 11)
        );
        assert_eq!(
            (c.popup_position, c.guard_mode),
            (Position::BottomCenter, GuardMode::AutoRevert)
        );
        assert!((c.popup_scale - 1.24).abs() < 1e-6);
    }
    #[test]
    fn watch_false_is_off() {
        assert_eq!(
            from_legacy(r#"{"WatchAudioChanges":false}"#)
                .unwrap()
                .guard_mode,
            GuardMode::Off
        );
    }
    #[test]
    fn empty_object_uses_old_defaults() {
        let c = from_legacy("{}").unwrap();
        assert!(c.popup_enabled && !c.border_enabled && c.mic_id.is_none());
        assert_eq!(c.guard_mode, GuardMode::Off);
    }
    #[test]
    fn position_by_name_and_norepeat_stripped() {
        let c = from_legacy(r#"{"OverlayPosition":"TopRight","HotkeyModifiers":16386}"#).unwrap();
        assert_eq!((c.popup_position, c.hotkey.mods), (Position::TopRight, 2));
    }
    #[test]
    fn garbage_is_err() {
        assert!(from_legacy("not json").is_err());
    }
    #[test]
    fn bad_or_null_guid_is_none() {
        let c = from_legacy(r#"{"MicDeviceId":"nope","DefaultInputId":null,"DefaultOutputId":""}"#)
            .unwrap();
        assert_eq!(
            (c.mic_id, c.default_input_id, c.default_output_id),
            (None, None, None)
        );
    }
    #[test]
    fn monitor_ids_kept() {
        let c =
            from_legacy(r#"{"OverlayAllMonitors":false,"OverlayMonitorIds":["\\\\.\\DISPLAY1"]}"#)
                .unwrap();
        assert!(!c.all_monitors);
        assert_eq!(c.monitor_ids, vec![r"\\.\DISPLAY1".to_string()]);
    }
}
