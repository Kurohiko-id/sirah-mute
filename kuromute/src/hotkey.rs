use crate::config::Hotkey;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey,
};

pub const MOD_ALT: u32 = 1;
pub const MOD_CONTROL: u32 = 2;
pub const MOD_SHIFT: u32 = 4;
pub const MOD_WIN: u32 = 8;
pub const MOD_NOREPEAT: u32 = 0x4000;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HotkeyError {
    Empty,
    BareF12,
    ModifierOnly,
}

pub fn validate(h: &Hotkey) -> Result<(), HotkeyError> {
    match h.vk {
        0 => Err(HotkeyError::Empty),
        0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5 => Err(HotkeyError::ModifierOnly),
        0x7B if h.mods == 0 => Err(HotkeyError::BareF12),
        _ => Ok(()),
    }
}

pub fn leaks_into_games(h: &Hotkey) -> bool {
    h.mods != 0 || h.vk == 0x7B
}

const FKEYS: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";

pub fn key_name(vk: u32) -> Option<&'static str> {
    Some(match vk {
        0x70..=0x87 => FKEYS[(vk - 0x70) as usize],
        0x41..=0x5A => {
            let i = (vk - 0x41) as usize;
            &LETTERS[i..=i]
        }
        0x30..=0x39 => {
            let i = (vk - 0x30) as usize;
            &DIGITS[i..=i]
        }
        0xB0 => "Next Track",
        0xB1 => "Previous Track",
        0xB2 => "Stop",
        0xB3 => "Play/Pause",
        0xAD => "Volume Mute",
        0xAE => "Volume Down",
        0xAF => "Volume Up",
        0x2D => "Insert",
        0x2E => "Delete",
        0x24 => "Home",
        0x23 => "End",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x13 => "Pause",
        0x91 => "Scroll Lock",
        _ => return None,
    })
}

pub const SELECTABLE_KEYS: &[u32] = &[
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x7B, 0x7C, 0x7D, 0x7E, 0x7F,
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58,
    0x59, 0x5A, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0xB0, 0xB1, 0xB2, 0xB3,
    0xAD, 0xAE, 0xAF, 0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22, 0x13, 0x91,
];

pub fn display(h: &Hotkey) -> String {
    let mut s = String::new();
    for (bit, name) in [
        (MOD_CONTROL, "Ctrl"),
        (MOD_ALT, "Alt"),
        (MOD_SHIFT, "Shift"),
        (MOD_WIN, "Win"),
    ] {
        if h.mods & bit != 0 {
            s += name;
            s += "+";
        }
    }
    match key_name(h.vk) {
        Some(n) => s += n,
        None => s += &format!("Key 0x{:02X}", h.vk),
    }
    s
}

pub fn register(hwnd: HWND, id: i32, h: &Hotkey) -> windows::core::Result<()> {
    unsafe {
        RegisterHotKey(
            Some(hwnd),
            id,
            HOT_KEY_MODIFIERS(h.mods | MOD_NOREPEAT),
            h.vk,
        )
    }
}

pub fn unregister(hwnd: HWND, id: i32) {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), id);
    }
}

/// Physical key state; RegisterHotKey only reports the press, so release is polled.
pub fn key_down(vk: u32) -> bool {
    (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
}

/// Cough hold: true = unmute on release (held at least the threshold).
pub fn cough_release(held: Duration, threshold_ms: u32) -> bool {
    held >= Duration::from_millis(threshold_ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cough_release_threshold() {
        let ms = Duration::from_millis;
        assert!(!cough_release(ms(399), 400));
        assert!(cough_release(ms(400), 400));
        assert!(cough_release(ms(1000), 400));
        assert!(!cough_release(ms(0), 200));
    }

    /// Spike: does GetAsyncKeyState see a key swallowed by RegisterHotKey?
    #[test]
    #[ignore]
    fn async_state_sees_registered_hotkey() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
            SendInput, VIRTUAL_KEY,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, MSG, PM_REMOVE,
            PeekMessageW, WINDOW_EX_STYLE, WM_HOTKEY, WS_OVERLAPPED,
        };
        use windows::core::w;
        let send = |up: bool| {
            let i = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0x87),
                        dwFlags: if up {
                            KEYEVENTF_KEYUP
                        } else {
                            KEYBD_EVENT_FLAGS(0)
                        },
                        ..Default::default()
                    },
                },
            };
            unsafe { SendInput(&[i], std::mem::size_of::<INPUT>() as i32) };
        };
        struct Guard(HWND);
        impl Drop for Guard {
            fn drop(&mut self) {
                unregister(self.0, 77);
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("spike"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .unwrap()
        };
        let _g = Guard(hwnd);
        register(hwnd, 77, &Hotkey { mods: 0, vk: 0x87 }).unwrap();
        send(false);
        let t0 = std::time::Instant::now();
        let mut got = false;
        while t0.elapsed() < Duration::from_secs(1) && !got {
            let mut m = MSG::default();
            while unsafe { PeekMessageW(&mut m, None, 0, 0, PM_REMOVE) }.as_bool() {
                if m.message == WM_HOTKEY {
                    got = true;
                }
                unsafe { DispatchMessageW(&m) };
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let down = key_down(0x87);
        send(true);
        std::thread::sleep(Duration::from_millis(50));
        let up = key_down(0x87);
        assert!(got, "no WM_HOTKEY");
        assert!(down, "key_down false while held");
        assert!(!up, "key_down true after release");
    }
    fn hk(mods: u32, vk: u32) -> Hotkey {
        Hotkey { mods, vk }
    }

    #[test]
    fn validate_rules() {
        assert_eq!(validate(&hk(0, 0x7B)), Err(HotkeyError::BareF12));
        assert_eq!(validate(&hk(2, 0x7B)), Ok(()));
        assert_eq!(validate(&hk(0, 0x7C)), Ok(()));
        assert_eq!(validate(&hk(2, 0x11)), Err(HotkeyError::ModifierOnly));
        assert_eq!(validate(&hk(0, 0)), Err(HotkeyError::Empty));
    }

    #[test]
    fn leaks() {
        assert!(leaks_into_games(&hk(2, 0x7B)));
        assert!(!leaks_into_games(&hk(0, 0x7C)));
    }

    #[test]
    fn display_names() {
        assert_eq!(display(&hk(3, 0x4D)), "Ctrl+Alt+M");
        assert_eq!(display(&hk(0, 0x87)), "F24");
        assert_eq!(display(&hk(0, 0xB3)), "Play/Pause");
        assert_eq!(display(&hk(8 | 4, 0x87)), "Shift+Win+F24");
        assert_eq!(display(&hk(0, 0xFF)), "Key 0xFF");
    }

    #[test]
    fn selectable_all_named() {
        for &vk in SELECTABLE_KEYS {
            assert!(key_name(vk).is_some(), "{vk:#x}");
        }
    }
}
