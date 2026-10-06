//! A preset's picture on its card (M5.6a): drawn from the preset's own
//! file (`presets/NAME.toml` through `edel::presets`) in the tokens'
//! colours, so a new preset shows its panels, its dock, its windows and
//! the side of their buttons with no picture to make, and a changed
//! preset or token changes its card.

use std::rc::Rc;

use gtk::cairo::Context;
use gtk::prelude::*;

use edel::presets::{Edge, Policy, Preset, Side, Style};
use edel::tokens::{Colour, Tokens};

use crate::style::Theme;

/// The picture's height in logical pixels; it is as wide as its card.
pub const HEIGHT: i32 = 92;

/// The picture of preset `name`, drawn again when the tokens change.
pub fn area(name: &str, theme: &Rc<Theme>) -> gtk::DrawingArea {
    let (preset, _) = edel::presets::named(Some(name));
    let area = gtk::DrawingArea::builder()
        .content_height(HEIGHT)
        .content_width(150)
        .hexpand(true)
        .build();
    let drawn = theme.clone();
    area.set_draw_func(move |_, cr, w, h| {
        draw(cr, f64::from(w), f64::from(h), &preset, &drawn.tokens());
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

/// Draws `preset` as a small screen `w` by `h`: the background, a bar or
/// a dock along each panel's edge, and two windows, floating one over the
/// other or tiled side by side, each with its title bar and buttons.
fn draw(cr: &Context, w: f64, h: f64, preset: &Preset, t: &Tokens) {
    let radius = f64::from(t.radius_control);
    rounded(cr, 0.0, 0.0, w, h, radius);
    cr.clip();
    // The screen, lit a little from above as the wallpaper's light is.
    let sky = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
    let bg = t.background;
    sky.add_color_stop_rgba(0.0, lift(bg.r), lift(bg.g), lift(bg.b), 1.0);
    sky.add_color_stop_rgba(1.0, f64::from(bg.r), f64::from(bg.g), f64::from(bg.b), 1.0);
    let _ = cr.set_source(&sky);
    let _ = cr.paint();

    // A 1280 by 800 screen, shrunk to the card.
    let s = (w / 1280.0).min(h / 800.0).max(0.01);
    let bar = (f64::from(t.panel_height) * s * 2.0).clamp(5.0, 12.0);
    let (mut top, mut bottom) = (0.0, h);
    for panel in &preset.panels {
        let at_top = panel.edge == Edge::Top;
        match panel.style {
            Style::Bar => {
                let y = if at_top { 0.0 } else { h - bar };
                fill(cr, t.panel, 0.0, y, w, bar, 0.0);
                let line_y = if at_top { bar - 0.5 } else { h - bar + 0.5 };
                stroke_line(cr, t.line, 0.0, line_y, w, line_y);
                marks(cr, t, panel, 0.0, y, w, bar);
                if at_top {
                    top = bar;
                } else {
                    bottom = h - bar;
                }
            }
            Style::Dock => {
                let dw = w * 0.42;
                let gap = bar * 0.45;
                let y = if at_top { gap } else { h - bar - gap };
                fill(cr, t.panel, (w - dw) / 2.0, y, dw, bar, bar * 0.35);
                for i in 0..5 {
                    let size = bar * 0.5;
                    let x = (w - dw) / 2.0 + dw * (0.16 + 0.17 * f64::from(i)) - size / 2.0;
                    fill(
                        cr,
                        t.title_text_unfocused.with(0.45),
                        x,
                        y + bar * 0.25,
                        size,
                        size,
                        size * 0.3,
                    );
                }
                if at_top {
                    top = y + bar + gap;
                } else {
                    bottom = y - gap;
                }
            }
        }
    }

    let gap = (f64::from(t.gap) * s * 2.5).max(3.0);
    let (x0, y0, x1, y1) = (gap, top + gap, w - gap, bottom - gap);
    let side = preset.windows.buttons;
    match preset.windows.policy {
        Policy::Tiling => {
            let mid = (x0 + x1) / 2.0;
            let half = (y0 + y1) / 2.0;
            let right = x1 - mid - gap / 2.0;
            let main = Rect {
                x: x0,
                y: y0,
                w: mid - gap / 2.0 - x0,
                h: y1 - y0,
            };
            window(cr, t, main, side, true);
            let upper = Rect {
                x: mid + gap / 2.0,
                y: y0,
                w: right,
                h: half - gap / 2.0 - y0,
            };
            window(cr, t, upper, side, false);
            let lower = Rect {
                y: half + gap / 2.0,
                h: y1 - half - gap / 2.0,
                ..upper
            };
            window(cr, t, lower, side, false);
        }
        Policy::Floating => {
            let (ww, wh) = ((x1 - x0) * 0.52, (y1 - y0) * 0.62);
            let behind = Rect {
                x: x0 + (x1 - x0) * 0.08,
                y: y0 + (y1 - y0) * 0.06,
                w: ww,
                h: wh,
            };
            window(cr, t, behind, side, false);
            let front = Rect {
                x: x0 + (x1 - x0) * 0.36,
                y: y0 + (y1 - y0) * 0.3,
                ..behind
            };
            window(cr, t, front, side, true);
        }
    }
}

/// Where a window is drawn, in the picture's pixels.
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// A window: its shadow, its page, its title bar and the three buttons on
/// `side`, the one with the keyboard drawn lighter.
fn window(cr: &Context, t: &Tokens, at: Rect, side: Side, focused: bool) {
    let Rect { x, y, w, h } = at;
    if w < 4.0 || h < 4.0 {
        return;
    }
    let r = (f64::from(t.radius_window) * 0.35).max(2.0);
    fill(cr, t.shadow.with(0.35), x, y + 1.0, w, h, r);
    fill(cr, t.card, x, y, w, h, r);
    let title = (h * 0.16).clamp(4.0, 9.0);
    let bar = if focused {
        t.title_bar_focused
    } else {
        t.title_bar
    };
    cr.save().ok();
    rounded(cr, x, y, w, h, r);
    cr.clip();
    fill(cr, bar, x, y, w, title, 0.0);
    cr.restore().ok();
    stroke_line(cr, t.line, x, y + title, x + w, y + title);
    let dot = (title * 0.18).max(0.8);
    for i in 0..3 {
        let step = dot * 3.2 * f64::from(i) + dot * 2.5;
        let cx = match side {
            Side::Left => x + step,
            Side::Right => x + w - step,
        };
        let colour = if i == 0 {
            t.title_close_hover
        } else {
            t.title_text_unfocused
        };
        set(cr, colour);
        cr.arc(cx, y + title / 2.0, dot, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
    set(cr, t.line);
    rounded(cr, x + 0.5, y + 0.5, w - 1.0, h - 1.0, r);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

/// The panel's widgets as small marks where the preset puts them: the
/// menu button, a search field, the windows or the focused one's title,
/// pinned apps, the workspace buttons and the clock.
fn marks(cr: &Context, t: &Tokens, panel: &edel::presets::Panel, x: f64, y: f64, w: f64, h: f64) {
    let size = h * 0.5;
    let my = y + (h - size) / 2.0;
    let faint = t.title_text_unfocused.with(0.45);
    let mut start = x + h * 0.4;
    for name in &panel.start {
        let width = match name.as_str() {
            "menu" => {
                fill(cr, t.accent, start, my, size, size, size * 0.3);
                size
            }
            "search" => {
                fill(cr, t.card, start, my, size * 6.0, size, size * 0.5);
                size * 6.0
            }
            "windows" | "title" | "apps" => {
                fill(
                    cr,
                    faint,
                    start,
                    my + size * 0.25,
                    size * 4.0,
                    size * 0.5,
                    size * 0.25,
                );
                size * 4.0
            }
            _ => 0.0,
        };
        start += width + size * 0.8;
    }
    if panel.centre.iter().any(|n| n == "apps") {
        let count = 5.0;
        let left = x + w / 2.0 - (count * size * 1.4) / 2.0;
        for i in 0..5 {
            fill(
                cr,
                faint,
                left + f64::from(i) * size * 1.4,
                my,
                size,
                size,
                size * 0.3,
            );
        }
    }
    let mut end = x + w - h * 0.4;
    for name in panel.end.iter().rev() {
        match name.as_str() {
            "clock" => {
                end -= size * 2.4;
                fill(
                    cr,
                    t.title_text_unfocused.with(0.7),
                    end,
                    my + size * 0.25,
                    size * 2.4,
                    size * 0.5,
                    size * 0.25,
                );
                end -= size * 0.8;
            }
            "workspaces" => {
                for i in 0..3 {
                    end -= size * 0.9;
                    let colour = if i == 2 { t.accent } else { faint };
                    set(cr, colour);
                    cr.arc(
                        end + size * 0.4,
                        my + size / 2.0,
                        size * 0.32,
                        0.0,
                        std::f64::consts::TAU,
                    );
                    let _ = cr.fill();
                }
                end -= size * 0.8;
            }
            _ => {}
        }
    }
}

fn set(cr: &Context, c: Colour) {
    cr.set_source_rgba(
        f64::from(c.r),
        f64::from(c.g),
        f64::from(c.b),
        f64::from(c.a),
    );
}

fn fill(cr: &Context, c: Colour, x: f64, y: f64, w: f64, h: f64, r: f64) {
    set(cr, c);
    rounded(cr, x, y, w, h, r);
    let _ = cr.fill();
}

fn stroke_line(cr: &Context, c: Colour, x0: f64, y0: f64, x1: f64, y1: f64) {
    set(cr, c);
    cr.set_line_width(1.0);
    cr.move_to(x0, y0);
    cr.line_to(x1, y1);
    let _ = cr.stroke();
}

fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let quarter = std::f64::consts::FRAC_PI_2;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -quarter, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, quarter);
    cr.arc(x + r, y + h - r, r, quarter, 2.0 * quarter);
    cr.arc(x + r, y + r, r, 2.0 * quarter, 3.0 * quarter);
    cr.close_path();
}

/// A channel a little nearer white, for the top of the screen.
fn lift(c: f32) -> f64 {
    f64::from(c) + (1.0 - f64::from(c)) * 0.25
}

/// A colour with its alpha scaled by `alpha`.
trait With {
    fn with(self, alpha: f32) -> Colour;
}

impl With for Colour {
    fn with(self, alpha: f32) -> Colour {
        Colour {
            a: self.a * alpha,
            ..self
        }
    }
}
