use crate::geometry::Rect;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
    MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EDD_GET_DEVICE_INTERFACE_NAME, MONITORINFOF_PRIMARY,
};
use windows::core::{BOOL, PCWSTR};

#[derive(Clone, Debug)]
pub struct Monitor {
    pub rect: Rect,
    pub work: Rect,
    pub dpi: u32,
    pub primary: bool,
    pub gdi_name: String,
    pub stable_id: String,
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

unsafe extern "system" fn on_monitor(hmon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<Monitor>) };
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(hmon, &mut mi as *mut _ as *mut MONITORINFO) }.as_bool() {
        return true.into();
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    if unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_err() || dx == 0 {
        dx = 96;
    }
    let mut dd = DISPLAY_DEVICEW {
        cb: size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    let stable_id = if unsafe {
        EnumDisplayDevicesW(
            PCWSTR(mi.szDevice.as_ptr()),
            0,
            &mut dd,
            EDD_GET_DEVICE_INTERFACE_NAME,
        )
    }
    .as_bool()
    {
        wstr(&dd.DeviceID)
    } else {
        String::new()
    };
    out.push(Monitor {
        rect: rect(mi.monitorInfo.rcMonitor),
        work: rect(mi.monitorInfo.rcWork),
        dpi: dx,
        primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        gdi_name: wstr(&mi.szDevice),
        stable_id,
    });
    true.into()
}

pub fn enumerate() -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(on_monitor),
            LPARAM(&mut out as *mut _ as isize),
        );
    }
    out
}

/// A saved id names a monitor if it equals the stable id (non-empty) or the GDI name (any case).
pub fn id_matches(id: &str, stable_id: &str, gdi_name: &str) -> bool {
    (!id.is_empty() && id == stable_id) || id.eq_ignore_ascii_case(gdi_name)
}

pub fn select<'a>(all: &'a [Monitor], all_monitors: bool, ids: &[String]) -> Vec<&'a Monitor> {
    if all_monitors {
        return all.iter().collect();
    }
    let hit: Vec<&Monitor> = all
        .iter()
        .filter(|m| {
            ids.iter()
                .any(|id| id_matches(id, &m.stable_id, &m.gdi_name))
        })
        .collect();
    if !hit.is_empty() {
        return hit;
    }
    all.iter()
        .find(|m| m.primary)
        .or(all.first())
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, id: &str, primary: bool) -> Monitor {
        let z = Rect {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        Monitor {
            rect: z,
            work: z,
            dpi: 96,
            primary,
            gdi_name: name.into(),
            stable_id: id.into(),
        }
    }
    fn three() -> Vec<Monitor> {
        vec![
            mon(r"\\.\DISPLAY1", "ID-A", true),
            mon(r"\\.\DISPLAY2", "ID-B", false),
            mon(r"\\.\DISPLAY3", "ID-C", false),
        ]
    }

    #[test]
    fn all_monitors_returns_all() {
        let v = three();
        assert_eq!(select(&v, true, &[]).len(), 3);
    }

    #[test]
    fn matches_gdi_name_case_insensitive() {
        let v = three();
        let s = select(&v, false, &[r"\\.\display2".to_string()]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].gdi_name, r"\\.\DISPLAY2");
    }

    #[test]
    fn matches_stable_id() {
        let v = three();
        let s = select(&v, false, &["ID-C".to_string()]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].gdi_name, r"\\.\DISPLAY3");
    }

    #[test]
    fn unplugged_falls_back_to_primary() {
        let v = three();
        let s = select(&v, false, &[r"\\.\DISPLAY9".to_string()]);
        assert_eq!(s.len(), 1);
        assert!(s[0].primary);
    }

    #[test]
    fn empty_id_never_matches_empty_stable_id() {
        let mut v = three();
        v[1].stable_id = String::new();
        let s = select(&v, false, &[String::new()]);
        assert_eq!(s.len(), 1);
        assert!(s[0].primary);
    }

    #[test]
    fn no_primary_falls_back_to_first() {
        let mut v = three();
        v[0].primary = false;
        let s = select(&v, false, &["x".to_string()]);
        assert_eq!(s[0].gdi_name, r"\\.\DISPLAY1");
    }

    #[test]
    #[ignore]
    fn print_monitors() {
        for m in enumerate() {
            println!("{m:#?}");
        }
    }
}
