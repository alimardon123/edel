//! The backend for real screens: the virtual GPU in CI and a laptop's GPU
//! (roadmap M4.2b, M4.6c). A libseat session (seatd, ADR-002) opens the
//! primary GPU and the input devices; GBM allocates the buffers, EGL and
//! GLES draw them (one renderer, no fallback), and smithay's DRM compositor
//! puts them on every connected screen, each with a CRTC of its own, at
//! `outputs.NAME.mode` or its preferred mode, at `outputs.NAME.position` or
//! right of the screens before it, unless `outputs.NAME.enabled` is false.
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
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
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
use edel_compositor::tokens::Tokens;

use crate::state::{Edel, listen};

/// Where the session tells root it is up (M4.1, M4.8).
const READY: &str = "/run/edel/session/ready";

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
    /// The mode it runs at, to notice a new `outputs.NAME.mode`.
    mode: smithay::reexports::drm::control::Mode,
    /// Something on screen changed since its last frame.
    dirty: bool,
    /// A frame was queued and its vblank has not come yet.
    pending: bool,
    /// Its first frame was shown and announced.
    ready: bool,
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
}

pub fn run(tokens: Tokens, bench: bool) -> Result<()> {
    let mut event_loop: EventLoop<Edel> =
        EventLoop::try_new().context("creating the event loop")?;
    let display: Display<Edel> = Display::new().context("creating the Wayland display")?;
    let mut state = Edel::new(display.handle(), event_loop.get_signal(), tokens)?;
    let handle = event_loop.handle();
    let name = listen(&handle, display)?;
    crate::xwayland::listen(&handle, &mut state, &name);
    crate::decoration::load_text(&handle, state.tokens.title_text_size);
    crate::watch::start(&handle, &mut state);

    let (mut session, session_events) = LibSeatSession::new()
        .context("opening a seat session (is seatd running and is this user in group seat?)")?;
    let seat = session.seat();
    let path = match primary_gpu(&seat).context("looking for the primary GPU")? {
        Some(path) => path,
        None => all_gpus(&seat)
            .context("looking for a GPU")?
            .into_iter()
            .next()
            .context("no GPU found")?,
    };
    let fd = session
        .open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .with_context(|| format!("opening {}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_events) =
        DrmDevice::new(fd.clone(), true).context("opening the GPU for display")?;
    let gbm = GbmDevice::new(fd).context("opening the GPU for buffers")?;
    // SAFETY: the GBM device lives as long as the display and the renderer.
    let egl = unsafe { EGLDisplay::new(gbm.clone()) }.context("starting EGL")?;
    let context = EGLContext::new(&egl).context("creating the GL context")?;
    // SAFETY: the context is current only on this thread.
    let renderer = unsafe { GlesRenderer::new(context) }.context("starting the GLES renderer")?;

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
                        eprintln!("edel-compositor: switching to terminal {vt} failed: {e}");
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
    }));
    gpu.borrow_mut().scan(&mut state);
    if gpu.borrow().screens.is_empty() {
        bail!("no connected screen");
    }

    let vblank = Rc::clone(&gpu);
    handle
        .insert_source(
            drm_events,
            move |event, metadata, state: &mut Edel| match event {
                DrmEvent::VBlank(crtc) => {
                    let mut gpu = vblank.borrow_mut();
                    gpu.presented(crtc, metadata.as_ref());
                    gpu.render(state);
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
                        eprintln!("edel-compositor: input devices did not come back");
                    }
                    if let Err(e) = gpu.drm.activate(false) {
                        eprintln!("edel-compositor: the screens did not come back: {e}");
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

/// A connector's name as the system file and the state file use it, such
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
            eprintln!("edel-compositor: reading the GPU's screens failed");
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
                    "edel-compositor: output {} stays dark: {e:#}",
                    connector_name(info)
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
                state.telemetry.frame(started.elapsed(), true);
                arm_report(&self.handle, state);
            }
            for window in state.space.elements() {
                window.send_frame(
                    &screen.output,
                    now,
                    Some(Duration::ZERO),
                    surface_primary_scanout_output,
                );
            }
            state.cursor_frame(&screen.output, now);
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
            Err(e) => eprintln!("edel-compositor: the frame was not shown: {e}"),
        }
        if !screen.ready {
            screen.ready = true;
            let first = !self.announced;
            self.announced = true;
            announce(&screen.output, first);
        }
    }
}

/// The mode `outputs.NAME.mode` names, if the screen has it, else the
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
            eprintln!("edel-compositor: rendering failed: {e}");
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
            eprintln!("edel-compositor: showing the frame failed: {e}");
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
    if first
        && std::path::Path::new(READY)
            .parent()
            .is_some_and(|dir| dir.is_dir())
    {
        if let Err(e) = std::fs::write(READY, format!("{line}\n")) {
            eprintln!("edel-compositor: writing {READY} failed: {e}");
        }
    }
}

/// Once frames are being drawn, a timer logs the telemetry when the screen
/// has been still for [`QUIET`]; nothing wakes the compositor while idle.
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
