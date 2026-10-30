//! Sharp scaling for games (roadmap M8.28). A game drawn below the
//! screen's resolution and shown at it through wp_viewporter, as games and
//! Xwayland's emulated modes do, is scaled pixel for pixel when it is shown
//! a whole number of times larger than it is drawn
//! (`edel_compositor::game::whole_upscale`), so its pixels stay sharp
//! instead of blurred; at any other size, and for every other window, the
//! picture is scaled smoothly as before. The game's surface keeps its
//! place and size, so its input needs nothing, and the display hardware
//! still takes it straight to the screen where its plane can scale
//! (`drm.rs`'s `scanout=direct`). Logged `game on NAME scaled sharply,
//! N times` and `pictures on NAME scaled smoothly again` when that changes.

use std::cell::Cell;

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::with_renderer_surface_state;
use smithay::backend::renderer::{Renderer, TextureFilter};
use smithay::desktop::Window;
use smithay::output::Output;

use edel_compositor::game::whole_upscale;

use crate::state::Edel;

/// How many times larger the game on a screen is shown, when it is
/// scaled sharply.
struct Sharp(Cell<Option<u32>>);

/// How many times larger `window`'s own surface is shown on a screen at
/// `scale` than its buffer, when that is a whole number of at least 2.
fn upscaled(window: &Window, scale: f64) -> Option<u32> {
    let surface = window.toplevel()?.wl_surface().clone();
    with_renderer_surface_state(&surface, |state| {
        let view = state.view()?;
        let pixels = f64::from(state.buffer_scale());
        whole_upscale(
            (view.src.size.w * pixels, view.src.size.h * pixels),
            (f64::from(view.dst.w) * scale, f64::from(view.dst.h) * scale),
        )
    })
    .flatten()
}

impl Edel {
    /// Sets how `renderer` scales pictures up for the frame `output` is
    /// about to draw at `scale`: pixel for pixel while a game plays there
    /// drawn a whole number of times smaller than it is shown, else
    /// smoothly.
    pub fn scale_for(&self, renderer: &mut GlesRenderer, output: &Output, scale: f64) {
        let times = self
            .playing_on(output)
            .and_then(|window| upscaled(window, scale));
        let filter = match times {
            Some(_) => TextureFilter::Nearest,
            None => TextureFilter::Linear,
        };
        if let Err(e) = renderer.upscale_filter(filter) {
            eprintln!("edel-compositor: choosing how pictures are scaled failed: {e}");
        }
        output
            .user_data()
            .insert_if_missing(|| Sharp(Cell::new(None)));
        let Some(sharp) = output.user_data().get::<Sharp>() else {
            return;
        };
        if sharp.0.replace(times) == times {
            return;
        }
        match times {
            Some(times) => eprintln!(
                "edel-compositor: game on {} scaled sharply, {times} times",
                output.name()
            ),
            None => eprintln!(
                "edel-compositor: pictures on {} scaled smoothly again",
                output.name()
            ),
        }
    }
}
