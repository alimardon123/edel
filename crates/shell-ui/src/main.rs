//! `edel-shell-ui`: the Edel shell's panels and menus (ADR-002), the second
//! long-running process beside the compositor, which starts it and starts
//! it again if it ends. It draws its own surfaces (ADR-002's shell-ui
//! toolkit decision of 2026-10-03): layer-shell surfaces through
//! smithay-client-toolkit, drawn with tiny-skia into shared memory, text
//! shaped by cosmic-text, all from the design tokens. Today it draws the
//! preset's panels (M5.1b, M5.1c): the system file's `shell.preset`, else
//! Classic, whose one panel runs along the bottom of the first screen
//! with the menu button and a clock. Each panel holds widgets from the
//! table in `widgets/` and is drawn again only when what a widget shows,
//! or the panel's size or scale, changes. It exits when the compositor
//! goes away.

mod a11y;
mod paint;
mod widgets;

use anyhow::{Context, Result};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use tiny_skia::Pixmap;

use edel::presets::{self, Edge};
use edel::system;
use edel::tokens::{self, Tokens};

use crate::paint::{Look, Row, Text};

/// The panels' layer surfaces' namespace, as the compositor's state file
/// lists it.
const NAMESPACE: &str = "edel-panel";

/// Where the compositor says how much the desktop animates (M5.11a).
const STATE: &str = "/run/edel/session/state.toml";

struct Shell {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    shm: Shm,
    pool: SlotPool,
    panels: Vec<Panel>,
    tokens: Tokens,
    text: Text,
    fillets: bool,
    exit: bool,
}

/// One of the preset's panels.
struct Panel {
    edge: Edge,
    surface: LayerSurface,
    row: Row,
    /// Its logical width, once the compositor has said it.
    width: u32,
    scale: u32,
    /// What was drawn last, so nothing is drawn twice.
    drawn: Option<Look>,
    /// What screen readers read of it (M5.1d).
    reader: a11y::Reader,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("edel-shell-ui: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let tokens = load_tokens();
    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    let (globals, queue) = registry_queue_init(&connection).context("reading the globals")?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let layers = LayerShell::bind(&globals, &qh).context("no zwlr_layer_shell_v1")?;
    let strip = paint::fillet_height(&tokens);
    let features = std::path::Path::new(edel::features::DIR);
    let mut panels = Vec::new();
    for spec in &preset().panels {
        let pick = |names: &[String]| {
            let (found, notes) = widgets::usable(names, features);
            for note in notes {
                eprintln!("edel-shell-ui: {note}");
            }
            found
        };
        let row = Row {
            start: pick(&spec.start),
            centre: pick(&spec.centre),
            end: pick(&spec.end),
        };
        // Along the edge of the first screen; the strip on its inner side
        // for the fillets is drawn but takes no space and no clicks.
        let surface = compositor.create_surface(&qh);
        let surface = layers.create_layer_surface(&qh, surface, Layer::Top, Some(NAMESPACE), None);
        let edge = match spec.edge {
            Edge::Top => Anchor::TOP,
            Edge::Bottom => Anchor::BOTTOM,
        };
        surface.set_anchor(edge | Anchor::LEFT | Anchor::RIGHT);
        surface.set_size(0, tokens.panel_height + strip);
        surface.set_exclusive_zone(tokens.panel_height as i32);
        surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        surface.commit();
        eprintln!(
            "edel-shell-ui: panel {NAMESPACE} along the {}",
            spec.edge.name()
        );
        panels.push(Panel {
            edge: spec.edge,
            surface,
            row,
            width: 0,
            scale: 1,
            drawn: None,
            reader: a11y::Reader::new(),
        });
    }
    let pool = SlotPool::new(1280 * (tokens.panel_height + strip) as usize * 4, &shm)
        .context("creating the shared memory pool")?;
    let mut shell = Shell {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        shm,
        pool,
        panels,
        text: Text::load(),
        tokens,
        fillets: fillets(),
        exit: false,
    };
    let mut event_loop: EventLoop<Shell> =
        EventLoop::try_new().context("starting the event loop")?;
    WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("watching the compositor: {e}"))?;
    tick(&event_loop.handle());
    while !shell.exit {
        // When the compositor goes away, so does the panel: the end.
        if event_loop.dispatch(None, &mut shell).is_err() {
            eprintln!("edel-shell-ui: the compositor went away");
            break;
        }
    }
    Ok(())
}

/// The image's tokens if it has them, else the built-in ones; anything
/// skipped is reported, never fatal (ADR-008).
fn load_tokens() -> Tokens {
    let Ok(text) = std::fs::read_to_string(tokens::PATH) else {
        return Tokens::built_in();
    };
    let (tokens, notes) = Tokens::read(&text);
    for note in notes {
        eprintln!("edel-shell-ui: {}: {note}", tokens::PATH);
    }
    tokens
}

/// The preset the system files name, the person's over the machine's,
/// else Classic; a broken file or an unknown name is reported, never fatal
/// (ADR-008).
fn preset() -> presets::Preset {
    let mut name = None;
    let files = [Some(system::MACHINE_FILE.into()), system::person_file()];
    for path in files.into_iter().flatten() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match system::read(&text) {
            Ok(read) if read.file.shell.preset.is_some() => name = read.file.shell.preset,
            Ok(_) => {}
            Err(e) => eprintln!(
                "edel-shell-ui: {}: {e:#}; its keys are left out",
                path.display()
            ),
        }
    }
    let (preset, note) = presets::named(name.as_deref());
    if let Some(note) = note {
        eprintln!("edel-shell-ui: {note}");
    }
    preset
}

/// Whether the fillets are drawn: not on the Lite tier, which keeps
/// surfaces flat (ADR-002), nor before the compositor has said.
fn fillets() -> bool {
    let tier = std::fs::read_to_string(STATE)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|t| t.get("tier")?.as_str().map(String::from));
    tier.is_some_and(|t| t != "lite")
}

/// Draws the panels again when the minute changes, for the clock, then
/// sleeps until the next one.
fn tick(handle: &LoopHandle<'static, Shell>) {
    let next = || widgets::clock::until_next_minute(&jiff::Zoned::now());
    let result = handle.insert_source(
        Timer::from_duration(next()),
        move |_, _, shell: &mut Shell| {
            for i in 0..shell.panels.len() {
                shell.draw(i);
            }
            TimeoutAction::ToDuration(next())
        },
    );
    if let Err(e) = result {
        eprintln!("edel-shell-ui: the clock's timer did not start: {e}");
    }
}

impl Shell {
    /// Draws panel `i` if anything it shows changed.
    fn draw(&mut self, i: usize) {
        let panel = &self.panels[i];
        if panel.width == 0 {
            return;
        }
        let strip = paint::fillet_height(&self.tokens);
        let look = Look {
            width: panel.width * panel.scale,
            height: (self.tokens.panel_height + strip) * panel.scale,
            scale: panel.scale,
            edge: panel.edge,
            fillets: self.fillets,
            shown: panel.row.shows(),
        };
        if panel.drawn.as_ref() == Some(&look) {
            return;
        }
        let placed = match self.show(i, &look) {
            Ok(placed) => placed,
            Err(e) => {
                eprintln!("edel-shell-ui: drawing the panel failed: {e:#}");
                return;
            }
        };
        // Screen readers get what was drawn, in logical pixels.
        let panel = &mut self.panels[i];
        let s = f64::from(panel.scale);
        let top = f64::from(paint::panel_top(panel.edge, &self.tokens));
        let bottom = top + f64::from(self.tokens.panel_height);
        let items: Vec<a11y::Item> = panel
            .row
            .all()
            .zip(&look.shown)
            .zip(placed)
            .map(|((widget, shown), (x, width))| a11y::Item {
                role: widget.role,
                label: (widget.label)(shown),
                bounds: accesskit::Rect::new(
                    f64::from(x) / s,
                    top,
                    f64::from(x + width) / s,
                    bottom,
                ),
            })
            .collect();
        let size = (
            f64::from(panel.width),
            f64::from(self.tokens.panel_height + strip),
        );
        panel.reader.update(a11y::tree(size, &items));
        panel.drawn = Some(look);
    }

    /// Draws panel `i` showing `look`, and says where each widget went.
    fn show(&mut self, i: usize, look: &Look) -> Result<Vec<(f32, f32)>> {
        let panel = &self.panels[i];
        let mut pixmap = Pixmap::new(look.width, look.height).context("a panel of no size")?;
        let placed = paint::paint(
            &mut pixmap,
            look,
            &self.tokens,
            Some(&mut self.text),
            &panel.row,
        );
        let (w, h) = (look.width as i32, look.height as i32);
        let (buffer, canvas) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
            .context("creating a buffer")?;
        paint::to_argb(&pixmap, canvas);
        let surface = panel.surface.wl_surface();
        // Only the panel itself is opaque and takes clicks; the fillets'
        // strip beside it lets both through.
        let top = paint::panel_top(panel.edge, &self.tokens) as i32;
        let panel_h = self.tokens.panel_height as i32;
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, top, panel.width as i32, panel_h);
            surface.set_opaque_region(Some(region.wl_region()));
            surface.set_input_region(Some(region.wl_region()));
        }
        surface.set_buffer_scale(panel.scale as i32);
        surface.damage_buffer(0, 0, w, h);
        buffer.attach_to(surface).context("attaching the buffer")?;
        panel.surface.commit();
        Ok(placed)
    }

    /// The panel whose surface is `surface`.
    fn panel_of(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.panels
            .iter()
            .position(|p| p.surface.wl_surface() == surface)
    }
}

impl LayerShellHandler for Shell {
    /// The compositor took a panel away (its screen went): shell-ui ends,
    /// and the compositor starts it again, every panel afresh.
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(i) = self.panel_of(surface.wl_surface()) else {
            return;
        };
        let (w, _) = configure.new_size;
        if w > 0 {
            self.panels[i].width = w;
        }
        self.draw(i);
    }
}

impl CompositorHandler for Shell {
    /// The screen's scale: the panel draws at it, sharp.
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        let Some(i) = self.panel_of(surface) else {
            return;
        };
        self.panels[i].scale = factor.clamp(1, 4) as u32;
        self.draw(i);
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Shell {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Shell {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Shell {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers!(OutputState);
}

delegate_registry!(Shell);
smithay_client_toolkit::delegate_dispatch2!(Shell);
