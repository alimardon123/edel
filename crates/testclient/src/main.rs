//! `edel-testclient` (roadmap M4.3): one window of a given size, solid
//! colour and title, for the desktop tests. A known colour at a known
//! place is what CI's pixel checks can trust under llvmpipe, and the
//! same arguments always draw the same window. It draws again only when
//! the compositor resizes it, and exits when asked to close. With
//! `--layer top` or `--layer bottom` it is a panel instead (M5.1a): a
//! layer surface along that edge, as wide as the screen when its width is
//! 0, on the top layer, keeping its height free of windows. With
//! `--workspace N` it draws nothing and shows workspace N instead
//! (`workspaces.rs`, M5.2b); with `--toplevels` it lists the windows, and
//! activates one given its title (`toplevels.rs`, M5.2d).
//!
//!     edel-testclient --size 300x200 --colour cc3333 --title one
//!     edel-testclient --layer bottom --size 0x40 --colour 2f343f
//!     edel-testclient --workspace 3
//!     edel-testclient --toplevels away

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_output, wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};

struct Args {
    width: u32,
    height: u32,
    /// Little-endian ARGB8888, the bytes as they go into the buffer.
    pixel: [u8; 4],
    title: String,
    /// The screen edge a panel runs along.
    layer: Option<Anchor>,
}

fn args() -> Result<Args> {
    let mut size = (300, 200);
    let mut pixel = [0x33, 0x33, 0xcc, 0xff];
    let mut title = "edel-testclient".to_string();
    let mut layer = None;
    let mut words = std::env::args().skip(1);
    while let Some(word) = words.next() {
        let value = words
            .next()
            .with_context(|| format!("{word} needs a value"))?;
        match word.as_str() {
            "--size" => {
                let (w, h) = value.split_once('x').context("--size is WIDTHxHEIGHT")?;
                size = (w.parse()?, h.parse()?);
                if size.0 > 8192 || size.1 == 0 || size.1 > 8192 {
                    bail!("--size must be from 0x1 to 8192x8192");
                }
            }
            "--colour" => {
                if value.len() != 6 {
                    bail!("--colour is RRGGBB");
                }
                let rgb = u32::from_str_radix(&value, 16).context("--colour is RRGGBB")?;
                pixel = [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8, 0xff];
            }
            "--title" => title = value,
            "--layer" => {
                layer = Some(match value.as_str() {
                    "top" => Anchor::TOP,
                    "bottom" => Anchor::BOTTOM,
                    _ => bail!("--layer is top or bottom"),
                })
            }
            _ => bail!(
                "unknown argument {word}; use --size, --colour, --title and --layer, or --workspace or --toplevels alone"
            ),
        }
    }
    if size.0 == 0 && layer.is_none() {
        bail!("only a panel (--layer) may have width 0, the screen's");
    }
    Ok(Args {
        width: size.0,
        height: size.1,
        pixel,
        title,
        layer,
    })
}

/// What the client shows: a window, or a panel.
enum Shown {
    Window(Window),
    Panel(LayerSurface),
}

impl Shown {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Shown::Window(window) => window.wl_surface(),
            Shown::Panel(panel) => panel.wl_surface(),
        }
    }

    fn commit(&self) {
        match self {
            Shown::Window(window) => window.commit(),
            Shown::Panel(panel) => panel.commit(),
        }
    }
}

struct Client {
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    shown: Shown,
    width: u32,
    height: u32,
    pixel: [u8; 4],
    closed: bool,
}

mod toplevels;
mod workspaces;

fn main() -> Result<()> {
    let words: Vec<String> = std::env::args().skip(1).collect();
    match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--workspace", name] => return workspaces::run(name),
        ["--toplevels"] => return toplevels::run(None),
        ["--toplevels", title] => return toplevels::run(Some(title)),
        _ => {}
    }
    let args = args()?;
    let connection = Connection::connect_to_env().context("connecting to the compositor")?;
    let (globals, mut queue) = registry_queue_init(&connection).context("reading the globals")?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let surface = compositor.create_surface(&qh);
    let shown = match args.layer {
        Some(edge) => {
            let shell = LayerShell::bind(&globals, &qh).context("no zwlr_layer_shell_v1")?;
            let panel =
                shell.create_layer_surface(&qh, surface, Layer::Top, Some("edel-testclient"), None);
            panel.set_anchor(edge | Anchor::LEFT | Anchor::RIGHT);
            panel.set_size(args.width, args.height);
            panel.set_exclusive_zone(args.height as i32);
            panel.set_keyboard_interactivity(KeyboardInteractivity::None);
            Shown::Panel(panel)
        }
        None => {
            let shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
            let window = shell.create_window(surface, WindowDecorations::ServerDefault, &qh);
            window.set_title(args.title);
            window.set_app_id("edel-testclient");
            Shown::Window(window)
        }
    };
    // The first commit carries no buffer; the compositor answers with a
    // configure, and the first draw follows it.
    shown.commit();
    let pool = SlotPool::new((args.width.max(1) * args.height * 4) as usize, &shm)
        .context("creating the shared memory pool")?;
    let mut client = Client {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        shown,
        width: args.width,
        height: args.height,
        pixel: args.pixel,
        closed: false,
    };
    while !client.closed {
        // When the compositor goes away, so does the window: the end.
        if let Err(e) = queue.blocking_dispatch(&mut client) {
            eprintln!("edel-testclient: the compositor went away: {e}");
            break;
        }
    }
    Ok(())
}

impl Client {
    fn draw(&mut self) -> Result<()> {
        let stride = self.width as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(
                self.width as i32,
                self.height as i32,
                stride,
                // Opaque, as real apps say they are where they draw solid
                // colour (an opaque region, or no alpha), so the
                // compositor can skip what such a window hides (M5.11b).
                wl_shm::Format::Xrgb8888,
            )
            .context("creating a buffer")?;
        for pixel in canvas.chunks_exact_mut(4) {
            pixel.copy_from_slice(&self.pixel);
        }
        let surface = self.shown.wl_surface();
        surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        buffer.attach_to(surface).context("attaching the buffer")?;
        self.shown.commit();
        Ok(())
    }
}

impl LayerShellHandler for Client {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.closed = true;
    }

    /// Takes the size the compositor gives, the screen's width for a
    /// panel of width 0.
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        if w > 0 {
            self.width = w;
        }
        if h > 0 {
            self.height = h;
        }
        if let Err(e) = self.draw() {
            eprintln!("edel-testclient: {e:#}");
            self.closed = true;
        }
    }
}

impl WindowHandler for Client {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.closed = true;
    }

    /// Keeps the asked size unless the compositor gives one (a resize).
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        if let (Some(w), Some(h)) = configure.new_size {
            self.width = w.get();
            self.height = h.get();
        }
        if let Err(e) = self.draw() {
            eprintln!("edel-testclient: {e:#}");
            self.closed = true;
        }
    }
}

impl CompositorHandler for Client {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
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

impl OutputHandler for Client {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Client {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Client {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers!(OutputState);
}

delegate_registry!(Client);
smithay_client_toolkit::delegate_dispatch2!(Client);
