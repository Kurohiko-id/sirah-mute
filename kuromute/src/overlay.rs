//! Layered overlay windows: per-monitor mute popup (Direct2D-rendered, per-pixel
//! alpha) and red border strips. Windows never take focus or mouse input.

use crate::config::{BorderStyle, Config};
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
    EndDeferWindowPos, FindWindowW, HTTRANSPARENT, HWND_TOPMOST, KillTimer, LWA_ALPHA,
    MA_NOACTIVATE, PostMessageW, RegisterClassExW, SET_WINDOW_POS_FLAGS, SWP_HIDEWINDOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ULW_ALPHA, UpdateLayeredWindow, WM_CLOSE,
    WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, Result, w};

pub const TIMER_POPUP_HIDE: usize = 1;
pub const TIMER_PULSE: usize = 6;
const PULSE_TICK_MS: u32 = 33;
const BLINK_MS: u32 = 550;
const BLINKS: u32 = 3;
const POPUP_HIDE_MS: u32 = 1500;
const POPUP_CLASS: PCWSTR = w!("KuroMutePopup");
const BORDER_CLASS: PCWSTR = w!("KuroMuteBorder");
const BORDER_COLOR: COLORREF = COLORREF(0x004D_48E5); // #E5484D as 0x00BBGGRR
const BORDER_RGB: u32 = 0xE5484D;
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
    pub pulse_start: Option<Instant>,
    /// Current global alpha of the strips (255 when idle).
    pub alpha: u8,
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
            let t = cfg.border_thickness;
            self.glow = cfg.border_style == BorderStyle::Glow;
            let depth = if self.glow { glow_extent(t) } else { t };
            for m in mons {
                for r in border_strips(m.rect, depth) {
                    let h = match create_window(BORDER_CLASS, r) {
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
        self.set_strips(muted);
        if muted && fresh && self.glow {
            self.pulse(owner);
        }
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
        arm_pulse(owner);
    }

    /// TIMER_PULSE handler. The app kills timers on WM_TIMER, so this re-arms.
    pub fn tick_pulse(&mut self, owner: HWND) {
        let Some(t0) = self.pulse_start else { return };
        let ms = u32::try_from(t0.elapsed().as_millis()).unwrap_or(u32::MAX);
        match pulse_alpha(ms) {
            Some(a) => {
                self.set_alpha(a);
                arm_pulse(owner);
            }
            None => self.stop_pulse(),
        }
    }

    /// Ends the pulse at full alpha. A pending tick (at most one) finds no
    /// `pulse_start` and does not re-arm, so no KillTimer (and no owner) is needed.
    fn stop_pulse(&mut self) {
        self.pulse_start = None;
        if self.alpha != 255 {
            self.set_alpha(255);
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
        self.popups.clear();
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
            self.stop_pulse(); // after the hide, so a mid-pulse unmute isn't slowed
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
fn arm_pulse(owner: HWND) {
    if !owner.is_invalid() {
        unsafe {
            SetTimer(Some(owner), TIMER_PULSE, PULSE_TICK_MS, None);
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
    fn stop_pulse_resets_alpha() {
        let mut ind = Indicators::new();
        ind.pulse(HWND::default()); // no strips: nothing to pulse
        assert_eq!(ind.pulse_start, None);
        ind.pulse_start = Some(Instant::now());
        ind.alpha = 100;
        ind.hide_all();
        assert_eq!(ind.pulse_start, None);
        assert_eq!(ind.alpha, 255);
    }
}
