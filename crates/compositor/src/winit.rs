//! The development backend: the compositor in a window of the desktop it
//! runs on (smithay's winit backend), for working on it without a VM. The
//! virtual GPU and real hardware get the DRM backend in M4.2b.
//!
//! A frame is drawn only when something changed (`Edel::dirty`), so an idle
//! desktop draws nothing; `--bench` runs for 5 s and prints the frame
//! telemetry.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputEvent, KeyboardKeyEvent, PointerButtonEvent,
};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::desktop::space::render_output;
use smithay::input::keyboard::FilterResult;
use smithay::input::pointer::{ButtonEvent, MotionEvent};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, Interest, Mode as TriggerMode, PostAction};
use smithay::reexports::wayland_server::Display;
use smithay::utils::{SERIAL_COUNTER, Transform};
use smithay::wayland::socket::ListeningSocketSource;

use edel_compositor::tokens::Tokens;

use crate::state::{Edel, new_client_data};

/// How long `--bench` measures.
const BENCH: Duration = Duration::from_secs(5);

struct Window {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage: OutputDamageTracker,
    started: Instant,
}

pub fn run(tokens: Tokens, bench: bool) -> Result<()> {
    let mut event_loop: EventLoop<Edel> =
        EventLoop::try_new().context("creating the event loop")?;
    let display: Display<Edel> = Display::new().context("creating the Wayland display")?;
    let mut state = Edel::new(display.handle(), event_loop.get_signal(), tokens)?;
    let handle = event_loop.handle();

    let socket = ListeningSocketSource::new_auto().context("opening a Wayland socket")?;
    let name = socket.socket_name().to_string_lossy().into_owned();
    handle
        .insert_source(socket, |stream, _, state: &mut Edel| {
            if let Err(e) = state.display.insert_client(stream, new_client_data()) {
                eprintln!("edel-compositor: a client could not connect: {e}");
            }
        })
        .map_err(|e| anyhow::anyhow!("listening on the socket: {e}"))?;
    handle
        .insert_source(
            Generic::new(display, Interest::READ, TriggerMode::Level),
            |_, display, state: &mut Edel| {
                // SAFETY: the display is never dropped while the loop runs.
                unsafe { display.get_mut().dispatch_clients(state)? };
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("watching the display: {e}"))?;

    let (backend, events) = winit::init::<GlesRenderer>()
        .map_err(|e| anyhow::anyhow!("opening a window on this desktop: {e}"))?;
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
            state.dirty = true;
        }
        WinitEvent::Input(event) => input(state, event),
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
            render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                &window.output,
                renderer,
                &mut framebuffer,
                1.0,
                age,
                [&state.space],
                &[],
                &mut window.damage,
                background,
            )
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
}

/// Keyboard to the focused window; the pointer focuses and clicks the
/// window under it.
fn input(state: &mut Edel, event: InputEvent<winit::WinitInput>) {
    let serial = SERIAL_COUNTER.next_serial();
    match event {
        InputEvent::Keyboard { event } => {
            if let Some(keyboard) = state.seat.get_keyboard() {
                keyboard.input::<(), _>(
                    state,
                    event.key_code(),
                    event.state(),
                    serial,
                    event.time_msec(),
                    |_, _, _| FilterResult::Forward,
                );
            }
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let Some(output) = state.space.outputs().next() else {
                return;
            };
            let Some(area) = state.space.output_geometry(output) else {
                return;
            };
            let location = event.position_transformed(area.size) + area.loc.to_f64();
            let focus = state.surface_under(location);
            if let Some(pointer) = state.seat.get_pointer() {
                pointer.motion(
                    state,
                    focus,
                    &MotionEvent {
                        location,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(state);
            }
        }
        InputEvent::PointerButton { event } => {
            let Some(pointer) = state.seat.get_pointer() else {
                return;
            };
            if event.state() == ButtonState::Pressed {
                let location = pointer.current_location();
                let window = state.space.element_under(location).map(|(w, _)| w.clone());
                if let Some(window) = window {
                    state.space.raise_element(&window, true);
                    let surface = window.toplevel().map(|t| t.wl_surface().clone());
                    if let Some(keyboard) = state.seat.get_keyboard() {
                        keyboard.set_focus(state, surface, serial);
                    }
                    state.dirty = true;
                }
            }
            pointer.button(
                state,
                &ButtonEvent {
                    button: event.button_code(),
                    state: event.state(),
                    serial,
                    time: event.time_msec(),
                },
            );
            pointer.frame(state);
        }
        _ => {}
    }
}
