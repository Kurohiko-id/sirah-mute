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
    pub border_enabled: bool,
    pub border_thickness: u32,
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
            border_enabled: false,
            border_thickness: 6,
            all_monitors: true,
            monitor_ids: Vec::new(),
            sound_enabled: false,
            force_on_startup: true,
            guard_mode: GuardMode::default(),
            guard_enabled: true,
            autostart: true,
            language: Lang::default(),
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
        self
    }
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("MuteMic")
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
    match serde_json::from_slice::<Config>(&bytes) {
        Ok(c) => LoadOutcome::Loaded(c.sanitized()),
        Err(e) if e.is_io() => LoadOutcome::Unreadable(e.to_string()),
        Err(_) => backup_corrupt(path),
    }
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
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let json = serde_json::to_vec_pretty(cfg).map_err(io::Error::other)?;
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&json)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kuromute-test-{}-{}", name, std::process::id()));
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
    fn missing_file_is_missing() {
        assert_eq!(
            load(&std::env::temp_dir().join("kuromute-nonexistent/config.json")),
            LoadOutcome::Missing
        );
    }
}
