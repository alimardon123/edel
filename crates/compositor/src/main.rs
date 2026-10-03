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

mod decoration;
mod drm;
mod grabs;
mod input;
mod layers;
mod outputs;
mod pointer;
mod program;
mod render;
mod state;
mod statefile;
mod tiers;
mod watch;
mod winit;
mod xwayland;

use std::process::ExitCode;

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
    let run = if nested { winit::run } else { drm::run };
    match run(load_tokens(), bench, program) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("edel-compositor: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// The image's tokens if it has them, else the built-in ones; anything
/// skipped is reported, never fatal (ADR-008).
fn load_tokens() -> Tokens {
    let Ok(text) = std::fs::read_to_string(tokens::PATH) else {
        return Tokens::built_in();
    };
    let (tokens, notes) = Tokens::read(&text);
    for note in notes {
        eprintln!("edel-compositor: {}: {note}", tokens::PATH);
    }
    tokens
}
