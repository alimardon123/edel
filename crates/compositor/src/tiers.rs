//! The session's effect tier (roadmap M5.11): picked once the renderer is
//! known (`edel_compositor::effects` holds the rules), lowered by the
//! frame-deadline monitor as frames are drawn, logged as `tier=NAME` and
//! written to the state file, where `edel shell tier` reads it.
//! `EDEL_EFFECTS=lite|balanced|full` sets the starting tier instead, for
//! tests.

use std::ffi::CStr;
use std::fs;
use std::os::raw::c_char;
use std::time::Duration;

use smithay::backend::renderer::gles::{GlesRenderer, ffi};
use smithay::output::Output;

use edel_compositor::effects::{Deadline, MISSES, Tier, WINDOW, starting_tier};

use crate::state::Edel;

/// The GL renderer's name, such as `llvmpipe (LLVM 22.1.3, 256 bits)`.
pub fn renderer_name(renderer: &mut GlesRenderer) -> String {
    renderer
        .with_context(|gl| {
            // SAFETY: GL is current inside with_context, and RENDERER is a
            // string GL owns for the context's life, read before it ends.
            let name = unsafe { gl.GetString(ffi::RENDERER) };
            if name.is_null() {
                return String::from("unknown");
            }
            unsafe { CStr::from_ptr(name as *const c_char) }
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|_| String::from("unknown"))
}

/// Whether the machine runs on a battery that is discharging.
fn on_battery() -> bool {
    let Ok(supplies) = fs::read_dir("/sys/class/power_supply") else {
        return false;
    };
    supplies.flatten().any(|supply| {
        let read = |name: &str| fs::read_to_string(supply.path().join(name)).unwrap_or_default();
        read("type").trim() == "Battery" && read("status").trim() == "Discharging"
    })
}

/// The time between two frames on `output`, 60 Hz when it has no mode.
pub fn refresh_interval(output: &Output) -> Duration {
    output
        .current_mode()
        .filter(|mode| mode.refresh > 0)
        .map_or(Duration::from_micros(16_667), |mode| {
            Duration::from_secs_f64(1000.0 / f64::from(mode.refresh))
        })
}

impl Edel {
    /// Sets the starting tier from the renderer, the battery or
    /// `EDEL_EFFECTS`, and says which.
    pub fn start_effects(&mut self, renderer: &str) {
        let forced = std::env::var("EDEL_EFFECTS")
            .ok()
            .and_then(|name| Tier::parse(&name));
        let battery = on_battery();
        let tier = forced.unwrap_or_else(|| starting_tier(renderer, battery));
        let why = match forced {
            Some(_) => String::from("EDEL_EFFECTS"),
            None if battery => format!("renderer {renderer}, on battery"),
            None => format!("renderer {renderer}"),
        };
        eprintln!("edel-compositor: tier={tier} ({why})");
        self.deadline = Deadline::new(tier);
        self.state_changed();
    }

    /// A frame took `took` to draw on a screen refreshing every
    /// `interval`; the tier drops when too many miss.
    pub fn frame_drawn(&mut self, took: Duration, interval: Duration) {
        if let Some(tier) = self.deadline.frame(took, interval) {
            eprintln!(
                "edel-compositor: tier={tier} after {MISSES} of {WINDOW} frames missed the screen's refresh"
            );
            self.state_changed();
            self.restyle();
        }
    }
}
