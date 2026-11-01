//! The frame rate overlay (roadmap M8.27): Super+Shift+G
//! (`toggle_frame_rate`) shows or hides, until the next login, a small
//! panel in the upper left corner of the screen a game plays on (in game
//! mode, fullscreen, `playing_on`): its frame rate over the last second,
//! its average frame time and the slowest 1 percent of its last seconds'
//! frames, over a graph of its last frames' times. The numbers are the
//! frames the game's record counts (M7.7d, `Game::live`), the commits of
//! its window that reached the screen, so nothing is put into the game
//! and a game with anti-cheat shows them too. The panel is drawn as the
//! overview's name pills are (`glance_look::name`): the backdrop's deep
//! colour darkened under the glass edge, in the tokens' colours, painted
//! again at most four times a second. With the overlay shown the game's
//! frames are composited, not sent straight to the screen; desktop-test's
//! `frame-rate` case holds them to the smoothness gate's budget.

use std::time::{Duration, Instant};

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::Output;
use smithay::utils::{Physical, Point, Rectangle, Size};
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap, Stroke, Transform};

use edel::draw::{paint_of, rounded};
use edel::tokens::{Colour, Tokens};
use edel_compositor::frame::Text;
use edel_compositor::frame_times::{GRAPHED, Shown};

use crate::glance::{Painted, painted};
use crate::render::{Drawn, Element};
use crate::state::Edel;

/// The panel's size and its place from the screen's corner, logical pixels.
const WIDTH: f32 = 176.0;
const MARGIN: i32 = 16;
const PAD: f32 = 12.0;
/// The frame rate's figure, the line under it and the graph's height.
const FIGURE: f32 = 22.0;
const SMALL: f32 = 11.0;
const GRAPH: f32 = 20.0;
/// How often the panel is painted again at most.
const EVERY: Duration = Duration::from_millis(250);

/// The overlay: whether it shows, and its last picture.
#[derive(Default)]
pub struct Overlay {
    pub on: bool,
    picture: Option<(Painted, Instant)>,
}

impl Edel {
    /// Super+Shift+G: the overlay shown or hidden until the next login.
    pub fn toggle_frame_rate(&mut self) {
        let on = {
            let mut overlay = self.game.overlay.borrow_mut();
            overlay.on = !overlay.on;
            overlay.picture = None;
            overlay.on
        };
        eprintln!(
            "edel-compositor: the frame rate overlay {} (Super+Shift+G){}",
            if on { "on" } else { "off" },
            if on && !self.playing() {
                "; it shows over a fullscreen game in game mode"
            } else {
                ""
            }
        );
        self.dirty = true;
    }

    /// The overlay over the game that plays on `output`, if it shows and
    /// the game has shown two frames there.
    pub fn frame_rate_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        scale: f64,
    ) -> Vec<Drawn> {
        if !self.game.overlay.borrow().on || self.playing_on(output).is_none() {
            return Vec::new();
        }
        let Some(shown) = self.game.live(&output.name()) else {
            return Vec::new();
        };
        let now = Instant::now();
        let stale = self
            .game
            .overlay
            .borrow()
            .picture
            .as_ref()
            .is_none_or(|(p, at)| {
                now.duration_since(*at) >= EVERY && p.shows != shows(&shown, scale)
            });
        if stale {
            let Some(text) = self.text.as_mut() else {
                return Vec::new();
            };
            let first = self.game.overlay.borrow().picture.is_none();
            let Some(pixmap) = paint(&shown, scale, &self.tokens, text) else {
                return Vec::new();
            };
            if first {
                let (rate, times) = shown.lines();
                eprintln!("edel-compositor: the frame rate overlay shows {rate}, {times}");
            }
            self.game.overlay.borrow_mut().picture =
                Some((painted(shows(&shown, scale), &pixmap), now));
        }
        let overlay = self.game.overlay.borrow();
        let Some((picture, _)) = overlay.picture.as_ref() else {
            return Vec::new();
        };
        let at =
            Point::<f64, Physical>::from((f64::from(MARGIN) * scale, f64::from(MARGIN) * scale));
        let pixels = (f64::from(picture.pixels.0), f64::from(picture.pixels.1));
        let size = Size::<f64, Physical>::from(pixels)
            .to_logical(scale)
            .to_i32_round();
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            at,
            &picture.buffer,
            None,
            Some(Rectangle::from_size(pixels.into())),
            Some(size),
            Kind::Unspecified,
        ) {
            Ok(element) => vec![Drawn::Plain(Element::Bar(element))],
            Err(e) => {
                eprintln!("edel-compositor: drawing the frame rate overlay failed: {e}");
                Vec::new()
            }
        }
    }
}

/// What a picture shows: its numbers to a tenth, its graph and its scale.
fn shows(shown: &Shown, scale: f64) -> String {
    let (rate, times) = shown.lines();
    format!("{rate} {times} {:?} {scale}", shown.graph)
}

/// The panel at `scale`: the frame rate's figure and unit, the graph of
/// the last frames' times, and the line of the average and the slowest
/// 1 percent.
fn paint(shown: &Shown, scale: f64, tokens: &Tokens, text: &mut Text) -> Option<Pixmap> {
    let s = scale as f32;
    let (rate, times) = shown.lines();
    let (figure, unit) = rate.split_once(' ').unwrap_or((rate.as_str(), ""));
    let (w, pad) = ((WIDTH * s).ceil(), PAD * s);
    let figure_h = FIGURE * s;
    let graph_top = pad + figure_h + 6.0 * s;
    let line_top = graph_top + GRAPH * s + 8.0 * s;
    let h = (line_top + SMALL * s * 1.3 + pad).ceil();
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let radius = tokens.radius_menu as f32 * s;
    let panel = rounded(0.5, 0.5, w - 1.0, h - 1.0, radius)?;
    let deep = tokens.backdrop_deep;
    let glass = Colour {
        r: deep.r * 0.55,
        g: deep.g * 0.55,
        b: deep.b * 0.55,
        a: 0.86,
    };
    pixmap.fill_path(
        &panel,
        &paint_of(glass),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let edge = Stroke {
        width: s.max(1.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &panel,
        &paint_of(tokens.glass_rim),
        &edge,
        Transform::identity(),
        None,
    );
    let ink = tokens.backdrop_text;
    let dim = Colour {
        a: ink.a * 0.7,
        ..ink
    };
    // The figure and its unit share a baseline.
    text.set_size(figure_h);
    let baseline = (pad + text.ascent()).round();
    text.draw_on(&mut pixmap, figure, pad.round(), baseline, ink);
    let after = pad + text.width(figure) + 4.0 * s;
    text.set_size(SMALL * s);
    text.draw_on(&mut pixmap, unit, after.round(), baseline, dim);
    // The graph: a bar a frame, tall as its time against twice the
    // average, so a steady game draws a level line halfway up and a
    // stutter stands out.
    let (left, room) = (pad, w - 2.0 * pad);
    let floor = graph_top + GRAPH * s;
    let track = rounded(left, graph_top, room, GRAPH * s, 3.0 * s)?;
    pixmap.fill_path(
        &track,
        &paint_of(tokens.glass),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    // The graph keeps inside the track's rounded corners.
    let mut mask = Mask::new(pixmap.width(), pixmap.height())?;
    mask.fill_path(&track, FillRule::Winding, true, Transform::identity());
    let top_ms = (2.0 * shown.frame_ms).max(1.0);
    let step = room / (GRAPHED - 1) as f32;
    let start = left + room - step * shown.graph.len().saturating_sub(1) as f32;
    let y_of = |ms: f32| floor - (ms / top_ms).clamp(0.04, 1.0) * GRAPH * s;
    let mut line = PathBuilder::new();
    for (i, ms) in shown.graph.iter().enumerate() {
        let point = (start + step * i as f32, y_of(*ms));
        if i == 0 {
            line.move_to(point.0, point.1);
        } else {
            line.line_to(point.0, point.1);
        }
    }
    if let Some(line) = line.finish() {
        // Under the line, a soft fill down to the track's floor.
        let mut area = PathBuilder::new();
        area.move_to(start, floor);
        for (i, ms) in shown.graph.iter().enumerate() {
            area.line_to(start + step * i as f32, y_of(*ms));
        }
        area.line_to(left + room, floor);
        area.close();
        if let Some(area) = area.finish() {
            let soft = Colour {
                a: tokens.accent.a * 0.35,
                ..tokens.accent
            };
            pixmap.fill_path(
                &area,
                &paint_of(soft),
                FillRule::Winding,
                Transform::identity(),
                Some(&mask),
            );
        }
        let pen = Stroke {
            width: 1.5 * s,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(
            &line,
            &paint_of(tokens.accent),
            &pen,
            Transform::identity(),
            Some(&mask),
        );
    }
    let baseline = (line_top + text.ascent()).round();
    text.draw_on(&mut pixmap, &times, pad.round(), baseline, dim);
    Some(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_paints_its_numbers_and_graph() {
        let Ok(mut text) = Text::load("Inter", 12.0) else {
            return;
        };
        let shown = Shown {
            fps: 60,
            frame_ms: 16.7,
            slowest_1_percent_ms: 21.4,
            graph: (0..GRAPHED)
                .map(|i| if i % 37 == 0 { 30.0 } else { 16.7 })
                .collect(),
        };
        for scale in [1.0, 2.0] {
            let pixmap = paint(&shown, scale, &Tokens::built_in(), &mut text).unwrap();
            assert_eq!(pixmap.width(), (WIDTH * scale as f32).ceil() as u32);
            // The panel, not the game, at its middle.
            let middle = pixmap.pixel(pixmap.width() / 2, 4).unwrap();
            assert!(middle.alpha() > 200);
        }
    }
}
