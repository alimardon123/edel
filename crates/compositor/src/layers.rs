//! Panels, docks and backgrounds (roadmap M5.1a): `zwlr_layer_shell_v1`,
//! the protocol shell-ui and other desktop parts draw with. Each screen
//! keeps its layer surfaces in smithay's `LayerMap`, which places them by
//! their anchors and margins; their exclusive zones shrink the area
//! windows are placed, tiled and maximized in. Background and bottom
//! layers are drawn under the windows, top and overlay layers over them,
//! and the pointer meets them in that order. A layer that takes the
//! keyboard gets it when clicked.

use smithay::backend::renderer::element::AsRenderElements;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::{LayerSurface, Window, WindowSurfaceType, layer_map_for_output};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Scale};
use smithay::wayland::compositor::{send_surface_state, with_states};
use smithay::wayland::fractional_scale::with_fractional_scale;
use smithay::wayland::shell::wlr_layer::{
    KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData,
    WlrLayerShellHandler, WlrLayerShellState,
};
use smithay::wayland::shell::xdg::PopupSurface;
use std::time::Duration;
use toml::{Table, Value};

use edel_compositor::desks::screen_in;

use crate::state::Edel;

/// The layers drawn over the windows, the topmost first.
pub const ABOVE: [Layer; 2] = [Layer::Overlay, Layer::Top];
/// shell-ui's dock's namespace (M5.4d), which may hide (M5.4f).
pub const DOCK: &str = "edel-dock";
/// How near the screen's edge the pointer brings a hidden dock back.
const EDGE: f64 = 2.0;
/// The layers drawn under them, the topmost first.
pub const BELOW: [Layer; 2] = [Layer::Bottom, Layer::Background];

impl Edel {
    /// Each screen's area left to windows once panels have taken their
    /// exclusive zones, by the screen's name, the first screen first:
    /// where windows open, tile and maximize, each on its own (M5.2g).
    pub fn window_areas(&self) -> Vec<(String, Rectangle<i32, Logical>)> {
        self.space
            .outputs()
            .filter_map(|output| {
                let screen = self.space.output_geometry(output)?;
                let mut zone = layer_map_for_output(output).non_exclusive_zone();
                zone.loc += screen.loc;
                Some((output.name(), zone))
            })
            .collect()
    }

    /// The first screen's area left to windows.
    pub fn window_area(&self) -> Option<Rectangle<i32, Logical>> {
        self.window_areas().into_iter().next().map(|(_, area)| area)
    }

    /// The screen at `point`, by name, with its area; else the first.
    pub fn screen_at(
        &self,
        point: Point<f64, Logical>,
    ) -> Option<(String, Rectangle<i32, Logical>)> {
        let areas = self.window_areas();
        let name = self.space.output_under(point).next().map(|o| o.name());
        screen_in(&areas, name.as_deref()).map(|(name, area)| (name.to_string(), area))
    }

    /// The screen the pointer is on, where a new window opens.
    pub fn pointer_screen(&self) -> Option<(String, Rectangle<i32, Logical>)> {
        let at = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        self.screen_at(at)
    }

    /// The screen `window` lies on in the shown workspace, with its area;
    /// else the first.
    pub fn home(&self, window: &Window) -> Option<(String, Rectangle<i32, Logical>)> {
        let areas = self.window_areas();
        let name = self.desks.layout().screen_of(window);
        screen_in(&areas, name).map(|(name, area)| (name.to_string(), area))
    }

    /// Places every screen's layers again and tells them its scale, after
    /// a screen came, went, or changed size or scale.
    pub fn screens_changed_for_layers(&mut self) {
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            map.arrange();
            for layer in map.layers() {
                send_scale(output, layer);
            }
        }
    }

    /// A commit to a layer surface: the screen's layers are placed again,
    /// the surface hears its first configure, and windows make room when
    /// an exclusive zone changed. Returns false for other surfaces.
    pub fn layer_commit(&mut self, surface: &WlSurface) -> bool {
        let Some(output) = self
            .space
            .outputs()
            .find(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            })
            .cloned()
        else {
            return false;
        };
        let before = self.window_areas();
        {
            let mut map = layer_map_for_output(&output);
            map.arrange();
            let configured = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .and_then(|data| data.lock().ok().map(|d| d.initial_configure_sent))
                    .unwrap_or(true)
            });
            if let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
                send_scale(&output, layer);
                if !configured {
                    layer.layer_surface().send_configure();
                }
            }
        }
        self.give_keyboard(&output, surface);
        if self.window_areas() != before {
            self.relayout();
        }
        self.dirty = true;
        self.state_changed();
        true
    }

    /// Every dock, with its place and its screen's, while docks hide when
    /// covered (M5.4f); none otherwise.
    fn hiding_docks(&self) -> Vec<Dock> {
        if !self.settings.dock_hides() {
            return Vec::new();
        }
        let mut docks = Vec::new();
        for output in self.space.outputs() {
            let Some(screen) = self.space.output_geometry(output) else {
                continue;
            };
            let map = layer_map_for_output(output);
            for layer in map.layers().filter(|l| l.namespace() == DOCK) {
                if let Some(mut place) = map.layer_geometry(layer) {
                    place.loc += screen.loc;
                    docks.push(Dock {
                        surface: layer.wl_surface().clone(),
                        place,
                        screen,
                    });
                }
            }
        }
        docks
    }

    /// The docks not drawn now (M5.4f): those a window's frame overlaps,
    /// unless the pointer has brought them back.
    pub fn hidden_layers(&self) -> Vec<WlSurface> {
        if self.dock_shown {
            return Vec::new();
        }
        self.hiding_docks()
            .into_iter()
            .filter(|dock| {
                self.space.elements().any(|window| {
                    self.space
                        .element_geometry(window)
                        .is_some_and(|w| self.insets(window).frame(w).overlaps(dock.place))
                })
            })
            .map(|dock| dock.surface)
            .collect()
    }

    /// The pointer at `point` brings a hidden dock back when it reaches
    /// the dock's edge of the screen, and lets it hide again once it
    /// leaves the dock (M5.4f).
    pub fn reveal_docks(&mut self, point: Point<f64, Logical>) {
        let shown = self.hiding_docks().into_iter().any(|dock| {
            reaches_edge(point, dock.place, dock.screen)
                || (self.dock_shown && dock.place.to_f64().contains(point))
        });
        if shown != self.dock_shown {
            self.dock_shown = shown;
            self.dirty = true;
        }
    }

    /// The layer surface under `point` among `layers`, with its place on
    /// screen.
    pub fn layer_under(
        &self,
        layers: &[Layer],
        point: Point<f64, Logical>,
    ) -> Option<(LayerSurface, WlSurface, Point<f64, Logical>)> {
        let hidden = self.hidden_layers();
        for output in self.space.outputs() {
            let Some(screen) = self.space.output_geometry(output) else {
                continue;
            };
            let map = layer_map_for_output(output);
            for layer in layers {
                for surface in map.layers_on(*layer).rev() {
                    if hidden.contains(surface.wl_surface()) {
                        continue;
                    }
                    let Some(place) = map.layer_geometry(surface) else {
                        continue;
                    };
                    let origin = (place.loc + screen.loc).to_f64();
                    if let Some((under, offset)) =
                        surface.surface_under(point - origin, WindowSurfaceType::ALL)
                    {
                        return Some((surface.clone(), under, origin + offset.to_f64()));
                    }
                }
            }
        }
        None
    }

    /// A layer above the windows that asks for the keyboard alone, such
    /// as the launcher, gets it once, when it first shows; a click
    /// elsewhere takes it away again, as the launcher expects.
    fn give_keyboard(&mut self, output: &Output, surface: &WlSurface) {
        if !crate::state::has_buffer(surface) {
            // Hidden without being destroyed: it gives the keyboard back,
            // and gets it again when it shows.
            if self.keyboard_layer.as_ref() == Some(surface) {
                self.take_keyboard_back();
                self.keyboard_layer = None;
            }
            return;
        }
        if self.keyboard_layer.as_ref() == Some(surface) {
            return;
        }
        let wants = {
            let map = layer_map_for_output(output);
            map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_some_and(|layer| {
                    ABOVE.contains(&layer.layer())
                        && layer.cached_state().keyboard_interactivity
                            == KeyboardInteractivity::Exclusive
                })
        };
        if !wants {
            return;
        }
        self.keyboard_layer = Some(surface.clone());
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(surface.clone()), SERIAL_COUNTER.next_serial());
        }
        self.sync_toplevels();
    }

    /// A click on nothing takes the keyboard back from such a layer, as a
    /// click on a window does: the launcher closes, and the top window
    /// has the keyboard again.
    pub fn take_keyboard_back(&mut self) {
        let (Some(layer), Some(keyboard)) = (self.keyboard_layer.clone(), self.seat.get_keyboard())
        else {
            return;
        };
        if keyboard.current_focus().as_ref() != Some(&layer) {
            return;
        }
        match self.space.elements().last().cloned() {
            Some(top) => self.focus(&top),
            None => {
                keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
                self.sync_toplevels();
            }
        }
    }

    /// A click on a layer that takes the keyboard gives it the keyboard.
    pub fn focus_layer(&mut self, layer: &LayerSurface) {
        if !layer.can_receive_keyboard_focus() {
            return;
        }
        let surface = layer.wl_surface().clone();
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
        }
        // The window list shows no window as focused.
        self.sync_toplevels();
    }

    /// The `[[layers]]` of the state file: each screen's layer surfaces,
    /// bottom first, with their namespace, layer and place on screen.
    pub fn layers_toml(&self) -> Vec<Value> {
        let mut rows = Vec::new();
        for output in self.space.outputs() {
            let Some(screen) = self.space.output_geometry(output) else {
                continue;
            };
            let map = layer_map_for_output(output);
            for layer in BELOW.iter().rev().chain(ABOVE.iter().rev()) {
                for surface in map.layers_on(*layer) {
                    let Some(place) = map.layer_geometry(surface) else {
                        continue;
                    };
                    let mut t = Table::new();
                    t.insert(
                        "namespace".into(),
                        Value::String(surface.namespace().into()),
                    );
                    t.insert("layer".into(), Value::String(name(*layer).into()));
                    t.insert("output".into(), Value::String(output.name()));
                    for (key, value) in [
                        ("x", place.loc.x + screen.loc.x),
                        ("y", place.loc.y + screen.loc.y),
                        ("width", place.size.w),
                        ("height", place.size.h),
                    ] {
                        t.insert(key.into(), Value::Integer(value.into()));
                    }
                    rows.push(Value::Table(t));
                }
            }
        }
        rows
    }
}

/// A dock that may hide, with its place and its screen's, on screen.
struct Dock {
    surface: WlSurface,
    place: Rectangle<i32, Logical>,
    screen: Rectangle<i32, Logical>,
}

/// Whether `point` is within `EDGE` of the screen's edge that a dock at
/// `place` lies along: the bottom for a dock in the lower half, else the
/// top.
fn reaches_edge(
    point: Point<f64, Logical>,
    place: Rectangle<i32, Logical>,
    screen: Rectangle<i32, Logical>,
) -> bool {
    let (place, screen) = (place.to_f64(), screen.to_f64());
    let across = point.x >= screen.loc.x && point.x < screen.loc.x + screen.size.w;
    let lower = place.loc.y + place.size.h / 2.0 > screen.loc.y + screen.size.h / 2.0;
    across
        && if lower {
            point.y >= screen.loc.y + screen.size.h - EDGE
        } else {
            point.y < screen.loc.y + EDGE
        }
}

/// Tells a layer's surfaces its screen's scale, as windows hear theirs.
fn send_scale(output: &Output, layer: &LayerSurface) {
    let scale = output.current_scale();
    let transform = output.current_transform();
    layer.with_surfaces(|surface, data| {
        send_surface_state(surface, data, scale.integer_scale(), transform);
        with_fractional_scale(data, |fractional| {
            fractional.set_preferred_scale(scale.fractional_scale());
        });
    });
}

fn name(layer: Layer) -> &'static str {
    match layer {
        Layer::Background => "background",
        Layer::Bottom => "bottom",
        Layer::Top => "top",
        Layer::Overlay => "overlay",
    }
}

/// `output`'s layer surfaces on `layers`, the topmost first, for drawing,
/// but for those `hidden`.
pub fn elements(
    renderer: &mut GlesRenderer,
    output: &Output,
    layers: &[Layer],
    scale: f64,
    hidden: &[WlSurface],
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    let map = layer_map_for_output(output);
    let mut elements = Vec::new();
    for layer in layers {
        for surface in map.layers_on(*layer).rev() {
            if hidden.contains(surface.wl_surface()) {
                continue;
            }
            let Some(place) = map.layer_geometry(surface) else {
                continue;
            };
            elements.extend(
                surface.render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    place.loc.to_physical_precise_round(scale),
                    Scale::from(scale),
                    1.0,
                ),
            );
        }
    }
    elements
}

/// Frame callbacks for `output`'s layer surfaces, after a frame is drawn.
pub fn send_frames(output: &Output, now: Duration) {
    let map = layer_map_for_output(output);
    for layer in map.layers() {
        layer.send_frame(output, now, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }
}

impl WlrLayerShellHandler for Edel {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell
    }

    /// A layer surface goes on the screen it asked for, else the first.
    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());
        let Some(output) = output else {
            eprintln!("edel-compositor: no screen for layer {namespace}");
            surface.send_close();
            return;
        };
        if let Err(e) =
            layer_map_for_output(&output).map_layer(&LayerSurface::new(surface, namespace))
        {
            eprintln!("edel-compositor: placing a layer failed: {e}");
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        let _ = self.popups.track_popup(popup.into());
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        if self.keyboard_layer.as_ref() == Some(surface.wl_surface()) {
            self.keyboard_layer = None;
        }
        // The keyboard goes back to the top window if the layer had it.
        let had_keyboard = self
            .seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .is_some_and(|focus| &focus == surface.wl_surface());
        if had_keyboard {
            if let Some(top) = self.space.elements().last().cloned() {
                self.focus(&top);
            } else if let Some(keyboard) = self.seat.get_keyboard() {
                keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
            }
        }
        let before = self.window_areas();
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            let gone = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = gone {
                map.unmap_layer(&layer);
            }
        }
        if self.window_areas() != before {
            self.relayout();
        }
        self.dirty = true;
        self.state_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dock_comes_back_only_from_its_own_edge_of_its_own_screen() {
        let screen = Rectangle::new((0, 0).into(), (1280, 800).into());
        let bottom = Rectangle::new((500, 740).into(), (280, 52).into());
        let top = Rectangle::new((500, 8).into(), (280, 52).into());
        let at = |x: f64, y: f64| Point::from((x, y));
        assert!(reaches_edge(at(10.0, 799.0), bottom, screen));
        assert!(!reaches_edge(at(640.0, 790.0), bottom, screen));
        assert!(!reaches_edge(at(640.0, 0.0), bottom, screen));
        assert!(reaches_edge(at(640.0, 0.5), top, screen));
        // The screen to the right is another screen.
        assert!(!reaches_edge(at(1300.0, 799.0), bottom, screen));
    }
}
