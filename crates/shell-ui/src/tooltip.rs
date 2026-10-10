//! The tray's tooltip (M5.9h, the fifth round's `tray.jpg`): a small dark
//! label over the tray's arrow, "Hidden icons (6)", shown once the pointer
//! has rested on the arrow for [`DELAY_MS`]. It goes when the pointer moves
//! off the arrow, presses, or the tray's grid opens. It is a surface of its
//! own above the panel with no keyboard and no clicks, so the pointer goes
//! on to what lies under it. Any widget can show one later through this
//! module. Plain data and drawing, tested without a display; the shell's
//! side is `impl Shell` below.

use std::time::Duration;

use edel::presets::Edge;
use edel::tokens::Tokens;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity};
use tiny_skia::Pixmap;

use crate::paint::{Face, Text, fill};
use crate::popup::Popup;
use crate::widgets;
use crate::{MARGIN, Shell, TOOLTIP};

/// How long the pointer rests on the arrow before the tooltip shows,
/// milliseconds.
pub const DELAY_MS: u64 = 600;

/// The label's height in logical pixels, its room at each side of the
/// words, and its distance from the panel's edge.
const HEIGHT: u32 = 24;
const PAD: f32 = 8.0;
const GAP: i32 = 6;

/// What the tooltip says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub text: String,
}

/// The open tooltip: its surface and what it shows.
pub struct Tip {
    pub popup: Popup<View>,
    pub view: View,
}

/// The label's size in logical pixels: the words at
/// `size.panel_text_small`, regular (Inter's medium weight spaces words
/// far too widely), and [`PAD`] on each side, [`HEIGHT`]
/// high. Without text it is only the padding.
pub fn size(view: &View, tokens: &Tokens, text: Option<&mut Text>) -> (u32, u32) {
    let size = tokens.panel_text_small_size as f32;
    let words = match text {
        Some(text) => text.line_in(&view.text, size, Face::REGULAR).width,
        None => 0.0,
    };
    ((words + 2.0 * PAD).ceil() as u32, HEIGHT)
}

/// Draws the label into `pixmap` at `s` device pixels per logical pixel: a
/// rounded rectangle of the panel's text colour, the words centred in the
/// panel's colour, so it stands out from the panel as the mockup's dark
/// label does.
pub fn paint(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: Option<&mut Text>, s: f32) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    let radius = tokens.radius_small as f32 * s;
    fill(pixmap, 0.0, 0.0, w, h, radius, tokens.panel_text);
    if let Some(text) = text {
        let size = tokens.panel_text_small_size as f32 * s;
        let mut line = text.line_in(&view.text, size, Face::REGULAR);
        // A line is 1.25 times its size high, as the text is laid out.
        let x = (w - line.width) / 2.0;
        let y = (h - size * 1.25) / 2.0;
        text.draw(pixmap, &mut line, x, y, tokens.panel);
    }
}

impl Shell {
    /// The pointer on panel `i` at `x` logical pixels along it. Over the
    /// tray's arrow the tooltip's timer starts, or keeps going; anywhere
    /// else the tooltip goes (M5.9h).
    pub fn tooltip_hover(&mut self, i: usize, x: f32) {
        let Some((middle, _)) = self.arrow_at(i, x) else {
            return self.hide_tooltip();
        };
        if self.tooltip.is_some() || self.tooltip_timer.is_some() {
            return;
        }
        self.tooltip_timer = self
            .handle
            .insert_source(
                Timer::from_duration(Duration::from_millis(DELAY_MS)),
                move |_, _, shell: &mut Shell| {
                    shell.tooltip_timer = None;
                    shell.show_tooltip(i, middle);
                    TimeoutAction::Drop
                },
            )
            .inspect_err(|e| eprintln!("edel-shell-ui: no timer for the tooltip: {e}"))
            .ok();
    }

    /// The tray's arrow on panel `i` at `x` logical pixels along it: where
    /// its middle lies along the panel, and how many items wait behind it.
    /// None when `x` is not on the arrow.
    fn arrow_at(&self, i: usize, x: f32) -> Option<(f32, usize)> {
        let panel = self.panels.get(i)?;
        let j = panel
            .places
            .iter()
            .position(|(left, w)| (*left..left + w).contains(&x))?;
        if panel.row.widget(j)?.name != widgets::tray::WIDGET.name {
            return None;
        }
        let shown = panel.drawn.as_ref()?.shown.get(j)?;
        let left = panel.places[j].0;
        let hidden = widgets::tray::behind_arrow(shown, x - left)?;
        Some((left + widgets::tray::arrow_middle(), hidden))
    }

    /// Shows the tooltip over the arrow whose middle is `at` logical pixels
    /// along panel `i`: centred on it and kept inside the screen, `GAP` px
    /// from the panel. Nothing is shown while the tray's grid is open.
    fn show_tooltip(&mut self, i: usize, at: f32) {
        if self.tray_grid.is_some() {
            return;
        }
        let Some((_, count)) = self.arrow_at(i, at) else {
            return;
        };
        let view = View {
            text: widgets::tray::hidden_label(count),
        };
        let (w, h) = size(&view, &self.tokens, Some(&mut self.text));
        let Some(panel) = self.panels.get(i) else {
            return;
        };
        let (edge, scale, panel_width) = (panel.edge, panel.scale, panel.width);
        let screen = match self.screen_width() {
            0 => panel_width,
            width => width,
        } as i32;
        let most = (screen - w as i32 - MARGIN).max(MARGIN);
        let left = (at as i32 - w as i32 / 2).clamp(MARGIN, most);
        let Some(mut popup) = Popup::new(self, TOOLTIP, (w, h), (w, h), scale, 0) else {
            return;
        };
        popup.set_no_input(&self.compositor);
        let side = match edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        popup.surface.set_anchor(side | Anchor::LEFT);
        popup.surface.set_margin(GAP, 0, GAP, left);
        // A tooltip never takes the keyboard.
        popup
            .surface
            .set_keyboard_interactivity(KeyboardInteractivity::None);
        popup.surface.commit();
        self.tooltip = Some(Tip { popup, view });
        self.draw_tooltip();
    }

    /// Draws the tooltip if it is not drawn yet.
    pub fn draw_tooltip(&mut self) {
        let Some(tip) = &mut self.tooltip else {
            return;
        };
        let Some(mut pixmap) = tip.popup.canvas(&tip.view) else {
            return;
        };
        let scale = tip.popup.scale();
        paint(
            &mut pixmap,
            &tip.view,
            &self.tokens,
            Some(&mut self.text),
            scale,
        );
        let first = tip
            .popup
            .show(tip.view.clone(), &pixmap, &self.tokens, "tooltip", &self.qh);
        if first {
            eprintln!("edel-shell-ui: tooltip shown, {}", tip.view.text);
        }
    }

    /// Ends the tooltip's wait and hides the tooltip if it shows. Nothing
    /// happens while neither is up.
    pub fn hide_tooltip(&mut self) {
        if let Some(token) = self.tooltip_timer.take() {
            self.handle.remove(token);
        }
        let Some(tip) = self.tooltip.take() else {
            return;
        };
        drop(tip);
        eprintln!("edel-shell-ui: tooltip hidden");
    }

    /// Whether `surface` is the tooltip's.
    pub fn is_tooltip(&self, surface: &wl_surface::WlSurface) -> bool {
        self.tooltip.as_ref().is_some_and(|t| t.popup.is(surface))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::tokens::Tokens;
    use tiny_skia::Pixmap;

    fn view(text: &str) -> View {
        View {
            text: text.to_string(),
        }
    }

    fn fonts() -> Option<Text> {
        let mut text = Text::load(&Tokens::built_in().font);
        (text.line("A", 13.0).width > 0.0).then_some(text)
    }

    #[test]
    fn the_label_grows_with_its_words_and_is_24_high() {
        let tokens = Tokens::built_in();
        let Some(mut text) = fonts() else {
            return;
        };
        let short = size(&view("Hidden"), &tokens, Some(&mut text));
        let long = size(&view("Hidden icons (6)"), &tokens, Some(&mut text));
        assert_eq!(short.1, 24);
        assert_eq!(long.1, 24);
        assert!(long.0 > short.0, "{long:?} is not wider than {short:?}");
        let bare = size(&view("Hidden icons (6)"), &tokens, None);
        assert_eq!(bare, (16, 24), "without text only the padding");
    }

    #[test]
    fn the_words_are_drawn_on_the_text_colour_and_the_corner_is_clear() {
        let tokens = Tokens::built_in();
        let Some(mut text) = fonts() else {
            return;
        };
        let v = view("Hidden icons (6)");
        let (w, h) = size(&v, &tokens, Some(&mut text));
        let mut pixmap = Pixmap::new(w, h).expect("a pixmap");
        paint(&mut pixmap, &v, &tokens, Some(&mut text), 1.0);
        let at = |x: u32, y: u32| pixmap.pixel(x, y).expect("in the pixmap");
        assert_eq!(at(0, 0).alpha(), 0, "the corner is clear, rounded");
        let fill = at(2, h / 2);
        assert_eq!(fill.alpha(), 255, "the label is drawn");
        let want = tokens.panel_text;
        assert!(
            (f32::from(fill.red()) - want.r * 255.0).abs() < 3.0,
            "the label is the panel's text colour"
        );
        let middle = at(w / 2, h / 2);
        assert_ne!(middle.alpha(), 0, "the middle is drawn");
    }

    #[test]
    fn without_fonts_the_label_is_still_drawn_in_its_colour() {
        let tokens = Tokens::built_in();
        let v = view("Hidden icons (6)");
        let (w, h) = size(&v, &tokens, None);
        let mut pixmap = Pixmap::new(w * 2, h * 2).expect("a pixmap");
        paint(&mut pixmap, &v, &tokens, None, 2.0);
        // Device pixels: 4 px in from the left, across the middle.
        let pixel = pixmap.pixel(4, h).expect("in the pixmap");
        assert_eq!(pixel.alpha(), 255, "the fill is drawn at scale 2");
    }
}
