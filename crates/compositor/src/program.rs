//! The one program a session runs (roadmap M4.7b): with
//! `edel-compositor -- PROGRAM ARG...` the compositor starts it once the
//! desktop is on screen, with the session's Wayland socket and X11 display,
//! and stops when it exits. greetd's greeter session is the compositor
//! running shell-ui's greeter (`edel-shell-ui --greeter`, M5.10b): when
//! the greeter logs someone in it exits, the greeter's compositor stops,
//! and greetd starts the person's session.
//!
//! A game session (M8.26, `edel-compositor --game -- PROGRAM ARG...`, the
//! gaming add-on's login choice) is a person's session, its log and its
//! own programs (PipeWire) included, with a shell-ui of no panels
//! (`edel-shell-ui --game`, which keeps the lock screen and the idle
//! ladder), no startup apps and no welcome: PROGRAM, Steam's Big Picture,
//! runs until it exits, which
//! ends the session and brings back the login screen; every window opens
//! fullscreen and game mode is on (`games.rs`).

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
    /// A game session's program (M8.26), not the greeter.
    pub game: bool,
}

impl Program {
    pub fn new(argv: Vec<String>, game: bool) -> Program {
        Program {
            argv,
            started: false,
            game,
        }
    }
}

impl Edel {
    /// Whether the session runs one program instead of a person's desktop:
    /// the greeter. A game session (M8.26) is a person's, with a shell-ui of
    /// no panels.
    pub fn greeter(&self) -> bool {
        self.program.as_ref().is_some_and(|p| !p.game)
    }

    /// Whether this is a game session (M8.26).
    pub fn game_session(&self) -> bool {
        self.program.as_ref().is_some_and(|p| p.game)
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
    if program.game {
        eprintln!(
            "edel-compositor: a game session: {}, every window fullscreen, game mode on, shell-ui with no panels",
            program.argv.join(" ")
        );
    }
    let mut command = Command::new(&name);
    command
        .args(&program.argv[1..])
        .envs(state.cursors.env())
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

/// Starts `name` in the session, as Super+T does with the terminal
/// (M5.13a), with the session's Wayland socket and X11 display; a thread
/// waits for it, so it leaves nothing behind when it exits.
pub fn open(state: &Edel, name: &str) {
    run(state, &[name.to_string()], name);
}

/// Opens the default app of `role` (M6.10), as the shortcut for it asks: the
/// app `[default_apps]` names when it is installed, else the first app of
/// its category. With none, `fallback` starts (foot for the terminal), or a
/// line says which key to set.
pub fn open_default(state: &Edel, role: &str, fallback: Option<&str>) {
    let apps = edel::apps::read_all(&edel::apps::dirs());
    let chosen = state.settings.default_apps.get(role).map(String::as_str);
    match edel::apps::default_app(&apps, role, chosen) {
        Some(app) => {
            eprintln!("edel-compositor: opened default {role} app {}", app.id);
            run(state, &edel::apps::command(app), &app.id);
        }
        None => match fallback {
            Some(name) => open(state, name),
            None => eprintln!("edel-compositor: {}", messages::no_default_app(role)),
        },
    }
}

/// Starts `argv`, a program and its arguments, called `name` in what is
/// logged, as [`open`] does: the overview's search starts apps this way
/// (M5.2j-b3), in the person's home folder.
pub fn run(state: &Edel, argv: &[String], name: &str) -> bool {
    let Some((program, args)) = argv.split_first() else {
        return false;
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(state.cursors.env())
        .env("WAYLAND_DISPLAY", &state.socket);
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
