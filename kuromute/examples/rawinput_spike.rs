//! Throwaway spike: do F13/F24 (macro) and media keys arrive via Raw Input?
use std::io::Write;
use std::mem::size_of;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIM_TYPEKEYBOARD, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

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
                if k.Flags & RI_KEY_BREAK == 0 {
                    println!("vk=0x{:02X} make=0x{:02X}", k.VKey, k.MakeCode);
                    let _ = std::io::stdout().flush();
                }
            }
        }
    }
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let hi = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hi.into(),
            lpszClassName: w!("KuroSpike"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        // top-level, never shown
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(), w!("KuroSpike"), w!("KuroSpike"),
            WS_OVERLAPPED, 0, 0, 0, 0, None, None, Some(hi.into()), None,
        )?;
        let rid = [RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        }];
        RegisterRawInputDevices(&rid, size_of::<RAWINPUTDEVICE>() as u32)?;
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
    Ok(())
}
