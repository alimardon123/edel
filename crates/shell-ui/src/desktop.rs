//! The desktop's widgets (M8.21 part 21j1, M5.33c): the clock and the date
//! the person places with `appearance.widgets`, drawn over the wallpaper and
//! under the windows, with the lock screen's `Placed`, anchors and layout
//! (`lock::widgets_layout`) and its drawing (`lock::paint_time`).
//!
//! Two surfaces per screen, both on the background layer. The area surface
//! has no buffer and no size: anchored to all four edges with exclusive zone
//! 0, the compositor gives it the usable area the panels leave (smithay's
//! `LayerMap::arrange`, as the notification centre's height is found in
//! `popup.rs`), and its configure says that size. The widgets' surface is
//! only the bounding box of what is drawn, placed by its margins from the
//! usable area's top left corner (exclusive zone 0 places it in that area),
//! so a screen costs about 0.23 MiB at scale 1, not a screen's buffer.
//!
//! Resting: the widgets draw only when `needs_draw` says so (a new minute, new
//! parts, area or scale), and never before the compositor showed the last
//! drawing (its frame callback) or took a new place (its configure). Windows
//! over them need nothing: the compositor skips what lies under an opaque
//! window, and shell-ui draws at most once a minute.
//!
//! Coordinates: the logs (`places`, which CI reads) give each widget in the
//! usable area's coordinates, not the screen's. The area's origin is the
//! screen's top left corner only when no panel lies on its top or left edge
//! (the Classic and Windows-like presets have one at the bottom); with a top
//! panel the clock's top is the panel's height lower on the screen.
//!
//! Logs: `desktop widgets off`, `desktop widgets N` (the parts),
//! `desktop widgets on NAME WxH: clock X+Y+WxH, date X+Y+WxH`.

use std::cell::RefCell;

use smithay_client_toolkit::compositor::{FrameCallbackData, Region};
use smithay_client_toolkit::reexports::client::protocol::{
    wl_output::WlOutput, wl_shm, wl_surface,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerSurface,
};
use smithay_client_toolkit::shm::slot::Buffer;
use tiny_skia::Pixmap;

use edel::lock_screen::{Background, Part, Placed};
use edel::widgets::Surface;

use crate::Shell;
use crate::lock::{self, Layout};
use crate::messages;
use crate::paint;
use crate::popup::Rect;

/// The namespaces the two surfaces are made under (shell-ui's own; the
/// compositor never reads them).
pub const AREA: &str = "edel-desktop-area";
pub const NAMESPACE: &str = "edel-desktop";

/// Whether the desktop draws a part: the clock and the date, the parts the
/// catalogue says fit the desktop.
pub fn draws(part: Part) -> bool {
    matches!(part, Part::Clock | Part::Date)
}

/// One screen's desktop: its surfaces, the usable area as the compositor
/// gave it and what was drawn last.
pub struct Screen {
    pub(crate) output: WlOutput,
    /// The screen's name, for the logs.
    name: String,
    area_surface: LayerSurface,
    /// The usable area's size, from the area surface's configure.
    area: Option<(u32, u32)>,
    /// The widgets' surface, made when there is something to draw.
    surface: Option<LayerSurface>,
    /// Where the widgets' surface lies in the usable area and how big it is,
    /// as last committed.
    place: Option<Rect>,
    scale: i32,
    /// A drawing waits for the compositor: a frame callback, or the configure
    /// of a new place.
    waiting: bool,
    drawn: Option<Drawn>,
    /// The line last logged, so a change is logged once.
    logged: String,
    /// The buffer shown, kept until the compositor lets go of it (`spent`).
    shown: Option<Buffer>,
    /// The zones the clocks name that this machine lacks, each once.
    zones_warned: RefCell<Vec<String>>,
}

/// What a drawing shows: the minute, the time in each zone a clock names,
/// the parts, the usable area and the scale. An equal one is not drawn again.
#[derive(Clone, PartialEq)]
pub struct Drawn {
    when: lock::When,
    zoned: Vec<(String, lock::When)>,
    parts: Vec<Placed>,
    area: (u32, u32),
    scale: i32,
}

/// Whether a drawing of `now` is due: never while the compositor has not
/// shown the last one, and then only when `now` differs from what was drawn.
pub fn needs_draw(drawn: Option<&Drawn>, now: &Drawn, waiting: bool) -> bool {
    !waiting && drawn != Some(now)
}

/// The rectangle the widgets' surface covers: the date and every clock placed,
/// grown outward to whole pixels; None when nothing is placed.
pub fn bounds(at: &Layout) -> Option<Rect> {
    let mut rects = at
        .date
        .into_iter()
        .chain(at.clocks.iter().filter_map(|one| one.rect));
    let first = rects.next()?;
    let (mut left, mut top) = (first.x, first.y);
    let (mut right, mut bottom) = (first.right(), first.y + first.h);
    for r in rects {
        left = left.min(r.x);
        top = top.min(r.y);
        right = right.max(r.right());
        bottom = bottom.max(r.y + r.h);
    }
    let (left, top) = (left.floor(), top.floor());
    Some(Rect::new(
        left,
        top,
        right.ceil() - left,
        bottom.ceil() - top,
    ))
}

/// The line CI reads: the usable area's size, then each widget placed in it,
/// the first clock as `clock`, the others `clockN`, and the date.
pub fn places(name: &str, area: (u32, u32), at: &Layout) -> String {
    let r = |r: Rect| format!("{:.0}+{:.0}+{:.0}x{:.0}", r.x, r.y, r.w, r.h);
    let mut parts: Vec<String> = Vec::new();
    for (i, one) in at.clocks.iter().enumerate() {
        if let Some(rect) = one.rect {
            let label = if i == 0 {
                "clock".to_owned()
            } else {
                format!("clock{}", i + 1)
            };
            parts.push(format!("{label} {}", r(rect)));
        }
    }
    if let Some(rect) = at.date {
        parts.push(format!("date {}", r(rect)));
    }
    let list = if parts.is_empty() {
        "nothing placed".to_owned()
    } else {
        parts.join(", ")
    };
    format!("desktop widgets on {name} {}x{}: {list}", area.0, area.1)
}

/// The layout moved by (`dx`, `dy`): the date and the clocks, which is what
/// `lock::paint_time` draws.
fn moved(mut at: Layout, dx: f32, dy: f32) -> Layout {
    let shift = |r: &mut Rect| {
        r.x += dx;
        r.y += dy;
    };
    if let Some(rect) = at.date.as_mut() {
        shift(rect);
    }
    for one in &mut at.clocks {
        if let Some(rect) = one.rect.as_mut() {
            shift(rect);
        }
    }
    at
}

/// Lets go of a layer surface: its role first, then its surface, as the
/// panels are let go (`remake_panels`).
fn let_go(surface: LayerSurface) {
    let wl = surface.wl_surface().clone();
    drop(surface);
    wl.destroy();
}

/// Lets go of a screen's surfaces, the widgets' first.
fn let_go_screen(screen: Screen) {
    if let Some(surface) = screen.surface {
        let_go(surface);
    }
    let_go(screen.area_surface);
}

impl Shell {
    /// Follows `appearance.widgets` (the person's over the machine's) at start
    /// and after each change of the settings files. Unchanged, nothing
    /// happens; with none left, the surfaces go.
    pub(crate) fn desktop_follow(&mut self, machine: Option<&str>, person: Option<&str>) {
        // A game session draws none (M8.26).
        if self.game_session {
            return;
        }
        let parts: Vec<Placed> = edel::settings::desktop_widgets_in(machine, person)
            .into_iter()
            .filter(|placed| draws(placed.part))
            .collect();
        if parts == self.desktop_parts {
            return;
        }
        if parts.is_empty() {
            self.desktop_parts.clear();
            for screen in std::mem::take(&mut self.desktops) {
                let_go_screen(screen);
            }
            eprintln!("edel-shell-ui: desktop widgets off");
            return;
        }
        self.desktop_parts = parts;
        if self.desktops.is_empty() {
            let outputs: Vec<WlOutput> = self.outputs.outputs().collect();
            self.make_desktops(&outputs);
        }
        eprintln!(
            "edel-shell-ui: desktop widgets {}",
            self.desktop_parts.len()
        );
        self.desktop_redraw();
    }

    /// Draws every screen's widgets again, as a new look or new parts ask.
    pub(crate) fn desktop_redraw(&mut self) {
        for screen in &mut self.desktops {
            screen.drawn = None;
        }
        for i in 0..self.desktops.len() {
            self.draw_desktop(i);
        }
    }

    /// Makes each output's surfaces, the area first. Made after the panels, as
    /// the layers' order is their mapping order and the area takes the zone the
    /// panels leave.
    fn make_desktops(&mut self, outputs: &[WlOutput]) {
        for output in outputs {
            let name = self
                .outputs
                .info(output)
                .and_then(|info| info.name)
                .unwrap_or_default();
            let surface = self.compositor.create_surface(&self.qh);
            let area_surface = self.layers.create_layer_surface(
                &self.qh,
                surface,
                Layer::Background,
                Some(AREA),
                Some(output),
            );
            area_surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            area_surface.set_size(0, 0);
            area_surface.set_exclusive_zone(0);
            area_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            if let Ok(region) = Region::new(&self.compositor) {
                area_surface
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
            }
            area_surface.commit();
            self.desktops.push(Screen {
                output: output.clone(),
                name,
                area_surface,
                area: None,
                surface: None,
                place: None,
                scale: 1,
                waiting: false,
                drawn: None,
                logged: String::new(),
                shown: None,
                zones_warned: RefCell::new(Vec::new()),
            });
        }
    }

    /// Makes the desktop's surfaces afresh on `outputs`, after the panels are
    /// made (`remake_panels`) or when the screens change (`outputs_changed`).
    pub(crate) fn remake_desktops(&mut self, outputs: &[WlOutput]) {
        for screen in std::mem::take(&mut self.desktops) {
            let_go_screen(screen);
        }
        if !self.desktop_parts.is_empty() {
            self.make_desktops(outputs);
        }
    }

    /// Which screen a surface of the desktop belongs to, and whether it is the
    /// area (true) or the widgets' surface (false).
    pub(crate) fn desktop_surface(&self, surface: &wl_surface::WlSurface) -> Option<(usize, bool)> {
        self.desktops.iter().enumerate().find_map(|(i, screen)| {
            if screen.area_surface.wl_surface() == surface {
                Some((i, true))
            } else if screen
                .surface
                .as_ref()
                .is_some_and(|s| s.wl_surface() == surface)
            {
                Some((i, false))
            } else {
                None
            }
        })
    }

    /// The usable area is known (the area surface's configure): the widgets
    /// are laid out in it and drawn.
    pub(crate) fn desktop_area(&mut self, i: usize, size: (u32, u32)) {
        let Some(screen) = self.desktops.get_mut(i) else {
            return;
        };
        screen.area = (size.0 > 0 && size.1 > 0).then_some(size);
        screen.drawn = None;
        self.draw_desktop(i);
    }

    /// The compositor showed the last drawing, or took the new place of the
    /// widgets' surface: draw again if what they show changed meanwhile.
    pub(crate) fn desktop_settled(&mut self, i: usize) {
        if let Some(screen) = self.desktops.get_mut(i) {
            screen.waiting = false;
        }
        self.draw_desktop(i);
    }

    /// The widgets' surface takes the screen's scale (`preferred_buffer_scale`,
    /// as the panels take it) and draws at it.
    pub(crate) fn desktop_scaled(&mut self, i: usize, factor: i32) {
        if let Some(screen) = self.desktops.get_mut(i) {
            screen.scale = factor.clamp(1, 4);
            screen.drawn = None;
        }
        self.draw_desktop(i);
    }

    /// The compositor took a surface away: its screen goes with it.
    pub(crate) fn desktop_closed(&mut self, i: usize) {
        if i < self.desktops.len() {
            let screen = self.desktops.remove(i);
            let_go_screen(screen);
        }
    }

    /// Draws screen `i`'s widgets if what they show changed and the compositor
    /// has shown the last drawing. A new place of the widgets' surface is
    /// committed first and drawn once its configure comes.
    pub(crate) fn draw_desktop(&mut self, i: usize) {
        let Some(area) = self.desktops.get(i).and_then(|screen| screen.area) else {
            return;
        };
        let now = edel::timezones::now();
        let screen = &self.desktops[i];
        let drawn = Drawn {
            when: lock::When::of(&now),
            zoned: lock::zoned_of_parts(&self.desktop_parts, &now, &screen.zones_warned),
            parts: self.desktop_parts.clone(),
            area,
            scale: screen.scale,
        };
        if !needs_draw(screen.drawn.as_ref(), &drawn, screen.waiting) {
            return;
        }
        let at = lock::widgets_layout(
            area.0 as f32,
            area.1 as f32,
            &self.tokens,
            &self.desktop_parts,
            &mut self.text,
        );
        let screen = &mut self.desktops[i];
        let Some(b) = bounds(&at) else {
            // Nothing fits: the widgets' surface goes until something does.
            if let Some(surface) = screen.surface.take() {
                let_go(surface);
            }
            screen.place = None;
            screen.drawn = Some(drawn);
            return;
        };
        if screen.surface.is_none() {
            let surface = self.compositor.create_surface(&self.qh);
            let surface = self.layers.create_layer_surface(
                &self.qh,
                surface,
                Layer::Background,
                Some(NAMESPACE),
                Some(&screen.output),
            );
            surface.set_anchor(Anchor::TOP | Anchor::LEFT);
            surface.set_exclusive_zone(0);
            surface.set_keyboard_interactivity(KeyboardInteractivity::None);
            if let Ok(region) = Region::new(&self.compositor) {
                // Empty: the widgets take no clicks, the windows under them do.
                surface
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
            }
            screen.surface = Some(surface);
            screen.place = None;
        }
        let Some(surface) = screen.surface.as_ref() else {
            return;
        };
        if screen.place != Some(b) {
            surface.set_margin(b.y as i32, 0, 0, b.x as i32);
            let resized = screen.place.is_none_or(|p| p.w != b.w || p.h != b.h);
            screen.place = Some(b);
            if resized {
                // A new size: committed now, drawn once it is configured.
                surface.set_size(b.w as u32, b.h as u32);
                surface.commit();
                screen.waiting = true;
                return;
            }
            // Only a new place: no configure follows a margin, so the
            // margin goes with the drawing below, in one commit.
        }
        let scale = screen.scale;
        let (bw, bh) = (b.w as u32, b.h as u32);
        let (w, h) = (bw * scale as u32, bh * scale as u32);
        let Some(mut pixmap) = Pixmap::new(w, h) else {
            return;
        };
        let line = places(&screen.name, area, &at);
        if line != screen.logged {
            eprintln!("edel-shell-ui: {line}");
            screen.logged = line;
        }
        let view = lock::View {
            when: Some(drawn.when),
            zoned: drawn.zoned.clone(),
            ..lock::View::new(String::new(), String::new(), String::new())
        };
        let at = moved(at, -b.x, -b.y);
        let behind = lock::behind_of(
            Surface::Desktop,
            &Background::Sharp,
            0,
            &self.wallpaper,
            &self.tokens,
        );
        lock::paint_time(
            &mut pixmap,
            &view,
            &at,
            &self.tokens,
            &mut self.text,
            scale as f32,
            behind,
        );
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        paint::to_argb(&pixmap, canvas);
        let wl = surface.wl_surface();
        wl.set_buffer_scale(scale);
        wl.damage_buffer(0, 0, w as i32, h as i32);
        if let Err(e) = buffer.attach_to(wl) {
            eprintln!(
                "edel-shell-ui: {}",
                messages::desktop_not_drawn(format!("{e:#}"))
            );
            return;
        }
        wl.frame(&self.qh, FrameCallbackData(wl.clone()));
        surface.commit();
        screen.waiting = true;
        screen.drawn = Some(drawn);
        if let Some(old) = screen.shown.replace(buffer) {
            self.spent.push(old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edel::lock_screen::desktop_default;
    use edel::tokens::Tokens;
    use edel::wallpapers::Wallpaper;
    use edel::widgets::{CATALOGUE, Surface};

    use crate::paint::Text;

    fn when(minute: i8) -> lock::When {
        lock::When {
            hour: 14,
            minute,
            weekday: 6,
            day: 3,
            month: 10,
            year: 2026,
        }
    }

    fn drawn(minute: i8, scale: i32) -> Drawn {
        Drawn {
            when: when(minute),
            zoned: Vec::new(),
            parts: desktop_default(),
            area: (1280, 764),
            scale,
        }
    }

    #[test]
    fn draws_is_the_catalogues_desktop_parts() {
        for entry in CATALOGUE.iter().filter(|w| w.fits(Surface::Desktop)) {
            assert!(
                Part::named(entry.name).is_some_and(draws),
                "{} fits the desktop but is not drawn",
                entry.name
            );
        }
        for part in Part::ALL {
            if draws(part) {
                assert!(part.widget().fits(Surface::Desktop), "{}", part.name());
            }
        }
        assert!(draws(Part::Clock) && draws(Part::Date) && !draws(Part::Person));
    }

    #[test]
    fn a_drawing_waits_for_the_compositor_and_for_a_new_minute() {
        let now = drawn(6, 1);
        assert!(needs_draw(None, &now, false), "the first drawing");
        assert!(!needs_draw(Some(&now), &now, false), "the same again");
        assert!(!needs_draw(None, &now, true), "while the compositor waits");
        assert!(needs_draw(Some(&now), &drawn(7, 1), false), "a new minute");
        assert!(needs_draw(Some(&now), &drawn(6, 2), false), "a new scale");
        let mut other = now.clone();
        other.parts.clear();
        assert!(needs_draw(Some(&now), &other, false), "new parts");
    }

    #[test]
    fn the_widgets_stand_at_the_top_right_in_a_small_surface() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let at = lock::widgets_layout(1280.0, 764.0, &tokens, &desktop_default(), &mut text);
        let b = bounds(&at).expect("the clock and the date stand");
        assert!(b.x >= 0.0 && b.y >= 0.0, "{b:?}");
        assert!(b.right() <= 1280.0 && b.y + b.h <= 764.0, "{b:?}");
        assert!(b.x > 1280.0 / 2.0, "the top right: {b:?}");
        // The default clock is Small at the lock's size (75 px, 394 wide with its
        // date under it), so the surface is about 0.2 MiB at scale 1.
        assert!(b.w < 420.0 && b.h < 160.0, "{b:?}");
    }

    #[test]
    fn the_places_line_names_each_placed_part_and_no_others() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let at = lock::widgets_layout(1280.0, 764.0, &tokens, &desktop_default(), &mut text);
        let line = places("Virtual-1", (1280, 764), &at);
        assert!(
            line.starts_with("desktop widgets on Virtual-1 1280x764: clock "),
            "{line}"
        );
        assert!(line.contains(", date "), "{line}");
        let none = lock::widgets_layout(1280.0, 764.0, &tokens, &[], &mut text);
        assert_eq!(
            places("Virtual-1", (1280, 764), &none),
            "desktop widgets on Virtual-1 1280x764: nothing placed"
        );
    }

    /// With `EDEL_LOCK_PNG` set it writes `desktop-widgets.png`: the widgets
    /// over a navy backdrop standing in for a wallpaper (M8.21 part 21j1).
    #[test]
    fn the_desktop_widgets_save_a_picture_when_asked() {
        let Ok(dir) = std::env::var("EDEL_LOCK_PNG") else {
            return;
        };
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let at = lock::widgets_layout(1280.0, 764.0, &tokens, &desktop_default(), &mut text);
        let mut pixmap = Pixmap::new(1280, 764).unwrap();
        lock::backdrop(&mut pixmap, &tokens);
        let view = lock::View {
            when: Some(when(6)),
            ..lock::View::new(String::new(), String::new(), String::new())
        };
        let behind = lock::behind_of(
            Surface::Desktop,
            &Background::Sharp,
            0,
            &Wallpaper::Background,
            &tokens,
        );
        lock::paint_time(&mut pixmap, &view, &at, &tokens, &mut text, 1.0, behind);
        pixmap
            .save_png(format!("{dir}/desktop-widgets.png"))
            .unwrap();
    }
}
