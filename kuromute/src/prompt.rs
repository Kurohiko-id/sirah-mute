//! Clickable "default device changed" prompt for guard Notify mode. Shown instead of a Windows
//! notification so Do Not Disturb can't swallow it. Topmost but never takes focus, so a game keeps
//! keyboard and mouse. A button click posts `WM_APP_DRIFT_CHOICE` (wparam = `Choice`) to the core.

use crate::geometry::Rect;
use crate::i18n::{Key, ResolvedLang, t};
use crate::monitors::Monitor;
use crate::msg::{WINDOW_CLASS, WM_APP_DRIFT_CHOICE};
use crate::overlay::{Renderer, hinstance, rgb, text_width};
use std::cell::Cell;
use std::sync::Once;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ROUNDED_RECT, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteTextFormat,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HDC,
    ScreenToClient, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GetCursorPos, HWND_TOPMOST,
    IDC_ARROW, IDC_HAND, LoadCursorW, MA_NOACTIVATE, PostMessageW, RegisterClassExW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetCursor, SetWindowPos, ULW_ALPHA,
    UpdateLayeredWindow, WM_CLOSE, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_SETCURSOR, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};

const CLASS: PCWSTR = w!("SirahMuteDrift");
const GLYPH_WARN: &str = "\u{E7BA}";
const BG: D2D1_COLOR_F = rgb(0x121216, 0.97);
const EDGE: D2D1_COLOR_F = rgb(0xFFFFFF, 0.08);
const AMBER: D2D1_COLOR_F = rgb(0xF5A524, 1.0);
const FG: D2D1_COLOR_F = rgb(0xECEDEF, 1.0);
const DIM: D2D1_COLOR_F = rgb(0xB8BDC6, 1.0);
const RED: D2D1_COLOR_F = rgb(0xE5484D, 1.0);
const BTN: D2D1_COLOR_F = rgb(0x2A2C33, 1.0);
const NO_RECT: Rect = Rect {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Choice {
    SwitchBack = 0,
    Keep = 1,
    Snooze = 2,
}

impl Choice {
    pub fn from_wparam(w: usize) -> Option<Self> {
        [Choice::SwitchBack, Choice::Keep, Choice::Snooze].get(w).copied()
    }
}

/// One drifted device: label ("Mic now:"), what Windows uses now, and the user's pick.
pub struct Row {
    pub now_label: Key,
    pub now: String,
    pub pick: String,
}

thread_local! {
    /// Button hit boxes of the visible prompt, window-relative. Only one prompt exists at a time.
    static BUTTONS: Cell<[Rect; 3]> = const { Cell::new([NO_RECT; 3]) };
}

fn button_at(x: i32, y: i32) -> Option<usize> {
    BUTTONS.with(|b| {
        b.get()
            .iter()
            .position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    })
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let core = || unsafe { FindWindowW(WINDOW_CLASS, PCWSTR::null()) };
    match m {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(h, &mut pt);
            let id = if button_at(pt.x, pt.y).is_some() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
            SetCursor(LoadCursorW(None, id).ok());
            LRESULT(1)
        },
        WM_LBUTTONUP => {
            let (x, y) = (lp.0 as i16 as i32, (lp.0 >> 16) as i16 as i32);
            if let (Some(i), Ok(c)) = (button_at(x, y), core()) {
                let _ = unsafe { PostMessageW(Some(c), WM_APP_DRIFT_CHOICE, WPARAM(i), LPARAM(0)) };
            }
            LRESULT(0)
        }
        // Same as the overlay windows: taskkill (no /f) may hit this visible window first.
        WM_CLOSE => {
            if let Ok(c) = core() {
                let _ = unsafe { PostMessageW(Some(c), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

pub struct Prompt {
    hwnd: HWND,
}

impl Drop for Prompt {
    fn drop(&mut self) {
        BUTTONS.with(|b| b.set([NO_RECT; 3]));
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Window size in physical pixels for `rows` drifted devices at scale `k` (dpi / 96).
fn size(k: f32, rows: usize) -> (i32, i32) {
    let h = 16.0 + 24.0 + 8.0 + rows as f32 * 2.0 * 21.0 + 14.0 + 34.0 + 16.0;
    ((440.0 * k).round() as i32, (h * k).round() as i32)
}

/// Shows the prompt in the bottom-right of `mon`'s work area (where Windows toasts appear).
pub fn show(mon: &Monitor, lang: ResolvedLang, title: Key, rows: &[Row]) -> Result<Prompt> {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance(),
            lpszClassName: CLASS,
            ..Default::default()
        });
    });
    let k = mon.dpi as f32 / 96.0;
    let (wi, hi) = size(k, rows.len());
    let margin = (16.0 * k) as i32;
    let pos = POINT {
        x: mon.work.right - wi - margin,
        y: mon.work.bottom - hi - margin,
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
            CLASS,
            PCWSTR::null(),
            WS_POPUP,
            pos.x,
            pos.y,
            wi,
            hi,
            None,
            None,
            Some(hinstance()),
            None,
        )?
    };
    let prompt = Prompt { hwnd };
    let r = Renderer::new()?;
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: wi,
            biHeight: -hi, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let dc = CreateCompatibleDC(None);
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(dc, bmp.into());
        let drawn = paint(&r, dc, (wi, hi), k, lang, title, rows);
        if drawn.is_ok() {
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&pos),
                Some(&SIZE { cx: wi, cy: hi }),
                Some(dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        drawn?;
    }
    Ok(prompt)
}

fn paint(
    r: &Renderer,
    dc: HDC,
    (wi, hi): (i32, i32),
    k: f32,
    lang: ResolvedLang,
    title: Key,
    rows: &[Row],
) -> Result<()> {
    let (wf, hf) = (wi as f32, hi as f32);
    let px = |v: f32| v * k;
    let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
    let one_line = |f: IDWriteTextFormat| -> Result<IDWriteTextFormat> {
        unsafe {
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                ..Default::default()
            };
            let sign = r.dw.CreateEllipsisTrimmingSign(&f)?;
            f.SetTrimming(&trim, &sign)?;
        }
        Ok(f)
    };
    let icon = r.format_w(r.icon_family, px(17.0), DWRITE_FONT_WEIGHT_NORMAL, lead)?;
    let head = r.format_w(w!("Segoe UI"), px(15.0), DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?;
    let label = r.format_w(w!("Segoe UI"), px(13.0), DWRITE_FONT_WEIGHT_NORMAL, lead)?;
    let name = one_line(r.format_w(w!("Segoe UI"), px(13.0), DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?)?;
    let btn = r.format_w(
        w!("Segoe UI"),
        px(13.0),
        DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWRITE_TEXT_ALIGNMENT_CENTER,
    )?;
    let brush = |c: &D2D1_COLOR_F| -> Result<ID2D1SolidColorBrush> {
        unsafe { r.rt.CreateSolidColorBrush(c, None) }
    };
    let text = |s: &str, f: &IDWriteTextFormat, c: &D2D1_COLOR_F, rc: D2D_RECT_F| -> Result<()> {
        let u: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            r.rt.DrawText(
                &u,
                f,
                &rc,
                &brush(c)?,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    };
    let rrect = |l: f32, top: f32, rr: f32, b: f32, rad: f32| D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: l,
            top,
            right: rr,
            bottom: b,
        },
        radiusX: rad,
        radiusY: rad,
    };
    let pad = px(16.0);
    unsafe {
        r.rt.BindDC(
            dc,
            &RECT {
                left: 0,
                top: 0,
                right: wi,
                bottom: hi,
            },
        )?;
        r.rt.BeginDraw();
        r.rt.Clear(Some(&D2D1_COLOR_F::default()));
        let card = rrect(0.5, 0.5, wf - 0.5, hf - 0.5, px(12.0));
        r.rt.FillRoundedRectangle(&card, &brush(&BG)?);
        r.rt.DrawRoundedRectangle(&card, &brush(&EDGE)?, 1.0, None);
    }
    let row = |top: f32, h: f32| D2D_RECT_F {
        left: pad,
        top,
        right: wf - pad,
        bottom: top + h,
    };
    let mut y = pad;
    text(GLYPH_WARN, &icon, &AMBER, row(y, px(24.0)))?;
    let mut tr = row(y, px(24.0));
    tr.left += px(28.0);
    text(t(lang, title), &head, &FG, tr)?;
    y += px(24.0 + 8.0);
    for rw in rows {
        for (lk, val) in [(rw.now_label, &rw.now), (Key::DriftYourPick, &rw.pick)] {
            let l = t(lang, lk);
            let lu: Vec<u16> = l.encode_utf16().collect();
            let lw = text_width(&r.dw, &lu, &label)?;
            let rc = row(y, px(21.0));
            text(l, &label, &DIM, rc)?;
            let mut nr = rc;
            nr.left += lw + px(6.0);
            text(val, &name, &FG, nr)?;
            y += px(21.0);
        }
    }
    y += px(14.0);
    let gap = px(8.0);
    let bw = (wf - 2.0 * pad - 2.0 * gap) / 3.0;
    let bh = px(34.0);
    let mut hits = [NO_RECT; 3];
    let labels = [Key::DriftSwitchBack, Key::DriftKeep, Key::DriftSnooze];
    for (i, lk) in labels.into_iter().enumerate() {
        let x = pad + i as f32 * (bw + gap);
        let b = rrect(x, y, x + bw, y + bh, px(7.0));
        unsafe { r.rt.FillRoundedRectangle(&b, &brush(if i == 0 { &RED } else { &BTN })?) };
        text(t(lang, lk), &btn, &FG, b.rect)?;
        hits[i] = Rect {
            left: x as i32,
            top: y as i32,
            right: (x + bw) as i32,
            bottom: (y + bh) as i32,
        };
    }
    BUTTONS.with(|b| b.set(hits));
    unsafe { r.rt.EndDraw(None, None) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_round_trips_through_wparam() {
        for c in [Choice::SwitchBack, Choice::Keep, Choice::Snooze] {
            assert_eq!(Choice::from_wparam(c as usize), Some(c));
        }
        assert_eq!(Choice::from_wparam(3), None);
    }

    #[test]
    fn height_grows_per_row() {
        assert_eq!(size(1.0, 1), (440, 154));
        assert_eq!(size(1.0, 2).1 - size(1.0, 1).1, 42);
        assert_eq!(size(1.5, 1).0, 660);
    }
}
