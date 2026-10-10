//! The panel's menu (M5.31b, M5.31c): a right click on a panel where no
//! widget gives an action for it (empty space, or a widget without a menu
//! of its own) opens a card at the pointer's place along the panel. Its
//! rows are what the panel is and what a person can change about it: Edit
//! panels, Bar or Dock, the Size, Floating or Hide when covered, On every
//! screen, moving the panel to the other edge, adding a panel there and
//! removing this one. Like the tiling styles' menu it is a card of rows,
//! the keyboard exclusive, Escape closing it, and it holds its surface only
//! while open. Plain data and drawing, tested without a display;
//! `panel_menu_card.rs` owns the surface and makes each change through
//! `edel::panel_edit`.

use accesskit::Role;
use anyhow::Result;
use edel::i18n::tr;
use edel::panel_edit;
use edel::presets::{self, Edge, Hide, Screens, Size, Style};
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item as Node;
use crate::paint::{Text, fill, lit};
use crate::popup::{self, INSET, PAD, Rect};
use crate::trayview::Key;

/// What a row is or does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Opens the editor (`editor_card.rs`).
    Edit,
    /// The panel as a bar or a dock: a segmented row.
    Style,
    /// The panel's height: a segmented row.
    Size,
    /// A switch, flipped by a press or Return.
    Switch(Switch),
    /// Moves the panel to this edge, swapping with a panel there.
    Move(Edge),
    /// Adds an empty bar along this edge.
    Add(Edge),
    /// Takes this panel away.
    Remove,
}

/// The switches: a bar floats or a dock hides while covered, and the panel
/// is on every screen or the main one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Floating,
    Hide,
    Screens,
}

/// What the panel is, from its spec, and what lies around it.
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    pub style: Style,
    pub size: Size,
    pub floating: bool,
    pub hide: Hide,
    pub screens: Screens,
    pub edge: Edge,
    /// Whether no panel lies along the other edge.
    pub other_edge_free: bool,
    /// How many panels there are.
    pub panels: usize,
}

/// The styles a Style row's segments choose, left to right, with the names
/// CI's places line uses.
pub const STYLES: [(Style, &str); 2] = [(Style::Bar, "bar"), (Style::Dock, "dock")];

/// The sizes a Size row's segments choose, left to right, with their names.
pub const SIZES: [(Size, &str); 3] = [
    (Size::Small, "small"),
    (Size::Medium, "medium"),
    (Size::Large, "large"),
];

/// The other edge of the screen.
pub fn opposite(edge: Edge) -> Edge {
    match edge {
        Edge::Top => Edge::Bottom,
        Edge::Bottom => Edge::Top,
    }
}

/// The rows of the menu for `facts`, top to bottom: Edit panels; Style and
/// Size; Floating for a bar or Hide when covered for a dock; On every
/// screen; moving to the other edge; adding a panel there when that edge is
/// free; and Remove, when there is more than one panel.
pub fn rows(facts: &Facts) -> Vec<Row> {
    let other = opposite(facts.edge);
    let mut rows = vec![Row::Edit, Row::Style, Row::Size];
    rows.push(match facts.style {
        Style::Bar => Row::Switch(Switch::Floating),
        Style::Dock => Row::Switch(Switch::Hide),
    });
    rows.push(Row::Switch(Switch::Screens));
    rows.push(Row::Move(other));
    if facts.other_edge_free {
        rows.push(Row::Add(other));
    }
    if facts.panels > 1 {
        rows.push(Row::Remove);
    }
    rows
}

/// The row as people read it.
pub fn label(row: Row) -> String {
    match row {
        Row::Edit => tr("Edit panels").into(),
        Row::Style => tr("Style").into(),
        Row::Size => tr("Size").into(),
        Row::Switch(Switch::Floating) => tr("Floating").into(),
        Row::Switch(Switch::Hide) => tr("Hide when a window covers it").into(),
        Row::Switch(Switch::Screens) => tr("On every screen").into(),
        Row::Move(Edge::Top) => tr("Move to the top").into(),
        Row::Move(Edge::Bottom) => tr("Move to the bottom").into(),
        Row::Add(Edge::Top) => tr("Add a panel at the top").into(),
        Row::Add(Edge::Bottom) => tr("Add a panel at the bottom").into(),
        Row::Remove => tr("Remove this panel").into(),
    }
}

/// The words of a segmented row's segments, left to right; none for the
/// other rows.
pub fn words(row: Row) -> Vec<&'static str> {
    match row {
        Row::Style => vec![tr("Bar"), tr("Dock")],
        Row::Size => vec![tr("Small"), tr("Medium"), tr("Large")],
        _ => Vec::new(),
    }
}

/// The segment of a segmented row that `facts` has chosen.
pub fn chosen(facts: &Facts, row: Row) -> Option<usize> {
    match row {
        Row::Style => STYLES.iter().position(|(s, _)| *s == facts.style),
        Row::Size => SIZES.iter().position(|(s, _)| *s == facts.size),
        _ => None,
    }
}

/// Whether switch `switch` is on for `facts`.
pub fn on(facts: &Facts, switch: Switch) -> bool {
    match switch {
        Switch::Floating => facts.floating,
        Switch::Hide => facts.hide == Hide::Covered,
        Switch::Screens => facts.screens == Screens::Every,
    }
}

/// The names CI's places line gives a row's parts: one for an action or a
/// switch, one for each segment of a segmented row.
fn names(row: Row) -> &'static [&'static str] {
    match row {
        Row::Edit => &["row edit"],
        Row::Style => &["style bar", "style dock"],
        Row::Size => &["size small", "size medium", "size large"],
        Row::Switch(Switch::Floating) => &["switch floating"],
        Row::Switch(Switch::Hide) => &["switch hide"],
        Row::Switch(Switch::Screens) => &["switch screens"],
        Row::Move(_) => &["row move"],
        Row::Add(_) => &["row add"],
        Row::Remove => &["row remove"],
    }
}

/// Its width in logical pixels.
pub const WIDTH: u32 = 300;
/// A segment of a segmented row, logical pixels across, and the room its
/// control keeps round the segments.
const SEGMENT: f32 = 64.0;
const INNER: f32 = 2.0;
/// The room between a row's label and its control.
const GAP: f32 = 8.0;
/// A hairline's height, logical pixels: the layout reserves one, and it is
/// drawn one screen pixel high, as the drawer's rule is.
const HAIRLINE: f32 = 1.0;

/// What the menu shows: the rows for the panel, its facts and the row the
/// pointer or the arrows are on.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub rows: Vec<Row>,
    pub facts: Facts,
    pub lit: usize,
}

/// Where everything lies, logical pixels from the card's top left corner:
/// each row, the control at a row's right (a switch's check box, a
/// segmented row's control, none for the others), each segment of a
/// segmented row and the hairlines.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub rows: Vec<Rect>,
    pub controls: Vec<Option<Rect>>,
    pub segments: Vec<Vec<Rect>>,
    pub hairlines: Vec<Rect>,
}

/// Whether a hairline lies before `row`: before the Style row (after Edit
/// panels) and before a move, which begins the card's last group.
fn breaks_before(row: Row) -> bool {
    matches!(row, Row::Style | Row::Move(_))
}

/// The control at the right of `row`, lying at `rect`, its right edge at
/// `right`, and its segments. A switch's check box is the panel's glyph
/// wide; a segmented row's control is the panel's control high, with its
/// segments `INNER` in from its edges.
fn control_of(row: Row, rect: Rect, right: f32, tokens: &Tokens) -> (Option<Rect>, Vec<Rect>) {
    match row {
        Row::Switch(_) => {
            let glyph = tokens.panel_glyph as f32;
            (
                Some(Rect::new(right - glyph, rect.y, glyph, rect.h)),
                Vec::new(),
            )
        }
        Row::Style | Row::Size => {
            let count = words(row).len();
            let width = count as f32 * SEGMENT + 2.0 * INNER;
            let height = tokens.panel_control as f32;
            let x = right - width;
            let y = rect.y + (rect.h - height) / 2.0;
            let segments = (0..count)
                .map(|j| {
                    Rect::new(
                        x + INNER + j as f32 * SEGMENT,
                        y + INNER,
                        SEGMENT,
                        height - 2.0 * INNER,
                    )
                })
                .collect();
            (Some(Rect::new(x, y, width, height)), segments)
        }
        _ => (None, Vec::new()),
    }
}

/// The part a press or a key chooses: a row, or a segment of a segmented
/// row, as its row and its segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    Segment(usize, usize),
}

/// The card's size and where each part lies: the one function that places
/// everything. The rows stack from the top with the hairlines between
/// them, each row `tokens.row` high.
pub fn layout(view: &View, tokens: &Tokens) -> Layout {
    let row_h = tokens.row as f32;
    let inner = WIDTH as f32 - 2.0 * PAD;
    let right = WIDTH as f32 - PAD - INSET;
    let mut out = Layout {
        size: (WIDTH, 0),
        rows: Vec::new(),
        controls: Vec::new(),
        segments: Vec::new(),
        hairlines: Vec::new(),
    };
    let mut y = PAD;
    for (i, row) in view.rows.iter().enumerate() {
        if i > 0 && breaks_before(*row) {
            out.hairlines.push(Rect::new(PAD, y, inner, HAIRLINE));
            y += HAIRLINE;
        }
        let rect = Rect::new(PAD, y, inner, row_h);
        y += row_h;
        let (control, segments) = control_of(*row, rect, right, tokens);
        out.rows.push(rect);
        out.controls.push(control);
        out.segments.push(segments);
    }
    out.size = (WIDTH, (y + PAD) as u32);
    out
}

/// The card's size in logical pixels.
pub fn size(view: &View, tokens: &Tokens) -> (u32, u32) {
    layout(view, tokens).size
}

/// The row whose rectangle holds `x`, `y` (logical pixels from the card's
/// top left corner), if any.
pub fn row_at(l: &Layout, x: f32, y: f32) -> Option<usize> {
    l.rows.iter().position(|r| r.contains(x, y))
}

/// The part under `x`, `y`: a row without segments, or a segment of a
/// segmented row. The rest of a segmented row, its label and the hairlines
/// do nothing.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    let i = row_at(l, x, y)?;
    match l.segments[i].iter().position(|s| s.contains(x, y)) {
        Some(j) => Some(Hit::Segment(i, j)),
        None if l.segments[i].is_empty() => Some(Hit::Row(i)),
        None => None,
    }
}

/// What a key asks for: the lit row moved to a row, a part chosen, the
/// menu closed, or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Move(usize),
    Choose(Hit),
    Close,
    Nothing,
}

/// What a key does with `view`: Escape closes; Up and Down move the lit
/// row, Tab moves down; Left and Right change the chosen segment of a
/// Style or Size row, choosing the next one at once; Return and space
/// choose an action row or flip a switch. Nothing else does anything.
pub fn key(view: &View, key: Key) -> Step {
    let last = view.rows.len().saturating_sub(1);
    let lit = view.lit;
    match key {
        Key::Escape => Step::Close,
        Key::Up => Step::Move(lit.saturating_sub(1)),
        Key::Down | Key::Tab => Step::Move((lit + 1).min(last)),
        Key::Left | Key::Right => segment_step(view, key),
        Key::Activate => match view.rows.get(lit) {
            Some(Row::Style | Row::Size) | None => Step::Nothing,
            Some(_) => Step::Choose(Hit::Row(lit)),
        },
        _ => Step::Nothing,
    }
}

/// Left and Right on a segmented row: the segment beside the chosen one,
/// when there is one.
fn segment_step(view: &View, key: Key) -> Step {
    let Some(row) = view.rows.get(view.lit).copied() else {
        return Step::Nothing;
    };
    let Some(now) = chosen(&view.facts, row) else {
        return Step::Nothing;
    };
    let next = match key {
        Key::Left => now.checked_sub(1),
        _ => Some(now + 1).filter(|j| *j < words(row).len()),
    };
    match next {
        Some(j) => Step::Choose(Hit::Segment(view.lit, j)),
        None => Step::Nothing,
    }
}

/// The change a choice makes to the panels `panels`, the panel at index
/// `panel` being the menu's: the new panels and what it is called in the
/// log, or `None` when nothing changes (a segment already chosen, or a
/// choice that is not a change). Refused as `panel_edit` refuses it.
pub fn change(
    panels: &[presets::Panel],
    panel: usize,
    row: Row,
    hit: Hit,
) -> Result<Option<(Vec<presets::Panel>, String)>> {
    let Some(spec) = panels.get(panel) else {
        return Ok(None);
    };
    let done = match (row, hit) {
        (Row::Style, Hit::Segment(_, j)) => {
            let Some((style, name)) = STYLES.get(j) else {
                return Ok(None);
            };
            if spec.style == *style {
                return Ok(None);
            }
            (
                panel_edit::set_style(panels, panel, *style)?,
                format!("style {name}"),
            )
        }
        (Row::Size, Hit::Segment(_, j)) => {
            let Some((size, name)) = SIZES.get(j) else {
                return Ok(None);
            };
            if spec.size == *size {
                return Ok(None);
            }
            (
                panel_edit::set_size(panels, panel, *size)?,
                format!("size {name}"),
            )
        }
        (Row::Switch(Switch::Floating), Hit::Row(_)) => {
            let floating = !spec.floating;
            let name = if floating {
                "floating on"
            } else {
                "floating off"
            };
            (
                panel_edit::set_floating(panels, panel, floating)?,
                name.into(),
            )
        }
        (Row::Switch(Switch::Hide), Hit::Row(_)) => {
            let hide = if spec.hide == Hide::Covered {
                Hide::Never
            } else {
                Hide::Covered
            };
            let name = if hide == Hide::Covered {
                "hide covered"
            } else {
                "hide never"
            };
            (panel_edit::set_hide(panels, panel, hide)?, name.into())
        }
        (Row::Switch(Switch::Screens), Hit::Row(_)) => {
            let screens = if spec.screens == Screens::Every {
                Screens::Main
            } else {
                Screens::Every
            };
            let name = if screens == Screens::Every {
                "screens every"
            } else {
                "screens main"
            };
            (
                panel_edit::set_screens(panels, panel, screens)?,
                name.into(),
            )
        }
        (Row::Move(edge), Hit::Row(_)) => {
            let name = format!("moved to the {}", edge.name());
            (panel_edit::move_panel(panels, panel, edge)?, name)
        }
        (Row::Add(edge), Hit::Row(_)) => {
            let name = format!("added a panel at the {}", edge.name());
            (panel_edit::add_panel(panels, edge)?, name)
        }
        (Row::Remove, Hit::Row(_)) => {
            let name = format!("removed the {} panel", spec.edge.name());
            (panel_edit::remove_panel(panels, panel)?, name)
        }
        _ => return Ok(None),
    };
    let (new, name) = done;
    Ok((new != panels).then_some((new, name)))
}

/// The places the card and its parts lie, as the one log line CI reads:
/// `card WxH, row edit X+Y+WxH, style bar X+Y+WxH, style dock ..., size
/// small ..., switch floating ..., row move ..., row add ...`, logical
/// pixels from the card's corner.
pub fn places(view: &View, tokens: &Tokens) -> String {
    let l = layout(view, tokens);
    let at = |r: &Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut out = format!("card {}x{}", l.size.0, l.size.1);
    for (i, row) in view.rows.iter().enumerate() {
        let rects = if l.segments[i].is_empty() {
            vec![l.rows[i]]
        } else {
            l.segments[i].clone()
        };
        for (name, rect) in names(*row).iter().zip(rects) {
            out.push_str(&format!(", {name} {}", at(&rect)));
        }
    }
    out
}

/// What a screen reader finds: each action row as a button, each switch as
/// a check box with its state, and each segmented row as a radio group
/// whose segments are radio buttons, the chosen one on.
pub fn nodes(view: &View, l: &Layout) -> Vec<Node> {
    let at = |r: &Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let item = |role: Role, label: String, bounds, toggled, children| Node {
        role,
        label,
        bounds,
        children,
        toggled,
        value: None,
    };
    view.rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let bounds = at(&l.rows[i]);
            match row {
                Row::Style | Row::Size => {
                    let chosen = chosen(&view.facts, *row);
                    let children = words(*row)
                        .into_iter()
                        .enumerate()
                        .map(|(j, word)| {
                            let on = Some(chosen == Some(j));
                            item(
                                Role::RadioButton,
                                word.into(),
                                at(&l.segments[i][j]),
                                on,
                                Vec::new(),
                            )
                        })
                        .collect();
                    item(Role::RadioGroup, label(*row), bounds, None, children)
                }
                Row::Switch(switch) => {
                    let state = Some(on(&view.facts, *switch));
                    item(Role::CheckBox, label(*row), bounds, state, Vec::new())
                }
                _ => item(Role::Button, label(*row), bounds, None, Vec::new()),
            }
        })
        .collect()
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`: the card,
/// the lit row filled, the hairlines, each segmented row's control with its
/// chosen segment in the accent, each switch's check mark when it is on,
/// and every row's words.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    l: &Layout,
    tokens: &Tokens,
    text: Option<&mut Text>,
    scale: f32,
) {
    let s = scale;
    let mut text = text;
    popup::card(pixmap, tokens, s);
    if let Some(r) = l.rows.get(view.lit) {
        let radius = tokens.radius_control as f32 * s;
        fill(
            pixmap,
            r.x * s,
            r.y * s,
            r.w * s,
            r.h * s,
            radius,
            lit(tokens),
        );
    }
    for h in &l.hairlines {
        fill(
            pixmap,
            h.x * s,
            h.y * s,
            h.w * s,
            s.max(1.0),
            0.0,
            tokens.line,
        );
    }
    let size = tokens.panel_text_size as f32 * s;
    for (i, row) in view.rows.iter().enumerate() {
        let rect = l.rows[i];
        let control = l.controls[i];
        match (row, control) {
            (Row::Switch(switch), Some(c)) if on(&view.facts, *switch) => {
                popup::icon_in(
                    pixmap,
                    "check",
                    tokens.panel_glyph as f32,
                    c,
                    s,
                    tokens.accent,
                );
            }
            (Row::Style | Row::Size, Some(c)) => {
                let chosen = chosen(&view.facts, *row);
                paint_control(pixmap, c, chosen.map(|j| l.segments[i][j]), tokens, s);
            }
            _ => {}
        }
        if let Some(text) = text.as_deref_mut() {
            let left = PAD + INSET;
            let right = control.map_or(WIDTH as f32 - PAD - INSET, |c| c.x - GAP);
            let mut line = text.fit(&label(*row), size, (right - left) * s);
            let y = popup::middle(rect.y, rect.h, size, s);
            text.draw(pixmap, &mut line, left * s, y, tokens.panel_text);
        }
        if let Some(text) = text.as_deref_mut() {
            paint_words(
                pixmap,
                text,
                *row,
                &l.segments[i],
                chosen(&view.facts, *row),
                tokens,
                s,
            );
        }
    }
}

/// A segmented row's control: a raised rounded rect, its chosen segment
/// filled with the accent.
fn paint_control(
    pixmap: &mut Pixmap,
    control: Rect,
    chosen: Option<Rect>,
    tokens: &Tokens,
    s: f32,
) {
    let (x, y, w, h) = control.device(s);
    fill(
        pixmap,
        x,
        y,
        w,
        h,
        tokens.radius_control as f32 * s,
        popup::raised(tokens),
    );
    if let Some(seg) = chosen {
        let (x, y, w, h) = seg.device(s);
        fill(
            pixmap,
            x,
            y,
            w,
            h,
            tokens.radius_small as f32 * s,
            tokens.accent,
        );
    }
}

/// The words of a segmented row, each centred in its segment: the chosen
/// one in the accent's text colour, the others in the panel's.
fn paint_words(
    pixmap: &mut Pixmap,
    text: &mut Text,
    row: Row,
    segments: &[Rect],
    chosen: Option<usize>,
    tokens: &Tokens,
    s: f32,
) {
    let size = tokens.panel_text_small_size as f32 * s;
    for (j, (word, seg)) in words(row).into_iter().zip(segments).enumerate() {
        let ink = if Some(j) == chosen {
            tokens.accent_text
        } else {
            tokens.panel_text
        };
        let mut line = text.fit(word, size, seg.w * s);
        let x = seg.x * s + ((seg.w * s - line.width) / 2.0).round();
        let y = popup::middle(seg.y, seg.h, size, s);
        text.draw(pixmap, &mut line, x, y, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;

    fn classic_bar() -> Facts {
        Facts {
            style: Style::Bar,
            size: Size::Medium,
            floating: false,
            hide: Hide::Never,
            screens: Screens::Main,
            edge: Edge::Bottom,
            other_edge_free: true,
            panels: 1,
        }
    }

    fn dock_of_two() -> Facts {
        Facts {
            style: Style::Dock,
            other_edge_free: false,
            panels: 2,
            ..classic_bar()
        }
    }

    fn view(facts: Facts) -> View {
        View {
            rows: rows(&facts),
            facts,
            lit: 0,
        }
    }

    fn classic() -> Vec<presets::Panel> {
        presets::named(Some("classic")).0.panels
    }

    /// A point in the middle of `r`, logical pixels.
    fn middle_of(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn a_bar_of_one_panel_offers_floating_and_adding_at_the_top() {
        assert_eq!(
            rows(&classic_bar()),
            [
                Row::Edit,
                Row::Style,
                Row::Size,
                Row::Switch(Switch::Floating),
                Row::Switch(Switch::Screens),
                Row::Move(Edge::Top),
                Row::Add(Edge::Top),
            ]
        );
    }

    #[test]
    fn a_dock_of_two_offers_hiding_not_floating_and_removing_not_adding() {
        assert_eq!(
            rows(&dock_of_two()),
            [
                Row::Edit,
                Row::Style,
                Row::Size,
                Row::Switch(Switch::Hide),
                Row::Switch(Switch::Screens),
                Row::Move(Edge::Top),
                Row::Remove,
            ]
        );
    }

    #[test]
    fn the_labels_are_english_and_name_their_edge() {
        assert_eq!(label(Row::Edit), "Edit panels");
        assert_eq!(label(Row::Move(Edge::Top)), "Move to the top");
        assert_eq!(label(Row::Add(Edge::Bottom)), "Add a panel at the bottom");
        assert_eq!(
            label(Row::Switch(Switch::Hide)),
            "Hide when a window covers it"
        );
        assert_eq!(label(Row::Remove), "Remove this panel");
        assert_eq!(words(Row::Size), ["Small", "Medium", "Large"]);
    }

    #[test]
    fn the_card_has_hairlines_after_edit_and_before_a_move() {
        let tokens = Tokens::built_in();
        let v = view(classic_bar());
        let l = layout(&v, &tokens);
        let row = tokens.row as f32;
        assert_eq!(l.hairlines.len(), 2);
        assert_eq!(l.hairlines[0].y, l.rows[0].y + row, "after Edit panels");
        assert_eq!(l.rows[1].y, l.hairlines[0].y + HAIRLINE);
        assert_eq!(l.hairlines[1].y, l.rows[4].y + row, "before the move");
        assert_eq!(
            l.size,
            (WIDTH, (2.0 * PAD + 7.0 * row + 2.0 * HAIRLINE) as u32)
        );
    }

    #[test]
    fn hit_finds_each_segment_and_switch_and_nothing_in_a_label_or_a_hairline() {
        let tokens = Tokens::built_in();
        let v = view(classic_bar());
        let l = layout(&v, &tokens);
        let (x, y) = middle_of(l.segments[1][1]);
        assert_eq!(hit(&l, x, y), Some(Hit::Segment(1, 1)), "Dock");
        let (x, y) = middle_of(l.segments[1][0]);
        assert_eq!(hit(&l, x, y), Some(Hit::Segment(1, 0)), "Bar");
        let (x, y) = middle_of(l.segments[2][2]);
        assert_eq!(hit(&l, x, y), Some(Hit::Segment(2, 2)), "Large");
        let (x, y) = middle_of(l.rows[3]);
        assert_eq!(hit(&l, x, y), Some(Hit::Row(3)), "Floating");
        let (x, y) = middle_of(l.rows[0]);
        assert_eq!(hit(&l, x, y), Some(Hit::Row(0)), "Edit panels");
        let label = (PAD + INSET + 2.0, l.rows[1].y + 4.0);
        assert_eq!(hit(&l, label.0, label.1), None, "the rest of Style");
        assert_eq!(hit(&l, 2.0, l.rows[0].y + 4.0), None, "the padding");
        let rule = l.hairlines[0];
        assert_eq!(hit(&l, rule.x + 20.0, rule.y), None, "the hairline");
    }

    #[test]
    fn its_places_say_exactly_where_each_part_lies() {
        let tokens = Tokens::built_in();
        assert_eq!(
            places(&view(classic_bar()), &tokens),
            "card 300x270, row edit 8+8+284x36, style bar 150+50+64x26, \
             style dock 214+50+64x26, size small 86+86+64x26, \
             size medium 150+86+64x26, size large 214+86+64x26, \
             switch floating 8+117+284x36, switch screens 8+153+284x36, \
             row move 8+190+284x36, row add 8+226+284x36"
        );
    }

    #[test]
    fn keys_move_the_lit_row_and_choose_what_return_and_the_arrows_say() {
        let mut v = view(classic_bar());
        assert_eq!(key(&v, Key::Down), Step::Move(1));
        assert_eq!(key(&v, Key::Up), Step::Move(0), "not above the first");
        assert_eq!(key(&v, Key::Tab), Step::Move(1));
        assert_eq!(key(&v, Key::Escape), Step::Close);
        assert_eq!(key(&v, Key::Activate), Step::Choose(Hit::Row(0)));
        v.lit = 1;
        assert_eq!(key(&v, Key::Activate), Step::Nothing, "a segmented row");
        assert_eq!(key(&v, Key::Right), Step::Choose(Hit::Segment(1, 1)));
        assert_eq!(key(&v, Key::Left), Step::Nothing, "Bar is the first");
        v.lit = 3;
        assert_eq!(key(&v, Key::Activate), Step::Choose(Hit::Row(3)));
        assert_eq!(
            key(&v, Key::Right),
            Step::Nothing,
            "a switch has no segments"
        );
        let mut docked = view(dock_of_two());
        docked.lit = 1;
        assert_eq!(key(&docked, Key::Left), Step::Choose(Hit::Segment(1, 0)));
        assert_eq!(key(&docked, Key::Right), Step::Nothing, "Dock is the last");
    }

    #[test]
    fn a_choice_makes_the_change_it_names_or_none() {
        let panels = classic();
        let (new, name) = change(&panels, 0, Row::Style, Hit::Segment(1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(name, "style dock");
        assert_eq!(new[0].style, Style::Dock);
        assert!(
            change(&panels, 0, Row::Style, Hit::Segment(1, 0))
                .unwrap()
                .is_none()
        );
        let moved = change(&panels, 0, Row::Move(Edge::Top), Hit::Row(5)).unwrap();
        assert_eq!(moved.unwrap().1, "moved to the top");
        let floated = change(&panels, 0, Row::Switch(Switch::Floating), Hit::Row(3)).unwrap();
        assert_eq!(floated.unwrap().1, "floating on");
        assert!(
            change(&panels, 0, Row::Edit, Hit::Row(0))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn nodes_are_buttons_check_boxes_and_radio_groups_with_their_state() {
        let tokens = Tokens::built_in();
        let v = view(classic_bar());
        let l = layout(&v, &tokens);
        let items = nodes(&v, &l);
        assert_eq!(items[0].role, Role::Button);
        assert_eq!(items[0].label, "Edit panels");
        assert_eq!(items[1].role, Role::RadioGroup);
        assert_eq!(items[1].label, "Style");
        let segments: Vec<Option<bool>> = items[1].children.iter().map(|c| c.toggled).collect();
        assert_eq!(segments, [Some(true), Some(false)], "Bar is chosen");
        assert_eq!(items[3].role, Role::CheckBox);
        assert_eq!(items[3].toggled, Some(false));
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn the_chosen_segment_is_the_accent_and_the_card_is_round() {
        let tokens = Tokens::built_in();
        let v = view(classic_bar());
        let l = layout(&v, &tokens);
        let (w, h) = l.size;
        let mut pixmap = Pixmap::new(w * 2, h * 2).unwrap();
        paint(&mut pixmap, &v, &l, &tokens, None, 2.0);
        let at = |x: f32, y: f32| {
            let c = pixmap
                .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                .unwrap()
                .demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        };
        assert_eq!(at(0.0, 0.0)[3], 0, "a round corner");
        let accent = tokens.accent.bytes();
        let medium = l.segments[2][1];
        let small = l.segments[2][0];
        assert_eq!(
            at(medium.x + 3.0, medium.y + medium.h / 2.0)[..3],
            accent[..3]
        );
        assert_ne!(at(small.x + 3.0, small.y + small.h / 2.0)[..3], accent[..3]);
    }

    #[test]
    fn it_draws_light_and_dark_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        let dir = std::env::var_os("EDEL_EDITOR_PNG").map(std::path::PathBuf::from);
        for (name, facts) in [
            ("panel-menu", classic_bar()),
            ("panel-menu-dock", dock_of_two()),
        ] {
            for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
                let tokens = Tokens::built_in_scheme(scheme);
                let mut v = view(facts.clone());
                v.lit = 1;
                let l = layout(&v, &tokens);
                let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
                paint(&mut pixmap, &v, &l, &tokens, Some(&mut text), 2.0);
                assert_eq!(
                    pixmap.pixel(0, 0).unwrap().alpha(),
                    0,
                    "{mode}: a round corner"
                );
                if let Some(dir) = &dir {
                    std::fs::create_dir_all(dir).unwrap();
                    pixmap
                        .save_png(dir.join(format!("{name}-{mode}.png")))
                        .unwrap();
                }
            }
        }
    }
}
