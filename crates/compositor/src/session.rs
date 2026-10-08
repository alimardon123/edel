//! The session's own programs (roadmap M5.7b): what the preset's
//! `[session] start` lists (`edel::presets::Session`), by default the
//! sound system, PipeWire, WirePlumber and the PulseAudio server apps
//! speak to it through. They run for this person only, as the person, so
//! nothing of them runs on a machine nobody is using, and they start once
//! the desktop is on screen, after shell-ui, in the order listed (each
//! needs the one before).
//!
//! Started once, not kept alive: a program that fails within a few
//! seconds of starting is tried again a few times, because WirePlumber
//! started before PipeWire's socket exists fails at once and is right
//! a moment later; one that fails after running a while is left, and the
//! log says so, rather than hiding that sound stopped. They end with the
//! compositor, the kernel sending each SIGTERM when it goes, a crash
//! included, so the next session never meets a second sound system. A
//! change of preset does not start or end them until the next login.

use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Signal, set_parent_process_death_signal};
use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel::{self, Event};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use edel_compositor::messages;

use crate::state::Edel;

/// A failure this soon after starting is tried again.
const QUICK: Duration = Duration::from_secs(5);

/// Runs of one program, the first included, before it is given up on.
const TRIES: u32 = 4;

/// How long after a quick failure it starts again.
const AGAIN: Duration = Duration::from_millis(500);

/// What the compositor knows of the session's programs.
#[derive(Default)]
pub struct Services {
    started: bool,
}

/// Starts the session's programs the first time the desktop is on
/// screen, unless the session runs one program instead (the greeter).
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel) {
    if state.services.started || state.program.is_some() {
        return;
    }
    state.services.started = true;
    for command in state.settings.session_start() {
        spawn(handle, command, 1);
    }
}

fn spawn(handle: &LoopHandle<'static, Edel>, command: String, run: u32) {
    let mut words = command.split_whitespace();
    let Some(name) = words.next().map(str::to_string) else {
        return;
    };
    let mut process = Command::new(&name);
    process.args(words).stdin(Stdio::null());
    // SAFETY: the closure only makes the prctl system call, which is safe
    // between fork and exec. The kernel then ends the program with the
    // thread that started it, the compositor's main thread, where every
    // spawn happens.
    unsafe {
        process.pre_exec(|| {
            set_parent_process_death_signal(Some(Signal::TERM))?;
            Ok(())
        });
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!(
                "edel-compositor: {}",
                messages::session_program_missing(&name)
            );
            return;
        }
        Err(e) => {
            eprintln!(
                "edel-compositor: {}",
                messages::session_program_not_started(&name, e)
            );
            return;
        }
    };
    eprintln!("edel-compositor: started {name}, pid {}", child.id());
    let began = Instant::now();
    let (sender, receiver) = channel::channel::<Option<ExitStatus>>();
    let again = handle.clone();
    let inserted = handle.insert_source(receiver, move |event, _, _: &mut Edel| {
        if let Event::Msg(status) = event {
            ended(&again, command.clone(), run, began, status);
        }
    });
    if let Err(e) = inserted {
        eprintln!("edel-compositor: watching {name} failed: {e}");
        return;
    }
    let waiter = thread::Builder::new()
        .name(format!("session-{name}"))
        .spawn(move || {
            let _ = sender.send(child.wait().ok());
        });
    if let Err(e) = waiter {
        eprintln!("edel-compositor: waiting for {name} failed: {e}");
    }
}

/// A program of the session ended: tried again if it failed right after
/// starting, said so if it ran a while and stopped.
fn ended(
    handle: &LoopHandle<'static, Edel>,
    command: String,
    run: u32,
    began: Instant,
    status: Option<ExitStatus>,
) {
    let name = command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let failed = status.is_none_or(|s| !s.success());
    let how = status.map_or_else(|| "was lost".to_string(), |s| format!("ended ({s})"));
    if !failed {
        eprintln!("edel-compositor: {name} {how}");
        return;
    }
    if began.elapsed() >= QUICK {
        eprintln!(
            "edel-compositor: {}",
            messages::session_program_stopped(&name, &how)
        );
        return;
    }
    if run >= TRIES {
        eprintln!(
            "edel-compositor: {}",
            messages::session_program_given_up(&name, &how, TRIES, QUICK.as_secs())
        );
        return;
    }
    eprintln!("edel-compositor: {name} {how}; starting it again");
    let later = handle.clone();
    let timer = handle.insert_source(Timer::from_duration(AGAIN), move |_, _, _: &mut Edel| {
        spawn(&later, command.clone(), run + 1);
        TimeoutAction::Drop
    });
    if let Err(e) = timer {
        eprintln!("edel-compositor: starting {name} again failed: {e}");
    }
}
