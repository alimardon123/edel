//! The in-use card (M6.14b): what the card shows and where each part lies,
//! plain data and drawing, tested without a display. The card lists each app
//! using the camera, the microphone, the screen or the place (M6.14c), what
//! it uses, and two buttons, Stop and Settings (Settings only for an app
//! with a page in Settings). Stop is there only for a PipeWire use: a place
//! has no client, so its row has Settings alone, where Location is set to
//! Never. `in_use_card.rs` holds its surface and the keyboard.
//!
//! The card is `popup`'s card, its rows, text sizes, radii and hover look
//! as the app windows menu's (`app_windows.rs`), so one change to the tokens
//! reaches both. `layout` is the one function that places everything, and
//! `places` is the log line CI reads.

use accesskit::{Rect as AkRect, Role};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use edel::i18n::{tr, trf};
use edel::tokens::Tokens;

use crate::a11y::Item;
use crate::in_use::{InUse, kinds_text};
use crate::paint::{self, Text, fill, lit, mix};
use crate::popup::{self, INSET, PAD, Rect};

/// The card's least width in logical pixels, as the app windows menu's.
pub const WIDTH: u32 = 280;
/// The most apps the card lists: more are left out.
pub const MOST: usize = 8;
/// The room between an app's two buttons, and between the text and the
/// buttons and the icon, logical pixels.
const GAP_BUTTON: f32 = 6.0;
const GAP: f32 = 10.0;
/// The widest an app's name and what it uses may be before they are cut
/// short with an ellipsis, logical pixels.
const TEXT_MOST: f32 = 320.0;

/// Which button a part of the card is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Stop the app's use, of the row at this index.
    Stop(usize),
    /// Open the app's page in Settings, of the row at this index.
    Settings(usize),
}

impl Button {
    /// The row the button belongs to.
    pub fn row(self) -> usize {
        match self {
            Button::Stop(row) | Button::Settings(row) => row,
        }
    }
}

/// One app's use: what the card shows of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub used: InUse,
    /// Whether the app has a page in Settings, so it has a Settings button.
    pub page: bool,
}

/// What the card shows: each app, and the button the keyboard or the
/// pointer holds, as its place in `Layout::buttons`.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub rows: Vec<Row>,
    pub focus: Option<usize>,
}

/// What a row holds, logical pixels from the card's top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct RowBox {
    pub top: f32,
    pub height: f32,
    /// The app's icon, a square.
    pub icon: Rect,
    /// Its name and what it uses, the lines' box.
    pub text: Rect,
}

/// Where everything lies: the card's size and each row and button.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub rows: Vec<RowBox>,
    pub buttons: Vec<(Button, Rect)>,
}

/// What the card shows for `uses` (at most `MOST`), with `pages` the
/// desktop file ids of the apps that have a page in Settings. An app with a
/// Flatpak id always has one.
pub fn view(uses: &[InUse], pages: &[String], focus: Option<usize>) -> View {
    let rows = uses
        .iter()
        .take(MOST)
        .map(|used| Row {
            used: used.clone(),
            page: used.flatpak || pages.contains(&used.app),
        })
        .collect();
    View { rows, focus }
}

/// Where everything of `view` lies. `measure` gives a text's width at a size
/// in logical pixels: the card is at least `WIDTH` and as wide as its widest
/// row, its buttons at the row's end, Stop then Settings.
pub fn layout(view: &View, tokens: &Tokens, measure: &mut dyn FnMut(&str, f32) -> f32) -> Layout {
    let size = tokens.panel_text_size as f32;
    let small = tokens.panel_text_small_size as f32;
    let head = tokens.row as f32;
    let row_h = (tokens.row * 3 / 2) as f32;
    let icon = tokens.panel_icon as f32;
    let control = tokens.panel_control as f32;
    let inset = PAD + INSET;
    let mut width = (WIDTH as f32).max(2.0 * inset + measure(tr("In use now"), size));
    // Each row's text and its buttons, measured.
    let mut measured: Vec<(f32, Vec<(Button, f32)>)> = Vec::new();
    for (r, row) in view.rows.iter().enumerate() {
        let uses = kinds_text(&row.used.kinds);
        let text_w = measure(&row.used.name, size)
            .max(measure(&uses, small))
            .min(TEXT_MOST);
        // A place (M6.14c) holds no PipeWire client to destroy, so its row has
        // no Stop: only its Settings button, where Location is set to Never.
        let mut buttons = Vec::new();
        if row.used.client.is_some() {
            buttons.push((Button::Stop(r), measure(tr("Stop"), size) + 2.0 * INSET));
        }
        if row.page {
            let words = measure(tr("Settings"), size);
            buttons.push((Button::Settings(r), words + 2.0 * INSET));
        }
        let buttons_w = buttons_width(&buttons);
        width = width.max(2.0 * inset + icon + GAP + text_w + GAP + buttons_w);
        measured.push((text_w, buttons));
    }
    let width = width.ceil();
    let mut rows = Vec::new();
    let mut placed = Vec::new();
    for (r, (text_w, buttons)) in measured.iter().enumerate() {
        let top = PAD + head + r as f32 * row_h;
        rows.push(RowBox {
            top,
            height: row_h,
            icon: Rect::new(inset, top + (row_h - icon) / 2.0, icon, icon),
            text: Rect::new(inset + icon + GAP, top, *text_w, row_h),
        });
        let mut x = width - inset - buttons_width(buttons);
        for &(button, w) in buttons {
            placed.push((
                button,
                Rect::new(x, top + (row_h - control) / 2.0, w, control),
            ));
            x += w + GAP_BUTTON;
        }
    }
    let height = PAD + head + rows.len() as f32 * row_h + PAD;
    Layout {
        size: (width as u32, height.ceil() as u32),
        rows,
        buttons: placed,
    }
}

/// The width of a row's buttons with the gaps between them.
fn buttons_width(buttons: &[(Button, f32)]) -> f32 {
    let words: f32 = buttons.iter().map(|(_, w)| w).sum();
    words + GAP_BUTTON * buttons.len().saturating_sub(1) as f32
}

/// The words of a button, in the app's name.
fn words(button: Button, view: &View) -> String {
    let name = view
        .rows
        .get(button.row())
        .map_or("", |row| row.used.name.as_str());
    match button {
        Button::Stop(_) => trf("Stop {app}", &[("app", name)]),
        Button::Settings(_) => trf("Settings for {app}", &[("app", name)]),
    }
}

/// Draws `view` at `scale` into `pixmap`, its size times `scale`: the card,
/// the heading, each app's icon (its app id's icon, else the generic one),
/// its name and what it uses, and the buttons, the held one lit and ringed.
pub fn paint(
    pixmap: &mut Pixmap,
    view: &View,
    layout: &Layout,
    tokens: &Tokens,
    mut text: Option<&mut Text>,
    mut icons: Option<&mut edel::app_icons::Icons>,
    scale: f32,
) {
    let s = scale;
    popup::card(pixmap, tokens, s);
    let size = tokens.panel_text_size as f32;
    let small = tokens.panel_text_small_size as f32;
    if let Some(text) = text.as_deref_mut() {
        let mut line = text.line(tr("In use now"), size * s);
        let y = popup::middle(PAD, tokens.row as f32, size * s, s);
        text.draw(pixmap, &mut line, (PAD + INSET) * s, y, popup::dim(tokens));
    }
    for (row, boxed) in view.rows.iter().zip(&layout.rows) {
        let px = (tokens.panel_icon as f32 * s).round();
        let (ix, iy, _, _) = boxed.icon.device(s);
        let drawn = icons
            .as_deref_mut()
            .and_then(|icons| icons.get(&row.used.app, px as u32))
            .map(|picture| {
                pixmap.draw_pixmap(
                    ix as i32,
                    iy as i32,
                    picture.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                )
            })
            .is_some();
        if !drawn {
            let generic = mix(tokens.panel_text, tokens.panel, 0.35);
            paint::icon(pixmap, "app-generic", px, ix, iy, generic);
        }
        if let Some(text) = text.as_deref_mut() {
            let (name_h, small_h) = (size * 1.25, small * 1.25);
            let top = boxed.text.y + (boxed.height - (name_h + small_h)) / 2.0;
            let room = boxed.text.w * s;
            let mut name = text.fit(&row.used.name, size * s, room);
            text.draw(
                pixmap,
                &mut name,
                boxed.text.x * s,
                top * s,
                tokens.panel_text,
            );
            let uses = kinds_text(&row.used.kinds);
            let mut what = text.fit(&uses, small * s, room);
            text.draw(
                pixmap,
                &mut what,
                boxed.text.x * s,
                (top + name_h) * s,
                popup::dim(tokens),
            );
        }
    }
    for (i, (button, rect)) in layout.buttons.iter().enumerate() {
        let held = view.focus == Some(i);
        let (x, y, w, h) = rect.device(s);
        let face = if held {
            lit(tokens)
        } else {
            popup::raised(tokens)
        };
        fill(pixmap, x, y, w, h, tokens.radius_control as f32 * s, face);
        if let Some(text) = text.as_deref_mut() {
            let word = match button {
                Button::Stop(_) => tr("Stop"),
                Button::Settings(_) => tr("Settings"),
            };
            let mut line = text.line(word, size * s);
            let tx = x + ((w - line.width) / 2.0).round();
            let ty = popup::middle(rect.y, rect.h, size * s, s);
            text.draw(pixmap, &mut line, tx, ty, tokens.panel_text);
        }
        if held {
            popup::focus_ring(pixmap, *rect, tokens.radius_control as f32, s, tokens);
        }
    }
}

/// What a screen reader reads of the card (M6.9, as `app_windows.rs`): each
/// app as a list item, its name and what it uses, then its buttons, each a
/// button named for the app, flat, in that order. The item of the held
/// button is the focus.
pub fn nodes(view: &View, layout: &Layout) -> (Vec<Item>, Option<usize>) {
    let held = view
        .focus
        .and_then(|f| layout.buttons.get(f))
        .map(|(button, _)| *button);
    let width = f64::from(layout.size.0);
    let mut items = Vec::new();
    let mut focus = None;
    for (r, (row, boxed)) in view.rows.iter().zip(&layout.rows).enumerate() {
        let top = f64::from(boxed.top);
        let bottom = top + f64::from(boxed.height);
        items.push(Item {
            role: Role::ListItem,
            label: trf(
                "{name}, {uses}",
                &[
                    ("name", &row.used.name),
                    ("uses", &kinds_text(&row.used.kinds)),
                ],
            ),
            bounds: AkRect::new(f64::from(PAD), top, width - f64::from(PAD), bottom),
            children: vec![],
            toggled: None,
            value: None,
            value_text: None,
            disabled: false,
        });
        for (button, rect) in layout.buttons.iter().filter(|(b, _)| b.row() == r) {
            if held == Some(*button) {
                focus = Some(items.len());
            }
            let (x0, y0) = (f64::from(rect.x), f64::from(rect.y));
            items.push(Item {
                role: Role::Button,
                label: words(*button, view),
                bounds: AkRect::new(x0, y0, x0 + f64::from(rect.w), y0 + f64::from(rect.h)),
                children: vec![],
                toggled: None,
                value: None,
                value_text: None,
                disabled: false,
            });
        }
    }
    (items, focus)
}

/// The log line CI reads: the card's size, `card WxH`, then each button's
/// place in logical pixels from the card's top left corner, as
/// `stop:APP X+Y+WxH` or `settings:APP X+Y+WxH`, APP being the app's id.
/// The card lies centred in its surface, whose place on the screen the
/// compositor's state file gives.
pub fn places(view: &View, layout: &Layout) -> String {
    let (w, h) = layout.size;
    let card = std::iter::once(format!("card {w}x{h}"));
    let buttons = layout.buttons.iter().map(|(button, rect)| {
        let (word, row) = match button {
            Button::Stop(r) => ("stop", r),
            Button::Settings(r) => ("settings", r),
        };
        let app = view.rows.get(*row).map_or("", |r| r.used.app.as_str());
        format!(
            "{word}:{app} {}+{}+{}x{}",
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.w.round() as i32,
            rect.h.round() as i32
        )
    });
    card.chain(buttons).collect::<Vec<_>>().join(", ")
}

/// The button under the point (`x`, `y`, card-local logical pixels), with
/// its place in `Layout::buttons`.
pub fn button_at(layout: &Layout, x: f32, y: f32) -> Option<(usize, Button)> {
    layout
        .buttons
        .iter()
        .enumerate()
        .find(|(_, (_, rect))| rect.contains(x, y))
        .map(|(i, (button, _))| (i, *button))
}

/// The button the keyboard holds after moving `by` from `focus`, of `count`
/// buttons: from none, a step forward takes the first and a step back the
/// last. It does not wrap.
pub fn moved(focus: Option<usize>, count: usize, by: i32) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(match focus {
        None if by >= 0 => 0,
        None => last,
        Some(f) => (f as i64 + i64::from(by)).clamp(0, last as i64) as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::app_use::Kind;

    /// A stand-in for the font: each letter a little more than half an em.
    fn measure(text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.55
    }

    fn used(app: &str, name: &str, flatpak: bool, kinds: &[Kind]) -> InUse {
        InUse {
            client: Some(60),
            app: app.into(),
            name: name.into(),
            flatpak,
            kinds: kinds.to_vec(),
        }
    }

    fn two_apps() -> Vec<InUse> {
        vec![
            used("org.gnome.Snapshot", "Snapshot", true, &[Kind::Camera]),
            used("ffmpeg", "ffmpeg", false, &[Kind::Microphone, Kind::Screen]),
        ]
    }

    #[test]
    fn a_row_per_app_and_settings_only_for_an_app_with_a_page() {
        let tokens = Tokens::built_in();
        let view = view(&two_apps(), &["foot".to_string()], None);
        assert_eq!(
            view.rows.iter().map(|r| r.page).collect::<Vec<_>>(),
            [true, false]
        );
        let layout = layout(&view, &tokens, &mut measure);
        let buttons: Vec<Button> = layout.buttons.iter().map(|(b, _)| *b).collect();
        assert_eq!(
            buttons,
            [Button::Stop(0), Button::Settings(0), Button::Stop(1)],
            "no Settings for ffmpeg"
        );
    }

    #[test]
    fn a_flatpak_app_has_a_page_and_an_unlisted_host_app_has_none() {
        let listed = view(&two_apps(), &[], None);
        assert!(listed.rows[0].page, "a Flatpak app has its page");
        assert!(!listed.rows[1].page, "a host app not in Settings has none");
        let host = view(
            &[used("foot", "foot", false, &[Kind::Camera])],
            &["foot".to_string()],
            None,
        );
        assert!(host.rows[0].page, "a host app listed in Settings has one");
    }

    /// A place's row (M6.14c) has no Stop, only Settings, and the keys and
    /// the reader's list see only the button there is.
    #[test]
    fn a_place_row_has_settings_and_no_stop() {
        let tokens = Tokens::built_in();
        let mut maps = used("org.gnome.Maps", "Maps", true, &[Kind::Location]);
        maps.client = None;
        let v = view(&[maps], &[], None);
        let layout = layout(&v, &tokens, &mut measure);
        let buttons: Vec<Button> = layout.buttons.iter().map(|(b, _)| *b).collect();
        assert_eq!(buttons, [Button::Settings(0)]);
        let (items, _) = nodes(&v, &layout);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Maps, Location", "Settings for Maps"]);
        let text = places(&v, &layout);
        assert!(!text.contains("stop:"), "{text}");
        assert!(text.contains("settings:org.gnome.Maps "), "{text}");
        assert_eq!(moved(None, layout.buttons.len(), 1), Some(0));
        assert_eq!(moved(Some(0), layout.buttons.len(), 1), Some(0));
        let (_, rect) = layout.buttons[0];
        let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        assert_eq!(button_at(&layout, cx, cy), Some((0, Button::Settings(0))));
    }

    #[test]
    fn the_card_is_at_least_as_wide_as_280_and_as_its_widest_row() {
        let tokens = Tokens::built_in();
        let short = view(&two_apps(), &[], None);
        assert_eq!(layout(&short, &tokens, &mut measure).size.0, WIDTH);
        let long = vec![used(
            "org.example.App",
            "A very long application name that goes on and on and on",
            true,
            &[Kind::Camera, Kind::Microphone, Kind::Screen],
        )];
        let wide = layout(&view(&long, &[], None), &tokens, &mut measure);
        assert!(wide.size.0 > WIDTH, "{:?}", wide.size);
        let (x, w) = (wide.buttons[0].1.x, wide.buttons[0].1.w);
        assert!(
            x + w <= wide.size.0 as f32,
            "the buttons lie inside the card"
        );
    }

    #[test]
    fn the_height_is_the_heading_and_a_row_per_app() {
        let tokens = Tokens::built_in();
        let layout = layout(&view(&two_apps(), &[], None), &tokens, &mut measure);
        let row = (tokens.row * 3 / 2) as f32;
        let expect = (2.0 * PAD + tokens.row as f32 + 2.0 * row) as u32;
        assert_eq!(layout.size.1, expect);
    }

    #[test]
    fn places_names_each_button_with_its_app() {
        let tokens = Tokens::built_in();
        let v = view(&two_apps(), &["foot".to_string()], None);
        let layout = layout(&v, &tokens, &mut measure);
        let text = places(&v, &layout);
        let (w, h) = layout.size;
        assert!(text.starts_with(&format!("card {w}x{h}, ")), "{text}");
        assert!(text.contains("stop:org.gnome.Snapshot "), "{text}");
        assert!(text.contains("settings:org.gnome.Snapshot "), "{text}");
        assert!(text.contains("stop:ffmpeg "), "{text}");
        assert!(!text.contains("settings:ffmpeg"), "{text}");
    }

    #[test]
    fn nodes_list_each_app_then_its_buttons_and_focus_the_held_one() {
        let tokens = Tokens::built_in();
        let v = view(&two_apps(), &["foot".to_string()], Some(1));
        let layout = layout(&v, &tokens, &mut measure);
        let (items, focus) = nodes(&v, &layout);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Snapshot, Camera",
                "Stop Snapshot",
                "Settings for Snapshot",
                "ffmpeg, Microphone and screen",
                "Stop ffmpeg",
            ]
        );
        assert_eq!(
            focus,
            Some(2),
            "the held button is Settings of the first app"
        );
        assert_eq!(items[0].role, Role::ListItem);
        assert_eq!(items[1].role, Role::Button);
    }

    #[test]
    fn a_click_finds_the_button_under_it() {
        let tokens = Tokens::built_in();
        let v = view(&two_apps(), &["foot".to_string()], None);
        let layout = layout(&v, &tokens, &mut measure);
        let (_, stop) = layout.buttons[0];
        let (cx, cy) = (stop.x + stop.w / 2.0, stop.y + stop.h / 2.0);
        assert_eq!(button_at(&layout, cx, cy), Some((0, Button::Stop(0))));
        assert_eq!(button_at(&layout, 0.5, 0.5), None);
    }

    #[test]
    fn keys_step_through_the_buttons_without_wrapping() {
        assert_eq!(moved(None, 3, 1), Some(0));
        assert_eq!(moved(None, 3, -1), Some(2));
        assert_eq!(moved(Some(2), 3, 1), Some(2));
        assert_eq!(moved(Some(0), 3, -1), Some(0));
        assert_eq!(moved(Some(1), 3, 1), Some(2));
        assert_eq!(moved(None, 0, 1), None);
    }

    #[test]
    fn the_held_button_is_lit_and_the_others_are_not() {
        let tokens = Tokens::built_in();
        let v = view(&two_apps(), &["foot".to_string()], Some(0));
        let layout = layout(&v, &tokens, &mut measure);
        let (w, h) = layout.size;
        let mut pixmap = Pixmap::new(w, h).unwrap();
        paint(&mut pixmap, &v, &layout, &tokens, None, None, 1.0);
        let at = |rect: Rect| {
            let c = pixmap
                .pixel(
                    (rect.x + rect.w / 4.0) as u32,
                    (rect.y + rect.h / 2.0) as u32,
                )
                .unwrap()
                .demultiply();
            [c.red(), c.green(), c.blue()]
        };
        let held = at(layout.buttons[0].1);
        let other = at(layout.buttons[1].1);
        assert_ne!(held, other, "the held button is lit");
    }
}
