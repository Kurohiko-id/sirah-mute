use crate::config::{PopupStyle, Position};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub fn popup_rect(work: Rect, pos: Position, style: PopupStyle, scale: f32, dpi: u32) -> Rect {
    let k = dpi as f32 / 96.0;
    // Pill: the window is wider than the drawn pill, which hugs its text.
    let (bw, bh) = match style {
        PopupStyle::Card => (220.0, 100.0),
        PopupStyle::Pill => (240.0, 44.0),
    };
    let w = (bw * scale * k).round() as i32;
    let h = (bh * scale * k).round() as i32;
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

/// Screen rects of a row of pills (widths in order) at `pos` in `work`. When the
/// main popup sits at the same position (`main_pos`), the row moves one pill
/// height + gap away from the screen edge so the two never overlap.
#[allow(clippy::too_many_arguments)]
pub fn pill_row(
    widths: &[i32],
    gap: i32,
    pill_h: i32,
    work: Rect,
    margin: i32,
    pos: Position,
    main_pos: Option<Position>,
) -> Vec<Rect> {
    let total = widths.iter().sum::<i32>() + gap * (widths.len() as i32 - 1).max(0);
    let top_side = matches!(
        pos,
        Position::TopLeft | Position::TopCenter | Position::TopRight
    );
    let dodge = if main_pos == Some(pos) {
        pill_h + gap
    } else {
        0
    };
    let top = if top_side {
        work.top + margin + dodge
    } else {
        work.bottom - margin - pill_h - dodge
    };
    let mut left = match pos {
        Position::TopLeft | Position::BottomLeft => work.left + margin,
        Position::TopRight | Position::BottomRight => work.right - margin - total,
        _ => work.left + (work.right - work.left - total) / 2,
    };
    widths
        .iter()
        .map(|&w| {
            let r = Rect {
                left,
                top,
                right: left + w,
                bottom: top + pill_h,
            };
            left += w + gap;
            r
        })
        .collect()
}

/// Index of the rect containing (x, y), if any.
pub fn hit(rects: &[Rect], x: i32, y: i32) -> Option<usize> {
    rects
        .iter()
        .position(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
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
            popup_rect(
                r(0, 0, 1920, 1040),
                Position::BottomCenter,
                PopupStyle::Card,
                1.0,
                96
            ),
            r(850, 916, 1070, 1016)
        );
    }

    #[test]
    fn popup_bottom_center_120() {
        assert_eq!(
            popup_rect(
                r(0, 0, 2560, 1392),
                Position::BottomCenter,
                PopupStyle::Card,
                1.0,
                120
            ),
            r(1142, 1237, 1417, 1362)
        );
    }

    #[test]
    fn popup_top_left_negative_origin() {
        assert_eq!(
            popup_rect(
                r(-1920, 0, 0, 1040),
                Position::TopLeft,
                PopupStyle::Card,
                1.0,
                96
            ),
            r(-1896, 24, -1676, 124)
        );
    }

    #[test]
    fn popup_pill_top_right() {
        assert_eq!(
            popup_rect(
                r(0, 0, 1920, 1040),
                Position::TopRight,
                PopupStyle::Pill,
                1.0,
                96
            ),
            r(1656, 24, 1896, 68)
        );
    }

    #[test]
    fn pill_row_left_center_right() {
        let work = r(0, 0, 1000, 800);
        let row = |pos| pill_row(&[100, 120], 8, 40, work, 24, pos, None);
        let bl = row(Position::BottomLeft);
        assert_eq!(bl, vec![r(24, 736, 124, 776), r(132, 736, 252, 776)]);
        assert_eq!(row(Position::BottomRight)[1].right, 976);
        let tc = row(Position::TopCenter);
        assert_eq!(tc[0].left, (1000 - 228) / 2);
        assert_eq!(tc[0].top, 24);
    }

    #[test]
    fn pill_row_dodges_main_popup() {
        let work = r(0, 0, 1000, 800);
        let row = |pos, main| pill_row(&[100, 120], 8, 40, work, 24, pos, main);
        let bl = row(Position::BottomLeft, Some(Position::BottomLeft));
        assert_eq!(bl[0].bottom, 776 - 48);
        let tl = row(Position::TopLeft, Some(Position::TopLeft));
        assert_eq!(tl[0].top, 24 + 48);
        // a popup elsewhere does not move the row
        assert_eq!(
            row(Position::TopLeft, Some(Position::BottomLeft))[0].top,
            24
        );
    }

    #[test]
    fn hit_finds_pill() {
        let rects = pill_row(
            &[100, 120],
            8,
            40,
            r(0, 0, 1000, 800),
            24,
            Position::BottomLeft,
            None,
        );
        assert_eq!(hit(&rects, 30, 750), Some(0));
        assert_eq!(hit(&rects, 140, 750), Some(1));
        assert_eq!(hit(&rects, 128, 750), None); // gap
        assert_eq!(hit(&rects, 30, 700), None);
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
