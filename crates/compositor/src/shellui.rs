//! shell-ui, the second long-running process (ADR-002, M5.1b): the
//! compositor starts `edel-shell-ui` once the desktop is on screen, in
//! every session but the greeter's (which runs one program, M4.7b), and
//! starts it again when it ends, so a crashed panel comes back and never
//! takes the windows with it. One that keeps failing within seconds of
//! starting is given up on after a few tries, and the log says so, rather
//! than burning the CPU. An image without it (the `shell` feature) is
//! said once and left alone.

use std::io::ErrorKind;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel::{self, Event};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::state::Edel;

/// The program's name, found on the `PATH`.
const NAME: &str = "edel-shell-ui";

/// A run shorter than this counts as a failure to start.
const QUICK: Duration = Duration::from_secs(10);

/// Quick failures in a row before it is given up on.
const TRIES: u32 = 5;

/// How long after an end it starts again.
const AGAIN: Duration = Duration::from_millis(500);

/// What the compositor knows of shell-ui.
#[derive(Default)]
pub struct ShellUi {
    started: bool,
    /// When the running one started.
    since: Option<Instant>,
    /// Quick failures in a row.
    failures: u32,
}

/// Starts shell-ui the first time the desktop is on screen, unless the
/// session runs one program instead (the greeter).
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel, wayland: &str) {
    if state.shell_ui.started || state.program.is_some() {
        return;
    }
    // Diagnostic only: never start shell-ui.
    if std::env::var_os("EDEL_NO_SHELL_UI").is_some() {
        state.shell_ui.started = true;
        eprintln!("edel-compositor: diagnostic: shell-ui not started");
        return;
    }
    state.shell_ui.started = true;
    spawn(handle, state, wayland.to_string());
}

fn spawn(handle: &LoopHandle<'static, Edel>, state: &mut Edel, wayland: String) {
    let mut command = Command::new(NAME);
    command.env("WAYLAND_DISPLAY", &wayland);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!("edel-compositor: no {NAME} in this image, so no panel");
            return;
        }
        Err(e) => {
            eprintln!("edel-compositor: {NAME} did not start: {e}");
            ended(handle, state, wayland, None);
            return;
        }
    };
    eprintln!("edel-compositor: started {NAME}, pid {}", child.id());
    state.shell_ui.since = Some(Instant::now());
    let (sender, receiver) = channel::channel::<Option<ExitStatus>>();
    let again = handle.clone();
    let inserted = handle.insert_source(receiver, move |event, _, state: &mut Edel| {
        if let Event::Msg(status) = event {
            ended(&again, state, wayland.clone(), status);
        }
    });
    if let Err(e) = inserted {
        eprintln!("edel-compositor: watching {NAME} failed: {e}");
        return;
    }
    let mut child = child;
    let waiter = thread::Builder::new()
        .name("shell-ui".into())
        .spawn(move || {
            let _ = sender.send(child.wait().ok());
        });
    if let Err(e) = waiter {
        eprintln!("edel-compositor: waiting for {NAME} failed: {e}");
    }
}

/// shell-ui ended, or could not start: start it again shortly, unless it
/// keeps failing.
fn ended(
    handle: &LoopHandle<'static, Edel>,
    state: &mut Edel,
    wayland: String,
    status: Option<ExitStatus>,
) {
    let quick = state
        .shell_ui
        .since
        .take()
        .is_none_or(|t| t.elapsed() < QUICK);
    state.shell_ui.failures = if quick {
        state.shell_ui.failures + 1
    } else {
        0
    };
    let how = status.map_or_else(|| "was lost".to_string(), |s| format!("ended ({s})"));
    if state.shell_ui.failures >= TRIES {
        eprintln!(
            "edel-compositor: {NAME} {how}, and failed {TRIES} times in a row within {} s of starting; it is not started again",
            QUICK.as_secs()
        );
        return;
    }
    eprintln!("edel-compositor: {NAME} {how}; starting it again");
    let restart = handle.clone();
    let timer = handle.insert_source(
        Timer::from_duration(AGAIN),
        move |_, _, state: &mut Edel| {
            spawn(&restart, state, wayland.clone());
            TimeoutAction::Drop
        },
    );
    if let Err(e) = timer {
        eprintln!("edel-compositor: starting {NAME} again failed: {e}");
    }
}
