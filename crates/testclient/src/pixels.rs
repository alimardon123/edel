//! `--pixels N` (roadmap M8.28): the window draws one buffer pixel for
//! every N by N of its own, through wp_viewporter, as a game drawn below
//! the screen's resolution does, in a checkerboard of its colour and black,
//! so a screenshot shows whether the compositor scaled it with whole pixels
//! or blurred it.

use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};

use crate::Client;

/// The viewport of `surface`, which shows its buffer at the window's size.
pub fn bind(
    globals: &GlobalList,
    qh: &QueueHandle<Client>,
    surface: &wl_surface::WlSurface,
) -> Result<WpViewport> {
    let viewporter: WpViewporter = globals.bind(qh, 1..=1, ()).context("no wp_viewporter")?;
    Ok(viewporter.get_viewport(surface, qh, ()))
}

/// The buffer's size for a window of `width` by `height` drawn with
/// `pixels` of its own to each buffer pixel; at least one pixel.
pub fn buffer_size(width: u32, height: u32, pixels: u32) -> (u32, u32) {
    ((width / pixels).max(1), (height / pixels).max(1))
}

/// Fills `canvas`, `width` pixels a row, with `pixel` and black in turn.
pub fn checker(canvas: &mut [u8], width: u32, pixel: [u8; 4]) {
    let black = [0, 0, 0, 0xff];
    for (at, out) in canvas.chunks_exact_mut(4).enumerate() {
        let (x, y) = (at as u32 % width, at as u32 / width);
        out.copy_from_slice(if (x + y) % 2 == 0 { &pixel } else { &black });
    }
}

impl Dispatch<WpViewporter, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for Client {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_size_buffer_is_a_checkerboard() {
        assert_eq!(buffer_size(1280, 800, 2), (640, 400));
        assert_eq!(buffer_size(3, 3, 4), (1, 1));
        let mut canvas = vec![0; 3 * 2 * 4];
        checker(&mut canvas, 3, [1, 2, 3, 0xff]);
        let pixels: Vec<&[u8]> = canvas.chunks_exact(4).collect();
        assert_eq!(pixels[0], [1, 2, 3, 0xff]);
        assert_eq!(pixels[1], [0, 0, 0, 0xff]);
        assert_eq!(pixels[3], [0, 0, 0, 0xff], "the second row starts black");
    }
}
