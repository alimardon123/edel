//! The overview on screen (roadmap M5.2j; laid out anew in M5.2j-b after
//! `docs/mockups/shell/overview.jpg`): Super+W, or a click on the panel
//! switcher's lit workspace, shows on each screen every workspace small in
//! a strip on a tray along one side (`workspaces.overview_strip`, the left
//! by default) and the shown workspace's windows spread out large on the
//! rest, each with its app and title under it and a close button while the
//! pointer is over it. The panels stay; the wallpaper is dimmed. A click on
//! a window goes to it; a click on a small workspace shows its windows
//! here; a window dragged onto a small workspace moves there; the frame
//! with a plus adds a workspace; a click on nothing, Escape or Super+W
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
use edel_compositor::overview::{Plan, Side, frame_at, plan, spread};

use crate::decoration::{app_id, data, title};
use crate::glance_look::{self, CLOSE, NAME_ICON, PILL};
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
    /// The window held by the pointer, if any.
    drag: Option<Drag>,
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

/// A window picked up in the overview.
struct Drag {
    window: Window,
    /// Its screen, by name.
    screen: String,
    /// Where it was pressed, and the pointer from the picture's corner.
    from: Point<f64, Logical>,
    grip: Point<f64, Logical>,
    /// Whether the pointer moved far enough for a drag.
    moving: bool,
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
                Some(Screen {
                    shown: self.desks.shown_on(&name),
                    name,
                    area,
                    plan: plan(area, free, count, add, side),
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
        if screen.plan.tray.to_f64().contains(point) {
            return Hit::Tray;
        }
        Hit::Nothing
    }

    /// A button went down while the overview is open: on a window it may
    /// become a drag; see `overview_release` for what a click does.
    pub fn overview_press(&mut self, point: Point<f64, Logical>) {
        let held = match self.overview_hit(point) {
            Hit::Window(screen, window, at) => Some((screen, window, at)),
            _ => None,
        };
        let Some(overview) = &mut self.overview else {
            return;
        };
        overview.drag = held.map(|(screen, window, at)| Drag {
            window,
            screen,
            from: point,
            grip: point - at.loc.to_f64(),
            moving: false,
        });
    }

    /// The pointer moved while the overview is open: a held window
    /// follows, and the close button follows the window under it.
    pub fn overview_motion(&mut self, point: Point<f64, Logical>) {
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
    /// shown one leaves to it); on the plus adds a workspace; on the tray
    /// does nothing; on nothing leaves.
    pub fn overview_release(&mut self, point: Point<f64, Logical>) {
        let Some(overview) = &mut self.overview else {
            return;
        };
        let drag = overview.drag.take();
        let hit = self.overview_hit(point);
        if let Some(drag) = drag.as_ref().filter(|d| d.moving) {
            if let Hit::Frame(name, to) = hit {
                if name == drag.screen {
                    self.carry_window(&drag.window, to);
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
        let held = overview
            .drag
            .as_ref()
            .filter(|d| d.moving)
            .map(|d| (d.window.clone(), d.grip));
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
        // The window under the pointer: its close button and its edge.
        let over = held
            .is_none()
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
            for (j, line) in edges(*at, 2).into_iter().enumerate() {
                front.push(solid(
                    &mut overview,
                    format!("over {j}"),
                    line,
                    tokens.accent,
                    area,
                    scale,
                ));
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
        }
        // The strip: each workspace's frame, its windows small, its edge.
        let shrink = |frame: Rectangle<i32, Logical>, place: Rectangle<i32, Logical>| {
            edel_compositor::overview::shrink(frame, screen.area, place)
        };
        for (i, frame) in screen.plan.frames.iter().enumerate() {
            let (edge, width) = if i == screen.shown {
                (tokens.accent, 2)
            } else {
                (tokens.edge, 1)
            };
            for (j, line) in edges(*frame, width).into_iter().enumerate() {
                front.push(solid(
                    &mut overview,
                    format!("edge {i} {j}"),
                    line,
                    edge,
                    area,
                    scale,
                ));
            }
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
            front.push(solid(
                &mut overview,
                format!("frame {i}"),
                *frame,
                tokens.background,
                area,
                scale,
            ));
        }
        front.extend(self.overview_tray(renderer, &mut overview, &screen, scale));
        // The wallpaper under it all, dimmed; the background colour where
        // there is none.
        front.push(solid(
            &mut overview,
            "veil".into(),
            area,
            Colour {
                r: 0.04,
                g: 0.05,
                b: 0.08,
                a: 0.32,
            },
            area,
            scale,
        ));
        front.extend(
            crate::layers::elements(renderer, output, &crate::layers::BELOW, scale, &hidden)
                .into_iter()
                .map(|e| Drawn::Plain(Element::Surface(e))),
        );
        front.push(solid(
            &mut overview,
            "screen".into(),
            area,
            tokens.background,
            area,
            scale,
        ));
        self.overview = Some(overview);
        front
    }

    /// Logs where the overview's parts lie on `screen` when that changes,
    /// for CI to click them: `overview places NAME SIDE, tray X+Y+WxH,
    /// frame1 X+Y+WxH, ..., add X+Y+WxH, window TITLE X+Y+WxH, ...`.
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
            parts.push(format!("frame{} {}", i + 1, at(*frame)));
        }
        if let Some(add) = screen.plan.add {
            parts.push(format!("add {}", at(add)));
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
        let shows = format!(
            "{:?} {labels:?} {new} {scale} {:?} {}",
            screen.plan,
            self.tokens.panel,
            self.text.is_some()
        );
        let stale = overview
            .trays
            .get(&screen.name)
            .is_none_or(|p| p.shows != shows);
        if stale {
            let Some(pixmap) = glance_look::tray(
                &screen.plan,
                &labels,
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
        let frame_data = data(window).borrow();
        let mut parts: Vec<Drawn> = Vec::new();
        if let Some((buffer, pixels)) = (insets.top > 0).then(|| frame_data.last_bar()).flatten() {
            let size = Size::<f64, Logical>::from((f64::from(place.size.w), f64::from(insets.top)))
                .upscale(k)
                .to_i32_round();
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                at(place.loc.to_f64()),
                &buffer,
                None,
                Some(Rectangle::from_size(
                    (f64::from(pixels.0), f64::from(pixels.1)).into(),
                )),
                Some(size),
                Kind::Unspecified,
            ) {
                Ok(element) => parts.push(Drawn::Plain(Element::Bar(element))),
                Err(e) => eprintln!("edel-compositor: drawing a title bar failed: {e}"),
            }
        }
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

/// The four edges of `frame`, `width` wide, inside it.
fn edges(frame: Rectangle<i32, Logical>, width: i32) -> [Rectangle<i32, Logical>; 4] {
    let (x, y, w, h) = (frame.loc.x, frame.loc.y, frame.size.w, frame.size.h);
    [
        Rectangle::new((x, y).into(), (w, width).into()),
        Rectangle::new((x, y + h - width).into(), (w, width).into()),
        Rectangle::new((x, y).into(), (width, h).into()),
        Rectangle::new((x + w - width, y).into(), (width, h).into()),
    ]
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
