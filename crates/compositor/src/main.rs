//! `edel-compositor`: windows, floating and tiling, title bars, outputs and
//! effects (ADR-002). Today it runs in a window of the desktop it is
//! started from (the winit backend, for development); M4.2b adds the
//! virtual GPU and real hardware.
//!
//!     edel-compositor           run until the window is closed
//!     edel-compositor --bench   run for 5 s, then print the frame telemetry

mod state;
mod winit;

use std::process::ExitCode;

use edel_compositor::tokens::{self, Tokens};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bench = match args.as_slice() {
        [] => false,
        [arg] if arg == "--bench" => true,
        _ => {
            eprintln!("usage: edel-compositor [--bench]");
            return ExitCode::from(2);
        }
    };
    match winit::run(load_tokens(), bench) {
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
