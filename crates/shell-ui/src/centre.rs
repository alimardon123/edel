//! The notification centre (M5.9b): what a click on the panel's clock
//! opens, one card holding, from the top, a heading with Clear all, the
//! notifications newest first, each a card of its own drawn as
//! `notice.rs` draws every notification, a Do not disturb row with its
//! switch, and a month calendar (the month and year with buttons to the
//! month before and after, the weekdays' letters from Monday, the days,
//! today in the accent). [`layout`] is the one function that says where
//! everything lies, so a redesign of the centre replaces it alone; on a
//! Compact screen the card is a sheet as wide as the screen and every
//! target is at least 44 px. The list shows as many whole cards as fit a
//! budget and scrolls by the wheel; a count says how many lie beyond.
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
use crate::popup::{self, Rect, dim, icon_in, knob, veil};
use crate::quick::Key;

/// The size of the words the centre sets: the heading's, the rows' and
/// the small ones' offsets from the tokens' `panel_text`.
pub fn sizes(tokens: &Tokens) -> (f32, f32, f32) {
    let text = tokens.panel_text_size as f32;
    (text + 1.0, text, text - 1.0)
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
/// notification `i`, Clear all, the Do not disturb row, or a month button.
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
    /// The width of the Clear all button, logical pixels.
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
    pub pad: f32,
    /// Between the card's sections
    pub section: f32,
    pub header: f32,
    /// The Do not disturb row, its circle and its switch
    pub row: f32,
    pub circle: f32,
    pub switch: (f32, f32),
    /// A month button's square, a weekday row, a week
    pub nav: f32,
    pub weekday: f32,
    pub day: f32,
    /// What the list of notifications may take, and the room between
    /// its cards
    pub list: f32,
    pub between: f32,
    /// The line saying how many lie beyond, and the empty list's
    pub more: f32,
    pub empty: f32,
}

pub fn metrics(compact: bool) -> Metrics {
    if compact {
        Metrics {
            pad: 14.0,
            section: 12.0,
            header: 44.0,
            row: 60.0,
            circle: 36.0,
            switch: (48.0, 28.0),
            nav: 44.0,
            weekday: 28.0,
            day: 40.0,
            list: 230.0,
            between: 8.0,
            more: 28.0,
            empty: 56.0,
        }
    } else {
        Metrics {
            pad: 12.0,
            section: 10.0,
            header: 32.0,
            row: 52.0,
            circle: 30.0,
            switch: (38.0, 22.0),
            nav: 28.0,
            weekday: 24.0,
            day: 34.0,
            list: 250.0,
            between: 8.0,
            more: 22.0,
            empty: 52.0,
        }
    }
}

/// The width of the card on a screen `screen` wide, and whether it is a
/// sheet: 360 px from Compact up, as wide as the screen below it.
pub fn width_for(screen: u32) -> (u32, bool) {
    crate::banner::width_for(screen)
}

/// The room round the Clear all label inside its button, each side.
const CLEAR_PAD: f32 = 10.0;

/// The centre showing `list` as `state` has it: the notifications from
/// `state.first` that fit the list's budget (at least one), each as
/// `notice::item` makes them with `measure`; `clear` is the width of the
/// words Clear all at the row's size and face.
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
    pub size: (u32, u32),
    pub metrics: Metrics,
    pub title: Rect,
    pub clear: Option<Rect>,
    /// The words of an empty list
    pub empty: Option<Rect>,
    pub cards: Vec<Placed>,
    pub more: Option<Rect>,
    /// The hairlines above the Do not disturb row and above the calendar
    pub rules: [f32; 2],
    pub dnd: Rect,
    pub circle: Rect,
    pub words: Rect,
    pub switch: Rect,
    pub month: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub weekdays: Rect,
    pub grid: Rect,
}

/// Where the parts of `view` lie: the one function that lays the centre
/// out.
pub fn layout(view: &View) -> Layout {
    let m = metrics(view.compact);
    let w = view.width as f32;
    let inner = w - 2.0 * m.pad;
    let mut y = m.pad;
    let clear = (!view.items.is_empty())
        .then(|| Rect::new(w - m.pad - view.clear, y, view.clear, m.header));
    let title = Rect::new(
        m.pad + 4.0,
        y,
        inner - 4.0 - clear.map_or(0.0, |c| c.w),
        m.header,
    );
    y += m.header;
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
    y += m.section;
    let rule_a = y;
    y += 1.0 + m.section / 2.0;
    let dnd = Rect::new(m.pad, y, inner, m.row);
    let circle = Rect::new(
        m.pad + 4.0,
        y + (m.row - m.circle) / 2.0,
        m.circle,
        m.circle,
    );
    let switch = Rect::new(
        w - m.pad - 4.0 - m.switch.0,
        y + (m.row - m.switch.1) / 2.0,
        m.switch.0,
        m.switch.1,
    );
    let words = Rect::new(
        circle.right() + 10.0,
        y,
        switch.x - 10.0 - (circle.right() + 10.0),
        m.row,
    );
    y += m.row + m.section / 2.0;
    let rule_b = y;
    y += 1.0 + m.section / 2.0;
    let next = Rect::new(w - m.pad - m.nav, y, m.nav, m.nav);
    let prev = Rect::new(next.x - 4.0 - m.nav, y, m.nav, m.nav);
    let month = Rect::new(m.pad + 4.0, y, prev.x - m.pad - 8.0, m.nav);
    y += m.nav + 2.0;
    let weekdays = Rect::new(m.pad, y, inner, m.weekday);
    y += m.weekday;
    let grid = Rect::new(m.pad, y, inner, m.day * calendar::WEEKS as f32);
    y += grid.h + m.pad;
    Layout {
        size: (view.width, y.ceil() as u32),
        metrics: m,
        title,
        clear,
        empty,
        cards,
        more: more_rect,
        rules: [rule_a, rule_b],
        dnd,
        circle,
        words,
        switch,
        month,
        prev,
        next,
        weekdays,
        grid,
    }
}

/// Where the card's parts lie, as one log line CI reads: `card WxH,
/// clear X+Y+WxH, dnd X+Y+WxH, prev ..., next ...` and, for each
/// notification shown, `notificationN X+Y+WxH`, logical pixels from the
/// card's corner.
pub fn places(layout: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut parts = vec![format!("card {}x{}", layout.size.0, layout.size.1)];
    if let Some(clear) = layout.clear {
        parts.push(format!("clear {}", at(clear)));
    }
    for (i, card) in layout.cards.iter().enumerate() {
        parts.push(format!("notification{i} {}", at(card.whole)));
        parts.push(format!("close{i} {}", at(card.close)));
    }
    for (name, rect) in [
        ("dnd", layout.dnd),
        ("prev", layout.prev),
        ("next", layout.next),
    ] {
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

/// What a keyboard can reach, in Tab's order: Clear all, then each
/// notification's body (when the app gave a `default` action), buttons
/// and close button, then Do not disturb and the two month buttons.
pub fn ring(view: &View) -> Vec<Part> {
    let mut ring = Vec::new();
    if !view.items.is_empty() {
        ring.push(Part::Clear);
    }
    for (i, item) in view.items.iter().enumerate() {
        if item.default {
            ring.push(Part::Card(i, Hit::Body));
        }
        ring.extend((0..item.buttons.len()).map(|a| Part::Card(i, Hit::Button(a))));
        ring.push(Part::Card(i, Hit::Close));
    }
    ring.extend([Part::Dnd, Part::Prev, Part::Next]);
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
/// order, each notification an alert followed by its buttons and close
/// button; the index of the one holding the keyboard.
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
            accesskit::Role::Switch,
            tr("Do not disturb").to_string(),
            layout.dnd,
            Some(view.dnd),
        ),
        Some(Part::Dnd),
    );
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

/// Draws `view` at `scale` into `pixmap`, which is the card's size times
/// it; without `text`, everything but the words.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    mut icons: Option<&mut Icons>,
    s: f32,
) {
    popup::card(pixmap, tokens, s);
    let l = layout(view);
    let m = l.metrics;
    let hair = (0.5 * s).max(1.0);
    let (big, row, small) = sizes(tokens);
    let hovered = |part: Part| view.hover == Some(part);
    let focused = |part: Part| view.focus == Some(part);
    let r_control = tokens.radius_control as f32 * s;

    // The heading and Clear all.
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.fit_in(tr("Notifications"), big * s, l.title.w * s, Face::SEMIBOLD);
        let y = popup::middle(l.title.y, l.title.h, big * s, s);
        text.draw(
            pixmap,
            &mut line,
            (l.title.x * s).round(),
            y,
            tokens.panel_text,
        );
    }
    if let Some(clear) = l.clear {
        let (x, y, w, h) = clear.device(s);
        if hovered(Part::Clear) {
            fill(pixmap, x, y, w, h, r_control, veil(tokens, 0.06));
        }
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.line_in(tr("Clear all"), row * s, Face::MEDIUM);
            let at = x + (w - line.width) / 2.0;
            let ty = popup::middle(clear.y, clear.h, row * s, s);
            text.draw(pixmap, &mut line, at, ty, tokens.accent);
        }
        if focused(Part::Clear) {
            outline(pixmap, (x, y, w, h), r_control, 2.0 * s, tokens.accent);
        }
    }

    // The notifications, or what an empty list says.
    if let Some(empty) = l.empty {
        if let Some(text) = text.as_deref_mut() {
            let mut line = text.line(tr("No new notifications"), row * s);
            let (x, _, w, _) = empty.device(s);
            let ty = popup::middle(empty.y, empty.h, row * s, s);
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
            (true, tokens.radius_control as f32),
            view.compact,
            s,
        );
    }
    if let (Some(rect), Some(words), Some(text)) = (l.more, more(view), text.as_deref_mut()) {
        let mut line = text.fit(&words, small * s, rect.w * s);
        let (x, _, w, _) = rect.device(s);
        let ty = popup::middle(rect.y, rect.h, small * s, s);
        let at = x + (w - line.width) / 2.0;
        text.draw(pixmap, &mut line, at, ty, dim(tokens));
    }

    // The two hairlines.
    for rule in l.rules {
        let (x, y, w, _) = Rect::new(m.pad, rule, view.width as f32 - 2.0 * m.pad, 1.0).device(s);
        fill(pixmap, x, y, w, hair, 0.0, tokens.line);
    }

    // The Do not disturb row.
    let (rx, ry, rw, rh) = l.dnd.device(s);
    if hovered(Part::Dnd) {
        fill(pixmap, rx, ry, rw, rh, r_control, veil(tokens, 0.04));
    }
    let (cx, cy, cw, ch) = l.circle.device(s);
    let (disc, ink) = if view.dnd {
        (tokens.accent, tokens.accent_text)
    } else {
        (veil(tokens, 0.085), tokens.panel_text)
    };
    fill(pixmap, cx, cy, cw, ch, cw / 2.0, disc);
    icon_in(pixmap, "do-not-disturb", m.circle * 0.55, l.circle, s, ink);
    if let Some(text) = text.as_deref_mut() {
        let block = row * 1.25 + small * 1.25 + 1.0;
        let top = l.words.y + (l.words.h - block) / 2.0;
        let mut title = text.fit_in(tr("Do not disturb"), row * s, l.words.w * s, Face::SEMIBOLD);
        text.draw(
            pixmap,
            &mut title,
            (l.words.x * s).round(),
            (top * s).round(),
            tokens.panel_text,
        );
        let mut sub = text.fit(tr("Silence banners"), small * s, l.words.w * s);
        text.draw(
            pixmap,
            &mut sub,
            (l.words.x * s).round(),
            ((top + row * 1.25 + 1.0) * s).round(),
            dim(tokens),
        );
    }
    switch(pixmap, tokens, l.switch, view.dnd, s);
    if focused(Part::Dnd) {
        outline(pixmap, (rx, ry, rw, rh), r_control, 2.0 * s, tokens.accent);
    }

    // The calendar.
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.fit_in(&view.month.title(), row * s, l.month.w * s, Face::SEMIBOLD);
        let y = popup::middle(l.month.y, l.month.h, row * s, s);
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
        // The button is drawn at its desktop size inside a touch-sized hit area.
        let side = m.nav.min(28.0);
        let shown = Rect::new(
            rect.x + (rect.w - side) / 2.0,
            rect.y + (rect.h - side) / 2.0,
            side,
            side,
        );
        let (x, y, w, h) = shown.device(s);
        let back = if hovered(part) { 0.085 } else { 0.055 };
        fill(pixmap, x, y, w, h, r_control, veil(tokens, back));
        icon_in(pixmap, icon, 12.0, shown, s, dim(tokens));
        if focused(part) {
            outline(pixmap, (x, y, w, h), r_control, 2.0 * s, tokens.accent);
        }
    }
    let cell = (l.grid.w / 7.0, m.day);
    let faint = mix(tokens.panel_text, tokens.panel, 0.65);
    if let Some(text) = text.as_deref_mut() {
        for d in 0..7 {
            let c = Rect::new(
                l.weekdays.x + d as f32 * cell.0,
                l.weekdays.y,
                cell.0,
                l.weekdays.h,
            );
            let face = Face::SEMIBOLD;
            centred(
                pixmap,
                text,
                &calendar::initial(d),
                (c, small * s, face),
                dim(tokens),
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
                cell.1,
            );
            if day.today {
                let side = (cell.1 - 6.0).min(cell.0 - 4.0).min(32.0);
                let disc = Rect::new(
                    c.x + (c.w - side) / 2.0,
                    c.y + (c.h - side) / 2.0,
                    side,
                    side,
                );
                let (x, y, w, h) = disc.device(s);
                fill(pixmap, x, y, w, h, w / 2.0, tokens.accent);
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
                (c, row * s, face),
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

/// A switch in `rect`: a track, accent when on, and a knob at the end it
/// is set towards.
fn switch(pixmap: &mut Pixmap, tokens: &Tokens, rect: Rect, on: bool, s: f32) {
    let (x, y, w, h) = rect.device(s);
    let track = if on { tokens.accent } else { veil(tokens, 0.2) };
    fill(pixmap, x, y, w, h, h / 2.0, track);
    let d = h - (4.0 * s).round();
    let inset = ((h - d) / 2.0).round();
    let kx = if on { x + w - inset - d } else { x + inset };
    let shadow = edel::tokens::Colour {
        a: 0.22,
        ..tokens.shadow
    };
    fill(pixmap, kx, y + inset + s, d, d, d / 2.0, shadow);
    fill(pixmap, kx, y + inset, d, d, d / 2.0, knob(tokens));
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
            note(
                2,
                "Screenshot saved",
                "Saved in Pictures",
                &["default", "", "open", "Open"],
            ),
            note(
                1,
                "Update ready",
                "Edel OS 2026.11 starts on the next restart. If it fails to start, your computer goes back.",
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
            .map(|id| note(id, "Hello", "A short body", &["a", "A"]))
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
        // From the top: the heading, the cards, the hairline, the row, the hairline, the calendar.
        assert!(l.cards[0].whole.y >= l.title.y + l.title.h);
        assert!(l.cards[1].whole.y >= l.cards[0].whole.y + l.cards[0].whole.h + m.between - 0.01);
        let last = l.cards[1].whole;
        assert!(l.rules[0] > last.y + last.h);
        assert!(l.dnd.y > l.rules[0] && l.rules[1] > l.dnd.y + l.dnd.h);
        assert!(l.prev.y > l.rules[1]);
        assert!(l.weekdays.y >= l.prev.y + l.prev.h);
        assert!(l.grid.y >= l.weekdays.y + l.weekdays.h);
        assert_eq!(l.grid.h, 6.0 * m.day);
        assert_eq!(l.size.1, (l.grid.y + l.grid.h + m.pad).ceil() as u32);
        // Everything lies inside the card.
        for r in [
            l.title, l.dnd, l.switch, l.circle, l.prev, l.next, l.weekdays, l.grid,
        ]
        .iter()
        .chain(l.clear.iter())
        {
            assert!(r.x >= 0.0 && r.right() <= 360.0 + 0.01, "{r:?}");
            assert!(r.y >= 0.0 && r.y + r.h <= l.size.1 as f32 + 0.01, "{r:?}");
        }
        // The month's buttons sit at the right end, next after prev.
        assert!(l.next.right() <= 360.0 - m.pad + 0.01);
        assert!(l.prev.right() < l.next.x);
        assert!(l.month.right() < l.prev.x);
        // The switch ends the row and the words keep clear of it.
        assert!(l.words.right() < l.switch.x);
    }

    #[test]
    fn an_empty_list_says_so_and_has_no_clear_all() {
        let v = view_of(1280, &[]);
        assert!(v.items.is_empty());
        let l = layout(&v);
        assert!(l.clear.is_none() && l.cards.is_empty() && l.more.is_none());
        let empty = l.empty.unwrap();
        assert!(empty.y >= l.title.y + l.title.h && empty.y + empty.h <= l.rules[0]);
        // Still the row and the calendar, and the same card height whatever the list.
        assert!(l.dnd.y > l.rules[0]);
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
        assert!(l.dnd.h >= 44.0);
        assert!(l.prev.w >= 44.0 && l.prev.h >= 44.0 && l.next.w >= 44.0);
        for card in &l.cards {
            assert!(card.close.w >= 44.0 && card.buttons.iter().all(|b| b.h >= 44.0));
        }
        let m = l.metrics;
        assert!(m.day >= 40.0 && m.row >= 44.0 && m.header >= 44.0);
        for r in [l.title, l.dnd, l.switch, l.prev, l.next, l.grid] {
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
        assert_eq!(hit(&l, 5.0, 5.0), None);
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
                Part::Clear,
                Part::Card(0, Hit::Body),
                Part::Card(0, Hit::Button(0)),
                Part::Card(0, Hit::Close),
                Part::Card(1, Hit::Button(0)),
                Part::Card(1, Hit::Button(1)),
                Part::Card(1, Hit::Close),
                Part::Dnd,
                Part::Prev,
                Part::Next,
            ]
        );
        // The first key gives the first part; Shift+Tab the last.
        assert_eq!(key(&v, None, Key::Tab(false)), (Some(Part::Clear), None));
        assert_eq!(key(&v, None, Key::Tab(true)), (Some(Part::Next), None));
        // Tab goes on and wraps, Shift+Tab goes back.
        assert_eq!(
            key(&v, Some(Part::Next), Key::Tab(false)).0,
            Some(Part::Clear)
        );
        assert_eq!(
            key(&v, Some(Part::Clear), Key::Tab(true)).0,
            Some(Part::Next)
        );
        assert_eq!(key(&v, Some(Part::Dnd), Key::Down).0, Some(Part::Prev));
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
            (Some(Part::Clear), None)
        );
    }

    #[test]
    fn a_screen_reader_hears_each_notification_then_the_row_and_the_month() {
        let mut v = view_of(1280, &two());
        v.dnd = true;
        v.focus = Some(Part::Dnd);
        let l = layout(&v);
        let (nodes, focused) = items(&v, &l);
        let said: Vec<(accesskit::Role, &str)> =
            nodes.iter().map(|n| (n.role, n.label.as_str())).collect();
        assert_eq!(said[0], (accesskit::Role::Heading, "Notifications"));
        assert_eq!(said[1], (accesskit::Role::Button, "Clear all"));
        assert_eq!(
            said[2],
            (
                accesskit::Role::Alert,
                "Notification from Files: Screenshot saved, Saved in Pictures"
            )
        );
        assert!(said.contains(&(accesskit::Role::Button, "Restart now")));
        let dnd = nodes
            .iter()
            .position(|n| n.label == "Do not disturb")
            .unwrap();
        assert_eq!(nodes[dnd].toggled, Some(true));
        assert_eq!(focused, Some(dnd), "the node holding the keyboard");
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
            "notification0 ",
            "close0 ",
            "notification1 ",
            "dnd ",
            "prev ",
            "next ",
        ] {
            assert!(line.contains(want), "{want} in {line}");
        }
    }

    #[test]
    fn it_draws_the_card_the_row_the_calendar_and_today_in_the_accent() {
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
                    // Today, 3 October, is the 6th cell of the first week: an accent disc.
                    let cell_w = l.grid.w / 7.0;
                    let today = (l.grid.x + 5.5 * cell_w, l.grid.y + l.metrics.day / 2.0);
                    assert_eq!(
                        at(today.0, today.1),
                        tokens.accent.bytes()[..3],
                        "{width} {dnd}"
                    );
                    // Another day is the card's own colour.
                    let other = (l.grid.x + 1.5 * cell_w, l.grid.y + l.metrics.day * 2.5);
                    assert_eq!(at(other.0, other.1), tokens.panel.bytes()[..3]);
                    // The switch's track is the accent only when on.
                    let track = (l.switch.x + 4.0, l.switch.y + l.switch.h / 2.0);
                    assert_eq!(
                        at(track.0, track.1) == tokens.accent.bytes()[..3],
                        dnd,
                        "the switch shows {dnd}"
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
            let (_, row, small) = sizes(&tokens);
            for screen in [1280u32, 360] {
                let s = 2.0;
                // The centre.
                let mut st = state(screen);
                st.dnd = true;
                st.hover = None;
                let clear = text.line_in("Clear all", row, Face::MEDIUM).width;
                let v = view(&st, &list, clear, |t, face| {
                    text.line_in(t, small, face).width
                });
                let l = layout(&v);
                let mut pixmap =
                    Pixmap::new((l.size.0 as f32 * s) as u32, (l.size.1 as f32 * s) as u32)
                        .unwrap();
                paint(&mut pixmap, &v, &tokens, Some(&mut text), None, s);
                // Some ink in the heading, the row and a day.
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
                assert!(
                    ink(l.words) > 20,
                    "{screen} {mode}: the row's words are drawn"
                );
                assert!(ink(l.grid) > 100, "{screen} {mode}: the days are drawn");
                preview(&pixmap, &tokens, &format!("centre-{mode}-{screen}"));
                // The banner for the second notification, with buttons.
                let b = crate::banner::view(&list[1], screen, |t, face| {
                    text.line_in(t, small, face).width
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
