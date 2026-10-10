//! `edel-compositor`: windows, floating and tiling, title bars, outputs and
//! effects (ADR-002). It draws on the GPU through DRM (`drm.rs`), or, when
//! started inside another desktop for development, in a window there
//! (`winit.rs`).
//!
//!     edel-compositor                  run until the window is closed
//!     edel-compositor --bench          run for 5 s, then print the frame
//!                                      telemetry
//!     edel-compositor -- PROGRAM ARG   run PROGRAM once the desktop is on
//!                                      screen, until it exits (M4.7b)

mod animate;
mod decoration;
mod dmabuf;
mod drm;
mod edelshell;
mod everyday;
mod extworkspace;
mod fullscreen;
mod glance;
mod glance_look;
mod glance_search;
mod grabs;
mod input;
mod keyboard;
mod layers;
mod outputs;
mod pointer;
mod program;
mod render;
mod sandbox;
mod session;
mod shellui;
mod shortcuts;
mod state;
mod statefile;
mod switcher;
mod tiers;
mod toplevels;
mod watch;
mod winit;
mod workspaces;
mod xwayland;

use std::process::ExitCode;

use edel_compositor::messages;
use edel_compositor::tokens::{self, Tokens};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (bench, program) = match args.as_slice() {
        [] => (false, None),
        [arg] if arg == "--bench" => (true, None),
        [dashes, argv @ ..] if dashes == "--" && !argv.is_empty() => {
            (false, Some(program::Program::new(argv.to_vec())))
        }
        _ => {
            eprintln!("usage: edel-compositor [--bench | -- PROGRAM [ARG...]]");
            return ExitCode::from(2);
        }
    };
    // In a desktop session (development), a window there; else the GPU.
    let nested = ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()));
    // A person's session on the screen, not the greeter's or a nested
    // one, writes to the person's log (M5.28a).
    let session = !nested && !bench && program.is_none();
    if session {
        session_log();
    }
    let run = if nested { winit::run } else { drm::run };
    match run(load_tokens(), bench, program) {
        Ok(()) => {
            if session {
                eprintln!("{}", edel::session_log::ENDED);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("edel-compositor: {}", messages::stopped(format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}

/// The image's tokens if it has them, else the built-in ones, in the
/// release's default scheme, light (M5.12a); the settings files may then
/// change it, as any later change does. Anything skipped is reported,
/// never fatal (ADR-008).
/// Sends what the compositor and everything it starts write, on standard
/// output and error, to the person's session log, after moving the log
/// of the session before aside; says where on the way.
fn session_log() {
    let Some(dir) = edel::places::person_state_dir() else {
        eprintln!(
            "edel-compositor: warning: neither XDG_STATE_HOME nor HOME is set, so this session keeps no log; its lines go to standard error"
        );
        return;
    };
    let (path, file, badly) = match edel::session_log::start(&dir) {
        Ok(started) => started,
        Err(e) => {
            eprintln!(
                "edel-compositor: warning: could not start the session's log in {}: {e}; its lines go to standard error",
                dir.display()
            );
            return;
        }
    };
    eprintln!("edel-compositor: the session's log is {}", path.display());
    if let Err(e) = rustix::stdio::dup2_stdout(&file).and(rustix::stdio::dup2_stderr(&file)) {
        eprintln!(
            "edel-compositor: warning: could not write to {}: {e}; lines go to standard error",
            path.display()
        );
        return;
    }
    eprintln!("edel-compositor: the session started");
    if badly {
        eprintln!(
            "edel-compositor: warning: the session before ended without closing; its log is {}",
            dir.join(edel::places::SESSION_LOG_BEFORE).display()
        );
    }
}

fn load_tokens() -> Tokens {
    let (tokens, notes) = tokens::load(tokens::Scheme::default());
    for note in notes {
        eprintln!("edel-compositor: {}: {note}", tokens::PATH);
    }
    tokens
}
