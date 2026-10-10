//! The workspace switcher (M5.2c, M5.2m), in the look `layout.workspaces_look`
//! names. `numbers` (the default) draws round buttons numbered as Super+1 to
//! Super+9 are, the shown workspace a wider pill in the accent colour, at
//! most `layout.workspaces_shown` (3 by default) at once, the shown
//! workspace and its neighbours, so the panel keeps its width whatever the
//! count. Where more lie, the next button peeks in at that side, and
//! `layout.workspaces_ends` says how: faded (`fade`), with a small arrow
//! over the fade (`arrows`, the default) or with how many lie beyond it
//! (`counts`). A workspace with a name shows its name in its pill, and every
//! button is as wide as the widest label, so the width does not change as the
//! shown workspace does. `button` draws the word Workspaces over an underline
//! split into one segment per workspace, the shown one in the accent colour.
//! Either look: a click shows the workspace under it, a scroll the next or
//! the previous one, through ext-workspace-v1 (`crate::workspaces`). Sizes
//! are logical pixels, drawn at the panel's scale.
//!
//! What the widget shows is text, so it can be compared and read back
//! ([`read`]): its fields are separated by `\u{1e}` and its names by
//! `\u{1f}`, and a name keeps no control character, so no name can hide a
//! separator. A button finds its workspace by its place, never by its name.

use accesskit::Role;
use edel::i18n::{n_, tr, trf};
use edel::presets::MOST_WORKSPACES;
use edel::settings::{WORKSPACES_ENDS, WORKSPACES_LOOK, WORKSPACES_SHOWN};
use edel::tokens::{Colour, Tokens};
use tiny_skia::{FillRule, LineCap, LineJoin, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

use super::{Action, Canvas, Input, Live, Widget};
use crate::paint::{Face, Text, fill, mix, paint_of, rounded};

pub const WIDGET: Widget = Widget {
    name: "workspaces",
    title: n_("Workspaces"),
    needs: None,
    shows,
    width,
    draw,
    input,
    parts: super::no_parts,
    role: Role::Group,
    label,
};

/// How many buttons show at once when the person says none.
pub const DEFAULT_SHOWN: usize = 3;
/// A button's height, and the least width of a label's button.
const BUTTON: f32 = 20.0;
/// The shown workspace's pill is this much wider than a button.
const PILL: f32 = 12.0;
/// The room a label's button adds beside its text.
const LABEL_ROOM: f32 = 12.0;
/// A label longer than this many characters is cut to one less and an
/// ellipsis.
const LABEL_MOST: usize = 10;
const GAP: f32 = 5.0;
/// How much of the next button peeks in where more workspaces lie, fading
/// out towards the widget's end (`fade` and `arrows`).
const PEEK: f32 = 14.0;
/// The same for `counts`, which needs room for its "+N".
const COUNTS: f32 = 20.0;
/// Space on each side, between it and its neighbours.
const ROOM: f32 = 6.0;
/// The arrow over a peek (`arrows`): 6 px tall, 3 px deep, a 1.5 px stroke.
const ARROW_HALF: f32 = 3.0;
const ARROW_DEPTH: f32 = 3.0;
const ARROW_STROKE: f32 = 1.5;
/// The button look: the word's line top rises this far above the middle,
/// the underline sits this far below the word's line, is this high, and
/// its segments are this far apart.
const WORD_RISE: f32 = 9.0;
const UNDER_DROP: f32 = 3.0;
const UNDER_HIGH: f32 = 2.0;
const SEGMENT_GAP: f32 = 2.0;
/// Separates the fields of [`shows`] and the names among them.
const FIELD: &str = "\u{1e}";
const NAME: &str = "\u{1f}";

/// How the switcher looks (`layout.workspaces_look`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Look {
    /// Round buttons, numbered or named (the default).
    #[default]
    Numbers,
    /// The word Workspaces over an underline, one segment per workspace.
    Button,
}

impl Look {
    /// The value the settings file names it by.
    pub fn name(self) -> &'static str {
        match self {
            Look::Numbers => "numbers",
            Look::Button => "button",
        }
    }

    fn of(text: &str) -> Option<Look> {
        match text {
            "numbers" => Some(Look::Numbers),
            "button" => Some(Look::Button),
            _ => None,
        }
    }
}

/// What the numbers look shows where more workspaces lie than show
/// (`layout.workspaces_ends`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ends {
    /// The next button, faded.
    Fade,
    /// The next button, faded, with a small arrow over the fade (the default).
    #[default]
    Arrows,
    /// The next button, faded, with how many lie beyond it over the fade.
    Counts,
}

impl Ends {
    /// The value the settings file names it by.
    pub fn name(self) -> &'static str {
        match self {
            Ends::Fade => "fade",
            Ends::Arrows => "arrows",
            Ends::Counts => "counts",
        }
    }

    fn of(text: &str) -> Option<Ends> {
        match text {
            "fade" => Some(Ends::Fade),
            "arrows" => Some(Ends::Arrows),
            "counts" => Some(Ends::Counts),
            _ => None,
        }
    }
}

/// The switcher's settings as the machine's and the person's files say
/// them (M5.2m): a missing or unknown value is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub look: Look,
    pub shown: usize,
    pub ends: Ends,
}

impl Settings {
    pub fn read(machine: Option<&str>, person: Option<&str>) -> Settings {
        let chosen = |key: &str| edel::settings::chosen(key, machine, person);
        Settings {
            look: chosen(WORKSPACES_LOOK)
                .and_then(|v| Look::of(&v))
                .unwrap_or_default(),
            shown: chosen(WORKSPACES_SHOWN)
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| (1..=MOST_WORKSPACES).contains(n))
                .unwrap_or(DEFAULT_SHOWN),
            ends: chosen(WORKSPACES_ENDS)
                .and_then(|v| Ends::of(&v))
                .unwrap_or_default(),
        }
    }
}

/// What it shows, read back from [`shows`]' text.
#[derive(Debug, Clone, PartialEq)]
pub struct View<'a> {
    pub look: Look,
    pub shown: usize,
    pub ends: Ends,
    /// The first button shown, from 0 (the numbers look).
    pub first: usize,
    /// The shown workspace's place, from 0.
    pub active: usize,
    /// Every workspace's label, in order.
    pub names: Vec<&'a str>,
}

/// The view `text` says; a text without fields, as no workspace gives,
/// says no names.
pub fn read(text: &str) -> View<'_> {
    let fields: Vec<&str> = text.split(FIELD).collect();
    match fields[..] {
        [look, shown, ends, first, active, names] => View {
            look: Look::of(look).unwrap_or_default(),
            shown: shown
                .parse::<usize>()
                .unwrap_or(DEFAULT_SHOWN)
                .clamp(1, MOST_WORKSPACES),
            ends: Ends::of(ends).unwrap_or_default(),
            first: first.parse().unwrap_or(0),
            active: active.parse().unwrap_or(0),
            names: if names.is_empty() {
                Vec::new()
            } else {
                names.split(NAME).collect()
            },
        },
        _ => View {
            look: Look::default(),
            shown: DEFAULT_SHOWN,
            ends: Ends::default(),
            first: 0,
            active: 0,
            names: Vec::new(),
        },
    }
}

/// What it shows: the look, how many show, the ends, the first shown, the
/// shown workspace's place and every workspace's label, each name with its
/// control characters taken out, or its number when none is left. Empty
/// without workspaces, as when the compositor offers no ext-workspace-v1.
fn shows(live: &Live) -> String {
    if live.workspaces.is_empty() {
        return String::new();
    }
    let count = live.workspaces.len();
    let shown = live.workspaces_shown.clamp(1, MOST_WORKSPACES);
    let active = live.workspaces.iter().position(|(_, on)| *on).unwrap_or(0);
    let first = first_shown(active, count, shown, live.view);
    let names: Vec<String> = live
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let name: String = name.chars().filter(|c| !c.is_control()).collect();
            if name.is_empty() {
                (i + 1).to_string()
            } else {
                name
            }
        })
        .collect();
    [
        live.workspaces_look.name().to_string(),
        shown.to_string(),
        live.workspaces_ends.name().to_string(),
        first.to_string(),
        active.to_string(),
        names.join(NAME),
    ]
    .join(FIELD)
}

/// The first button shown: the one before the shown workspace, or where a
/// scroll left the view, kept so that `shown` show when there are that many.
pub fn first_shown(active: usize, count: usize, shown: usize, view: Option<usize>) -> usize {
    view.unwrap_or(active.saturating_sub(1))
        .min(count.saturating_sub(shown))
}

/// What a label shows: its name, or the name cut to nine characters and an
/// ellipsis when it is longer than ten.
pub fn cut(name: &str) -> String {
    if name.chars().count() > LABEL_MOST {
        let kept: String = name.chars().take(LABEL_MOST - 1).collect();
        format!("{kept}\u{2026}")
    } else {
        name.to_string()
    }
}

/// `words` in `face` at the small panel text size, in logical pixels: its
/// width, or 0 without fonts.
pub fn measure(
    text: Option<&mut Text>,
    tokens: &Tokens,
    scale: f32,
    words: &str,
    face: Face,
) -> f32 {
    let size = tokens.panel_text_small_size as f32 * scale;
    text.map_or(0.0, |text| text.line_in(words, size, face).width / scale)
}

/// The width of the word Workspaces in the button look, in logical pixels.
pub fn word_width(text: &mut Text, tokens: &Tokens, scale: f32) -> f32 {
    measure(Some(text), tokens, scale, tr("Workspaces"), Face::REGULAR)
}

/// A button's width in logical pixels, from `measure` (a text's width in a
/// face): the widest label's width plus `LABEL_ROOM`, and never less than
/// `BUTTON`. It is the same whichever workspace shows.
pub fn slot(names: &[&str], measure: &mut dyn FnMut(&str, Face) -> f32) -> f32 {
    let widest = names
        .iter()
        .map(|name| measure(&cut(name), Face::SEMIBOLD.tabular()))
        .fold(0.0, f32::max);
    (widest + LABEL_ROOM).max(BUTTON)
}

/// Whether more workspaces lie than show, so that each end has a peek.
fn more(view: &View) -> bool {
    view.names.len() > view.shown
}

/// How wide an end's strip is.
fn end_width(ends: Ends) -> f32 {
    match ends {
        Ends::Counts => COUNTS,
        Ends::Fade | Ends::Arrows => PEEK,
    }
}

/// A button of the numbers look: its workspace's place and label, whether
/// it is the shown one, and its left edge and width from the widget's left,
/// in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub place: usize,
    pub label: String,
    pub on: bool,
    pub left: f32,
    pub width: f32,
}

/// The buttons the numbers look shows: at most `shown`, the shown one a pill.
pub fn buttons(view: &View, slot: f32) -> Vec<Button> {
    let mut x = ROOM
        + if more(view) {
            end_width(view.ends) + GAP
        } else {
            0.0
        };
    let mut out = Vec::new();
    for (place, name) in view
        .names
        .iter()
        .enumerate()
        .skip(view.first)
        .take(view.shown)
    {
        let on = place == view.active;
        let width = if on { slot + PILL } else { slot };
        out.push(Button {
            place,
            label: cut(name),
            on,
            left: x,
            width,
        });
        x += width + GAP;
    }
    out
}

/// The place of the shown button under `x` logical pixels from the widget's
/// left, in the numbers look, as a click finds it (M5.2p): none over the
/// room, a peek, a gap, or in the button look. `slot` is the buttons' width
/// as [`slot`] gives it for the names.
pub fn button_at(shown: &str, slot: f32, x: f32) -> Option<usize> {
    let view = read(shown);
    if view.look != Look::Numbers {
        return None;
    }
    buttons(&view, slot)
        .iter()
        .find(|b| (b.left..b.left + b.width).contains(&x))
        .map(|b| b.place)
}

/// The place a dragged button lands on when it is let go at `x` logical
/// pixels from the widget's left, in the numbers look (M5.2p): the shown
/// button under `x`; left of the first or right of the last, the first or
/// the last shown; in a gap, the nearer button. None in the button look.
pub fn landing(shown: &str, slot: f32, x: f32) -> Option<usize> {
    let view = read(shown);
    if view.look != Look::Numbers {
        return None;
    }
    let distance = |b: &Button| {
        if x < b.left {
            b.left - x
        } else if x > b.left + b.width {
            x - b.left - b.width
        } else {
            0.0
        }
    };
    buttons(&view, slot)
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .map(|b| b.place)
}

/// The names of the workspaces after the one at place `from` moves to place
/// `to` (M5.2p), as `layout.workspace_names` lists them: padded with empty
/// names to `count` (or to as many names as there are), the moved name's
/// place as the compositor's `Desks::reorder` moves it, then the empty names
/// at the end taken off. A place past the list is left as it is.
pub fn reordered_names(names: &[String], count: usize, from: usize, to: usize) -> Vec<String> {
    let mut list = names.to_vec();
    list.resize(count.max(names.len()), String::new());
    if from < list.len() && to < list.len() {
        let name = list.remove(from);
        list.insert(to, name);
    }
    while list.last().is_some_and(String::is_empty) {
        list.pop();
    }
    list
}

/// The next workspace where more lie, at one end of the strip: its place
/// and label, its strip's left edge and width, its button's width (all in
/// logical pixels), whether it lies on the left, and how many lie beyond
/// that side, itself included.
#[derive(Debug, Clone, PartialEq)]
pub struct Peek {
    pub place: usize,
    pub label: String,
    pub left: f32,
    pub width: f32,
    pub button: f32,
    pub on_left: bool,
    pub beyond: usize,
}

/// The peeks of the numbers look: one at each side where more lie.
pub fn peeks(view: &View, slot: f32) -> Vec<Peek> {
    let mut out = Vec::new();
    if !more(view) {
        return out;
    }
    let width = end_width(view.ends);
    if view.first > 0 {
        out.push(Peek {
            place: view.first - 1,
            label: cut(view.names[view.first - 1]),
            left: ROOM,
            width,
            button: slot,
            on_left: true,
            beyond: view.first,
        });
    }
    let after = view.first + view.shown;
    if let Some(name) = view.names.get(after) {
        out.push(Peek {
            place: after,
            label: cut(name),
            left: logical_width(view, slot) - ROOM - width,
            width,
            button: slot,
            on_left: false,
            beyond: view.names.len() - after,
        });
    }
    out
}

/// The numbers look's width in logical pixels: the room, the ends where more
/// lie, and the buttons it shows with the pill among them.
pub fn logical_width(view: &View, slot: f32) -> f32 {
    if view.names.is_empty() {
        return 0.0;
    }
    let shown = view.names.len().min(view.shown) as f32;
    let ends = if more(view) {
        2.0 * (end_width(view.ends) + GAP)
    } else {
        0.0
    };
    2.0 * ROOM + ends + (slot + PILL) + (shown - 1.0) * (slot + GAP)
}

/// The button look's width in logical pixels, for the word's width.
pub fn word_button_width(word: f32) -> f32 {
    word + 2.0 * ROOM
}

/// The underline's segments, one per workspace: each one's left edge from
/// the widget's left and its width, in logical pixels, the word's width
/// shared out with a gap between them.
pub fn segments(count: usize, word: f32) -> Vec<(f32, f32)> {
    if count == 0 {
        return Vec::new();
    }
    let each = ((word - (count - 1) as f32 * SEGMENT_GAP) / count as f32).max(0.0);
    (0..count)
        .map(|i| (ROOM + i as f32 * (each + SEGMENT_GAP), each))
        .collect()
}

/// The segment nearest `at`, so that a gap between two belongs to the nearer.
pub fn nearest(segments: &[(f32, f32)], at: f32) -> Option<usize> {
    let distance = |&(left, width): &(f32, f32)| {
        if at < left {
            left - at
        } else if at > left + width {
            at - left - width
        } else {
            0.0
        }
    };
    segments
        .iter()
        .enumerate()
        .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
        .map(|(i, _)| i)
}

/// The underline's segments as the log says them (`edel-shell-ui: workspace
/// segments`): `LEFT+WIDTH` in logical pixels from the widget's left,
/// comma-separated. `None` for the numbers look and without workspaces.
pub fn segments_line(shown: &str, word: f32) -> Option<String> {
    let view = read(shown);
    if view.look != Look::Button || view.names.is_empty() {
        return None;
    }
    let list: Vec<String> = segments(view.names.len(), word)
        .iter()
        .map(|(left, width)| format!("{left:.1}+{width:.1}"))
        .collect();
    Some(list.join(","))
}

/// The text on a `counts` peek: how many lie beyond it.
fn beyond_text(n: usize) -> String {
    format!("+{n}")
}

/// Draws a workspace's button, `on` if it is the shown one, at `x`, `y`
/// in `pixmap`, `w` by `h` pixels.
#[allow(clippy::too_many_arguments)]
fn button(
    pixmap: &mut Pixmap,
    text: Option<&mut Text>,
    tokens: &Tokens,
    name: &str,
    on: bool,
    (x, y, w, h): (f32, f32, f32, f32),
    size: f32,
) {
    let off = Colour {
        a: 0.14,
        ..tokens.panel_text
    };
    if let Some(path) = rounded(x, y, w, h, h / 2.0) {
        let fill = paint_of(if on { tokens.accent } else { off });
        pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
    }
    if let Some(text) = text {
        let mut line = text.line_in(name, size, Face::SEMIBOLD.tabular());
        let tx = x + (w - line.width) / 2.0;
        let ty = y + (h - size * 1.25) / 2.0;
        let ink = if on {
            tokens.accent_text
        } else {
            mix(tokens.panel_text, tokens.panel, 0.3)
        };
        text.draw(pixmap, &mut line, tx, ty, ink);
    }
}

/// The arrow over a peek (`arrows`): a chevron pointing out of the widget
/// (to the left when `left`), centred at `x`, `y` in pixels, `s` pixels per
/// logical pixel.
fn chevron(pixmap: &mut Pixmap, x: f32, y: f32, left: bool, s: f32, ink: Colour) {
    let (half, depth) = (ARROW_HALF * s, ARROW_DEPTH * s / 2.0);
    let (tip, back) = if left {
        (x - depth, x + depth)
    } else {
        (x + depth, x - depth)
    };
    let mut path = PathBuilder::new();
    path.move_to(back, y - half);
    path.line_to(tip, y);
    path.line_to(back, y + half);
    let Some(path) = path.finish() else {
        return;
    };
    let stroke = Stroke {
        width: ARROW_STROKE * s,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint_of(ink), &stroke, Transform::identity(), None);
}

/// The width of a button's label or the word in logical pixels, measured on
/// the canvas.
fn measure_on(canvas: &mut Canvas, words: &str, face: Face) -> f32 {
    measure(
        canvas.text.as_deref_mut(),
        canvas.tokens,
        canvas.scale,
        words,
        face,
    )
}

/// The numbers look's button width, measured on the canvas.
fn slot_on(canvas: &mut Canvas, view: &View) -> f32 {
    slot(&view.names, &mut |words, face| {
        measure_on(canvas, words, face)
    })
}

/// The word's width, measured on the canvas.
fn word_on(canvas: &mut Canvas) -> f32 {
    measure_on(canvas, tr("Workspaces"), Face::REGULAR)
}

fn width(canvas: &mut Canvas, shown: &str) -> f32 {
    let view = read(shown);
    if view.names.is_empty() {
        return 0.0;
    }
    let logical = match view.look {
        Look::Numbers => {
            let slot = slot_on(canvas, &view);
            logical_width(&view, slot)
        }
        Look::Button => word_button_width(word_on(canvas)),
    };
    logical * canvas.scale
}

fn draw(canvas: &mut Canvas, shown: &str, x: f32) {
    let view = read(shown);
    if view.names.is_empty() {
        return;
    }
    match view.look {
        Look::Numbers => draw_numbers(canvas, &view, x),
        Look::Button => draw_button(canvas, &view, x),
    }
}

fn draw_numbers(canvas: &mut Canvas, view: &View, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let height = BUTTON * s;
    let y = canvas.top + ((canvas.height - height) / 2.0).round();
    let size = tokens.panel_text_small_size as f32 * s;
    let slot = slot_on(canvas, view);
    for b in buttons(view, slot) {
        let place = ((x + b.left * s).round(), y, b.width * s, height);
        button(
            canvas.pixmap,
            canvas.text.as_deref_mut(),
            tokens,
            &b.label,
            b.on,
            place,
            size,
        );
    }
    // The next button at each side where more lie: drawn whole into a
    // strip as wide as the end, the part beyond it cut off, then faded
    // towards the widget's end, and an arrow over it; `counts` shows only
    // how many lie there, as a faded button under it would blur the
    // figure.
    let strip = (end_width(view.ends) * s).round();
    let muted = mix(tokens.panel_text, tokens.panel, 0.3);
    for p in peeks(view, slot) {
        let (px, py) = ((x + p.left * s).round(), y);
        let cx = px + strip * if p.on_left { 0.25 } else { 0.75 };
        let cy = y + height / 2.0;
        if let Ends::Counts = view.ends {
            if let Some(text) = canvas.text.as_deref_mut() {
                let mut line = text.line_in(&beyond_text(p.beyond), size, Face::SEMIBOLD.tabular());
                let tx = px + (strip - line.width) / 2.0;
                let ty = y + (height - size * 1.25) / 2.0;
                text.draw(canvas.pixmap, &mut line, tx, ty, muted);
            }
            continue;
        }
        let Some(mut peek) = Pixmap::new(strip as u32, height.ceil() as u32) else {
            continue;
        };
        let bx = if p.on_left { strip - p.button * s } else { 0.0 };
        button(
            &mut peek,
            canvas.text.as_deref_mut(),
            tokens,
            &p.label,
            false,
            (bx, 0.0, p.button * s, height),
            size,
        );
        fade(&mut peek, p.on_left);
        canvas.pixmap.draw_pixmap(
            px as i32,
            py as i32,
            peek.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        // At the middle of the strip's outer half.
        if let Ends::Arrows = view.ends {
            chevron(canvas.pixmap, cx, cy, p.on_left, s, muted);
        }
    }
}

fn draw_button(canvas: &mut Canvas, view: &View, x: f32) {
    let s = canvas.scale;
    let tokens = canvas.tokens;
    let size = tokens.panel_text_small_size as f32 * s;
    let word = word_on(canvas);
    let top = (canvas.top + canvas.height / 2.0 - WORD_RISE * s).round();
    if let Some(text) = canvas.text.as_deref_mut() {
        let mut line = text.line_in(tr("Workspaces"), size, Face::REGULAR);
        text.draw(
            canvas.pixmap,
            &mut line,
            (x + ROOM * s).round(),
            top,
            tokens.panel_text,
        );
    }
    let under = (top + size * 1.25 + UNDER_DROP * s).round();
    for (i, (left, width)) in segments(view.names.len(), word).into_iter().enumerate() {
        let colour = if i == view.active {
            tokens.accent
        } else {
            Colour {
                a: 0.25,
                ..tokens.panel_text
            }
        };
        fill(
            canvas.pixmap,
            x + left * s,
            under,
            width * s,
            UNDER_HIGH * s,
            0.0,
            colour,
        );
    }
}

/// Fades `pixmap` out towards its left side if `to_left`, else its right:
/// full at the inner side, nothing at the outer one.
fn fade(pixmap: &mut Pixmap, to_left: bool) {
    let w = pixmap.width() as usize;
    for (i, pixel) in pixmap.data_mut().chunks_exact_mut(4).enumerate() {
        let column = (i % w) as f32 + 0.5;
        let kept = if to_left {
            column / w as f32
        } else {
            1.0 - column / w as f32
        };
        for channel in pixel {
            *channel = (f32::from(*channel) * kept).round() as u8;
        }
    }
}

/// What a click at `at` does in the numbers look: a peek scrolls the view
/// one step towards it, a button shows its workspace.
fn click_numbers(view: &View, slot: f32, at: f32) -> Option<Action> {
    let within = |left: f32, width: f32| (left..left + width).contains(&at);
    if let Some(peek) = peeks(view, slot).iter().find(|p| within(p.left, p.width)) {
        let first = if peek.on_left {
            view.first - 1
        } else {
            view.first + 1
        };
        return Some(Action::View(first));
    }
    buttons(view, slot)
        .iter()
        .find(|b| within(b.left, b.width))
        .map(|b| Action::Show(b.place))
}

/// What a click at `at` does in the button look: the nearest segment's
/// workspace.
fn click_button(view: &View, word: f32, at: f32) -> Option<Action> {
    nearest(&segments(view.names.len(), word), at).map(Action::Show)
}

/// A scroll: the numbers look moves its view a step, keeping `shown` in
/// sight; the button look shows the next or the previous workspace. Both
/// stop at the ends.
fn scroll(view: &View, steps: i32) -> Option<Action> {
    let count = view.names.len();
    match view.look {
        Look::Numbers => {
            let last = count.saturating_sub(view.shown) as i64;
            let to = (view.first as i64 + i64::from(steps)).clamp(0, last) as usize;
            (to != view.first).then_some(Action::View(to))
        }
        Look::Button => {
            let last = count.saturating_sub(1) as i64;
            let to = (view.active as i64 + i64::from(steps)).clamp(0, last) as usize;
            (count > 0 && to != view.active).then_some(Action::Show(to))
        }
    }
}

fn input(canvas: &mut Canvas, shown: &str, what: Input) -> Option<Action> {
    let view = read(shown);
    match (what, view.look) {
        (Input::Click(at, _), Look::Numbers) => {
            let slot = slot_on(canvas, &view);
            click_numbers(&view, slot, at)
        }
        (Input::Click(at, _), Look::Button) => {
            let word = word_on(canvas);
            click_button(&view, word, at)
        }
        (Input::Scroll(steps), _) => scroll(&view, steps),
        (Input::Menu(..), _) => None,
    }
}

/// The shown workspace among how many: "Workspaces: 2 shown, 2 of 4".
fn label(shown: &str) -> String {
    let view = read(shown);
    match view.names.get(view.active).copied() {
        Some(name) => trf(
            "Workspaces: {name} shown, {n} of {count}",
            &[
                ("name", name),
                ("n", &(view.active + 1).to_string()),
                ("count", &view.names.len().to_string()),
            ],
        ),
        None => tr("Workspaces").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each character 7 logical pixels wide, as a stand-in for the font.
    fn seven(words: &str, _: Face) -> f32 {
        words.chars().count() as f32 * 7.0
    }

    /// `count` workspaces of the numbers look, the `active` one shown.
    fn live(count: usize, active: usize, view: Option<usize>) -> Live {
        Live {
            workspaces: (1..=count)
                .map(|n| (n.to_string(), n == active + 1))
                .collect(),
            view,
            workspaces_shown: DEFAULT_SHOWN,
            workspaces_look: Look::Numbers,
            workspaces_ends: Ends::Arrows,
            ..Live::default()
        }
    }

    /// The same workspaces in the button look.
    fn button_live(count: usize, active: usize) -> Live {
        Live {
            workspaces_look: Look::Button,
            ..live(count, active, None)
        }
    }

    /// The first button shown and the shown workspace's place.
    fn at(live: &Live) -> (usize, usize) {
        let text = shows(live);
        let view = read(&text);
        (view.first, view.active)
    }

    #[test]
    fn three_show_at_once_round_the_shown_workspace() {
        assert_eq!(at(&live(4, 0, None)), (0, 0));
        assert_eq!(at(&live(4, 2, None)), (1, 2));
        // The last: still three in sight.
        assert_eq!(at(&live(4, 3, None)), (1, 3));
        assert_eq!(at(&live(9, 6, None)), (5, 6));
        assert_eq!(at(&live(2, 1, None)), (0, 1));
        assert_eq!(shows(&live(0, 0, None)), "");
        // A scroll's view holds until the shown workspace changes.
        assert_eq!(at(&live(9, 0, Some(4))), (4, 0));
        assert_eq!(at(&live(9, 0, Some(8))), (6, 0));
    }

    #[test]
    fn what_it_shows_reads_back_whatever_the_names_hold() {
        let mut live = live(4, 1, None);
        live.workspaces = vec![
            ("a,b*c;d".into(), false),
            ("e\u{1f}f\u{1e}g".into(), true),
            (String::new(), false),
            ("\u{7}".into(), false),
        ];
        let text = shows(&live);
        let view = read(&text);
        assert_eq!(view.names, ["a,b*c;d", "efg", "3", "4"]);
        assert_eq!((view.active, view.first, view.shown), (1, 0, 3));
        assert_eq!((view.look, view.ends), (Look::Numbers, Ends::Arrows));
        assert!(read("").names.is_empty());
    }

    #[test]
    fn the_shown_one_is_a_wider_pill_and_the_width_stays() {
        let text = shows(&live(4, 0, None));
        let view = read(&text);
        let first = buttons(&view, BUTTON);
        let places: Vec<(usize, bool)> = first.iter().map(|b| (b.place, b.on)).collect();
        assert_eq!(places, [(0, true), (1, false), (2, false)]);
        // Room, then the peek's place on the left, as more lie to the right.
        assert_eq!(
            (first[0].left, first[0].width),
            (ROOM + PEEK + GAP, BUTTON + PILL)
        );
        assert_eq!(first[1].left, ROOM + PEEK + GAP + (BUTTON + PILL) + GAP);
        assert_eq!(logical_width(&view, BUTTON), 132.0);
        let other = shows(&live(4, 2, None));
        assert_eq!(
            logical_width(&view, BUTTON),
            logical_width(&read(&other), BUTTON),
            "the widget keeps its width as the shown workspace changes"
        );
        assert_eq!(logical_width(&read(""), BUTTON), 0.0);
        let two = shows(&live(2, 1, None));
        let three = shows(&live(3, 1, None));
        assert!(logical_width(&read(&two), BUTTON) < logical_width(&read(&three), BUTTON));
    }

    #[test]
    fn numbers_keep_today_s_sizes_and_a_name_widens_its_button() {
        let names = ["1", "2", "3", "4"];
        assert_eq!(slot(&names, &mut seven), BUTTON, "20 px, as today");
        assert_eq!(slot(&["Mail", "2"], &mut seven), 40.0, "the label and 12");
        assert_eq!(slot(&[], &mut seven), BUTTON);
        let mut live = live(4, 0, None);
        live.workspaces[0].0 = "Mail".into();
        let text = shows(&live);
        let view = read(&text);
        let slot = slot(&view.names, &mut seven);
        assert_eq!(slot, 40.0);
        let first = buttons(&view, slot);
        assert_eq!((first[0].width, first[1].width), (40.0 + PILL, 40.0));
    }

    #[test]
    fn a_long_name_is_cut_to_nine_and_an_ellipsis() {
        assert_eq!(cut("Marketing team"), "Marketing\u{2026}");
        assert_eq!(cut("Abcdefghij"), "Abcdefghij", "ten characters stay");
        assert_eq!(cut("Abcdefghijk"), "Abcdefghi\u{2026}");
        assert_eq!(cut("2"), "2");
    }

    #[test]
    fn a_shown_count_of_five_shows_five() {
        let mut live = live(9, 4, None);
        live.workspaces_shown = 5;
        let text = shows(&live);
        let view = read(&text);
        assert_eq!(view.first, 3);
        let places: Vec<usize> = buttons(&view, BUTTON).iter().map(|b| b.place).collect();
        assert_eq!(places, [3, 4, 5, 6, 7]);
        assert_eq!(
            logical_width(&view, BUTTON),
            2.0 * ROOM + 2.0 * (PEEK + GAP) + 32.0 + 4.0 * (BUTTON + GAP)
        );
    }

    #[test]
    fn the_next_workspace_peeks_in_where_more_lie_and_a_click_scrolls_to_it() {
        // At the start, only 4 peeks in, at the right end.
        let start = shows(&live(4, 0, None));
        let view = read(&start);
        let width = logical_width(&view, BUTTON);
        let peek = peeks(&view, BUTTON);
        assert_eq!(peek.len(), 1);
        assert_eq!(
            (peek[0].place, peek[0].label.as_str(), peek[0].on_left),
            (3, "4", false)
        );
        assert_eq!(peek[0].left, width - ROOM - PEEK);
        assert!(peeks(&read(&shows(&live(3, 0, None))), BUTTON).is_empty());
        // In the middle of nine, one at each side.
        let middle = shows(&live(9, 4, None));
        let view = read(&middle);
        let sides: Vec<(usize, bool)> = peeks(&view, BUTTON)
            .iter()
            .map(|p| (p.place, p.on_left))
            .collect();
        assert_eq!(sides, [(2, true), (6, false)]);
        assert_eq!(
            click_numbers(&view, BUTTON, ROOM + 1.0),
            Some(Action::View(2))
        );
        let width = logical_width(&view, BUTTON);
        assert_eq!(
            click_numbers(&view, BUTTON, width - ROOM - 1.0),
            Some(Action::View(4))
        );
    }

    #[test]
    fn counts_says_how_many_lie_beyond_each_side() {
        assert_eq!(beyond_text(2), "+2");
        let mut counts = live(9, 0, Some(1));
        counts.workspaces_ends = Ends::Counts;
        let text = shows(&counts);
        let view = read(&text);
        let beyond: Vec<usize> = peeks(&view, BUTTON).iter().map(|p| p.beyond).collect();
        assert_eq!(beyond, [1, 5]);
        // Its strip is wider, so the widget is too.
        let arrows = shows(&live(9, 0, Some(1)));
        assert_eq!(
            logical_width(&view, BUTTON) - logical_width(&read(&arrows), BUTTON),
            2.0 * (COUNTS - PEEK)
        );
    }

    #[test]
    fn a_click_on_a_named_button_gives_its_place() {
        let mut live = live(4, 0, None);
        live.workspaces = vec![
            ("Mail".into(), true),
            ("2".into(), false),
            ("Code".into(), false),
            ("4".into(), false),
        ];
        let text = shows(&live);
        let view = read(&text);
        let slot = slot(&view.names, &mut seven);
        let found = buttons(&view, slot);
        let middle = |b: &Button| b.left + b.width / 2.0;
        assert_eq!(
            click_numbers(&view, slot, middle(&found[1])),
            Some(Action::Show(1))
        );
        assert_eq!(
            click_numbers(&view, slot, middle(&found[2])),
            Some(Action::Show(2))
        );
        assert_eq!(
            click_numbers(&view, slot, 0.0),
            None,
            "the room is no button"
        );
    }

    #[test]
    fn a_scroll_moves_the_view_or_the_shown_workspace_and_stops_at_the_ends() {
        let text = shows(&live(4, 0, None));
        let view = read(&text);
        assert_eq!(scroll(&view, 1), Some(Action::View(1)));
        assert_eq!(
            scroll(&view, 5),
            Some(Action::View(1)),
            "only as far as the last three"
        );
        assert_eq!(scroll(&view, -1), None, "already at the first");
        assert_eq!(scroll(&read(&shows(&live(2, 0, None))), 1), None);
        let button = shows(&button_live(4, 1));
        let view = read(&button);
        assert_eq!(scroll(&view, 1), Some(Action::Show(2)));
        assert_eq!(scroll(&view, -3), Some(Action::Show(0)), "the first");
        assert_eq!(scroll(&view, 9), Some(Action::Show(3)), "the last");
    }

    #[test]
    fn the_button_look_splits_the_word_and_a_gap_goes_to_the_nearer_segment() {
        let segs = segments(4, 60.0);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0], (ROOM, 13.5));
        assert_eq!(segs[1], (ROOM + 13.5 + SEGMENT_GAP, 13.5));
        let last = segs[3];
        assert_eq!(last.0 + last.1, ROOM + 60.0, "the segments span the word");
        assert_eq!(nearest(&segs, 20.0), Some(0));
        assert_eq!(
            nearest(&segs, 20.8),
            Some(1),
            "the gap belongs to the nearer"
        );
        assert_eq!(nearest(&segs, -5.0), Some(0));
        assert_eq!(nearest(&segs, 500.0), Some(3));
        assert_eq!(word_button_width(60.0), 72.0);
        assert!(segments(0, 60.0).is_empty());
        // A click on the second segment shows the second workspace.
        let text = shows(&button_live(4, 0));
        let view = read(&text);
        assert_eq!(click_button(&view, 60.0, 30.0), Some(Action::Show(1)));
        assert_eq!(
            segments_line(&shows(&button_live(4, 0)), 60.0).as_deref(),
            Some("6.0+13.5,21.5+13.5,37.0+13.5,52.5+13.5")
        );
        assert_eq!(segments_line(&shows(&live(4, 0, None)), 60.0), None);
    }

    #[test]
    fn a_screen_reader_hears_the_shown_workspace() {
        let mut live = live(4, 1, None);
        assert_eq!(label(&shows(&live)), "Workspaces: 2 shown, 2 of 4");
        live.workspaces[0].0 = "Mail".into();
        live.workspaces[0].1 = true;
        live.workspaces[1].1 = false;
        assert_eq!(label(&shows(&live)), "Workspaces: Mail shown, 1 of 4");
        assert_eq!(label(""), "Workspaces");
    }

    #[test]
    fn the_settings_default_when_missing_or_unknown_and_the_person_wins() {
        assert_eq!(
            Settings::read(None, None),
            Settings {
                look: Look::Numbers,
                shown: 3,
                ends: Ends::Arrows,
            }
        );
        let person = "format = 1\n[layout]\nworkspaces_look = \"button\"\nworkspaces_shown = 5\nworkspaces_ends = \"counts\"\n";
        assert_eq!(
            Settings::read(None, Some(person)),
            Settings {
                look: Look::Button,
                shown: 5,
                ends: Ends::Counts,
            }
        );
        let machine = "format = 1\n[layout]\nworkspaces_shown = 7\nworkspaces_ends = \"fade\"\n";
        let person = "format = 1\n[layout]\nworkspaces_shown = 4\n";
        assert_eq!(
            Settings::read(Some(machine), Some(person)),
            Settings {
                look: Look::Numbers,
                shown: 4,
                ends: Ends::Fade,
            }
        );
        let unknown = "format = 1\n[layout]\nworkspaces_look = \"dots\"\nworkspaces_shown = 12\nworkspaces_ends = 2\n";
        assert_eq!(
            Settings::read(None, Some(unknown)),
            Settings {
                look: Look::Numbers,
                shown: 3,
                ends: Ends::Arrows,
            }
        );
    }

    #[test]
    fn a_peek_fades_out_towards_the_outer_side() {
        let mut strip = Pixmap::new(10, 2).unwrap();
        strip.fill(tiny_skia::Color::WHITE);
        fade(&mut strip, false);
        let alpha = |p: &Pixmap, x: u32| p.pixel(x, 0).unwrap().alpha();
        assert!(alpha(&strip, 0) > 230 && alpha(&strip, 9) < 20);
        let mut strip = Pixmap::new(10, 2).unwrap();
        strip.fill(tiny_skia::Color::WHITE);
        fade(&mut strip, true);
        assert!(alpha(&strip, 0) < 20 && alpha(&strip, 9) > 230);
    }

    #[test]
    fn a_press_finds_the_button_under_it_and_none_over_the_room_or_a_gap() {
        // Four workspaces, the second shown first: the buttons 2 at 25 to 45,
        // the pill 3 at 50 to 82, 4 at 87 to 107 (BUTTON is the slot).
        let text = shows(&live(4, 2, None));
        assert_eq!(button_at(&text, BUTTON, 35.0), Some(1));
        assert_eq!(button_at(&text, BUTTON, 66.0), Some(2), "the pill");
        assert_eq!(button_at(&text, BUTTON, 97.0), Some(3));
        assert_eq!(button_at(&text, BUTTON, 10.0), None, "the peek");
        assert_eq!(button_at(&text, BUTTON, 47.0), None, "the gap");
        assert_eq!(button_at(&text, BUTTON, 300.0), None);
        assert_eq!(button_at(&shows(&button_live(4, 2)), BUTTON, 66.0), None);
        assert_eq!(button_at("", BUTTON, 66.0), None);
    }

    #[test]
    fn a_drop_lands_on_the_button_under_it_or_the_nearer_end() {
        let text = shows(&live(4, 2, None));
        assert_eq!(landing(&text, BUTTON, 35.0), Some(1));
        assert_eq!(landing(&text, BUTTON, 97.0), Some(3));
        assert_eq!(landing(&text, BUTTON, 0.0), Some(1), "left of the first");
        assert_eq!(landing(&text, BUTTON, 500.0), Some(3), "right of the last");
        assert_eq!(landing(&text, BUTTON, 47.0), Some(1), "the nearer in a gap");
        assert_eq!(landing(&text, BUTTON, 48.0), Some(2), "the nearer in a gap");
        assert_eq!(landing(&shows(&button_live(4, 2)), BUTTON, 35.0), None);
    }

    #[test]
    fn a_moved_name_takes_its_place_and_the_empty_ends_go() {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            reordered_names(&names(&["Mail"]), 4, 0, 2),
            names(&["", "", "Mail"])
        );
        assert!(reordered_names(&[], 4, 2, 0).is_empty());
        assert_eq!(
            reordered_names(&names(&["A", "B"]), 3, 1, 0),
            names(&["B", "A"])
        );
        assert_eq!(
            reordered_names(&names(&["A", "B", "C"]), 3, 0, 2),
            names(&["B", "C", "A"]),
            "the ones between shift by one"
        );
        assert_eq!(
            reordered_names(&names(&["A"]), 2, 0, 9),
            names(&["A"]),
            "a place past the list changes nothing"
        );
    }

    #[test]
    fn each_look_draws_and_writes_pngs() {
        let mut text = crate::paint::Text::load(&edel::tokens::Tokens::built_in().font);
        if text.line("A", 13.0).width <= 0.0 {
            return; // no fonts on this machine
        }
        let tokens = edel::tokens::Tokens::built_in();
        let names = |count: usize, active: usize, named: &str| -> Vec<(String, bool)> {
            (0..count)
                .map(|i| {
                    let label = if i == 0 && !named.is_empty() {
                        named.to_string()
                    } else {
                        (i + 1).to_string()
                    };
                    (label, i == active)
                })
                .collect()
        };
        for (name, look, ends, workspaces) in [
            ("numbers", Look::Numbers, Ends::Arrows, names(4, 0, "Mail")),
            ("fade", Look::Numbers, Ends::Fade, names(9, 4, "")),
            ("counts", Look::Numbers, Ends::Counts, names(9, 4, "")),
            ("button", Look::Button, Ends::Arrows, names(4, 1, "")),
        ] {
            let live = Live {
                workspaces,
                workspaces_look: look,
                workspaces_shown: DEFAULT_SHOWN,
                workspaces_ends: ends,
                ..Live::default()
            };
            let shown = shows(&live);
            let mut pixmap = Pixmap::new(400, 80).unwrap();
            let p = tokens.panel;
            pixmap.fill(tiny_skia::Color::from_rgba(p.r, p.g, p.b, 1.0).unwrap());
            let mut canvas = Canvas {
                pixmap: &mut pixmap,
                tokens: &tokens,
                text: Some(&mut text),
                icons: None,
                scale: 2.0,
                top: 0.0,
                height: 80.0,
                dock: false,
                along_top: false,
            };
            let w = width(&mut canvas, &shown);
            assert!(w > 0.0 && w < 400.0, "{name}: {w}");
            draw(&mut canvas, &shown, 0.0);
            if let Some(dir) = std::env::var_os("EDEL_SWITCHER_PNG") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                pixmap
                    .save_png(dir.join(format!("switcher-{name}.png")))
                    .unwrap();
            }
        }
    }
}
