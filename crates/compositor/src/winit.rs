//! The development backend: the compositor in a window of the desktop it
//! runs on (smithay's winit backend), for working on it without a VM. The
//! virtual GPU and real hardware use `drm.rs`; this one runs when
//! `WAYLAND_DISPLAY` or `DISPLAY` says there is a desktop to run in.
//!
//! A frame is drawn only when something changed (`Edel::dirty`), so an idle
//! desktop draws nothing; `--bench` runs for 5 s and prints the frame
//! telemetry.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::Display;
use smithay::utils::Transform;

use edel_compositor::tokens::Tokens;

use crate::program::Program;
use crate::state::Edel;

/// How long `--bench` measures.
const BENCH: Duration = Duration::from_secs(5);

struct Window {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage: OutputDamageTracker,
    started: Instant,
}

pub fn run(tokens: Tokens, bench: bool, program: Option<Program>) -> Result<()> {
    let mut event_loop: EventLoop<Edel> =
        EventLoop::try_new().context("creating the event loop")?;
    let display: Display<Edel> = Display::new().context("creating the Wayland display")?;
    let mut state = Edel::new(display.handle(), event_loop.get_signal(), tokens)?;
    let handle = event_loop.handle();

    let name = crate::state::listen(&handle, display)?;
    crate::xwayland::listen(&handle, &mut state, &name);
    state.program = program;
    crate::decoration::load_text(&handle, state.tokens.title_text_size);
    crate::watch::start(&handle, &mut state);

    let (mut backend, events) = winit::init::<GlesRenderer>()
        .map_err(|e| anyhow::anyhow!("opening a window on this desktop: {e}"))?;
    state.start_effects(&crate::tiers::renderer_name(backend.renderer()));
    let size = backend.window_size();
    let output = Output::new(
        "winit".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Edel OS".into(),
            model: "development window".into(),
        },
    );
    let mode = Mode {
        size,
        refresh: 60_000,
    };
    output.create_global::<Edel>(&state.display);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    state.space.map_output(&output, (0, 0));
    state.apply_scales();
    state.outputs_changed();
    // From inside the loop, so a program that cannot start ends it.
    let (later, socket) = (handle.clone(), name.clone());
    handle.insert_idle(move |state| {
        crate::program::start(&later, state, &socket);
        crate::shellui::start(&later, state, &socket);
    });
    let window = Rc::new(RefCell::new(Window {
        damage: OutputDamageTracker::from_output(&output),
        backend,
        output,
        started: Instant::now(),
    }));

    if bench {
        handle
            .insert_source(Timer::from_duration(BENCH), |_, _, state: &mut Edel| {
                state.signal.stop();
                TimeoutAction::Drop
            })
            .map_err(|e| anyhow::anyhow!("starting the bench timer: {e}"))?;
    }
    let events_window = Rc::clone(&window);
    handle
        .insert_source(events, move |event, _, state: &mut Edel| {
            on_event(&mut events_window.borrow_mut(), state, event)
        })
        .map_err(|e| anyhow::anyhow!("watching the window: {e}"))?;

    eprintln!("edel-compositor: listening on {name}");
    event_loop.run(None, &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        let _ = state.display.flush_clients();
        // A client's commit or new window marks the screen dirty outside
        // the window's events; ask for a frame here, once per dispatch.
        if state.dirty {
            window.borrow().backend.window().request_redraw();
        }
    })?;
    if bench {
        println!("edel-compositor: {}", state.telemetry.summary());
    }
    Ok(())
}

fn on_event(window: &mut Window, state: &mut Edel, event: WinitEvent) {
    match event {
        WinitEvent::Resized { size, .. } => {
            let mode = Mode {
                size,
                refresh: 60_000,
            };
            window
                .output
                .change_current_state(Some(mode), None, None, None);
            window.output.set_preferred(mode);
            state.outputs_changed();
        }
        WinitEvent::Input(event) => {
            // The host desktop switches its own terminals.
            let _ = state.input(event);
        }
        WinitEvent::Redraw => {
            if state.dirty {
                draw(window, state);
            }
        }
        WinitEvent::CloseRequested => state.signal.stop(),
        _ => {}
    }
}

fn draw(window: &mut Window, state: &mut Edel) {
    let started = Instant::now();
    let age = window.backend.buffer_age().unwrap_or(0);
    let background = state.tokens.background.rgba();
    let damage = match window.backend.bind() {
        Ok((renderer, mut framebuffer)) => {
            let elements = state.elements(renderer, &window.output);
            window
                .damage
                .render_output(renderer, &mut framebuffer, age, &elements, background)
                .map(|result| result.damage.cloned())
        }
        Err(e) => {
            eprintln!("edel-compositor: binding the window failed: {e}");
            return;
        }
    };
    match damage {
        Ok(Some(damage)) => {
            if let Err(e) = window.backend.submit(Some(&damage)) {
                eprintln!("edel-compositor: showing the frame failed: {e}");
            }
            state.telemetry.frame(started.elapsed(), !damage.is_empty());
            state.frame_drawn(
                started.elapsed(),
                crate::tiers::refresh_interval(&window.output),
            );
        }
        Ok(None) => {}
        Err(e) => eprintln!("edel-compositor: rendering failed: {e:?}"),
    }
    state.dirty = false;
    let now = window.started.elapsed();
    for w in state.space.elements() {
        w.send_frame(&window.output, now, Some(Duration::ZERO), |_, _| {
            Some(window.output.clone())
        });
    }
    crate::layers::send_frames(&window.output, now);
    state.cursor_frame(&window.output, now);
}
