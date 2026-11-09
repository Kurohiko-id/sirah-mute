//! Layered overlay windows: per-monitor mute popup (Direct2D-rendered, per-pixel
//! alpha) and red border strips. Windows never take focus or mouse input.

use crate::config::Config;
use crate::geometry::{Rect, border_strips, popup_rect};
use crate::i18n::{Key, ResolvedLang, t};
use crate::monitors::Monitor;
use std::sync::Once;
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
    ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
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
    EndDeferWindowPos, HTTRANSPARENT, HWND_TOPMOST, KillTimer, LWA_ALPHA, MA_NOACTIVATE,
    RegisterClassExW, SET_WINDOW_POS_FLAGS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOREDRAW, SWP_NOSIZE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
    ULW_ALPHA, UpdateLayeredWindow, WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, Result, w};

pub const TIMER_POPUP_HIDE: usize = 1;
const POPUP_HIDE_MS: u32 = 1500;
const POPUP_CLASS: PCWSTR = w!("KuroMutePopup");
const BORDER_CLASS: PCWSTR = w!("KuroMuteBorder");
const BORDER_COLOR: COLORREF = COLORREF(0x004D_48E5); // #E5484D as 0x00BBGGRR
const GLYPH_MUTED: &str = "\u{E74F}"; // MicOff (OverlayForm.cs:88)
const GLYPH_UNMUTED: &str = "\u{E720}"; // Microphone

const fn rgb(c: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((c >> 16) & 0xFF) as f32 / 255.0,
        g: ((c >> 8) & 0xFF) as f32 / 255.0,
        b: (c & 0xFF) as f32 / 255.0,
        a,
    }
}
const FG: D2D1_COLOR_F = rgb(0xD7DADF, 1.0);
const BG: D2D1_COLOR_F = rgb(0x131417, 242.0 / 255.0);

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

fn hinstance() -> HINSTANCE {
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
        wc.lpszClassName = BORDER_CLASS;
        wc.hbrBackground = CreateSolidBrush(BORDER_COLOR);
        RegisterClassExW(&wc);
    });
}

fn create_window(class: PCWSTR, r: Rect) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_LAYERED,
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
    /// [unmuted, muted], both selected into `dc` on demand.
    bmp: [HBITMAP; 2],
    rect: Rect,
}

impl Popup {
    fn show(&self, muted: bool) {
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
            SelectObject(self.dc, self.bmp[muted as usize].into());
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
struct Renderer {
    rt: ID2D1DCRenderTarget,
    dw: IDWriteFactory,
    icon_family: PCWSTR,
}

impl Renderer {
    fn new() -> Result<Self> {
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
        unsafe {
            let f = self.dw.CreateTextFormat(
                family,
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                px,
                w!("en-us"),
            )?;
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            Ok(f)
        }
    }

    fn popup(&self, mon: &Monitor, cfg: &Config, lang: ResolvedLang) -> Result<Popup> {
        let rect = popup_rect(mon.work, cfg.popup_position, cfg.popup_scale, mon.dpi);
        let hwnd = create_window(POPUP_CLASS, rect)?;
        let mut p = Popup {
            hwnd,
            dc: unsafe { CreateCompatibleDC(None) },
            old: HGDIOBJ::default(),
            bmp: [HBITMAP::default(); 2],
            rect,
        };
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let s = cfg.popup_scale;
        let k = mon.dpi as f32 / 96.0;
        let pt = mon.dpi as f32 / 72.0; // physical px per point
        let icon_fmt = self.format(self.icon_family, 22.0 * s * pt)?;
        let text_fmt = self.format(w!("Segoe UI"), 9.5 * s * pt)?;
        let icon_h = 60.0 * s * k;
        let (wf, hf) = (w as f32, h as f32);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        for muted in [false, true] {
            let (glyph, key) = if muted {
                (GLYPH_MUTED, Key::Muted)
            } else {
                (GLYPH_UNMUTED, Key::Unmuted)
            };
            unsafe {
                let mut bits = std::ptr::null_mut();
                let bmp = CreateDIBSection(Some(p.dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
                p.bmp[muted as usize] = bmp;
                let prev = SelectObject(p.dc, bmp.into());
                if p.old.is_invalid() {
                    p.old = prev;
                }
                self.rt.BindDC(
                    p.dc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: w,
                        bottom: h,
                    },
                )?;
                let fg = self.rt.CreateSolidColorBrush(&FG, None)?;
                let bg = self.rt.CreateSolidColorBrush(&BG, None)?;
                self.rt.BeginDraw();
                self.rt.Clear(Some(&D2D1_COLOR_F::default()));
                let r = 14.0 * s * k;
                self.rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F {
                            left: 0.0,
                            top: 0.0,
                            right: wf,
                            bottom: hf,
                        },
                        radiusX: r,
                        radiusY: r,
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
                    self.rt.DrawText(
                        &u,
                        fmt,
                        &lr,
                        &fg,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                };
                draw(glyph, &icon_fmt, 0.0, icon_h);
                draw(t(lang, key), &text_fmt, icon_h, hf);
                self.rt.EndDraw(None, None)?;
            }
        }
        Ok(p)
    }
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

#[derive(Default)]
pub struct Indicators {
    popups: Vec<Popup>,
    strips: Vec<HWND>,
    persist: bool,
}

impl Indicators {
    pub fn new() -> Self {
        Self {
            popups: Vec::new(),
            strips: Vec::new(),
            persist: false,
        }
    }

    pub fn rebuild(&mut self, cfg: &Config, mons: &[&Monitor], lang: ResolvedLang) {
        self.destroy();
        register_classes();
        self.persist = cfg.popup_persist_while_muted;
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
                }
                Err(e) => crate::log::write(format!("overlay: renderer init failed: {e}")),
            }
        }
        if cfg.border_enabled {
            for m in mons {
                for r in border_strips(m.rect, cfg.border_thickness) {
                    match create_window(BORDER_CLASS, r) {
                        Ok(h) => {
                            unsafe {
                                let _ = SetLayeredWindowAttributes(h, COLORREF(0), 255, LWA_ALPHA);
                            }
                            self.strips.push(h);
                        }
                        Err(e) => crate::log::write(format!("overlay: border strip failed: {e}")),
                    }
                }
            }
            // Shown (topmost) once here, cloaked; show/hide only toggles the cloak.
            self.cloak_strips(true);
            self.raise_strips(SWP_SHOWWINDOW);
        }
    }

    pub fn show(&mut self, owner: HWND, muted: bool) {
        self.set_strips(muted);
        if self.popups.is_empty() {
            return;
        }
        for p in &self.popups {
            p.show(muted);
        }
        unsafe {
            if muted && self.persist {
                let _ = KillTimer(Some(owner), TIMER_POPUP_HIDE);
            } else {
                SetTimer(Some(owner), TIMER_POPUP_HIDE, POPUP_HIDE_MS, None);
            }
        }
    }

    /// Hides everything without showing a state (mic absent).
    pub fn hide_all(&mut self) {
        self.set_strips(false);
        self.hide_popup();
    }

    pub fn hide_popup(&mut self) {
        for p in &self.popups {
            p.hide();
        }
    }

    pub fn destroy(&mut self) {
        self.popups.clear();
        for h in self.strips.drain(..) {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
    }

    /// Hot path: one cheap DWM cloak per strip, no show/z-order work (that was
    /// ~10-30 ms for 12 strips). Topmost is re-asserted while hidden, off the mute path.
    fn set_strips(&self, visible: bool) {
        self.cloak_strips(!visible);
        if !visible {
            self.raise_strips(SWP_NOREDRAW);
        }
    }

    fn cloak_strips(&self, cloak: bool) {
        let v = BOOL(cloak as i32);
        for &h in &self.strips {
            unsafe {
                let _ = DwmSetWindowAttribute(
                    h,
                    DWMWA_CLOAK,
                    &v as *const BOOL as *const _,
                    size_of::<BOOL>() as u32,
                );
            }
        }
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
                    Err(_) => return, // batch already freed by the system
                }
            }
            let _ = EndDeferWindowPos(hdwp);
        }
    }
}

impl Drop for Indicators {
    fn drop(&mut self) {
        self.destroy();
    }
}
