//! What the launcher and the window switcher share (M5.3b, M5.3c): a
//! surface of their own above everything, made when shown and let go when
//! hidden, so shell-ui's idle memory stays the panel's; and a card in the
//! menus' colour and corners holding rows of text, the chosen one lit.
//! Sizes come from the tokens, so a change there reaches both. Drawing is
//! plain and tested without a display.

use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::{wl_shm, wl_surface};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Layer, LayerSurface};
use smithay_client_toolkit::shm::slot::SlotPool;
use tiny_skia::Pixmap;

use edel::tokens::Tokens;

use crate::Shell;
use crate::paint::{self, Text, fill, lit};

/// The room round a card's rows, and text's from a row's left, in logical
/// pixels.
pub const PAD: f32 = 8.0;
pub const INSET: f32 = 12.0;

/// A popup's surface, its own buffers and what it last drew, `V`.
pub struct Popup<V> {
    pub surface: LayerSurface,
    pool: SlotPool,
    /// Its size in logical pixels, and its screen's scale.
    size: (u32, u32),
    scale: u32,
    /// Configured, so it may draw.
    ready: bool,
    /// Drawn and not yet shown by the compositor: the next drawing waits
    /// for its frame, as the panel's does.
    waiting: bool,
    drawn: Option<(V, u32)>,
}

impl<V: Clone + PartialEq> Popup<V> {
    /// A surface named `name`, `size` big, with buffers for up to `most`
    /// at `scale`; the caller places it and commits.
    pub fn new(
        shell: &Shell,
        name: &'static str,
        size: (u32, u32),
        most: (u32, u32),
        scale: u32,
    ) -> Option<Popup<V>> {
        let bytes = (most.0 * most.1 * 4 * scale * scale) as usize;
        let pool = SlotPool::new(bytes, &shell.shm)
            .inspect_err(|e| eprintln!("edel-shell-ui: no memory for the {name}: {e}"))
            .ok()?;
        let surface = shell.compositor.create_surface(&shell.qh);
        let surface =
            shell
                .layers
                .create_layer_surface(&shell.qh, surface, Layer::Overlay, Some(name), None);
        surface.set_size(size.0, size.1);
        Some(Popup {
            surface,
            pool,
            size,
            scale,
            ready: false,
            waiting: false,
            drawn: None,
        })
    }

    /// A new size, drawn once the compositor agrees.
    pub fn resize(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            self.ready = false;
            self.surface.set_size(size.0, size.1);
            self.surface.commit();
        }
    }

    pub fn configured(&mut self) {
        self.ready = true;
    }

    /// The compositor showed the last drawing.
    pub fn framed(&mut self) {
        self.waiting = false;
    }

    pub fn set_scale(&mut self, factor: i32) {
        self.scale = factor.clamp(1, 4) as u32;
    }

    pub fn scale(&self) -> f32 {
        self.scale as f32
    }

    pub fn is(&self, surface: &wl_surface::WlSurface) -> bool {
        self.surface.wl_surface() == surface
    }

    /// A clear pixmap to draw `view` into, unless it may not draw yet or
    /// already shows it.
    pub fn canvas(&self, view: &V) -> Option<Pixmap> {
        if !self.ready
            || self.waiting
            || self
                .drawn
                .as_ref()
                .is_some_and(|(v, s)| v == view && *s == self.scale)
        {
            return None;
        }
        Pixmap::new(self.size.0 * self.scale, self.size.1 * self.scale)
    }

    /// Shows `pixmap`, which draws `view`; whether it is the first time.
    pub fn show(&mut self, view: V, pixmap: &Pixmap, name: &str, qh: &QueueHandle<Shell>) -> bool {
        let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
        let (buffer, canvas) = match self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        {
            Ok(made) => made,
            Err(e) => {
                eprintln!("edel-shell-ui: drawing the {name} failed: {e}");
                return false;
            }
        };
        paint::to_argb(pixmap, canvas);
        let surface = self.surface.wl_surface();
        surface.set_buffer_scale(self.scale as i32);
        surface.damage_buffer(0, 0, w, h);
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("edel-shell-ui: drawing the {name} failed: {e}");
            return false;
        }
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.surface.commit();
        self.waiting = true;
        let first = self.drawn.is_none();
        self.drawn = Some((view, self.scale));
        first
    }
}

/// Clears `pixmap` and draws the card, the whole of it, at scale `s`.
pub fn card(pixmap: &mut Pixmap, tokens: &Tokens, s: f32) {
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    fill(
        pixmap,
        0.0,
        0.0,
        w,
        h,
        tokens.radius as f32 * s,
        tokens.panel,
    );
}

/// Where text `size` pixels high sits to be in the middle of a row from
/// `top`, `height` high, in logical pixels, at scale `s`.
pub fn middle(top: f32, height: f32, size: f32, s: f32) -> f32 {
    (top + height / 2.0) * s - size * 0.625
}

/// Draws `labels` as rows from `top` logical pixels down, the card's width
/// less its padding, `chosen` lit, in `ink`, at scale `s`; without `text`,
/// only the light.
pub fn rows<'a>(
    pixmap: &mut Pixmap,
    tokens: &Tokens,
    text: Option<&mut Text>,
    labels: impl Iterator<Item = &'a str>,
    top: f32,
    chosen: Option<usize>,
    s: f32,
) {
    let w = pixmap.width() as f32 / s;
    let row = tokens.row as f32;
    if let Some(i) = chosen {
        let y = top + i as f32 * row;
        let r = tokens.radius_control as f32 * s;
        fill(
            pixmap,
            PAD * s,
            y * s,
            (w - 2.0 * PAD) * s,
            row * s,
            r,
            lit(tokens),
        );
    }
    let Some(text) = text else {
        return;
    };
    let size = tokens.panel_text_size as f32 * s;
    let room = (w - 2.0 * (PAD + INSET)) * s;
    for (i, label) in labels.enumerate() {
        let mut line = text.fit(label, size, room);
        let y = middle(top + i as f32 * row, row, size, s);
        text.draw(pixmap, &mut line, (PAD + INSET) * s, y, tokens.panel_text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = pixmap.pixel(x, y).unwrap().demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn the_card_is_round_and_the_chosen_row_is_lit_at_any_scale() {
        let tokens = Tokens::built_in();
        let row = tokens.row;
        for s in [1, 2] {
            let mut pixmap = Pixmap::new(200 * s, (2 * row + 16) * s).unwrap();
            card(&mut pixmap, &tokens, s as f32);
            rows(
                &mut pixmap,
                &tokens,
                None,
                ["a", "b"].into_iter(),
                PAD,
                Some(1),
                s as f32,
            );
            assert_eq!(pixel(&pixmap, 0, 0)[3], 0, "a round corner");
            let back = tokens.panel.bytes();
            let at = |i: u32| {
                pixel(
                    &pixmap,
                    (PAD as u32 + 4) * s,
                    (PAD as u32 + i * row + row / 2) * s,
                )
            };
            assert_eq!(at(0), back);
            assert_ne!(at(1), back);
        }
    }
}
