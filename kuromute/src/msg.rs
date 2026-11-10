use windows::Win32::UI::WindowsAndMessaging::WM_APP;
use windows::core::{PCWSTR, w};

/// wparam: 1 muted, 0 unmuted.
pub const WM_APP_MUTE: u32 = WM_APP + 1;
pub const WM_APP_DEVICES: u32 = WM_APP + 2;
/// Result is in `app::UPDATE` (not lparam); wparam 1 = manual check.
pub const WM_APP_UPDATE: u32 = WM_APP + 3;
#[allow(dead_code)] // reserved; set_default workers log their own result
pub const WM_APP_DEFAULTS_SET: u32 = WM_APP + 4;
pub const WM_APP_TRAY: u32 = WM_APP + 5;
/// From the drift prompt; wparam: `prompt::Choice`.
pub const WM_APP_DRIFT_CHOICE: u32 = WM_APP + 6;

/// Names for RegisterWindowMessageW.
pub const MSG_RELOAD: PCWSTR = w!("SirahMute.Reload");
pub const MSG_SHOW_SETTINGS: PCWSTR = w!("SirahMute.ShowSettings");
pub const MSG_TOGGLE: PCWSTR = w!("SirahMute.Toggle");
/// wparam: 1 = pause (unregister hotkey), 0 = resume.
/// lparam (pause only): settings process ID to watch, 0 = unknown (core falls back to its own child).
pub const MSG_PAUSE_HOTKEY: PCWSTR = w!("SirahMute.PauseHotkey");

pub const WINDOW_CLASS: PCWSTR = w!("SirahMuteCore");
