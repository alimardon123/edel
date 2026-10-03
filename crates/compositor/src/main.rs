//! `edel-compositor`: windows, floating and tiling, title bars, outputs and
//! effects (ADR-002). It draws on the GPU through DRM (`drm.rs`), or, when
//! started inside another desktop for development, in a window there
//! (`winit.rs`).
//!
//!     edel-compositor           run until the window is closed
//!     edel-compositor --bench   run for 5 s, then print the frame telemetry

mod decoration;
mod drm;
mod grabs;
mod input;
mod render;
mod state;
mod statefile;
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
    // In a desktop session (development), a window there; else the GPU.
    let nested = ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()));
    let run = if nested { winit::run } else { drm::run };
    match run(load_tokens(), bench) {
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
