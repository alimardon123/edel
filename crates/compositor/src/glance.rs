//! The overview on screen (roadmap M5.2j): Super+W shows every workspace
//! of each screen side by side, each as a frame of the screen's shape with
//! its windows drawn smaller where they lie, the shown one edged in the
//! accent. A click on a frame shows that workspace and leaves; a window
//! dragged to another frame moves there; Escape or Super+W again leaves.
//! The pictures are those each window last showed, so nothing is drawn
//! again until something changes. `edel_compositor::overview` holds the
//! geometry.

use std::collections::HashMap;

use smithay::backend::renderer::Renderer;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::{SolidColorBuffer, SolidColorRenderElement};
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::Window;
use smithay::output::Output;
use smithay::utils::{Logical, Point, Rectangle};

use edel::tokens::Colour;
use edel_compositor::overview::{frame_at, frames, into, scale, shrink};

use crate::decoration::data;
use crate::render::{Drawn, Element};
use crate::state::Edel;

/// How far the pointer moves, in logical pixels, before a press on a
/// window becomes a drag rather than a click.
const DRAG_FROM: f64 = 6.0;

/// The overview while it is open.
#[derive(Default)]
pub struct Overview {
    /// The window held by the pointer, if any.
    drag: Option<Drag>,
    /// The flat colours drawn, each kept by what it is, so a frame drawn
    /// again unchanged is not redrawn.
    solids: HashMap<String, SolidColorBuffer>,
    /// The ids of the windows' pictures, by window and surface.
    ids: HashMap<(Window, usize), Id>,
    /// The plain cards of windows never drawn yet.
    cards: HashMap<Window, SolidColorBuffer>,
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

/// One screen in the overview: its area, its workspaces' frames and the
/// windows of each, bottom first, with their frames on the screen.
pub struct Screen {
    pub name: String,
    pub area: Rectangle<i32, Logical>,
    pub frames: Vec<Rectangle<i32, Logical>>,
    pub windows: Vec<(usize, Window, Rectangle<i32, Logical>)>,
}

impl Edel {
    /// Super+W: shows the overview, or leaves it.
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
        let mut screens: Vec<Screen> = self
            .space
            .outputs()
            .filter_map(|output| {
                let area = self.space.output_geometry(output)?;
                Some(Screen {
                    name: output.name(),
                    area,
                    frames: frames(area, count),
                    windows: Vec::new(),
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
                .iter_mut()
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
        screens
    }

    /// The window drawn under `point` in the overview, with its screen and
    /// its picture's frame there, the top one first.
    fn overview_window_at(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(String, Window, Rectangle<i32, Logical>)> {
        self.overview_screens().into_iter().find_map(|screen| {
            screen.windows.iter().rev().find_map(|(desk, window, frame)| {
                let at = shrink(screen.frames[*desk], screen.area, *frame);
                at.to_f64()
                    .contains(point)
                    .then(|| (screen.name.clone(), window.clone(), at))
            })
        })
    }

    /// A button went down while the overview is open: on a window it may
    /// become a drag; on a frame, or after a click, see `overview_release`.
    pub fn overview_press(&mut self, point: Point<f64, Logical>) {
        let held = self.overview_window_at(point);
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

    /// The pointer moved while the overview is open: a held window follows.
    pub fn overview_motion(&mut self, point: Point<f64, Logical>) {
        let Some(drag) = self.overview.as_mut().and_then(|o| o.drag.as_mut()) else {
            return;
        };
        let moved = point - drag.from;
        if moved.x.hypot(moved.y) >= DRAG_FROM {
            drag.moving = true;
        }
        self.dirty = true;
    }

    /// The button came up while the overview is open. A window dragged to
    /// another frame of its screen moves to that workspace and the
    /// overview stays; a click shows the workspace under it, with the
    /// window clicked on top, and leaves; a click outside every frame
    /// leaves.
    pub fn overview_release(&mut self, point: Point<f64, Logical>) {
        let Some(overview) = &mut self.overview else {
            return;
        };
        let drag = overview.drag.take();
        let screens = self.overview_screens();
        let target = screens
            .iter()
            .find(|s| s.area.to_f64().contains(point))
            .and_then(|s| Some((s.name.clone(), frame_at(&s.frames, point)?)));
        if let Some(drag) = drag.as_ref().filter(|d| d.moving) {
            if let Some((_, to)) = target.filter(|(name, _)| *name == drag.screen) {
                self.carry_window(&drag.window, to);
            }
            self.dirty = true;
            return;
        }
        let Some((name, to)) = target else {
            self.leave_overview();
            return;
        };
        self.leave_overview();
        if self.desks.per_screen() {
            self.switch_screen(to, &name);
        } else {
            self.switch_workspace(to);
        }
        if let Some(drag) = drag {
            if self.space.elements().any(|w| *w == drag.window) {
                self.space.raise_element(&drag.window, true);
                self.focus(&drag.window);
            }
        }
    }

    /// What the overview draws on `output` at `scale`, front to back.
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
        let shown = self.desks.shown_on(&name);
        let tokens = self.tokens.clone();
        let Some(mut overview) = self.overview.take() else {
            return Vec::new();
        };
        let held = overview
            .drag
            .as_ref()
            .filter(|d| d.moving)
            .map(|d| (d.window.clone(), d.grip));
        let mut front = Vec::new();
        let mut back = Vec::new();
        // The window held, at the pointer, over everything.
        if let Some((window, grip)) = &held {
            if let Some((desk, _, frame)) = screen.windows.iter().find(|(_, w, _)| w == window) {
                let at = shrink(screen.frames[*desk], screen.area, *frame);
                let moved = Rectangle::new((pointer - *grip).to_i32_round(), at.size);
                if screen.area.overlaps(moved) {
                    front.extend(self.overview_window(
                        renderer,
                        &mut overview,
                        window,
                        *frame,
                        screen.frames[*desk],
                        &screen,
                        Some(moved.loc - at.loc),
                        scale,
                    ));
                }
            }
        }
        for (i, frame) in screen.frames.iter().enumerate() {
            let (edge, width) = if i == shown {
                (tokens.accent, 2)
            } else {
                (tokens.edge, 1)
            };
            for (j, line) in edges(*frame, width).into_iter().enumerate() {
                front.push(solid(&mut overview, format!("edge {i} {j}"), line, edge, area, scale));
            }
            let windows = screen
                .windows
                .iter()
                .rev()
                .filter(|(desk, w, _)| *desk == i && held.as_ref().is_none_or(|(h, _)| h != w));
            for (_, window, place) in windows {
                front.extend(self.overview_window(
                    renderer,
                    &mut overview,
                    window,
                    *place,
                    *frame,
                    &screen,
                    None,
                    scale,
                ));
            }
            back.push(solid(&mut overview, format!("frame {i}"), *frame, tokens.background, area, scale));
        }
        back.push(solid(&mut overview, "screen".into(), area, tokens.panel, area, scale));
        self.overview = Some(overview);
        front.extend(back);
        front
    }

    /// `window`, whose frame on the screen is `place`, drawn smaller in
    /// workspace frame `frame`, moved by `by` when held; its parts front
    /// to back.
    #[allow(clippy::too_many_arguments)]
    fn overview_window(
        &mut self,
        renderer: &mut GlesRenderer,
        overview: &mut Overview,
        window: &Window,
        place: Rectangle<i32, Logical>,
        frame: Rectangle<i32, Logical>,
        screen: &Screen,
        by: Option<Point<i32, Logical>>,
        scale_out: f64,
    ) -> Vec<Drawn> {
        let k = scale(frame, screen.area);
        let by = by.unwrap_or_default().to_f64();
        let at = |p: Point<f64, Logical>| {
            (into(frame, screen.area, p) + by - screen.area.loc.to_f64()).to_physical(scale_out)
        };
        let insets = self.insets(window);
        let inner = insets.window(place);
        let origin = (inner.loc - window.geometry().loc).to_f64();
        let context = renderer.context_id();
        let frame_data = data(window).borrow();
        let mut parts: Vec<Drawn> = Vec::new();
        if let Some((buffer, pixels)) = (insets.top > 0).then(|| frame_data.last_bar()).flatten() {
            let size = smithay::utils::Size::<f64, Logical>::from((
                f64::from(place.size.w),
                f64::from(insets.top),
            ))
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
                .entry((window.clone(), n))
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
            let card = shrink(frame, screen.area, place);
            let card = Rectangle::new(card.loc + by.to_i32_round(), card.size);
            let buffer = overview.cards.entry(window.clone()).or_default();
            parts.push(flat(buffer, card, self.tokens.title_bar, screen.area, scale_out));
        }
        parts
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
    flat(overview.solids.entry(key).or_default(), place, colour, area, scale)
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
    let at = (place.loc - area.loc).to_f64().to_physical(scale).to_i32_round();
    Drawn::Plain(Element::Border(SolidColorRenderElement::from_buffer(
        buffer,
        at,
        scale,
        1.0,
        Kind::Unspecified,
    )))
}
