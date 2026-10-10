//! The notification centre (M5.9b): what a click on the panel's clock
//! opens, two cards holding the notifications and the month. The first
//! card has a heading with a Do not disturb button (a moon) and, when two
//! or more notifications lie in the list, Clear all; below it the
//! notifications newest first, each a card of its own drawn as
//! `notice.rs` draws every notification. The second card is the month
//! calendar: the month and year with buttons to the month before and
//! after, the weekdays' letters from Monday, the days, today in the accent.
//! The two cards lie [`CARD_GAP`] apart on one surface; the room between
//! them takes no clicks ([`cards`] tells the popup where they are).
//! [`layout`] is the one function that says where everything lies, so a
//! redesign of the centre replaces it alone; on a Compact screen the card
//! is a sheet as wide as the screen and every target is at least 44 px.
//! The list shows as many whole cards as fit a budget and scrolls by the
//! wheel; a count says how many lie beyond.
//!
//! Plain data and drawing, tested without a display: [`view`] says what
//! shows, [`layout`] where, [`paint`] draws, [`hit`] and [`key`] turn the
//! pointer and the keyboard into an [`Act`], and [`items`] are what a
//! screen reader finds. `notify_card.rs` owns the surface, which exists
//! only while the centre is open.

use edel::app_icons::Icons;
use edel::i18n::{tr, trf};
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item as Node;
use crate::calendar::{self, Month, Ymd};
use crate::notice::{self, Hit, Placed};
use crate::notify::Notification;
use crate::paint::{Face, Text, fill, mix, outline};
use crate::popup::{self, Card, Rect, dim, icon_in, veil};
use crate::quick::Key;

/// The corner radius of both cards, logical pixels.
pub const RADIUS: f32 = 30.0;
/// The room between the two cards, logical pixels.
pub const CARD_GAP: f32 = 10.0;
/// The room round Clear all's label inside its pill, each side.
const CLEAR_PAD: f32 = 12.0;
/// Between Clear all and the Do not disturb button, logical pixels.
const HEAD_GAP: f32 = 8.0;
/// The room the month's name keeps from the card's edge, to line up with
/// the heading.
const TITLE_INDENT: f32 = 4.0;

/// The size of the words the centre sets, logical pixels: the titles',
/// the body's and the small ones' (Clear all, the count), from the
/// tokens' `panel_text`.
pub fn sizes(tokens: &Tokens) -> (f32, f32, f32) {
    let text = tokens.panel_text_size as f32;
    (text + 2.0, text - 0.5, text - 1.5)
}

/// What the open centre keeps besides the list: its width, whether
/// banners are kept away, the month shown and today, which notification
/// the list starts at, and where the pointer and the keyboard are.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub width: u32,
    pub compact: bool,
    pub dnd: bool,
    pub month: Month,
    pub today: Ymd,
    pub first: usize,
    pub hover: Option<Part>,
    pub focus: Option<Part>,
}

/// Where a pointer or the keyboard can be: a part of the shown
/// notification `i`, Clear all, the Do not disturb button (the moon), or
/// a month button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Clear,
    Card(usize, Hit),
    Dnd,
    Prev,
    Next,
}

/// Everything the centre shows at one moment, so it is drawn again only
/// when it changes.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub width: u32,
    pub compact: bool,
    /// The notifications shown, newest first, from the one the list
    /// starts at.
    pub items: Vec<notice::Item>,
    /// How many lie before them and after them, which the list's count
    /// line says.
    pub newer: usize,
    pub older: usize,
    pub dnd: bool,
    /// The width of the Clear all pill, logical pixels.
    pub clear: f32,
    pub month: Month,
    pub today: Ymd,
    pub hover: Option<Part>,
    pub focus: Option<Part>,
}

/// What a click or a key asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Dismiss the notification with this id
    Close(u32),
    /// Tell its app this action was invoked, then dismiss it
    Action(u32, String),
    /// A click on a notification's body: its `default` action, if it has
    /// one
    Default(u32),
    /// Dismiss every notification
    Clear,
    /// Switch do not disturb
    Dnd,
    /// The month shown, this many months on
    Month(i32),
    /// The list, this many notifications on
    Scroll(i32),
    /// Close the centre
    Hide,
}

// ---- What shows ----

/// The sizes of the centre's parts, logical pixels: the mockups' on a
/// desktop, touch sizes (at least 44 px where a finger lands) on a
/// Compact screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// The card's padding, all round its contents
    pub pad: f32,
    /// The heading row and the month's row: the moon button is as big as
    /// a row is high, and so are the month's buttons
    pub row: f32,
    /// Clear all's pill
    pub pill: f32,
    /// Between the heading and the list, and between the list and nothing
    pub section: f32,
    /// Between two notifications
    pub between: f32,
    /// The line saying how many lie beyond, and the empty list's row
    pub more: f32,
    pub empty: f32,
    /// The weekdays' row, a week of days, the room between two weeks
    pub weekday: f32,
    pub day: f32,
    pub week_gap: f32,
    /// Today's disc
    pub today: f32,
    /// What the list of notifications may take
    pub list: f32,
}

pub fn metrics(compact: bool) -> Metrics {
    if compact {
        Metrics {
            pad: 14.0,
            row: 44.0,
            pill: 44.0,
            section: 10.0,
            between: 8.0,
            more: 28.0,
            empty: 52.0,
            weekday: 28.0,
            day: 40.0,
            week_gap: 2.0,
            today: 36.0,
            list: 320.0,
        }
    } else {
        Metrics {
            pad: 16.0,
            row: 28.0,
            pill: 26.0,
            section: 10.0,
            between: 8.0,
            more: 22.0,
            empty: 52.0,
            weekday: 20.0,
            day: 30.0,
            week_gap: 2.0,
            today: 30.0,
            list: 280.0,
        }
    }
}

/// The width of the card on a screen `screen` wide, and whether it is a
/// sheet: 360 px from Compact up, as wide as the screen below it.
pub fn width_for(screen: u32) -> (u32, bool) {
    crate::banner::width_for(screen)
}

/// Whether Clear all shows: when two or more notifications lie in the
/// list, shown or not.
pub fn shows_clear(view: &View) -> bool {
    view.items.len() + view.newer + view.older >= 2
}

/// The centre showing `list` as `state` has it: the notifications from
/// `state.first` that fit the list's budget (at least one), each as
/// `notice::item` makes them with `measure`; `clear` is the width of the
/// words Clear all at their size and face.
pub fn view(
    state: &State,
    list: &[Notification],
    clear: f32,
    mut measure: impl FnMut(&str, Face) -> f32,
) -> View {
    let m = metrics(state.compact);
    let width = state.width as f32 - 2.0 * m.pad;
    let first = state.first.min(list.len().saturating_sub(1));
    let mut items: Vec<notice::Item> = Vec::new();
    let mut used = 0.0;
    for n in &list[first..] {
        let item = notice::item(n, width, state.compact, &mut measure);
        let h = notice::place(&item, (0.0, 0.0, width), state.compact)
            .whole
            .h;
        let next = used + if items.is_empty() { 0.0 } else { m.between } + h;
        if !items.is_empty() && next > m.list {
            break;
        }
        used = next;
        items.push(item);
    }
    View {
        width: state.width,
        compact: state.compact,
        newer: first,
        older: list.len() - first - items.len(),
        items,
        dnd: state.dnd,
        clear: clear + 2.0 * CLEAR_PAD,
        month: state.month,
        today: state.today,
        hover: state.hover,
        focus: state.focus,
    }
}

/// The line under the list when some notifications lie beyond it:
/// `2 newer, 3 older`; none when everything shows.
pub fn more(view: &View) -> Option<String> {
    let (newer, older) = (view.newer.to_string(), view.older.to_string());
    match (view.newer, view.older) {
        (0, 0) => None,
        (0, _) => Some(trf("{older} older", &[("older", &older)])),
        (_, 0) => Some(trf("{newer} newer", &[("newer", &newer)])),
        _ => Some(trf(
            "{newer} newer, {older} older",
            &[("newer", &newer), ("older", &older)],
        )),
    }
}

// ---- Where it lies ----

/// Where everything of a [`View`] lies, logical pixels from the card's
/// corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The whole surface: the two cards and the room between them.
    pub size: (u32, u32),
    pub metrics: Metrics,
    /// The notifications card and the calendar card, from the top
    pub boxes: [Rect; 2],
    pub title: Rect,
    pub clear: Option<Rect>,
    /// The words of an empty list
    pub empty: Option<Rect>,
    pub cards: Vec<Placed>,
    pub more: Option<Rect>,
    /// The Do not disturb button, the moon
    pub dnd: Rect,
    pub month: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub weekdays: Rect,
    pub grid: Rect,
}

/// Where the parts of `view` lie: the one function that lays the centre
/// out. The notifications card is `boxes[0]`, the calendar `boxes[1]`,
/// [`CARD_GAP`] apart.
pub fn layout(view: &View) -> Layout {
    let m = metrics(view.compact);
    let w = view.width as f32;
    let inner = w - 2.0 * m.pad;
    // The heading row, from the right: the moon, then Clear all.
    let moon = Rect::new(w - m.pad - m.row, m.pad, m.row, m.row);
    let clear = shows_clear(view).then(|| {
        Rect::new(
            moon.x - HEAD_GAP - view.clear,
            m.pad + (m.row - m.pill) / 2.0,
            view.clear,
            m.pill,
        )
    });
    let title_x = m.pad + TITLE_INDENT;
    let title_right = clear.unwrap_or(moon).x - HEAD_GAP;
    let title = Rect::new(title_x, m.pad, (title_right - title_x).max(0.0), m.row);
    // The notifications, or what an empty list says.
    let mut y = m.pad + m.row + m.section;
    let mut cards = Vec::new();
    let mut empty = None;
    let mut more_rect = None;
    if view.items.is_empty() {
        empty = Some(Rect::new(m.pad, y, inner, m.empty));
        y += m.empty;
    } else {
        for (i, item) in view.items.iter().enumerate() {
            if i > 0 {
                y += m.between;
            }
            let placed = notice::place(item, (m.pad, y, inner), view.compact);
            y += placed.whole.h;
            cards.push(placed);
        }
        if more(view).is_some() {
            more_rect = Some(Rect::new(m.pad, y + 2.0, inner, m.more));
            y += 2.0 + m.more;
        }
    }
    let first = Rect::new(0.0, 0.0, w, y + m.pad);
    // The calendar card: the month row, the weekdays, the days.
    let top = first.y + first.h + CARD_GAP;
    let mut y = top + m.pad;
    let next = Rect::new(w - m.pad - m.row, y, m.row, m.row);
    let prev = Rect::new(next.x - 4.0 - m.row, y, m.row, m.row);
    let month = Rect::new(title_x, y, (prev.x - 8.0 - title_x).max(0.0), m.row);
    y += m.row + 4.0;
    let weekdays = Rect::new(m.pad, y, inner, m.weekday);
    y += m.weekday;
    let weeks = calendar::WEEKS as f32;
    let grid_h = weeks * m.day + (weeks - 1.0) * m.week_gap;
    let grid = Rect::new(m.pad, y, inner, grid_h);
    y += grid_h + m.pad;
    let second = Rect::new(0.0, top, w, y - top);
    Layout {
        size: (view.width, (second.y + second.h).ceil() as u32),
        metrics: m,
        boxes: [first, second],
        title,
        clear,
        empty,
        cards,
        more: more_rect,
        dnd: moon,
        month,
        prev,
        next,
        weekdays,
        grid,
    }
}

/// The two cards as the popup holds them, the notifications card first.
pub fn cards(layout: &Layout) -> Vec<Card> {
    layout
        .boxes
        .iter()
        .map(|&rect| Card {
            rect,
            radius: RADIUS,
        })
        .collect()
}

/// Where the card's parts lie, as one log line CI reads: `card WxH,
/// clear X+Y+WxH, dnd X+Y+WxH, notificationN X+Y+WxH, closeN ..., prev
/// ..., next ...`, logical pixels from the card's corner.
pub fn places(layout: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut parts = vec![format!("card {}x{}", layout.size.0, layout.size.1)];
    if let Some(clear) = layout.clear {
        parts.push(format!("clear {}", at(clear)));
    }
    parts.push(format!("dnd {}", at(layout.dnd)));
    for (i, card) in layout.cards.iter().enumerate() {
        parts.push(format!("notification{i} {}", at(card.whole)));
        parts.push(format!("close{i} {}", at(card.close)));
    }
    for (name, rect) in [("prev", layout.prev), ("next", layout.next)] {
        parts.push(format!("{name} {}", at(rect)));
    }
    parts.join(", ")
}

/// The part of the card at `x`, `y` (logical pixels from its corner).
pub fn hit(layout: &Layout, x: f32, y: f32) -> Option<Part> {
    if layout.clear.is_some_and(|r| r.contains(x, y)) {
        return Some(Part::Clear);
    }
    for (i, card) in layout.cards.iter().enumerate() {
        if let Some(part) = notice::hit(card, x, y) {
            return Some(Part::Card(i, part));
        }
    }
    if layout.dnd.contains(x, y) {
        return Some(Part::Dnd);
    }
    if layout.prev.contains(x, y) {
        return Some(Part::Prev);
    }
    layout.next.contains(x, y).then_some(Part::Next)
}

/// What a press on `part` asks for.
pub fn press(view: &View, part: Part) -> Option<Act> {
    Some(match part {
        Part::Clear => Act::Clear,
        Part::Dnd => Act::Dnd,
        Part::Prev => Act::Month(-1),
        Part::Next => Act::Month(1),
        Part::Card(i, hit) => {
            let item = view.items.get(i)?;
            match hit {
                Hit::Close => Act::Close(item.id),
                Hit::Button(a) => Act::Action(item.id, item.buttons.get(a)?.key.clone()),
                Hit::Body if item.default => Act::Default(item.id),
                Hit::Body => return None,
            }
        }
    })
}

// ---- Keys ----

/// What a keyboard can reach, in Tab's order: Do not disturb, Clear all
/// when shown, then each notification's body (when the app gave a
/// `default` action), buttons and close button, then the two month
/// buttons.
pub fn ring(view: &View) -> Vec<Part> {
    let mut ring = vec![Part::Dnd];
    if shows_clear(view) {
        ring.push(Part::Clear);
    }
    for (i, item) in view.items.iter().enumerate() {
        if item.default {
            ring.push(Part::Card(i, Hit::Body));
        }
        ring.extend((0..item.buttons.len()).map(|a| Part::Card(i, Hit::Button(a))));
        ring.push(Part::Card(i, Hit::Close));
    }
    ring.extend([Part::Prev, Part::Next]);
    ring
}

/// What `key` does with the keyboard on `focus`: where it goes and what
/// it asks for. Escape closes; the first key to move gives the first
/// part, or with Shift and Up or Left the last.
pub fn key(view: &View, focus: Option<Part>, key: Key) -> (Option<Part>, Option<Act>) {
    let ring = ring(view);
    if key == Key::Escape {
        return (focus, Some(Act::Hide));
    }
    let Some(now) = focus.filter(|f| ring.contains(f)) else {
        let to = match key {
            Key::Tab(true) | Key::Up | Key::Left | Key::End => ring.last().copied(),
            _ => ring.first().copied(),
        };
        return (to, None);
    };
    let at = ring.iter().position(|f| *f == now).unwrap_or(0);
    let step = |by: usize| ring.get((at + by) % ring.len()).copied();
    match key {
        Key::Activate => (focus, press(view, now)),
        Key::Tab(false) | Key::Right | Key::Down => (step(1), None),
        Key::Tab(true) | Key::Left | Key::Up => (step(ring.len() - 1), None),
        Key::Home => (ring.first().copied(), None),
        Key::End => (ring.last().copied(), None),
        Key::Escape => (focus, Some(Act::Hide)),
    }
}

// ---- What a screen reader reads ----

/// The card's parts as a screen reader finds them, flat and in Tab's
/// order: the heading, the Do not disturb toggle, Clear all, each
/// notification as an alert followed by its buttons and close button,
/// then the month's heading and buttons; the index of the one holding the
/// keyboard.
pub fn items(view: &View, layout: &Layout) -> (Vec<Node>, Option<usize>) {
    let rect = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let mut out: Vec<Node> = Vec::new();
    let mut focused = None;
    let mut put = |out: &mut Vec<Node>, node: Node, part: Option<Part>| {
        if part.is_some() && part == view.focus {
            focused = Some(out.len());
        }
        out.push(node);
    };
    let plain = |role, label: String, r: Rect, toggled| Node {
        role,
        label,
        bounds: rect(r),
        children: Vec::new(),
        toggled,
        value: None,
    };
    put(
        &mut out,
        plain(
            accesskit::Role::Heading,
            tr("Notifications").to_string(),
            layout.title,
            None,
        ),
        None,
    );
    put(
        &mut out,
        plain(
            accesskit::Role::Switch,
            tr("Do not disturb").to_string(),
            layout.dnd,
            Some(view.dnd),
        ),
        Some(Part::Dnd),
    );
    if let Some(clear) = layout.clear {
        put(
            &mut out,
            plain(
                accesskit::Role::Button,
                tr("Clear all").to_string(),
                clear,
                None,
            ),
            Some(Part::Clear),
        );
    }
    if view.items.is_empty() {
        if let Some(empty) = layout.empty {
            put(
                &mut out,
                plain(
                    accesskit::Role::Label,
                    tr("No new notifications").to_string(),
                    empty,
                    None,
                ),
                None,
            );
        }
    }
    for (i, (item, placed)) in view.items.iter().zip(&layout.cards).enumerate() {
        let mut nodes = notice::nodes(item, placed, (0.0, 0.0));
        let close = nodes.pop();
        let mut rest = nodes.into_iter();
        if let Some(alert) = rest.next() {
            let part = item.default.then_some(Part::Card(i, Hit::Body));
            put(&mut out, alert, part);
        }
        for (a, button) in rest.enumerate() {
            put(&mut out, button, Some(Part::Card(i, Hit::Button(a))));
        }
        if let Some(close) = close {
            put(&mut out, close, Some(Part::Card(i, Hit::Close)));
        }
    }
    put(
        &mut out,
        plain(
            accesskit::Role::Heading,
            view.month.title(),
            layout.month,
            None,
        ),
        None,
    );
    put(
        &mut out,
        plain(
            accesskit::Role::Button,
            tr("Previous month").to_string(),
            layout.prev,
            None,
        ),
        Some(Part::Prev),
    );
    put(
        &mut out,
        plain(
            accesskit::Role::Button,
            tr("Next month").to_string(),
            layout.next,
            None,
        ),
        Some(Part::Next),
    );
    (out, focused)
}

// ---- How it looks ----

/// Draws `view` at `scale` into `pixmap`, which is the surface's size
/// times it: the two cards, then their contents; without `text`,
/// everything but the words.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    mut icons: Option<&mut Icons>,
    s: f32,
) {
    let l = layout(view);
    let m = l.metrics;
    popup::cards(pixmap, tokens, s, &cards(&l));
    let (title_px, body_px, small_px) = sizes(tokens);
    let hovered = |part: Part| view.hover == Some(part);
    let focused = |part: Part| view.focus == Some(part);
    let accent = tokens.accent;

    // The heading, the moon and Clear all.
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.fit_in(
            tr("Notifications"),
            title_px * s,
            l.title.w * s,
            Face::SEMIBOLD,
        );
        let y = popup::middle(l.title.y, l.title.h, title_px * s, s);
        text.draw(
            pixmap,
            &mut line,
            (l.title.x * s).round(),
            y,
            tokens.panel_text,
        );
    }
    let (mx, my, mw, mh) = l.dnd.device(s);
    let (back, ink) = if view.dnd {
        (accent, tokens.accent_text)
    } else {
        (veil(tokens, 0.05), dim(tokens))
    };
    fill(pixmap, mx, my, mw, mh, mw / 2.0, back);
    if hovered(Part::Dnd) {
        fill(pixmap, mx, my, mw, mh, mw / 2.0, veil(tokens, 0.05));
    }
    icon_in(pixmap, "moon", 14.0, l.dnd, s, ink);
    if focused(Part::Dnd) {
        outline(pixmap, (mx, my, mw, mh), mw / 2.0, 2.0 * s, accent);
    }
    if let Some(clear) = l.clear {
        let (x, y, w, h) = clear.device(s);
        fill(pixmap, x, y, w, h, h / 2.0, veil(tokens, 0.05));
        if hovered(Part::Clear) {
            fill(pixmap, x, y, w, h, h / 2.0, veil(tokens, 0.05));
        }
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.line_in(tr("Clear all"), small_px * s, Face::SEMIBOLD);
            let at = x + (w - line.width) / 2.0;
            let ty = popup::middle(clear.y, clear.h, small_px * s, s);
            text.draw(pixmap, &mut line, at, ty, accent);
        }
        if focused(Part::Clear) {
            outline(pixmap, (x, y, w, h), h / 2.0, 2.0 * s, accent);
        }
    }

    // The notifications, or what an empty list says.
    if let Some(empty) = l.empty {
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.line(tr("No new notifications"), body_px * s);
            let (x, _, w, _) = empty.device(s);
            let ty = popup::middle(empty.y, empty.h, body_px * s, s);
            let at = x + (w - line.width) / 2.0;
            text.draw(pixmap, &mut line, at, ty, dim(tokens));
        }
    }
    for (i, (item, placed)) in view.items.iter().zip(&l.cards).enumerate() {
        let on = |part: Part| match part {
            Part::Card(j, hit) if j == i => Some(hit),
            _ => None,
        };
        notice::paint(
            pixmap,
            tokens,
            text.as_deref_mut(),
            icons.as_deref_mut(),
            item,
            placed,
            (view.hover.and_then(on), view.focus.and_then(on)),
            (true, notice::CARD_RADIUS),
            view.compact,
            s,
        );
    }
    if let (Some(rect), Some(words), Some(text)) = (l.more, more(view), text.as_deref_mut()) {
        let mut line = text.fit(&words, small_px * s, rect.w * s);
        let (x, _, w, _) = rect.device(s);
        let ty = popup::middle(rect.y, rect.h, small_px * s, s);
        let at = x + (w - line.width) / 2.0;
        text.draw(pixmap, &mut line, at, ty, dim(tokens));
    }

    // The calendar: its month, the two buttons, the weekdays and the days.
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.fit_in(
            &view.month.title(),
            title_px * s,
            l.month.w * s,
            Face::SEMIBOLD,
        );
        let y = popup::middle(l.month.y, l.month.h, title_px * s, s);
        text.draw(
            pixmap,
            &mut line,
            (l.month.x * s).round(),
            y,
            tokens.panel_text,
        );
    }
    for (part, rect, icon) in [
        (Part::Prev, l.prev, "chevron-left"),
        (Part::Next, l.next, "chevron-right"),
    ] {
        let (x, y, w, h) = rect.device(s);
        let back = if hovered(part) { 0.085 } else { 0.05 };
        fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, back));
        icon_in(pixmap, icon, 12.0, rect, s, dim(tokens));
        if focused(part) {
            outline(pixmap, (x, y, w, h), w / 2.0, 2.0 * s, accent);
        }
    }
    let cell = (l.grid.w / 7.0, m.day + m.week_gap);
    let faint = mix(tokens.panel_text, tokens.panel, 0.65);
    if let Some(text) = text.as_deref_mut() {
        for d in 0..7 {
            let c = Rect::new(
                l.weekdays.x + d as f32 * cell.0,
                l.weekdays.y,
                cell.0,
                l.weekdays.h,
            );
            centred(
                pixmap,
                text,
                &calendar::initial(d),
                (c, (small_px - 1.0) * s, Face::SEMIBOLD),
                mix(tokens.panel_text, tokens.panel, 0.5),
                s,
            );
        }
    }
    for (w, week) in calendar::grid(view.month, Some(view.today))
        .iter()
        .enumerate()
    {
        for (d, day) in week.iter().enumerate() {
            let c = Rect::new(
                l.grid.x + d as f32 * cell.0,
                l.grid.y + w as f32 * cell.1,
                cell.0,
                m.day,
            );
            if day.today {
                let side = m.today.min(cell.0);
                let disc = Rect::new(
                    c.x + (c.w - side) / 2.0,
                    c.y + (c.h - side) / 2.0,
                    side,
                    side,
                );
                let (x, y, w, h) = disc.device(s);
                fill(pixmap, x, y, w, h, w / 2.0, accent);
            }
            let Some(text) = text.as_deref_mut() else {
                continue;
            };
            let (ink, face) = match (day.today, day.inside) {
                (true, _) => (tokens.accent_text, Face::SEMIBOLD.tabular()),
                (false, true) => (tokens.panel_text, Face::REGULAR.tabular()),
                (false, false) => (faint, Face::REGULAR.tabular()),
            };
            centred(
                pixmap,
                text,
                &day.day.to_string(),
                (c, body_px * s, face),
                ink,
                s,
            );
        }
    }
}

/// `words` in `rect`, in the middle both ways, at `size` device pixels.
fn centred(
    pixmap: &mut Pixmap,
    text: &mut Text,
    words: &str,
    (rect, size, face): (Rect, f32, Face),
    ink: edel::tokens::Colour,
    s: f32,
) {
    let mut line = text.line_in(words, size, face);
    let (x, _, w, _) = rect.device(s);
    let y = popup::middle(rect.y, rect.h, size, s);
    let at = x + (w - line.width) / 2.0;
    text.draw(pixmap, &mut line, at, y, ink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::{Call, received};

    fn six(s: &str, _: Face) -> f32 {
        s.chars().count() as f32 * 6.0
    }

    fn note(id: u32, summary: &str, body: &str, actions: &[&str]) -> Notification {
        received(
            id,
            &Call {
                app: "Files",
                icon: "",
                summary,
                body,
                actions,
                urgency: None,
                timeout: -1,
            },
        )
    }

    fn state(screen: u32) -> State {
        let (width, compact) = width_for(screen);
        State {
            width,
            compact,
            dnd: false,
            month: Month {
                year: 2026,
                month: 10,
            },
            today: (2026, 10, 3),
            first: 0,
            hover: None,
            focus: None,
        }
    }

    fn two() -> Vec<Notification> {
        vec![
            // Two cards that fit the list's budget together: the first is
            // a line of body and no button, the second two buttons.
            note(2, "Screenshot saved", "Saved in Pictures", &["default", ""]),
            note(
                1,
                "Update ready",
                "",
                &["now", "Restart now", "later", "Later"],
            ),
        ]
    }

    fn view_of(width: u32, list: &[Notification]) -> View {
        view(&state(width), list, 60.0, six)
    }

    #[test]
    fn newest_first_with_what_does_not_fit_counted() {
        let v = view_of(1280, &two());
        assert_eq!(v.width, 360);
        assert_eq!(
            v.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            [2, 1],
            "the list is newest first"
        );
        assert_eq!((v.newer, v.older), (0, 0));
        assert_eq!(more(&v), None);
        // Fifty fill the budget: a few whole cards show and the rest are counted.
        let many: Vec<Notification> = (1..=50)
            .rev()
            .map(|id| note(id, "Hello", "A short body", &[]))
            .collect();
        let v = view_of(1280, &many);
        assert!(
            v.items.len() >= 2 && v.items.len() < 10,
            "{}",
            v.items.len()
        );
        assert_eq!(v.older, 50 - v.items.len());
        assert_eq!(v.items[0].id, 50);
        let words = more(&v).unwrap();
        assert!(
            words.ends_with(" older") && !words.contains("newer"),
            "{words}"
        );
        // Scrolled on, the earlier ones are counted too.
        let mut s = state(1280);
        s.first = 3;
        let v = view(&s, &many, 60.0, six);
        assert_eq!(v.items[0].id, 47);
        assert_eq!(v.newer, 3);
        assert!(more(&v).unwrap().starts_with("3 newer, "), "{:?}", more(&v));
        // Scrolled past the end, the last still shows.
        s.first = 400;
        let v = view(&s, &many, 60.0, six);
        assert_eq!(v.items[0].id, 1);
        assert_eq!((v.items.len(), v.older), (1, 0));
    }

    #[test]
    fn one_notification_always_shows_even_when_it_is_taller_than_the_budget() {
        let tall = note(
            1,
            "x",
            "word ".repeat(100).as_str(),
            &["a", "A", "b", "B", "c", "C"],
        );
        let v = view_of(1280, &[tall]);
        assert_eq!(v.items.len(), 1);
    }

    #[test]
    fn at_1280_the_card_stacks_heading_list_row_and_calendar() {
        let v = view_of(1280, &two());
        let l = layout(&v);
        assert_eq!(l.size.0, 360);
        let m = l.metrics;
        assert!(l.clear.is_some());
        assert_eq!(l.cards.len(), 2);
        // From the top: the heading and the moon, the cards, then the month, the weekdays, the days.
        assert!(l.cards[0].whole.y >= l.title.y + l.title.h);
        assert!(l.cards[1].whole.y >= l.cards[0].whole.y + l.cards[0].whole.h + m.between - 0.01);
        assert!(l.boxes[0].y + l.boxes[0].h >= l.cards[1].whole.y + l.cards[1].whole.h);
        assert!(l.prev.y >= l.boxes[1].y + m.pad - 0.01);
        assert!(l.weekdays.y >= l.prev.y + l.prev.h);
        assert!(l.grid.y >= l.weekdays.y + l.weekdays.h);
        assert_eq!(l.grid.h, 6.0 * m.day + 5.0 * m.week_gap);
        assert_eq!(
            l.size.1,
            (l.grid.y + l.grid.h + m.pad).ceil() as u32,
            "the calendar card ends a pad below the days"
        );
        // Everything lies inside the card it is in.
        for r in [l.title, l.dnd, l.prev, l.next, l.weekdays, l.grid]
            .iter()
            .chain(l.clear.iter())
        {
            assert!(r.x >= 0.0 && r.right() <= 360.0 + 0.01, "{r:?}");
            assert!(r.y >= 0.0 && r.y + r.h <= l.size.1 as f32 + 0.01, "{r:?}");
        }
        // The moon ends the heading row, Clear all lies left of it with 8 px between.
        assert!(l.dnd.right() <= 360.0 - m.pad + 0.01);
        assert_eq!(l.clear.unwrap().right(), l.dnd.x - 8.0);
        // The month's buttons sit at the right end, next after prev.
        assert!(l.next.right() <= 360.0 - m.pad + 0.01);
        assert!(l.prev.right() < l.next.x);
        assert!(l.month.right() < l.prev.x);
    }

    #[test]
    fn the_centre_is_two_cards_apart() {
        let v = view_of(1280, &two());
        let l = layout(&v);
        let [a, b] = l.boxes;
        assert_eq!((a.w, b.w), (360.0, 360.0), "both as wide as the card");
        assert_eq!(a.y, 0.0);
        assert!((a.y + a.h + 10.0 - b.y).abs() < 0.01, "{a:?} {b:?}");
        assert_eq!(l.size.1, (b.y + b.h).ceil() as u32);
        let held = cards(&l);
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].rect, a);
        assert_eq!(held[1].rect, b);
        assert!(held.iter().all(|c| c.radius == RADIUS));
    }

    #[test]
    fn clear_all_shows_with_two_or_more() {
        let one = [note(1, "Saved", "", &[])];
        let v = view_of(1280, &one);
        assert!(!shows_clear(&v));
        assert!(layout(&v).clear.is_none());
        let v = view_of(1280, &two());
        assert!(shows_clear(&v));
        assert!(layout(&v).clear.is_some());
        // Scrolled to the last, with one newer beyond the list's end, it still shows.
        let mut s = state(1280);
        s.first = 1;
        let v = view(&s, &two(), 60.0, six);
        assert_eq!((v.items.len(), v.newer), (1, 1));
        assert!(layout(&v).clear.is_some());
        // The ring holds it only when it shows.
        assert!(!ring(&view_of(1280, &one)).contains(&Part::Clear));
        assert!(ring(&view_of(1280, &two())).contains(&Part::Clear));
    }

    #[test]
    fn an_empty_list_says_so_and_has_no_clear_all() {
        let v = view_of(1280, &[]);
        assert!(v.items.is_empty());
        let l = layout(&v);
        assert!(l.clear.is_none() && l.cards.is_empty() && l.more.is_none());
        let empty = l.empty.unwrap();
        assert!(empty.y >= l.title.y + l.title.h && empty.y + empty.h <= l.boxes[0].h);
        // Still the calendar, the same card layout whatever the list.
        assert!(l.prev.y > l.boxes[0].y + l.boxes[0].h);
        let ring: Vec<Part> = ring(&v);
        assert_eq!(ring, [Part::Dnd, Part::Prev, Part::Next]);
    }

    #[test]
    fn at_compact_width_it_is_a_sheet_with_44_px_targets() {
        let v = view_of(360, &two());
        assert!(v.compact);
        let l = layout(&v);
        assert_eq!(l.size.0, 360);
        assert!(l.clear.unwrap().h >= 44.0);
        assert!(l.dnd.h >= 44.0 && l.dnd.w >= 44.0);
        assert!(l.prev.w >= 44.0 && l.prev.h >= 44.0 && l.next.w >= 44.0);
        for card in &l.cards {
            assert!(card.close.w >= 44.0 && card.buttons.iter().all(|b| b.h >= 44.0));
        }
        let m = l.metrics;
        assert!(m.day >= 40.0 && m.row >= 44.0 && m.pill >= 44.0);
        for r in [l.title, l.dnd, l.prev, l.next, l.grid] {
            assert!(r.x >= 0.0 && r.right() <= 360.0 + 0.01, "{r:?}");
        }
    }

    #[test]
    fn a_click_finds_each_part_and_asks_for_the_right_thing() {
        let v = view_of(1280, &two());
        let l = layout(&v);
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let part = |r: Rect| {
            let (x, y) = mid(r);
            hit(&l, x, y)
        };
        assert_eq!(part(l.clear.unwrap()), Some(Part::Clear));
        assert_eq!(part(l.dnd), Some(Part::Dnd));
        assert_eq!(part(l.prev), Some(Part::Prev));
        assert_eq!(part(l.next), Some(Part::Next));
        assert_eq!(part(l.cards[0].close), Some(Part::Card(0, Hit::Close)));
        assert_eq!(
            part(l.cards[1].buttons[1]),
            Some(Part::Card(1, Hit::Button(1)))
        );
        assert_eq!(
            hit(&l, 100.0, l.cards[0].whole.y + 25.0),
            Some(Part::Card(0, Hit::Body))
        );
        // The padding round the notifications is the card's own, and the room between the cards is none.
        assert_eq!(hit(&l, 5.0, 5.0), None);
        assert_eq!(hit(&l, 100.0, l.boxes[0].y + l.boxes[0].h + 5.0), None);
        // What each press asks.
        assert_eq!(press(&v, Part::Clear), Some(Act::Clear));
        assert_eq!(press(&v, Part::Dnd), Some(Act::Dnd));
        assert_eq!(press(&v, Part::Prev), Some(Act::Month(-1)));
        assert_eq!(press(&v, Part::Next), Some(Act::Month(1)));
        assert_eq!(press(&v, Part::Card(0, Hit::Close)), Some(Act::Close(2)));
        assert_eq!(
            press(&v, Part::Card(1, Hit::Button(1))),
            Some(Act::Action(1, "later".into()))
        );
        // The first has a default action, the second has none.
        assert_eq!(press(&v, Part::Card(0, Hit::Body)), Some(Act::Default(2)));
        assert_eq!(press(&v, Part::Card(1, Hit::Body)), None);
        assert_eq!(press(&v, Part::Card(9, Hit::Close)), None);
    }

    #[test]
    fn the_keyboard_goes_round_and_acts() {
        let v = view_of(1280, &two());
        let ring = ring(&v);
        assert_eq!(
            ring,
            [
                Part::Dnd,
                Part::Clear,
                Part::Card(0, Hit::Body),
                Part::Card(0, Hit::Close),
                Part::Card(1, Hit::Button(0)),
                Part::Card(1, Hit::Button(1)),
                Part::Card(1, Hit::Close),
                Part::Prev,
                Part::Next,
            ]
        );
        // The first key gives the first part; Shift+Tab the last.
        assert_eq!(key(&v, None, Key::Tab(false)), (Some(Part::Dnd), None));
        assert_eq!(key(&v, None, Key::Tab(true)), (Some(Part::Next), None));
        // Tab goes on and wraps, Shift+Tab goes back.
        assert_eq!(
            key(&v, Some(Part::Next), Key::Tab(false)).0,
            Some(Part::Dnd)
        );
        assert_eq!(key(&v, Some(Part::Dnd), Key::Tab(true)).0, Some(Part::Next));
        assert_eq!(key(&v, Some(Part::Dnd), Key::Down).0, Some(Part::Clear));
        assert_eq!(
            key(&v, Some(Part::Clear), Key::Tab(true)).0,
            Some(Part::Dnd)
        );
        // Return and space act, Escape closes.
        assert_eq!(
            key(&v, Some(Part::Card(0, Hit::Close)), Key::Activate),
            (Some(Part::Card(0, Hit::Close)), Some(Act::Close(2)))
        );
        assert_eq!(key(&v, Some(Part::Dnd), Key::Activate).1, Some(Act::Dnd));
        assert_eq!(key(&v, None, Key::Escape).1, Some(Act::Hide));
        // A part that is gone (its notification was dismissed) starts again.
        assert_eq!(
            key(&v, Some(Part::Card(5, Hit::Close)), Key::Tab(false)),
            (Some(Part::Dnd), None)
        );
    }

    #[test]
    fn a_screen_reader_hears_the_heading_the_toggle_each_notification_then_the_month() {
        let mut v = view_of(1280, &two());
        v.dnd = true;
        v.focus = Some(Part::Dnd);
        let l = layout(&v);
        let (nodes, focused) = items(&v, &l);
        let said: Vec<(accesskit::Role, &str)> =
            nodes.iter().map(|n| (n.role, n.label.as_str())).collect();
        assert_eq!(said[0], (accesskit::Role::Heading, "Notifications"));
        assert_eq!(said[1], (accesskit::Role::Switch, "Do not disturb"));
        assert_eq!(said[2], (accesskit::Role::Button, "Clear all"));
        assert_eq!(
            said[3],
            (
                accesskit::Role::Alert,
                "Notification from Files: Screenshot saved, Saved in Pictures"
            )
        );
        assert!(said.contains(&(accesskit::Role::Button, "Restart now")));
        assert_eq!(nodes[1].toggled, Some(true));
        assert_eq!(focused, Some(1), "the node holding the keyboard");
        assert!(said.contains(&(accesskit::Role::Heading, "October 2026")));
        assert_eq!(
            said.last().unwrap(),
            &(accesskit::Role::Button, "Next month")
        );
        // The empty list is a label.
        let (nodes, _) = items(&view_of(1280, &[]), &layout(&view_of(1280, &[])));
        assert!(nodes.iter().any(|n| n.label == "No new notifications"));
    }

    #[test]
    fn its_places_are_one_line_for_ci() {
        let l = layout(&view_of(1280, &two()));
        let line = places(&l);
        assert!(
            line.starts_with(&format!("card 360x{}, clear ", l.size.1)),
            "{line}"
        );
        for want in [
            "dnd ",
            "notification0 ",
            "close0 ",
            "notification1 ",
            "prev ",
            "next ",
        ] {
            assert!(line.contains(want), "{want} in {line}");
        }
    }

    #[test]
    fn it_draws_the_cards_the_moon_the_calendar_and_today_in_the_accent() {
        for scheme in [edel::tokens::Scheme::Light, edel::tokens::Scheme::Dark] {
            let tokens = Tokens::built_in_scheme(scheme);
            for width in [1280, 360] {
                for dnd in [false, true] {
                    let mut v = view_of(width, &two());
                    v.dnd = dnd;
                    let l = layout(&v);
                    let s = 2.0;
                    let mut pixmap =
                        Pixmap::new((l.size.0 as f32 * s) as u32, (l.size.1 as f32 * s) as u32)
                            .unwrap();
                    paint(&mut pixmap, &v, &tokens, None, None, s);
                    let at = |x: f32, y: f32| {
                        let c = pixmap
                            .pixel((x * s) as u32, (y * s) as u32)
                            .unwrap()
                            .demultiply();
                        [c.red(), c.green(), c.blue()]
                    };
                    assert_eq!(pixmap.pixel(0, 0).unwrap().alpha(), 0, "a round corner");
                    // The room between the two cards is clear.
                    let gap_y = l.boxes[0].y + l.boxes[0].h + 5.0;
                    assert_eq!(
                        pixmap
                            .pixel((180.0 * s) as u32, (gap_y * s) as u32)
                            .unwrap()
                            .alpha(),
                        0,
                        "{width}: the room between the cards"
                    );
                    // The notifications card, in its padding above the heading, is the panel's.
                    assert_eq!(at(180.0, 8.0), tokens.panel.bytes()[..3], "{width}");
                    // Today, 3 October, is the 6th cell of the first week: an accent disc,
                    // sampled off its digit.
                    let cell_w = l.grid.w / 7.0;
                    let today = (
                        l.grid.x + 5.5 * cell_w + 10.0,
                        l.grid.y + l.metrics.day / 2.0,
                    );
                    assert_eq!(
                        at(today.0, today.1),
                        tokens.accent.bytes()[..3],
                        "{width} {dnd}"
                    );
                    // Another day, Tuesday the 6th, is the card's own colour.
                    let other = (
                        l.grid.x + 1.1 * cell_w,
                        l.grid.y + l.metrics.day + l.metrics.week_gap + 2.0,
                    );
                    assert_eq!(at(other.0, other.1), tokens.panel.bytes()[..3]);
                    // The moon is the accent only when on.
                    let moon = (l.dnd.x + 3.0, l.dnd.y + l.dnd.h / 2.0);
                    assert_eq!(
                        at(moon.0, moon.1) == tokens.accent.bytes()[..3],
                        dnd,
                        "the moon shows {dnd}"
                    );
                }
            }
        }
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    /// With fonts, draws the banner and the centre light and dark at 1280
    /// and at 360 wide, and with `EDEL_NOTIFY_PNG=DIR` writes them as PNGs
    /// over the screen's colour with the shadow's room, as the previews.
    #[test]
    fn the_banner_and_the_centre_draw_with_fonts_and_write_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        let list = two();
        for (scheme, mode) in [
            (edel::tokens::Scheme::Light, "light"),
            (edel::tokens::Scheme::Dark, "dark"),
        ] {
            let tokens = Tokens::built_in_scheme(scheme);
            let (_, body, _, _) = notice::sizes(&tokens);
            let (_, _, small) = sizes(&tokens);
            for screen in [1280u32, 360] {
                let s = 2.0;
                // The centre.
                let mut st = state(screen);
                st.dnd = true;
                st.hover = None;
                let clear = text.line_in("Clear all", small, Face::SEMIBOLD).width;
                let v = view(&st, &list, clear, |t, face| {
                    let _ = face;
                    text.line_in(t, body, Face::REGULAR).width
                });
                let l = layout(&v);
                let mut pixmap =
                    Pixmap::new((l.size.0 as f32 * s) as u32, (l.size.1 as f32 * s) as u32)
                        .unwrap();
                paint(&mut pixmap, &v, &tokens, Some(&mut text), None, s);
                // Some ink in the heading, the moon and the days.
                let ink = |r: Rect| {
                    let (x, y, w, h) = r.device(s);
                    (y as u32..(y + h) as u32)
                        .flat_map(|py| (x as u32..(x + w) as u32).map(move |px| (px, py)))
                        .filter(|&(px, py)| {
                            let c = pixmap.pixel(px, py).unwrap().demultiply();
                            [c.red(), c.green(), c.blue()] != tokens.panel.bytes()[..3]
                        })
                        .count()
                };
                assert!(ink(l.title) > 20, "{screen} {mode}: the heading is drawn");
                assert!(ink(l.month) > 20, "{screen} {mode}: the month is drawn");
                assert!(ink(l.grid) > 100, "{screen} {mode}: the days are drawn");
                preview(&pixmap, &tokens, &format!("centre-{mode}-{screen}"));
                // The banner for the second notification, with buttons.
                let b = crate::banner::view(&list[1], screen, |t, face| {
                    let _ = face;
                    text.line_in(t, body, Face::REGULAR).width
                });
                let bl = crate::banner::layout(&b);
                let mut pixmap =
                    Pixmap::new((bl.size.0 as f32 * s) as u32, (bl.size.1 as f32 * s) as u32)
                        .unwrap();
                crate::banner::paint(&mut pixmap, &b, &tokens, Some(&mut text), None, s);
                preview(&pixmap, &tokens, &format!("banner-{mode}-{screen}"));
            }
        }
    }

    /// Writes `card` over the screen's colour with the shadow's room, as a
    /// PNG named `name` in `EDEL_NOTIFY_PNG`, when that is set.
    fn preview(card: &Pixmap, tokens: &Tokens, name: &str) {
        use tiny_skia::{PixmapPaint, Transform};
        let Some(dir) = std::env::var_os("EDEL_NOTIFY_PNG") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let room = 24u32 * 2;
        let mut screen = Pixmap::new(card.width() + 2 * room, card.height() + 2 * room).unwrap();
        screen.fill(
            tiny_skia::Color::from_rgba(
                tokens.background.r,
                tokens.background.g,
                tokens.background.b,
                1.0,
            )
            .unwrap(),
        );
        if let Some(shadow) = crate::paint::shadow(
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
}
