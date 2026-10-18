//! The Access portal's dialog (M6.3's screenshot): "Allow this app?", what
//! xdg-desktop-portal asks the person before an app may take a screenshot.
//! Plain data and drawing, tested without a display; `access_card.rs` owns
//! the surface, the keyboard and the answer. The card is `WIDTH` wide (less
//! on a narrow screen), the title in bold, the subtitle and the body under
//! it, and Deny (the quiet button, focused first) and Allow (the accent one)
//! at its bottom right. Every colour and size comes from the tokens or from
//! `edit_kit` and `popup`, so the dialog looks like the editor's buttons.
//!
//! The rectangles of `Layout` are the card's own, from its top left corner;
//! `hit`, `places` and `nodes` speak in the surface's coordinates, which hold
//! the room round the card for its shadow (`Layout::room`), as the pointer's
//! do on every popup.

use accesskit::Role;
use edel::i18n::trf;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item;
use crate::edit_kit::{self, BUTTON, BUTTON_TOUCH, ButtonKind, State, Style};
use crate::paint::{Face, Text};
use crate::popup::{self, Rect};
use crate::quick;
use crate::shell_bus::Answer;

/// The card's width, logical pixels; a screen narrower than it less `2 *
/// EDGE`, so the card keeps a gutter on each side.
pub const WIDTH: f32 = 400.0;
const EDGE: f32 = 16.0;
/// The room round the text inside the card, logical pixels.
const PAD: f32 = 20.0;
/// Between two blocks of text, between the text and the buttons, and between
/// the two buttons, logical pixels.
const GAP: f32 = 10.0;
const ACTIONS_GAP: f32 = 20.0;
const BUTTON_GAP: f32 = 8.0;
/// A button's width, logical pixels; its height is `BUTTON` or, on a Compact
/// screen, `BUTTON_TOUCH`, where a finger lands.
const BUTTON_WIDTH: f32 = 96.0;
/// The most lines each block takes; the last holds the rest, which is cut
/// short with an ellipsis when drawn (`Text::fit_in`).
const TITLE_LINES: usize = 2;
const SUBTITLE_LINES: usize = 3;
const BODY_LINES: usize = 4;
/// A line's height in em, and the width of a letter in em, which break the
/// text into lines before it is drawn. The letter width is an estimate from
/// Inter's average, a little over the true one so a line is never drawn
/// wider than its room.
const LEADING: f32 = 1.4;
const LETTER: f32 = 0.6;

/// The answers the Access portal knows: 0 when the app may take the picture,
/// 1 when it may not, and 2 when the question ended without an answer.
pub const ALLOW: u32 = 0;
pub const DENY: u32 = 1;
pub const CANCEL: u32 = 2;

/// A question an app's request put to the person: the app's name to show
/// (`app`, the portal's app id or "An app"), the portal's title, subtitle and
/// body (the title may be empty), the two button labels, and where the answer
/// goes. The portal's `choices` and `modal` are not kept: the dialog has
/// these two buttons and is always modal.
pub struct Ask {
    pub app: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub deny: String,
    pub grant: String,
    pub answer: Answer,
}

impl Ask {
    /// The title the dialog shows: the portal's own, or when it gives none,
    /// "Allow APP to take screenshots?", the question the screenshot portal
    /// asks.
    pub fn shown_title(&self) -> String {
        if self.title.is_empty() {
            trf(
                "Allow {app} to take screenshots?",
                &[("app", self.app.as_str())],
            )
        } else {
            self.title.clone()
        }
    }
}

/// One of the two buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Deny,
    Allow,
}

/// The other button: Tab and Shift+Tab move between the two.
pub fn other(b: Button) -> Button {
    match b {
        Button::Deny => Button::Allow,
        Button::Allow => Button::Deny,
    }
}

/// What the dialog shows and where the pointer and the keyboard are.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The title as shown (`Ask::shown_title`), and the portal's texts.
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub deny: String,
    pub allow: String,
    /// The button with the keyboard's focus: Deny at first, so a dialog that
    /// comes up while someone types is not answered by a stray Return.
    pub focus: Button,
    /// The button under the pointer, if any.
    pub hover: Option<Button>,
    /// The screen's width in logical pixels, 0 when unknown.
    pub width: u32,
    /// The room round the card for its shadow, logical pixels.
    pub room: u32,
}

/// A block of text the card holds: its lines from `top`, each `size` pixels
/// high times `LEADING`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub top: f32,
    pub size: u32,
    pub lines: Vec<String>,
    /// The title's weight, and the subtitle's dimmer ink.
    pub bold: bool,
    pub dim: bool,
}

impl Block {
    fn step(&self) -> f32 {
        self.size as f32 * LEADING
    }

    /// The line below the block's last line, logical pixels from the card's top.
    pub fn bottom(&self) -> f32 {
        self.top + self.lines.len() as f32 * self.step()
    }
}

/// Where the card's parts lie: its size, the room round it, the text blocks
/// that have lines, and the two buttons, all from the card's top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The card's size, logical pixels, without the room.
    pub size: (u32, u32),
    pub room: u32,
    pub text: Vec<Block>,
    pub deny: Rect,
    pub allow: Rect,
}

/// Whether the screen is narrower than quick settings' Compact width.
fn compact(view: &View) -> bool {
    view.width > 0 && view.width < quick::COMPACT_BELOW
}

/// `text` broken into at most `lines` lines in `room` pixels, by the estimate.
fn wrap_lines(text: &str, lines: usize, size: u32, room: f32) -> Vec<String> {
    popup::wrap(text, lines, room, |line| {
        line.chars().count() as f32 * size as f32 * LETTER
    })
}

/// The card's size and where its parts lie: the title, the subtitle and the
/// body from the top down, each block `GAP` below the one before and omitted
/// when empty, then the buttons `ACTIONS_GAP` below the text, Allow at the
/// right end, Deny beside it, and `PAD` at the bottom.
pub fn layout(view: &View, tokens: &Tokens) -> Layout {
    let width = if view.width > 0 {
        WIDTH.min(view.width as f32 - 2.0 * EDGE)
    } else {
        WIDTH
    };
    let inner = width - 2.0 * PAD;
    let mut text: Vec<Block> = Vec::new();
    let mut y = PAD;
    let parts = [
        (
            &view.title,
            TITLE_LINES,
            tokens.title_text_size,
            true,
            false,
        ),
        (
            &view.subtitle,
            SUBTITLE_LINES,
            tokens.text_size,
            false,
            true,
        ),
        (&view.body, BODY_LINES, tokens.text_size, false, false),
    ];
    for (words, lines, size, bold, dim) in parts {
        let lines = wrap_lines(words, lines, size, inner);
        if lines.is_empty() {
            continue;
        }
        if !text.is_empty() {
            y += GAP;
        }
        let block = Block {
            top: y,
            size,
            lines,
            bold,
            dim,
        };
        y = block.bottom();
        text.push(block);
    }
    y += ACTIONS_GAP;
    let button = if compact(view) { BUTTON_TOUCH } else { BUTTON };
    let allow = Rect::new(PAD + inner - BUTTON_WIDTH, y, BUTTON_WIDTH, button);
    let deny = Rect::new(allow.x - BUTTON_GAP - BUTTON_WIDTH, y, BUTTON_WIDTH, button);
    y += button + PAD;
    Layout {
        size: (width.ceil() as u32, y.ceil() as u32),
        room: view.room,
        text,
        deny,
        allow,
    }
}

/// `r` moved by the room, into the surface's coordinates.
fn in_surface(r: Rect, room: u32) -> Rect {
    let room = room as f32;
    Rect::new(r.x + room, r.y + room, r.w, r.h)
}

/// The button under `x`, `y` (the surface's logical pixels), if any.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Button> {
    let room = l.room as f32;
    let (x, y) = (x - room, y - room);
    if l.deny.contains(x, y) {
        Some(Button::Deny)
    } else if l.allow.contains(x, y) {
        Some(Button::Allow)
    } else {
        None
    }
}

/// Where the parts lie, as the log line CI reads, in the surface's logical
/// pixels: `card WxH, deny X+Y+WxH, allow X+Y+WxH`, where WxH is the card's.
pub fn places(l: &Layout) -> String {
    let at = |r: Rect| {
        let r = in_surface(r, l.room);
        format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h)
    };
    format!(
        "card {}x{}, deny {}, allow {}",
        l.size.0,
        l.size.1,
        at(l.deny),
        at(l.allow)
    )
}

/// What a screen reader finds: the dialog named by its title, its text when
/// it has any, then Deny and Allow, in that order and last.
pub fn nodes(view: &View, l: &Layout) -> Vec<Item> {
    let at = |r: Rect| {
        let r = in_surface(r, l.room);
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let item = |role, label: &str, bounds| Item {
        role,
        label: label.to_string(),
        bounds,
        children: Vec::new(),
        toggled: None,
        value: None,
        value_text: None,
        disabled: false,
    };
    let card = Rect::new(0.0, 0.0, l.size.0 as f32, l.size.1 as f32);
    let mut items = vec![item(Role::Dialog, &view.title, at(card))];
    let words: Vec<&str> = [view.subtitle.as_str(), view.body.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if !words.is_empty() {
        // The blocks that are not the title, the bold one.
        let blocks: Vec<&Block> = l.text.iter().filter(|b| !b.bold).collect();
        let top = blocks.first().map_or(0.0, |b| b.top);
        let bottom = blocks.last().map_or(top, |b| b.bottom());
        let area = Rect::new(PAD, top, card.w - 2.0 * PAD, bottom - top);
        items.push(item(Role::Label, &words.join(" "), at(area)));
    }
    items.push(item(Role::Button, &view.deny, at(l.deny)));
    items.push(item(Role::Button, &view.allow, at(l.allow)));
    items
}

/// The index in `nodes` of the button with the keyboard's focus.
pub fn focus_item(view: &View, l: &Layout) -> usize {
    let count = nodes(view, l).len();
    count - 2
        + match view.focus {
            Button::Deny => 0,
            Button::Allow => 1,
        }
}

/// The state of a button: focused, else under the pointer, else at rest.
fn state(view: &View, b: Button) -> State {
    if view.focus == b {
        State::Focus
    } else if view.hover == Some(b) {
        State::Hover
    } else {
        State::Rest
    }
}

/// Draws `view` at `s` into `pixmap`, which is the card's size times it (the
/// shadow is the popup's): the card in the panel's colour, the text in the
/// panel's ink, and the two buttons from `edit_kit` in the desktop's look.
/// Without `text`, the shapes only.
pub fn paint(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: Option<&mut Text>, s: f32) {
    let l = layout(view, tokens);
    popup::card(pixmap, tokens, s);
    let mut text = text;
    let inner = (l.size.0 as f32 - 2.0 * PAD) * s;
    if let Some(engine) = text.as_deref_mut() {
        for block in &l.text {
            let size = block.size as f32 * s;
            let ink = if block.dim {
                popup::dim(tokens)
            } else {
                tokens.panel_text
            };
            for (i, words) in block.lines.iter().enumerate() {
                let top = block.top + i as f32 * block.step();
                let mut line = if block.bold {
                    engine.fit_in(words, size, inner, Face::SEMIBOLD)
                } else {
                    engine.fit(words, size, inner)
                };
                let y = popup::middle(top, block.step(), size, s);
                engine.draw(pixmap, &mut line, PAD * s, y, ink);
            }
        }
    }
    for (b, rect, label, kind) in [
        (Button::Deny, l.deny, &view.deny, ButtonKind::Quiet),
        (Button::Allow, l.allow, &view.allow, ButtonKind::Accent),
    ] {
        edit_kit::button(
            pixmap,
            rect,
            label,
            kind,
            state(view, b),
            tokens,
            text.as_deref_mut(),
            Style::DESKTOP,
            s,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;
    use tiny_skia::Color;

    fn view(width: u32, body: &str) -> View {
        View {
            title: "Allow Shot to take screenshots?".into(),
            subtitle: String::new(),
            body: body.into(),
            deny: "Deny".into(),
            allow: "Allow".into(),
            focus: Button::Deny,
            hover: None,
            width,
            room: 0,
        }
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn the_layout_fits_the_text_and_both_buttons_inside_the_card() {
        let tokens = Tokens::built_in();
        let l = layout(
            &view(1280, "The app takes a picture of the whole screen."),
            &tokens,
        );
        assert_eq!(l.size.0, 400, "the card is WIDTH wide on a wide screen");
        assert_eq!(l.text.len(), 2, "the title and the body");
        let body = &l.text[1];
        assert!(
            body.bottom() <= l.deny.y - ACTIONS_GAP + 0.5,
            "the text ends above the buttons"
        );
        let (w, h) = (l.size.0 as f32, l.size.1 as f32);
        assert!(
            l.allow.right() <= w - PAD + 0.5,
            "Allow is inside the padding"
        );
        assert!(l.deny.x >= PAD - 0.5, "Deny is inside the padding");
        assert!(
            l.allow.y + l.allow.h + PAD <= h + 0.5,
            "the card ends PAD below the buttons"
        );
        assert_eq!(l.allow.h, BUTTON);
        assert_eq!(l.deny.y, l.allow.y, "the two buttons share a row");
    }

    #[test]
    fn a_longer_body_makes_a_taller_card_and_an_empty_one_leaves_its_block_out() {
        let tokens = Tokens::built_in();
        let short = layout(&view(1280, "Short."), &tokens);
        let long = layout(
            &view(1280, &"a long sentence about the screen ".repeat(6)),
            &tokens,
        );
        assert!(
            long.size.1 > short.size.1,
            "{} against {}",
            long.size.1,
            short.size.1
        );
        assert!(
            long.text[1].lines.len() <= BODY_LINES,
            "the body takes at most its lines"
        );
        let none = layout(&view(1280, ""), &tokens);
        assert_eq!(none.text.len(), 1, "no body, no block");
    }

    #[test]
    fn a_compact_screen_gives_touch_buttons_and_a_card_within_its_gutters() {
        let tokens = Tokens::built_in();
        let touch = layout(&view(360, "Picture."), &tokens);
        assert_eq!(touch.allow.h, BUTTON_TOUCH, "44 px where a finger lands");
        assert_eq!(touch.deny.h, BUTTON_TOUCH);
        assert_eq!(touch.size.0 as f32, 360.0 - 2.0 * EDGE);
        assert!(touch.allow.right() <= touch.size.0 as f32 - PAD + 0.5);
    }

    #[test]
    fn hit_finds_each_button_and_nothing_else() {
        let tokens = Tokens::built_in();
        let mut v = view(1280, "Picture.");
        v.room = 30;
        let l = layout(&v, &tokens);
        let at = |r: Rect| (r.x + r.w / 2.0 + 30.0, r.y + r.h / 2.0 + 30.0);
        let (dx, dy) = at(l.deny);
        let (ax, ay) = at(l.allow);
        assert_eq!(hit(&l, dx, dy), Some(Button::Deny));
        assert_eq!(hit(&l, ax, ay), Some(Button::Allow));
        assert_eq!(hit(&l, 2.0, 2.0), None, "the room round the card");
        assert_eq!(hit(&l, 40.0, 40.0), None, "the title");
        assert_eq!(
            hit(&l, 30.0 + 5.0, 30.0 + l.size.1 as f32 + 5.0),
            None,
            "below the card"
        );
    }

    #[test]
    fn places_is_one_line_in_the_surfaces_coordinates() {
        let tokens = Tokens::built_in();
        let l = layout(&view(1280, ""), &tokens);
        let text = places(&l);
        let expected = format!(
            "card 400x{}, deny {:.0}+{:.0}+{:.0}x{:.0}, allow {:.0}+{:.0}+{:.0}x{:.0}",
            l.size.1,
            l.deny.x,
            l.deny.y,
            l.deny.w,
            l.deny.h,
            l.allow.x,
            l.allow.y,
            l.allow.w,
            l.allow.h,
        );
        assert_eq!(text, expected);
        assert_eq!(l.deny.x, 180.0, "Deny sits one gap left of Allow");
        let shifted = {
            let mut v = view(1280, "");
            v.room = 30;
            places(&layout(&v, &tokens))
        };
        assert!(shifted.starts_with("card 400x"));
        assert!(
            shifted.contains("deny 210+"),
            "the room moves it: {shifted}"
        );
    }

    #[test]
    fn a_screen_reader_hears_the_title_its_text_and_the_two_buttons() {
        let tokens = Tokens::built_in();
        let mut v = view(1280, "Picture.");
        v.subtitle = "Shot wants a picture".into();
        let l = layout(&v, &tokens);
        let heard = nodes(&v, &l);
        assert_eq!(heard.len(), 4);
        assert_eq!(heard[0].role, Role::Dialog);
        assert_eq!(heard[0].label, "Allow Shot to take screenshots?");
        assert_eq!(heard[1].role, Role::Label);
        assert_eq!(heard[1].label, "Shot wants a picture Picture.");
        assert_eq!(heard[2].label, "Deny");
        assert_eq!(heard[3].label, "Allow");
        assert_eq!(focus_item(&v, &l), 2, "Deny holds the focus first");
        v.focus = Button::Allow;
        assert_eq!(focus_item(&v, &l), 3);
    }

    #[test]
    fn the_title_is_the_portals_or_a_default_naming_the_app() {
        let ask = |app: &str, title: &str| Ask {
            app: app.into(),
            title: title.into(),
            subtitle: String::new(),
            body: String::new(),
            deny: "Deny".into(),
            grant: "Allow".into(),
            answer: crate::shell_bus::answer().0,
        };
        assert_eq!(
            ask("Shot", "").shown_title(),
            "Allow Shot to take screenshots?"
        );
        assert_eq!(
            ask("Shot", "Let Shot look?").shown_title(),
            "Let Shot look?"
        );
    }

    #[test]
    fn other_toggles_between_the_two_buttons() {
        assert_eq!(other(Button::Deny), Button::Allow);
        assert_eq!(other(Button::Allow), Button::Deny);
    }

    #[test]
    fn the_card_and_the_buttons_draw_without_fonts() {
        let tokens = Tokens::built_in();
        let v = view(1280, "Picture.");
        let l = layout(&v, &tokens);
        let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
        paint(&mut pixmap, &v, &tokens, None, 2.0);
        let alpha = |x: f32, y: f32| {
            pixmap
                .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                .unwrap()
                .alpha()
        };
        assert_eq!(alpha(0.0, 0.0), 0, "a round corner");
        assert_ne!(alpha(200.0, 4.0), 0, "the card");
        let allow = l.allow;
        assert_eq!(
            alpha(allow.x + allow.w / 2.0, allow.y + allow.h / 2.0),
            255,
            "the accent button is filled"
        );
    }

    #[test]
    fn it_draws_light_and_dark_and_wide_and_compact_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            let tokens = Tokens::built_in_scheme(scheme);
            for (width, shape) in [(1280, "wide"), (360, "compact")] {
                let mut v = view(width, "The app takes a picture of this whole screen.");
                v.subtitle = "Shot wants to share a picture of the screen".into();
                v.room = 0;
                let l = layout(&v, &tokens);
                let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
                paint(&mut pixmap, &v, &tokens, Some(&mut text), 2.0);
                assert_ne!(pixmap.data().iter().filter(|b| **b != 0).count(), 0);
                if let Some(dir) = std::env::var_os("EDEL_ACCESS_PNG") {
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    // Over the screen's colour, so the card's edge shows.
                    let room = 24u32 * 2;
                    let mut screen =
                        Pixmap::new(pixmap.width() + 2 * room, pixmap.height() + 2 * room).unwrap();
                    screen.fill(Color::from_rgba8(
                        (tokens.background.r * 255.0) as u8,
                        (tokens.background.g * 255.0) as u8,
                        (tokens.background.b * 255.0) as u8,
                        255,
                    ));
                    screen.draw_pixmap(
                        room as i32,
                        room as i32,
                        pixmap.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        None,
                    );
                    screen
                        .save_png(dir.join(format!("access-{mode}-{shape}.png")))
                        .unwrap();
                }
            }
        }
    }
}
