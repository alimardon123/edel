//! The overview's geometry (M5.2j): every workspace of a screen as a small
//! frame of the screen's shape, each window drawn inside its workspace's
//! frame where it lies on the screen, smaller. Since M5.2j-b
//! (`docs/mockups/shell/overview.jpg`) the frames stand in a strip on a
//! tray along one side of the screen ([`Side`], `workspaces.overview_strip`)
//! and the shown workspace's windows are spread out large on the rest
//! ([`plan`], [`spread`]). Pure, so it is tested here; `glance.rs` of the
//! binary draws and handles it.

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

/// The side of the screen the strip of workspaces lies on
/// (`workspaces.overview_strip`, M5.2j-b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    /// The side a value of `workspaces.overview_strip` names; anything
    /// else is the default, the left.
    pub fn named(name: &str) -> Side {
        match name {
            "right" => Side::Right,
            "top" => Side::Top,
            "bottom" => Side::Bottom,
            _ => Side::Left,
        }
    }

    /// Whether the strip runs across the screen (top or bottom) rather
    /// than down it.
    pub fn across(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }
}

/// A small frame's width across the screen (top or bottom) and down a
/// side, logical pixels; its height follows the screen's shape.
pub const FRAME_ACROSS: i32 = 96;
pub const FRAME_DOWN: i32 = 72;
/// Under each frame, its number or name: the room it takes.
pub const LABEL: i32 = 18;
/// Between the strip's frames, across and down.
pub const STRIP_GAP_ACROSS: i32 = 12;
pub const STRIP_GAP_DOWN: i32 = 8;
/// The tray's padding round its frames, and its room from the screen's
/// edge (the panels' edge where a panel lies there).
pub const TRAY_PAD: i32 = 10;
pub const TRAY_EDGE: i32 = 10;
/// The tray's corners.
pub const TRAY_RADIUS: i32 = 18;
/// The room kept between the tray and the spread windows, and round the
/// spread windows, and between them.
pub const ROOM: i32 = 16;
pub const SPREAD_GAP: i32 = 32;
/// Under each spread window, its app and title: the room it takes.
pub const NAME_ROOM: i32 = 28;

/// Where the overview lays one screen out: the tray, the workspaces'
/// frames in it (in order), the frame that adds a workspace (when one
/// more may be added) and the stage where the shown workspace's windows
/// spread.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub tray: Rectangle<i32, Logical>,
    pub frames: Vec<Rectangle<i32, Logical>>,
    pub add: Option<Rectangle<i32, Logical>>,
    pub stage: Rectangle<i32, Logical>,
}

/// The overview of a screen `screen` big whose free area (less its panels)
/// is `area`, with `count` workspaces and, if `add`, a frame to add one,
/// the strip along `side`. The frames keep the screen's shape; the strip
/// is centred along its side; the stage is the rest of the free area less
/// `ROOM`.
pub fn plan(
    screen: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
    count: usize,
    add: bool,
    side: Side,
) -> Plan {
    let shape = if screen.size.w > 0 {
        f64::from(screen.size.h) / f64::from(screen.size.w)
    } else {
        0.625
    };
    let across = side.across();
    let w = if across { FRAME_ACROSS } else { FRAME_DOWN };
    let size = Size::<i32, Logical>::from((w, (f64::from(w) * shape).round() as i32));
    let gap = if across {
        STRIP_GAP_ACROSS
    } else {
        STRIP_GAP_DOWN
    };
    let items = (count + usize::from(add)) as i32;
    let item_long = if across { size.w } else { size.h + LABEL };
    let item_short = if across { size.h + LABEL } else { size.w };
    let long = items * item_long + (items - 1).max(0) * gap + 2 * TRAY_PAD;
    let short = item_short + 2 * TRAY_PAD;
    let (a, z) = (area.loc, area.loc + area.size);
    let tray: Rectangle<i32, Logical> = match side {
        Side::Left => Rectangle::new(
            (a.x + TRAY_EDGE, a.y + (area.size.h - long) / 2).into(),
            (short, long).into(),
        ),
        Side::Right => Rectangle::new(
            (z.x - TRAY_EDGE - short, a.y + (area.size.h - long) / 2).into(),
            (short, long).into(),
        ),
        Side::Top => Rectangle::new(
            (a.x + (area.size.w - long) / 2, a.y + TRAY_EDGE).into(),
            (long, short).into(),
        ),
        Side::Bottom => Rectangle::new(
            (a.x + (area.size.w - long) / 2, z.y - TRAY_EDGE - short).into(),
            (long, short).into(),
        ),
    };
    let place = |i: usize| -> Rectangle<i32, Logical> {
        let step = i as i32 * (item_long + gap);
        let at = if across {
            (tray.loc.x + TRAY_PAD + step, tray.loc.y + TRAY_PAD)
        } else {
            (tray.loc.x + TRAY_PAD, tray.loc.y + TRAY_PAD + step)
        };
        Rectangle::new(at.into(), size)
    };
    let frames = (0..count).map(place).collect();
    let add = add.then(|| place(count));
    let edge = TRAY_EDGE + short + ROOM;
    let stage = match side {
        Side::Left => Rectangle::new(
            (a.x + edge, a.y + ROOM).into(),
            (area.size.w - edge - ROOM, area.size.h - 2 * ROOM).into(),
        ),
        Side::Right => Rectangle::new(
            (a.x + ROOM, a.y + ROOM).into(),
            (area.size.w - edge - ROOM, area.size.h - 2 * ROOM).into(),
        ),
        Side::Top => Rectangle::new(
            (a.x + ROOM, a.y + edge).into(),
            (area.size.w - 2 * ROOM, area.size.h - edge - ROOM).into(),
        ),
        Side::Bottom => Rectangle::new(
            (a.x + ROOM, a.y + ROOM).into(),
            (area.size.w - 2 * ROOM, area.size.h - edge - ROOM).into(),
        ),
    };
    Plan {
        tray,
        frames,
        add,
        stage,
    }
}

/// Where the windows of `sizes` (their frames' sizes, in stacking order)
/// spread out on `stage`: in a grid of the columns that shows them
/// largest, all at one scale no larger than their own size, each centred
/// in its cell, the rows centred, with `NAME_ROOM` under each for its
/// name.
pub fn spread(
    stage: Rectangle<i32, Logical>,
    sizes: &[Size<i32, Logical>],
) -> Vec<Rectangle<i32, Logical>> {
    let n = sizes.len();
    if n == 0 || stage.size.w <= 0 || stage.size.h <= 0 {
        return Vec::new();
    }
    let cell = |cols: usize| -> (i32, i32) {
        let rows = n.div_ceil(cols) as i32;
        let w = (stage.size.w - SPREAD_GAP * (cols as i32 - 1)) / cols as i32;
        let h = (stage.size.h - SPREAD_GAP * (rows - 1)) / rows - NAME_ROOM;
        (w, h)
    };
    let fit = |cols: usize| -> f64 {
        let (w, h) = cell(cols);
        sizes
            .iter()
            .map(|s| {
                (f64::from(w) / f64::from(s.w.max(1)))
                    .min(f64::from(h) / f64::from(s.h.max(1)))
                    .clamp(0.0, 1.0)
            })
            .fold(1.0, f64::min)
    };
    // The columns that give the largest common scale; on a tie, fewer.
    let cols = (1..=n)
        .max_by(|a, b| fit(*a).total_cmp(&fit(*b)).then(b.cmp(a)))
        .unwrap_or(1);
    let k = fit(cols);
    let (cell_w, _) = cell(cols);
    let rows = n.div_ceil(cols);
    let scaled: Vec<Size<i32, Logical>> = sizes
        .iter()
        .map(|s| {
            Size::from((
                (f64::from(s.w) * k).floor() as i32,
                (f64::from(s.h) * k).floor() as i32,
            ))
        })
        .collect();
    // Each row as tall as its tallest window and its name, the rows
    // together centred on the stage.
    let row_h: Vec<i32> = (0..rows)
        .map(|r| {
            scaled[r * cols..((r + 1) * cols).min(n)]
                .iter()
                .map(|s| s.h + NAME_ROOM)
                .max()
                .unwrap_or(0)
        })
        .collect();
    let tall = row_h.iter().sum::<i32>() + SPREAD_GAP * (rows as i32 - 1);
    let mut y = stage.loc.y + (stage.size.h - tall) / 2;
    let mut out = Vec::with_capacity(n);
    for (r, h) in row_h.iter().enumerate() {
        let row = &scaled[r * cols..((r + 1) * cols).min(n)];
        let wide = row.len() as i32 * cell_w + SPREAD_GAP * (row.len() as i32 - 1);
        let left = stage.loc.x + (stage.size.w - wide) / 2;
        for (c, s) in row.iter().enumerate() {
            let x = left + c as i32 * (cell_w + SPREAD_GAP) + (cell_w - s.w) / 2;
            out.push(Rectangle::new(
                (x, y + (h - NAME_ROOM - s.h) / 2).into(),
                *s,
            ));
        }
        y += h + SPREAD_GAP;
    }
    out
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
    fn the_strip_stands_down_the_left_by_default_and_the_stage_takes_the_rest() {
        let screen = rect(0, 0, 1280, 800);
        // A 48 px panel along the bottom.
        let area = rect(0, 0, 1280, 752);
        assert_eq!(Side::named("diagonal"), Side::Left);
        assert_eq!(Side::named("bottom"), Side::Bottom);
        let p = plan(screen, area, 4, true, Side::Left);
        // Frames 72 by 45 (the screen's shape), each with its label under it.
        assert!(p.frames.iter().all(|f| f.size == Size::from((72, 45))));
        assert_eq!(p.frames.len(), 4);
        assert_eq!(p.tray.loc.x, TRAY_EDGE);
        assert_eq!(p.tray.size.w, 72 + 2 * TRAY_PAD);
        // Five items (four and the add frame) of 45 + 18, 8 apart, padded.
        assert_eq!(p.tray.size.h, 5 * 63 + 4 * 8 + 2 * TRAY_PAD);
        // Centred down the free area.
        assert_eq!(p.tray.loc.y, (752 - p.tray.size.h) / 2);
        assert_eq!(p.frames[1].loc.y, p.frames[0].loc.y + 63 + 8);
        let add = p.add.unwrap();
        assert_eq!(add.loc.y, p.frames[3].loc.y + 63 + 8);
        // The stage starts ROOM right of the tray and stays in the area.
        assert_eq!(p.stage.loc.x, p.tray.loc.x + p.tray.size.w + ROOM);
        assert!(p.stage.loc.y + p.stage.size.h <= 752 - ROOM);
        // Nine frames still fit down a 752 px side.
        let nine = plan(screen, area, 9, false, Side::Left);
        assert!(nine.tray.loc.y >= 0 && nine.tray.loc.y + nine.tray.size.h <= 752);
    }

    #[test]
    fn across_the_bottom_the_frames_are_larger_and_the_stage_is_above() {
        let screen = rect(0, 0, 1280, 800);
        let area = rect(0, 0, 1280, 752);
        let p = plan(screen, area, 9, true, Side::Bottom);
        assert!(p.frames.iter().all(|f| f.size == Size::from((96, 60))));
        assert_eq!(p.tray.loc.y + p.tray.size.h, 752 - TRAY_EDGE);
        assert!(p.tray.loc.x >= 0 && p.tray.loc.x + p.tray.size.w <= 1280);
        assert_eq!(p.stage.loc.y, ROOM);
        assert_eq!(p.stage.loc.y + p.stage.size.h, p.tray.loc.y - ROOM);
        let right = plan(screen, area, 4, false, Side::Right);
        assert_eq!(right.tray.loc.x + right.tray.size.w, 1280 - TRAY_EDGE);
        assert_eq!(right.stage.loc.x, ROOM);
        let top = plan(screen, area, 4, false, Side::Top);
        assert_eq!(top.tray.loc.y, TRAY_EDGE);
        assert_eq!(top.stage.loc.y, top.tray.loc.y + top.tray.size.h + ROOM);
    }

    #[test]
    fn windows_spread_in_a_grid_never_larger_than_themselves() {
        let stage = rect(118, 16, 1146, 720);
        assert!(spread(stage, &[]).is_empty());
        // One small window keeps its size, centred.
        let one = spread(stage, &[Size::from((300, 200))]);
        assert_eq!(one[0].size, Size::from((300, 200)));
        assert_eq!(one[0].loc.x, 118 + (1146 - 300) / 2);
        // Two big ones at one scale, inside the stage.
        let two = spread(stage, &[Size::from((1262, 715)), Size::from((800, 600))]);
        assert_eq!(two.len(), 2);
        let k0 = f64::from(two[0].size.w) / 1262.0;
        let k1 = f64::from(two[1].size.w) / 800.0;
        assert!((k0 - k1).abs() < 0.01, "{k0} {k1}");
        let five = spread(stage, &[Size::from((640, 400)); 5]);
        for r in two.iter().chain(five.iter()) {
            assert!(stage.contains_rect(*r), "{r:?} outside {stage:?}");
        }
        // They never overlap.
        for (i, a) in five.iter().enumerate() {
            for b in &five[i + 1..] {
                assert!(!a.overlaps(*b), "{a:?} {b:?}");
            }
        }
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
