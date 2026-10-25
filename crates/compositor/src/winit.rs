//! The development backend: the compositor in a window of the desktop it
//! runs on (smithay's winit backend), for working on it without a VM. The
//! virtual GPU and real hardware use `drm.rs`; this one runs when
//! `WAYLAND_DISPLAY` or `DISPLAY` says there is a desktop to run in.
//!
//! A frame is drawn only when something changed (`Edel::dirty`), so an idle
//! desktop draws nothing; `--bench` runs for 5 s and prints the frame
//! telemetry. It runs in `backend.rs`'s loop, as `drm.rs` does.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent, WinitGraphicsBackend};
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::LoopHandle;
use smithay::utils::Transform;

use crate::backend::Backend;
use crate::state::Edel;

struct Window {
    backend: WinitGraphicsBackend<GlesRenderer>,
    output: Output,
    damage: OutputDamageTracker,
    started: Instant,
}

/// Opens a window on the desktop the compositor runs in (`backend::Open`).
pub fn open(
    handle: &LoopHandle<'static, Edel>,
    state: &mut Edel,
) -> Result<Rc<RefCell<dyn Backend>>> {
    let (mut backend, events) = winit::init::<GlesRenderer>()
        .map_err(|e| anyhow::anyhow!("opening a window on this desktop: {e}"))?;
    state.start_effects(&crate::tiers::renderer_name(backend.renderer()));
    state.start_animations(backend.renderer());
    state.start_rounded(backend.renderer());
    state.start_backdrop(backend.renderer());
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
    let (later, socket) = (handle.clone(), state.socket.clone());
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
    let events_window = Rc::clone(&window);
    handle
        .insert_source(events, move |event, _, state: &mut Edel| {
            on_event(&mut events_window.borrow_mut(), state, event)
        })
        .map_err(|e| anyhow::anyhow!("watching the window: {e}"))?;
    Ok(window)
}

impl Backend for Window {
    /// One window, one screen: nothing to look for.
    fn scan(&mut self, _: &mut Edel) {}

    /// A client's commit or new window marks the screen dirty outside the
    /// window's events; ask for a frame here, once per dispatch.
    fn render(&mut self, state: &mut Edel) {
        if state.dirty {
            self.backend.window().request_redraw();
        }
    }
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
    // Age 0 draws the whole window, for a new background (M5.5c).
    let age = if std::mem::take(&mut state.repaint) {
        0
    } else {
        window.backend.buffer_age().unwrap_or(0)
    };
    let background = state.tokens.background.rgba();
    let damage = match window.backend.bind() {
        Ok((renderer, mut framebuffer)) => {
            let elements = state.elements(renderer, &window.output);
            let drawn = window
                .damage
                .render_output(renderer, &mut framebuffer, age, &elements, background)
                .map(|result| result.damage.cloned());
            // Screen copies asked for this output (M6.3), from what it drew.
            state.fill_copies(renderer, &window.output, &elements);
            drawn
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
            state.lock_drawn(&window.output.name());
            state.frame_drawn(
                started.elapsed(),
                crate::tiers::refresh_interval(&window.output),
            );
        }
        Ok(None) => {}
        Err(e) => eprintln!("edel-compositor: rendering failed: {e:?}"),
    }
    // While a window opens, closes or slides, the next frame is wanted
    // too (M5.11b).
    state.dirty = state.animations.running();
    let now = window.started.elapsed();
    for w in state.space.elements() {
        // Behind a game, a window draws again once it is visible (M7.7c).
        if state.behind_game(w) {
            continue;
        }
        w.send_frame(&window.output, now, Some(Duration::ZERO), |_, _| {
            Some(window.output.clone())
        });
    }
    // A game plays alone on its screen: only the layers over it get
    // frames (M7.7c).
    let only = state
        .playing_on(&window.output)
        .is_some()
        .then(|| state.layers_over(&window.output));
    crate::layers::send_frames(&window.output, now, only);
    state.lock_frames(&window.output, now);
    state.cursor_frame(&window.output, now);
}
