//! `edel-shell-ui`: the Edel shell's panels and menus (ADR-002), the second
//! long-running process beside the compositor, which starts it and starts
//! it again if it ends. It draws its own surfaces (ADR-002's shell-ui
//! toolkit decision of 2026-10-03): layer-shell surfaces through
//! smithay-client-toolkit, drawn with tiny-skia into shared memory, text
//! shaped by cosmic-text, all from the design tokens. Today it is the
//! Classic preset's panel (M5.1b): along the bottom of the first screen,
//! with the menu button's icon and a clock, redrawn only when the minute
//! changes or the panel's size or scale does. It exits when the
//! compositor goes away.

mod clock;
mod paint;

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

use edel::tokens::{self, Tokens};

use crate::paint::{Look, Text};

/// The layer surface's namespace, as the compositor's state file lists it.
const NAMESPACE: &str = "edel-panel";

/// Where the compositor says how much the desktop animates (M5.11a).
const STATE: &str = "/run/edel/session/state.toml";

struct Shell {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    shm: Shm,
    pool: SlotPool,
    panel: LayerSurface,
    tokens: Tokens,
    text: Text,
    /// The panel's logical width, once the compositor has said it.
    width: u32,
    scale: u32,
    /// What was drawn last, so nothing is drawn twice.
    drawn: Option<Look>,
    fillets: bool,
    exit: bool,
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
    // Along the bottom of the first screen; the strip above it for the
    // fillets is drawn but takes no space and no clicks.
    let surface = compositor.create_surface(&qh);
    let panel = layers.create_layer_surface(&qh, surface, Layer::Top, Some(NAMESPACE), None);
    let strip = paint::fillet_height(&tokens);
    panel.set_anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    panel.set_size(0, tokens.panel_height + strip);
    panel.set_exclusive_zone(tokens.panel_height as i32);
    panel.set_keyboard_interactivity(KeyboardInteractivity::None);
    panel.commit();
    let pool = SlotPool::new(1280 * (tokens.panel_height + strip) as usize * 4, &shm)
        .context("creating the shared memory pool")?;
    let mut shell = Shell {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        shm,
        pool,
        panel,
        text: Text::load(),
        tokens,
        width: 0,
        scale: 1,
        drawn: None,
        fillets: fillets(),
        exit: false,
    };
    let mut event_loop: EventLoop<Shell> =
        EventLoop::try_new().context("starting the event loop")?;
    WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("watching the compositor: {e}"))?;
    tick(&event_loop.handle());
    eprintln!("edel-shell-ui: panel {NAMESPACE} along the bottom");
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

/// Whether the fillets are drawn: not on the Lite tier, which keeps
/// surfaces flat (ADR-002), nor before the compositor has said.
fn fillets() -> bool {
    let tier = std::fs::read_to_string(STATE)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|t| t.get("tier")?.as_str().map(String::from));
    tier.is_some_and(|t| t != "lite")
}

/// Draws the clock again when the minute changes, then sleeps until the
/// next one.
fn tick(handle: &LoopHandle<'static, Shell>) {
    let first = clock::until_next_minute(&jiff::Zoned::now());
    let result = handle.insert_source(Timer::from_duration(first), |_, _, shell: &mut Shell| {
        shell.draw();
        TimeoutAction::ToDuration(clock::until_next_minute(&jiff::Zoned::now()))
    });
    if let Err(e) = result {
        eprintln!("edel-shell-ui: the clock's timer did not start: {e}");
    }
}

impl Shell {
    /// Draws the panel if anything it shows changed.
    fn draw(&mut self) {
        if self.width == 0 {
            return;
        }
        let strip = paint::fillet_height(&self.tokens);
        let look = Look {
            width: self.width * self.scale,
            height: (self.tokens.panel_height + strip) * self.scale,
            scale: self.scale,
            clock: clock::text(&jiff::Zoned::now()),
            fillets: self.fillets,
        };
        if self.drawn.as_ref() == Some(&look) {
            return;
        }
        if let Err(e) = self.show(&look) {
            eprintln!("edel-shell-ui: drawing the panel failed: {e:#}");
            return;
        }
        self.drawn = Some(look);
    }

    fn show(&mut self, look: &Look) -> Result<()> {
        let mut pixmap = Pixmap::new(look.width, look.height).context("a panel of no size")?;
        paint::paint(&mut pixmap, look, &self.tokens, Some(&mut self.text));
        let (w, h) = (look.width as i32, look.height as i32);
        let (buffer, canvas) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
            .context("creating a buffer")?;
        paint::to_argb(&pixmap, canvas);
        let surface = self.panel.wl_surface();
        // Only the panel itself is opaque and takes clicks; the fillets'
        // strip above it lets both through.
        let strip = paint::fillet_height(&self.tokens) as i32;
        let panel_h = self.tokens.panel_height as i32;
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, strip, self.width as i32, panel_h);
            surface.set_opaque_region(Some(region.wl_region()));
            surface.set_input_region(Some(region.wl_region()));
        }
        surface.set_buffer_scale(self.scale as i32);
        surface.damage_buffer(0, 0, w, h);
        buffer.attach_to(surface).context("attaching the buffer")?;
        self.panel.commit();
        Ok(())
    }
}

impl LayerShellHandler for Shell {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, _) = configure.new_size;
        if w > 0 {
            self.width = w;
        }
        self.draw();
    }
}

impl CompositorHandler for Shell {
    /// The screen's scale: the panel draws at it, sharp.
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        self.scale = factor.clamp(1, 4) as u32;
        self.draw();
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
