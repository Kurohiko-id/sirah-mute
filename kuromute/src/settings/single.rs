use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::PCWSTR;

pub const SETTINGS_MUTEX: &str = r"Local\KuroMuteSettings";
/// Slint window title; the UI (Task 5) must use the same string.
pub const WINDOW_TITLE: &str = "Kuro Mute Settings";

pub struct SingleGuard(HANDLE);

impl Drop for SingleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
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
        let h = CreateMutexW(None, false, PCWSTR(name.as_ptr())).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            // No window yet means the other instance is still starting; nothing to raise.
            let title = wide(WINDOW_TITLE);
            if let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return None;
        }
        Some(SingleGuard(h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutex_name_is_local_and_stable() {
        assert_eq!(SETTINGS_MUTEX, r"Local\KuroMuteSettings");
    }
}
