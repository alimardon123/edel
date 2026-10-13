//! Input methods (roadmap M6.8): text-input-v3 and input-method-v2, so
//! fcitx5 types for apps when `region.input_method` asks for it
//! (`session.rs` starts and stops it).
//!
//! - Text input follows the keyboard focus by itself (smithay): an app
//!   that asks for text gets the input method, and foot and GTK 4 ask
//!   for it on Wayland, so no toolkit module is needed.
//! - The keyboard goes to the input method while it holds a grab. The
//!   key path in `input.rs` runs the shortcuts first, so Super keys still
//!   act while an input method types, and everything else reaches the
//!   grab through `KeyboardHandle::input`.
//! - fcitx5 uses input-method-v2 only when a virtual keyboard is offered
//!   too (zwp_virtual_keyboard_v1), which it types the keys it does not
//!   take through; both are smithay's.
//! - The input method and virtual keyboard globals are hidden from
//!   sandboxed clients (`sandbox::shell_may`), so a Flatpak app never
//!   becomes an input method or types as one: only programs the session
//!   started may.
//! - Its candidate list is a popup: `PopupManager` tracks it under the
//!   window it types into, and the frame draws it above that window
//!   (`popup_elements`, called from `render.rs`). Only input method popups
//!   are drawn that way; an app's xdg popups are not drawn by `render.rs`
//!   (they were not before this step either).
//! - smithay has no hook when an input method binds or grabs the
//!   keyboard, so the log says `input method popup shown` when the first
//!   popup appears, which is the line CI waits for.

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::element::surface::{
    WaylandSurfaceRenderElement, render_elements_from_surface_tree,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::delegate_input_method_manager;
use smithay::delegate_text_input_manager;
use smithay::delegate_virtual_keyboard_manager;
use smithay::desktop::{PopupKind, PopupManager};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Physical, Point, Rectangle};
use smithay::wayland::input_method::{InputMethodHandler, PopupSurface};

use crate::state::Edel;

impl InputMethodHandler for Edel {
    /// The input method shows a popup. Its parent is already set when
    /// smithay calls this, so the popup joins the parent's popup tree.
    fn new_popup(&mut self, surface: PopupSurface) {
        let _ = self.popups.track_popup(surface.into());
        eprintln!("edel-compositor: input method popup shown");
        self.dirty = true;
    }

    /// The popup goes: out of its parent's tree, as smithay's own popups do.
    fn dismiss_popup(&mut self, surface: PopupSurface) {
        let parent = surface.get_parent().map(|parent| parent.surface.clone());
        if let Some(parent) = parent {
            let _ = PopupManager::dismiss_popup(&parent, &PopupKind::from(surface));
        }
        self.dirty = true;
    }

    fn popup_repositioned(&mut self, _surface: PopupSurface) {
        self.dirty = true;
    }

    /// Where the window the popup belongs to is, for smithay's placement.
    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        self.window_of(parent)
            .map(|window| window.geometry())
            .unwrap_or_default()
    }
}

/// The input method popups above `parent`, as surface elements to draw
/// with `parent`'s origin at `origin` on a screen of `scale`, at `alpha`.
/// A popup sits where the input method put it, relative to its parent's
/// origin.
pub fn popup_elements(
    renderer: &mut GlesRenderer,
    parent: &WlSurface,
    origin: Point<i32, Physical>,
    scale: f64,
    alpha: f32,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    PopupManager::popups_for_surface(parent)
        .filter(|(popup, _)| matches!(popup, PopupKind::InputMethod(_)))
        .flat_map(|(popup, at)| {
            let location = origin + at.to_physical_precise_round(scale);
            render_elements_from_surface_tree(
                renderer,
                popup.wl_surface(),
                location,
                scale,
                alpha,
                Kind::Unspecified,
            )
        })
        .collect()
}

delegate_text_input_manager!(Edel);
delegate_input_method_manager!(Edel);
delegate_virtual_keyboard_manager!(Edel);
