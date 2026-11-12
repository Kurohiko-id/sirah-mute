use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::PCWSTR;

pub const SETTINGS_MUTEX: &str = r"Local\SirahMuteSettings";
/// Slint window title; settings.slint hardcodes the same string (ui.rs tests assert equality).
pub const WINDOW_TITLE: &str = "Sirah Mute Settings";

/// Holds the mutex; `None` handle = CreateMutexW failed, we run unguarded rather than never open.
pub struct SingleGuard(Option<HANDLE>);

impl Drop for SingleGuard {
    fn drop(&mut self) {
        if let Some(h) = self.0 {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// `None` = another settings process runs (its window is raised if found).
pub fn acquire() -> Option<SingleGuard> {
    let name = wide(SETTINGS_MUTEX);
    unsafe {
        let Ok(h) = CreateMutexW(None, false, PCWSTR(name.as_ptr())) else {
            return Some(SingleGuard(None));
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            raise();
            return None;
        }
        Some(SingleGuard(Some(h)))
    }
}

/// Bring the open settings window to the front (restore only if minimized, so a maximized
/// window stays maximized). No window yet = the instance is still starting; nothing to raise.
/// Works from the core because it is foreground right after a tray-menu pick.
pub fn raise() {
    let title = wide(WINDOW_TITLE);
    unsafe {
        if let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutex_name_is_local_and_stable() {
        assert_eq!(SETTINGS_MUTEX, r"Local\SirahMuteSettings");
    }
}
