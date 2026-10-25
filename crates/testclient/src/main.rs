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
//! activates one given its title (`toplevels.rs`, M5.2d). `--app-id` sets
//! the window's app id, `edel-testclient` without it, so a test can stand
//! in for an app the panel pins (M5.4c). `--fullscreen` asks to fill the
//! screen before the window is shown (M5.20). `--globals` lists the
//! globals it is offered, and `--security-context` those a sandboxed app
//! is offered (`sandbox.rs`, M5.22). `--lock-pointer` locks the pointer
//! and prints the mouse's motion, and `--inhibit-idle` keeps the screen
//! on (`hold.rs`, M5.23). `--sni` draws nothing: it is a tray item on the
//! session's bus (`sni.rs`, M5.2e). `--mpris` draws nothing either: it is
//! a media player on the session's bus for quick settings' now playing
//! (`mpris.rs`, M5.9d). `--screenshot` and `--screencast` draw nothing
//! either: they ask xdg-desktop-portal for a screenshot or to share the
//! screen, as an app does, and print what it answers (`portal.rs`, M6.3).
//! `--lid SECONDS` draws nothing either: as root, it shuts a lid switch on
//! uinput for SECONDS and opens it again, as a laptop's lid does
//! (`lid.rs`, M7.8). `--location` draws nothing either: it asks the
//! Location portal for the device's position and prints the first update
//! (`portal.rs`, M6.14c), holding the session SECONDS more when given, so
//! the in-use sign stays. `--recolour RRGGBB@FILE` draws the window in that
//! colour once FILE exists, so a test can see whether a picture of it
//! follows it live (M5.2j-b4). `--clipboard` makes the window a clipboard
//! user: keys 1 and 2 copy a text, Ctrl+V pastes the selection, and each
//! prints what it did (`clipboard.rs`, M7.13). Its window is titled `clip`
//! unless `--title` says otherwise. `--busy` draws on every frame the
//! compositor gives the window, alternating two colours, and prints
//! `frames N WxH` (the size it draws at, M7.7d) for each, so a test can
//! see whether a window the desktop hides still gets frames (M7.7c). With
//! `--smooth N` it also asks for presentation feedback on each frame and
//! prints one line once N intervals between presented frames are measured
//! (`smooth.rs`, M7.7e). `--flash N` opens N windows one after another,
//! each closed before the compositor draws a frame of it, every other one
//! before its first configure, as a splash screen or a dialog that closes
//! at once does (M8.21, the other projects' bugs review), and prints
//! `flashed N`. `--sandboxed` connects through a security context, as a
//! Flatpak app does, and `--activate FILE` asks to come forward once FILE
//! exists (`activate.rs`). `--wifi-join SSID PASSWORD` draws nothing: it
//! joins that Wi-Fi network through `edel::network::join`, as Settings
//! does, and prints whether it joined (`wifi.rs`).
//!
//!     edel-testclient --size 300x200 --colour cc3333 --title one
//!     edel-testclient --layer bottom --size 0x40 --colour 2f343f
//!     edel-testclient --workspace 3
//!     edel-testclient --toplevels away
//!     edel-testclient --security-context
//!     edel-testclient --sni
//!     edel-testclient --mpris
//!     edel-testclient --mpris NAME
//!     edel-testclient --screenshot
//!     edel-testclient --screencast
//!     edel-testclient --lid 8
//!     edel-testclient --location
//!     edel-testclient --location SECONDS
//!     edel-testclient --clipboard
//!     edel-testclient --flash 40
//!     edel-testclient --wifi-join edel-psk PASSWORD

use anyhow::{Context, Result, bail};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, FrameCallbackData};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::Dispatch;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::wl_callback;
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
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

struct Args {
    width: u32,
    height: u32,
    /// Little-endian ARGB8888, the bytes as they go into the buffer.
    pixel: [u8; 4],
    title: String,
    app_id: String,
    /// The screen edge a panel runs along.
    layer: Option<Anchor>,
    /// A window that asks to fill its screen before it is shown (M5.20).
    fullscreen: bool,
    /// A window that locks the pointer, or keeps the screen on (M5.23).
    lock_pointer: bool,
    inhibit_idle: bool,
    /// The colour drawn once the file exists, and the file.
    recolour: Option<([u8; 4], PathBuf)>,
    /// A window that copies and pastes text (M7.13).
    clipboard: bool,
    /// A window that draws on every frame callback (M7.7c).
    busy: bool,
    /// How many intervals between presented frames to measure, then exit
    /// after printing them (M7.7e). Needs `busy`.
    smooth: Option<usize>,
    /// How many windows to open and close at once (M8.21).
    flash: Option<u32>,
    /// Connected through a security context, as a Flatpak app is.
    sandboxed: bool,
    /// The file whose making asks the window forward (`activate.rs`).
    activate: Option<PathBuf>,
}

/// `RRGGBB` as the bytes of a little-endian ARGB8888 pixel.
fn pixel_of(value: &str, flag: &str) -> Result<[u8; 4]> {
    if value.len() != 6 {
        bail!("{flag} is RRGGBB");
    }
    let rgb = u32::from_str_radix(value, 16).with_context(|| format!("{flag} is RRGGBB"))?;
    Ok([rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8, 0xff])
}

fn args() -> Result<Args> {
    let mut size = (300, 200);
    let mut pixel = [0x33, 0x33, 0xcc, 0xff];
    let mut title = None;
    let mut app_id = "edel-testclient".to_string();
    let mut layer = None;
    let mut fullscreen = false;
    let (mut lock_pointer, mut inhibit_idle) = (false, false);
    let mut recolour = None;
    let mut clipboard = false;
    let mut busy = false;
    let mut smooth = None;
    let mut flash = None;
    let mut sandboxed = false;
    let mut activate = None;
    let mut words = std::env::args().skip(1);
    while let Some(word) = words.next() {
        match word.as_str() {
            "--fullscreen" => fullscreen = true,
            "--lock-pointer" => lock_pointer = true,
            "--inhibit-idle" => inhibit_idle = true,
            "--clipboard" => clipboard = true,
            // Steam's own flag for Big Picture, which the game session
            // starts it with (M8.26), so the stand-in for Steam
            // (ci/apps/make-repo.sh) draws as a game does.
            "--busy" | "-gamepadui" => busy = true,
            "--sandboxed" => sandboxed = true,
            _ => {}
        }
        if matches!(
            word.as_str(),
            "--fullscreen"
                | "--lock-pointer"
                | "--inhibit-idle"
                | "--clipboard"
                | "--busy"
                | "-gamepadui"
                | "--sandboxed"
        ) {
            continue;
        }
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
            "--colour" => pixel = pixel_of(&value, "--colour")?,
            "--recolour" => {
                let (colour, file) = value.split_once('@').context("--recolour is RRGGBB@FILE")?;
                recolour = Some((pixel_of(colour, "--recolour")?, PathBuf::from(file)));
            }
            "--title" => title = Some(value),
            "--activate" => activate = Some(PathBuf::from(value)),
            "--app-id" => app_id = value,
            "--smooth" => {
                smooth = match value.parse::<usize>() {
                    Ok(frames) if frames > 0 => Some(frames),
                    _ => bail!("--smooth takes a number of frames, not {value}"),
                }
            }
            "--flash" => {
                flash = match value.parse::<u32>() {
                    Ok(times) if times > 0 => Some(times),
                    _ => bail!("--flash takes a number of windows, not {value}"),
                }
            }
            "--layer" => {
                layer = Some(match value.as_str() {
                    "top" => Anchor::TOP,
                    "bottom" => Anchor::BOTTOM,
                    _ => bail!("--layer is top or bottom"),
                })
            }
            _ => bail!(
                "unknown argument {word}; use --size, --colour, --recolour, --title, --app-id, --fullscreen, --lock-pointer, --inhibit-idle, --clipboard, --busy, --smooth, --sandboxed, --activate and --layer, or --workspace, --toplevels, --globals, --security-context, --sni, --mpris or --location alone"
            ),
        }
    }
    if size.0 == 0 && layer.is_none() {
        bail!("only a panel (--layer) may have width 0, the screen's");
    }
    if smooth.is_some() && !busy {
        bail!("--smooth measures --busy's frames; add --busy");
    }
    let title =
        title.unwrap_or_else(|| if clipboard { "clip" } else { "edel-testclient" }.to_string());
    Ok(Args {
        width: size.0,
        height: size.1,
        pixel,
        title,
        app_id,
        layer,
        fullscreen,
        lock_pointer,
        inhibit_idle,
        recolour,
        clipboard,
        busy,
        smooth,
        flash,
        sandboxed,
        activate,
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
    clipboard: Option<clipboard::State>,
    /// `--busy` (M7.7c): the colour it starts with, the frames it has drawn,
    /// and whether it has asked for the next frame callback.
    busy: bool,
    colour: [u8; 4],
    frames: u32,
    waiting: bool,
    /// `--smooth` (M7.7e): the presentation feedback it measures.
    smooth: Option<smooth::Smooth>,
}

/// The wake-up `--recolour`'s watch sends itself once the file exists: a
/// `wl_display.sync`, whose answer ends the blocking dispatch.
struct Wake;

impl Dispatch<wl_callback::WlCallback, Wake> for Client {
    fn event(
        _: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &Wake,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

mod activate;
mod clipboard;
mod hold;
mod lid;
mod mpris;
mod portal;
mod sandbox;
mod smooth;
mod sni;
mod toplevels;
mod wifi;
mod workspaces;

fn main() -> Result<()> {
    let words: Vec<String> = std::env::args().skip(1).collect();
    match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--workspace", name] => return workspaces::run(name),
        ["--toplevels"] => return toplevels::run(None),
        ["--sni"] => return sni::run(),
        ["--mpris"] => return mpris::run(None),
        ["--screenshot"] => return portal::screenshot(),
        ["--screencast"] => return portal::screencast(),
        ["--lid", seconds] => return lid::run(seconds),
        ["--location"] => return portal::location(0),
        ["--location", seconds] => {
            let seconds = seconds
                .parse()
                .with_context(|| format!("--location takes whole seconds, not {seconds}"))?;
            return portal::location(seconds);
        }
        ["--mpris", name] => return mpris::run(Some(name)),
        ["--globals"] => return sandbox::run(false),
        ["--security-context"] => return sandbox::run(true),
        ["--toplevels", title] => return toplevels::run(Some(title)),
        ["--wifi-join", ssid, password] => return wifi::join(ssid, password),
        _ => {}
    }
    let args = args()?;
    let (connection, _sandbox) = if args.sandboxed {
        let (connection, sandbox) = sandbox::connect()?;
        (connection, Some(sandbox))
    } else {
        let connection = Connection::connect_to_env().context("connecting to the compositor")?;
        (connection, None)
    };
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
            window.set_title(args.title.clone());
            window.set_app_id(args.app_id.clone());
            if args.fullscreen {
                window.set_fullscreen(None);
            }
            Shown::Window(window)
        }
    };
    if args.lock_pointer {
        hold::lock(&globals, &qh, shown.wl_surface())?;
    }
    if args.inhibit_idle {
        hold::inhibit(&globals, &qh, shown.wl_surface())?;
    }
    let clipboard = if args.clipboard {
        Some(clipboard::bind(&globals, &qh)?)
    } else {
        None
    };
    let smooth = args
        .smooth
        .map(|want| smooth::bind(&globals, &qh, want))
        .transpose()?;
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
        clipboard,
        busy: args.busy,
        colour: args.pixel,
        frames: 0,
        waiting: false,
        smooth,
    };
    if let Some(times) = args.flash {
        let shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
        for round in 0..times {
            // Every other window gets its configure, so it draws and
            // maps; the next window's commit and this one's end follow
            // at once, within the compositor's frame.
            if round % 2 == 0 {
                queue
                    .roundtrip(&mut client)
                    .context("waiting for the configure")?;
            }
            if round + 1 < times {
                let window = shell.create_window(
                    compositor.create_surface(&qh),
                    WindowDecorations::ServerDefault,
                    &qh,
                );
                window.set_title(args.title.clone());
                window.set_app_id(args.app_id.clone());
                window.commit();
                client.shown = Shown::Window(window);
                connection.flush().context("closing the window")?;
            }
        }
        drop(client);
        connection.flush().context("closing the last window")?;
        println!("flashed {times}");
        return Ok(());
    }
    let activation = args
        .activate
        .as_ref()
        .map(|_| activate::bind(&globals, &qh))
        .transpose()?;
    let mut recolour = args.recolour;
    let mut activate = args.activate;
    for file in recolour.iter().map(|(_, file)| file).chain(&activate) {
        let (display, wake) = (connection.display(), qh.clone());
        let (connection, file) = (connection.clone(), file.clone());
        std::thread::spawn(move || {
            while !file.exists() {
                std::thread::sleep(Duration::from_millis(100));
            }
            display.sync(&wake, Wake);
            let _ = connection.flush();
        });
    }
    while !client.closed {
        if let (Some(file), Some(activation)) = (&activate, &activation) {
            if file.exists() {
                activate = None;
                activate::ask(activation, &qh, client.shown.wl_surface());
                let _ = std::io::stdout().flush();
            }
        }
        if let Some((pixel, file)) = &recolour {
            if file.exists() {
                client.pixel = *pixel;
                recolour = None;
                if let Err(e) = client.draw(&qh) {
                    eprintln!("edel-testclient: {e:#}");
                    break;
                }
                println!("recoloured");
            }
        }
        // When the compositor goes away, so does the window: the end.
        if let Err(e) = queue.blocking_dispatch(&mut client) {
            eprintln!("edel-testclient: the compositor went away: {e}");
            break;
        }
        if let Some(line) = client
            .smooth
            .as_ref()
            .filter(|smooth| smooth.done())
            .map(smooth::Smooth::line)
        {
            println!("{line}");
            let _ = std::io::stdout().flush();
            break;
        }
        if let Err(e) = clipboard::paste(&connection, &mut queue, &mut client) {
            eprintln!("edel-testclient: {e:#}");
            break;
        }
    }
    if let (Some(want), Some(smooth)) = (
        args.smooth,
        client.smooth.as_ref().filter(|smooth| !smooth.done()),
    ) {
        bail!(
            "--smooth saw {} of {want} intervals before the window closed",
            smooth.seen()
        );
    }
    Ok(())
}

impl Client {
    /// Draws one buffer. With `--busy` it also asks for the next frame
    /// callback before it commits, unless one is already on its way (M7.7c).
    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
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
        if self.busy && !self.waiting {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.waiting = true;
        }
        // Not before the first frame callback, so the window's start is
        // not measured (M7.7e).
        if let Some(smooth) = self.smooth.as_ref().filter(|_| self.frames > 0) {
            smooth.ask(surface, qh);
        }
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
        qh: &QueueHandle<Self>,
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
        if let Err(e) = self.draw(qh) {
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
        qh: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        if let (Some(w), Some(h)) = configure.new_size {
            self.width = w.get();
            self.height = h.get();
        }
        if let Err(e) = self.draw(qh) {
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

    /// With `--busy` (M7.7c): each frame callback draws the other colour,
    /// prints the count and the size it draws at (M7.7d) and asks for the next callback. Without it, nothing.
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if !self.busy {
            return;
        }
        self.waiting = false;
        self.frames += 1;
        self.pixel = if self.frames % 2 == 0 {
            self.colour
        } else {
            self.colour.map(|byte| !byte)
        };
        println!("frames {} {}x{}", self.frames, self.width, self.height);
        let _ = std::io::stdout().flush();
        if let Err(e) = self.draw(qh) {
            eprintln!("edel-testclient: {e:#}");
            self.closed = true;
        }
    }

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
