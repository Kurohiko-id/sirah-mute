//! Pill group for other watched mics: one layered window per selected monitor,
//! one clickable pill per muted mic. Never takes focus; click-through outside pills.

use crate::config::{Config, Position};
use crate::geometry::{Rect, hit, pill_row, popup_rect};
use crate::monitors::Monitor;
use crate::overlay::{
    BORDER_RGB, GLYPH_MIC_OFF, MUTE_RED, PillText, Renderer, clickable_common, create_window,
    hinstance, paint_pill, pill_width, post_to_core, rgb,
};
use std::cell::RefCell;
use std::sync::Once;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, ClientToScreen,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, RegisterClassExW,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos,
    ULW_ALPHA, UpdateLayeredWindow, WM_CLOSE, WM_LBUTTONUP, WM_NCHITTEST, WNDCLASSEXW,
};
use windows::core::{PCWSTR, Result, w};

const CLASS: PCWSTR = w!("SirahMuteOthers");

thread_local! {
    /// Screen rects of the pills each window shows, for hit tests (core thread only).
    static RECTS: RefCell<Vec<(isize, Vec<Rect>)>> = const { RefCell::new(Vec::new()) };
}

fn pill_at(h: HWND, x: i32, y: i32) -> Option<usize> {
    RECTS.with(|r| {
        let r = r.try_borrow().ok()?;
        let (_, rects) = r.iter().find(|(k, _)| *k == h.0 as isize)?;
        hit(rects, x, y)
    })
}

fn set_rects(h: HWND, rects: Vec<Rect>) {
    RECTS.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|(k, _)| *k != h.0 as isize);
        if !rects.is_empty() {
            r.push((h.0 as isize, rects));
        }
    });
}

/// Signed x/y packed in an lparam.
fn xy(lp: LPARAM) -> (i32, i32) {
    (lp.0 as i16 as i32, (lp.0 >> 16) as i16 as i32)
}

unsafe extern "system" fn proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if let Some(r) = clickable_common(m) {
        return r;
    }
    match m {
        WM_NCHITTEST => {
            let (x, y) = xy(lp);
            let ht = match pill_at(h, x, y) {
                Some(_) => HTCLIENT,
                None => HTTRANSPARENT as u32,
            };
            LRESULT(ht as isize)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            let mut p = POINT { x, y };
            let _ = unsafe { ClientToScreen(h, &mut p) };
            if let Some(i) = pill_at(h, p.x, p.y) {
                post_to_core(crate::msg::WM_APP_PILL_CLICK, 1, i as isize);
            }
            LRESULT(0)
        }
        WM_CLOSE => unsafe { crate::overlay::wndproc(h, m, wp, lp) },
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: hinstance(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

struct Win {
    hwnd: HWND,
    work: Rect,
    /// Physical px per DIP on this monitor.
    k: f32,
    fmts: [IDWriteTextFormat; 3],
    /// Main popup's position and real height on this monitor, if it is enabled.
    main: Option<(Position, i32)>,
}

pub struct OtherPills {
    wins: Vec<Win>,
    renderer: Option<Renderer>,
    scale: f32,
    pos: Position,
}

impl OtherPills {
    pub fn new() -> Self {
        Self {
            wins: Vec::new(),
            renderer: None,
            scale: 1.0,
            pos: Position::BottomLeft,
        }
    }

    pub fn rebuild(&mut self, cfg: &Config, mons: &[&Monitor]) {
        self.destroy();
        // No D2D/DWrite or windows at all unless the feature is in use (RAM budget).
        if !cfg.other_mics.iter().any(|m| m.enabled) {
            return;
        }
        register_class();
        self.scale = cfg.popup_scale;
        self.pos = cfg.other_mics_position;
        let r = match Renderer::new() {
            Ok(r) => r,
            Err(e) => {
                crate::log::write(format!("others: renderer init failed: {e}"));
                return;
            }
        };
        for m in mons {
            let res = r
                .pill_formats(self.scale, m.dpi as f32 / 72.0)
                .and_then(|fmts| {
                    let at = Rect {
                        left: m.work.left,
                        top: m.work.top,
                        right: m.work.left + 1,
                        bottom: m.work.top + 1,
                    };
                    Ok((create_window(CLASS, at, false)?, fmts))
                });
            match res {
                Ok((hwnd, fmts)) => self.wins.push(Win {
                    hwnd,
                    work: m.work,
                    k: m.dpi as f32 / 96.0,
                    fmts,
                    main: cfg.popup_enabled.then(|| {
                        let p = popup_rect(
                            m.work,
                            cfg.popup_position,
                            cfg.popup_style,
                            cfg.popup_scale,
                            m.dpi,
                        );
                        (cfg.popup_position, p.bottom - p.top)
                    }),
                }),
                Err(e) => {
                    crate::log::write(format!("others: window for {} failed: {e}", m.gdi_name))
                }
            }
        }
        self.renderer = Some(r);
    }

    /// Shows one pill per name (in order) on every monitor; empty hides all.
    pub fn show(&mut self, labels: &[String]) {
        let Some(r) = &self.renderer else { return };
        let labels: Vec<String> = labels.iter().map(|n| format!("{n} mute")).collect();
        for win in &self.wins {
            if labels.is_empty() {
                set_rects(win.hwnd, Vec::new());
                swp(win.hwnd, SWP_HIDEWINDOW);
                continue;
            }
            if let Err(e) = self.draw(r, win, &labels) {
                crate::log::write(format!("others: draw failed: {e}"));
                // Never leave an old layout clickable: its indices no longer match.
                set_rects(win.hwnd, Vec::new());
                swp(win.hwnd, SWP_HIDEWINDOW);
            }
        }
    }

    fn draw(&self, r: &Renderer, win: &Win, labels: &[String]) -> Result<()> {
        let k = win.k;
        let pill_h = (44.0 * self.scale * k).round() as i32; // main pill height
        let hf = pill_h as f32;
        let gap = (8.0 * k).round() as i32;
        let margin = (24.0 * k).round() as i32; // popup_rect's margin
        let f = [&win.fmts[0], &win.fmts[1], &win.fmts[2]];
        let pills: Vec<PillText> = labels
            .iter()
            .map(|l| PillText {
                glyph: GLYPH_MIC_OFF,
                label: l,
                clock: None,
                accent: MUTE_RED,
                edge: rgb(BORDER_RGB, 0.8),
            })
            .collect();
        let widths = pills
            .iter()
            .map(|p| Ok(pill_width(r, f, p, hf)?.ceil() as i32))
            .collect::<Result<Vec<i32>>>()?;
        let rects = pill_row(
            &widths,
            gap,
            pill_h,
            win.work,
            margin,
            self.pos,
            win.main,
        );
        let (first, last) = (rects[0], rects[rects.len() - 1]);
        let bb = Rect {
            left: first.left,
            top: first.top,
            right: last.right,
            bottom: first.bottom,
        };
        push(win.hwnd, bb, r, |r| {
            for (p, rc) in pills.iter().zip(&rects) {
                let x = (rc.left - bb.left) as f32;
                paint_pill(r, f, p, x, (rc.right - rc.left) as f32, hf)?;
            }
            Ok(())
        })?;
        set_rects(win.hwnd, rects);
        swp(win.hwnd, SWP_SHOWWINDOW);
        Ok(())
    }

    pub fn destroy(&mut self) {
        for w in self.wins.drain(..) {
            set_rects(w.hwnd, Vec::new());
            unsafe {
                let _ = DestroyWindow(w.hwnd);
            }
        }
        self.renderer = None;
    }
}

impl Drop for OtherPills {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn swp(h: HWND, extra: windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS) {
    unsafe {
        let _ = SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | extra,
        );
    }
}

/// Renders `paint` into a one-shot DIB the size of `bb` and pushes it to `h` at `bb`.
fn push(
    h: HWND,
    bb: Rect,
    r: &Renderer,
    paint: impl FnOnce(&Renderer) -> Result<()>,
) -> Result<()> {
    let (w, ht) = (bb.right - bb.left, bb.bottom - bb.top);
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
        let old = SelectObject(dc, bmp.into());
        let res =
            r.rt.BindDC(
                dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: ht,
                },
            )
            .and_then(|()| {
                r.rt.BeginDraw();
                r.rt.Clear(Some(&D2D1_COLOR_F::default()));
                let painted = paint(r);
                let end = r.rt.EndDraw(None, None);
                painted.and(end)
            })
            .and_then(|()| {
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                UpdateLayeredWindow(
                    h,
                    None,
                    Some(&POINT {
                        x: bb.left,
                        y: bb.top,
                    }),
                    Some(&SIZE { cx: w, cy: ht }),
                    Some(dc),
                    Some(&POINT::default()),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )
            });
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        res
    }
}
