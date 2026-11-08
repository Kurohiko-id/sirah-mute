use crate::config::Position;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub fn popup_rect(work: Rect, pos: Position, scale: f32, dpi: u32) -> Rect {
    let k = dpi as f32 / 96.0;
    let w = (220.0 * scale * k).round() as i32;
    let h = (100.0 * scale * k).round() as i32;
    let m = (24.0 * k).round() as i32;
    let top = match pos {
        Position::TopLeft | Position::TopCenter | Position::TopRight => work.top + m,
        _ => work.bottom - h - m,
    };
    let left = match pos {
        Position::TopLeft | Position::BottomLeft => work.left + m,
        Position::TopRight | Position::BottomRight => work.right - w - m,
        _ => work.left + (work.right - work.left - w) / 2,
    };
    Rect {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
}

/// Order: top, bottom, left, right. Left/right sit between top and bottom.
pub fn border_strips(mon: Rect, t: u32) -> [Rect; 4] {
    let t = t as i32;
    let r = |left, top, right, bottom| Rect {
        left,
        top,
        right,
        bottom,
    };
    [
        r(mon.left, mon.top, mon.right, mon.top + t),
        r(mon.left, mon.bottom - t, mon.right, mon.bottom),
        r(mon.left, mon.top + t, mon.left + t, mon.bottom - t),
        r(mon.right - t, mon.top + t, mon.right, mon.bottom - t),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    const fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn popup_bottom_center_96() {
        assert_eq!(
            popup_rect(r(0, 0, 1920, 1040), Position::BottomCenter, 1.0, 96),
            r(850, 916, 1070, 1016)
        );
    }

    #[test]
    fn popup_bottom_center_120() {
        assert_eq!(
            popup_rect(r(0, 0, 2560, 1392), Position::BottomCenter, 1.0, 120),
            r(1142, 1237, 1417, 1362)
        );
    }

    #[test]
    fn popup_top_left_negative_origin() {
        assert_eq!(
            popup_rect(r(-1920, 0, 0, 1040), Position::TopLeft, 1.0, 96),
            r(-1896, 24, -1676, 124)
        );
    }

    #[test]
    fn border_strips_layout() {
        assert_eq!(
            border_strips(r(0, 0, 1920, 1080), 11),
            [
                r(0, 0, 1920, 11),
                r(0, 1069, 1920, 1080),
                r(0, 11, 11, 1069),
                r(1909, 11, 1920, 1069)
            ]
        );
    }
}
