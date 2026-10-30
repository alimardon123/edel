//! Screens and their scale (roadmap M4.6): each output gets the scale
//! `displays.NAME.scale` gives it in the settings file, or the one worked out
//! from its size when it appeared (`layout::auto_scale`), and a change in
//! the file applies at once. A real screen is turned as
//! `displays.NAME.rotation` says (M9.8), and laid out at its turned size;
//! a development window is never turned. Windows hear the scale through
//! `wl_surface.preferred_buffer_scale` and `wp_fractional_scale_v1`, and
//! draw at it with `wp_viewporter`, so text stays sharp at 1.25 or 1.5.

use std::cell::Cell;

use smithay::desktop::Window;
use smithay::output::{Output, Scale};
use smithay::utils::Transform;
use smithay::wayland::compositor::send_surface_state;
use smithay::wayland::fractional_scale::{FractionalScaleHandler, with_fractional_scale};

use edel_compositor::layout::{snap_scale, turned};

use crate::state::Edel;

/// The scale an output gets when the settings file names none.
struct AutoScale(Cell<f64>);

/// Remembers the scale `output`'s size gives it, for when the settings file
/// names none.
pub fn set_auto_scale(output: &Output, scale: f64) {
    output
        .user_data()
        .insert_if_missing(|| AutoScale(Cell::new(1.0)));
    if let Some(auto) = output.user_data().get::<AutoScale>() {
        auto.0.set(snap_scale(scale));
    }
}

/// The scale `output`'s size gives it when the settings file names none,
/// which the state file reports so Settings knows what a choice of it
/// would change (M5.7a).
pub fn auto_scale(output: &Output) -> f64 {
    output
        .user_data()
        .get::<AutoScale>()
        .map_or(1.0, |auto| auto.0.get())
}

/// How far a real screen is turned (M9.8); a development window has none,
/// as its own transform only flips GL's rows.
struct Turn(Cell<Transform>);

/// Marks `output`, a real screen, as one the settings file may turn.
pub fn turnable(output: &Output) {
    output
        .user_data()
        .insert_if_missing(|| Turn(Cell::new(Transform::Normal)));
}

/// How far `output` is turned: what absolute input, which reports the
/// glass's own way up, is turned by (`input.rs`).
pub fn turn(output: &Output) -> Transform {
    output
        .user_data()
        .get::<Turn>()
        .map_or(Transform::Normal, |turn| turn.0.get())
}

impl Edel {
    /// Gives every output the scale the settings, or its size, say, and
    /// turns every real screen as they say; returns whether any changed,
    /// so the windows are laid out again.
    pub fn apply_scales_and_turns(&mut self) -> bool {
        let mut changed = false;
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        for output in outputs {
            let wanted = self
                .settings
                .outputs
                .get(&output.name())
                .and_then(|o| o.scale)
                .map_or_else(|| auto_scale(&output), snap_scale);
            if (output.current_scale().fractional_scale() - wanted).abs() > 1e-9 {
                output.change_current_state(None, None, Some(Scale::Fractional(wanted)), None);
                eprintln!("edel-compositor: output {} scale {wanted}", output.name());
                changed = true;
            }
            if let Some(turn) = output.user_data().get::<Turn>() {
                let wanted = turned(
                    self.settings
                        .outputs
                        .get(&output.name())
                        .and_then(|o| o.rotation),
                );
                if turn.0.replace(wanted) != wanted {
                    output.change_current_state(None, Some(wanted), None, None);
                    eprintln!(
                        "edel-compositor: output {} turned {} degrees",
                        output.name(),
                        wanted.degrees()
                    );
                    changed = true;
                }
            }
        }
        changed
    }

    /// Tells `window`'s surfaces the scale to draw at: its screen's.
    pub fn send_scale(&self, window: &Window) {
        let Some(output) = self.space.outputs().next() else {
            return;
        };
        let scale = output.current_scale();
        let transform = output.current_transform();
        window.with_surfaces(|surface, data| {
            send_surface_state(surface, data, scale.integer_scale(), transform);
            with_fractional_scale(data, |fractional| {
                fractional.set_preferred_scale(scale.fractional_scale());
            });
        });
    }
}

impl FractionalScaleHandler for Edel {}
