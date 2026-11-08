use windows::Win32::UI::WindowsAndMessaging::WM_APP;
use windows::core::{PCWSTR, w};

/// wparam: 1 muted, 0 unmuted.
pub const WM_APP_MUTE: u32 = WM_APP + 1;
pub const WM_APP_DEVICES: u32 = WM_APP + 2;
/// lparam: `Box<Release>` as raw pointer; receiver takes ownership back.
pub const WM_APP_UPDATE: u32 = WM_APP + 3;
pub const WM_APP_DEFAULTS_SET: u32 = WM_APP + 4;
pub const WM_APP_TRAY: u32 = WM_APP + 5;

/// Names for RegisterWindowMessageW.
pub const MSG_RELOAD: PCWSTR = w!("KuroMute.Reload");
pub const MSG_SHOW_SETTINGS: PCWSTR = w!("KuroMute.ShowSettings");
pub const MSG_TOGGLE: PCWSTR = w!("KuroMute.Toggle");

pub const WINDOW_CLASS: PCWSTR = w!("KuroMuteCore");
