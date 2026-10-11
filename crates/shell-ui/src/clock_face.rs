//! The lock screen's clock faces and the date's format (M5.10c5b): the
//! text each face and the date show, pure and tested without a display,
//! and the drawing of the faces with tiny-skia and the shell's text. The
//! options are `edel::lock_screen`'s (`face`, `hours`, `size`, `weight`
//! of a clock; `show` and `style` of a date), read from the settings file's
//! `lock_screen.parts`; `lock.rs` lays the parts out and calls these.

// Nothing calls these until `lock.rs` draws the parts with them (the next
// part of M5.10c5b); this line goes then, so the compiler checks every use.
#![allow(dead_code)]

use edel::lock_screen::{DatePart, DateStyle, Face, Size};
use edel::tokens::Colour;
use tiny_skia::{LineCap, PathBuilder, Pixmap, Stroke, Transform};

use crate::calendar;
use crate::paint::{self, Text};
use crate::popup::Rect;

/// A line's height as a multiple of its size, as `Text` sets it.
const LINE: f32 = 1.25;
/// Stacked: each line's size as a share of the clock's size.
const STACKED: f32 = 0.8;
/// Stacked: the lines stand this far apart as a share of their height.
const STACK_STEP: f32 = 0.95;
/// Minimal: the size as a share of the clock's size.
const MINIMAL: f32 = 0.55;
/// Minimal: the weight, thin.
const MINIMAL_WEIGHT: u16 = 200;
/// Analog: the dial's width as a share of the clock's size.
const ANALOG: f32 = 1.3;
/// Analog: the ring's width in logical pixels, and its colour's share.
const RING: f32 = 2.0;
const RING_SHARE: f32 = 0.5;
/// Analog: a tick's outer end and its inner end, as shares of the radius;
/// the quarters' inner end is lower, so they are longer.
const TICK_OUTER: f32 = 0.92;
const TICK_INNER: f32 = 0.84;
const TICK_QUARTER_INNER: f32 = 0.76;
/// Analog: the ticks' widths in logical pixels, the quarters' wider.
const TICK: f32 = 2.0;
const TICK_QUARTER: f32 = 3.0;
/// Analog: the hour hand reaches this share of the radius and is this many
/// logical pixels wide; the minute hand reaches further and is thinner.
const HOUR_HAND: f32 = 0.6;
const HOUR_WIDTH: f32 = 6.0;
const MINUTE_HAND: f32 = 0.85;
const MINUTE_WIDTH: f32 = 4.0;
/// Analog: the dot in the middle, its radius in logical pixels.
const DOT: f32 = 4.0;
/// The widest figures a clock shows: tabular, so every digit has one width.
const SAMPLE: &str = "00:00";

/// The figures of a clock in `weight`, with one width for each digit.
fn figures(weight: u16) -> paint::Face {
    paint::Face {
        weight,
        tabular: true,
    }
}

/// The share of the token's clock size a clock's size takes: large is all of
/// it, medium two thirds, small a half. Absent is large.
pub fn size_share(size: Option<Size>) -> f32 {
    match size {
        None | Some(Size::Large) => 1.0,
        Some(Size::Medium) => 2.0 / 3.0,
        Some(Size::Small) => 0.5,
    }
}

/// The clock's text: one line for the digital and minimal faces, `["14:06"]`
/// in 24 hours or `["2:06"]` in 12 (no leading zero, no AM or PM), two for
/// the stacked face, `["14", "06"]` (the hour without a leading zero only
/// in 12 hours), and none for the analog face, which is drawn. Midnight and
/// noon are `12:00` in 12 hours.
pub fn time_lines(hour: i8, minute: i8, face: Face, hours: u8) -> Vec<String> {
    let hour = hour.rem_euclid(24);
    let minute = format!("{minute:02}");
    let hour = if hours == 12 {
        let hour = if hour % 12 == 0 { 12 } else { hour % 12 };
        hour.to_string()
    } else {
        format!("{hour:02}")
    };
    match face {
        Face::Digital | Face::Minimal => vec![format!("{hour}:{minute}")],
        Face::Stacked => vec![hour, minute],
        Face::Analog => Vec::new(),
    }
}

/// The first three characters of a name, by characters so a translation's
/// letters are not cut in the middle.
fn short(name: &str) -> String {
    name.chars().take(3).collect()
}

/// The date's text: the parts it shows (`show`, in the order weekday, day,
/// month, year whatever order `show` lists them), each only when shown, in
/// `style`. `weekday` is 1 for Monday to 7 for Sunday.
///
/// Long is `Saturday 3 October 2026`; Short is `Sat 3 Oct 2026`; Numeric is
/// `03.10.2026`, a weekday shown as its short name before it (`Sat 03.10.2026`),
/// and a part not shown is left out with its dot.
pub fn date_text(
    weekday: i8,
    day: i8,
    month: i8,
    year: i16,
    show: &[DatePart],
    style: DateStyle,
) -> String {
    let has = |part: DatePart| show.contains(&part);
    match style {
        DateStyle::Long | DateStyle::Short => {
            let short_names = style == DateStyle::Short;
            let mut words = Vec::new();
            if has(DatePart::Weekday) {
                let name = calendar::weekday(weekday);
                words.push(if short_names {
                    short(name)
                } else {
                    name.to_string()
                });
            }
            if has(DatePart::Day) {
                words.push(day.to_string());
            }
            if has(DatePart::Month) {
                let name = calendar::name(month);
                words.push(if short_names {
                    short(name)
                } else {
                    name.to_string()
                });
            }
            if has(DatePart::Year) {
                words.push(year.to_string());
            }
            words.join(" ")
        }
        DateStyle::Numeric => {
            let mut numbers = Vec::new();
            if has(DatePart::Day) {
                numbers.push(format!("{day:02}"));
            }
            if has(DatePart::Month) {
                numbers.push(format!("{month:02}"));
            }
            if has(DatePart::Year) {
                numbers.push(year.to_string());
            }
            let dates = numbers.join(".");
            let mut text = String::new();
            if has(DatePart::Weekday) {
                text.push_str(&short(calendar::weekday(weekday)));
            }
            if !dates.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&dates);
            }
            text
        }
    }
}

/// The width and height of a face at `clock_size` logical pixels, for the
/// layout. Digital and minimal are one line, stacked two, analog a square
/// dial (`clock_size` times 1.3 across). `weight` is the clock's weight; the
/// minimal face is always thin.
pub fn size(face: Face, clock_size: f32, text: &mut Text, weight: u16) -> (f32, f32) {
    match face {
        Face::Digital => {
            let line = text.line_in(SAMPLE, clock_size, figures(weight));
            (line.width, clock_size * LINE)
        }
        Face::Stacked => {
            let size = clock_size * STACKED;
            let line = text.line_in("00", size, figures(weight));
            let line_h = size * LINE;
            (line.width, line_h + line_h * STACK_STEP)
        }
        Face::Minimal => {
            let size = clock_size * MINIMAL;
            let line = text.line_in(SAMPLE, size, figures(MINIMAL_WEIGHT));
            (line.width, size * LINE)
        }
        Face::Analog => {
            let across = clock_size * ANALOG;
            (across, across)
        }
    }
}

/// The point `radius` from `centre` at `turns` of a circle, clockwise from
/// twelve o'clock.
fn point(centre: (f32, f32), radius: f32, turns: f32) -> (f32, f32) {
    let angle = turns * std::f32::consts::TAU;
    (
        centre.0 + radius * angle.sin(),
        centre.1 - radius * angle.cos(),
    )
}

/// One stroke from `from` to `to` in pixels, round at both ends.
fn stroke(pixmap: &mut Pixmap, from: (f32, f32), to: (f32, f32), width: f32, colour: Colour) {
    let mut path = PathBuilder::new();
    path.move_to(from.0, from.1);
    path.line_to(to.0, to.1);
    if let Some(path) = path.finish() {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &path,
            &paint::paint_of(colour),
            &stroke,
            Transform::identity(),
            None,
        );
    }
}

/// Draws the lines of text centred on `centre` (pixels), each `size` logical
/// pixels high in `weight`, one below the other as the face has them.
fn write_lines(
    pixmap: &mut Pixmap,
    text: &mut Text,
    lines: &[String],
    (size, weight): (f32, u16),
    centre: (f32, f32),
    colour: Colour,
    s: f32,
) {
    let px = size * s;
    let line_h = px * LINE;
    let step = line_h * STACK_STEP;
    let total = line_h + step * lines.len().saturating_sub(1) as f32;
    let mut top = centre.1 - total / 2.0;
    for words in lines {
        let mut line = text.line_in(words, px, figures(weight));
        let left = centre.0 - line.width / 2.0;
        text.draw(pixmap, &mut line, left, top, colour);
        top += step;
    }
}

/// Draws the analog dial centred on `centre` (pixels), `radius` pixels out,
/// with its ring, ticks and two hands at `now`'s hour and minute. There is no
/// second hand: it would redraw every second.
fn dial(
    pixmap: &mut Pixmap,
    centre: (f32, f32),
    radius: f32,
    (hour, minute): (i8, i8),
    colour: Colour,
    s: f32,
) {
    let ring = Colour {
        a: RING_SHARE,
        ..colour
    };
    paint::outline(
        pixmap,
        (
            centre.0 - radius,
            centre.1 - radius,
            2.0 * radius,
            2.0 * radius,
        ),
        radius,
        RING * s,
        ring,
    );
    for k in 0..12 {
        let quarter = k % 3 == 0;
        let (inner, width) = if quarter {
            (TICK_QUARTER_INNER, TICK_QUARTER)
        } else {
            (TICK_INNER, TICK)
        };
        let turns = k as f32 / 12.0;
        let from = point(centre, radius * inner, turns);
        let to = point(centre, radius * TICK_OUTER, turns);
        stroke(pixmap, from, to, width * s, colour);
    }
    let hours = (f32::from(hour.rem_euclid(12)) + f32::from(minute) / 60.0) / 12.0;
    let minutes = f32::from(minute) / 60.0;
    let hour_end = point(centre, radius * HOUR_HAND, hours);
    stroke(pixmap, centre, hour_end, HOUR_WIDTH * s, colour);
    let minute_end = point(centre, radius * MINUTE_HAND, minutes);
    stroke(pixmap, centre, minute_end, MINUTE_WIDTH * s, colour);
    paint::fill(
        pixmap,
        centre.0 - DOT * s,
        centre.1 - DOT * s,
        2.0 * DOT * s,
        2.0 * DOT * s,
        DOT * s,
        colour,
    );
}

/// Draws a clock's face into `rect` (logical pixels), centred in it, at
/// `now`'s time (hour, minute, second; the second is not shown) in `hours`
/// (12 or 24). `clock_size` is the face's size, the token times
/// `size_share`, and `weight` its weight; `s` is the scale drawn at.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    pixmap: &mut Pixmap,
    rect: Rect,
    now: (i8, i8, i8),
    face: Face,
    hours: u8,
    clock_size: f32,
    weight: u16,
    colour: Colour,
    text: &mut Text,
    s: f32,
) {
    let centre = ((rect.x + rect.w / 2.0) * s, (rect.y + rect.h / 2.0) * s);
    let lines = time_lines(now.0, now.1, face, hours);
    match face {
        Face::Analog => {
            let radius = clock_size * ANALOG / 2.0 * s;
            dial(pixmap, centre, radius, (now.0, now.1), colour, s);
        }
        Face::Digital => {
            write_lines(
                pixmap,
                text,
                &lines,
                (clock_size, weight),
                centre,
                colour,
                s,
            );
        }
        Face::Stacked => {
            let size = clock_size * STACKED;
            write_lines(pixmap, text, &lines, (size, weight), centre, colour, s);
        }
        Face::Minimal => {
            let size = clock_size * MINIMAL;
            write_lines(
                pixmap,
                text,
                &lines,
                (size, MINIMAL_WEIGHT),
                centre,
                colour,
                s,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::i18n::{parse_po, with_catalogue};
    use edel::tokens::Tokens;

    fn text() -> Text {
        Text::load(&Tokens::built_in().font)
    }

    #[test]
    fn digital_and_minimal_show_the_time_in_24_or_12_hours() {
        for face in [Face::Digital, Face::Minimal] {
            assert_eq!(time_lines(14, 6, face, 24), ["14:06"]);
            assert_eq!(time_lines(14, 6, face, 12), ["2:06"]);
            assert_eq!(time_lines(9, 5, face, 24), ["09:05"]);
            assert_eq!(time_lines(0, 0, face, 12), ["12:00"], "midnight");
            assert_eq!(time_lines(12, 0, face, 12), ["12:00"], "noon");
            assert_eq!(time_lines(0, 0, face, 24), ["00:00"]);
        }
    }

    #[test]
    fn stacked_shows_the_hour_over_the_minutes() {
        assert_eq!(time_lines(14, 6, Face::Stacked, 24), ["14", "06"]);
        assert_eq!(time_lines(14, 6, Face::Stacked, 12), ["2", "06"]);
        assert_eq!(time_lines(9, 5, Face::Stacked, 24), ["09", "05"]);
        assert_eq!(time_lines(0, 0, Face::Stacked, 12), ["12", "00"]);
        assert_eq!(time_lines(12, 30, Face::Stacked, 12), ["12", "30"]);
    }

    #[test]
    fn analog_is_drawn_and_writes_no_text() {
        assert!(time_lines(14, 6, Face::Analog, 24).is_empty());
        assert!(time_lines(14, 6, Face::Analog, 12).is_empty());
    }

    #[test]
    fn the_date_in_long_is_the_lock_screen_text() {
        let saturday = jiff::civil::date(2026, 10, 3);
        assert_eq!(saturday.weekday().to_monday_one_offset(), 6);
        let shown = [DatePart::Weekday, DatePart::Day, DatePart::Month];
        let text = date_text(6, 3, 10, 2026, &shown, DateStyle::Long);
        assert_eq!(text, "Saturday 3 October");
        assert_eq!(text, saturday.strftime("%A %-d %B").to_string());
    }

    #[test]
    fn each_style_writes_the_date_its_way() {
        let all = [
            DatePart::Weekday,
            DatePart::Day,
            DatePart::Month,
            DatePart::Year,
        ];
        assert_eq!(
            date_text(6, 3, 10, 2026, &all, DateStyle::Long),
            "Saturday 3 October 2026"
        );
        assert_eq!(
            date_text(6, 3, 10, 2026, &all, DateStyle::Short),
            "Sat 3 Oct 2026"
        );
        assert_eq!(
            date_text(6, 3, 10, 2026, &all, DateStyle::Numeric),
            "Sat 03.10.2026"
        );
    }

    #[test]
    fn a_part_not_shown_is_left_out_with_its_dot() {
        let numeric = |show: &[DatePart]| date_text(6, 3, 10, 2026, show, DateStyle::Numeric);
        assert_eq!(numeric(&[DatePart::Day, DatePart::Month]), "03.10");
        assert_eq!(numeric(&[DatePart::Weekday, DatePart::Year]), "Sat 2026");
        assert_eq!(numeric(&[DatePart::Year]), "2026");
        assert_eq!(numeric(&[DatePart::Weekday]), "Sat");
        assert_eq!(
            date_text(
                6,
                3,
                10,
                2026,
                &[DatePart::Month, DatePart::Day],
                DateStyle::Long
            ),
            "3 October",
            "the order shown is weekday, day, month, year whatever the list says"
        );
    }

    #[test]
    fn a_year_alone_is_the_year_in_each_style() {
        for style in [DateStyle::Long, DateStyle::Short, DateStyle::Numeric] {
            assert_eq!(date_text(6, 3, 10, 2026, &[DatePart::Year], style), "2026");
        }
    }

    #[test]
    fn a_translated_date_keeps_its_letters_whole() {
        // Cyrillic names are two bytes a letter: the short name takes three
        // characters, not three bytes.
        let russian = parse_po(
            "msgid \"Saturday\"\nmsgstr \"Суббота\"\n\n\
             msgid \"October\"\nmsgstr \"Октябрь\"\n",
        );
        with_catalogue(russian, || {
            let all = [
                DatePart::Weekday,
                DatePart::Day,
                DatePart::Month,
                DatePart::Year,
            ];
            assert_eq!(
                date_text(6, 3, 10, 2026, &all, DateStyle::Long),
                "Суббота 3 Октябрь 2026"
            );
            assert_eq!(
                date_text(6, 3, 10, 2026, &all, DateStyle::Short),
                "Суб 3 Окт 2026"
            );
            assert_eq!(
                date_text(6, 3, 10, 2026, &all, DateStyle::Numeric),
                "Суб 03.10.2026"
            );
        });
    }

    #[test]
    fn the_size_shares_are_two_thirds_and_a_half() {
        assert_eq!(size_share(None), 1.0);
        assert_eq!(size_share(Some(Size::Large)), 1.0);
        assert_eq!(size_share(Some(Size::Medium)), 2.0 / 3.0);
        assert_eq!(size_share(Some(Size::Small)), 0.5);
    }

    #[test]
    fn every_face_has_a_size_and_the_analog_one_is_square() {
        let mut text = text();
        for face in [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal] {
            let (w, h) = size(face, 150.0, &mut text, 250);
            assert!(w > 0.0 && h > 0.0, "{face:?}: {w} by {h}");
        }
        let (w, h) = size(Face::Analog, 150.0, &mut text, 250);
        assert_eq!(w, h);
        assert_eq!(w, 150.0 * ANALOG);
    }

    /// Four faces at 14:06 in `colour`, each in its `rect`, on `pixmap`.
    fn draw_all(pixmap: &mut Pixmap, rects: &[Rect], colour: Colour, text: &mut Text) {
        let faces = [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal];
        for (face, rect) in faces.into_iter().zip(rects) {
            draw(
                pixmap,
                *rect,
                (14, 6, 0),
                face,
                24,
                90.0,
                250,
                colour,
                text,
                1.0,
            );
        }
    }

    /// Whether a pixel of `pixmap` at `x`, `y` has any ink.
    fn inked(pixmap: &Pixmap, x: u32, y: u32) -> bool {
        pixmap.pixel(x, y).is_some_and(|p| p.alpha() > 0)
    }

    #[test]
    fn each_face_draws_inside_its_rect_and_nowhere_else() {
        let colour = Tokens::built_in().backdrop_text;
        for face in [Face::Digital, Face::Stacked, Face::Analog, Face::Minimal] {
            let mut pixmap = Pixmap::new(600, 300).unwrap();
            let rect = Rect::new(100.0, 50.0, 400.0, 200.0);
            let mut text = text();
            draw(
                &mut pixmap,
                rect,
                (14, 6, 0),
                face,
                24,
                90.0,
                250,
                colour,
                &mut text,
                1.0,
            );
            let mut inside = 0;
            for y in 0..300 {
                for x in 0..600 {
                    let within = (x as f32) >= rect.x
                        && (x as f32) < rect.x + rect.w
                        && (y as f32) >= rect.y
                        && (y as f32) < rect.y + rect.h;
                    if inked(&pixmap, x, y) {
                        assert!(within, "{face:?} inks {x},{y} outside its rect");
                        inside += 1;
                    }
                }
            }
            assert!(inside > 100, "{face:?} drew only {inside} pixels");
        }
    }

    #[test]
    fn the_four_faces_side_by_side_as_a_picture() {
        let Ok(dir) = std::env::var("EDEL_LOCK_PNG") else {
            return;
        };
        let tokens = Tokens::built_in();
        let mut text = text();
        let (width, height) = (1200, 300);
        let mut pixmap = Pixmap::new(width, height).unwrap();
        paint::fill(
            &mut pixmap,
            0.0,
            0.0,
            width as f32,
            height as f32,
            0.0,
            tokens.lock_backdrop_deep,
        );
        let names = ["Digital", "Stacked", "Analog", "Minimal"];
        let rects: Vec<Rect> = (0..4)
            .map(|i| Rect::new(i as f32 * 300.0, 0.0, 300.0, 240.0))
            .collect();
        draw_all(&mut pixmap, &rects, tokens.backdrop_text, &mut text);
        for (name, rect) in names.iter().zip(&rects) {
            let mut line = text.line(name, 14.0);
            let left = rect.x + rect.w / 2.0 - line.width / 2.0;
            text.draw(&mut pixmap, &mut line, left, 262.0, tokens.backdrop_text);
        }
        pixmap.save_png(format!("{dir}/clock-faces.png")).unwrap();
    }
}
