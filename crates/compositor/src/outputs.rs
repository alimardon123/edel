//! Screens and their scale (roadmap M4.6): each output gets the scale
//! `displays.NAME.scale` gives it in the settings file, or the one worked out
//! from its size when it appeared (`layout::auto_scale`), and a change in
//! the file applies at once. Windows hear the scale through
//! `wl_surface.preferred_buffer_scale` and `wp_fractional_scale_v1`, and
//! draw at it with `wp_viewporter`, so text stays sharp at 1.25 or 1.5.

use std::cell::Cell;

use smithay::desktop::Window;
use smithay::output::{Output, Scale};
use smithay::wayland::compositor::send_surface_state;
use smithay::wayland::fractional_scale::{FractionalScaleHandler, with_fractional_scale};

use edel_compositor::layout::snap_scale;

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

impl Edel {
    /// Gives every output the scale the settings, or its size, say;
    /// returns whether any changed, so the windows are laid out again.
    pub fn apply_scales(&mut self) -> bool {
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
