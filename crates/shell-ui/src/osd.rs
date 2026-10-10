//! The volume or brightness pop-up (M5.9c, the fifth round's "Volume and
//! brightness pop-up"): what a volume or brightness key shows. It is the
//! shelf's slider and its round chevron button alone, each its own card
//! with nothing behind, over the status area along the panel, and gone
//! 1.5 s after the last key. [`layout`] is the one function that sizes it
//! and places its parts; the slider is drawn by quick settings' shelf
//! (`quick::slider`), so the two look the same. Plain data and drawing,
//! tested without a display; `osd_card.rs` owns the surface, which exists
//! only while the pop-up shows.

use accesskit::Role;
use edel::i18n::tr;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::a11y::Item;
use crate::paint::{Text, fill, outline};
use crate::popup::{self, Card, Rect, icon_in, raised, veil};
use crate::quick;

/// The slider's length, logical pixels.
pub const TRACK: f32 = 240.0;
/// Between the slider and its button, logical pixels.
pub const GAP: f32 = 10.0;
/// How long the pop-up stays after the last key, milliseconds.
pub const MS: u64 = 1500;

/// What the pop-up sets: the sound's volume or the screen's brightness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Volume,
    Brightness,
}

/// What the pop-up shows and where the pointer is.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub kind: Kind,
    pub percent: u32,
    pub muted: bool,
    /// A screen narrower than quick settings' Compact width: the bar is
    /// 44 px high then, where a finger lands.
    pub compact: bool,
    pub hover: Option<Part>,
}

/// The parts a press can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Track,
    More,
}

/// Where the pop-up's parts lie: its size and each part's rectangle,
/// logical pixels from its top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub size: (u32, u32),
    pub track: Rect,
    pub more: Rect,
}

/// The pop-up's size and where its parts lie: the slider, then the button
/// `GAP` further on, both as high as quick settings' bar.
pub fn layout(view: &View) -> Layout {
    let bar = quick::metrics(view.compact).bar;
    Layout {
        size: ((TRACK + GAP + bar).ceil() as u32, bar.ceil() as u32),
        track: Rect::new(0.0, 0.0, TRACK, bar),
        more: Rect::new(TRACK + GAP, 0.0, bar, bar),
    }
}

/// The two cards the pop-up holds, each with corners half its height
/// round, so each is a pill and a round button.
pub fn cards(l: &Layout) -> Vec<Card> {
    [l.track, l.more]
        .into_iter()
        .map(|rect| Card {
            rect,
            radius: rect.h / 2.0,
        })
        .collect()
}

/// The part under `x`, `y` (logical pixels from the pop-up's corner), if
/// any: nothing in the gap between the two.
pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Part> {
    if l.track.contains(x, y) {
        Some(Part::Track)
    } else if l.more.contains(x, y) {
        Some(Part::More)
    } else {
        None
    }
}

/// The percent at `x` along the slider, 0 to 100, as quick settings'
/// slider reads it.
pub fn level_at(l: &Layout, x: f32) -> u32 {
    quick::percent_at(Some(l.track), x)
}

/// Where the parts lie, as one log line CI reads:
/// `card WxH, track X+Y+WxH, more X+Y+WxH`, logical pixels.
pub fn places(l: &Layout) -> String {
    let at = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    format!(
        "card {}x{}, track {}, more {}",
        l.size.0,
        l.size.1,
        at(l.track),
        at(l.more)
    )
}

/// The quick settings page the pop-up's button opens: Sound for the
/// volume, Displays for the brightness.
pub fn page(kind: Kind) -> &'static str {
    match kind {
        Kind::Volume => quick::SOUND_PAGE,
        Kind::Brightness => quick::DISPLAYS_PAGE,
    }
}

/// What a screen reader finds: the slider with its value and the button
/// that opens its page.
pub fn nodes(view: &View, l: &Layout) -> Vec<Item> {
    let at = |r: Rect| {
        accesskit::Rect::new(
            f64::from(r.x),
            f64::from(r.y),
            f64::from(r.right()),
            f64::from(r.y + r.h),
        )
    };
    let (slider, button) = match view.kind {
        Kind::Volume => (tr("Volume"), tr("Sound settings")),
        Kind::Brightness => (tr("Brightness"), tr("Display settings")),
    };
    let item = |role, label: &str, bounds, value| Item {
        role,
        label: label.to_string(),
        bounds,
        children: Vec::new(),
        toggled: None,
        value,
    };
    vec![
        item(
            Role::Slider,
            slider,
            at(l.track),
            Some((f64::from(view.percent), 0.0, 100.0)),
        ),
        item(Role::Button, button, at(l.more), None),
    ]
}

/// Draws `view` at `s` into `pixmap`, which is the pop-up's size times it:
/// the two cards, the slider on the first with the level, and the button on
/// the second with its chevron. Without `text`, everything but the words.
pub fn paint(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: Option<&mut Text>, s: f32) {
    let l = layout(view);
    popup::cards(pixmap, tokens, s, &cards(&l));
    let icon = match view.kind {
        Kind::Volume => quick::volume_icon(view.percent, view.muted),
        Kind::Brightness => "brightness",
    };
    let bar = quick::Bar {
        percent: view.percent,
        muted: view.muted,
        icon,
        focused: false,
    };
    quick::slider(pixmap, l.track, bar, text, tokens, s);
    // The round button: raised, edged, lit a little under the pointer.
    let (x, y, w, h) = l.more.device(s);
    let hair = (0.5 * s).max(1.0);
    fill(pixmap, x, y, w, h, w / 2.0, raised(tokens));
    outline(pixmap, (x, y, w, h), w / 2.0, hair, tokens.line);
    if view.hover == Some(Part::More) {
        fill(pixmap, x, y, w, h, w / 2.0, veil(tokens, 0.05));
    }
    icon_in(pixmap, "chevron-right", 14.0, l.more, s, tokens.panel_text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Scheme;
    use tiny_skia::{Color, PixmapPaint, Transform};

    fn view(kind: Kind, percent: u32, compact: bool) -> View {
        View {
            kind,
            percent,
            muted: false,
            compact,
            hover: None,
        }
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn the_layout_is_the_slider_and_its_button_as_high_as_the_bar() {
        let desktop = layout(&view(Kind::Volume, 55, false));
        assert_eq!(desktop.size, (286, 36));
        assert_eq!(desktop.track, Rect::new(0.0, 0.0, 240.0, 36.0));
        assert_eq!(desktop.more, Rect::new(250.0, 0.0, 36.0, 36.0));
        let compact = layout(&view(Kind::Brightness, 70, true));
        assert_eq!(compact.size, (294, 44));
        assert_eq!(compact.more.h, 44.0);
        // Two cards, each a pill or a round button.
        let cards = cards(&compact);
        assert_eq!(cards.len(), 2);
        assert!(cards.iter().all(|c| c.radius == 22.0));
    }

    #[test]
    fn hit_finds_each_part_and_nothing_in_the_gap() {
        let l = layout(&view(Kind::Volume, 55, false));
        assert_eq!(hit(&l, 0.0, 0.0), Some(Part::Track));
        assert_eq!(hit(&l, 239.9, 35.0), Some(Part::Track));
        assert_eq!(hit(&l, 250.0, 0.0), Some(Part::More));
        assert_eq!(hit(&l, 285.0, 35.0), Some(Part::More));
        assert_eq!(hit(&l, 245.0, 18.0), None, "the gap");
        assert_eq!(hit(&l, 100.0, 40.0), None, "below the bar");
    }

    #[test]
    fn a_press_on_the_track_reads_the_level_from_its_left_end() {
        let l = layout(&view(Kind::Volume, 55, false));
        assert_eq!(level_at(&l, 0.0), 0);
        assert_eq!(level_at(&l, 120.0), 50);
        assert_eq!(level_at(&l, 240.0), 100);
        assert_eq!(level_at(&l, 500.0), 100, "past the end is the end");
    }

    #[test]
    fn its_places_are_one_line_for_ci() {
        let l = layout(&view(Kind::Volume, 55, false));
        assert_eq!(
            places(&l),
            "card 286x36, track 0+0+240x36, more 250+0+36x36"
        );
    }

    #[test]
    fn each_kind_opens_its_own_page() {
        assert_eq!(page(Kind::Volume), "sound");
        assert_eq!(page(Kind::Brightness), "displays");
    }

    #[test]
    fn a_screen_reader_hears_a_slider_with_its_value_and_a_button() {
        let v = view(Kind::Volume, 55, false);
        let heard = nodes(&v, &layout(&v));
        assert_eq!(heard.len(), 2);
        assert_eq!(heard[0].role, Role::Slider);
        assert_eq!(heard[0].label, "Volume");
        assert_eq!(heard[0].value, Some((55.0, 0.0, 100.0)));
        assert_eq!(heard[1].role, Role::Button);
        assert_eq!(heard[1].label, "Sound settings");
        let b = view(Kind::Brightness, 70, false);
        let heard = nodes(&b, &layout(&b));
        assert_eq!(heard[0].label, "Brightness");
        assert_eq!(heard[1].label, "Display settings");
    }

    #[test]
    fn the_gap_is_clear_and_the_button_drawn_without_fonts() {
        let tokens = Tokens::built_in();
        for kind in [Kind::Volume, Kind::Brightness] {
            let v = view(kind, 55, false);
            let l = layout(&v);
            let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
            paint(&mut pixmap, &v, &tokens, None, 2.0);
            let alpha = |x: f32, y: f32| {
                pixmap
                    .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                    .unwrap()
                    .alpha()
            };
            assert_ne!(alpha(120.0, 18.0), 0, "the track is drawn");
            assert_ne!(alpha(268.0, 18.0), 0, "the button is drawn");
            assert_eq!(alpha(245.0, 18.0), 0, "the gap is clear");
        }
    }

    #[test]
    fn it_draws_light_and_dark_with_fonts_and_writes_pngs() {
        let Some(mut text) = fonts() else {
            return; // no fonts on this machine
        };
        for (scheme, mode) in [(Scheme::Light, "light"), (Scheme::Dark, "dark")] {
            let tokens = Tokens::built_in_scheme(scheme);
            for (kind, percent, name) in [
                (Kind::Volume, 55, "volume"),
                (Kind::Brightness, 70, "brightness"),
            ] {
                let v = view(kind, percent, false);
                let l = layout(&v);
                let mut pixmap = Pixmap::new(l.size.0 * 2, l.size.1 * 2).unwrap();
                paint(&mut pixmap, &v, &tokens, Some(&mut text), 2.0);
                let alpha = |x: f32, y: f32| {
                    pixmap
                        .pixel((x * 2.0) as u32, (y * 2.0) as u32)
                        .unwrap()
                        .alpha()
                };
                assert_ne!(alpha(268.0, 18.0), 0, "{name} {mode}: the button");
                assert_eq!(alpha(245.0, 18.0), 0, "{name} {mode}: the gap");
                if let Some(dir) = std::env::var_os("EDEL_OSD_PNG") {
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    // Over the screen's colour, so the clear gap shows.
                    let room = 24u32 * 2;
                    let mut screen =
                        Pixmap::new(pixmap.width() + 2 * room, pixmap.height() + 2 * room).unwrap();
                    screen.fill(
                        Color::from_rgba(
                            tokens.background.r,
                            tokens.background.g,
                            tokens.background.b,
                            1.0,
                        )
                        .unwrap(),
                    );
                    screen.draw_pixmap(
                        room as i32,
                        room as i32,
                        pixmap.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                    screen
                        .save_png(dir.join(format!("osd-{name}-{mode}.png")))
                        .unwrap();
                }
            }
        }
    }
}
