//! The clipboard history's list (M7.13): what the card shows, where each
//! part lies and how it is drawn, plain data and drawing, tested without a
//! display. The card is `popup`'s card, its rows and its lit row as the
//! other menus' (`app_windows.rs`, `in_use_menu.rs`), and `banner::width_for`
//! gives its width, so a Compact screen gets touch sizes. `layout` is the
//! one function that places everything, `places` is the log line CI reads
//! and `nodes` is what a screen reader reads. `clipboard_card.rs` holds
//! its surface and the keyboard.

use accesskit::{Rect as AkRect, Role};
use tiny_skia::Pixmap;

use edel::i18n::tr;
use edel::tokens::Tokens;

use crate::a11y::Item;
use crate::banner;
use crate::clipboard::{History, Item as Copied};
use crate::paint::{self, Face, Text, fill, lit};
use crate::popup::{self, INSET, PAD, Rect, dim, veil};

/// A picture's row, logical pixels high.
pub const PICTURE_ROW: f32 = 64.0;
/// The heading row: "Clipboard" and, with items, "Clear all" at its end.
pub const HEADING: f32 = 44.0;
/// A text row's height on a Compact screen, and the remove button's side
/// there: touch-sized targets.
pub const TOUCH: f32 = 44.0;
/// Between a line's text or a thumbnail and the remove button, logical
/// pixels, and the remove button's inset from the row's end.
const GAP: f32 = 4.0;
/// The room a thumbnail keeps above and below it in its row.
const PICTURE_PAD: f32 = 4.0;

/// What the list shows: its lines (newest first, from `History`), the row
/// the arrows or the pointer are on, whether keeping is on, the screen's
/// width in logical pixels and the part the pointer is over.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub lines: Vec<Line>,
    pub lit: usize,
    pub on: bool,
    pub screen_width: u32,
    pub hover: Option<Part>,
}

/// One item as the list shows it: a text's line, or a picture's thumbnail.
#[derive(Clone, Debug)]
pub enum Line {
    Text(String),
    Picture(std::sync::Arc<Pixmap>),
}

impl PartialEq for Line {
    /// Texts by their words, pictures by their thumbnail's own picture.
    fn eq(&self, other: &Line) -> bool {
        match (self, other) {
            (Line::Text(a), Line::Text(b)) => a == b,
            (Line::Picture(a), Line::Picture(b)) => std::sync::Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// The parts of the list a point or the keyboard can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The item at this index, newest first: its paste.
    Row(usize),
    /// Takes the item at this index out of the list.
    Remove(usize),
    /// Empties the list.
    Clear,
}

/// Where everything lies, logical pixels from the card's top left corner.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub card: (u32, u32),
    pub heading: Rect,
    pub clear: Option<Rect>,
    pub rows: Vec<Rect>,
    pub removes: Vec<Rect>,
    pub note: Option<Rect>,
}

/// The list's view of `history`, its lit row `lit` on a screen `screen_width`
/// logical pixels wide (0 when not yet known).
pub fn view(history: &History, lit: usize, screen_width: u32) -> View {
    let lines = history
        .items()
        .map(|item| match item {
            Copied::Text(_) => Line::Text(item.line().unwrap_or_default()),
            Copied::Picture { thumb, .. } => Line::Picture(thumb.clone()),
        })
        .collect();
    View {
        lines,
        lit,
        on: history.on(),
        screen_width,
        hover: None,
    }
}

/// A text row's height: the tokens' row, at least `TOUCH` on a Compact screen.
fn text_row(tokens: &Tokens, compact: bool) -> f32 {
    let row = tokens.row as f32;
    if compact { row.max(TOUCH) } else { row }
}

/// The room a row's text or thumbnail has left of its remove button, from
/// the card's text inset to `remove`'s left edge less the gap.
fn room_before(remove: Rect) -> f32 {
    remove.x - (PAD + INSET) - GAP
}

/// A thumbnail `tw` by `th` pixels fitted into a box `box_w` by `box_h`
/// logical pixels, never scaled up: its size in logical pixels.
fn fitted(tw: f32, th: f32, box_w: f32, box_h: f32) -> (f32, f32) {
    let k = (box_h / th).min(box_w / tw).clamp(0.0, 1.0);
    (tw * k, th * k)
}

/// Where everything of `view` lies. `measure` gives a text's width at a size
/// in logical pixels, for "Clear all" (as `in_use_menu::layout` takes it).
pub fn layout(view: &View, tokens: &Tokens, measure: &mut dyn FnMut(&str, f32) -> f32) -> Layout {
    let (width, compact) = banner::width_for(view.screen_width);
    let w = width as f32;
    let text_h = text_row(tokens, compact);
    let small = tokens.panel_text_small_size as f32;
    let heading = Rect::new(PAD, PAD, w - 2.0 * PAD, HEADING);
    let clear = (!view.lines.is_empty()).then(|| {
        let cw = measure(tr("Clear all"), small) + 2.0 * INSET;
        let ch = if compact {
            TOUCH
        } else {
            tokens.panel_control as f32
        };
        Rect::new(
            heading.right() - INSET - cw,
            heading.y + (HEADING - ch) / 2.0,
            cw,
            ch,
        )
    });
    let mut rows = Vec::new();
    let mut removes = Vec::new();
    let mut y = PAD + HEADING;
    for line in &view.lines {
        let h = match line {
            Line::Text(_) => text_h,
            Line::Picture(_) => PICTURE_ROW,
        };
        let row = Rect::new(PAD, y, w - 2.0 * PAD, h);
        removes.push(Rect::new(
            row.right() - GAP - text_h,
            y + (h - text_h) / 2.0,
            text_h,
            text_h,
        ));
        rows.push(row);
        y += h;
    }
    let note = view.lines.is_empty().then(|| {
        let h = if view.on { text_h } else { 2.0 * text_h };
        Rect::new(PAD, PAD + HEADING, w - 2.0 * PAD, h)
    });
    let bottom = note
        .map(|n| n.y + n.h)
        .or_else(|| rows.last().map(|r| r.y + r.h))
        .unwrap_or(PAD + HEADING);
    let height = (bottom + PAD).ceil();
    Layout {
        card: (width, height as u32),
        heading,
        clear,
        rows,
        removes,
        note,
    }
}

/// The part under the point (`x`, `y`, card-local logical pixels): Clear
/// all, a row's remove button, then the row itself.
pub fn hit(layout: &Layout, x: f32, y: f32) -> Option<Part> {
    if layout.clear.is_some_and(|c| c.contains(x, y)) {
        return Some(Part::Clear);
    }
    if let Some(i) = layout.removes.iter().position(|r| r.contains(x, y)) {
        return Some(Part::Remove(i));
    }
    layout
        .rows
        .iter()
        .position(|r| r.contains(x, y))
        .map(Part::Row)
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`: the card,
/// the heading, the lit row's light, each text cut where it is too long and
/// each picture's thumbnail, the remove buttons on the lit or hovered row,
/// and the note when the list is empty or off. `text` is `None` in tests
/// without fonts.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    layout: &Layout,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    scale: f32,
) {
    let s = scale;
    popup::card(pixmap, tokens, s);
    let hovered = |part: Part| view.hover == Some(part);
    let title = tokens.panel_text_size as f32;
    let small = tokens.panel_text_small_size as f32;
    let heading = layout.heading;

    if let Some(text) = text.as_deref_mut() {
        let room = match layout.clear {
            Some(clear) => clear.x - (PAD + INSET) - GAP,
            None => heading.w - 2.0 * INSET,
        };
        let mut line = text.fit_in(tr("Clipboard"), title * s, room * s, Face::SEMIBOLD);
        let y = popup::middle(heading.y, heading.h, title * s, s);
        text.draw(pixmap, &mut line, (PAD + INSET) * s, y, tokens.panel_text);
    }
    if let Some(clear) = layout.clear {
        let (x, y, w, h) = clear.device(s);
        fill(pixmap, x, y, w, h, h / 2.0, veil(tokens, 0.05));
        if hovered(Part::Clear) {
            fill(pixmap, x, y, w, h, h / 2.0, veil(tokens, 0.05));
        }
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.line_in(tr("Clear all"), small * s, Face::SEMIBOLD);
            let at = x + (w - line.width) / 2.0;
            let ty = popup::middle(clear.y, clear.h, small * s, s);
            text.draw(pixmap, &mut line, at, ty, tokens.accent);
        }
    }

    for (i, (line, row)) in view.lines.iter().zip(&layout.rows).enumerate() {
        if i == view.lit {
            let (x, y, w, h) = row.device(s);
            fill(
                pixmap,
                x,
                y,
                w,
                h,
                tokens.radius_control as f32 * s,
                lit(tokens),
            );
        }
        let room = room_before(layout.removes[i]);
        match line {
            Line::Text(words) => {
                if let Some(text) = text.as_deref_mut() {
                    let mut drawn = text.fit(words, title * s, room * s);
                    let y = popup::middle(row.y, row.h, title * s, s);
                    text.draw(pixmap, &mut drawn, (PAD + INSET) * s, y, tokens.panel_text);
                }
            }
            Line::Picture(thumb) => {
                let box_h = row.h - 2.0 * PICTURE_PAD;
                let (tw, th) = (thumb.width() as f32, thumb.height() as f32);
                let (w, h) = fitted(tw, th, room, box_h);
                let x = ((PAD + INSET) * s).round();
                let y = ((row.y + (row.h - h) / 2.0) * s).round();
                let device = (x, y, (w * s).round(), (h * s).round());
                let radius = tokens.radius_small as f32 * s;
                paint::rounded_picture(pixmap, thumb, device, radius);
            }
        }
    }

    for (i, remove) in layout.removes.iter().enumerate() {
        let shown = view.lit == i || hovered(Part::Row(i)) || hovered(Part::Remove(i));
        if !shown {
            continue;
        }
        if hovered(Part::Remove(i)) {
            let (x, y, w, h) = remove.device(s);
            fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.08));
        }
        popup::icon_in(pixmap, "close", 10.0, *remove, s, dim(tokens));
    }

    if let (Some(note), Some(text)) = (layout.note, text) {
        let text_h = if view.on { note.h } else { note.h / 2.0 };
        let (words, face_size) = if view.on {
            (tr("Nothing copied yet"), title)
        } else {
            (tr("Clipboard history is off"), title)
        };
        let mut first = text.line(words, face_size * s);
        let at = note.x + (note.w - first.width / s) / 2.0;
        let y = popup::middle(note.y, text_h, face_size * s, s);
        text.draw(pixmap, &mut first, (at * s).round(), y, dim(tokens));
        if !view.on {
            let mut second = text.line(tr("Turn it on in Settings, Shortcuts"), small * s);
            let at = note.x + (note.w - second.width / s) / 2.0;
            let y = popup::middle(note.y + text_h, text_h, small * s, s);
            text.draw(pixmap, &mut second, (at * s).round(), y, dim(tokens));
        }
    }
}

/// What a screen reader reads (M6.9, as `in_use_menu.rs`): a list named
/// "Clipboard history", then flat in order each item as a list item named by
/// its text ("Picture" for a picture) with its "Remove" button after it,
/// then "Clear all" as a button, and the note's lines as labels. The
/// focus is the lit row's list item.
pub fn nodes(view: &View, layout: &Layout) -> (Vec<Item>, Option<usize>) {
    let mut items = vec![Item {
        role: Role::List,
        label: tr("Clipboard history").to_string(),
        bounds: ak(Rect::new(
            0.0,
            0.0,
            layout.card.0 as f32,
            layout.card.1 as f32,
        )),
        children: vec![],
        toggled: None,
        value: None,
        value_text: None,
        disabled: false,
    }];
    let mut focus = None;
    for (i, (line, row)) in view.lines.iter().zip(&layout.rows).enumerate() {
        let label = match line {
            Line::Text(words) => words.clone(),
            Line::Picture(_) => tr("Picture").to_string(),
        };
        if i == view.lit {
            focus = Some(items.len());
        }
        items.push(plain(Role::ListItem, label, ak(*row)));
        items.push(plain(
            Role::Button,
            tr("Remove").to_string(),
            ak(layout.removes[i]),
        ));
    }
    if let Some(clear) = layout.clear {
        items.push(plain(Role::Button, tr("Clear all").to_string(), ak(clear)));
    }
    if let Some(note) = layout.note {
        let lines = if view.on {
            vec![tr("Nothing copied yet")]
        } else {
            vec![
                tr("Clipboard history is off"),
                tr("Turn it on in Settings, Shortcuts"),
            ]
        };
        for words in lines {
            items.push(plain(Role::Label, words.to_string(), ak(note)));
        }
    }
    (items, focus)
}

/// An item with no state, its bounds `bounds`.
fn plain(role: Role, label: String, bounds: AkRect) -> Item {
    Item {
        role,
        label,
        bounds,
        children: vec![],
        toggled: None,
        value: None,
        value_text: None,
        disabled: false,
    }
}

/// A `Rect` as the accessibility tree's, in the same logical pixels.
fn ak(r: Rect) -> AkRect {
    AkRect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x + r.w),
        f64::from(r.y + r.h),
    )
}

/// `X+Y+WxH` of a rectangle, whole logical pixels.
fn spot(r: Rect) -> String {
    format!(
        "{}+{}+{}x{}",
        r.x.round() as i32,
        r.y.round() as i32,
        r.w.round() as i32,
        r.h.round() as i32
    )
}

/// The log line CI reads: the card's size, then Clear all's place when it
/// shows, then each row's place, in logical pixels from the card's top left
/// corner, as `card WxH, clear X+Y+WxH, row0 X+Y+WxH, row1 ...`.
pub fn places(layout: &Layout) -> String {
    let (w, h) = layout.card;
    let mut parts = vec![format!("card {w}x{h}")];
    if let Some(clear) = layout.clear {
        parts.push(format!("clear {}", spot(clear)));
    }
    parts.extend(
        layout
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| format!("row{i} {}", spot(*row))),
    );
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A stand-in for the font: each letter a little more than half an em.
    fn measure(text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.55
    }

    fn text_view(words: &[&str], lit: usize, screen: u32) -> View {
        View {
            lines: words.iter().map(|w| Line::Text(w.to_string())).collect(),
            lit,
            on: true,
            screen_width: screen,
            hover: None,
        }
    }

    fn picture_line() -> Line {
        Line::Picture(Arc::new(Pixmap::new(48, 24).unwrap()))
    }

    fn lay(view: &View) -> Layout {
        layout(view, &Tokens::built_in(), &mut measure)
    }

    #[test]
    fn an_empty_list_shows_its_note_and_no_rows() {
        let tokens = Tokens::built_in();
        let l = lay(&View {
            lines: vec![],
            lit: 0,
            on: true,
            screen_width: 1280,
            hover: None,
        });
        assert!(l.rows.is_empty() && l.clear.is_none());
        let note = l.note.expect("a note");
        assert_eq!(note.h, tokens.row as f32, "one line");
        assert_eq!(l.card.0, banner::WIDTH);
        assert_eq!(l.card.1 as f32, (PAD + HEADING + note.h + PAD).ceil());
    }

    #[test]
    fn a_turned_off_list_has_a_note_of_two_lines() {
        let tokens = Tokens::built_in();
        let mut v = text_view(&[], 0, 1280);
        v.on = false;
        let l = lay(&v);
        assert_eq!(l.note.unwrap().h, 2.0 * tokens.row as f32);
    }

    #[test]
    fn each_row_adds_its_height_and_a_picture_is_taller() {
        let tokens = Tokens::built_in();
        let row = tokens.row as f32;
        let one = lay(&text_view(&["Hello"], 0, 1280));
        let text_bottom = PAD + HEADING + row;
        assert_eq!(one.rows[0].h, row);
        assert_eq!(one.card.1 as f32, (text_bottom + PAD).ceil());
        assert!(one.clear.is_some(), "Clear all with items");
        let picture = View {
            lines: vec![picture_line()],
            lit: 0,
            on: true,
            screen_width: 1280,
            hover: None,
        };
        let pic = lay(&picture);
        assert_eq!(pic.rows[0].h, PICTURE_ROW);
        assert!(pic.card.1 > one.card.1);
    }

    #[test]
    fn ten_rows_stack_without_gaps() {
        let words: Vec<String> = (0..10).map(|i| format!("item {i}")).collect();
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let l = lay(&text_view(&refs, 3, 1280));
        assert_eq!(l.rows.len(), 10);
        for pair in l.rows.windows(2) {
            assert_eq!(pair[0].y + pair[0].h, pair[1].y);
        }
        assert_eq!(l.removes.len(), 10);
    }

    #[test]
    fn a_compact_screen_gives_touch_rows_and_a_card_as_wide_as_the_screen() {
        let tokens = Tokens::built_in();
        let l = lay(&text_view(&["Hello"], 0, 360));
        assert_eq!(l.card.0, 360, "as wide as the screen below 600");
        assert_eq!(l.rows[0].h, TOUCH.max(tokens.row as f32));
        assert_eq!(l.removes[0].w, TOUCH.max(tokens.row as f32));
        let desktop = lay(&text_view(&["Hello"], 0, 1280));
        assert_eq!(desktop.card.0, banner::WIDTH);
    }

    #[test]
    fn a_click_finds_a_row_its_remove_button_and_clear_all() {
        let l = lay(&text_view(&["one", "two"], 0, 1280));
        let row1 = l.rows[1];
        assert_eq!(hit(&l, row1.x + 2.0, row1.y + 2.0), Some(Part::Row(1)));
        let remove = l.removes[0];
        let (cx, cy) = (remove.x + remove.w / 2.0, remove.y + remove.h / 2.0);
        assert_eq!(hit(&l, cx, cy), Some(Part::Remove(0)));
        let clear = l.clear.unwrap();
        assert_eq!(
            hit(&l, clear.x + clear.w / 2.0, clear.y + clear.h / 2.0),
            Some(Part::Clear)
        );
        assert_eq!(hit(&l, 0.5, 0.5), None);
        let empty = lay(&text_view(&[], 0, 1280));
        assert_eq!(
            hit(&empty, PAD + 2.0, PAD + HEADING + 2.0),
            None,
            "the note is no part"
        );
    }

    #[test]
    fn places_names_the_card_clear_all_and_each_row() {
        let l = lay(&text_view(&["one", "two"], 0, 1280));
        let text = places(&l);
        let (w, h) = l.card;
        assert!(text.starts_with(&format!("card {w}x{h}, clear ")), "{text}");
        assert!(text.contains(", row0 "), "{text}");
        assert!(text.contains(", row1 "), "{text}");
        assert!(!text.contains("row2"), "{text}");
        let empty = places(&lay(&text_view(&[], 0, 1280)));
        assert!(!empty.contains("clear"), "{empty}");
    }

    #[test]
    fn nodes_list_the_items_then_clear_all_and_focus_the_lit_row() {
        let v = text_view(&["one", "two"], 1, 1280);
        let l = lay(&v);
        let (items, focus) = nodes(&v, &l);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Clipboard history",
                "one",
                "Remove",
                "two",
                "Remove",
                "Clear all",
            ]
        );
        assert_eq!(focus, Some(3), "the lit row's list item");
        assert_eq!(items[0].role, Role::List);
        assert_eq!(items[1].role, Role::ListItem);
        assert_eq!(items[2].role, Role::Button);
        let pic = View {
            lines: vec![picture_line()],
            lit: 0,
            on: true,
            screen_width: 1280,
            hover: None,
        };
        let (items, focus) = nodes(&pic, &lay(&pic));
        assert_eq!(items[1].label, "Picture");
        assert_eq!(focus, Some(1));
    }

    #[test]
    fn nodes_of_an_off_list_read_its_note() {
        let mut v = text_view(&[], 0, 1280);
        v.on = false;
        let (items, focus) = nodes(&v, &lay(&v));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Clipboard history",
                "Clipboard history is off",
                "Turn it on in Settings, Shortcuts",
            ]
        );
        assert_eq!(focus, None);
    }

    #[test]
    fn the_view_takes_the_history_newest_first() {
        let mut history = History::new(true);
        history.add(Copied::text("first".into()).unwrap());
        history.add(Copied::text("second".into()).unwrap());
        let v = view(&history, 0, 1280);
        assert_eq!(
            v.lines,
            vec![Line::Text("second".into()), Line::Text("first".into())]
        );
        assert!(v.on);
        let mut off = History::new(false);
        off.add(Copied::text("x".into()).unwrap());
        assert!(view(&off, 0, 1280).lines.is_empty());
    }

    #[test]
    fn a_picture_line_is_equal_only_to_its_own_thumbnail() {
        let a = picture_line();
        let b = picture_line();
        assert_ne!(a, b, "two thumbnails are two pictures");
        assert_eq!(a, a.clone());
        assert_ne!(Line::Text("x".into()), a);
    }

    #[test]
    fn a_thumbnail_is_fitted_into_its_box_and_never_grown() {
        assert_eq!(fitted(96.0, 48.0, 200.0, 56.0), (96.0, 48.0));
        assert_eq!(fitted(96.0, 48.0, 50.0, 56.0), (50.0, 25.0));
        assert_eq!(fitted(20.0, 10.0, 200.0, 56.0), (20.0, 10.0), "not up");
    }

    /// Fonts for the drawing tests, `None` on a machine with none.
    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    /// Draws the list with three texts and a picture, light and dark, and
    /// with `EDEL_CLIPBOARD_PNG=DIR` writes them as PNGs over the screen's
    /// colour with the shadow's room, as the other cards' previews.
    #[test]
    fn the_list_draws_with_fonts_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        let mut history = History::new(true);
        history.add(Copied::picture(sample_png()).unwrap());
        history.add(Copied::text("a second line of text, a bit long for one row".into()).unwrap());
        history.add(Copied::text("first copied".into()).unwrap());
        for (scheme, mode) in [
            (edel::tokens::Scheme::Light, "light"),
            (edel::tokens::Scheme::Dark, "dark"),
        ] {
            let tokens = Tokens::built_in_scheme(scheme);
            let v = View {
                hover: Some(Part::Remove(1)),
                ..view(&history, 0, 1280)
            };
            let l = layout(&v, &tokens, &mut |t, size| text.line(t, size).width);
            let (w, h) = l.card;
            let s = 2.0;
            let mut pixmap = Pixmap::new(w * 2, h * 2).unwrap();
            paint(&mut pixmap, &v, &l, &tokens, Some(&mut text), s);
            let at = |x: f32, y: f32| {
                let c = pixmap.pixel(x as u32, y as u32).unwrap().demultiply();
                [c.red(), c.green(), c.blue()]
            };
            let row0 = l.rows[0];
            assert_ne!(
                at((row0.x + row0.w / 2.0) * s, (row0.y + row0.h / 2.0) * s),
                tokens.panel.bytes()[..3],
                "{mode}: the lit row is lit"
            );
            preview(&pixmap, &tokens, &format!("clipboard-{mode}"));
        }
    }

    /// A small PNG with a light border, a picture for the list.
    fn sample_png() -> Vec<u8> {
        let mut pixmap = Pixmap::new(160, 80).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(70, 130, 220, 255));
        pixmap.encode_png().unwrap()
    }

    /// Writes `card` over the screen's colour with the shadow's room, as a
    /// PNG named `name` in `EDEL_CLIPBOARD_PNG`, when that is set.
    fn preview(card: &Pixmap, tokens: &Tokens, name: &str) {
        use tiny_skia::{PixmapPaint, Transform};
        let Some(dir) = std::env::var_os("EDEL_CLIPBOARD_PNG") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let room = 24u32 * 2;
        let mut screen = Pixmap::new(card.width() + 2 * room, card.height() + 2 * room).unwrap();
        screen.fill(tiny_skia::Color::from_rgba8(
            (tokens.background.r * 255.0) as u8,
            (tokens.background.g * 255.0) as u8,
            (tokens.background.b * 255.0) as u8,
            255,
        ));
        if let Some(shadow) = paint::shadow(
            card.width(),
            card.height(),
            room,
            tokens.radius_menu as f32 * 2.0,
            tokens,
            2.0,
        ) {
            screen.draw_pixmap(
                0,
                0,
                shadow.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        screen.draw_pixmap(
            room as i32,
            room as i32,
            card.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        screen.save_png(dir.join(format!("{name}.png"))).unwrap();
    }

    /// Without fonts the card still draws: the paint's light and card are
    /// there at every row.
    #[test]
    fn the_card_is_drawn_without_text() {
        let tokens = Tokens::built_in();
        let v = text_view(&["one"], 0, 1280);
        let l = lay(&v);
        let (w, h) = l.card;
        let mut pixmap = Pixmap::new(w, h).unwrap();
        paint(&mut pixmap, &v, &l, &tokens, None, 1.0);
        let c = pixmap.pixel(w / 2, h / 2).unwrap();
        assert_ne!(c.alpha(), 0, "the card is drawn");
    }
}
