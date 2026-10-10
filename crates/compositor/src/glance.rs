//! The overview on screen (roadmap M5.2j; laid out anew in M5.2j-b after
//! `docs/mockups/shell/overview.jpg`): Super+W, or a click on the panel
//! switcher's lit workspace, shows on each screen every workspace small in
//! a strip on a tray along one side (`workspaces.overview_strip`, the left
//! by default) and the shown workspace's windows spread out large on the
//! rest, each with its app and title under it and a close button while the
//! pointer is over it. The panels stay; the wallpaper is dimmed. A click on
//! a window goes to it; a click on a small workspace shows its windows
//! here; a window dragged onto a small workspace moves there; a small
//! workspace dragged onto another takes its place (M5.2j-b2); the frame
//! with a plus adds a workspace; a strip too long for its side scrolls
//! with the wheel or its arrows; a click on nothing, Escape or Super+W
//! leaves. The pictures are those each window last showed, and no window
//! is moved or resized: tiling or floating, every workspace is as it was
//! when the overview closes. `edel_compositor::overview` holds the
//! geometry and `glance_look.rs` paints the tray, the names and the close
//! button.

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::Renderer;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::solid::{SolidColorBuffer, SolidColorRenderElement};
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::Window;
use smithay::output::Output;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

use edel::i18n::tr;
use edel::tokens::Colour;
use edel_compositor::overview::{Plan, Side, frame_at, in_view, plan, spread};

use crate::decoration::{app_id, data, title};
use crate::glance_look::{self, CLOSE, Mark, NAME_ICON, PILL};
use crate::glance_search::{self, Search};
use crate::render::{Drawn, Element};
use crate::state::Edel;

/// How far the pointer moves, in logical pixels, before a press on a
/// window becomes a drag rather than a click.
const DRAG_FROM: f64 = 6.0;
/// The gap between a spread window and its name pill.
const PILL_GAP: i32 = 6;

/// The overview while it is open.
#[derive(Default)]
pub struct Overview {
    /// The window or workspace held by the pointer, if any.
    drag: Option<Drag>,
    /// What was typed and what matches it (M5.2j-b3).
    pub search: Search,
    /// The search field and its card, as last painted.
    field: Option<Painted>,
    card: Option<Painted>,
    /// Where each screen's strip is scrolled to, once scrolled, and the
    /// wheel's part of an item not yet scrolled.
    first: HashMap<String, usize>,
    wheel: f64,
    /// The flat colours drawn, each kept by what it is, so a frame drawn
    /// again unchanged is not redrawn.
    solids: HashMap<String, SolidColorBuffer>,
    /// The ids of the windows' pictures, by window, place and surface.
    ids: HashMap<(Window, bool, usize), Id>,
    /// The plain cards of windows never drawn yet.
    cards: HashMap<(Window, bool), SolidColorBuffer>,
    /// The painted parts, each kept with what it shows: each screen's
    /// tray, each window's name, the close button.
    trays: HashMap<String, Painted>,
    names: HashMap<Window, Painted>,
    close: Option<Painted>,
    /// Each window's title bar, large and small; each window's shadow;
    /// each screen's backdrop; the held workspace's frame.
    bars: HashMap<(Window, bool), Painted>,
    shadows: HashMap<Window, (Painted, i32)>,
    backdrops: HashMap<String, Painted>,
    held: Option<(Painted, i32)>,
    /// The accent ring round the window under the pointer.
    ring: Option<(Painted, i32)>,
    /// The last places line logged for each screen.
    logged: HashMap<String, String>,
}

/// A painted picture, what it shows (to paint it again only when that
/// changes) and its size in pixels.
struct Painted {
    shows: String,
    buffer: MemoryRenderBuffer,
    pixels: (i32, i32),
}

/// A window or a small workspace picked up in the overview.
struct Drag {
    held: Held,
    /// Its screen, by name.
    screen: String,
    /// Where it was pressed, and the pointer from the picture's corner.
    from: Point<f64, Logical>,
    grip: Point<f64, Logical>,
    /// Whether the pointer moved far enough for a drag.
    moving: bool,
}

/// What a drag holds.
#[derive(Clone, PartialEq)]
enum Held {
    Window(Window),
    Frame(usize),
}

/// One screen in the overview: its name and area, its plan, the workspace
/// it shows, every window with its workspace and frame (bottom first), and
/// the shown workspace's windows spread out (frame, place on the stage).
pub struct Screen {
    pub name: String,
    pub area: Rectangle<i32, Logical>,
    pub plan: Plan,
    pub shown: usize,
    pub windows: Vec<(usize, Window, Rectangle<i32, Logical>)>,
    pub spread: Vec<Spread>,
}

/// A window of the shown workspace spread on the stage: the window, its
/// frame on the screen and where the overview draws it.
pub type Spread = (Window, Rectangle<i32, Logical>, Rectangle<i32, Logical>);

/// What lies under the pointer in the overview.
enum Hit {
    Close(Window),
    Window(String, Window, Rectangle<i32, Logical>),
    Frame(String, usize),
    Arrow(String, bool),
    /// The search field, and a row of its card.
    Search,
    Result(usize),
    Add,
    Tray,
    Nothing,
}

/// The close button over the corner of a window spread at `at`.
fn close_rect(at: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new(
        (
            at.loc.x + at.size.w - CLOSE / 2 - 2,
            at.loc.y - CLOSE / 2 + 2,
        )
            .into(),
        (CLOSE, CLOSE).into(),
    )
}

/// The room round a spread window that still counts as over it: the
/// close button's half outside its corner.
fn reach(at: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new(
        (at.loc.x, at.loc.y - CLOSE / 2).into(),
        (at.size.w + CLOSE / 2, at.size.h + CLOSE / 2).into(),
    )
}

impl Edel {
    /// Super+W, or a click on the panel switcher's lit workspace: shows
    /// the overview, or leaves it.
    pub fn toggle_overview(&mut self) {
        if self.overview.is_some() {
            self.leave_overview();
            return;
        }
        if self.dragging || self.switcher.is_some() {
            eprintln!(
                "edel-compositor: overview not shown: {}",
                if self.dragging {
                    "a window is being dragged"
                } else {
                    "the window switcher is open"
                }
            );
            return;
        }
        self.overview = Some(Overview::default());
        eprintln!("edel-compositor: overview shown");
        self.hover = None;
        self.dirty = true;
        self.repoint();
    }

    /// Leaves the overview, as it was.
    pub fn leave_overview(&mut self) {
        if self.overview.take().is_some() {
            eprintln!("edel-compositor: overview hidden");
            self.dirty = true;
            self.repoint();
        }
    }

    /// Every screen as the overview lays it out.
    pub fn overview_screens(&self) -> Vec<Screen> {
        let count = self.desks.count();
        let side = Side::named(self.settings.overview_strip());
        let add = !self.settings.dynamic() && count < edel_compositor::desks::MOST;
        let areas = self.window_areas();
        let mut screens: Vec<Screen> = self
            .space
            .outputs()
            .filter_map(|output| {
                let area = self.space.output_geometry(output)?;
                let name = output.name();
                let free = areas
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map_or(area, |(_, r)| *r);
                let shown = self.desks.shown_on(&name);
                // Until scrolled, the strip keeps the shown workspace in view.
                let scrolled = self.overview.as_ref().and_then(|o| o.first.get(&name));
                let mut laid = plan(area, free, count, add, side, scrolled.copied().unwrap_or(0));
                if scrolled.is_none() && laid.scrolls() {
                    let items = count + usize::from(add);
                    let first = in_view(items, laid.shows, 0, shown);
                    laid = plan(area, free, count, add, side, first);
                }
                Some(Screen {
                    shown,
                    name,
                    area,
                    plan: laid,
                    windows: Vec::new(),
                    spread: Vec::new(),
                })
            })
            .collect();
        let screen_of = |window: &Window| {
            self.desks
                .layout_of(window)
                .screen_of(window)
                .map(str::to_string)
        };
        let mut put = |desk: usize, window: &Window, frame: Rectangle<i32, Logical>| {
            let name = screen_of(window);
            let screen = screens
                .iter()
                .position(|s| Some(&s.name) == name.as_ref())
                .unwrap_or(0);
            if let Some(screen) = screens.get_mut(screen) {
                screen.windows.push((desk, window.clone(), frame));
            }
        };
        for window in self.space.elements() {
            let (Some(desk), Some(frame)) = (self.desks.desk_of(window), self.frame_of(window))
            else {
                continue;
            };
            put(desk, window, frame);
        }
        for (desk, window, frame) in self.desks.hidden() {
            if self.desks.minimized(window).is_none() {
                put(desk, window, frame);
            }
        }
        for screen in &mut screens {
            let mine: Vec<_> = screen
                .windows
                .iter()
                .filter(|(desk, _, _)| *desk == screen.shown)
                .map(|(_, w, f)| (w.clone(), *f))
                .collect();
            let sizes: Vec<Size<i32, Logical>> = mine.iter().map(|(_, f)| f.size).collect();
            let at = spread(screen.plan.stage, &sizes);
            screen.spread = mine
                .into_iter()
                .zip(at)
                .map(|((w, f), at)| (w, f, at))
                .collect();
        }
        screens
    }

    /// What lies under `point`, and the window under it if any (for the
    /// close button, shown while the pointer is over a window).
    fn overview_hit(&self, point: Point<f64, Logical>) -> Hit {
        let screens = self.overview_screens();
        let Some(screen) = screens.iter().find(|s| s.area.to_f64().contains(point)) else {
            return Hit::Nothing;
        };
        let rows = self.overview.as_ref().map_or(0, |o| o.search.found.len());
        if rows > 0 {
            let card = glance_search::card(screen.plan.search, rows);
            if card.to_f64().contains(point) {
                return glance_search::row_at(card, rows, point.y).map_or(Hit::Search, Hit::Result);
            }
        }
        if screen.plan.search.to_f64().contains(point) {
            return Hit::Search;
        }
        let over = screen
            .spread
            .iter()
            .rev()
            .find(|(_, _, at)| reach(*at).to_f64().contains(point));
        if let Some((window, _, at)) = over {
            if close_rect(*at).to_f64().contains(point) {
                return Hit::Close(window.clone());
            }
            if at.to_f64().contains(point) {
                return Hit::Window(screen.name.clone(), window.clone(), *at);
            }
        }
        if let Some(i) = frame_at(&screen.plan.frames, point) {
            return Hit::Frame(screen.name.clone(), i);
        }
        if screen.plan.add.is_some_and(|a| a.to_f64().contains(point)) {
            return Hit::Add;
        }
        for (arrow, forward) in [(screen.plan.before, false), (screen.plan.after, true)] {
            if arrow.is_some_and(|a| a.to_f64().contains(point)) {
                return Hit::Arrow(screen.name.clone(), forward);
            }
        }
        if screen.plan.tray.to_f64().contains(point) {
            return Hit::Tray;
        }
        Hit::Nothing
    }

    /// A button went down while the overview is open: on a window or a
    /// small workspace it may become a drag; see `overview_release` for
    /// what a click does.
    pub fn overview_press(&mut self, point: Point<f64, Logical>) {
        let held = match self.overview_hit(point) {
            Hit::Window(screen, window, at) => Some((screen, Held::Window(window), at)),
            Hit::Frame(screen, i) => self
                .overview_screens()
                .into_iter()
                .find(|s| s.name == screen)
                .and_then(|s| s.plan.frames.get(i).copied())
                .map(|at| (screen, Held::Frame(i), at)),
            _ => None,
        };
        let Some(overview) = &mut self.overview else {
            return;
        };
        overview.drag = held.map(|(screen, held, at)| Drag {
            held,
            screen,
            from: point,
            grip: point - at.loc.to_f64(),
            moving: false,
        });
    }

    /// The wheel turned `steps` items' worth while the overview is open:
    /// over a strip that scrolls, it moves along, whole items at a time.
    pub fn overview_scroll(&mut self, point: Point<f64, Logical>, steps: f64) {
        let Some(screen) = self
            .overview_screens()
            .into_iter()
            .find(|s| s.plan.tray.to_f64().contains(point))
        else {
            return;
        };
        if !screen.plan.scrolls() {
            return;
        }
        let Some(overview) = &mut self.overview else {
            return;
        };
        overview.wheel += steps;
        let whole = overview.wheel.trunc();
        if whole == 0.0 {
            return;
        }
        overview.wheel -= whole;
        let first = screen.plan.first as i64 + whole as i64;
        self.overview_scroll_to(&screen.name, usize::try_from(first.max(0)).unwrap_or(0));
    }

    /// Scrolls the strip of screen `name` to start at item `first`.
    fn overview_scroll_to(&mut self, name: &str, first: usize) {
        if let Some(overview) = &mut self.overview {
            eprintln!(
                "edel-compositor: overview strip of {name} scrolled to {}",
                first + 1
            );
            overview.first.insert(name.to_string(), first);
            self.dirty = true;
        }
    }

    /// The pointer moved while the overview is open: a held window
    /// follows, and the close button follows the window under it.
    pub fn overview_motion(&mut self, point: Point<f64, Logical>) {
        // The row under the pointer is the chosen one, as Up and Down choose.
        if let Hit::Result(i) = self.overview_hit(point) {
            if let Some(overview) = self.overview.as_mut() {
                overview.search.chosen = i;
            }
        }
        if let Some(drag) = self.overview.as_mut().and_then(|o| o.drag.as_mut()) {
            let moved = point - drag.from;
            if moved.x.hypot(moved.y) >= DRAG_FROM {
                drag.moving = true;
            }
        }
        self.dirty = true;
    }

    /// The button came up while the overview is open. A window dragged
    /// onto a small workspace of its screen moves there and the overview
    /// stays; a click on a window goes to it and leaves; on its close
    /// button closes it; on a small workspace shows its windows here (the
    /// shown one leaves to it); a small workspace dragged onto another
    /// takes its place; on the plus adds a workspace; on an arrow scrolls
    /// the strip; on the tray does nothing; on nothing leaves.
    pub fn overview_release(&mut self, point: Point<f64, Logical>) {
        let Some(overview) = &mut self.overview else {
            return;
        };
        let drag = overview.drag.take();
        let hit = self.overview_hit(point);
        if let Some(drag) = drag.as_ref().filter(|d| d.moving) {
            if let Hit::Frame(name, to) = hit {
                if name == drag.screen {
                    match &drag.held {
                        Held::Window(window) => {
                            self.carry_window(window, to);
                        }
                        Held::Frame(from) => self.move_workspace(*from, to),
                    }
                }
            }
            self.dirty = true;
            return;
        }
        match hit {
            Hit::Close(window) => {
                self.close(&window);
                self.dirty = true;
            }
            Hit::Window(_, window, _) => {
                self.leave_overview();
                if self.space.elements().any(|w| *w == window) {
                    self.space.raise_element(&window, true);
                    self.focus(&window);
                }
            }
            Hit::Frame(name, to) => {
                let shown = self.desks.shown_on(&name);
                if to == shown {
                    self.leave_overview();
                    return;
                }
                if self.desks.per_screen() {
                    self.switch_screen(to, &name);
                } else {
                    self.switch_workspace(to);
                }
                self.dirty = true;
            }
            Hit::Add => self.add_workspace(),
            Hit::Result(i) => self.overview_go(i),
            Hit::Search => {}
            Hit::Arrow(name, forward) => {
                if let Some(screen) = self.overview_screens().into_iter().find(|s| s.name == name) {
                    let step = screen.plan.shows.saturating_sub(1).max(1);
                    let first = if forward {
                        screen.plan.first + step
                    } else {
                        screen.plan.first.saturating_sub(step)
                    };
                    self.overview_scroll_to(&name, first);
                }
            }
            Hit::Tray => {}
            Hit::Nothing => self.leave_overview(),
        }
    }

    /// The plus in the strip: one workspace more, written to the person's
    /// settings file as `workspaces.count`, as Settings' Workspaces page
    /// writes it; the file's watch then adds it here.
    fn add_workspace(&mut self) {
        let count = self.desks.count() + 1;
        let Some(path) = edel::places::person_settings() else {
            eprintln!(
                "edel-compositor: {}",
                edel_compositor::messages::workspace_not_added("no settings file for this person")
            );
            return;
        };
        match edel::settings::write(&path, "workspaces.count", Some(&count.to_string())) {
            Ok(()) => eprintln!("edel-compositor: overview added workspace {count}"),
            Err(e) => eprintln!(
                "edel-compositor: {}",
                edel_compositor::messages::workspace_not_added(format!("{e:#}"))
            ),
        }
    }

    /// What the overview draws on `output` at `scale`, front to back: the
    /// panels, the held window, the close button and names, the spread
    /// windows, the strip, the tray, the dimmed wallpaper.
    pub fn overview_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        scale: f64,
    ) -> Vec<Drawn> {
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let name = output.name();
        let screens = self.overview_screens();
        let Some(screen) = screens.into_iter().find(|s| s.name == name) else {
            return Vec::new();
        };
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let tokens = self.tokens.clone();
        let Some(mut overview) = self.overview.take() else {
            return Vec::new();
        };
        self.log_places(&mut overview, &screen);
        let moving = overview.drag.as_ref().filter(|d| d.moving);
        let held = moving.and_then(|d| match &d.held {
            Held::Window(window) => Some((window.clone(), d.grip)),
            Held::Frame(_) => None,
        });
        let held_frame = moving.and_then(|d| match d.held {
            Held::Frame(i) if d.screen == screen.name => Some((i, d.grip)),
            _ => None,
        });
        // Where a held window or workspace would land, lit.
        let target = moving
            .is_some()
            .then(|| frame_at(&screen.plan.frames, pointer))
            .flatten();
        let hidden = self.hidden_layers();
        let mut front: Vec<Drawn> =
            crate::layers::elements(renderer, output, self.layers_over(output), scale, &hidden)
                .into_iter()
                .map(|e| Drawn::Plain(Element::Surface(e)))
                .collect();
        // The window held, at the pointer, over everything else.
        if let Some((window, grip)) = &held {
            if let Some((_, place, at)) = screen.spread.iter().find(|(w, _, _)| w == window) {
                let moved = Rectangle::new((pointer - *grip).to_i32_round(), at.size);
                if area.overlaps(moved) {
                    front.extend(self.overview_window(
                        renderer,
                        &mut overview,
                        window,
                        *place,
                        moved,
                        area,
                        false,
                        scale,
                    ));
                }
            }
        }
        front.extend(self.overview_search(renderer, &mut overview, &screen, area, scale));
        // The workspace held, at the pointer, with its windows small.
        if let Some((i, grip)) = held_frame {
            if let Some(frame) = screen.plan.frames.get(i) {
                let moved = Rectangle::new((pointer - grip).to_i32_round(), frame.size);
                for (_, window, place) in screen.windows.iter().rev().filter(|(d, _, _)| *d == i) {
                    let to = edel_compositor::overview::shrink(moved, screen.area, *place);
                    front.extend(self.overview_window(
                        renderer,
                        &mut overview,
                        window,
                        *place,
                        to,
                        area,
                        true,
                        scale,
                    ));
                }
                let bar = frame_bar(&screen.plan, screen.area, &tokens);
                let shows = format!("{:?} {bar} {scale} {:?}", frame.size, tokens.backdrop);
                if overview.held.as_ref().is_none_or(|(p, _)| p.shows != shows) {
                    overview.held =
                        glance_look::frame(frame.size.w, frame.size.h, bar, scale, &tokens)
                            .map(|(pixmap, margin)| (painted(shows, &pixmap), margin));
                }
                if let Some((painted, margin)) = &overview.held {
                    let out = (f64::from(*margin) / scale).ceil() as i32;
                    let place = Rectangle::new(
                        moved.loc - Point::from((out, out)),
                        (moved.size.w + 2 * out, moved.size.h + 2 * out).into(),
                    );
                    front.extend(placed(renderer, painted, place, area, scale));
                }
            }
        }
        // The window under the pointer: its close button and its edge.
        let over = (held.is_none() && held_frame.is_none())
            .then(|| {
                screen
                    .spread
                    .iter()
                    .rev()
                    .find(|(_, _, at)| reach(*at).to_f64().contains(pointer))
            })
            .flatten();
        if let Some((_, _, at)) = over {
            front.extend(self.overview_close(
                renderer,
                &mut overview,
                close_rect(*at),
                area,
                scale,
            ));
            let shows = format!("{:?} {scale} {:?}", at.size, tokens.accent);
            if overview.ring.as_ref().is_none_or(|(p, _)| p.shows != shows) {
                overview.ring = glance_look::ring(at.size.w, at.size.h, scale, &tokens)
                    .map(|(pixmap, margin)| (painted(shows, &pixmap), margin));
            }
            if let Some((painted, margin)) = &overview.ring {
                let out = f64::from(*margin) / scale;
                let place = Rectangle::new(
                    (at.loc.to_f64() - Point::from((out, out))).to_i32_round(),
                    (at.size.to_f64() + Size::from((2.0 * out, 2.0 * out))).to_i32_round(),
                );
                front.extend(placed(renderer, painted, place, area, scale));
            }
        }
        // Each spread window's name under it, then the windows.
        for (window, _, at) in &screen.spread {
            if held.as_ref().is_some_and(|(h, _)| h == window) {
                continue;
            }
            front.extend(self.overview_name(renderer, &mut overview, window, *at, area, scale));
        }
        for (window, place, at) in screen.spread.iter().rev() {
            if held.as_ref().is_some_and(|(h, _)| h == window) {
                continue;
            }
            front.extend(self.overview_window(
                renderer,
                &mut overview,
                window,
                *place,
                *at,
                area,
                false,
                scale,
            ));
            front.extend(overview_shadow(
                renderer,
                &mut overview,
                window,
                *at,
                area,
                scale,
                &tokens,
            ));
        }
        // The strip: each workspace's frame, its windows small, its edge.
        let shrink = |frame: Rectangle<i32, Logical>, place: Rectangle<i32, Logical>| {
            edel_compositor::overview::shrink(frame, screen.area, place)
        };
        let marks: Vec<Mark> = (0..screen.plan.frames.len())
            .map(|i| {
                if held_frame.is_some_and(|(h, _)| h == i) {
                    Mark::Away
                } else if i == screen.shown || target == Some(i) {
                    Mark::Lit
                } else {
                    Mark::Plain
                }
            })
            .collect();
        for (i, frame) in screen.plan.frames.iter().enumerate() {
            // Scrolled out of view, or held by the pointer.
            if frame.is_empty() || marks[i] == Mark::Away {
                continue;
            }
            // The panel's strip at the frame's bottom stays clear.
            for (_, window, place) in screen.windows.iter().rev().filter(|(d, _, _)| *d == i) {
                front.extend(self.overview_window(
                    renderer,
                    &mut overview,
                    window,
                    *place,
                    shrink(*frame, *place),
                    area,
                    true,
                    scale,
                ));
            }
        }
        front.extend(self.overview_tray(renderer, &mut overview, &screen, &marks, scale));
        // The wallpaper under it all, dimmed a little; the backdrop where
        // there is none.
        front.push(solid(
            &mut overview,
            "veil".into(),
            area,
            Colour {
                r: 0.04,
                g: 0.05,
                b: 0.08,
                a: 0.12,
            },
            area,
            scale,
        ));
        front.extend(
            crate::layers::elements(renderer, output, &crate::layers::BELOW, scale, &hidden)
                .into_iter()
                .map(|e| Drawn::Plain(Element::Surface(e))),
        );
        let pixels = (
            (f64::from(area.size.w) * scale).ceil() as u32,
            (f64::from(area.size.h) * scale).ceil() as u32,
        );
        let shows = format!(
            "{pixels:?} {:?} {:?}",
            tokens.backdrop, tokens.backdrop_deep
        );
        if overview
            .backdrops
            .get(&name)
            .is_none_or(|p| p.shows != shows)
        {
            if let Some(pixmap) = glance_look::backdrop(pixels.0, pixels.1, &tokens) {
                overview
                    .backdrops
                    .insert(name.clone(), painted(shows, &pixmap));
            }
        }
        if let Some(backdrop) = overview.backdrops.get(&name) {
            front.extend(placed(renderer, backdrop, area, area, scale));
        }
        self.overview = Some(overview);
        front
    }

    /// The search field over the stage and, while something matches, the
    /// card of results under it, each painted again only when what it
    /// shows changes.
    fn overview_search(
        &mut self,
        renderer: &mut GlesRenderer,
        overview: &mut Overview,
        screen: &Screen,
        area: Rectangle<i32, Logical>,
        scale: f64,
    ) -> Vec<Drawn> {
        let field = screen.plan.search;
        if field.is_empty() {
            return Vec::new();
        }
        let mut drawn = Vec::new();
        let rows = overview.search.found.len();
        if rows > 0 {
            let names: Vec<(String, String, bool)> = overview
                .search
                .found
                .iter()
                .map(|f| (f.name(), f.app(), f.is_window()))
                .collect();
            let (window_word, app_word) = (tr("Window"), tr("App"));
            let shows = format!(
                "{names:?} {} {} {scale} {:?} {:?}",
                overview.search.chosen, field.size.w, self.tokens.panel, self.tokens.accent
            );
            if overview.card.as_ref().is_none_or(|p| p.shows != shows) {
                let px = (20.0 * scale).round() as u32;
                let icons: Vec<Option<tiny_skia::Pixmap>> = names
                    .iter()
                    .map(|(_, app, _)| {
                        let name = self.app_icons.name(app)?;
                        self.app_icons.picture(&name, px).cloned()
                    })
                    .collect();
                let rows: Vec<glance_look::Row> = names
                    .iter()
                    .zip(&icons)
                    .map(|((name, _, window), icon)| glance_look::Row {
                        icon: icon.as_ref(),
                        name,
                        kind: if *window { window_word } else { app_word },
                    })
                    .collect();
                if let Some(text) = self.text.as_mut() {
                    overview.card = glance_look::results(
                        &rows,
                        overview.search.chosen,
                        field.size.w,
                        scale,
                        &self.tokens,
                        text,
                    )
                    .map(|pixmap| painted(shows, &pixmap));
                }
            }
            if let Some(card) = &overview.card {
                let place = glance_search::card(field, rows);
                drawn.extend(placed(renderer, card, place, area, scale));
            }
        }
        let placeholder = tr("Type to search");
        let shows = format!(
            "{} {placeholder} {:?} {scale} {:?}",
            overview.search.query, field.size, self.tokens.panel
        );
        if overview.field.as_ref().is_none_or(|p| p.shows != shows) {
            if let Some(text) = self.text.as_mut() {
                overview.field = glance_look::search(
                    field.size.w,
                    field.size.h,
                    &overview.search.query,
                    placeholder,
                    scale,
                    &self.tokens,
                    text,
                )
                .map(|pixmap| painted(shows, &pixmap));
            }
        }
        if let Some(painted) = &overview.field {
            drawn.extend(placed(renderer, painted, field, area, scale));
        }
        drawn
    }

    /// Logs where the overview's parts lie on `screen` when that changes,
    /// for CI to click them: `overview places NAME SIDE, tray X+Y+WxH,
    /// frame1 X+Y+WxH, ..., add X+Y+WxH, search X+Y+WxH, before X+Y+WxH, after X+Y+WxH,
    /// window TITLE X+Y+WxH, ...`, the frames out of view and the arrows
    /// not there left out.
    fn log_places(&self, overview: &mut Overview, screen: &Screen) {
        let at = |r: Rectangle<i32, Logical>| {
            format!("{}+{}+{}x{}", r.loc.x, r.loc.y, r.size.w, r.size.h)
        };
        let side = self.settings.overview_strip();
        let mut parts = vec![
            format!("{} {side}", screen.name),
            format!("tray {}", at(screen.plan.tray)),
        ];
        for (i, frame) in screen.plan.frames.iter().enumerate() {
            if !frame.is_empty() {
                parts.push(format!("frame{} {}", i + 1, at(*frame)));
            }
        }
        if let Some(add) = screen.plan.add {
            parts.push(format!("add {}", at(add)));
        }
        if !screen.plan.search.is_empty() {
            parts.push(format!("search {}", at(screen.plan.search)));
        }
        for (word, arrow) in [("before", screen.plan.before), ("after", screen.plan.after)] {
            if let Some(arrow) = arrow {
                parts.push(format!("{word} {}", at(arrow)));
            }
        }
        for (window, _, spread) in &screen.spread {
            parts.push(format!("window {} {}", title(window), at(*spread)));
        }
        let line = parts.join(", ");
        if overview.logged.get(&screen.name) != Some(&line) {
            eprintln!("edel-compositor: overview places {line}");
            overview.logged.insert(screen.name.clone(), line);
        }
    }

    /// The tray of `screen`, painted again only when its labels, its
    /// plan, the scale or the colours change.
    fn overview_tray(
        &mut self,
        renderer: &mut GlesRenderer,
        overview: &mut Overview,
        screen: &Screen,
        marks: &[Mark],
        scale: f64,
    ) -> Vec<Drawn> {
        let names = self.desks.names();
        let labels: Vec<String> = (0..screen.plan.frames.len())
            .map(|i| match names.get(i).filter(|n| !n.is_empty()) {
                Some(name) => format!("{} \u{b7} {name}", i + 1),
                None => (i + 1).to_string(),
            })
            .collect();
        let new = tr("New");
        let bar = frame_bar(&screen.plan, screen.area, &self.tokens);
        let shows = format!(
            "{:?} {labels:?} {marks:?} {bar} {new} {scale} {:?} {:?} {}",
            screen.plan,
            self.tokens.panel,
            self.tokens.backdrop,
            self.text.is_some()
        );
        let stale = overview
            .trays
            .get(&screen.name)
            .is_none_or(|p| p.shows != shows);
        if stale {
            let frames: Vec<(String, Mark)> =
                labels.into_iter().zip(marks.iter().copied()).collect();
            let Some(pixmap) = glance_look::tray(
                &screen.plan,
                &frames,
                bar,
                new,
                scale,
                &self.tokens,
                self.text.as_mut(),
            ) else {
                return Vec::new();
            };
            overview
                .trays
                .insert(screen.name.clone(), painted(shows, &pixmap));
        }
        let Some(painted) = overview.trays.get(&screen.name) else {
            return Vec::new();
        };
        placed(renderer, painted, screen.plan.tray, screen.area, scale)
    }

    /// `window`'s name pill under its spread picture at `at`.
    fn overview_name(
        &mut self,
        renderer: &mut GlesRenderer,
        overview: &mut Overview,
        window: &Window,
        at: Rectangle<i32, Logical>,
        area: Rectangle<i32, Logical>,
        scale: f64,
    ) -> Vec<Drawn> {
        let words = title(window);
        let icon_name = self.app_icons.name(&app_id(window));
        let most = (at.size.w + 40).max(120);
        let shows = format!(
            "{words} {icon_name:?} {most} {scale} {:?}",
            self.tokens.panel
        );
        let stale = overview.names.get(window).is_none_or(|p| p.shows != shows);
        if stale {
            let px = (f64::from(NAME_ICON) * scale).round() as u32;
            let icon = icon_name
                .as_deref()
                .and_then(|name| self.app_icons.picture(name, px))
                .cloned();
            let Some(text) = self.text.as_mut() else {
                return Vec::new();
            };
            let Some(pixmap) =
                glance_look::name(&words, icon.as_ref(), most, scale, &self.tokens, text)
            else {
                return Vec::new();
            };
            overview
                .names
                .insert(window.clone(), painted(shows, &pixmap));
        }
        let Some(painted) = overview.names.get(window) else {
            return Vec::new();
        };
        let w = (f64::from(painted.pixels.0) / scale).round() as i32;
        let place = Rectangle::new(
            (
                at.loc.x + (at.size.w - w) / 2,
                at.loc.y + at.size.h + PILL_GAP,
            )
                .into(),
            (w, PILL).into(),
        );
        placed(renderer, painted, place, area, scale)
    }

    /// The close button at `place`.
    fn overview_close(
        &mut self,
        renderer: &mut GlesRenderer,
        overview: &mut Overview,
        place: Rectangle<i32, Logical>,
        area: Rectangle<i32, Logical>,
        scale: f64,
    ) -> Vec<Drawn> {
        let shows = format!(
            "{scale} {:?} {:?}",
            self.tokens.panel, self.tokens.panel_text
        );
        if overview.close.as_ref().is_none_or(|p| p.shows != shows) {
            let Some(pixmap) = glance_look::close(scale, &self.tokens) else {
                return Vec::new();
            };
            overview.close = Some(painted(shows, &pixmap));
        }
        let Some(painted) = &overview.close else {
            return Vec::new();
        };
        placed(renderer, painted, place, area, scale)
    }

    /// `window`, whose frame on the screen is `place`, drawn into `to` (a
    /// rect of `place`'s shape) on the screen at `area`; `small` for the
    /// strip's frames, whose pictures are kept apart from the stage's.
    #[allow(clippy::too_many_arguments)]
    fn overview_window(
        &mut self,
        renderer: &mut GlesRenderer,
        overview: &mut Overview,
        window: &Window,
        place: Rectangle<i32, Logical>,
        to: Rectangle<i32, Logical>,
        area: Rectangle<i32, Logical>,
        small: bool,
        scale_out: f64,
    ) -> Vec<Drawn> {
        let k = f64::from(to.size.w) / f64::from(place.size.w.max(1));
        let at = |p: Point<f64, Logical>| {
            (to.loc.to_f64() + (p - place.loc.to_f64()).upscale(k) - area.loc.to_f64())
                .to_physical(scale_out)
        };
        let insets = self.insets(window);
        let inner = insets.window(place);
        let origin = (inner.loc - window.geometry().loc).to_f64();
        let context = renderer.context_id();
        let mut parts: Vec<Drawn> = Vec::new();
        // Every window's title bar alike, painted here, so a window
        // without the keyboard looks no different from the one with it.
        if insets.top > 0 {
            let size = Size::<f64, Logical>::from((f64::from(place.size.w), f64::from(insets.top)))
                .upscale(k)
                .to_i32_round();
            let pixels = (
                (f64::from(size.w) * scale_out).ceil() as u32,
                (f64::from(size.h) * scale_out).ceil() as u32,
            );
            let words = title(window);
            let title_size = self.tokens.title_text_size as f32 * (k * scale_out) as f32;
            let shows = format!(
                "{words} {pixels:?} {title_size} {:?}",
                self.tokens.title_bar_focused
            );
            let key = (window.clone(), small);
            if overview.bars.get(&key).is_none_or(|p| p.shows != shows) {
                if let Some(pixmap) = glance_look::bar(
                    pixels.0,
                    pixels.1,
                    &words,
                    title_size,
                    &self.tokens,
                    self.text.as_mut(),
                ) {
                    overview.bars.insert(key.clone(), painted(shows, &pixmap));
                }
            }
            if let Some(bar) = overview.bars.get(&key) {
                parts.extend(placed(
                    renderer,
                    bar,
                    Rectangle::new(to.loc, size),
                    area,
                    scale_out,
                ));
            }
        }
        let frame_data = data(window).borrow();
        for (n, part) in frame_data.picture.iter().enumerate().rev() {
            let id = overview
                .ids
                .entry((window.clone(), small, n))
                .or_insert_with(Id::new)
                .clone();
            let size = part.size.to_f64().upscale(k).to_i32_round();
            parts.push(Drawn::Plain(Element::Picture(
                TextureRenderElement::from_static_texture(
                    id,
                    context.clone(),
                    at(origin + part.at.to_f64()),
                    part.texture.clone(),
                    part.scale,
                    part.transform,
                    None,
                    Some(part.src),
                    Some(size),
                    None,
                    Kind::Unspecified,
                ),
            )));
        }
        let none_drawn = frame_data.picture.is_empty();
        drop(frame_data);
        // A window never drawn yet shows as a plain card in its place.
        if none_drawn {
            let buffer = overview.cards.entry((window.clone(), small)).or_default();
            parts.push(flat(buffer, to, self.tokens.title_bar, area, scale_out));
        }
        parts
    }
}

/// `pixmap` as a buffer to draw, with what it shows.
fn painted(shows: String, pixmap: &tiny_skia::Pixmap) -> Painted {
    let pixels = (pixmap.width() as i32, pixmap.height() as i32);
    Painted {
        shows,
        buffer: MemoryRenderBuffer::from_slice(
            pixmap.data(),
            Fourcc::Abgr8888,
            pixels,
            1,
            Transform::Normal,
            None,
        ),
        pixels,
    }
}

/// `painted` drawn over `place` on the screen at `area`.
fn placed(
    renderer: &mut GlesRenderer,
    painted: &Painted,
    place: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
    scale: f64,
) -> Vec<Drawn> {
    let at = (place.loc - area.loc).to_f64().to_physical(scale);
    match MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        at,
        &painted.buffer,
        None,
        Some(Rectangle::from_size(
            (f64::from(painted.pixels.0), f64::from(painted.pixels.1)).into(),
        )),
        Some(place.size),
        Kind::Unspecified,
    ) {
        Ok(element) => vec![Drawn::Plain(Element::Bar(element))],
        Err(e) => {
            eprintln!("edel-compositor: drawing the overview failed: {e}");
            Vec::new()
        }
    }
}

/// A flat `colour` over `place` on the screen at `area`, kept as `key`.
fn solid(
    overview: &mut Overview,
    key: String,
    place: Rectangle<i32, Logical>,
    colour: Colour,
    area: Rectangle<i32, Logical>,
    scale: f64,
) -> Drawn {
    flat(
        overview.solids.entry(key).or_default(),
        place,
        colour,
        area,
        scale,
    )
}

/// `buffer` filled with `colour` over `place` on the screen at `area`.
fn flat(
    buffer: &mut SolidColorBuffer,
    place: Rectangle<i32, Logical>,
    colour: Colour,
    area: Rectangle<i32, Logical>,
    scale: f64,
) -> Drawn {
    buffer.update(place.size, colour.rgba());
    let at = (place.loc - area.loc)
        .to_f64()
        .to_physical(scale)
        .to_i32_round();
    Drawn::Plain(Element::Border(SolidColorRenderElement::from_buffer(
        buffer,
        at,
        scale,
        1.0,
        Kind::Unspecified,
    )))
}

/// The height of the panel along a frame's bottom, logical pixels: the
/// panel's share of the screen, as small as the frame.
fn frame_bar(plan: &Plan, screen: Rectangle<i32, Logical>, tokens: &edel::tokens::Tokens) -> f32 {
    let frame_h = plan.frames.iter().map(|f| f.size.h).max().unwrap_or(0);
    (frame_h as f32 * tokens.panel_height as f32 / screen.size.h.max(1) as f32).max(2.0)
}

/// The soft shadow under `window` spread at `at`, painted again only
/// when its size changes.
fn overview_shadow(
    renderer: &mut GlesRenderer,
    overview: &mut Overview,
    window: &Window,
    at: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
    scale: f64,
    tokens: &edel::tokens::Tokens,
) -> Vec<Drawn> {
    let shows = format!("{:?} {scale} {:?}", at.size, tokens.shadow);
    if overview
        .shadows
        .get(window)
        .is_none_or(|(p, _)| p.shows != shows)
    {
        match glance_look::shadow(at.size.w, at.size.h, scale, tokens) {
            Some((pixmap, margin)) => {
                overview
                    .shadows
                    .insert(window.clone(), (painted(shows, &pixmap), margin));
            }
            None => return Vec::new(),
        }
    }
    let Some((painted, margin)) = overview.shadows.get(window) else {
        return Vec::new();
    };
    let out = f64::from(*margin) / scale;
    let place = Rectangle::new(
        (at.loc.to_f64() - Point::from((out, out))).to_i32_round(),
        (at.size.to_f64() + Size::from((2.0 * out, 2.0 * out))).to_i32_round(),
    );
    placed(renderer, painted, place, area, scale)
}
