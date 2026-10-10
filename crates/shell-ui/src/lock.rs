//! The lock screen's look and its password check (M5.10), plain data and
//! drawing, tested without a display. As the phone mockup's lock screen
//! (`docs/mockups/phone-quick.jpg`) on every screen: the date over a large
//! clock in the upper third, then the person's initial on an accent disc,
//! their name and a password field, on the backdrop's gradient deepened
//! towards the bottom. The field shows a dot per letter typed, never the
//! letters, and under it what happened ("Wrong password", "Checking").
//! The password is checked by the system's own helper,
//! `edel::places::PASSWORD_CHECKERS` (Linux-PAM's `unix_chkpwd`), which a
//! person's own program may ask about that person's password alone; a
//! wrong one waits two seconds before the next try, so guessing is slow.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use edel::i18n::tr;
use edel::tokens::Tokens;
use tiny_skia::Pixmap;

use crate::paint::{Face, Text, fill, fill_vertical, mix, outline};

/// What the field says under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Nothing typed, or typing.
    Ready,
    /// The helper is checking the password.
    Checking,
    /// The last password was wrong.
    Wrong,
    /// The system has no password helper: nothing can unlock.
    NoChecker,
}

/// What a screen shows.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// `14:06` and `Saturday 3 October`.
    pub time: String,
    pub date: String,
    pub name: String,
    /// How many letters are typed.
    pub dots: usize,
    pub field: Field,
}

/// Where a screen's parts lie, logical pixels from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub date: (f32, f32),
    pub clock: (f32, f32),
    pub clock_size: f32,
    /// The disc's centre and radius.
    pub avatar: (f32, f32, f32),
    pub name: f32,
    /// The field: x, y, width, height.
    pub field: (f32, f32, f32, f32),
    pub note: f32,
}

/// The screen's width below which it is Compact (M5.6c's size classes).
const COMPACT_BELOW: f32 = 600.0;
/// The field's size: as wide as `FIELD_WIDE`, or the screen less 48 px,
/// and touch-sized (44 px) high.
const FIELD_WIDE: f32 = 300.0;
const FIELD_HIGH: f32 = 44.0;
const AVATAR: f32 = 36.0;
const DATE: f32 = 20.0;
const NAME: f32 = 17.0;
const NOTE: f32 = 13.0;

/// The one function that places everything on a screen `w` by `h`.
pub fn layout(w: f32, h: f32) -> Layout {
    let compact = w < COMPACT_BELOW;
    let clock_size = if compact { 72.0 } else { 104.0 };
    let date_y = (h * 0.16).max(24.0);
    let clock_y = date_y + DATE * 1.4;
    let field_w = FIELD_WIDE.min(w - 48.0).max(120.0);
    // The disc, the name and the field stand together below the middle.
    let avatar_y = (h * 0.53).max(clock_y + clock_size * 1.25 + AVATAR + 16.0);
    let name_y = avatar_y + AVATAR + 12.0;
    let field_y = name_y + NAME * 1.25 + 18.0;
    Layout {
        date: (w / 2.0, date_y),
        clock: (w / 2.0, clock_y),
        clock_size,
        avatar: (w / 2.0, avatar_y, AVATAR),
        name: name_y,
        field: ((w - field_w) / 2.0, field_y, field_w, FIELD_HIGH),
        note: field_y + FIELD_HIGH + 12.0,
    }
}

/// Draws `view` at `scale` into `pixmap`, the screen's size times `scale`.
pub fn paint(pixmap: &mut Pixmap, view: &View, tokens: &Tokens, text: &mut Text, scale: f32) {
    let s = scale;
    let (pw, ph) = (pixmap.width() as f32, pixmap.height() as f32);
    let (w, h) = (pw / s, ph / s);
    let top = mix(tokens.backdrop, tokens.backdrop_deep, 0.45);
    // The gradient settles on the deep colour a little above the bottom,
    // which the compositor's cover also shows before this is drawn.
    let settled = (ph * 0.85).round();
    fill_vertical(
        pixmap,
        (0.0, 0.0, pw, settled),
        0.0,
        top,
        tokens.backdrop_deep,
    );
    fill(
        pixmap,
        0.0,
        settled,
        pw,
        ph - settled,
        0.0,
        tokens.backdrop_deep,
    );
    let ink = tokens.backdrop_text;
    let quiet = mix(ink, tokens.backdrop_deep, 0.35);
    let at = layout(w, h);
    centred(
        pixmap,
        text,
        s,
        w,
        &view.date,
        DATE,
        Face::MEDIUM,
        at.date,
        quiet,
    );
    centred(
        pixmap,
        text,
        s,
        w,
        &view.time,
        at.clock_size,
        Face::SEMIBOLD.tabular(),
        at.clock,
        ink,
    );
    // The person: their initial on an accent disc, their name under it.
    let (cx, cy, r) = at.avatar;
    fill(
        pixmap,
        (cx - r) * s,
        (cy - r) * s,
        2.0 * r * s,
        2.0 * r * s,
        r * s,
        tokens.accent,
    );
    let initial: String = view
        .name
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .collect();
    let mut line = text.line_in(&initial, 30.0 * s, Face::SEMIBOLD);
    let x = cx * s - line.width / 2.0;
    text.draw(
        pixmap,
        &mut line,
        x,
        (cy - 30.0 * 0.62) * s,
        tokens.accent_text,
    );
    centred(
        pixmap,
        text,
        s,
        w,
        &view.name,
        NAME,
        Face::SEMIBOLD,
        (cx, at.name),
        ink,
    );
    // The field: a pill, a dot per letter, else the placeholder.
    let (fx, fy, fw, fh) = at.field;
    let wrong = view.field == Field::Wrong;
    fill(
        pixmap,
        fx * s,
        fy * s,
        fw * s,
        fh * s,
        fh / 2.0 * s,
        mix(tokens.backdrop_deep, ink, 0.14),
    );
    let edge = if wrong {
        tokens.title_close_hover
    } else {
        mix(tokens.backdrop_deep, ink, 0.3)
    };
    outline(
        pixmap,
        (fx * s, fy * s, fw * s, fh * s),
        fh / 2.0 * s,
        s,
        edge,
    );
    if view.dots == 0 {
        let mut line = text.fit(tr("Password"), 15.0 * s, (fw - 40.0) * s);
        text.draw(
            pixmap,
            &mut line,
            (fx + 20.0) * s,
            (fy + (fh - 15.0 * 1.25) / 2.0) * s,
            quiet,
        );
    } else {
        // At most as many dots as fit, 14 px apart.
        let most = ((fw - 40.0) / 14.0) as usize;
        for i in 0..view.dots.min(most) {
            let d = 8.0;
            let x = fx + 20.0 + i as f32 * 14.0;
            fill(
                pixmap,
                x * s,
                (fy + (fh - d) / 2.0) * s,
                d * s,
                d * s,
                d / 2.0 * s,
                ink,
            );
        }
    }
    // The close button's red, lifted towards the text so it reads on the
    // dark backdrop.
    let warn = mix(tokens.title_close_hover, ink, 0.3);
    let note = match view.field {
        Field::Ready => None,
        Field::Checking => Some((tr("Checking"), quiet)),
        Field::Wrong => Some((tr("Wrong password, try again"), warn)),
        Field::NoChecker => Some((tr("This system cannot check passwords"), warn)),
    };
    if let Some((words, colour)) = note {
        centred(
            pixmap,
            text,
            s,
            w,
            words,
            NOTE,
            Face::MEDIUM,
            (w / 2.0, at.note),
            colour,
        );
    }
}

/// Draws `words` centred on `x`, its top at `y`, logical pixels on a
/// screen `w` wide drawn at scale `s`.
#[allow(clippy::too_many_arguments)]
fn centred(
    pixmap: &mut Pixmap,
    text: &mut Text,
    s: f32,
    w: f32,
    words: &str,
    size: f32,
    face: Face,
    (x, y): (f32, f32),
    colour: edel::tokens::Colour,
) {
    let mut line = text.fit_in(words, size * s, (w - 32.0) * s, face);
    let left = x * s - line.width / 2.0;
    text.draw(pixmap, &mut line, left, y * s, colour);
}

/// A password being typed: its bytes, wiped when it is dropped or cleared.
#[derive(Default)]
pub struct Password(Vec<u8>);

impl Password {
    pub fn push(&mut self, text: &str) {
        // Long enough for any passphrase, short enough to bound memory.
        if self.0.len() + text.len() <= 1024 {
            self.0.extend_from_slice(text.as_bytes());
        }
    }

    /// Takes the last letter back.
    pub fn pop(&mut self) {
        let Ok(text) = std::str::from_utf8(&self.0) else {
            return self.clear();
        };
        let keep = text.char_indices().last().map_or(0, |(i, _)| i);
        self.0[keep..].fill(0);
        self.0.truncate(keep);
    }

    pub fn clear(&mut self) {
        self.0.fill(0);
        self.0.clear();
    }

    /// How many letters it holds.
    pub fn letters(&self) -> usize {
        std::str::from_utf8(&self.0).map_or(0, |t| t.chars().count())
    }

    /// Takes the bytes out, leaving it empty.
    pub fn take(&mut self) -> Password {
        Password(std::mem::take(&mut self.0))
    }
}

impl Drop for Password {
    fn drop(&mut self) {
        self.clear();
    }
}

/// What the helper said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Right,
    Wrong,
    /// No helper could run: why.
    NoChecker(String),
}

/// The password helper, the first of `PASSWORD_CHECKERS` that exists.
pub fn checker() -> Option<PathBuf> {
    edel::places::PASSWORD_CHECKERS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// How long a wrong password waits before the next try.
pub const WRONG_WAIT: Duration = Duration::from_secs(2);

/// Asks `helper` whether `password` is `user`'s: it reads the password,
/// ended by a NUL byte, on its standard input and exits 0 when it is
/// right. Runs on a thread of its own, never the drawing one; a wrong
/// answer comes back after `WRONG_WAIT`.
pub fn check(helper: &std::path::Path, user: &str, password: Password) -> Checked {
    let child = Command::new(helper)
        .arg(user)
        .arg("nonull")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Checked::NoChecker(format!("{}: {e}", helper.display())),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&password.0);
        let _ = stdin.write_all(&[0]);
    }
    drop(password);
    match child.wait() {
        Ok(status) if status.success() => Checked::Right,
        Ok(_) => {
            std::thread::sleep(WRONG_WAIT);
            Checked::Wrong
        }
        Err(e) => Checked::NoChecker(format!("{}: {e}", helper.display())),
    }
}

/// The person's login name: `USER`, else `LOGNAME`, as greetd and every
/// login set them.
pub fn user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default()
}

/// The time and the date as the lock screen shows them.
pub fn time_and_date(now: &jiff::Zoned) -> (String, String) {
    (
        now.strftime("%H:%M").to_string(),
        now.strftime("%A %-d %B").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parts_stand_in_order_down_the_middle_and_fit_a_phone() {
        for (w, h) in [(1280.0, 800.0), (1024.0, 768.0), (360.0, 740.0)] {
            let at = layout(w, h);
            assert!(at.date.1 < at.clock.1);
            assert!(at.clock.1 + at.clock_size < at.avatar.1 - at.avatar.2);
            assert!(at.avatar.1 + at.avatar.2 < at.name);
            assert!(at.name < at.field.1);
            assert!(at.field.1 + at.field.3 < at.note);
            assert!(at.note + 20.0 < h, "{w}x{h}");
            assert!(at.field.0 >= 24.0 - 0.01 && at.field.0 + at.field.2 <= w - 24.0 + 0.01);
            assert_eq!(at.date.0, w / 2.0);
            assert!(at.field.3 >= 44.0, "touch-sized");
        }
        assert!(layout(360.0, 740.0).clock_size < layout(1280.0, 800.0).clock_size);
    }

    #[test]
    fn a_password_counts_letters_takes_one_back_and_wipes_itself() {
        let mut p = Password::default();
        p.push("pä");
        p.push("ss");
        assert_eq!(p.letters(), 4);
        p.pop();
        assert_eq!(p.letters(), 3);
        assert_eq!(p.0, "päs".as_bytes());
        let taken = p.take();
        assert_eq!(p.letters(), 0);
        assert_eq!(taken.letters(), 3);
        p.push("x");
        p.clear();
        assert!(p.0.is_empty());
    }

    #[test]
    fn a_missing_helper_is_said_and_never_unlocks() {
        let got = check(
            std::path::Path::new("/nonexistent/unix_chkpwd"),
            "ci",
            Password::default(),
        );
        assert!(matches!(got, Checked::NoChecker(_)), "{got:?}");
    }

    #[test]
    fn the_helper_answers_right_by_its_exit_and_reads_the_password() {
        let dir = std::env::temp_dir().join(format!("edel-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("chk");
        // Right when the bytes before the NUL are "open".
        std::fs::write(
            &helper,
            "#!/bin/sh\n[ \"$1\" = ci ] && [ \"$2\" = nonull ] || exit 9\n[ \"$(tr -d '\\000')\" = open ]\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut right = Password::default();
        right.push("open");
        assert_eq!(check(&helper, "ci", right), Checked::Right);
        let mut wrong = Password::default();
        wrong.push("shut");
        let started = std::time::Instant::now();
        assert_eq!(check(&helper, "ci", wrong), Checked::Wrong);
        assert!(started.elapsed() >= WRONG_WAIT);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_screen_draws_with_its_field_and_note() {
        let tokens = Tokens::built_in();
        let mut text = Text::load(&tokens.font);
        let view = View {
            time: "14:06".into(),
            date: "Saturday 3 October".into(),
            name: "ci".into(),
            dots: 5,
            field: Field::Wrong,
        };
        let mut pixmap = Pixmap::new(1280, 800).unwrap();
        paint(&mut pixmap, &view, &tokens, &mut text, 1.0);
        let at = layout(1280.0, 800.0);
        // The bottom is the backdrop's deep colour, the field's middle is
        // not, and the disc is the accent.
        let px = |x: f32, y: f32| pixmap.pixel(x as u32, y as u32).unwrap();
        let deep = px(4.0, 796.0);
        assert!((deep.red() as f32 / 255.0 - tokens.backdrop_deep.r).abs() < 0.02);
        let disc = px(at.avatar.0 - at.avatar.2 + 4.0, at.avatar.1);
        assert!((disc.blue() as f32 / 255.0 - tokens.accent.b).abs() < 0.02);
        if let Ok(dir) = std::env::var("EDEL_LOCK_PNG") {
            pixmap.save_png(format!("{dir}/lock.png")).unwrap();
        }
    }
}
