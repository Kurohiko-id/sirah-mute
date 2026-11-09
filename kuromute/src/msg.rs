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

/// Names for RegisterWindowMessageW.
pub const MSG_RELOAD: PCWSTR = w!("KuroMute.Reload");
pub const MSG_SHOW_SETTINGS: PCWSTR = w!("KuroMute.ShowSettings");
pub const MSG_TOGGLE: PCWSTR = w!("KuroMute.Toggle");
/// wparam: 1 = pause (unregister hotkey), 0 = resume.
pub const MSG_PAUSE_HOTKEY: PCWSTR = w!("KuroMute.PauseHotkey");

pub const WINDOW_CLASS: PCWSTR = w!("KuroMuteCore");
