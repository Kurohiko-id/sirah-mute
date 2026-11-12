use serde::{Deserialize, Deserializer, Serialize};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Position {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum GuardMode {
    Off,
    Notify,
    #[default]
    AutoRevert,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum BorderStyle {
    Solid,
    #[default]
    Glow,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum PopupStyle {
    #[default]
    Card,
    Pill,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Lang {
    #[default]
    Auto,
    En,
    Id,
}

/// `mods` uses Win32 `MOD_*`; never stores `MOD_NOREPEAT`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Hotkey {
    pub mods: u32,
    pub vk: u32,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey {
            mods: 0x2 | 0x1,
            vk: 0x4D,
        }
    }
}

/// Unknown/invalid value -> field default instead of failing the whole file.
fn or_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(T::deserialize(d).unwrap_or_default())
}

/// Extra mic shown as a clickable pill. `name` is display-only (never sent to stats).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct OtherMic {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}
pub const MAX_OTHER_MICS: usize = 6;
pub const OTHER_NAME_MAX: usize = 16;

/// Like `or_default`, but falls back to BottomLeft (`Position::default` is BottomCenter).
/// A missing field is covered by `Config::default()` via `#[serde(default)]`.
fn other_pos_or_default<'de, D: Deserializer<'de>>(d: D) -> Result<Position, D::Error> {
    Ok(Position::deserialize(d).unwrap_or(Position::BottomLeft))
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub mic_id: Option<String>,
    pub default_input_id: Option<String>,
    pub default_output_id: Option<String>,
    pub hotkey: Hotkey,
    pub popup_enabled: bool,
    #[serde(deserialize_with = "or_default")]
    pub popup_position: Position,
    pub popup_scale: f32,
    pub popup_persist_while_muted: bool,
    #[serde(deserialize_with = "or_default")]
    pub popup_style: PopupStyle,
    pub border_enabled: bool,
    pub border_thickness: u32,
    #[serde(deserialize_with = "or_default")]
    pub border_style: BorderStyle,
    pub all_monitors: bool,
    pub monitor_ids: Vec<String>,
    pub sound_enabled: bool,
    pub force_on_startup: bool,
    #[serde(deserialize_with = "or_default")]
    pub guard_mode: GuardMode,
    pub guard_enabled: bool,
    pub autostart: bool,
    #[serde(deserialize_with = "or_default")]
    pub language: Lang,
    pub talk_warn_enabled: bool,
    pub talk_warn_mic_id: Option<String>,
    pub cough_hold: bool,
    pub cough_hold_ms: u32,
    pub other_mics: Vec<OtherMic>,
    #[serde(deserialize_with = "other_pos_or_default")]
    pub other_mics_position: Position,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: 2,
            mic_id: None,
            default_input_id: None,
            default_output_id: None,
            hotkey: Hotkey::default(),
            popup_enabled: true,
            popup_position: Position::default(),
            popup_scale: 1.0,
            popup_persist_while_muted: false,
            popup_style: PopupStyle::default(),
            border_enabled: false,
            border_thickness: 6,
            border_style: BorderStyle::default(),
            all_monitors: true,
            monitor_ids: Vec::new(),
            sound_enabled: false,
            force_on_startup: true,
            guard_mode: GuardMode::default(),
            guard_enabled: true,
            autostart: true,
            language: Lang::default(),
            talk_warn_enabled: false,
            talk_warn_mic_id: None,
            cough_hold: false,
            cough_hold_ms: 400,
            other_mics: Vec::new(),
            other_mics_position: Position::BottomLeft,
        }
    }
}

impl Config {
    pub fn sanitized(mut self) -> Config {
        self.popup_scale = if self.popup_scale.is_nan() {
            1.0
        } else {
            self.popup_scale.clamp(0.5, 2.0)
        };
        self.border_thickness = self.border_thickness.clamp(1, 30);
        self.cough_hold_ms = self.cough_hold_ms.clamp(200, 2000);
        // no duplicates, and the main mic can't also be an "other mic" row (the core skips it)
        let mut seen: std::collections::HashSet<String> = self.mic_id.iter().cloned().collect();
        self.other_mics.retain(|m| seen.insert(m.id.clone()));
        self.other_mics.truncate(MAX_OTHER_MICS);
        for m in &mut self.other_mics {
            m.name = m.name.chars().take(OTHER_NAME_MAX).collect();
        }
        self
    }
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("SirahMute")
}
/// Pre-rename data folder (v1 `settings.json` and Kuro Mute v2 `config.json`).
pub fn legacy_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("MuteMic")
}

/// First run after the rename: copies `config.json` from `old` into `new` when
/// `new` has none yet. The old folder is left alone (v1 may still read it).
pub fn adopt_legacy(old: &Path, new: &Path) -> bool {
    let (src, dst) = (old.join("config.json"), new.join("config.json"));
    if dst.exists() || !src.exists() {
        return false;
    }
    fs::create_dir_all(new).is_ok() && fs::copy(&src, &dst).is_ok()
}
pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

#[derive(PartialEq, Debug)]
pub enum LoadOutcome {
    Loaded(Config),
    Missing,
    Corrupt {
        backup: PathBuf,
    },
    /// I/O failure (locked, access denied, failed backup rename). Caller must keep its
    /// in-memory config and NOT save.
    Unreadable(String),
}

pub fn load(path: &Path) -> LoadOutcome {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return LoadOutcome::Missing,
        Err(e) => return LoadOutcome::Unreadable(e.to_string()),
    };
    match parse(&bytes) {
        Some(c) => LoadOutcome::Loaded(c),
        None => backup_corrupt(path),
    }
}

/// Read-only `load` for other readers: never renames a corrupt file (that's the core's job).
/// `Ok(None)` = missing; `Err` = unreadable or corrupt.
pub fn read(path: &Path) -> Result<Option<Config>, String> {
    match fs::read(path) {
        Ok(b) => parse(&b).map(Some).ok_or_else(|| "corrupt".into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Parses config bytes (BOM allowed). A bad value for one key resets only that key;
/// `None` only when the bytes aren't a JSON object.
pub fn parse(bytes: &[u8]) -> Option<Config> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    // ponytail: one trial deserialize per key (~30), microseconds; fine for a settings file
    let serde_json::Value::Object(user) = serde_json::from_slice(bytes).ok()? else {
        return None;
    };
    let mut merged = serde_json::to_value(Config::default()).ok()?;
    for (k, v) in user {
        let mut trial = merged.clone();
        trial[k.as_str()] = v;
        if serde_json::from_value::<Config>(trial.clone()).is_ok() {
            merged = trial;
        }
    }
    serde_json::from_value::<Config>(merged)
        .ok()
        .map(Config::sanitized)
}

fn backup_corrupt(path: &Path) -> LoadOutcome {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bad");
    let backup = path.with_file_name(name);
    // fs::rename replaces an existing target on Windows
    match fs::rename(path, &backup) {
        Ok(()) => LoadOutcome::Corrupt { backup },
        Err(e) => LoadOutcome::Unreadable(format!("backup rename failed: {e}")),
    }
}

pub fn save(path: &Path, cfg: &Config) -> io::Result<()> {
    write_atomic(
        path,
        &serde_json::to_vec_pretty(cfg).map_err(io::Error::other)?,
    )
}

/// Writes `<path>.tmp`, fsyncs, then renames over `path`, so readers never see a torn file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sirahmute-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(
            c.hotkey,
            Hotkey {
                mods: 0x3,
                vk: 0x4D
            }
        );
        assert_eq!(c.guard_mode, GuardMode::AutoRevert);
        assert_eq!((c.border_thickness, c.popup_scale), (6, 1.0));
    }
    #[test]
    fn sanitize_clamps() {
        let c = Config {
            popup_scale: 5.0,
            border_thickness: 0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!((c.popup_scale, c.border_thickness), (2.0, 1));
        let c = Config {
            popup_scale: f32::NAN,
            border_thickness: 99,
            ..Default::default()
        }
        .sanitized();
        assert_eq!((c.popup_scale, c.border_thickness), (1.0, 30));
    }
    #[test]
    fn bad_enum_field_falls_back_not_corrupt() {
        let c: Config =
            serde_json::from_str(r#"{"popup_position":"Middle","border_thickness":11,"zzz":1}"#)
                .unwrap();
        assert_eq!(
            (c.popup_position, c.border_thickness),
            (Position::BottomCenter, 11)
        );
    }
    #[test]
    fn adopt_legacy_copies_once() {
        let base = std::env::temp_dir().join(format!("sirahmute-adopt-{}", std::process::id()));
        let (old, new) = (base.join("old"), base.join("new"));
        let _ = fs::remove_dir_all(&base);
        assert!(!adopt_legacy(&old, &new)); // nothing to adopt
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("config.json"), "{}").unwrap();
        assert!(adopt_legacy(&old, &new));
        fs::write(new.join("config.json"), "{\"v\":1}").unwrap();
        assert!(!adopt_legacy(&old, &new)); // never overwrites
        assert_eq!(
            fs::read_to_string(new.join("config.json")).unwrap(),
            "{\"v\":1}"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn popup_style_defaults_to_card_for_old_configs() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.popup_style, PopupStyle::Card);
        let c: Config = serde_json::from_str(r#"{"popup_style":"Pill"}"#).unwrap();
        assert_eq!(c.popup_style, PopupStyle::Pill);
    }

    #[test]
    fn border_style_defaults_to_glow_for_old_configs() {
        let c: Config = serde_json::from_str(r#"{"border_thickness":6}"#).unwrap();
        assert_eq!(c.border_style, BorderStyle::Glow);
        let c: Config = serde_json::from_str(r#"{"border_style":"Neon"}"#).unwrap();
        assert_eq!(c.border_style, BorderStyle::Glow);
    }
    #[test]
    fn old_config_has_talk_warn_off() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.talk_warn_enabled);
        assert_eq!(c.talk_warn_mic_id, None);
    }
    #[test]
    fn save_then_load_roundtrip_and_no_tmp_left() {
        let d = tmp("rt");
        let p = d.join("config.json");
        let c = Config {
            mic_id: Some("{0.0.1}".into()),
            border_thickness: 9,
            ..Default::default()
        };
        save(&p, &c).unwrap();
        assert_eq!(load(&p), LoadOutcome::Loaded(c));
        assert!(!d.join("config.json.tmp").exists());
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn truncated_file_is_corrupt_and_backed_up() {
        let d = tmp("bad");
        let p = d.join("config.json");
        fs::write(d.join("config.json.bad"), "old").unwrap();
        fs::write(&p, "{\"mic_id\": \"{0.0.1").unwrap();
        match load(&p) {
            LoadOutcome::Corrupt { backup } => {
                assert!(backup.exists());
                assert!(backup.ends_with("config.json.bad"));
            }
            o => panic!("{o:?}"),
        }
        assert!(!p.exists());
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn stale_bad_is_replaced() {
        let d = tmp("stale");
        let p = d.join("config.json");
        fs::write(d.join("config.json.bad"), "old").unwrap();
        fs::write(&p, "{broken").unwrap();
        assert!(matches!(load(&p), LoadOutcome::Corrupt { .. }));
        assert_eq!(
            fs::read_to_string(d.join("config.json.bad")).unwrap(),
            "{broken"
        );
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn save_replaces_existing() {
        let d = tmp("over");
        let p = d.join("config.json");
        save(&p, &Config::default()).unwrap();
        let c = Config {
            border_thickness: 12,
            ..Default::default()
        };
        save(&p, &c).unwrap();
        assert_eq!(load(&p), LoadOutcome::Loaded(c));
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn load_clamps_out_of_range() {
        let d = tmp("clamp");
        let p = d.join("config.json");
        fs::write(&p, r#"{"popup_scale": 5.0}"#).unwrap();
        match load(&p) {
            LoadOutcome::Loaded(c) => assert_eq!(c.popup_scale, 2.0),
            o => panic!("{o:?}"),
        }
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn load_strips_utf8_bom() {
        let d = tmp("bom");
        let p = d.join("config.json");
        fs::write(&p, b"\xEF\xBB\xBF{\"popup_scale\": 1.5}").unwrap();
        match load(&p) {
            LoadOutcome::Loaded(c) => assert_eq!(c.popup_scale, 1.5),
            o => panic!("{o:?}"),
        }
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn bad_value_resets_only_that_key() {
        for bad in [
            r#""border_thickness": -1"#,
            r#""popup_scale": "big""#,
            r#""cough_hold_ms": 1e12"#,
            r#""hotkey": "x""#,
        ] {
            let json = format!(r#"{{"mic_id": "{{0.0.1}}", "sound_enabled": true, {bad}}}"#);
            let c = parse(json.as_bytes()).unwrap_or_else(|| panic!("{bad}"));
            assert_eq!(c.mic_id.as_deref(), Some("{0.0.1}"), "{bad}");
            assert!(c.sound_enabled, "{bad}");
            let d = Config::default();
            assert_eq!(
                (c.border_thickness, c.popup_scale, c.cough_hold_ms, c.hotkey),
                (d.border_thickness, d.popup_scale, d.cough_hold_ms, d.hotkey),
                "{bad}"
            );
        }
        let c = parse(br#"{"mic_id": 5, "border_thickness": 9}"#).unwrap();
        assert_eq!((c.mic_id, c.border_thickness), (None, 9));
        assert_eq!(parse(b"[1]"), None);
        assert_eq!(parse(b"{broken"), None);
    }
    #[test]
    fn load_keeps_good_keys_and_file() {
        let d = tmp("onebad");
        let p = d.join("config.json");
        fs::write(&p, r#"{"mic_id": "{0.0.1}", "border_thickness": -1}"#).unwrap();
        match load(&p) {
            LoadOutcome::Loaded(c) => assert_eq!(c.mic_id.as_deref(), Some("{0.0.1}")),
            o => panic!("{o:?}"),
        }
        assert!(p.exists() && !d.join("config.json.bad").exists());
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn read_never_renames_corrupt_file() {
        let d = tmp("ro");
        let p = d.join("config.json");
        assert_eq!(read(&p), Ok(None));
        fs::write(&p, "{broken").unwrap();
        assert!(read(&p).is_err());
        assert!(p.exists() && !d.join("config.json.bad").exists());
        fs::remove_dir_all(&d).unwrap();
    }
    #[test]
    fn missing_file_is_missing() {
        assert_eq!(
            load(&std::env::temp_dir().join("sirahmute-nonexistent/config.json")),
            LoadOutcome::Missing
        );
    }

    #[test]
    fn old_config_has_v21_defaults() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.cough_hold);
        assert_eq!(c.cough_hold_ms, 400);
        assert!(c.other_mics.is_empty());
        assert_eq!(c.other_mics_position, Position::BottomLeft);
        let c: Config = serde_json::from_str(r#"{"other_mics_position":"Nope"}"#).unwrap();
        assert_eq!(c.other_mics_position, Position::BottomLeft);
    }
    #[test]
    fn sanitize_cough_ms() {
        for (i, o) in [(50, 200), (9000, 2000), (400, 400)] {
            let c = Config {
                cough_hold_ms: i,
                ..Default::default()
            }
            .sanitized();
            assert_eq!(c.cough_hold_ms, o);
        }
    }
    #[test]
    fn sanitize_other_mics() {
        let ids = ["a", "a", "b", "c", "c", "d", "e", "f", "g", "h"];
        let other_mics = ids
            .iter()
            .enumerate()
            .map(|(i, id)| OtherMic {
                id: id.to_string(),
                name: if i == 0 { "x".repeat(40) } else { "n".into() },
                enabled: true,
            })
            .collect();
        let c = Config {
            other_mics,
            ..Default::default()
        }
        .sanitized();
        let got: Vec<&str> = c.other_mics.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(got, ["a", "b", "c", "d", "e", "f"]);
        let c = Config {
            mic_id: Some("b".into()),
            ..c
        }
        .sanitized();
        assert!(c.other_mics.iter().all(|m| m.id != "b"));
        assert!(c.other_mics.len() <= MAX_OTHER_MICS);
        assert!(
            c.other_mics
                .iter()
                .all(|m| m.name.chars().count() <= OTHER_NAME_MAX)
        );
    }
}
