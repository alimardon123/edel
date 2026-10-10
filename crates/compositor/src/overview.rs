//! The overview's geometry (M5.2j): every workspace of a screen side by
//! side as a frame of the screen's shape, each window drawn inside its
//! workspace's frame where it lies on the screen, smaller. Pure, so it is
//! tested here; `src/overview.rs` of the binary draws and handles it.

use smithay::utils::{Logical, Point, Rectangle, Size};

/// The room between the frames and round them, in logical pixels.
pub const GAP: i32 = 32;

/// How many frames go in a row: up to four side by side, more in two rows
/// (three for nine), so each stays large enough to tell its windows apart.
fn columns(count: usize) -> usize {
    if count <= 4 {
        count
    } else {
        count.div_ceil(2).min(5)
    }
}

/// The frames of `count` workspaces on a screen at `screen`, each of the
/// screen's shape, centred row by row, a short last row centred too. None
/// for no workspace or an empty screen.
pub fn frames(screen: Rectangle<i32, Logical>, count: usize) -> Vec<Rectangle<i32, Logical>> {
    if count == 0 || screen.size.w <= 0 || screen.size.h <= 0 {
        return Vec::new();
    }
    let cols = columns(count);
    let rows = count.div_ceil(cols);
    let shape = f64::from(screen.size.h) / f64::from(screen.size.w);
    let by_width = f64::from(screen.size.w - GAP * (cols as i32 + 1)) / cols as f64;
    let by_height = f64::from(screen.size.h - GAP * (rows as i32 + 1)) / rows as f64 / shape;
    let w = by_width.min(by_height).max(1.0);
    let size = Size::<i32, Logical>::from((w.floor() as i32, (w * shape).floor().max(1.0) as i32));
    let tall = rows as i32 * size.h + (rows as i32 - 1) * GAP;
    let top = screen.loc.y + (screen.size.h - tall) / 2;
    (0..count)
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            let in_row = (count - row * cols).min(cols) as i32;
            let wide = in_row * size.w + (in_row - 1) * GAP;
            let left = screen.loc.x + (screen.size.w - wide) / 2;
            Rectangle::new(
                Point::from((
                    left + col as i32 * (size.w + GAP),
                    top + row as i32 * (size.h + GAP),
                )),
                size,
            )
        })
        .collect()
}

/// How much smaller than the screen a frame draws it.
pub fn scale(frame: Rectangle<i32, Logical>, screen: Rectangle<i32, Logical>) -> f64 {
    f64::from(frame.size.w) / f64::from(screen.size.w.max(1))
}

/// `point` of the screen where `frame` draws it.
pub fn into(
    frame: Rectangle<i32, Logical>,
    screen: Rectangle<i32, Logical>,
    point: Point<f64, Logical>,
) -> Point<f64, Logical> {
    let k = scale(frame, screen);
    frame.loc.to_f64() + (point - screen.loc.to_f64()).upscale(k)
}

/// `area` of the screen as `frame` draws it.
pub fn shrink(
    frame: Rectangle<i32, Logical>,
    screen: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let k = scale(frame, screen);
    let at = into(frame, screen, area.loc.to_f64()).to_i32_round();
    let size = area.size.to_f64().upscale(k).to_i32_round();
    Rectangle::new(at, size)
}

/// The frame under `point`, if any.
pub fn frame_at(frames: &[Rectangle<i32, Logical>], point: Point<f64, Logical>) -> Option<usize> {
    frames
        .iter()
        .position(|frame| frame.to_f64().contains(point))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new(Point::from((x, y)), Size::from((w, h)))
    }

    #[test]
    fn four_workspaces_lie_side_by_side_in_the_screen_s_shape_and_centred() {
        let screen = rect(0, 0, 1280, 800);
        let placed = frames(screen, 4);
        assert_eq!(placed.len(), 4);
        // (1280 - 5 * 32) / 4 = 280 wide, 175 high, as 1280 by 800.
        assert!(placed.iter().all(|f| f.size == Size::from((280, 175))));
        assert_eq!(placed[0].loc, Point::from((32, (800 - 175) / 2)));
        assert_eq!(placed[3].loc.x, 32 + 3 * (280 + 32));
        assert!(placed.windows(2).all(|p| p[0].loc.y == p[1].loc.y));
    }

    #[test]
    fn more_workspaces_take_two_rows_the_last_one_centred() {
        let screen = rect(0, 0, 1280, 800);
        let placed = frames(screen, 5);
        assert_eq!(placed.len(), 5);
        assert_eq!(placed[0].loc.y, placed[2].loc.y);
        assert!(placed[3].loc.y > placed[0].loc.y);
        // The second row's two frames are centred under the first row's three.
        let first_middle = (placed[0].loc.x + placed[2].loc.x + placed[2].size.w) / 2;
        let second_middle = (placed[3].loc.x + placed[4].loc.x + placed[4].size.w) / 2;
        assert!((first_middle - second_middle).abs() <= 1);
        assert_eq!(frames(screen, 9).len(), 9);
        assert!(frames(screen, 0).is_empty());
        assert!(frames(rect(0, 0, 0, 0), 3).is_empty());
    }

    #[test]
    fn a_window_is_drawn_inside_its_frame_where_it_lies_on_the_screen() {
        let screen = rect(1280, 0, 1280, 800);
        let frame = rect(1312, 100, 320, 200);
        assert_eq!(scale(frame, screen), 0.25);
        let window = rect(1280 + 400, 200, 400, 300);
        assert_eq!(
            shrink(frame, screen, window),
            rect(1312 + 100, 150, 100, 75)
        );
        let at = into(frame, screen, Point::from((1280.0 + 640.0, 400.0)));
        assert_eq!(at, Point::from((1312.0 + 160.0, 200.0)));
    }

    #[test]
    fn the_frame_under_the_pointer_is_found() {
        let placed = frames(rect(0, 0, 1280, 800), 4);
        let middle = |f: Rectangle<i32, Logical>| {
            Point::<f64, Logical>::from((f64::from(f.loc.x + f.size.w / 2), f64::from(f.loc.y + 5)))
        };
        assert_eq!(frame_at(&placed, middle(placed[2])), Some(2));
        assert_eq!(frame_at(&placed, Point::from((2.0, 2.0))), None);
    }
}
