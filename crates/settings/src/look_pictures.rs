//! The lock screen's look pictures (M5.10c7c): each look's card on the Lock
//! screen page shows the lock as shell-ui draws it, not Settings' own drawing
//! of it. A picture is made by `edel-shell-ui --look-picture NAME --out FILE`
//! (found on PATH, and when shell-ui is not there nothing is drawn), kept in
//! the person's folder `places::look_pictures_dir` under a name made from what
//! it shows, and swapped in when it is there. Until then, and where no picture
//! can be made, the card keeps Settings' drawing, so the page never waits for
//! a render. Renders run in the background, two at a time, and the folder
//! keeps only the pictures the page shows.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gtk::{gio, glib};

use edel::lock_screen::{self, Background, Placed};
use edel::places;
use edel::tokens::Tokens;
use edel::wallpapers;

use gtk::prelude::*;

/// The width a picture is drawn at, in pixels: the size the cards are
/// drawn at, 16 to 10 (shell-ui scales what it draws to it).
pub const WIDTH: u32 = 480;

/// How many pictures are drawn at once (M5.10c7c: a page of looks does not
/// start a render for each).
const AT_ONCE: usize = 2;

/// What a picture shows besides its look: the wallpaper that applies, the
/// scheme and the tokens (the colours, accent and shape the desktop uses).
/// A change to any of them makes a new picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inputs {
    wallpaper: String,
    dark: bool,
    tokens: String,
}

impl Inputs {
    /// The inputs the files and the tokens give now: the wallpaper that
    /// applies in the scheme the files choose, as shell-ui reads them
    /// (`wallpapers::value_for`, `quick::is_dark`).
    pub fn of(light: Option<&str>, night: Option<&str>, dark: bool, tokens: &Tokens) -> Inputs {
        Inputs {
            wallpaper: wallpapers::value_for(light, night, dark)
                .unwrap_or_default()
                .to_string(),
            dark,
            tokens: format!("{tokens:?}"),
        }
    }
}

/// The name of a look's picture (without `.png`): a hash of its parts, its
/// background, its dim and the inputs, so a picture is kept for as long as it would
/// draw the same. FNV-1a, which does not change between runs.
pub fn key(parts: &[Placed], background: &Background, dim: u32, inputs: &Inputs) -> String {
    let text = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        lock_screen::widgets_value(parts).unwrap_or_default(),
        lock_screen::background_value(background),
        dim,
        inputs.wallpaper,
        inputs.dark,
        inputs.tokens,
        WIDTH,
    );
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The file a picture of `key` is kept in, when the person has a folder.
fn file_of(key: &str) -> Option<PathBuf> {
    places::look_pictures_dir().map(|dir| dir.join(format!("{key}.png")))
}

/// Shows the picture of look `name` under `key` over `frame`, an overlay whose
/// main child is Settings' drawing of the same size (it sets the size, the
/// picture covers it): at once when its file is there, else in the
/// background, laid over the drawing when shell-ui has drawn it. `frame` may
/// go before that, which the swap then ignores.
pub fn want(frame: &gtk::Overlay, name: &str, key: &str) {
    // The frame remembers the key it waits for, so a picture drawn late for
    // a look that is no longer shown is left out.
    frame.set_widget_name(key);
    let Some(file) = file_of(key) else {
        return;
    };
    if file.exists() {
        show(frame, &file, name);
        return;
    }
    let weak = frame.downgrade();
    let (name_owned, key_owned) = (name.to_string(), key.to_string());
    queue(Job {
        name: name.to_string(),
        file,
        shown: Box::new(move |file| {
            if let Some(frame) = weak.upgrade() {
                if frame.widget_name() == key_owned {
                    show(&frame, file, &name_owned);
                }
            }
        }),
    });
}

/// Takes the picture off `frame`, so the drawing shows again, and forgets the
/// key it waited for. A look that is no longer the one shown never keeps the
/// picture of the one before.
pub fn show_drawn(frame: &gtk::Overlay) {
    frame.set_widget_name("");
    clear_pictures(frame);
}

/// Removes the pictures laid over `frame`.
fn clear_pictures(frame: &gtk::Overlay) {
    let mut child = frame.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Ok(picture) = widget.downcast::<gtk::Picture>() {
            frame.remove_overlay(&picture);
        }
    }
}

/// Lays the picture at `file` over `frame`, covering the drawing, cut to the
/// card's corners. The drawing sets the size: it follows the card's width at
/// the screen's 16 to 10 (`lock`'s `follow_width`).
fn show(frame: &gtk::Overlay, file: &Path, name: &str) {
    clear_pictures(frame);
    let picture = gtk::Picture::for_filename(file);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_target(false);
    // GTK clips a child to its rounded corners only when the widget asks
    // for it; the corners come from the class's `border-radius`.
    picture.set_overflow(gtk::Overflow::Hidden);
    picture.add_css_class("edel-look-shot");
    frame.add_overlay(&picture);
    eprintln!("edel-settings: lock_screen picture {name} ready");
}

/// Deletes the pictures in the folder that no shown look uses, `keep` being
/// their names without `.png`. Nothing is kept of a folder that cannot be
/// read, and a file that cannot be removed stays.
pub fn prune(keep: &[String]) {
    let Some(dir) = places::look_pictures_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(stem) = file.to_str().and_then(|f| f.strip_suffix(".png")) else {
            continue;
        };
        if !keep.iter().any(|k| k == stem) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// One picture to draw: the look's name, the file it goes to and what shows
/// it once the file is there.
struct Job {
    name: String,
    file: PathBuf,
    shown: Box<dyn FnOnce(&Path)>,
}

#[derive(Default)]
struct Queue {
    running: usize,
    waiting: VecDeque<Job>,
}

thread_local! {
    static QUEUE: RefCell<Queue> = RefCell::new(Queue::default());
}

/// Queues `job` and starts what the queue allows.
fn queue(job: Job) {
    QUEUE.with(|q| q.borrow_mut().waiting.push_back(job));
    start();
}

/// Starts jobs while fewer than `AT_ONCE` run: each draws on a thread of
/// GIO's, and when it ends its file is shown, if it is there.
fn start() {
    loop {
        let Some(job) = QUEUE.with(|q| {
            let mut q = q.borrow_mut();
            if q.running >= AT_ONCE {
                return None;
            }
            let job = q.waiting.pop_front()?;
            q.running += 1;
            Some(job)
        }) else {
            return;
        };
        glib::spawn_future_local(async move {
            let Job { name, file, shown } = job;
            let (drawn_name, drawn_file) = (name.clone(), file.clone());
            let _ = gio::spawn_blocking(move || draw(&drawn_name, &drawn_file)).await;
            QUEUE.with(|q| q.borrow_mut().running -= 1);
            if file.exists() {
                shown(&file);
            }
            start();
        });
    }
}

/// Runs `edel-shell-ui --look-picture NAME --out FILE` and waits for it. A
/// shell-ui not on PATH draws nothing, silently; a failure says why, once.
fn draw(name: &str, file: &Path) {
    let run = Command::new("edel-shell-ui")
        .args(["--look-picture", name, "--out"])
        .arg(file)
        .args(["--width", &WIDTH.to_string()])
        .stdin(Stdio::null())
        .output();
    match run {
        Ok(out) if out.status.success() => {}
        Ok(out) => eprintln!(
            "edel-settings: lock_screen picture {name} not drawn: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(why) if why.kind() == ErrorKind::NotFound => {}
        Err(why) => eprintln!("edel-settings: lock_screen picture {name} not drawn: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_named_by_what_it_shows() {
        let parts = lock_screen::classic(false);
        let inputs = Inputs::default();
        let blurred = key(&parts, &Background::Blurred, 30, &inputs);
        assert_eq!(blurred, key(&parts, &Background::Blurred, 30, &inputs));
        assert_eq!(blurred.len(), 16, "a hash of 16 hex digits");
        assert_ne!(blurred, key(&parts, &Background::Sharp, 30, &inputs));
        assert_ne!(blurred, key(&parts, &Background::Blurred, 50, &inputs));
        let dark = Inputs {
            dark: true,
            ..Inputs::default()
        };
        assert_ne!(blurred, key(&parts, &Background::Blurred, 30, &dark));
        let other_wallpaper = Inputs {
            wallpaper: "hills".into(),
            ..Inputs::default()
        };
        assert_ne!(
            blurred,
            key(&parts, &Background::Blurred, 30, &other_wallpaper)
        );
    }

    #[test]
    fn the_inputs_read_the_scheme_the_files_choose() {
        let tokens = Tokens::built_in();
        assert!(Inputs::of(None, None, true, &tokens).dark);
        assert!(!Inputs::of(None, None, false, &tokens).dark);
        assert!(!Inputs::of(None, None, false, &tokens).dark);
    }
}
