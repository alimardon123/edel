//! The one program a session runs (roadmap M4.7b): with
//! `edel-compositor -- PROGRAM ARG...` the compositor starts it once the
//! desktop is on screen, with the session's Wayland socket and X11 display,
//! and stops when it exits. greetd's greeter session is the compositor
//! running agreety, the text greeter, in foot, until the compositor greets
//! people itself (M5.10): when the greeter logs someone in it exits, the
//! greeter's compositor stops, and greetd starts the person's session.

use std::io;
use std::process::{Command, ExitStatus};
use std::thread;

use smithay::reexports::calloop::LoopHandle;
use smithay::reexports::calloop::channel::{self, Event};

use edel_compositor::messages;

use crate::state::Edel;

pub struct Program {
    argv: Vec<String>,
    started: bool,
}

impl Program {
    pub fn new(argv: Vec<String>) -> Program {
        Program {
            argv,
            started: false,
        }
    }
}

/// Starts the program, once; the session ends when it exits, or at once
/// if it cannot start.
pub fn start(handle: &LoopHandle<'static, Edel>, state: &mut Edel, wayland: &str) {
    let Some(program) = state.program.as_mut() else {
        return;
    };
    if program.started {
        return;
    }
    program.started = true;
    let name = program.argv[0].clone();
    let mut command = Command::new(&name);
    command
        .args(&program.argv[1..])
        .env("WAYLAND_DISPLAY", wayland);
    match &state.x11 {
        Some(x11) => command.env("DISPLAY", format!(":{}", x11.number)),
        None => command.env_remove("DISPLAY"),
    };
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!(
                "edel-compositor: {}",
                messages::program_not_started(&name, e)
            );
            state.signal.stop();
            return;
        }
    };
    if let Err(e) = watch(handle, name.clone(), child) {
        eprintln!("edel-compositor: watching {name} failed: {e}; the session ends");
        state.signal.stop();
    }
}

fn watch(
    handle: &LoopHandle<'static, Edel>,
    name: String,
    mut child: std::process::Child,
) -> io::Result<()> {
    let (sender, receiver) = channel::channel::<io::Result<ExitStatus>>();
    handle
        .insert_source(receiver, move |event, _, state: &mut Edel| {
            if let Event::Msg(status) = event {
                match status {
                    Ok(status) => {
                        eprintln!("edel-compositor: {name} exited: {status}; the session ends")
                    }
                    Err(e) => eprintln!("edel-compositor: {name} was lost: {e}; the session ends"),
                }
                state.signal.stop();
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    thread::Builder::new()
        .name("program".into())
        .spawn(move || {
            let _ = sender.send(child.wait());
        })?;
    Ok(())
}

/// Starts `name` in the session, as Ctrl+Alt+T does with the terminal
/// (M5.13a), with the session's Wayland socket and X11 display; a thread
/// waits for it, so it leaves nothing behind when it exits.
pub fn open(state: &Edel, name: &str) {
    run(state, &[name.to_string()], name);
}

/// Starts `argv`, a program and its arguments, called `name` in what is
/// logged, as [`open`] does: the overview's search starts apps this way
/// (M5.2j-b3), in the person's home folder.
pub fn run(state: &Edel, argv: &[String], name: &str) -> bool {
    let Some((program, args)) = argv.split_first() else {
        return false;
    };
    let mut command = Command::new(program);
    command.args(args).env("WAYLAND_DISPLAY", &state.socket);
    if let Some(home) = std::env::var_os("HOME") {
        command.current_dir(home);
    }
    match &state.x11 {
        Some(x11) => command.env("DISPLAY", format!(":{}", x11.number)),
        None => command.env_remove("DISPLAY"),
    };
    match command.spawn() {
        Ok(mut child) => {
            let waiter = thread::Builder::new()
                .name(format!("wait-{name}"))
                .spawn(move || child.wait());
            if let Err(e) = waiter {
                eprintln!("edel-compositor: waiting for {name} failed: {e}");
            }
            true
        }
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::app_not_started(name, e));
            false
        }
    }
}
