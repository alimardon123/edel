//! The one event loop every backend runs in (the whole-system review of
//! 2026-10-25): it makes the Wayland display and `Edel`, listens, opens
//! each backend's screens and inputs, and lets every backend draw once per
//! dispatch. A backend is a [`Backend`]: `drm.rs` for real screens and
//! `winit.rs` for the development window. A virtual screen or a paired
//! device's screen (ADR-011, section 19) is one more, opened beside them.
//! A backend opened after another starts no effects of its own; the step
//! that adds a second one decides whose renderer picks the tier.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, LoopHandle};
use smithay::reexports::wayland_server::Display;

use edel_compositor::tokens::Tokens;

use crate::program::Program;
use crate::state::{Edel, listen};

/// How long `--bench` measures.
const BENCH: Duration = Duration::from_secs(5);

/// Screens and inputs of one kind. Its own events (vblanks, input, plugs)
/// come through the sources its opener put in the loop; these two are
/// what the loop asks of every backend.
pub trait Backend {
    /// Looks for screens again, after a change to `[displays]`.
    fn scan(&mut self, state: &mut Edel);
    /// Draws what changed on its screens, once per dispatch.
    fn render(&mut self, state: &mut Edel);
}

/// Opens a backend: puts its event sources in the loop and returns it.
pub type Open = fn(&LoopHandle<'static, Edel>, &mut Edel) -> Result<Rc<RefCell<dyn Backend>>>;

pub fn run(tokens: Tokens, bench: bool, program: Option<Program>, opens: &[Open]) -> Result<()> {
    let mut event_loop: EventLoop<Edel> =
        EventLoop::try_new().context("creating the event loop")?;
    let display: Display<Edel> = Display::new().context("creating the Wayland display")?;
    let mut state = Edel::new(
        display.handle(),
        event_loop.get_signal(),
        event_loop.handle(),
        tokens,
    )?;
    let handle = event_loop.handle();
    let name = listen(&handle, &mut state, display)?;
    state.socket = name.clone();
    crate::off_loop::start(&handle, &mut state);
    crate::xwayland::listen(&handle, &mut state, &name);
    state.program = program;
    state.game.session = state.game_session();
    crate::decoration::load_text(&handle, &state.tokens.font, state.tokens.title_text_size);
    crate::watch::start(&handle, &mut state);

    let backends = opens
        .iter()
        .map(|open| open(&handle, &mut state))
        .collect::<Result<Vec<_>>>()?;
    if bench {
        handle
            .insert_source(Timer::from_duration(BENCH), |_, _, state: &mut Edel| {
                state.signal.stop();
                TimeoutAction::Drop
            })
            .map_err(|e| anyhow::anyhow!("starting the bench timer: {e}"))?;
    }

    eprintln!("edel-compositor: listening on {name}");
    event_loop.run(None, &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        state.sync_idle_inhibit();
        let _ = state.display.flush_clients();
        let scan = std::mem::take(&mut state.screens_changed);
        for backend in &backends {
            if let Ok(mut backend) = backend.try_borrow_mut() {
                if scan {
                    backend.scan(state);
                }
                backend.render(state);
            }
        }
    })?;
    if bench {
        println!("edel-compositor: {}", state.telemetry.summary());
    }
    Ok(())
}
