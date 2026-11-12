//! Layered overlay windows: per-monitor mute popup (Direct2D-rendered, per-pixel
//! alpha) and red border strips. Windows never take focus; only the muted Pill takes clicks.

use crate::config::{BorderStyle, Config, PopupStyle, Position};
use crate::geometry::{Rect, border_strips, popup_rect};
use crate::i18n::{Key, ResolvedLang, t};
use crate::monitors::Monitor;
use std::sync::Once;
use std::time::Instant;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory,
    IDWriteTextFormat,
};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CreateWindowExW, DefWindowProcW, DeferWindowPos, DestroyWindow,
    EndDeferWindowPos, FindWindowW, GWL_EXSTYLE, GetWindowLongPtrW, HTTRANSPARENT,
    HWND_TOPMOST, IDC_HAND, KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, PostMessageW,
    RegisterClassExW, SET_WINDOW_POS_FLAGS, SWP_HIDEWINDOW, SetCursor, SetWindowLongPtrW,
    WM_LBUTTONUP, WM_SETCURSOR,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ULW_ALPHA, UpdateLayeredWindow, WM_CLOSE,
    WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, Result, w};

pub const TIMER_POPUP_HIDE: usize = 1;
pub const TIMER_PULSE: usize = 6;
pub const TIMER_MUTE_CLOCK: usize = 7;
const PULSE_TICK_MS: u32 = 33;
const BLINK_MS: u32 = 550;
const BLINKS: u32 = 3;
const POPUP_HIDE_MS: u32 = 1500;
const POPUP_CLASS: PCWSTR = w!("SirahMutePopup");
const PILL_CLASS: PCWSTR = w!("SirahMutePill");
const BORDER_CLASS: PCWSTR = w!("SirahMuteBorder");
const BORDER_COLOR: COLORREF = COLORREF(0x004D_48E5); // #E5484D as 0x00BBGGRR
pub(crate) const BORDER_RGB: u32 = 0xE5484D;
const GLYPH_MUTED: &str = "\u{E74F}"; // MicOff (OverlayForm.cs:88)
const GLYPH_UNMUTED: &str = "\u{E720}"; // Microphone
pub(crate) const GLYPH_MIC_OFF: &str = "\u{EC54}"; // MicOff (pill style)
const PILL_MUTED: &str = "Mic mute";
const PILL_UNMUTED: &str = "Mic on";

pub(crate) const fn rgb(c: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((c >> 16) & 0xFF) as f32 / 255.0,
        g: ((c >> 8) & 0xFF) as f32 / 255.0,
        b: (c & 0xFF) as f32 / 255.0,
        a,
    }
}
const FG: D2D1_COLOR_F = rgb(0xD7DADF, 1.0);
const BG: D2D1_COLOR_F = rgb(0x131417, 242.0 / 255.0);
const PILL_BG: D2D1_COLOR_F = rgb(0x120A0B, 0.9);
const DIM: D2D1_COLOR_F = rgb(0xD7DADF, 0.35);
const WHITE: D2D1_COLOR_F = rgb(0xFFFFFF, 1.0);
pub(crate) const MUTE_RED: D2D1_COLOR_F = rgb(BORDER_RGB, 1.0);

pub(crate) unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        // taskkill (no /f) closes one visible top-level window per call, and the core
        // window is hidden, so it hits a strip. Treat that as "close the app".
        WM_CLOSE => unsafe {
            if let Ok(core) = FindWindowW(crate::msg::WINDOW_CLASS, PCWSTR::null()) {
                let _ = PostMessageW(Some(core), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        },
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

/// Whether a popup is click-through: the Card always, the Pill unless the muted
/// pill shows (so clicks during the brief "Mic on" pill reach the game). Done with
/// WS_EX_TRANSPARENT: HTTRANSPARENT only passes clicks to the same thread's windows.
pub fn click_through(style: PopupStyle, muted_shown: bool) -> bool {
    style == PopupStyle::Card || !muted_shown
}

fn set_click_through(h: HWND, on: bool) {
    let t = WS_EX_TRANSPARENT.0 as isize;
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let new = if on { ex | t } else { ex & !t };
        if new != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, new);
        }
    }
}

/// Shared by clickable overlay windows: never take focus, hand cursor.
pub(crate) fn clickable_common(m: u32) -> Option<LRESULT> {
    match m {
        WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
        WM_SETCURSOR => {
            unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
            Some(LRESULT(1))
        }
        _ => None,
    }
}

/// Posts a click message to the core window.
pub(crate) fn post_to_core(m: u32, wp: usize, lp: isize) {
    if let Ok(core) = unsafe { FindWindowW(crate::msg::WINDOW_CLASS, PCWSTR::null()) } {
        let _ = unsafe { PostMessageW(Some(core), m, WPARAM(wp), LPARAM(lp)) };
    }
}

unsafe extern "system" fn pill_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(r) = clickable_common(m) {
        return r;
    }
    match m {
        WM_LBUTTONUP => {
            post_to_core(crate::msg::WM_APP_PILL_CLICK, 0, 0);
            LRESULT(0)
        }
        WM_CLOSE => unsafe { wndproc(h, m, wp, lp) },
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

pub(crate) fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|m| HINSTANCE(m.0))
        .unwrap_or_default()
}

fn register_classes() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let mut wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance(),
            lpszClassName: POPUP_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        wc.lpszClassName = PILL_CLASS;
        wc.lpfnWndProc = Some(pill_proc);
        RegisterClassExW(&wc);
        wc.lpfnWndProc = Some(wndproc);
        wc.lpszClassName = BORDER_CLASS;
        wc.hbrBackground = CreateSolidBrush(BORDER_COLOR);
        RegisterClassExW(&wc);
    });
}

pub(crate) fn create_window(class: PCWSTR, r: Rect, transparent: bool) -> Result<HWND> {
    let click = if transparent { WS_EX_TRANSPARENT } else { Default::default() };
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | click | WS_EX_TOPMOST | WS_EX_LAYERED,
            class,
            PCWSTR::null(),
            WS_POPUP,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(hinstance()),
            None,
        )
    }
}

struct Popup {
    hwnd: HWND,
    dc: HDC,
    old: HGDIOBJ,
    /// [unmuted, muted "· 0:00", muted live clock], selected into `dc` on demand.
    bmp: [HBITMAP; 3],
    rect: Rect,
    bmi: BITMAPINFO,
    style: PopupStyle,
    /// Pill only: where the pill sits in the window (0 left, 0.5 center, 1 right).
    align: f32,
    icon_fmt: IDWriteTextFormat,
    text_fmt: IDWriteTextFormat,
    clock_fmt: IDWriteTextFormat,
    icon_h: f32,
    radius: f32,
}

/// Width of `s` laid out in `fmt`.
pub(crate) fn text_width(dw: &IDWriteFactory, s: &[u16], fmt: &IDWriteTextFormat) -> Result<f32> {
    unsafe {
        let l = dw.CreateTextLayout(s, fmt, 10_000.0, 1_000.0)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        l.GetMetrics(&mut m)?;
        Ok(m.widthIncludingTrailingWhitespace)
    }
}

/// What one pill shows. Formats are `[icon, label, clock]`.
pub(crate) struct PillText<'a> {
    pub(crate) glyph: &'a str,
    pub(crate) label: &'a str,
    pub(crate) clock: Option<&'a str>,
    pub(crate) accent: D2D1_COLOR_F,
    pub(crate) edge: D2D1_COLOR_F,
}

/// (pad, icon/label gap, label/clock gap) for a pill of height `hf`.
fn pill_spacing(hf: f32, clock: bool) -> (f32, f32, f32) {
    let gap = hf * 0.2;
    (hf * 0.36, gap, if clock { gap * 1.4 } else { 0.0 })
}

/// Widths of glyph, label and clock (0 without one).
fn pill_parts(r: &Renderer, f: [&IDWriteTextFormat; 3], p: &PillText) -> Result<[f32; 3]> {
    let u = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    Ok([
        text_width(&r.dw, &u(p.glyph), f[0])?,
        text_width(&r.dw, &u(p.label), f[1])?,
        match p.clock {
            Some(c) => text_width(&r.dw, &u(c), f[2])?,
            None => 0.0,
        },
    ])
}

/// Full width of pill `p` at height `hf`, the pill hugging its text.
pub(crate) fn pill_width(
    r: &Renderer,
    f: [&IDWriteTextFormat; 3],
    p: &PillText,
    hf: f32,
) -> Result<f32> {
    let [iw, lw, cw] = pill_parts(r, f, p)?;
    let (pad, gap, cgap) = pill_spacing(hf, p.clock.is_some());
    Ok(pad + iw + gap + lw + cgap + cw + pad)
}

/// Draws pill `p` (`pw` x `hf`) at `x0` (top 0) into the bound render target.
pub(crate) fn paint_pill(
    r: &Renderer,
    f: [&IDWriteTextFormat; 3],
    p: &PillText,
    x0: f32,
    pw: f32,
    hf: f32,
) -> Result<()> {
    let u = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let [iw, lw, cw] = pill_parts(r, f, p)?;
    let (pad, gap, cgap) = pill_spacing(hf, p.clock.is_some());
    let sw = (hf / 30.0).max(1.0); // outline width
    let brush = |c: &D2D1_COLOR_F| -> Result<ID2D1SolidColorBrush> {
        unsafe { r.rt.CreateSolidColorBrush(c, None) }
    };
    let shape = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x0 + sw / 2.0,
            top: sw / 2.0,
            right: x0 + pw - sw / 2.0,
            bottom: hf - sw / 2.0,
        },
        radiusX: (hf - sw) / 2.0,
        radiusY: (hf - sw) / 2.0,
    };
    let put = |s: &str, fmt: &IDWriteTextFormat, c: &D2D1_COLOR_F, x: f32, w: f32| -> Result<()> {
        let lr = D2D_RECT_F {
            left: x,
            top: 0.0,
            right: x + w + 1.0,
            bottom: hf,
        };
        unsafe {
            r.rt.DrawText(
                &u(s),
                fmt,
                &lr,
                &brush(c)?,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        Ok(())
    };
    unsafe {
        r.rt.FillRoundedRectangle(&shape, &brush(&PILL_BG)?);
        r.rt.DrawRoundedRectangle(&shape, &brush(&p.edge)?, sw, None);
    }
    let x = x0 + pad;
    put(p.glyph, f[0], &p.accent, x, iw)?;
    let x = x + iw + gap;
    put(p.label, f[1], &WHITE, x, lw)?;
    if let Some(c) = p.clock {
        put(c, f[2], &FG, x + lw + cgap, cw)?;
    }
    Ok(())
}

impl Popup {
    /// Renders the unmuted (`muted_secs` None) or muted state into bitmap `slot`
    /// (created on first use, then reused).
    fn draw(
        &mut self,
        r: &Renderer,
        slot: usize,
        lang: ResolvedLang,
        muted_secs: Option<u64>,
    ) -> Result<()> {
        let (w, h) = (
            self.rect.right - self.rect.left,
            self.rect.bottom - self.rect.top,
        );
        let (wf, hf) = (w as f32, h as f32);
        unsafe {
            if self.bmp[slot].is_invalid() {
                let mut bits = std::ptr::null_mut();
                self.bmp[slot] =
                    CreateDIBSection(Some(self.dc), &self.bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            }
            let prev = SelectObject(self.dc, self.bmp[slot].into());
            if self.old.is_invalid() {
                self.old = prev;
            }
            r.rt.BindDC(
                self.dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                },
            )?;
            r.rt.BeginDraw();
            r.rt.Clear(Some(&D2D1_COLOR_F::default()));
            let res = match self.style {
                PopupStyle::Card => self.paint_card(r, wf, hf, lang, muted_secs),
                PopupStyle::Pill => self.paint_pill(r, wf, hf, muted_secs),
            };
            let end = r.rt.EndDraw(None, None);
            res.and(end)
        }
    }

    fn paint_card(
        &self,
        r: &Renderer,
        wf: f32,
        hf: f32,
        lang: ResolvedLang,
        muted_secs: Option<u64>,
    ) -> Result<()> {
        let (glyph, text) = match muted_secs {
            Some(s) => (GLYPH_MUTED, muted_text(lang, s)),
            None => (GLYPH_UNMUTED, t(lang, Key::Unmuted).to_string()),
        };
        let text = text.as_str();
        unsafe {
            let fg = r.rt.CreateSolidColorBrush(&FG, None)?;
            let bg = r.rt.CreateSolidColorBrush(&BG, None)?;
            r.rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: wf,
                        bottom: hf,
                    },
                    radiusX: self.radius,
                    radiusY: self.radius,
                },
                &bg,
            );
            let draw = |s: &str, fmt: &IDWriteTextFormat, top: f32, bottom: f32| {
                let u: Vec<u16> = s.encode_utf16().collect();
                let lr = D2D_RECT_F {
                    left: 0.0,
                    top,
                    right: wf,
                    bottom,
                };
                r.rt.DrawText(
                    &u,
                    fmt,
                    &lr,
                    &fg,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            };
            draw(glyph, &self.icon_fmt, 0.0, self.icon_h);
            draw(text, &self.text_fmt, self.icon_h, hf);
            Ok(())
        }
    }

    fn paint_pill(&self, r: &Renderer, wf: f32, hf: f32, muted_secs: Option<u64>) -> Result<()> {
        let clock = muted_secs.map(fmt_elapsed);
        let p = match muted_secs {
            Some(_) => PillText {
                glyph: GLYPH_MIC_OFF,
                label: PILL_MUTED,
                clock: clock.as_deref(),
                accent: MUTE_RED,
                edge: rgb(BORDER_RGB, 0.8),
            },
            None => PillText {
                glyph: GLYPH_UNMUTED,
                label: PILL_UNMUTED,
                clock: None,
                accent: FG,
                edge: DIM,
            },
        };
        let f = [&self.icon_fmt, &self.text_fmt, &self.clock_fmt];
        let pw = pill_width(r, f, &p, hf)?.min(wf);
        paint_pill(r, f, &p, (wf - pw) * self.align, pw, hf)
    }

    fn show(&self, slot: usize) {
        let size = SIZE {
            cx: self.rect.right - self.rect.left,
            cy: self.rect.bottom - self.rect.top,
        };
        let dst = POINT {
            x: self.rect.left,
            y: self.rect.top,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            // Pill is clickable only while the muted pill (slot 1/2) shows.
            set_click_through(self.hwnd, click_through(self.style, slot > 0));
            SelectObject(self.dc, self.bmp[slot].into());
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&dst),
                Some(&size),
                Some(self.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    fn hide(&self) {
        unsafe {
            set_click_through(self.hwnd, true);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
            );
        }
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        unsafe {
            if !self.old.is_invalid() {
                SelectObject(self.dc, self.old);
            }
            for b in self.bmp {
                if !b.is_invalid() {
                    let _ = DeleteObject(b.into());
                }
            }
            if !self.dc.is_invalid() {
                let _ = DeleteDC(self.dc);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Shared D2D/DWrite objects for one `rebuild`.
pub(crate) struct Renderer {
    pub(crate) rt: ID2D1DCRenderTarget,
    pub(crate) dw: IDWriteFactory,
    pub(crate) icon_family: PCWSTR,
}

impl Renderer {
    pub(crate) fn new() -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            // dpi 96 => 1 DIP == 1 physical pixel; sizes are pre-scaled per monitor.
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = d2d.CreateDCRenderTarget(&props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let icon_family = if has_family(&dw, w!("Segoe Fluent Icons")) {
                w!("Segoe Fluent Icons")
            } else {
                w!("Segoe MDL2 Assets")
            };
            Ok(Self {
                rt,
                dw,
                icon_family,
            })
        }
    }

    fn format(&self, family: PCWSTR, px: f32) -> Result<IDWriteTextFormat> {
        self.format_w(
            family,
            px,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_TEXT_ALIGNMENT_CENTER,
        )
    }

    pub(crate) fn format_w(
        &self,
        family: PCWSTR,
        px: f32,
        weight: DWRITE_FONT_WEIGHT,
        align: windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT,
    ) -> Result<IDWriteTextFormat> {
        unsafe {
            let f = self.dw.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                px,
                w!("en-us"),
            )?;
            f.SetTextAlignment(align)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            Ok(f)
        }
    }

    /// Pill formats `[icon, label, clock]` at popup scale `s`, `pt` physical px per point.
    pub(crate) fn pill_formats(&self, s: f32, pt: f32) -> Result<[IDWriteTextFormat; 3]> {
        let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
        Ok([
            self.format_w(
                self.icon_family,
                12.5 * s * pt,
                DWRITE_FONT_WEIGHT_NORMAL,
                lead,
            )?,
            self.format_w(
                w!("Segoe UI"),
                11.5 * s * pt,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                lead,
            )?,
            self.format_w(
                w!("Segoe UI"),
                11.5 * s * pt,
                DWRITE_FONT_WEIGHT_NORMAL,
                lead,
            )?,
        ])
    }

    fn popup(&self, mon: &Monitor, cfg: &Config, lang: ResolvedLang) -> Result<Popup> {
        let style = cfg.popup_style;
        let rect = popup_rect(
            mon.work,
            cfg.popup_position,
            style,
            cfg.popup_scale,
            mon.dpi,
        );
        let s = cfg.popup_scale;
        let k = mon.dpi as f32 / 96.0;
        let pt = mon.dpi as f32 / 72.0; // physical px per point
        let (icon_fmt, text_fmt, clock_fmt) = match style {
            PopupStyle::Card => (
                self.format(self.icon_family, 22.0 * s * pt)?,
                self.format(w!("Segoe UI"), 9.5 * s * pt)?,
                self.format(w!("Segoe UI"), 9.5 * s * pt)?,
            ),
            PopupStyle::Pill => {
                let [icon, text, clock] = self.pill_formats(s, pt)?;
                (icon, text, clock)
            }
        };
        let align = match cfg.popup_position {
            Position::TopLeft | Position::BottomLeft => 0.0,
            Position::TopRight | Position::BottomRight => 1.0,
            _ => 0.5,
        };
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rect.right - rect.left,
                biHeight: -(rect.bottom - rect.top), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let hwnd = match style {
            PopupStyle::Pill => create_window(PILL_CLASS, rect, true)?, // starts hidden
            PopupStyle::Card => create_window(POPUP_CLASS, rect, true)?,
        };
        let mut p = Popup {
            hwnd,
            dc: unsafe { CreateCompatibleDC(None) },
            old: HGDIOBJ::default(),
            bmp: [HBITMAP::default(); 3],
            rect,
            bmi,
            style,
            align,
            icon_fmt,
            text_fmt,
            clock_fmt,
            icon_h: 60.0 * s * k,
            radius: 14.0 * s * k,
        };
        p.draw(self, 0, lang, None)?;
        p.draw(self, 1, lang, Some(0))?;
        Ok(p)
    }
}

/// Glow strip depth: solid core plus fade, 5x thickness capped at 96 px.
pub fn glow_extent(t: u32) -> u32 {
    (5 * t).min(96).max(t)
}

/// Alpha at `d` px from the screen edge: solid core of `t`, then a quadratic fade
/// from 128 to 0 over the rest of `glow_extent(t)`.
pub fn glow_alpha(d: u32, t: u32) -> u8 {
    let e = glow_extent(t);
    if d < t {
        return 255;
    }
    if d >= e {
        return 0;
    }
    let x = (d - t) as f32 / (e - t) as f32;
    (128.0 * (1.0 - x) * (1.0 - x)).round() as u8
}

/// Paints one glow strip (per-pixel alpha, premultiplied) and pushes it once.
/// The DIB and DC are freed before returning; the window keeps its own copy.
fn paint_glow(h: HWND, r: Rect, mon: Rect, t: u32) -> Result<()> {
    let (w, ht) = (r.right - r.left, r.bottom - r.top);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -ht, // top-down
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
        let bmp = match CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) => b,
            Err(e) => {
                let _ = DeleteDC(dc);
                return Err(e);
            }
        };
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * ht) as usize);
        for y in 0..ht {
            let sy = r.top + y;
            for x in 0..w {
                let sx = r.left + x;
                let d = (sx - mon.left)
                    .min(mon.right - 1 - sx)
                    .min(sy - mon.top)
                    .min(mon.bottom - 1 - sy)
                    .max(0) as u32;
                let a = glow_alpha(d, t) as u32;
                let ch = |sh: u32| ((BORDER_RGB >> sh) & 0xFF) * a / 255;
                px[(y * w + x) as usize] = a << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
            }
        }
        let old = SelectObject(dc, bmp.into());
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let res = UpdateLayeredWindow(
            h,
            None,
            Some(&POINT {
                x: r.left,
                y: r.top,
            }),
            Some(&SIZE { cx: w, cy: ht }),
            Some(dc),
            Some(&POINT::default()),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        res
    }
}

/// Strip alpha `elapsed_ms` into the opening pulse: 3 cosine blinks 255 -> 64 -> 255.
/// `None` once the pulse is over.
pub fn pulse_alpha(elapsed_ms: u32) -> Option<u8> {
    if elapsed_ms >= BLINK_MS * BLINKS {
        return None;
    }
    let ph = std::f32::consts::TAU * elapsed_ms as f32 / BLINK_MS as f32;
    Some((255.0 - 191.0 * (1.0 - ph.cos()) / 2.0).round() as u8)
}

/// Mute duration as `m:ss`, or `h:mm:ss` from one hour on.
pub fn fmt_elapsed(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn muted_text(lang: ResolvedLang, secs: u64) -> String {
    format!("{} · {}", t(lang, Key::Muted), fmt_elapsed(secs))
}

/// Delay to the next clock tick: the middle of the next displayed second, so
/// timer jitter and re-arm drift never skip or repeat a number.
fn clock_delay_ms(elapsed_ms: u128) -> u32 {
    1500 - (elapsed_ms % 1000) as u32
}

fn has_family(dw: &IDWriteFactory, name: PCWSTR) -> bool {
    let mut coll = None;
    unsafe {
        if dw.GetSystemFontCollection(&mut coll, false).is_err() {
            return false;
        }
        let Some(c) = coll else { return false };
        let (mut i, mut ok) = (0u32, BOOL(0));
        c.FindFamilyName(name, &mut i, &mut ok).is_ok() && ok.as_bool()
    }
}

pub struct Indicators {
    popups: Vec<Popup>,
    strips: Vec<HWND>,
    persist: bool,
    /// Strips were built with per-pixel alpha (UpdateLayeredWindow), not LWA_ALPHA.
    glow: bool,
    /// DWM cloak failed: strips are shown/hidden the old (slower) SWP way.
    fallback: bool,
    strips_visible: bool,
    /// Start of the running opening pulse; `None` when idle.
    pulse_start: Option<Instant>,
    /// Current global alpha of the strips (255 when idle).
    alpha: u8,
    /// Kept after `rebuild` so the clock slot can be redrawn on ticks.
    renderer: Option<Renderer>,
    lang: ResolvedLang,
    popup_visible: bool,
    /// Start of the current mute. Survives `rebuild`/`destroy` (display change,
    /// settings reload) so the clock keeps counting; cleared on unmute.
    muted_since: Option<Instant>,
}

impl Indicators {
    pub fn new() -> Self {
        Self {
            popups: Vec::new(),
            strips: Vec::new(),
            persist: false,
            glow: false,
            fallback: false,
            strips_visible: false,
            pulse_start: None,
            alpha: 255,
            renderer: None,
            lang: ResolvedLang::En,
            popup_visible: false,
            muted_since: None,
        }
    }

    pub fn rebuild(&mut self, cfg: &Config, mons: &[&Monitor], lang: ResolvedLang) {
        self.destroy();
        register_classes();
        self.persist = cfg.popup_persist_while_muted;
        self.lang = lang;
        if cfg.popup_enabled {
            match Renderer::new() {
                Ok(r) => {
                    for m in mons {
                        match r.popup(m, cfg, lang) {
                            Ok(p) => self.popups.push(p),
                            Err(e) => crate::log::write(format!(
                                "overlay: popup for {} failed: {e}",
                                m.gdi_name
                            )),
                        }
                    }
                    self.renderer = Some(r);
                }
                Err(e) => crate::log::write(format!("overlay: renderer init failed: {e}")),
            }
        }
        if cfg.border_enabled {
            let t = cfg.border_thickness;
            self.glow = cfg.border_style == BorderStyle::Glow;
            let depth = if self.glow { glow_extent(t) } else { t };
            for m in mons {
                for r in border_strips(m.rect, depth) {
                    let h = match create_window(BORDER_CLASS, r, true) {
                        Ok(h) => h,
                        Err(e) => {
                            crate::log::write(format!("overlay: border strip failed: {e}"));
                            continue;
                        }
                    };
                    if self.glow {
                        if let Err(e) = paint_glow(h, r, m.rect, t) {
                            crate::log::write(format!("overlay: glow strip failed: {e}"));
                            unsafe {
                                let _ = DestroyWindow(h);
                            }
                            continue;
                        }
                    } else {
                        unsafe {
                            let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
                        }
                    }
                    self.strips.push(h);
                }
            }
            // Shown (topmost) once here, cloaked; show/hide only toggles the cloak.
            self.fallback = !self.cloak_strips(true);
            if self.fallback {
                crate::log::write("overlay: DWM cloak failed, using show/hide fallback");
                self.cloak_strips(false);
            } else {
                self.raise_strips(SWP_SHOWWINDOW);
            }
        }
    }

    /// `fresh`: a new mute/unmute event (starts the Glow pulse), not a redraw.
    pub fn show(&mut self, owner: HWND, muted: bool, fresh: bool) {
        if !muted {
            self.muted_since = None;
        } else if fresh || self.muted_since.is_none() {
            self.muted_since = Some(Instant::now());
        }
        self.set_strips(muted);
        if muted && fresh && self.glow {
            self.pulse(owner);
        }
        if self.pulse_start.is_none() && self.alpha != 255 {
            arm_pulse(owner, 1); // a hide cut a pulse short: restore alpha off this path
        }
        if self.popups.is_empty() {
            return;
        }
        // Hot path (fresh mute) shows the pre-rendered "· 0:00"; a rebuild redraw
        // renders the live clock instead so the count carries on.
        let slot = match (muted, fresh) {
            (false, _) => 0,
            (true, true) => 1,
            (true, false) => {
                self.draw_clock();
                2
            }
        };
        for p in &self.popups {
            p.show(slot);
        }
        self.popup_visible = true;
        unsafe {
            if muted && self.persist {
                let _ = KillTimer(Some(owner), TIMER_POPUP_HIDE);
            } else {
                SetTimer(Some(owner), TIMER_POPUP_HIDE, POPUP_HIDE_MS, None);
            }
        }
        // Only a persistent popup ticks; otherwise it hides on "· 0:00".
        if muted && self.persist {
            self.arm_clock(owner);
        }
    }

    /// Re-shows the muted popup with the live clock (talk-while-muted warning).
    /// Hides again on the usual timer unless it persists while muted.
    pub fn flash_popup(&mut self, owner: HWND) {
        if self.popups.is_empty() || self.muted_since.is_none() {
            return;
        }
        self.draw_clock();
        for p in &self.popups {
            p.show(2);
        }
        self.popup_visible = true;
        if self.persist {
            self.arm_clock(owner);
        } else if !owner.is_invalid() {
            unsafe {
                SetTimer(Some(owner), TIMER_POPUP_HIDE, POPUP_HIDE_MS, None);
            }
        }
    }

    /// Renders the current mute duration into every popup's clock slot (2).
    fn draw_clock(&mut self) {
        let (Some(r), Some(t0)) = (&self.renderer, self.muted_since) else {
            return;
        };
        let secs = t0.elapsed().as_secs();
        for p in &mut self.popups {
            if let Err(e) = p.draw(r, 2, self.lang, Some(secs)) {
                crate::log::write(format!("overlay: clock draw failed: {e}"));
            }
        }
    }

    /// TIMER_MUTE_CLOCK handler. The app kills timers on WM_TIMER, so this re-arms
    /// itself; it stops once the mic is unmuted or the popup is hidden.
    pub fn tick_clock(&mut self, owner: HWND) {
        if !self.popup_visible || self.muted_since.is_none() {
            return;
        }
        self.draw_clock();
        for p in &self.popups {
            p.show(2);
        }
        self.arm_clock(owner);
    }

    /// SetTimer for the next clock tick; skipped for a null owner (unit tests).
    fn arm_clock(&self, owner: HWND) {
        if let Some(t0) = self.muted_since
            && !owner.is_invalid()
        {
            let ms = clock_delay_ms(t0.elapsed().as_millis());
            unsafe {
                SetTimer(Some(owner), TIMER_MUTE_CLOCK, ms, None);
            }
        }
    }

    /// Hides everything without showing a state (mic absent).
    pub fn hide_all(&mut self) {
        self.set_strips(false);
        self.hide_popup();
    }

    pub fn hide_popup(&mut self) {
        self.popup_visible = false;
        for p in &self.popups {
            p.hide();
        }
    }

    /// Re-asserts topmost for visible strips; call after the latency-critical show.
    pub fn raise(&self) {
        if self.strips_visible {
            self.raise_strips(SWP_NOREDRAW);
        }
    }

    /// Starts (or restarts) the opening pulse. Hot-path safe: an Instant and a
    /// timer only; alpha changes happen in `tick_pulse`. No-op unless strips are shown.
    pub fn pulse(&mut self, owner: HWND) {
        if !self.strips_visible {
            return;
        }
        self.pulse_start = Some(Instant::now());
        arm_pulse(owner, PULSE_TICK_MS);
    }

    /// TIMER_PULSE handler. The app kills timers on WM_TIMER, so this re-arms while
    /// a pulse runs. With no pulse it only restores full alpha (after a hide) and stops.
    pub fn tick_pulse(&mut self, owner: HWND) {
        let a = self.pulse_start.and_then(|t0| {
            pulse_alpha(u32::try_from(t0.elapsed().as_millis()).unwrap_or(u32::MAX))
        });
        match a {
            Some(a) => {
                self.set_alpha(a);
                arm_pulse(owner, PULSE_TICK_MS);
            }
            None => {
                self.pulse_start = None;
                if self.alpha != 255 {
                    self.set_alpha(255);
                }
            }
        }
    }

    fn set_alpha(&mut self, a: u8) {
        self.alpha = a;
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: a,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        for &h in &self.strips {
            unsafe {
                let _ = if self.glow {
                    UpdateLayeredWindow(
                        h,
                        None,
                        None,
                        None,
                        None,
                        None,
                        COLORREF(0),
                        Some(&blend),
                        ULW_ALPHA,
                    )
                } else {
                    SetLayeredWindowAttributes(h, COLORREF(0), a, LWA_ALPHA)
                };
            }
        }
    }

    pub fn destroy(&mut self) {
        self.strips_visible = false;
        self.glow = false;
        self.pulse_start = None;
        self.alpha = 255;
        self.popup_visible = false;
        self.popups.clear();
        self.renderer = None;
        for h in self.strips.drain(..) {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
    }

    /// Hot path: one cheap DWM cloak per strip, no show/z-order work (that was
    /// ~10-30 ms for 12 strips). Topmost is re-asserted while hidden, off the mute path.
    fn set_strips(&mut self, visible: bool) {
        self.strips_visible = visible && !self.strips.is_empty();
        if !self.fallback && !self.cloak_strips(!visible) {
            crate::log::write("overlay: DWM cloak failed, using show/hide fallback");
            self.fallback = true;
            self.cloak_strips(false);
        }
        if self.fallback {
            self.raise_strips(if visible {
                SWP_SHOWWINDOW
            } else {
                SWP_HIDEWINDOW
            });
        } else if !visible {
            self.raise_strips(SWP_NOREDRAW);
        }
        if !visible {
            // Hot path: no alpha calls here; `show` arms a restore tick instead.
            // A pending pulse tick finds no `pulse_start` and does not re-arm.
            self.pulse_start = None;
        }
    }

    /// True only if every strip's cloak state was set.
    fn cloak_strips(&self, cloak: bool) -> bool {
        let v = BOOL(cloak as i32);
        let mut ok = true;
        for &h in &self.strips {
            ok &= unsafe {
                DwmSetWindowAttribute(
                    h,
                    DWMWA_CLOAK,
                    &v as *const BOOL as *const _,
                    size_of::<BOOL>() as u32,
                )
            }
            .is_ok();
        }
        ok
    }

    fn raise_strips(&self, extra: SET_WINDOW_POS_FLAGS) {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | extra;
        if self.strips.is_empty() {
            return;
        }
        unsafe {
            let Ok(mut hdwp) = BeginDeferWindowPos(self.strips.len() as i32) else {
                return;
            };
            for &h in &self.strips {
                match DeferWindowPos(hdwp, h, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) {
                    Ok(n) => hdwp = n,
                    Err(e) => {
                        // batch already freed by the system
                        crate::log::write(format!("overlay: DeferWindowPos failed: {e}"));
                        return;
                    }
                }
            }
            let _ = EndDeferWindowPos(hdwp);
        }
    }
}

/// SetTimer for the next pulse tick; skipped for a null owner (unit tests).
fn arm_pulse(owner: HWND, ms: u32) {
    if !owner.is_invalid() {
        unsafe {
            SetTimer(Some(owner), TIMER_PULSE, ms, None);
        }
    }
}

impl Drop for Indicators {
    fn drop(&mut self) {
        self.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_through_unless_muted_pill() {
        assert!(!click_through(PopupStyle::Pill, true));
        assert!(click_through(PopupStyle::Pill, false));
        assert!(click_through(PopupStyle::Card, true));
        assert!(click_through(PopupStyle::Card, false));
    }

    #[test]
    fn glow_falloff() {
        assert_eq!(glow_extent(6), 30); // 5t
        assert_eq!(glow_extent(30), 96); // capped
        assert_eq!(glow_alpha(0, 6), 255);
        assert_eq!(glow_alpha(5, 6), 255);
        assert!((110..=140).contains(&glow_alpha(6, 6)));
        assert_eq!(glow_alpha(glow_extent(6), 6), 0);
        for d in 6..30 {
            assert!(glow_alpha(d + 1, 6) <= glow_alpha(d, 6));
        }
    }

    #[test]
    fn pulse_curve() {
        assert_eq!(pulse_alpha(0), Some(255));
        assert!(pulse_alpha(275).unwrap() < 90); // trough of blink 1
        assert_eq!(pulse_alpha(550), Some(255));
        assert!(pulse_alpha(1375).unwrap() < 90); // trough of blink 3
        assert_eq!(pulse_alpha(1650), None);
    }

    #[test]
    fn hide_stops_pulse_then_tick_restores_alpha() {
        let mut ind = Indicators::new();
        ind.pulse(HWND::default()); // no strips: nothing to pulse
        assert_eq!(ind.pulse_start, None);
        ind.pulse_start = Some(Instant::now());
        ind.alpha = 100;
        ind.show(HWND::default(), false, true); // unmute mid-pulse
        assert_eq!(ind.pulse_start, None);
        assert_eq!(ind.alpha, 100); // hot path left alpha alone
        ind.tick_pulse(HWND::default()); // the restore tick
        assert_eq!(ind.alpha, 255);
    }

    #[test]
    fn elapsed_text() {
        assert_eq!(fmt_elapsed(0), "0:00");
        assert_eq!(fmt_elapsed(42), "0:42");
        assert_eq!(fmt_elapsed(3599), "59:59");
        assert_eq!(fmt_elapsed(3600), "1:00:00");
        assert_eq!(fmt_elapsed(36125), "10:02:05");
    }

    #[test]
    fn clock_survives_non_fresh_show() {
        let mut ind = Indicators::new();
        ind.show(HWND::default(), true, true);
        let t0 = ind.muted_since;
        assert!(t0.is_some());
        ind.show(HWND::default(), true, false); // rebuild redraw
        assert_eq!(ind.muted_since, t0);
        ind.destroy();
        assert_eq!(ind.muted_since, t0);
        ind.show(HWND::default(), false, true);
        assert_eq!(ind.muted_since, None);
    }
}
