//! The backend for real screens: the virtual GPU in CI and a laptop's GPU
//! (roadmap M4.2b, M4.6c). A libseat session (seatd, ADR-002) opens the
//! primary GPU and the input devices; GBM allocates the buffers, EGL and
//! GLES draw them (one renderer, no fallback), and smithay's DRM compositor
//! puts them on every connected screen, each with a CRTC of its own, at
//! `displays.NAME.resolution` and `refresh_rate` or its preferred mode, at `displays.NAME.position` or
//! right of the screens before it, unless `displays.NAME.enabled` is false.
//! Screens plugged in or out while running, and changes to those keys,
//! scan the connectors again.
//!
//! A screen draws a frame when something changed and its previous one has
//! been shown (its vblank), so an idle desktop draws nothing and a busy one
//! at most once per refresh. After a screen's first frame is shown the
//! compositor logs `edel-compositor: output NAME WxH ready`, and the first
//! one writes `/run/edel/session/ready`, the health file the boot guard
//! waits for (M4.8). Once the screen has been still for 2 s after drawing,
//! it logs its frame telemetry, which CI reads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags, PrimaryPlaneElement};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{
    DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime, DrmNode, NodeType,
};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::ImportDma;
use smithay::backend::renderer::element::default_primary_scanout_output_compare;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu};
use smithay::desktop::utils::{
    OutputPresentationFeedback, surface_presentation_feedback_flags_from_states,
    surface_primary_scanout_output, update_surface_primary_scanout_output,
};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, LoopHandle};
use smithay::reexports::drm::control::{Device as _, ModeTypeFlags, connector, crtc};
use smithay::reexports::input::Libinput;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::reexports::wayland_server::Display;
use smithay::utils::{Clock, DeviceFd, Monotonic, Point, Size};
use smithay::wayland::presentation::Refresh;

use edel_compositor::layout::{auto_scale, parse_mode, pick_mode, place_screens};
use edel_compositor::messages;
use edel_compositor::tokens::Tokens;

use crate::program::Program;
use crate::state::{Edel, listen};

/// Where the session tells root it is up (M4.1, M4.8).
const READY: &str = edel::places::READY_FILE;

/// How long the screen stays still before the telemetry is logged.
const QUIET: Duration = Duration::from_secs(2);

type Compositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    Option<OutputPresentationFeedback>,
    DrmDeviceFd,
>;

/// One lit screen.
struct Screen {
    output: Output,
    connector: connector::Handle,
    compositor: Compositor,
    /// The mode it runs at, to notice a new `displays.NAME.resolution` and `refresh_rate`.
    mode: smithay::reexports::drm::control::Mode,
    /// Something on screen changed since its last frame.
    dirty: bool,
    /// A frame was queued and its vblank has not come yet.
    pending: bool,
    /// Its first frame was shown and announced.
    ready: bool,
}

/// Opens the seat, trying for up to 10 s: at boot greetd may start a
/// session (the live stick's, M3.6) the moment seatd's service is marked
/// started, before seatd listens, and libseat then fails at once.
fn open_seat() -> Result<(
    LibSeatSession,
    smithay::backend::session::libseat::LibSeatSessionNotifier,
)> {
    let mut tries = 0;
    loop {
        match LibSeatSession::new() {
            Ok(opened) => return Ok(opened),
            Err(err) if tries < 20 => {
                if tries == 0 {
                    eprintln!("edel-compositor: waiting for the seat: {err}");
                }
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("could not open the seat: {}", seat_diagnosis()));
            }
        }
    }
}

/// The GPU to draw with: the primary one, else the first, once its
/// device file exists. At boot udev can name a card a moment before its
/// file under /dev/dri is made (the desktop stick's greeter failed five
/// times in 12 s on 2026-10-08, seatd saying it could not find
/// /dev/dri/card0, and greetd gave up); so wait for it as for the seat, up
/// to 10 s, saying so once. A card can also go while the firmware's
/// framebuffer hands the screen to the real driver, which may come back
/// as card1 while udev still names card0 (the stick's `live` session
/// waited 10 s for a card0 that never came, 2026-10-10), so the first card
/// whose file exists wins, udev's primary first, then the files under
/// /dev/dri themselves.
fn wait_for_gpu(seat: &str) -> Result<std::path::PathBuf> {
    let find = || -> Result<Option<std::path::PathBuf>> {
        let primary = primary_gpu(seat).context(messages::GPU_LIST)?;
        let all = all_gpus(seat).context(messages::GPU_LIST)?;
        Ok(pick_gpu(primary, all, &cards_in_dev(), |p| p.exists()))
    };
    let until = Instant::now() + Duration::from_secs(10);
    let mut said = false;
    loop {
        let found = find()?;
        match found {
            Some(path) if path.exists() => return Ok(path),
            _ if Instant::now() >= until => {
                return match found {
                    Some(path) => Err(anyhow::anyhow!(messages::gpu_missing(
                        &path,
                        &dri_seen(&path)
                    ))),
                    None => Err(anyhow::anyhow!(messages::NO_GPU)),
                };
            }
            _ => {
                if !said {
                    let what =
                        found.map_or("a graphics card".to_string(), |p| p.display().to_string());
                    eprintln!("edel-compositor: waiting for {what} to appear");
                    said = true;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// What this user finds of a card's file: the error reading it, and what
/// /dev/dri holds or the error listing it (for the log of a card that
/// never appeared).
fn dri_seen(path: &Path) -> String {
    let file = match std::fs::metadata(path) {
        Ok(_) => "there".to_string(),
        Err(e) => e.kind().to_string(),
    };
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    // What the kernel has registered beside what /dev shows, and what /dev
    // is: a card in sysfs with no file under a devtmpfs /dev was taken out
    // by a program, not by the kernel (the stick's `live` session,
    // 2026-10-10).
    format!(
        "{}: {file}; /dev/dri: {}; /sys/class/drm: {}; /dev is {}",
        path.display(),
        listing(Path::new("/dev/dri")),
        listing(Path::new("/sys/class/drm")),
        dev_mount(&mounts)
    )
}

/// The names in `dir`, sorted and joined by spaces, `empty` for none, or
/// why it could not be read.
fn listing(dir: &Path) -> String {
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            if names.is_empty() {
                "empty".to_string()
            } else {
                names.join(" ")
            }
        }
        Err(e) => e.kind().to_string(),
    }
}

/// The file system mounted on /dev, from the text of /proc/mounts: the
/// last mount there wins, as it is the one seen.
fn dev_mount(mounts: &str) -> String {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (_, at, kind) = (fields.next()?, fields.next()?, fields.next()?);
            (at == "/dev").then(|| kind.to_string())
        })
        .next_back()
        .unwrap_or_else(|| "not mounted".to_string())
}

/// The card files under /dev/dri, `card0` first.
fn cards_in_dev() -> Vec<std::path::PathBuf> {
    let mut cards: Vec<std::path::PathBuf> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("card"))
        })
        .collect();
    cards.sort();
    cards
}

/// Which card to draw with: udev's primary card if its file `exists`, else
/// the first of udev's cards whose file does, else the first card file
/// under /dev/dri; with none there, the card udev names, to wait for.
fn pick_gpu(
    primary: Option<std::path::PathBuf>,
    all: Vec<std::path::PathBuf>,
    in_dev: &[std::path::PathBuf],
    exists: impl Fn(&Path) -> bool,
) -> Option<std::path::PathBuf> {
    if let Some(path) = primary.as_ref().filter(|p| exists(p)) {
        return Some(path.clone());
    }
    if let Some(path) = all.iter().find(|p| exists(p)) {
        return Some(path.clone());
    }
    if let Some(path) = in_dev.iter().find(|p| exists(p)) {
        return Some(path.clone());
    }
    primary.or_else(|| all.into_iter().next())
}

/// Why the seat may not open, in words a person can act on: whether
/// seatd's socket takes a connection, and whether this user is in group
/// seat, as the socket wants.
fn seat_diagnosis() -> String {
    let socket = std::env::var("SEATD_SOCK").unwrap_or_else(|_| "/run/seatd.sock".into());
    let connect = match std::os::unix::net::UnixStream::connect(&socket) {
        Ok(_) => format!("seatd's socket {socket} takes connections"),
        Err(e) => format!("seatd's socket {socket}: {e}"),
    };
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let groups: Vec<&str> = status
        .lines()
        .find_map(|l| l.strip_prefix("Groups:"))
        .map(|g| g.split_whitespace().collect())
        .unwrap_or_default();
    let group_file = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let seat_gid = group_file
        .lines()
        .find_map(|l| l.strip_prefix("seat:"))
        .and_then(|rest| rest.split(':').nth(1).map(str::to_string));
    let membership = match seat_gid {
        Some(gid) if groups.contains(&gid.as_str()) => "this user is in group seat".to_string(),
        Some(gid) => format!(
            "this user is not in group seat (GID {gid}); its groups are {}",
            groups.join(" ")
        ),
        None => "there is no group seat".to_string(),
    };
    let vt = std::env::var("XDG_VTNR").unwrap_or_else(|_| "unset".into());
    format!("{connect}; {membership}; XDG_VTNR is {vt}")
}

struct Gpu {
    handle: LoopHandle<'static, Edel>,
    _session: LibSeatSession,
    libinput: Libinput,
    drm: DrmDevice,
    gbm: GbmDevice<DrmDeviceFd>,
    renderer: GlesRenderer,
    /// By CRTC, the screens lit now.
    screens: HashMap<crtc::Handle, Screen>,
    clock: Clock<Monotonic>,
    /// The session has the seat (not switched away to another VT).
    active: bool,
    /// The health file was written.
    announced: bool,
    started: Instant,
    /// Every frame is drawn whole: the screen's driver shows only the
    /// areas a frame says changed, onto one picture of its own
    /// (`draws_whole`).
    whole_frames: bool,
}

/// Whether frames on a screen driven by `driver` must be drawn whole. The
/// firmware's framebuffer (simpledrm, efidrm, vesadrm) and the plain
/// virtual cards (bochs, cirrus) keep one picture and copy into it only
/// the areas a frame names; our frames name only what changed since that
/// buffer was last drawn, so the screen showed black where nothing had
/// changed and old pointers where one had (the live stick on -vga std,
/// M3.6). A GPU driver flips whole buffers and keeps the fast path.
fn draws_whole(driver: &str) -> bool {
    matches!(
        driver,
        "simpledrm" | "efidrm" | "vesadrm" | "bochs" | "bochs-drm" | "cirrus" | "cirrus-qemu"
    )
}

pub fn run(tokens: Tokens, bench: bool, program: Option<Program>) -> Result<()> {
    let mut event_loop: EventLoop<Edel> =
        EventLoop::try_new().context("creating the event loop")?;
    let display: Display<Edel> = Display::new().context("creating the Wayland display")?;
    let mut state = Edel::new(display.handle(), event_loop.get_signal(), tokens)?;
    let handle = event_loop.handle();
    let name = listen(&handle, &mut state, display)?;
    state.socket = name.clone();
    crate::xwayland::listen(&handle, &mut state, &name);
    state.program = program;
    crate::decoration::load_text(&handle, &state.tokens.font, state.tokens.title_text_size);
    crate::watch::start(&handle, &mut state);

    let (mut session, session_events) = open_seat()?;
    let seat = session.seat();
    let path = wait_for_gpu(&seat)?;
    let fd = session
        .open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .with_context(|| messages::gpu_open(&path))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let driver = smithay::reexports::drm::Device::get_driver(&fd)
        .map(|d| d.name().to_string_lossy().into_owned())
        .unwrap_or_default();
    let whole_frames = draws_whole(&driver);
    if whole_frames {
        eprintln!(
            "edel-compositor: screen driver {driver}: drawing every frame whole, as it shows only the areas a frame names"
        );
    } else {
        eprintln!("edel-compositor: screen driver {driver}");
    }
    let (drm, drm_events) =
        DrmDevice::new(fd.clone(), true).context("opening the GPU for display")?;
    let gbm = GbmDevice::new(fd).context("opening the GPU for buffers")?;
    // SAFETY: the GBM device lives as long as the display and the renderer.
    let egl = unsafe { EGLDisplay::new(gbm.clone()) }.context(messages::NO_EGL)?;
    let context = EGLContext::new(&egl).context("creating the GL context")?;
    // SAFETY: the context is current only on this thread.
    let mut renderer =
        unsafe { GlesRenderer::new(context) }.context("starting the GLES renderer")?;
    state.start_effects(&crate::tiers::renderer_name(&mut renderer));
    state.start_animations(&renderer);

    let mut libinput = Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));
    if libinput.udev_assign_seat(&seat).is_err() {
        bail!("libinput could not take seat {seat}");
    }
    let mut switcher = session.clone();
    handle
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            move |event, _, state: &mut Edel| {
                if let InputEvent::DeviceAdded { device } = &event {
                    configure(device.clone());
                }
                if let Some(vt) = state.input(event) {
                    if let Err(e) = switcher.change_vt(vt) {
                        eprintln!("edel-compositor: {}", messages::terminal_switch(vt, e));
                    }
                }
            },
        )
        .map_err(|e| anyhow::anyhow!("watching the input devices: {e}"))?;

    let gpu = Rc::new(RefCell::new(Gpu {
        handle: handle.clone(),
        _session: session,
        libinput,
        drm,
        gbm,
        renderer,
        screens: HashMap::new(),
        clock: Clock::new(),
        active: true,
        announced: false,
        started: Instant::now(),
        whole_frames,
    }));
    // Apps' GPU buffers (M5.19): the renderer's formats on the GPU's render
    // node, each buffer checked by importing it once.
    let device =
        DrmNode::from_path(&path)
            .ok()
            .map(|node| match node.node_with_type(NodeType::Render) {
                Some(Ok(render)) => render,
                _ => node,
            });
    if let Some(device) = device {
        let formats = gpu.borrow().renderer.dmabuf_formats().into_iter().collect();
        let importer = Rc::clone(&gpu);
        state.offer_dmabuf(
            device.dev_id(),
            formats,
            Rc::new(move |dmabuf| {
                importer
                    .borrow_mut()
                    .renderer
                    .import_dmabuf(dmabuf, None)
                    .is_ok()
            }),
        );
    }
    gpu.borrow_mut().scan(&mut state);
    if gpu.borrow().screens.is_empty() {
        // Healthy all the same (M4.8): with the screen unplugged or off,
        // the desktop waits for one, as udev will say, rather than leave
        // the boot guard restarting the machine for want of a frame.
        let line = "no screen connected yet; waiting for one";
        eprintln!("edel-compositor: {line}");
        write_ready(line);
    }

    let vblank = Rc::clone(&gpu);
    let later = handle.clone();
    let socket = name.clone();
    handle
        .insert_source(
            drm_events,
            move |event, metadata, state: &mut Edel| match event {
                DrmEvent::VBlank(crtc) => {
                    let mut gpu = vblank.borrow_mut();
                    gpu.presented(crtc, metadata.as_ref());
                    gpu.render(state);
                    // Once the desktop is on screen (M4.7b).
                    if gpu.announced {
                        crate::program::start(&later, state, &socket);
                        crate::shellui::start(&later, state, &socket);
                        crate::session::start(&later, state);
                    }
                }
                DrmEvent::Error(e) => eprintln!("edel-compositor: the GPU reported an error: {e}"),
            },
        )
        .map_err(|e| anyhow::anyhow!("watching the GPU: {e}"))?;
    // Screens plugged in or out.
    let device = gpu.borrow().drm.device_id();
    let plugged = Rc::clone(&gpu);
    let udev = UdevBackend::new(&seat).context("watching the GPU for screens")?;
    handle
        .insert_source(udev, move |event, _, state: &mut Edel| {
            if let UdevEvent::Changed { device_id } = event {
                if device_id == device {
                    plugged.borrow_mut().scan(state);
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("watching the GPU for screens: {e}"))?;
    let switch = Rc::clone(&gpu);
    handle
        .insert_source(session_events, move |event, _, state: &mut Edel| {
            let mut gpu = switch.borrow_mut();
            match event {
                SessionEvent::PauseSession => {
                    gpu.libinput.suspend();
                    gpu.drm.pause();
                    gpu.active = false;
                }
                SessionEvent::ActivateSession => {
                    if gpu.libinput.resume().is_err() {
                        eprintln!("edel-compositor: {}", messages::INPUT_LOST);
                    }
                    if let Err(e) = gpu.drm.activate(false) {
                        eprintln!("edel-compositor: {}", messages::screens_lost(e));
                    }
                    for screen in gpu.screens.values_mut() {
                        if let Err(e) = screen.compositor.reset_state() {
                            eprintln!("edel-compositor: resetting a screen failed: {e}");
                        }
                        screen.pending = false;
                    }
                    gpu.active = true;
                    state.dirty = true;
                    // Screens may have changed while another terminal had them.
                    gpu.scan(state);
                    gpu.render(state);
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("watching the seat: {e}"))?;
    if bench {
        handle
            .insert_source(
                Timer::from_duration(Duration::from_secs(5)),
                |_, _, state: &mut Edel| {
                    state.signal.stop();
                    TimeoutAction::Drop
                },
            )
            .map_err(|e| anyhow::anyhow!("starting the bench timer: {e}"))?;
    }

    eprintln!("edel-compositor: listening on {name}");
    gpu.borrow_mut().render(&mut state);
    event_loop.run(None, &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display.flush_clients();
        if let Ok(mut gpu) = gpu.try_borrow_mut() {
            if std::mem::take(&mut state.screens_changed) {
                gpu.scan(state);
            }
            gpu.render(state);
        }
    })?;
    if bench {
        println!("edel-compositor: {}", state.telemetry.summary());
    }
    Ok(())
}

/// A connector's name as the settings file and the state file use it, such
/// as `eDP-1` or `HDMI-A-1`.
fn connector_name(info: &connector::Info) -> String {
    format!("{}-{}", info.interface().as_str(), info.interface_id())
}

impl Gpu {
    /// Lights every connected screen the settings want, at its mode, darkens
    /// the ones gone or turned off, and places them all.
    fn scan(&mut self, state: &mut Edel) {
        if !self.active {
            return;
        }
        let Ok(resources) = self.drm.resource_handles() else {
            eprintln!("edel-compositor: {}", messages::SCREEN_LIST);
            return;
        };
        let mut wanted = Vec::new();
        for &handle in resources.connectors() {
            // Probed, not the kernel's cached answer: while the compositor
            // holds the GPU, nothing else reads a newly plugged screen's
            // modes.
            let Ok(info) = self.drm.get_connector(handle, true) else {
                continue;
            };
            let name = connector_name(&info);
            let on = state
                .settings
                .outputs
                .get(&name)
                .and_then(|o| o.enabled)
                .unwrap_or(true);
            if info.state() == connector::State::Connected && !info.modes().is_empty() && on {
                wanted.push(info);
            }
        }
        // Screens gone, turned off, or wanting another mode go dark first,
        // which frees their CRTCs.
        let gone: Vec<crtc::Handle> = self
            .screens
            .iter()
            .filter(|(_, s)| {
                wanted
                    .iter()
                    .find(|i| i.handle() == s.connector)
                    .is_none_or(|info| choose_mode(info, state) != s.mode)
            })
            .map(|(crtc, _)| *crtc)
            .collect();
        for crtc in gone {
            if let Some(screen) = self.screens.remove(&crtc) {
                eprintln!("edel-compositor: output {} off", screen.output.name());
                state.space.unmap_output(&screen.output);
            }
        }
        for info in &wanted {
            if self.screens.values().any(|s| s.connector == info.handle()) {
                continue;
            }
            if let Err(e) = self.light(info, state) {
                eprintln!(
                    "edel-compositor: {}",
                    messages::screen_dark(&connector_name(info), format!("{e:#}"))
                );
            }
        }
        // Left to right in connector order, unless the file places them.
        let mut lit: Vec<&Screen> = self.screens.values().collect();
        lit.sort_by_key(|s| {
            resources
                .connectors()
                .iter()
                .position(|c| *c == s.connector)
        });
        state.apply_scales();
        let sizes: Vec<_> = lit
            .iter()
            .map(|s| {
                let mode = s.output.current_mode().map(|m| m.size).unwrap_or_default();
                let size = mode
                    .to_f64()
                    .to_logical(s.output.current_scale().fractional_scale())
                    .to_i32_round();
                let position = state
                    .settings
                    .outputs
                    .get(&s.output.name())
                    .and_then(|o| o.position)
                    .map(Point::from);
                (size, position)
            })
            .collect();
        for (screen, at) in lit.iter().zip(place_screens(&sizes)) {
            state.space.map_output(&screen.output, at);
            screen
                .output
                .change_current_state(None, None, None, Some(at));
        }
        for screen in self.screens.values_mut() {
            screen.dirty = true;
        }
        state.outputs_changed();
    }

    /// Lights one screen: a free CRTC its encoders can drive, its mode, an
    /// output for clients and the layout, and frames for it.
    fn light(&mut self, info: &connector::Info, state: &mut Edel) -> Result<()> {
        let resources = self.drm.resource_handles()?;
        let used: Vec<crtc::Handle> = self.screens.keys().copied().collect();
        let crtc = info
            .encoders()
            .iter()
            .filter_map(|e| self.drm.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .find(|c| !used.contains(c))
            .context("no free CRTC drives it")?;
        let mode = choose_mode(info, state);
        let name = connector_name(info);
        let (mm_w, mm_h) = info.size().unwrap_or((0, 0));
        let output = Output::new(
            name,
            PhysicalProperties {
                size: (mm_w as i32, mm_h as i32).into(),
                subpixel: Subpixel::Unknown,
                make: "Edel OS".into(),
                model: "screen".into(),
            },
        );
        // Every mode the screen has, so the state file can list them
        // for Settings (M5.7a).
        for other in info.modes() {
            output.add_mode(Mode::from(*other));
        }
        let wl_mode = Mode::from(mode);
        output.change_current_state(Some(wl_mode), None, None, None);
        output.set_preferred(wl_mode);
        output.create_global::<Edel>(&state.display);
        // Built-in panels are seen from closer than monitors (M4.6a).
        let built_in = matches!(
            info.interface(),
            connector::Interface::EmbeddedDisplayPort
                | connector::Interface::LVDS
                | connector::Interface::DSI
        );
        let (w, h) = mode.size();
        crate::outputs::set_auto_scale(
            &output,
            auto_scale(
                (i32::from(w), i32::from(h)).into(),
                (mm_w as i32, mm_h as i32).into(),
                built_in,
            ),
        );
        let surface = self
            .drm
            .create_surface(crtc, mode, &[info.handle()])
            .context("setting up the screen")?;
        let compositor = DrmCompositor::new(
            &output,
            surface,
            None,
            GbmAllocator::new(
                self.gbm.clone(),
                GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
            ),
            GbmFramebufferExporter::new(self.gbm.clone(), None),
            [Fourcc::Argb8888, Fourcc::Xrgb8888],
            self.renderer.egl_context().dmabuf_render_formats().clone(),
            self.drm.cursor_size(),
            Some(self.gbm.clone()),
        )
        .map_err(|e| anyhow::anyhow!("setting up frames for the screen: {e}"))?;
        state.space.map_output(&output, (0, 0));
        self.screens.insert(
            crtc,
            Screen {
                output,
                connector: info.handle(),
                compositor,
                mode,
                dirty: true,
                pending: false,
                ready: false,
            },
        );
        Ok(())
    }

    /// Draws a frame on every screen where something changed, the seat is
    /// ours and the last frame has been shown; then lets clients draw their
    /// next one.
    fn render(&mut self, state: &mut Edel) {
        if std::mem::take(&mut state.dirty) {
            for screen in self.screens.values_mut() {
                screen.dirty = true;
            }
        }
        if std::mem::take(&mut state.repaint) || self.whole_frames {
            for screen in self.screens.values_mut() {
                screen.compositor.reset_buffer_ages();
            }
        }
        if !self.active {
            return;
        }
        let now = self.started.elapsed();
        for screen in self.screens.values_mut() {
            if !screen.dirty || screen.pending {
                continue;
            }
            screen.dirty = false;
            let started = Instant::now();
            if render_screen(screen, &mut self.renderer, state) {
                screen.pending = true;
                state.last_frame = Some(Instant::now());
                state.telemetry.frame(started.elapsed(), true);
                state.frame_drawn(
                    started.elapsed(),
                    crate::tiers::refresh_interval(&screen.output),
                );
                arm_report(&self.handle, state);
            }
            // A live overview shows pictures, not the windows' surfaces,
            // so the windows on this screen are told to draw by where they
            // are rather than by what the last frame showed (M5.2j-b4).
            let live = state.overview_is_live();
            for window in state.space.elements() {
                if live {
                    if state
                        .space
                        .outputs_for_element(window)
                        .contains(&screen.output)
                    {
                        let output = screen.output.clone();
                        window.send_frame(&screen.output, now, Some(Duration::ZERO), |_, _| {
                            Some(output.clone())
                        });
                    }
                    continue;
                }
                window.send_frame(
                    &screen.output,
                    now,
                    Some(Duration::ZERO),
                    surface_primary_scanout_output,
                );
            }
            crate::layers::send_frames(&screen.output, now);
            state.cursor_frame(&screen.output, now);
        }
        // While a window opens, closes or slides, the screens draw again
        // one refresh later (M5.11b): animations step with the display,
        // never faster, even where a frame is shown the moment it is
        // queued, as on a virtual GPU.
        if state.animations.running() {
            let interval = self
                .screens
                .values()
                .map(|s| crate::tiers::refresh_interval(&s.output))
                .min()
                .unwrap_or(Duration::from_micros(16_667));
            arm_animation(&self.handle, state, interval);
        }
    }

    /// The frame queued on `crtc` is shown: tell the clients when, and
    /// announce the screen's first one.
    fn presented(&mut self, crtc: crtc::Handle, metadata: Option<&DrmEventMetadata>) {
        let now = Duration::from(self.clock.now());
        let Some(screen) = self.screens.get_mut(&crtc) else {
            return;
        };
        screen.pending = false;
        match screen.compositor.frame_submitted() {
            Ok(Some(Some(mut feedback))) => {
                let (time, sequence) = match metadata {
                    Some(DrmEventMetadata {
                        time: DrmEventTime::Monotonic(time),
                        sequence,
                    }) => (*time, u64::from(*sequence)),
                    _ => (now, 0),
                };
                let refresh = screen
                    .output
                    .current_mode()
                    .filter(|mode| mode.refresh > 0)
                    .map_or(Refresh::Unknown, |mode| {
                        Refresh::Fixed(Duration::from_secs_f64(1000.0 / f64::from(mode.refresh)))
                    });
                feedback.presented::<_, Monotonic>(
                    time,
                    refresh,
                    sequence,
                    wp_presentation_feedback::Kind::Vsync
                        | wp_presentation_feedback::Kind::HwClock
                        | wp_presentation_feedback::Kind::HwCompletion,
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("edel-compositor: {}", messages::frame_not_shown(e)),
        }
        if !screen.ready {
            screen.ready = true;
            let first = !self.announced;
            self.announced = true;
            announce(&screen.output, first);
        }
    }
}

/// The mode `displays.NAME.resolution` and `refresh_rate` names, if the screen has it, else the
/// screen's preferred mode, else its first.
fn choose_mode(info: &connector::Info, state: &Edel) -> smithay::reexports::drm::control::Mode {
    let modes = info.modes();
    let wanted = state
        .settings
        .outputs
        .get(&connector_name(info))
        .and_then(|o| o.mode.as_deref())
        .and_then(parse_mode);
    let listed: Vec<(i32, i32, i32)> = modes
        .iter()
        .map(|m| {
            let (w, h) = m.size();
            // vrefresh is in Hz; pick_mode wants mHz.
            (i32::from(w), i32::from(h), m.vrefresh() as i32 * 1000)
        })
        .collect();
    if let Some(i) = wanted.and_then(|w| pick_mode(&listed, w)) {
        return modes[i];
    }
    modes
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .copied()
        .unwrap_or(modes[0])
}

/// Draws one screen; returns whether a frame was queued.
fn render_screen(screen: &mut Screen, renderer: &mut GlesRenderer, state: &mut Edel) -> bool {
    let Screen {
        compositor, output, ..
    } = screen;
    let elements = state.elements(renderer, output);
    let frame = match compositor.render_frame(
        renderer,
        &elements,
        state.tokens.background.rgba(),
        FrameFlags::DEFAULT,
    ) {
        Ok(frame) => frame,
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::frame_not_drawn(e));
            return false;
        }
    };
    for window in state.space.elements() {
        window.with_surfaces(|surface, data| {
            update_surface_primary_scanout_output(
                surface,
                output,
                data,
                &frame.states,
                default_primary_scanout_output_compare,
            );
        });
    }
    if frame.is_empty {
        return false;
    }
    // A driver without fences (bochs-drm and simpledrm, the ones
    // `draws_whole` lists) leaves the wait to us: without it, llvmpipe's
    // threads were still drawing when the frame went to the screen, and
    // a live stick's panel stayed partly black.
    if frame.needs_sync() {
        if let PrimaryPlaneElement::Swapchain(element) = &frame.primary_element {
            let _ = element.sync.wait();
        }
    }
    let mut feedback = OutputPresentationFeedback::new(output);
    for window in state.space.elements() {
        window.take_presentation_feedback(
            &mut feedback,
            surface_primary_scanout_output,
            |surface, _| surface_presentation_feedback_flags_from_states(surface, &frame.states),
        );
    }
    match compositor.queue_frame(Some(feedback)) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::frame_not_shown(e));
            false
        }
    }
}

/// A new input device's options (M4.6b): tapping a touchpad clicks and
/// typing pauses it, as most laptops' owners expect; presets change them
/// from M5.4.
fn configure(mut device: smithay::reexports::input::Device) {
    if device.config_tap_finger_count() > 0 {
        let _ = device.config_tap_set_enabled(true);
    }
    if device.config_dwt_is_available() {
        let _ = device.config_dwt_set_enabled(true);
    }
}

/// Logs a screen's first frame; the first screen's also writes the health
/// file. Outside an Edel OS session (no `/run/edel/session`) only the log
/// line appears.
fn announce(output: &Output, first: bool) {
    let size: Size<i32, _> = output.current_mode().map(|m| m.size).unwrap_or_default();
    let line = format!("output {} {}x{} ready", output.name(), size.w, size.h);
    eprintln!("edel-compositor: {line}");
    if first {
        write_ready(&line);
    }
}

/// Writes the health file, in an Edel OS session.
fn write_ready(line: &str) {
    let path = std::path::Path::new(READY);
    if path.parent().is_some_and(|dir| dir.is_dir()) {
        if let Err(e) = crate::statefile::replace(path, &format!("{line}\n")) {
            eprintln!("edel-compositor: {}", messages::ready_not_written(READY, e));
        }
    }
}

/// Once frames are being drawn, a timer logs the telemetry when the screen
/// has been still for [`QUIET`]; nothing wakes the compositor while idle.
/// Marks the screens dirty for the next step of an animation once a
/// refresh `interval` has passed without a frame; the event loop then
/// draws them. While clients' frames come anyway, each of them steps the
/// animation, so it adds no frame of its own: on a virtual GPU, which
/// shows a frame the moment it is queued, an extra frame would make the
/// next client frame wait for it.
fn arm_animation(handle: &LoopHandle<'static, Edel>, state: &mut Edel, interval: Duration) {
    if state.animation_armed {
        return;
    }
    state.animation_armed = true;
    let result = handle.insert_source(
        Timer::from_duration(interval),
        move |_, _, state: &mut Edel| {
            let since = state.last_frame.map_or(interval, |at| at.elapsed());
            if since < interval && state.animations.running() {
                return TimeoutAction::ToDuration(interval - since);
            }
            state.animation_armed = false;
            state.dirty = true;
            TimeoutAction::Drop
        },
    );
    if let Err(e) = result {
        eprintln!("edel-compositor: the animation timer did not start: {e}");
        state.animation_armed = false;
        state.dirty = true;
    }
}

fn arm_report(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    if state.report_armed {
        return;
    }
    state.report_armed = true;
    let result =
        handle.insert_source(
            Timer::from_duration(QUIET),
            |_, _, state: &mut Edel| match state.telemetry.still_for(QUIET) {
                Some(wait) => TimeoutAction::ToDuration(wait),
                None => {
                    eprintln!("edel-compositor: {}", state.telemetry.summary());
                    state.report_armed = false;
                    TimeoutAction::Drop
                }
            },
        );
    if let Err(e) = result {
        eprintln!("edel-compositor: the telemetry timer did not start: {e}");
        state.report_armed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_drivers_draw_whole_frames_and_gpus_do_not() {
        assert!(draws_whole("simpledrm"));
        assert!(draws_whole("bochs-drm"));
        assert!(!draws_whole("i915"));
        assert!(!draws_whole("virtio_gpu"));
        assert!(!draws_whole(""));
    }

    #[test]
    fn the_first_card_whose_file_exists_wins() {
        let p = |s: &str| std::path::PathBuf::from(s);
        let only = |live: &'static [&'static str]| {
            move |path: &Path| live.iter().any(|l| path == Path::new(l))
        };
        // The primary card, when its file is there.
        assert_eq!(
            pick_gpu(
                Some(p("/dev/dri/card0")),
                vec![p("/dev/dri/card0")],
                &[],
                only(&["/dev/dri/card0"])
            ),
            Some(p("/dev/dri/card0"))
        );
        // The framebuffer's card0 went and the driver's card came back as
        // card1, which udev lists while still calling card0 primary.
        assert_eq!(
            pick_gpu(
                Some(p("/dev/dri/card0")),
                vec![p("/dev/dri/card0"), p("/dev/dri/card1")],
                &[p("/dev/dri/card1")],
                only(&["/dev/dri/card1"])
            ),
            Some(p("/dev/dri/card1"))
        );
        // udev has not heard of card1 yet, but its file is there.
        assert_eq!(
            pick_gpu(
                Some(p("/dev/dri/card0")),
                vec![p("/dev/dri/card0")],
                &[p("/dev/dri/card1")],
                only(&["/dev/dri/card1"])
            ),
            Some(p("/dev/dri/card1"))
        );
        // Nothing there yet: wait for the card udev names.
        assert_eq!(
            pick_gpu(Some(p("/dev/dri/card0")), vec![], &[], only(&[])),
            Some(p("/dev/dri/card0"))
        );
        assert_eq!(pick_gpu(None, vec![], &[], only(&[])), None);
    }

    #[test]
    fn the_file_system_on_dev_is_the_last_mounted_there() {
        let mounts =
            "proc /proc proc rw 0 0\ndevtmpfs /dev devtmpfs rw 0 0\ntmpfs /dev tmpfs rw 0 0\n";
        assert_eq!(dev_mount(mounts), "tmpfs");
        assert_eq!(dev_mount("proc /proc proc rw 0 0\n"), "not mounted");
    }

    #[test]
    fn a_listing_is_sorted_and_says_empty_or_why() {
        let dir = std::env::temp_dir().join(format!("edel-listing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(listing(&dir), "empty");
        std::fs::write(dir.join("renderD128"), "").unwrap();
        std::fs::write(dir.join("card0"), "").unwrap();
        assert_eq!(listing(&dir), "card0 renderD128");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(listing(&dir), "entity not found");
    }
}
