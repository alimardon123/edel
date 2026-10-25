//! The screenshot an app asked for (M8.21 part 21f), taken by shell-ui
//! itself as the screenshot portal's backend (`portal.rs`). Every screen is
//! copied with wlr-screencopy (the compositor offers version 3, always
//! `Xrgb8888`, in physical pixels and not flipped), the copies are laid side
//! by side in one picture, and that is written as a PNG, mode 0600, into a
//! folder of mode 0700 in the person's runtime directory (`edel::places`).
//! So no other program reads the screens, nothing is written to `/tmp`, and
//! grim is not needed. The runtime folder lives in memory, and a new picture
//! removes the folder's pictures older than a minute, since an app may still
//! read a recent one through the document portal. A copy starts with a
//! request and ends when every screen answers, or after five seconds; nothing
//! runs between requests.

use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use smithay_client_toolkit::reexports::calloop::RegistrationToken;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_output::WlOutput,
    wl_shm,
};
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle, WEnum};
use smithay_client_toolkit::reexports::protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::raw::RawPool;
use tiny_skia::Pixmap;

use crate::Shell;
use crate::access;
use crate::messages;
use crate::shell_bus::Answer;

/// How long a copy may wait for every screen before it fails (M8.21 21f).
const WAIT: Duration = Duration::from_secs(5);
/// How old another picture in the folder must be before a new one removes it.
const KEEP: Duration = Duration::from_secs(60);

/// The compositor's screen copy manager, version 3, or `None` when it has
/// none: then no app gets a screenshot (M8.21 part 21f).
pub fn bind(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Option<ZwlrScreencopyManagerV1> {
    globals
        .bind(qh, 3..=3, ())
        .inspect_err(|_| {
            eprintln!(
                "edel-shell-ui: the compositor offers no zwlr_screencopy_manager_v1, so apps get no screenshot"
            )
        })
        .ok()
}

/// One screenshot being taken (M8.21 part 21f): its screens' copies, the way
/// back to the portal's call, the file it becomes and the timer that fails it.
pub struct Shot {
    id: u64,
    path: PathBuf,
    answer: Option<Answer>,
    screens: Vec<Screen>,
    timer: Option<RegistrationToken>,
}

/// One screen's copy: its frame, its logical place, what the compositor said
/// of its buffer, and the buffer shell-ui made for it.
struct Screen {
    frame: ZwlrScreencopyFrameV1,
    place: (i32, i32, i32, i32),
    /// The buffer's format, and its width, height and stride.
    format: Option<wl_shm::Format>,
    size: Option<(u32, u32, u32)>,
    buffer: Option<(RawPool, WlBuffer)>,
    ready: bool,
}

/// The frame's user data: which screenshot and which of its screens.
#[derive(Clone, Copy)]
pub struct Part {
    shot: u64,
    screen: usize,
}

/// The user data of the buffers shell-ui makes for a copy; they need no
/// events of their own.
struct ShotBuffer;

impl Shell {
    /// Takes a picture of every screen into `path` (M8.21 part 21f) and
    /// gives `answer` the outcome when the picture is written. With no screen
    /// copy, or no screen, the answer is dropped, which cancels the call.
    pub fn screenshot(&mut self, path: PathBuf, answer: Answer) {
        let Some(manager) = self.screencopy.clone() else {
            eprintln!(
                "edel-shell-ui: {}",
                messages::screenshot_not_saved("the compositor offers no screen copy")
            );
            return;
        };
        let screens: Vec<(WlOutput, (i32, i32, i32, i32))> = self
            .outputs
            .outputs()
            .filter_map(|output| {
                let info = self.outputs.info(&output)?;
                let (x, y) = info.logical_position?;
                let (w, h) = info.logical_size?;
                Some((output, (x, y, w, h)))
            })
            .collect();
        if screens.is_empty() {
            eprintln!(
                "edel-shell-ui: {}",
                messages::screenshot_not_saved("no screen is known to copy")
            );
            return;
        }
        self.next_shot += 1;
        let id = self.next_shot;
        let qh = self.qh.clone();
        let screens: Vec<Screen> = screens
            .iter()
            .enumerate()
            .map(|(index, (output, place))| Screen {
                frame: manager.capture_output(
                    0,
                    output,
                    &qh,
                    Part {
                        shot: id,
                        screen: index,
                    },
                ),
                place: *place,
                format: None,
                size: None,
                buffer: None,
                ready: false,
            })
            .collect();
        let timer = self.handle.insert_source(
            Timer::from_duration(WAIT),
            move |_, _, shell: &mut Shell| {
                // The event loop drops this timer when it returns, so it is not
                // removed again here.
                let waiting = shell.shots.iter_mut().find(|s| s.id == id).map(|shot| {
                    shot.timer = None;
                });
                if waiting.is_some() {
                    fail(shell, id, "a screen did not answer in 5 seconds");
                }
                TimeoutAction::Drop
            },
        );
        let timer = match timer {
            Ok(token) => token,
            Err(e) => {
                eprintln!(
                    "edel-shell-ui: {}",
                    messages::screenshot_not_saved(format!("could not start its timer: {e}"))
                );
                return;
            }
        };
        self.shots.push(Shot {
            id,
            path,
            answer: Some(answer),
            screens,
            timer: Some(timer),
        });
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for Shell {
    /// The manager has no events.
    fn event(
        _: &mut Shell,
        _: &ZwlrScreencopyManagerV1,
        _: zwlr_screencopy_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

impl Dispatch<WlBuffer, ShotBuffer> for Shell {
    /// The buffers' release events are not needed: a buffer is destroyed
    /// with its shot.
    fn event(
        _: &mut Shell,
        _: &WlBuffer,
        _: wl_buffer::Event,
        _: &ShotBuffer,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, Part> for Shell {
    /// A screen's copy: its buffer's size, once it is known the buffer is
    /// made and the copy asked for, and when the compositor says it is ready
    /// the picture is written once every screen is.
    fn event(
        shell: &mut Shell,
        _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        part: &Part,
        _: &Connection,
        qh: &QueueHandle<Shell>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let format = match format {
                    WEnum::Value(f @ (wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888)) => f,
                    _ => return,
                };
                if let Some(screen) = screen_of(&mut shell.shots, *part) {
                    screen.format = Some(format);
                    screen.size = Some((width, height, stride));
                }
            }
            Event::BufferDone => buffer_done(shell, *part, qh),
            Event::Ready { .. } => ready(shell, *part),
            Event::Failed => fail(shell, part.shot, "the compositor could not copy a screen"),
            _ => {}
        }
    }
}

/// The screen a frame's user data names, in the shot that still waits.
fn screen_of(shots: &mut [Shot], part: Part) -> Option<&mut Screen> {
    shots
        .iter_mut()
        .find(|s| s.id == part.shot)?
        .screens
        .get_mut(part.screen)
}

/// The buffer's sizes are all known: a shared buffer is made for the screen
/// and the copy asked for into it. A screen that gave no size fails the shot.
fn buffer_done(shell: &mut Shell, part: Part, qh: &QueueHandle<Shell>) {
    let copied = {
        let Shell { shm, shots, .. } = &mut *shell;
        match screen_of(shots, part) {
            None => return,
            Some(screen) => match (screen.size, screen.format) {
                (Some(size), Some(format)) => start_copy(shm, screen, size, format, qh),
                _ => Err("the compositor gave no buffer for a screen".to_string()),
            },
        }
    };
    if let Err(why) = copied {
        fail(shell, part.shot, why);
    }
}

/// Makes the screen's shared memory and buffer, and asks the compositor to
/// copy the screen into it.
fn start_copy(
    shm: &Shm,
    screen: &mut Screen,
    (width, height, stride): (u32, u32, u32),
    format: wl_shm::Format,
    qh: &QueueHandle<Shell>,
) -> Result<(), String> {
    let len = stride as usize * height as usize;
    let mut pool =
        RawPool::new(len, shm).map_err(|e| format!("could not share memory for a screen: {e}"))?;
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        format,
        ShotBuffer,
        qh,
    );
    screen.frame.copy(&buffer);
    screen.buffer = Some((pool, buffer));
    Ok(())
}

/// A screen's copy is in its buffer. Once every screen of the shot is, the
/// picture is written.
fn ready(shell: &mut Shell, part: Part) {
    let all = {
        let Some(shot) = shell.shots.iter_mut().find(|s| s.id == part.shot) else {
            return;
        };
        if let Some(screen) = shot.screens.get_mut(part.screen) {
            screen.ready = true;
        }
        shot.screens.iter().all(|s| s.ready)
    };
    if all {
        finish(shell, part.shot);
    }
}

/// Gives up a screenshot (M8.21 part 21f): the copies are destroyed, the
/// answer is dropped so the call is cancelled, and why is logged.
fn fail(shell: &mut Shell, id: u64, why: impl std::fmt::Display) {
    let Some(index) = shell.shots.iter().position(|s| s.id == id) else {
        return;
    };
    let mut shot = shell.shots.remove(index);
    if let Some(token) = shot.timer.take() {
        shell.handle.remove(token);
    }
    eprintln!("edel-shell-ui: {}", messages::screenshot_not_saved(why));
    drop(shot.answer.take());
    release(shot.screens);
}

/// Destroys the frames and buffers of a shot's screens; their shared memory
/// goes with the pools.
fn release(screens: Vec<Screen>) {
    for screen in screens {
        screen.frame.destroy();
        if let Some((_, buffer)) = screen.buffer {
            buffer.destroy();
        }
    }
}

/// Writes the picture of a shot whose screens are all copied (M8.21 part
/// 21f), then answers the call: allowed once the file is written, else
/// cancelled, with the reason logged.
fn finish(shell: &mut Shell, id: u64) {
    let Some(index) = shell.shots.iter().position(|s| s.id == id) else {
        return;
    };
    let mut shot = shell.shots.remove(index);
    if let Some(token) = shot.timer.take() {
        shell.handle.remove(token);
    }
    let answer = shot.answer.take();
    // Each screen's pixels are copied out of the shared memory, so the
    // Wayland objects go at once, and the picture is put together, encoded
    // and written on a thread of its own: on large screens that takes long
    // enough to stall the panels and the lock screen (the whole-system
    // review of 2026-10-25).
    let copies: Vec<Copied> = shot
        .screens
        .iter_mut()
        .filter_map(|screen| {
            let place = screen.place;
            let (pool, _) = screen.buffer.as_mut()?;
            let (width, height, stride) = screen.size?;
            Some(Copied {
                place,
                width,
                height,
                stride,
                bytes: pool.mmap().to_vec(),
            })
        })
        .collect();
    release(shot.screens);
    let path = shot.path;
    let spawned = std::thread::Builder::new()
        .name("screenshot".into())
        .spawn(move || write_picture(&path, &copies, answer));
    if let Err(e) = spawned {
        eprintln!("edel-shell-ui: {}", messages::screenshot_not_saved(e));
    }
}

/// One screen's pixels copied out of its shared memory, as [`Picture`]
/// borrows them.
struct Copied {
    place: (i32, i32, i32, i32),
    width: u32,
    height: u32,
    stride: u32,
    bytes: Vec<u8>,
}

/// Puts `copies` together, writes the picture to `path` and answers the
/// call: allowed once the file is written, else cancelled, with the reason
/// logged.
fn write_picture(path: &Path, copies: &[Copied], answer: Option<Answer>) {
    let pictures: Vec<Picture> = copies
        .iter()
        .map(|copy| Picture {
            place: copy.place,
            width: copy.width,
            height: copy.height,
            stride: copy.stride,
            bytes: &copy.bytes,
        })
        .collect();
    let outcome = match compose(&pictures) {
        None => Err("no screen could be copied".to_string()),
        Some(pixmap) => save(path, &pixmap)
            .map(|()| (pixmap.width(), pixmap.height(), pictures.len()))
            .map_err(|e| e.to_string()),
    };
    match outcome {
        Ok((width, height, count)) => {
            eprintln!(
                "edel-shell-ui: screenshot saved, {width}x{height} from {count} screens in the person's runtime folder"
            );
            if let Some(answer) = answer {
                answer.send(access::ALLOW);
            }
        }
        Err(why) => {
            eprintln!("edel-shell-ui: {}", messages::screenshot_not_saved(why));
            drop(answer);
        }
    }
}

/// One screen's pixels for [`compose`]: its logical place, its size in
/// pixels, its stride, and its bytes as the compositor wrote them, 4 bytes a
/// pixel in wl_shm's little-endian XRGB or ARGB order (B, G, R, X).
pub struct Picture<'a> {
    pub place: (i32, i32, i32, i32),
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bytes: &'a [u8],
}

/// Whether a picture's place and bytes hold what [`compose`] reads.
fn covered(p: &Picture) -> bool {
    p.place.2 > 0
        && p.place.3 > 0
        && p.width > 0
        && p.height > 0
        && p.stride as usize >= p.width as usize * 4
        && p.bytes.len() >= p.stride as usize * p.height as usize
}

/// Every screen in one picture, as they lie side by side: the canvas is the
/// screens' logical bounding box at the largest scale among them (a screen's
/// width in pixels over its logical width), and each screen is drawn at its
/// place, scaled to that (bilinear when scaled, nearest at 1:1), opaque.
/// None when there are no pictures or the canvas would be empty.
pub fn compose(pictures: &[Picture]) -> Option<Pixmap> {
    let usable: Vec<&Picture> = pictures.iter().filter(|p| covered(p)).collect();
    let scale = usable
        .iter()
        .map(|p| f64::from(p.width) / f64::from(p.place.2))
        .fold(0.0, f64::max);
    if usable.is_empty() || scale <= 0.0 {
        return None;
    }
    let min_x = usable.iter().map(|p| p.place.0).min()?;
    let min_y = usable.iter().map(|p| p.place.1).min()?;
    let max_x = usable.iter().map(|p| p.place.0 + p.place.2).max()?;
    let max_y = usable.iter().map(|p| p.place.1 + p.place.3).max()?;
    let canvas_w = (f64::from(max_x - min_x) * scale).round() as u32;
    let canvas_h = (f64::from(max_y - min_y) * scale).round() as u32;
    if canvas_w == 0 || canvas_h == 0 {
        return None;
    }
    let mut pixmap = Pixmap::new(canvas_w, canvas_h)?;
    let row = canvas_w as usize;
    let data = pixmap.data_mut();
    for px in data.chunks_exact_mut(4) {
        px.copy_from_slice(&[0, 0, 0, 255]);
    }
    for p in &usable {
        let ox = f64::from(p.place.0 - min_x) * scale;
        let oy = f64::from(p.place.1 - min_y) * scale;
        let ow = f64::from(p.place.2) * scale;
        let oh = f64::from(p.place.3) * scale;
        // Nearest when the screen's pixels are the canvas's, else bilinear.
        let exact = (ow - f64::from(p.width)).abs() < 0.5 && (oh - f64::from(p.height)).abs() < 0.5;
        let x0 = ox.floor().max(0.0) as u32;
        let x1 = ((ox + ow).ceil().min(f64::from(canvas_w))) as u32;
        let y0 = oy.floor().max(0.0) as u32;
        let y1 = ((oy + oh).ceil().min(f64::from(canvas_h))) as u32;
        for cy in y0..y1 {
            for cx in x0..x1 {
                let u = (f64::from(cx) + 0.5 - ox) / ow * f64::from(p.width);
                let v = (f64::from(cy) + 0.5 - oy) / oh * f64::from(p.height);
                let rgb = sample(p, u, v, exact);
                let at = (cy as usize * row + cx as usize) * 4;
                data[at..at + 3].copy_from_slice(&rgb);
            }
        }
    }
    Some(pixmap)
}

/// The red, green and blue of a picture at `(u, v)` in its own pixels, the
/// pixel centres at half steps: the nearest pixel when `exact`, else the
/// four around it, mixed by their distance.
fn sample(picture: &Picture, u: f64, v: f64, exact: bool) -> [u8; 3] {
    let at = |x: i64, y: i64| -> [f64; 3] {
        let x = x.clamp(0, i64::from(picture.width) - 1) as usize;
        let y = y.clamp(0, i64::from(picture.height) - 1) as usize;
        let i = y * picture.stride as usize + x * 4;
        let bgr = &picture.bytes[i..i + 3];
        [f64::from(bgr[2]), f64::from(bgr[1]), f64::from(bgr[0])]
    };
    let rgb = if exact {
        at(u.floor() as i64, v.floor() as i64)
    } else {
        let (fx, fy) = (u - 0.5, v - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = lerp(at(x0, y0), at(x0 + 1, y0), tx);
        let bottom = lerp(at(x0, y0 + 1), at(x0 + 1, y0 + 1), tx);
        lerp(top, bottom, ty)
    };
    rgb.map(|c| c.round().clamp(0.0, 255.0) as u8)
}

/// `a` to `b` by `t`, per channel.
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// Writes `pixmap` as a PNG at `path` (M8.21 part 21f): makes the folder
/// with mode 0700, removes the folder's other `screenshot-*.png` files that
/// are older than a minute, and writes the file with mode 0600, created or
/// truncated.
pub fn save(path: &Path, pixmap: &Pixmap) -> std::io::Result<()> {
    let png = pixmap.encode_png().map_err(std::io::Error::other)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let now = SystemTime::now();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("screenshot-") && name.ends_with(".png")) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age >= KEEP));
        if old {
            std::fs::remove_file(entry.path())?;
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&png)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// The bytes of a `width` by `height` picture, each pixel `[B, G, R]` from
    /// `colour`, in wl_shm's order with the X byte last.
    fn pixels(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [b, g, r] = colour(x, y);
                bytes.extend_from_slice(&[b, g, r, 0]);
            }
        }
        bytes
    }

    /// The red, green, blue and alpha of a pixel of the pixmap.
    fn at(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * pixmap.width() + x) * 4) as usize;
        pixmap.data()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn one_screen_is_copied_pixel_for_pixel() {
        let bytes = pixels(4, 2, |x, y| {
            if (x, y) == (1, 0) {
                [10, 20, 30]
            } else {
                [0, 0, 0]
            }
        });
        let picture = Picture {
            place: (0, 0, 4, 2),
            width: 4,
            height: 2,
            stride: 16,
            bytes: &bytes,
        };
        let pixmap = compose(&[picture]).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (4, 2));
        assert_eq!(at(&pixmap, 1, 0), [30, 20, 10, 255]);
    }

    #[test]
    fn two_screens_sit_side_by_side_at_the_largest_scale() {
        // A 2 by 2 screen at 1:1, and a 4 by 4 screen whose logical size is
        // 2 by 2, so it is at scale 2: the canvas is 4 by 2 logical, 8 by 4.
        let left = pixels(2, 2, |_, _| [1, 2, 3]);
        let right = pixels(4, 4, |_, _| [7, 8, 9]);
        let pictures = [
            Picture {
                place: (0, 0, 2, 2),
                width: 2,
                height: 2,
                stride: 8,
                bytes: &left,
            },
            Picture {
                place: (2, 0, 2, 2),
                width: 4,
                height: 4,
                stride: 16,
                bytes: &right,
            },
        ];
        let pixmap = compose(&pictures).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (8, 4));
        assert_eq!(at(&pixmap, 0, 0), [3, 2, 1, 255]);
        assert_eq!(at(&pixmap, 4, 0), [9, 8, 7, 255]);
        assert_eq!(at(&pixmap, 7, 3), [9, 8, 7, 255]);
    }

    #[test]
    fn no_picture_gives_no_canvas() {
        assert!(compose(&[]).is_none());
    }

    #[test]
    fn a_picture_is_written_private_into_a_private_folder() {
        let dir = std::env::temp_dir()
            .join(format!("edel-shot-new-{}", std::process::id()))
            .join("screenshots");
        let _ = fs::remove_dir_all(dir.parent().unwrap());
        let bytes = pixels(2, 2, |_, _| [1, 2, 3]);
        let picture = Picture {
            place: (0, 0, 2, 2),
            width: 2,
            height: 2,
            stride: 8,
            bytes: &bytes,
        };
        let pixmap = compose(&[picture]).unwrap();
        let file = dir.join("screenshot-1.png");
        save(&file, &pixmap).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&file), 0o600);
        let written = fs::read(&file).unwrap();
        assert_eq!(&written[..4], [0x89, 0x50, 0x4E, 0x47]);
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    /// A new picture removes the folder's pictures older than a minute, and
    /// keeps a recent one and every other file (M8.21 part 21f).
    #[test]
    fn saving_removes_only_the_old_pictures_of_the_folder() {
        let dir = std::env::temp_dir().join(format!("edel-shot-old-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = fs::File::create(dir.join("screenshot-1.png")).unwrap();
        old.set_modified(SystemTime::now() - Duration::from_secs(120))
            .unwrap();
        fs::write(dir.join("screenshot-2.png"), b"recent").unwrap();
        fs::write(dir.join("keep.txt"), b"mine").unwrap();
        let bytes = pixels(2, 2, |_, _| [1, 2, 3]);
        let picture = Picture {
            place: (0, 0, 2, 2),
            width: 2,
            height: 2,
            stride: 8,
            bytes: &bytes,
        };
        let pixmap = compose(&[picture]).unwrap();
        save(&dir.join("screenshot-3.png"), &pixmap).unwrap();
        assert!(
            !dir.join("screenshot-1.png").exists(),
            "an old picture goes"
        );
        assert!(
            dir.join("screenshot-2.png").exists(),
            "a recent picture stays"
        );
        assert!(dir.join("keep.txt").exists(), "another file stays");
        assert!(dir.join("screenshot-3.png").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
