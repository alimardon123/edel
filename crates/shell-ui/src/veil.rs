//! The veil (M8.21 part 21o): while the launcher or the window switcher is
//! open, the screen behind it is blurred and dimmed as the lock screen's
//! picture is, so the card stands out and the eye goes to it. It is one
//! surface over the whole screen in the windows' layer above (Top), under
//! the cards (Overlay), and takes no clicks: the pointer goes on to what
//! lies under it, as before. Its picture is one pixel of the lock's deep
//! backdrop colour, stretched over the screen with wp_viewporter, so it
//! costs four bytes, not a screen's worth. On the Full tier it asks the
//! compositor to blur behind it (ext-background-effect-v1) and dims as the
//! lock dims a blurred picture, `lock_screen::DIM`; without the blur it
//! dims as the lock dims a sharp one, `lock_screen::DIM_SHARP`, which is
//! all the Lite tier draws: a flat dim, no blur, as Lite keeps surfaces.

use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerSurface,
};
use smithay_client_toolkit::shm::slot::SlotPool;

use edel::lock_screen;
use edel::tokens::{Colour, Tokens};

use crate::Shell;

/// The veil's layer-shell namespace.
pub const NAME: &str = "edel-veil";

/// The compositor's wp_viewporter, or `None` when it has none: then no
/// veil is drawn, as a whole screen's buffer would cost more than it gives.
pub fn bind(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Option<WpViewporter> {
    globals
        .bind(qh, 1..=1, ())
        .inspect_err(|_| {
            eprintln!("edel-shell-ui: the compositor offers no wp_viewporter, so no veil")
        })
        .ok()
}

impl Dispatch<WpViewporter, ()> for Shell {
    fn event(
        _: &mut Shell,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for Shell {
    fn event(
        _: &mut Shell,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

/// The veil's colour: the lock's deep backdrop at the lock's dimming, the
/// blurred picture's when `blurred`, else the sharp one's.
pub fn colour(tokens: &Tokens, blurred: bool) -> Colour {
    let dim = if blurred {
        lock_screen::DIM
    } else {
        lock_screen::DIM_SHARP
    };
    Colour {
        a: dim as f32 / 100.0,
        ..tokens.lock_backdrop_deep
    }
}

/// `colour` as one premultiplied pixel in `wl_shm`'s ARGB8888 byte order,
/// blue first.
pub fn pixel(colour: Colour) -> [u8; 4] {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = colour.a.clamp(0.0, 1.0);
    [
        byte(colour.b * a),
        byte(colour.g * a),
        byte(colour.r * a),
        byte(a),
    ]
}

/// The veil's surface and what it needs to draw.
pub struct Veil {
    surface: LayerSurface,
    viewport: WpViewport,
    effect: Option<ExtBackgroundEffectSurfaceV1>,
    pool: SlotPool,
}

impl Drop for Veil {
    fn drop(&mut self) {
        self.viewport.destroy();
        if let Some(effect) = &self.effect {
            effect.destroy();
        }
    }
}

impl Veil {
    /// Whether `surface` is the veil's.
    pub fn is(&self, surface: &wl_surface::WlSurface) -> bool {
        self.surface.wl_surface() == surface
    }
}

impl Shell {
    /// Puts the veil over the screen, unless it is there already or the
    /// compositor cannot stretch a picture.
    pub fn show_veil(&mut self) {
        if self.veil.is_some() {
            return;
        }
        let Some(viewporter) = &self.viewporter else {
            return;
        };
        let Ok(pool) = SlotPool::new(4, &self.shm) else {
            return;
        };
        let surface = self.compositor.create_surface(&self.qh);
        let viewport = viewporter.get_viewport(&surface, &self.qh, ());
        let effect = self
            .backdrops
            .as_ref()
            .map(|manager| manager.get_background_effect(&surface, &self.qh, ()));
        let surface =
            self.layers
                .create_layer_surface(&self.qh, surface, Layer::Top, Some(NAME), None);
        surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        surface.set_exclusive_zone(-1);
        surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&self.compositor) {
            // Empty: every click goes on to what lies under the veil.
            surface
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        surface.commit();
        self.veil = Some(Veil {
            surface,
            viewport,
            effect,
            pool,
        });
    }

    /// Takes the veil away, when neither the launcher nor the switcher is
    /// open.
    pub fn hide_veil(&mut self) {
        if self.menu.is_some() || self.flip.is_some() {
            return;
        }
        if self.veil.take().is_some() {
            eprintln!("edel-shell-ui: veil hidden");
        }
    }

    pub fn is_veil(&self, surface: &wl_surface::WlSurface) -> bool {
        self.veil.as_ref().is_some_and(|v| v.is(surface))
    }

    /// The compositor sized the veil, the whole screen: its one pixel is
    /// stretched over it, the blur asked for behind all of it on Full.
    pub fn veil_configured(&mut self, (w, h): (u32, u32)) {
        let blurred = self.live_blur;
        let colour = colour(&self.tokens, blurred);
        let Some(veil) = &mut self.veil else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        let Ok((buffer, canvas)) = veil.pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        canvas[..4].copy_from_slice(&pixel(colour));
        veil.viewport.set_destination(w as i32, h as i32);
        if let Some(effect) = &veil.effect {
            let region = if blurred {
                Region::new(&self.compositor).ok()
            } else {
                None
            };
            if let Some(region) = &region {
                region.add(0, 0, w as i32, h as i32);
            }
            effect.set_blur_region(region.as_ref().map(|r| r.wl_region()));
        }
        let surface = veil.surface.wl_surface();
        if buffer.attach_to(surface).is_err() {
            return;
        }
        surface.damage_buffer(0, 0, 1, 1);
        veil.surface.commit();
        let how = if blurred { "blurred" } else { "dimmed" };
        eprintln!("edel-shell-ui: veil shown {w}x{h} {how}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_veil_dims_as_the_lock_does() {
        let tokens = Tokens::built_in();
        assert_eq!(colour(&tokens, true).a, lock_screen::DIM as f32 / 100.0);
        assert_eq!(
            colour(&tokens, false).a,
            lock_screen::DIM_SHARP as f32 / 100.0
        );
        let deep = tokens.lock_backdrop_deep;
        let c = colour(&tokens, true);
        assert_eq!((c.r, c.g, c.b), (deep.r, deep.g, deep.b));
    }

    #[test]
    fn its_pixel_is_premultiplied_blue_first() {
        let p = pixel(Colour {
            r: 1.0,
            g: 0.5,
            b: 0.0,
            a: 0.5,
        });
        assert_eq!(p, [0, 64, 128, 128]);
    }
}
