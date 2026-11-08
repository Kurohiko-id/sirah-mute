//! Notification-area icon: state badges, context menu and balloon notifications.

use crate::i18n::{Key, ResolvedLang, t};
use crate::log;
use crate::msg::WM_APP_TRAY;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, HBITMAP,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_RESPECT_QUIET_TIME,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreateIconIndirect, CreatePopupMenu, DestroyIcon,
    DestroyMenu, GetCursorPos, GetIconInfo, GetSystemMetrics, HICON, ICONINFO, LR_DEFAULTCOLOR,
    LookupIconIdFromDirectoryEx, MF_CHECKED, MF_SEPARATOR, MF_STRING, PostMessageW, SM_CXSMICON,
    SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};
use windows::core::{PCWSTR, Result};

const APP_ICO: &[u8] = include_bytes!("../../app.ico");
const ICON_ID: u32 = 1;
const RED: u32 = 0xE5484D;
const GREY: u32 = 0x8A8F98;
/// Badge diameter as a fraction of the icon size.
const BADGE: f32 = 0.45;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TrayState {
    Unmuted,
    Muted,
    Disconnected,
    NoMic,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MenuCmd {
    Settings,
    ToggleGuard,
    CheckUpdate,
    ReportBug,
    Exit,
}

const MENU: [MenuCmd; 5] = [
    MenuCmd::Settings,
    MenuCmd::ToggleGuard,
    MenuCmd::CheckUpdate,
    MenuCmd::ReportBug,
    MenuCmd::Exit,
];

pub struct Tray {
    hwnd: HWND,
    plain: HICON,
    red: HICON,
    grey: HICON,
    state: TrayState,
    tip: String,
}

impl Tray {
    pub fn add(hwnd: HWND) -> Self {
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
        let plain = load_app_icon(size).unwrap_or_else(|e| {
            log!("tray: app icon load failed: {e}");
            HICON::default()
        });
        let badge = |c| {
            badged(plain, size, c).unwrap_or_else(|e| {
                log!("tray: badge icon failed: {e}");
                plain
            })
        };
        let mut tray = Tray {
            hwnd,
            plain,
            red: badge(RED),
            grey: badge(GREY),
            state: TrayState::Unmuted,
            tip: t(ResolvedLang::En, Key::AppName).to_string(),
        };
        tray.readd();
        tray
    }

    fn icon(&self) -> HICON {
        match self.state {
            TrayState::Unmuted => self.plain,
            TrayState::Muted => self.red,
            TrayState::Disconnected | TrayState::NoMic => self.grey,
        }
    }

    fn base(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            ..Default::default()
        }
    }

    fn full(&self) -> NOTIFYICONDATAW {
        let mut nid = self.base();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = self.icon();
        copy_trunc(&mut nid.szTip, &self.tip);
        nid
    }

    pub fn set(&mut self, state: TrayState, tooltip: &str) {
        self.state = state;
        self.tip = tooltip.to_string();
        let nid = self.full();
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) }.as_bool() {
            log!("tray: NIM_MODIFY failed");
        }
    }

    /// Re-adds the icon, e.g. after `TaskbarCreated` (Explorer restart).
    pub fn readd(&mut self) {
        let mut nid = self.full();
        unsafe {
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                log!("tray: NIM_ADD failed");
            }
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    pub fn balloon(&self, title: &str, body: &str) {
        let mut nid = self.base();
        nid.uFlags = NIF_INFO | NIF_SHOWTIP;
        nid.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
        copy_trunc(&mut nid.szInfoTitle, title);
        copy_trunc(&mut nid.szInfo, body);
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) }.as_bool() {
            log!("tray: balloon failed");
        }
    }

    pub fn menu(&self, hwnd: HWND, guard_on: bool, lang: ResolvedLang) -> Option<MenuCmd> {
        unsafe {
            let m = CreatePopupMenu().ok()?;
            let item = |cmd: MenuCmd, key: Key, flags| {
                let id = MENU.iter().position(|&c| c == cmd).unwrap() + 1;
                let label: Vec<u16> = t(lang, key).encode_utf16().chain([0]).collect();
                let _ = AppendMenuW(m, MF_STRING | flags, id, PCWSTR(label.as_ptr()));
            };
            let sep = || {
                let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
            };
            let checked = if guard_on { MF_CHECKED } else { MF_STRING };
            item(MenuCmd::Settings, Key::MenuSettings, MF_STRING);
            item(MenuCmd::ToggleGuard, Key::MenuGuard, checked);
            sep();
            item(MenuCmd::CheckUpdate, Key::MenuCheckUpdate, MF_STRING);
            item(MenuCmd::ReportBug, Key::MenuReportBug, MF_STRING);
            sep();
            item(MenuCmd::Exit, Key::MenuExit, MF_STRING);

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(hwnd);
            let id = TrackPopupMenu(
                m,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                None,
                hwnd,
                None,
            )
            .0;
            // KB135788: lets the menu dismiss correctly on the next click elsewhere.
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(m);
            MENU.get((id as usize).checked_sub(1)?).copied()
        }
    }

    pub fn remove(&mut self) {
        let nid = self.base();
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            for h in [self.red, self.grey] {
                if h != self.plain && !h.is_invalid() {
                    let _ = DestroyIcon(h);
                }
            }
            if !self.plain.is_invalid() {
                let _ = DestroyIcon(self.plain);
            }
        }
    }
}

/// Copies `s` as a NUL-terminated UTF-16 string, truncated to `dst.len() - 1` units.
fn copy_trunc(dst: &mut [u16], s: &str) {
    let n = dst.len() - 1;
    let mut i = 0;
    for u in s.encode_utf16().take(n) {
        dst[i] = u;
        i += 1;
    }
    // Don't leave a dangling high surrogate at the cut.
    if i == n && (0xD800..0xDC00).contains(&dst[i - 1]) {
        i -= 1;
    }
    dst[i] = 0;
}

fn load_app_icon(size: i32) -> Result<HICON> {
    unsafe {
        let off = LookupIconIdFromDirectoryEx(APP_ICO.as_ptr(), true, size, size, LR_DEFAULTCOLOR);
        if off <= 0 {
            return Err(windows::core::Error::from_thread());
        }
        CreateIconFromResourceEx(
            &APP_ICO[off as usize..],
            true,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
}

/// Fraction (0..=1) of pixel (x, y) covered by a circle of radius `r` centred at (cx, cy),
/// estimated with 4x4 supersampling.
pub fn coverage(x: u32, y: u32, cx: f32, cy: f32, r: f32) -> f32 {
    let mut hits = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let px = x as f32 + (sx as f32 + 0.5) / 4.0 - cx;
            let py = y as f32 + (sy as f32 + 0.5) / 4.0 - cy;
            if px * px + py * py <= r * r {
                hits += 1;
            }
        }
    }
    hits as f32 / 16.0
}

/// Reads the icon's colour bitmap as top-down straight-alpha BGRA, paints a filled circle of
/// `rgb` in the bottom-right, and returns it as a new icon.
fn badged(base: HICON, size: i32, rgb: u32) -> Result<HICON> {
    unsafe {
        let mut ii = ICONINFO::default();
        GetIconInfo(base, &mut ii)?;
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let n = (size * size) as usize;
        let mut px = vec![0u8; n * 4];
        let dc = CreateCompatibleDC(None);
        let got = GetDIBits(
            dc,
            ii.hbmColor,
            0,
            size as u32,
            Some(px.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        if !px.chunks(4).any(|p| p[3] != 0) {
            // Legacy icon without alpha: take transparency from the AND mask.
            let mut mask = vec![0u8; n * 4];
            GetDIBits(
                dc,
                ii.hbmMask,
                0,
                size as u32,
                Some(mask.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            for (p, m) in px.chunks_mut(4).zip(mask.chunks(4)) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        let _ = DeleteDC(dc);
        let _ = DeleteObject(ii.hbmColor.into());
        let _ = DeleteObject(ii.hbmMask.into());
        if got == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let r = size as f32 * BADGE / 2.0;
        let (cx, cy) = (size as f32 - r, size as f32 - r);
        let src = [
            (rgb & 0xFF) as f32,
            ((rgb >> 8) & 0xFF) as f32,
            ((rgb >> 16) & 0xFF) as f32,
        ];
        for y in 0..size as u32 {
            for x in 0..size as u32 {
                let a_s = coverage(x, y, cx, cy, r);
                if a_s == 0.0 {
                    continue;
                }
                let p = &mut px[((y * size as u32 + x) * 4) as usize..][..4];
                let a_d = p[3] as f32 / 255.0;
                let a_o = a_s + a_d * (1.0 - a_s);
                for c in 0..3 {
                    let v = (src[c] * a_s + p[c] as f32 * a_d * (1.0 - a_s)) / a_o;
                    p[c] = v.round() as u8;
                }
                p[3] = (a_o * 255.0).round() as u8;
            }
        }

        let mut bits = std::ptr::null_mut();
        let color: HBITMAP = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits.cast(), px.len());
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            hbmColor: color,
            hbmMask: mask,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_inside_edge_outside() {
        assert_eq!(coverage(10, 10, 10.5, 10.5, 4.0), 1.0);
        assert_eq!(coverage(0, 0, 10.5, 10.5, 4.0), 0.0);
        let edge = coverage(14, 10, 10.5, 10.5, 4.0);
        assert!(edge > 0.0 && edge < 1.0, "edge = {edge}");
    }

    #[test]
    fn copy_trunc_limits_and_terminates() {
        let mut buf = [0xFFFFu16; 4];
        copy_trunc(&mut buf, "abcdef");
        assert_eq!(buf, [b'a' as u16, b'b' as u16, b'c' as u16, 0]);
        copy_trunc(&mut buf, "ab\u{1F600}"); // surrogate pair would be split at the cut
        assert_eq!(&buf[..3], &[b'a' as u16, b'b' as u16, 0]);
    }
}
