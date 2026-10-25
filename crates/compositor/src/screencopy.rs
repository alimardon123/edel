//! wlr-screencopy-unstable-v1, version 3 (roadmap M6.3): copies of an
//! output or a part of it into a client's shared memory buffer. The
//! portals' Screenshot and ScreenCast (xdg-desktop-portal-wlr, feature
//! `capture`) copy the screen this way, and so does anything else that
//! asks the compositor for a picture.
//!
//! A client asks for a frame of an output (or a region of it, in the
//! output's logical coordinates), is told its buffer's format and size,
//! then copies into a buffer of its own. The copy is filled at the next
//! frame the output draws: `fill_copies` renders the same elements the
//! screen just drew into an offscreen texture, reads it back and copies
//! the rows the client's region covers into its buffer, honouring the
//! buffer's stride. A frame never blocks drawing; the readback costs one
//! extra render only while a client asks. Only shared memory is offered
//! (no linux-dmabuf), as the protocol allows.

use std::time::Duration;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::{Element as _, Kind};
use smithay::backend::renderer::gles::{GlesMapping, GlesRenderer, GlesTexture};
use smithay::backend::renderer::{Bind, ExportMem, Offscreen, TextureMapping};
use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_frame_v1::{
    self, Flags, ZwlrScreencopyFrameV1,
};
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_manager_v1::{
    self, ZwlrScreencopyManagerV1,
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::utils::{Buffer, Clock, Monotonic, Physical, Rectangle, Size, Transform};
use smithay::wayland::shm::{BufferAccessError, with_buffer_contents_mut};

use crate::render::Drawn;
use crate::state::Edel;

/// The protocol's version: frames say when their buffer types are done
/// and carry damage for `copy_with_damage`.
const VERSION: u32 = 3;

/// A rectangle of pixels: where a copy starts in the output's buffer and
/// how big it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The part of the output's buffer a region of its logical coordinates
/// covers: scaled by the buffer's size over the output's, its edges
/// rounded outwards, and clamped to the buffer. `None` when nothing of
/// the region is on the output.
pub fn buffer_area(region: Area, logical: (i32, i32), buffer: (i32, i32)) -> Option<Area> {
    if logical.0 <= 0 || logical.1 <= 0 || buffer.0 <= 0 || buffer.1 <= 0 {
        return None;
    }
    let sx = f64::from(buffer.0) / f64::from(logical.0);
    let sy = f64::from(buffer.1) / f64::from(logical.1);
    let x0 = (f64::from(region.x) * sx)
        .floor()
        .clamp(0.0, f64::from(buffer.0));
    let y0 = (f64::from(region.y) * sy)
        .floor()
        .clamp(0.0, f64::from(buffer.1));
    let x1 = (f64::from(region.x + region.w) * sx)
        .ceil()
        .clamp(0.0, f64::from(buffer.0));
    let y1 = (f64::from(region.y + region.h) * sy)
        .ceil()
        .clamp(0.0, f64::from(buffer.1));
    let (x0, y0, x1, y1) = (x0 as i32, y0 as i32, x1 as i32, y1 as i32);
    (x1 > x0 && y1 > y0).then(|| Area {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Copies `area` of `pixels`, a whole output's rows of `width` pixels
/// of four bytes each, into `dst` from `offset`, with `stride` bytes
/// from one row of the client's buffer to the next. When `bottom_up`,
/// the rows of `pixels` run from the bottom up, as a GL framebuffer's
/// readback is laid out, so the output's top row comes out first; a
/// mapping that is already top down is copied as it is. Bytes between the copied rows, such
/// as a stride wider than the copy, are left as they were. `false` when
/// a row would fall outside either buffer.
pub fn copy_area(
    pixels: &[u8],
    width: usize,
    bottom_up: bool,
    area: Area,
    dst: &mut [u8],
    offset: usize,
    stride: usize,
) -> bool {
    let row = width * 4;
    if row == 0 || pixels.len() % row != 0 {
        return false;
    }
    let height = pixels.len() / row;
    let (Ok(ax), Ok(ay), Ok(aw), Ok(ah)) = (
        usize::try_from(area.x),
        usize::try_from(area.y),
        usize::try_from(area.w),
        usize::try_from(area.h),
    ) else {
        return false;
    };
    if ax + aw > width || ay + ah > height || aw == 0 || ah == 0 {
        return false;
    }
    let bytes = aw * 4;
    if stride < bytes {
        return false;
    }
    for r in 0..ah {
        let src_row = if bottom_up {
            height - 1 - (ay + r)
        } else {
            ay + r
        };
        let from = src_row * row + ax * 4;
        let to = offset + r * stride;
        let (Some(src), Some(dst)) = (pixels.get(from..from + bytes), dst.get_mut(to..to + bytes))
        else {
            return false;
        };
        dst.copy_from_slice(src);
    }
    true
}

/// A frame a client asked for and what it has done since (`pending_copies`).
pub struct Pending {
    frame: ZwlrScreencopyFrameV1,
    output: Output,
    /// Where the copy comes from, in the output's buffer pixels.
    area: Area,
    /// The whole buffer's size when the frame was made, to notice a
    /// change of mode before filling.
    buffer: (i32, i32),
    /// Whether the cursor is in the copy (`overlay_cursor`).
    overlay_cursor: bool,
    /// The copy the client asked for, once it did.
    copy: Option<Copy>,
}

struct Copy {
    buffer: WlBuffer,
    with_damage: bool,
}

/// The name a log line gives a client: its process, as the kernel names it.
fn client_name(client: &Client, display: &DisplayHandle) -> String {
    match client.get_credentials(display) {
        Ok(credentials) => format!("pid {}", credentials.pid),
        Err(_) => "a client".into(),
    }
}

/// Renders `elements` into `texture`, the output's offscreen texture at
/// its buffer size, and starts reading it back; the mapping says how GL
/// laid the rows out (`flipped`). The cursor is left out unless `cursor`
/// says otherwise.
fn read_output(
    renderer: &mut GlesRenderer,
    texture: &mut GlesTexture,
    elements: &[Drawn],
    size: (i32, i32),
    scale: f64,
    background: [f32; 4],
    cursor: bool,
) -> Result<GlesMapping, String> {
    let buffer: Size<i32, Buffer> = size.into();
    let kept: Vec<&Drawn> = elements
        .iter()
        .filter(|e| cursor || !matches!(e.kind(), Kind::Cursor))
        .collect();
    let mut damage =
        OutputDamageTracker::new(Size::<i32, Physical>::from(size), scale, Transform::Normal);
    let mut target = renderer
        .bind(texture)
        .map_err(|e| format!("binding the copy's texture: {e}"))?;
    damage
        .render_output(renderer, &mut target, 0, &kept, background)
        .map_err(|e| format!("drawing the copy: {e}"))?;
    renderer
        .copy_framebuffer(&target, Rectangle::from_size(buffer), Fourcc::Argb8888)
        .map_err(|e| format!("reading the copy back: {e}"))
}

impl Edel {
    /// Fills the copies `output` has been asked for, from what `elements`
    /// drew on it this frame. A frame that cannot be filled is told
    /// `failed`; frames whose client went away are dropped. Called by
    /// both backends right after they render an output.
    pub fn fill_copies(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        elements: &[Drawn],
    ) {
        self.pending_copies.retain(|p| p.frame.is_alive());
        let (mine, rest): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.pending_copies)
            .into_iter()
            .partition(|p| p.copy.is_some() && &p.output == output);
        self.pending_copies = rest;
        if mine.is_empty() {
            return;
        }
        let Some(mode) = output.current_mode() else {
            for p in &mine {
                p.frame.failed();
            }
            return;
        };
        let size = (mode.size.w, mode.size.h);
        let scale = output.current_scale().fractional_scale();
        let background = self.tokens.background.rgba();
        // The texture is kept for the next frame at the same size, so a
        // screen shared in a call allocates nothing per frame (the
        // whole-system review of 2026-10-25).
        let name = output.name();
        let mut texture = match self.copy_textures.remove(&name) {
            Some((kept, texture)) if kept == size => Ok(texture),
            _ => renderer
                .create_buffer(Fourcc::Argb8888, Size::<i32, Buffer>::from(size))
                .map_err(|e| format!("no texture for the copy: {e}")),
        };
        // One readback serves every frame the same way; the cursor is
        // read again only when a frame asked for it and another did not.
        let mut readbacks: Vec<(bool, Result<GlesMapping, String>)> = Vec::new();
        let now: Duration = Clock::<Monotonic>::new().now().into();
        for pending in mine {
            let cursor = pending.overlay_cursor;
            if !readbacks.iter().any(|(c, _)| *c == cursor) {
                let read = match &mut texture {
                    Ok(texture) => {
                        read_output(renderer, texture, elements, size, scale, background, cursor)
                    }
                    Err(why) => Err(why.clone()),
                };
                readbacks.push((cursor, read));
            }
            let read = &readbacks
                .iter()
                .find(|(c, _)| *c == cursor)
                .expect("the readback was just made")
                .1;
            let Some(copy) = pending.copy.as_ref() else {
                continue;
            };
            // The client's rows are copied straight from the mapped
            // readback, with no copy of the whole screen in between.
            let read = match read {
                Ok(_) if pending.buffer != size => {
                    Err("the output changed size since the frame was made".to_string())
                }
                Ok(mapping) => renderer
                    .map_texture(mapping)
                    .map(|pixels| (pixels, mapping.flipped()))
                    .map_err(|e| format!("mapping the copy: {e}")),
                Err(why) => Err(why.clone()),
            };
            let (pixels, flipped) = match read {
                Ok(read) => read,
                Err(why) => {
                    eprintln!(
                        "edel-compositor: {}",
                        crate::messages::copy_failed(&pending.output.name(), &why)
                    );
                    pending.frame.failed();
                    continue;
                }
            };
            let area = pending.area;
            let copied = with_buffer_contents_mut(&copy.buffer, |ptr, len, data| {
                // SAFETY: `ptr` and `len` are the client's shared memory
                // pool, mapped for this call only; nothing else in this
                // process holds a reference into it while the rows are
                // written, and the offsets are checked by `copy_area`.
                let dst = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
                let (Ok(offset), Ok(stride)) =
                    (usize::try_from(data.offset), usize::try_from(data.stride))
                else {
                    return false;
                };
                copy_area(pixels, size.0 as usize, !flipped, area, dst, offset, stride)
            });
            if !matches!(copied, Ok(true)) {
                pending.frame.failed();
                continue;
            }
            let pending_frame = &pending.frame;
            pending_frame.flags(Flags::empty());
            if copy.with_damage {
                pending_frame.damage(0, 0, area.w as u32, area.h as u32);
            }
            let secs = now.as_secs();
            pending_frame.ready((secs >> 32) as u32, secs as u32, now.subsec_nanos());
            self.log_copy(pending_frame, &pending.output, size);
        }
        if let Ok(texture) = texture {
            self.copy_textures.insert(name, (size, texture));
        }
    }

    /// Logs a client's first copy of the session, once per client (for
    /// CI's desktop test), not once per frame.
    fn log_copy(&mut self, frame: &ZwlrScreencopyFrameV1, output: &Output, size: (i32, i32)) {
        let Some(client) = frame.client() else {
            return;
        };
        if self.copy_clients.contains(&client.id()) {
            return;
        }
        self.copy_clients.push(client.id());
        let name = client_name(&client, &self.display);
        eprintln!(
            "edel-compositor: screen copied for {name} on {} {}x{}",
            output.name(),
            size.0,
            size.1
        );
    }

    /// Drops the frame a client destroyed before its copy was filled.
    fn forget_copy(&mut self, frame: &ZwlrScreencopyFrameV1) {
        self.pending_copies.retain(|p| &p.frame != frame);
    }
}

impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for Edel {
    /// Hidden from sandboxed apps, as the shell's protocols are (M5.22): a
    /// sandboxed app copies the screen through the portal, which asks.
    fn can_view(client: Client, _: &()) -> bool {
        crate::sandbox::shell_may(&client)
    }

    fn bind(
        _: &mut Edel,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Edel>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        _: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Edel>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame,
                overlay_cursor,
                output,
            } => state.capture(data_init, frame, overlay_cursor, &output, None),
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame,
                overlay_cursor,
                output,
                x,
                y,
                width,
                height,
            } => state.capture(
                data_init,
                frame,
                overlay_cursor,
                &output,
                Some(Area {
                    x,
                    y,
                    w: width,
                    h: height,
                }),
            ),
            _ => {}
        }
    }
}

impl Edel {
    /// Makes a frame for `output` (or `region` of it), tells the client
    /// its buffer, and keeps it until the client copies into it.
    fn capture(
        &mut self,
        data_init: &mut DataInit<'_, Edel>,
        frame: New<ZwlrScreencopyFrameV1>,
        overlay_cursor: i32,
        output: &WlOutput,
        region: Option<Area>,
    ) {
        let frame = data_init.init(frame, ());
        let made = Output::from_resource(output).and_then(|output| {
            let mode = output.current_mode()?;
            let buffer = (mode.size.w, mode.size.h);
            let logical = self.space.output_geometry(&output)?;
            let area = match region {
                None => Area {
                    x: 0,
                    y: 0,
                    w: buffer.0,
                    h: buffer.1,
                },
                Some(region) => buffer_area(region, (logical.size.w, logical.size.h), buffer)?,
            };
            Some((output, buffer, area))
        });
        let Some((output, buffer, area)) = made else {
            frame.failed();
            return;
        };
        frame.buffer(
            wl_shm::Format::Xrgb8888,
            area.w as u32,
            area.h as u32,
            (area.w * 4) as u32,
        );
        if frame.version() >= 3 {
            frame.buffer_done();
        }
        self.pending_copies.push(Pending {
            frame,
            output,
            area,
            buffer,
            overlay_cursor: overlay_cursor != 0,
            copy: None,
        });
    }

    /// The client's copy into `buffer`: checked now, filled at the next
    /// frame of its output.
    fn copy_into(&mut self, frame: &ZwlrScreencopyFrameV1, buffer: WlBuffer, with_damage: bool) {
        let Some(pending) = self.pending_copies.iter_mut().find(|p| &p.frame == frame) else {
            return;
        };
        if pending.copy.is_some() {
            frame.post_error(
                zwlr_screencopy_frame_v1::Error::AlreadyUsed,
                "this frame already copied a buffer",
            );
            return;
        }
        let want = (pending.area.w, pending.area.h);
        let fits = with_buffer_contents_mut(&buffer, |_, _, data| {
            let format = data.format;
            (format, data.width, data.height, data.stride)
        });
        let ok = match fits {
            Ok((format, w, h, stride)) => {
                matches!(format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888)
                    && (w, h) == want
                    && i64::from(stride) >= i64::from(w) * 4
            }
            Err(BufferAccessError::NotManaged) | Err(_) => false,
        };
        if !ok {
            frame.failed();
            self.forget_copy(frame);
            return;
        }
        pending.copy = Some(Copy {
            buffer,
            with_damage,
        });
        self.dirty = true;
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for Edel {
    fn request(
        state: &mut Edel,
        _: &Client,
        resource: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Edel>,
    ) {
        match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => {
                state.copy_into(resource, buffer, false);
            }
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => {
                state.copy_into(resource, buffer, true);
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Edel, _: ClientId, resource: &ZwlrScreencopyFrameV1, _: &()) {
        state.forget_copy(resource);
    }
}

/// Offers `zwlr_screencopy_manager_v1` at version 3 (M6.3).
pub fn create_global(display: &DisplayHandle) {
    display.create_global::<Edel, ZwlrScreencopyManagerV1, ()>(VERSION, ());
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Area = Area {
        x: 0,
        y: 0,
        w: 1280,
        h: 800,
    };

    #[test]
    fn a_whole_output_is_its_whole_buffer() {
        assert_eq!(buffer_area(FULL, (1280, 800), (1280, 800)), Some(FULL));
    }

    #[test]
    fn a_region_scales_to_the_buffer_pixels() {
        // A 2x output: logical 640x400 is 1280x800 pixels.
        let region = Area {
            x: 10,
            y: 20,
            w: 100,
            h: 50,
        };
        assert_eq!(
            buffer_area(region, (640, 400), (1280, 800)),
            Some(Area {
                x: 20,
                y: 40,
                w: 200,
                h: 100,
            })
        );
    }

    #[test]
    fn a_region_is_clamped_to_the_output() {
        let past_the_right_and_bottom = Area {
            x: 1200,
            y: 700,
            w: 200,
            h: 200,
        };
        assert_eq!(
            buffer_area(past_the_right_and_bottom, (1280, 800), (1280, 800)),
            Some(Area {
                x: 1200,
                y: 700,
                w: 80,
                h: 100,
            })
        );
        let before_the_top_left = Area {
            x: -10,
            y: -10,
            w: 30,
            h: 30,
        };
        assert_eq!(
            buffer_area(before_the_top_left, (1280, 800), (1280, 800)),
            Some(Area {
                x: 0,
                y: 0,
                w: 20,
                h: 20,
            })
        );
    }

    #[test]
    fn a_region_off_the_output_is_nothing() {
        let off = Area {
            x: 2000,
            y: 0,
            w: 10,
            h: 10,
        };
        assert_eq!(buffer_area(off, (1280, 800), (1280, 800)), None);
        assert_eq!(buffer_area(FULL, (0, 800), (1280, 800)), None);
    }

    /// Two rows of two pixels, bottom row first as GL reads them: the
    /// output's top row comes out first, and a stride of three pixels
    /// leaves the padding bytes alone.
    #[test]
    fn rows_are_copied_top_first_with_the_clients_stride() {
        let pixel = |v: u8| [v; 4];
        // GL's first row is the bottom one (0xb0, 0xb1); the top one is 0x70, 0x71.
        let pixels: Vec<u8> = [pixel(0xb0), pixel(0xb1), pixel(0x70), pixel(0x71)].concat();
        let mut dst = vec![0xeeu8; 24];
        let whole = Area {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        };
        assert!(copy_area(&pixels, 2, true, whole, &mut dst, 0, 12));
        let expected: Vec<u8> = [
            pixel(0x70).as_slice(),
            pixel(0x71).as_slice(),
            &[0xee; 4],
            pixel(0xb0).as_slice(),
            pixel(0xb1).as_slice(),
            &[0xee; 4],
        ]
        .concat();
        assert_eq!(dst, expected);
    }

    #[test]
    fn a_region_is_cut_from_the_rows_it_covers() {
        let pixel = |v: u8| [v; 4];
        let pixels: Vec<u8> = [pixel(0xb0), pixel(0xb1), pixel(0x70), pixel(0x71)].concat();
        let mut dst = vec![0u8; 4];
        // The top row's right pixel, 0x71, from the bottom-up rows.
        let right = Area {
            x: 1,
            y: 0,
            w: 1,
            h: 1,
        };
        assert!(copy_area(&pixels, 2, true, right, &mut dst, 0, 4));
        assert_eq!(dst, pixel(0x71));
    }

    #[test]
    fn a_copy_that_would_leave_its_buffer_is_refused() {
        let pixels = vec![0u8; 16];
        let mut small = vec![0u8; 4];
        let whole = Area {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        };
        assert!(!copy_area(&pixels, 2, false, whole, &mut small, 0, 8));
        assert!(!copy_area(&pixels, 2, false, whole, &mut [0u8; 16], 0, 4));
    }
}
