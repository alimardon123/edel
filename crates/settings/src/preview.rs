//! A preset's picture on its card (M5.6a): drawn from the preset's own
//! file (`presets/NAME.toml` through `edel::presets`) in the tokens'
//! colours, so a new preset shows its panels, its dock, its windows and
//! where windows open with no picture to make, and a changed preset or
//! token changes its card. The Displays page's picture of the screens
//! side by side is here too (M5.7a).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::cairo::Context;
use gtk::prelude::*;

use edel::presets::{Edge, Policy, Preset, Style};
use edel::tokens::{Colour, Tokens};

use crate::screens::{self, Screen};
use crate::style::Theme;

/// The picture's height in logical pixels; it is as wide as its card.
pub const HEIGHT: i32 = 66;

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

/// The arrangement picture's height in logical pixels.
pub const ARRANGEMENT_HEIGHT: i32 = 150;

/// The screens side by side as the layout places them (M5.7a): a pane
/// for each, to scale with the others, in the tokens' colours, with its
/// name and its size in logical pixels; one that is off is only an
/// outline. It draws whatever `shown` holds when it is drawn, so a page
/// sets the list and asks for a draw; it assumes no number of screens
/// and none being local (ADR-011).
pub fn arrangement(theme: &Rc<Theme>, shown: Rc<RefCell<Vec<Screen>>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(ARRANGEMENT_HEIGHT)
        .content_width(200)
        .hexpand(true)
        .css_classes(["edel-arrangement"])
        .build();
    let drawn = theme.clone();
    area.set_draw_func(move |_, cr, w, h| {
        draw_screens(
            cr,
            f64::from(w),
            f64::from(h),
            &shown.borrow(),
            &drawn.tokens(),
        );
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

fn draw_screens(cr: &Context, w: f64, h: f64, screens: &[Screen], t: &Tokens) {
    let rects = screens::arrange(screens, w, h, 18.0);
    cr.select_font_face(
        &t.font,
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    for (screen, rect) in screens.iter().zip(rects) {
        let Some([x, y, sw, sh]) = rect else { continue };
        // A hair of space between neighbours, so touching screens read as two.
        let (x, y, sw, sh) = (x + 1.0, y + 1.0, (sw - 2.0).max(0.0), (sh - 2.0).max(0.0));
        let radius = f64::from(t.radius_small);
        if screen.on {
            let sky = gtk::cairo::LinearGradient::new(x, y, x + sw * 0.1, y + sh);
            let top = mix(t.background, t.accent, 0.22);
            let bottom = mix(t.background, t.accent, 0.06);
            for (at, c) in [(0.0, top), (1.0, bottom)] {
                sky.add_color_stop_rgba(at, f64::from(c.r), f64::from(c.g), f64::from(c.b), 1.0);
            }
            rounded(cr, x, y, sw, sh, radius);
            let _ = cr.set_source(&sky);
            let _ = cr.fill_preserve();
            set(cr, t.accent.with(0.55));
        } else {
            rounded(cr, x, y, sw, sh, radius);
            set(cr, t.line);
        }
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        let label = if screen.on {
            t.title_text
        } else {
            t.title_text_unfocused
        };
        set(cr, label);
        cr.set_font_size(f64::from(t.text_size));
        let name = &screen.name;
        if let Ok(extents) = cr.text_extents(name) {
            if extents.width() < sw - 6.0 {
                cr.move_to(
                    x + (sw - extents.width()) / 2.0 - extents.x_bearing(),
                    y + sh / 2.0 - 1.0,
                );
                let _ = cr.show_text(name);
            }
        }
        if let Some(place) = screen.place.filter(|_| screen.on) {
            set(cr, t.title_text_unfocused);
            cr.set_font_size((f64::from(t.text_size) - 2.0).max(8.0));
            let size = format!("{} x {}", place.w, place.h);
            if let Ok(extents) = cr.text_extents(&size) {
                if extents.width() < sw - 6.0 && sh > 44.0 {
                    cr.move_to(
                        x + (sw - extents.width()) / 2.0 - extents.x_bearing(),
                        y + sh / 2.0 + 14.0,
                    );
                    let _ = cr.show_text(&size);
                }
            }
        }
    }
}

/// The mockups' sketch, 128 by 64: drawn in these units and scaled.
const UNITS: (f64, f64) = (128.0, 64.0);

/// Draws `preset` as the mockups sketch it (`docs/mockups`, `settings()`
/// in `edel-mockups.html`): a screen lit from above, a bar along each
/// panel's edge or a dock, and its windows as soft cards: two floating
/// ones, one with a dock or a taskbar, or tiles.
fn draw(cr: &Context, w: f64, h: f64, preset: &Preset, t: &Tokens) {
    let radius = f64::from(t.radius_small) + 1.0;
    rounded(cr, 0.0, 0.0, w, h, radius);
    cr.clip();
    let sky = gtk::cairo::LinearGradient::new(0.0, 0.0, w * 0.09, h);
    let top = mix(t.background, t.accent, 0.16);
    let bg = t.background;
    sky.add_color_stop_rgba(
        0.0,
        f64::from(top.r),
        f64::from(top.g),
        f64::from(top.b),
        1.0,
    );
    sky.add_color_stop_rgba(1.0, f64::from(bg.r), f64::from(bg.g), f64::from(bg.b), 1.0);
    let _ = cr.set_source(&sky);
    let _ = cr.paint();

    let (sx, sy) = (w / UNITS.0, h / UNITS.1);
    let bar = t.title_bar.with(0.9);
    let at = |x: f64, y: f64, ww: f64, hh: f64| Rect {
        x: x * sx,
        y: y * sy,
        w: ww * sx,
        h: hh * sy,
    };
    let mut dock = false;
    let mut taskbar = false;
    for panel in &preset.panels {
        let at_top = panel.edge == Edge::Top;
        match panel.style {
            Style::Bar => {
                let thick = if at_top { 6.0 } else { 8.0 };
                let y = if at_top { 0.0 } else { UNITS.1 - thick };
                let r = at(0.0, y, UNITS.0, thick);
                fill(cr, bar, r.x, r.y, r.w, r.h, 0.0);
                taskbar |= panel.centre.iter().any(|n| n == "apps");
            }
            Style::Dock => {
                let y = if at_top { 4.0 } else { UNITS.1 - 13.0 };
                let r = at(42.0, y, UNITS.0 - 84.0, 9.0);
                fill(cr, bar, r.x, r.y, r.w, r.h, 3.0 * sy);
                dock = true;
            }
        }
    }
    let windows: &[(f64, f64, f64, f64)] = match preset.windows.policy {
        Policy::Tiling => &[
            (4.0, 10.0, 58.0, 50.0),
            (66.0, 10.0, 58.0, 23.0),
            (66.0, 37.0, 58.0, 23.0),
        ],
        Policy::Floating if dock => &[(24.0, 12.0, 70.0, 32.0)],
        Policy::Floating if taskbar => &[(14.0, 7.0, 92.0, 42.0)],
        Policy::Floating => &[(18.0, 9.0, 62.0, 34.0), (48.0, 17.0, 62.0, 30.0)],
    };
    for (x, y, ww, hh) in windows {
        window(cr, t, at(*x, *y, *ww, *hh));
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

/// A window: a pane with a soft shadow below it, as the mockups' are.
fn window(cr: &Context, t: &Tokens, at: Rect) {
    let Rect { x, y, w, h } = at;
    let r = 3.0;
    for i in 1..=3 {
        let spread = f64::from(i) * 0.6;
        fill(
            cr,
            t.shadow.with(0.1),
            x - spread / 2.0,
            y + spread,
            w + spread,
            h + spread / 2.0,
            r + spread,
        );
    }
    fill(cr, t.title_bar_focused.with(0.96), x, y, w, h, r);
}

/// `a` with `amount` of `b` mixed in.
fn mix(a: Colour, b: Colour, amount: f32) -> Colour {
    let m = |x: f32, y: f32| x + (y - x) * amount;
    Colour {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: 1.0,
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
