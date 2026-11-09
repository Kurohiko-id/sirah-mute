//! Hotkey recording through Raw Input (no low-level hook). Registered only while recording.
use crate::hotkey::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use std::cell::Cell;
use std::mem::size_of;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER,
    RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetForegroundWindow, GetWindowThreadProcessId, RegisterClassW, WINDOW_EX_STYLE, WM_INPUT, WNDCLASSW,
    WS_OVERLAPPED,
};
use windows::core::w;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordEvent {
    Key { vk: u32, mods: u32 },
    Cancel,
}

/// Raw Input sink also sees keys typed in other apps; give up after this long.
pub const RECORD_TIMEOUT_MS: u64 = 15_000;

/// Recording is abandoned on timeout or when another app has the foreground.
pub fn should_cancel(elapsed_ms: u64, foreground_is_ours: bool) -> bool {
    elapsed_ms >= RECORD_TIMEOUT_MS || !foreground_is_ours
}

/// True when the foreground window belongs to this (settings) process.
pub fn foreground_is_ours() -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

pub fn event_from_vk(vk: u32, mods: u32) -> RecordEvent {
    if vk == 0x1B {
        RecordEvent::Cancel
    } else {
        RecordEvent::Key { vk, mods }
    }
}

fn mod_bit(vk: u32) -> Option<u32> {
    match vk {
        0x10 | 0xA0 | 0xA1 => Some(MOD_SHIFT),
        0x11 | 0xA2 | 0xA3 => Some(MOD_CONTROL),
        0x12 | 0xA4 | 0xA5 => Some(MOD_ALT),
        0x5B | 0x5C => Some(MOD_WIN),
        _ => None,
    }
}

/// Modifier bits after a Raw Input make/break of `vk`; non-modifiers leave `mods` unchanged.
pub fn apply_mod(mods: u32, vk: u32, is_break: bool) -> u32 {
    match mod_bit(vk) {
        Some(b) if is_break => mods & !b,
        Some(b) => mods | b,
        None => mods,
    }
}

fn held_mods() -> u32 {
    let down = |vk: i32| unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0;
    let mut m = 0;
    if down(0x11) {
        m |= MOD_CONTROL;
    }
    if down(0x12) {
        m |= MOD_ALT;
    }
    if down(0x10) {
        m |= MOD_SHIFT;
    }
    if down(0x5B) || down(0x5C) {
        m |= MOD_WIN;
    }
    m
}

thread_local! {
    // wndproc and the poll timer both run on the settings UI thread.
    static PENDING: Cell<Option<RecordEvent>> = const { Cell::new(None) };
    // Modifier state from the Raw Input stream itself, so a fast macro (Ctrl down, F13, Ctrl up)
    // still records Ctrl+F13 even when WM_INPUT is dispatched after Ctrl is released.
    static MODS: Cell<u32> = const { Cell::new(0) };
}

const RI_KEY_BREAK: u16 = 1;

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        let mut buf = [0u8; 256];
        let mut size = buf.len() as u32;
        let n = unsafe {
            GetRawInputData(
                HRAWINPUT(lp.0 as _),
                RID_INPUT,
                Some(buf.as_mut_ptr().cast()),
                &mut size,
                size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if n != u32::MAX {
            let ri = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
            if ri.header.dwType == RIM_TYPEKEYBOARD.0 {
                let k = unsafe { ri.data.keyboard };
                let vk = k.VKey as u32;
                let is_break = k.Flags & RI_KEY_BREAK != 0;
                // Modifiers alone never finish recording; they only update MODS.
                if mod_bit(vk).is_some() {
                    MODS.with(|m| m.set(apply_mod(m.get(), vk, is_break)));
                } else if !is_break && vk != 0xFF {
                    let mods = MODS.with(Cell::get);
                    PENDING.with(|p| {
                        if p.get().is_none() {
                            p.set(Some(event_from_vk(vk, mods)));
                        }
                    });
                }
            }
        }
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// Hidden top-level window on the calling thread (the Slint/winit pump dispatches to it).
/// Receives WM_INPUT while recording; also given to `Audio::new` (its device posts are ignored).
pub fn create_hidden_window() -> windows::core::Result<HWND> {
    unsafe {
        let hi = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hi.into(),
            lpszClassName: w!("KuroMuteSettingsSink"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("KuroMuteSettingsSink"),
            w!("KuroMuteSettingsSink"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hi.into()),
            None,
        )
    }
}

fn register(flags: RAWINPUTDEVICE_FLAGS, hwnd: HWND) -> windows::core::Result<()> {
    let rid = [RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06,
        dwFlags: flags,
        hwndTarget: hwnd,
    }];
    unsafe { RegisterRawInputDevices(&rid, size_of::<RAWINPUTDEVICE>() as u32) }
}

/// Keyboard Raw Input registration; removed on drop.
pub struct Recorder;

impl Recorder {
    pub fn start(hwnd: HWND) -> windows::core::Result<Recorder> {
        PENDING.with(|p| p.set(None));
        MODS.with(|m| m.set(held_mods())); // modifiers already down before Rekam
        register(RIDEV_INPUTSINK, hwnd)?;
        Ok(Recorder)
    }

    pub fn poll(&mut self) -> Option<RecordEvent> {
        PENDING.with(|p| p.take())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = register(RIDEV_REMOVE, HWND::default());
        PENDING.with(|p| p.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_cancels() {
        assert_eq!(event_from_vk(0x1B, 0), RecordEvent::Cancel);
    }

    #[test]
    fn mods_follow_make_and_break() {
        let m = apply_mod(0, 0x11, false); // Ctrl down
        assert_eq!(m, MOD_CONTROL);
        let m = apply_mod(m, 0x5C, false); // RWin down
        assert_eq!(m, MOD_CONTROL | MOD_WIN);
        let m = apply_mod(m, 0x7C, false); // F13: not a modifier
        assert_eq!(m, MOD_CONTROL | MOD_WIN);
        assert_eq!(apply_mod(m, 0x11, true), MOD_WIN); // Ctrl up
    }

    #[test]
    fn key_with_mods() {
        assert_eq!(
            event_from_vk(0x7C, 2),
            RecordEvent::Key { vk: 0x7C, mods: 2 }
        );
    }

    #[test]
    fn cancel_on_timeout_or_lost_focus() {
        assert!(!should_cancel(0, true));
        assert!(!should_cancel(RECORD_TIMEOUT_MS - 1, true));
        assert!(should_cancel(RECORD_TIMEOUT_MS, true));
        assert!(should_cancel(10, false));
    }
}
