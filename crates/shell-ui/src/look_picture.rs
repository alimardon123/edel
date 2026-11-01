//! `edel-shell-ui --look-picture NAME --out FILE [--width W]` (M5.10c7c): draws
//! one lock look as the lock draws it, with fixed sample values, and writes it
//! as a PNG for Settings' Lock screen page, which shows it on the look's card
//! in place of the drawing it makes itself. Handed over in `run` before any
//! Wayland connection, as the greeter is, so it needs no compositor and no
//! session: only the settings files, the look's own parts and the fonts.
//!
//! The sample is the same for every look: the time 9:41, Sunday 11 October,
//! the person's name (their full name, else "Ali"), their wallpaper and the
//! look's background, two notifications from Mail and Calendar, the music
//! player paused on "Night Drive" by "Aurora", and a battery at 82 percent
//! where the look has one.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use edel::lock_screen;
use edel::settings::LockScreen;
use edel::tokens::{Scheme, Tokens};
use edel::wallpapers::Wallpaper;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::lock::{self, When};
use crate::lock_editor;
use crate::paint::Text;
use crate::{messages, player, quick};

/// The width a picture is written at when `--width` is not given: the size
/// Settings' cards draw at, 16 to 10.
pub const DEFAULT_WIDTH: u32 = 480;

/// The sample's name when the person has none.
const SAMPLE_NAME: &str = "Ali";

/// What `--look-picture` takes from its arguments.
#[derive(Debug, PartialEq)]
struct Args {
    name: String,
    out: PathBuf,
    width: u32,
}

impl Args {
    /// The arguments after `--look-picture`: a name, `--out FILE` and an
    /// optional `--width W`, in any order after the name.
    fn parse(words: &[String]) -> Result<Args> {
        let mut name = None;
        let mut out = None;
        let mut width = DEFAULT_WIDTH;
        let mut rest = words.iter();
        while let Some(word) = rest.next() {
            match word.as_str() {
                "--out" => out = Some(PathBuf::from(rest.next().context("--out takes a file")?)),
                "--width" => {
                    let value = rest.next().context("--width takes a number of pixels")?;
                    width = value.parse().with_context(|| {
                        format!("--width takes a number of pixels, not {value:?}")
                    })?;
                }
                other if name.is_none() && !other.starts_with("--") => {
                    name = Some(other.to_string());
                }
                other => bail!("--look-picture does not take {other:?}"),
            }
        }
        let name = name.context("--look-picture takes a look's name")?;
        let out = out.context("--look-picture takes --out FILE, where the picture is written")?;
        if width == 0 {
            bail!("--width must be at least 1 pixel");
        }
        Ok(Args { name, out, width })
    }
}

/// The looks this machine and the person have: the built-in ones, then the
/// person's own by name (the `looks` the lock settings name).
pub fn known_looks(lock: &LockScreen) -> Vec<String> {
    let built_in = lock_screen::LOOKS.iter().map(|name| name.to_string());
    let own = lock
        .looks
        .iter()
        .flat_map(|looks| looks.keys())
        .filter(|name| !lock_screen::is_built_in(name.as_str()))
        .cloned();
    built_in.chain(own).collect()
}

/// Runs `--look-picture` with the arguments after it (`words`), writing the
/// picture to its file atomically: a `.new` file renamed over the picture, so
/// a reader never sees half of one. An unknown look fails with the looks
/// there are, and the caller exits 1 with the message.
pub fn run(words: &[String]) -> Result<()> {
    let args = Args::parse(words)?;
    let (machine, person) = crate::settings_texts();
    let lock = lock::lock_settings(machine.as_deref(), person.as_deref());
    let known = known_looks(&lock);
    if !known.contains(&args.name) {
        bail!("{}", messages::look_not_known(&args.name, &known));
    }
    let dark = quick::is_dark(machine.as_deref(), person.as_deref());
    let wallpaper = lock::wallpaper_of(machine.as_deref(), person.as_deref(), dark);
    let tokens = scheme_tokens(dark);
    let icon_theme = edel::settings::icon_theme_in(machine.as_deref(), person.as_deref());
    let picture = render(
        &args.name,
        &lock,
        &wallpaper,
        &tokens,
        &icon_theme,
        args.width,
    )
    .context("could not draw the lock look")?;
    write_png(&picture, &args.out)?;
    Ok(())
}

/// The tokens of the scheme `dark` names, as the desktop reads them.
fn scheme_tokens(dark: bool) -> Tokens {
    let scheme = if dark { Scheme::Dark } else { Scheme::Light };
    let (tokens, notes) = edel::tokens::on_this_machine(scheme);
    for note in notes {
        eprintln!("edel-shell-ui: {}: {note}", edel::tokens::PATH);
    }
    tokens
}

/// The screen the look is laid out for before it is scaled down: a 16 to 10
/// screen of 1280 by 800, the size the lock's tests lay out for. The editor's
/// small `THUMB` (960 by 600) is too short for classic's clock, which the
/// layout then leaves out, so a picture of a look is never drawn at it.
pub const SAMPLE_SCREEN: (u32, u32) = (1280, 800);

/// The look `name` drawn at `width` pixels wide, 16 to 10 (`SAMPLE_SCREEN` scaled):
/// `lock` gives its parts and background, as Settings' card applies the look
/// alone, and `wallpaper` what lies behind the glass; `icon_theme` is the
/// app icons' theme (M8.21 part g). None when the picture has no pixels to
/// draw on.
pub fn render(
    name: &str,
    lock: &LockScreen,
    wallpaper: &Wallpaper,
    tokens: &Tokens,
    icon_theme: &str,
    width: u32,
) -> Option<Pixmap> {
    let look = LockScreen {
        look: Some(name.to_string()),
        looks: lock.looks.clone(),
        ..LockScreen::default()
    };
    let applied = lock_screen::applied(&look, false);
    let mut text = Text::load(&tokens.font);
    let mut icons = edel::app_icons::Icons::new(edel::apps::data_dirs(), icon_theme);
    let mut pictures = lock::Pictures::default();
    let mut backdrops = lock::Backdrops::default();
    let view = sample_view(sample_name());
    let playing = sample_player();
    let drawn = lock_editor::thumbnail(
        &view,
        &applied.parts,
        &applied.background,
        applied.dim,
        true,
        tokens,
        &mut text,
        Some(&mut icons),
        &mut pictures,
        &mut backdrops,
        wallpaper,
        SAMPLE_SCREEN,
        Some(&playing),
    )?;
    let height =
        (f64::from(width) * f64::from(SAMPLE_SCREEN.1) / f64::from(SAMPLE_SCREEN.0)).round() as u32;
    let mut out = Pixmap::new(width, height.max(1))?;
    let scale = width as f32 / SAMPLE_SCREEN.0 as f32;
    out.draw_pixmap(
        0,
        0,
        drawn.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        Transform::from_scale(scale, scale),
        None,
    );
    Some(out)
}

/// The person's name for the sample: their full name from the account the
/// lock names (`lock::user`), else "Ali".
fn sample_name() -> String {
    let login = lock::user();
    edel::users::read(None)
        .into_iter()
        .find(|person| person.name == login)
        .map(|person| person.shown().to_string())
        .filter(|shown| !shown.is_empty())
        .unwrap_or_else(|| SAMPLE_NAME.to_string())
}

/// The lock's view for the sample: 9:41 on Sunday 11 October 2026, two
/// unread notifications from Mail and Calendar, and an 82 percent battery.
fn sample_view(name: String) -> lock::View {
    let mut view = lock::View::new("09:41".into(), "Sunday 11 October".into(), name);
    view.when = Some(When {
        hour: 9,
        minute: 41,
        weekday: 7,
        day: 11,
        month: 10,
        year: 2026,
    });
    view.notices = 2;
    view.apps = vec![
        ("Mail".into(), "mail-unread".into()),
        ("Calendar".into(), "x-office-calendar".into()),
    ];
    view.battery = Some((82, false));
    view
}

/// The music player the sample shows: "Night Drive" by "Aurora" from Music,
/// paused.
fn sample_player() -> player::PlayerView {
    player::PlayerView {
        title: "Night Drive".into(),
        artist: "Aurora".into(),
        identity: "Music".into(),
        playing: false,
        can_previous: true,
        can_next: true,
        cover: None,
        app_icon: None,
        position: None,
        length: None,
    }
}

/// Writes `picture` to `out` as a PNG with `edel::files::replace`, which makes
/// the folder when it is missing.
pub(crate) fn write_png(picture: &Pixmap, out: &Path) -> Result<()> {
    let png = picture.encode_png().context("could not make the PNG")?;
    edel::files::replace(out, &png).with_context(|| format!("could not write {}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_arguments_take_a_name_an_out_file_and_a_width() {
        let args = Args::parse(&words(&["classic", "--out", "a.png", "--width", "320"])).unwrap();
        assert_eq!(
            args,
            Args {
                name: "classic".into(),
                out: PathBuf::from("a.png"),
                width: 320,
            }
        );
        let args = Args::parse(&words(&["minimal", "--out", "b.png"])).unwrap();
        assert_eq!(args.width, DEFAULT_WIDTH);
    }

    #[test]
    fn the_arguments_refuse_a_missing_name_or_file() {
        assert!(Args::parse(&words(&["--out", "a.png"])).is_err());
        assert!(Args::parse(&words(&["classic"])).is_err());
        assert!(Args::parse(&words(&["classic", "--out", "a.png", "--width", "x"])).is_err());
    }

    #[test]
    fn the_built_in_looks_are_known_without_settings() {
        let known = known_looks(&LockScreen::default());
        assert_eq!(known, ["classic", "magazine", "minimal"]);
    }

    #[test]
    fn classic_draws_a_picture_of_the_asked_width() {
        let tokens = Tokens::built_in();
        let wallpaper = lock::wallpaper_of(None, None, false);
        let picture = render(
            "classic",
            &LockScreen::default(),
            &wallpaper,
            &tokens,
            edel::settings::ICON_THEME_DEFAULT,
            480,
        )
        .expect("classic draws");
        assert_eq!((picture.width(), picture.height()), (480, 300));
        let middle = picture.pixel(240, 150).expect("a middle pixel");
        assert_ne!(middle.alpha(), 0, "the middle of the picture is drawn");
    }
}
