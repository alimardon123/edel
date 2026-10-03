//! The backend for real screens: the virtual GPU in CI and a laptop's GPU
//! (roadmap M4.2b). A libseat session (seatd, ADR-002) opens the primary
//! GPU and the input devices; GBM allocates the buffers, EGL and GLES draw
//! them (one renderer, no fallback), and smithay's DRM compositor puts
//! them on one screen, the first connected one at its preferred mode.
//!
//! A frame is drawn when something changed and the previous one has been
//! shown (its vblank), so an idle desktop draws nothing and a busy one at
//! most once per refresh. After the first frame is on screen the compositor
//! writes `/run/edel/session/ready`, the health file the boot guard waits
//! for (M4.8), and logs `edel-compositor: output NAME WxH ready`. Once the
//! screen has been still for 2 s after drawing, it logs its frame
//! telemetry, which CI reads.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmEventTime};
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::element::default_primary_scanout_output_compare;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::{all_gpus, primary_gpu};
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
use smithay::utils::{Clock, DeviceFd, Monotonic};
use smithay::wayland::presentation::Refresh;

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

struct Gpu {
    handle: LoopHandle<'static, Edel>,
    _session: LibSeatSession,
    libinput: Libinput,
    drm: DrmDevice,
    compositor: Compositor,
    renderer: GlesRenderer,
    output: Output,
    clock: Clock<Monotonic>,
    /// The session has the seat (not switched away to another VT).
    active: bool,
    /// A frame was queued and its vblank has not come yet.
    pending: bool,
    /// The first frame was shown and announced.
    ready: bool,
    started: Instant,
}

pub fn run(tokens: Tokens, bench: bool) -> Result<()> {
    let mut event_loop: EventLoop<Edel> =
        EventLoop::try_new().context("creating the event loop")?;
    let display: Display<Edel> = Display::new().context("creating the Wayland display")?;
    let mut state = Edel::new(display.handle(), event_loop.get_signal(), tokens)?;
    let handle = event_loop.handle();
    let name = listen(&handle, display)?;

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
    let (mut drm, drm_events) =
        DrmDevice::new(fd.clone(), true).context("opening the GPU for display")?;
    let gbm = GbmDevice::new(fd).context("opening the GPU for buffers")?;
    // SAFETY: the GBM device lives as long as the display and the renderer.
    let egl = unsafe { EGLDisplay::new(gbm.clone()) }.context("starting EGL")?;
    let context = EGLContext::new(&egl).context("creating the GL context")?;
    // SAFETY: the context is current only on this thread.
    let renderer = unsafe { GlesRenderer::new(context) }.context("starting the GLES renderer")?;

    let (connector, crtc, mode, info) = first_screen(&drm)?;
    let output_name = format!("{}-{}", info.interface().as_str(), info.interface_id());
    let (mm_w, mm_h) = info.size().unwrap_or((0, 0));
    let output = Output::new(
        output_name,
        PhysicalProperties {
            size: (mm_w as i32, mm_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "Edel OS".into(),
            model: "screen".into(),
        },
    );
    let wl_mode = Mode::from(mode);
    output.change_current_state(Some(wl_mode), None, None, Some((0, 0).into()));
    output.set_preferred(wl_mode);
    output.create_global::<Edel>(&state.display);
    state.space.map_output(&output, (0, 0));

    let surface = drm
        .create_surface(crtc, mode, &[connector])
        .context("setting up the screen")?;
    let compositor = DrmCompositor::new(
        &output,
        surface,
        None,
        GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        GbmFramebufferExporter::new(gbm.clone(), None),
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        renderer.egl_context().dmabuf_render_formats().clone(),
        drm.cursor_size(),
        Some(gbm),
    )
    .map_err(|e| anyhow::anyhow!("setting up frames for the screen: {e}"))?;

    let mut libinput = Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));
    if libinput.udev_assign_seat(&seat).is_err() {
        bail!("libinput could not take seat {seat}");
    }
    let mut switcher = session.clone();
    handle
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            move |event, _, state: &mut Edel| {
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
        compositor,
        renderer,
        output,
        clock: Clock::new(),
        active: true,
        pending: false,
        ready: false,
        started: Instant::now(),
    }));

    let vblank = Rc::clone(&gpu);
    handle
        .insert_source(
            drm_events,
            move |event, metadata, state: &mut Edel| match event {
                DrmEvent::VBlank(_) => {
                    let mut gpu = vblank.borrow_mut();
                    presented(&mut gpu, metadata.as_ref());
                    render(&mut gpu, state);
                }
                DrmEvent::Error(e) => eprintln!("edel-compositor: the GPU reported an error: {e}"),
            },
        )
        .map_err(|e| anyhow::anyhow!("watching the GPU: {e}"))?;
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
                        eprintln!("edel-compositor: the screen did not come back: {e}");
                    }
                    if let Err(e) = gpu.compositor.reset_state() {
                        eprintln!("edel-compositor: resetting the screen failed: {e}");
                    }
                    gpu.active = true;
                    gpu.pending = false;
                    state.dirty = true;
                    render(&mut gpu, state);
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
    render(&mut gpu.borrow_mut(), &mut state);
    event_loop.run(None, &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display.flush_clients();
        if let Ok(mut gpu) = gpu.try_borrow_mut() {
            render(&mut gpu, state);
        }
    })?;
    if bench {
        println!("edel-compositor: {}", state.telemetry.summary());
    }
    Ok(())
}

/// The first connected connector, its preferred mode (else its first) and
/// a CRTC that can drive it.
fn first_screen(
    drm: &DrmDevice,
) -> Result<(
    connector::Handle,
    crtc::Handle,
    smithay::reexports::drm::control::Mode,
    connector::Info,
)> {
    let resources = drm
        .resource_handles()
        .context("reading the GPU's outputs")?;
    for &handle in resources.connectors() {
        let info = drm
            .get_connector(handle, false)
            .context("reading a connector")?;
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }
        let modes = info.modes();
        let mode = modes
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .copied()
            .unwrap_or(modes[0]);
        for &encoder in info.encoders() {
            let Ok(encoder) = drm.get_encoder(encoder) else {
                continue;
            };
            if let Some(&crtc) = resources.filter_crtcs(encoder.possible_crtcs()).first() {
                return Ok((handle, crtc, mode, info));
            }
        }
    }
    bail!("no connected screen")
}

/// Draws a frame if something changed, the screen is ours and the last
/// frame has been shown; then lets clients draw their next one.
fn render(gpu: &mut Gpu, state: &mut Edel) {
    if !state.dirty || gpu.pending || !gpu.active {
        return;
    }
    state.dirty = false;
    let started = Instant::now();
    let Gpu {
        compositor,
        renderer,
        output,
        ..
    } = gpu;
    let elements = match state
        .space
        .render_elements_for_output(renderer, output, 1.0)
    {
        Ok(elements) => elements,
        Err(e) => {
            eprintln!("edel-compositor: listing what to draw failed: {e:?}");
            return;
        }
    };
    let queued = match compositor.render_frame(
        renderer,
        &elements,
        state.tokens.background.rgba(),
        FrameFlags::DEFAULT,
    ) {
        Ok(frame) => {
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
                false
            } else {
                let mut feedback = OutputPresentationFeedback::new(output);
                for window in state.space.elements() {
                    window.take_presentation_feedback(
                        &mut feedback,
                        surface_primary_scanout_output,
                        |surface, _| {
                            surface_presentation_feedback_flags_from_states(surface, &frame.states)
                        },
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
        }
        Err(e) => {
            eprintln!("edel-compositor: rendering failed: {e}");
            false
        }
    };
    if queued {
        gpu.pending = true;
        state.telemetry.frame(started.elapsed(), true);
        arm_report(&gpu.handle, state);
    }
    let now = gpu.started.elapsed();
    for window in state.space.elements() {
        window.send_frame(
            &gpu.output,
            now,
            Some(Duration::ZERO),
            surface_primary_scanout_output,
        );
    }
}

/// The queued frame is on screen: tell the clients when, and announce the
/// first one.
fn presented(gpu: &mut Gpu, metadata: Option<&DrmEventMetadata>) {
    gpu.pending = false;
    match gpu.compositor.frame_submitted() {
        Ok(Some(Some(mut feedback))) => {
            let (time, sequence) = match metadata {
                Some(DrmEventMetadata {
                    time: DrmEventTime::Monotonic(time),
                    sequence,
                }) => (*time, u64::from(*sequence)),
                _ => (Duration::from(gpu.clock.now()), 0),
            };
            let refresh = gpu
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
    if !gpu.ready {
        gpu.ready = true;
        announce(&gpu.output);
    }
}

/// Logs the screen and writes the health file. Outside an Edel OS session
/// (no `/run/edel/session`) only the log line appears.
fn announce(output: &Output) {
    let size = output.current_mode().map(|m| m.size).unwrap_or_default();
    let line = format!("output {} {}x{} ready", output.name(), size.w, size.h);
    eprintln!("edel-compositor: {line}");
    if std::path::Path::new(READY)
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
