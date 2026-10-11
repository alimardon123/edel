//! The Appearance page's Wallpaper group (M5.10c4c): a card per wallpaper
//! in `places::WALLPAPERS_DIR`, its picture the light half and the dark
//! half side by side, then None, Your picture (a file of the person's) and
//! Colour (a colour chosen in the system's dialog). Each choice is one
//! line of the person's settings file, `appearance.wallpaper`, written with
//! `Files::choose_over` so a choice that is what would apply is taken out
//! and never written (ADR-008). What applies is `shown`: the person's or
//! the machine's value, else `release_value`, the default wallpaper when the
//! `wallpapers` feature carries it, else none. The group follows the files
//! through `layout.rs`'s `Ui::update`, as `tray.rs` does, and every GTK call
//! is here or in `widgets.rs` (ADR-004).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use edel::i18n::tr;
use edel::places;
use edel::tokens::{Colour, Tokens};
use edel::wallpapers;

use crate::files::{self, Files};
use crate::style::Theme;
use crate::{rows, widgets};

/// The key this group chooses.
const KEY: &str = "appearance.wallpaper";
/// A card's picture in logical pixels: the size of the pictures' thumbnails.
const WIDTH: i32 = 192;
const HEIGHT: i32 = 120;
/// The keys of the three cards that are not a wallpaper's name, as the
/// log names them.
const NONE_CARD: &str = "none";
const PICTURE_CARD: &str = "picture";
const COLOUR_CARD: &str = "colour";

/// What applies when nobody chose: the default wallpaper when `dir` has
/// it, else none, the background colour alone.
pub fn release_value(dir: &Path) -> String {
    if wallpapers::find(dir, wallpapers::DEFAULT, false).is_some() {
        wallpapers::DEFAULT.to_string()
    } else {
        wallpapers::NONE.to_string()
    }
}

/// The value that applies: the chosen one, else what the release gives.
pub fn shown(chosen: Option<String>, dir: &Path) -> String {
    chosen.unwrap_or_else(|| release_value(dir))
}

/// What a value asks for, as the cards show it.
#[derive(Debug, PartialEq)]
enum Kind {
    Pair(String),
    None,
    Picture(PathBuf),
    Colour(String),
}

fn kind(value: &str) -> Kind {
    if value == wallpapers::NONE {
        Kind::None
    } else if value.starts_with('#') {
        Kind::Colour(value.to_string())
    } else if value.starts_with('/') {
        Kind::Picture(PathBuf::from(value))
    } else {
        Kind::Pair(value.to_string())
    }
}

/// A value as its line names it: a wallpaper's name with a capital, None,
/// the file name of the person's picture, or the colour.
fn title_of(value: &str) -> String {
    match kind(value) {
        Kind::Pair(name) => files::title(&name),
        Kind::None => tr("None").to_string(),
        Kind::Picture(path) => path.file_name().map_or_else(
            || tr("Your picture").to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        Kind::Colour(hex) => hex,
    }
}

/// The Wallpaper group: a card per wallpaper, None, Your picture and
/// Colour, all in one group of toggles, with the group's source line and
/// Reset and Copy as command.
pub struct Card {
    dir: PathBuf,
    theme: Rc<Theme>,
    /// Every card in the order people see them: each wallpaper's name, then
    /// `none`, `picture` and `colour`, each with its badge.
    cards: Vec<(String, gtk::ToggleButton, gtk::Box)>,
    /// How many of `cards` are wallpapers' names.
    pairs: usize,
    /// The colour card's picture and the colour it shows, none for the accent.
    colour_area: gtk::DrawingArea,
    colour_shown: Rc<Cell<Option<Colour>>>,
    source: widgets::Source,
    problem: gtk::Label,
    /// Set while `show` moves a toggle, so that is not a choice.
    quiet: Cell<bool>,
}

impl Card {
    /// Shows the value that applies: its card chosen and badged, the
    /// colour card's colour, and where the value comes from.
    pub fn show(&self, files: &Files) {
        let value = shown(files.chosen(KEY), &self.dir);
        let at = match kind(&value) {
            Kind::Pair(name) => self.cards[..self.pairs]
                .iter()
                .position(|(key, _, _)| *key == name),
            Kind::None => Some(self.pairs),
            Kind::Picture(_) => Some(self.pairs + 1),
            Kind::Colour(_) => Some(self.pairs + 2),
        };
        self.quiet.set(true);
        for (index, (_, button, badge)) in self.cards.iter().enumerate() {
            let on = Some(index) == at;
            button.set_active(on);
            badge.set_visible(on);
        }
        self.quiet.set(false);
        self.colour_shown.set(match kind(&value) {
            Kind::Colour(hex) => Colour::parse(&hex),
            _ => None,
        });
        self.colour_area.queue_draw();
        let source = files.source(KEY);
        self.source
            .label
            .set_label(&rows::describe(&source, &title_of(&value)));
        self.source.reset.set_visible(rows::resettable(&source));
    }

    /// Writes `value` as the choice, then shows what applies now.
    fn write(&self, value: &str) {
        let release = release_value(&self.dir);
        let result = Files::here().choose_over(KEY, value, Some(&release));
        self.report(result);
        self.show(&Files::here());
    }

    /// Shows why a change was not written, or takes an old message away.
    fn report(&self, result: Result<(), String>) {
        match result {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
    }
}

/// The group's card in `content` after its heading (M5.10c4c), with the
/// cards in a flow box built as the presets' are.
pub fn card(content: &gtk::Box, problem: &gtk::Label, theme: &Rc<Theme>) -> Rc<Card> {
    let dir = PathBuf::from(places::WALLPAPERS_DIR);
    let source = widgets::section(content, rows::title(KEY));
    let names = wallpapers::names(&dir);
    let pairs = names.len();

    let colour_shown = Rc::new(Cell::new(None));
    let shown_colour = colour_shown.clone();
    let colour_area = swatch(theme, move |t| shown_colour.get().unwrap_or(t.accent));

    let mut parts: Vec<(String, String, gtk::Widget)> = names
        .iter()
        .map(|name| (name.clone(), files::title(name), thumbnail(&dir, name)))
        .collect();
    parts.push((
        NONE_CARD.to_string(),
        tr("None").to_string(),
        swatch(theme, |t| t.background).upcast::<gtk::Widget>(),
    ));
    parts.push((
        PICTURE_CARD.to_string(),
        tr("Your picture").to_string(),
        picture_prompt().upcast::<gtk::Widget>(),
    ));
    parts.push((
        COLOUR_CARD.to_string(),
        tr("Colour").to_string(),
        colour_area.clone().upcast::<gtk::Widget>(),
    ));

    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let mut first: Option<gtk::ToggleButton> = None;
    let mut cards = Vec::new();
    for (key, title, picture) in &parts {
        let (button, badge) = widgets::picture_card(title, picture);
        match &first {
            Some(first) => button.set_group(Some(first)),
            None => first = Some(button.clone()),
        }
        flow.append(&button);
        // The card takes the focus, not the box the flow wraps it in.
        if let Some(child) = button.parent() {
            child.set_focusable(false);
        }
        cards.push((key.clone(), button, badge));
    }
    content.append(&flow);

    let logged = cards.clone();
    let card = Rc::new(Card {
        dir,
        theme: theme.clone(),
        cards,
        pairs,
        colour_area,
        colour_shown,
        source,
        problem: problem.clone(),
        quiet: Cell::new(false),
    });
    wire(&card);
    log_places(&flow, logged, card.source.reset.clone());
    card.show(&Files::here());
    card
}

/// Connects each card, Reset and Copy as command. A wallpaper and None
/// are chosen when they turn on; Your picture and Colour open their
/// dialogs on every click, so a click on the one chosen asks again.
fn wire(card: &Rc<Card>) {
    let weak = Rc::downgrade(card);
    for (index, (key, button, _)) in card.cards.iter().enumerate() {
        let weak = weak.clone();
        match index.checked_sub(card.pairs) {
            None | Some(0) => {
                let value = key.clone();
                button.connect_toggled(move |button| {
                    if !button.is_active() {
                        return;
                    }
                    if let Some(card) = weak.upgrade() {
                        if !card.quiet.get() {
                            card.write(&value);
                        }
                    }
                });
            }
            Some(1) => {
                button.connect_clicked(move |button| {
                    pick_picture(weak.clone(), button.clone().upcast::<gtk::Widget>());
                });
            }
            Some(_) => {
                button.connect_clicked(move |button| pick_colour(weak.clone(), button.clone()));
            }
        }
    }
    let reset_weak = weak.clone();
    card.source.reset.connect_clicked(move |_| {
        let Some(card) = reset_weak.upgrade() else {
            return;
        };
        let result = Files::here().set(KEY, None);
        card.report(result);
        card.show(&Files::here());
    });
    let dir = card.dir.clone();
    card.source.copy.connect_clicked(move |button| {
        let value = shown(Files::here().chosen(KEY), &dir);
        button.clipboard().set_text(&rows::command(&[(KEY, value)]));
        widgets::copied(button);
    });
}

/// Asks for a picture and chooses it. A cancelled choice leaves the card
/// as it was.
fn pick_picture(card: Weak<Card>, near: gtk::Widget) {
    glib::spawn_future_local(async move {
        let picked = widgets::pick_file(&near, tr("Choose a picture"), None).await;
        let Some(card) = card.upgrade() else {
            return;
        };
        match picked {
            None => card.show(&Files::here()),
            Some(path) => match picture_value(&path) {
                Ok(value) => card.write(&value),
                Err(e) => {
                    card.report(Err(e));
                    card.show(&Files::here());
                }
            },
        }
    });
}

/// The value a picture file chooses: its path, if it is a JPEG or PNG.
fn picture_value(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match (ext.as_deref(), path.to_str()) {
        (Some("jpg" | "jpeg" | "png"), Some(text)) => Ok(text.to_string()),
        _ => Err(tr("Pick a JPEG or PNG picture").to_string()),
    }
}

/// Asks for a colour, starting from the one chosen (else the accent), and
/// chooses it as `#rrggbb`. A cancelled choice leaves the card as it was.
fn pick_colour(card: Weak<Card>, button: gtk::ToggleButton) {
    let Some(now) = card.upgrade() else {
        return;
    };
    let current = now
        .colour_shown
        .get()
        .unwrap_or_else(|| now.theme.tokens().accent);
    let initial = current.hex().parse::<gdk::RGBA>().ok();
    drop(now);
    let window = button.root().and_downcast::<gtk::Window>();
    gtk::ColorDialog::new().choose_rgba(
        window.as_ref(),
        initial.as_ref(),
        None::<&gio::Cancellable>,
        move |picked| {
            let Some(card) = card.upgrade() else {
                return;
            };
            match picked {
                Ok(rgba) => card.write(&hex_of(&rgba)),
                Err(_) => card.show(&Files::here()),
            }
        },
    );
}

/// A colour as `#rrggbb`, lowercase, each channel rounded to 0 to 255.
fn hex_of(c: &gdk::RGBA) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(c.red()),
        channel(c.green()),
        channel(c.blue())
    )
}

/// An empty picture the card's size, which the thumbnails and the prompt
/// share.
fn frame() -> gtk::Box {
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["edel-wallpaper-thumb"])
        .build();
    frame.set_size_request(WIDTH, HEIGHT);
    frame
}

/// Your picture's card: a prompt where a picture would be.
fn picture_prompt() -> gtk::Box {
    let frame = frame();
    frame.append(
        &gtk::Label::builder()
            .label(tr("Choose a picture"))
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .build(),
    );
    frame
}

/// A wallpaper's card picture: its light picture on the left half and its
/// dark picture on the right, each scaled to the card. A name with only one
/// picture uses it for both halves; a picture that will not load leaves an
/// empty card.
fn thumbnail(dir: &Path, name: &str) -> gtk::Widget {
    let (light, dark) = match (
        wallpapers::find(dir, name, false),
        wallpapers::find(dir, name, true),
    ) {
        (Some(light), Some(dark)) => (light, dark),
        (Some(one), None) | (None, Some(one)) => (one.clone(), one),
        (None, None) => return frame().upcast(),
    };
    let load = |path: &Path| Pixbuf::from_file_at_scale(path, WIDTH, HEIGHT, false).ok();
    let (Some(light), Some(dark)) = (load(&light), load(&dark)) else {
        return frame().upcast();
    };
    let Some(both) = Pixbuf::new(Colorspace::Rgb, false, 8, WIDTH, HEIGHT) else {
        return frame().upcast();
    };
    let half = WIDTH / 2;
    light.copy_area(0, 0, half, HEIGHT, &both, 0, 0);
    dark.copy_area(half, 0, half, HEIGHT, &both, half, 0);
    let picture = gtk::Picture::for_paintable(&gdk::Texture::for_pixbuf(&both));
    picture.set_can_shrink(false);
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_size_request(WIDTH, HEIGHT);
    picture.add_css_class("edel-wallpaper-thumb");
    picture.set_overflow(gtk::Overflow::Hidden);
    picture.upcast()
}

/// A card's colour as drawn: the colour `pick` gives over the whole card,
/// with a hairline in the tokens' line colour. Drawn again when the tokens
/// change.
fn swatch(theme: &Rc<Theme>, pick: impl Fn(&Tokens) -> Colour + 'static) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(WIDTH)
        .content_height(HEIGHT)
        .css_classes(["edel-wallpaper-thumb"])
        .build();
    let drawn = theme.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let t = drawn.tokens();
        let (w, h) = (f64::from(w), f64::from(h));
        let c = pick(&t);
        cr.set_source_rgba(
            f64::from(c.r),
            f64::from(c.g),
            f64::from(c.b),
            f64::from(c.a),
        );
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        let line = t.line;
        cr.set_source_rgba(
            f64::from(line.r),
            f64::from(line.g),
            f64::from(line.b),
            f64::from(line.a),
        );
        cr.set_line_width(1.0);
        cr.rectangle(0.5, 0.5, w - 1.0, h - 1.0);
        let _ = cr.stroke();
    });
    let weak = area.downgrade();
    theme.watch(move || {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area
}

/// Logs where the group's cards and its Reset lie in the window, once the
/// flow is laid out, for CI to click (logical pixels), as `edel-settings:
/// wallpaper group places NAME X+Y+WxH ..., reset X+Y+WxH`. Asked for by
/// this row's key, the page first scrolls the group to the top and gives
/// the keyboard to the chosen card, as the Style group does; it logs once
/// two frames agree.
fn log_places(
    flow: &gtk::FlowBox,
    cards: Vec<(String, gtk::ToggleButton, gtk::Box)>,
    reset: gtk::Button,
) {
    let scrolled = Cell::new(false);
    let last = RefCell::new(String::new());
    flow.add_tick_callback(move |flow, _| {
        let Some(window) = flow.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !flow.is_mapped() || flow.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            if widgets::asked_row() != Some(KEY) {
                return glib::ControlFlow::Break;
            }
            widgets::scroll_to_top(flow.upcast_ref());
            let page = std::iter::successors(
                Some(flow.clone().upcast::<gtk::Widget>()),
                gtk::Widget::parent,
            )
            .find(|w| w.has_css_class(widgets::ASKED));
            let chosen = cards
                .iter()
                .find(|(_, button, _)| button.is_active())
                .map(|(_, button, _)| button.clone());
            if let (Some(page), Some(chosen)) = (page, chosen) {
                widgets::take_asked(&page, &chosen);
            }
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = cards
            .iter()
            .map(|(key, button, _)| {
                format!("{key} {}", widgets::place(button.compute_bounds(&window)))
            })
            .collect();
        let line = format!(
            "edel-settings: wallpaper group places {}, reset {}",
            places.join(" "),
            widgets::place(reset.compute_bounds(&window)),
        );
        // The view settles over a few frames, so the places are logged only
        // once two frames agree.
        if last.replace(line.clone()) != line {
            return glib::ControlFlow::Continue;
        }
        eprintln!("{line}");
        glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, named after the test and this process.
    fn folder(test: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edel-settings-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for file in files {
            std::fs::write(dir.join(file), b"").unwrap();
        }
        dir
    }

    #[test]
    fn nobody_choosing_gives_the_default_when_carried_else_none() {
        let with = folder("release_default", &["hills-light.jpg"]);
        assert_eq!(release_value(&with), "hills");
        let without = folder("release_none", &["earth.png"]);
        assert_eq!(release_value(&without), "none");
    }

    #[test]
    fn the_value_shown_is_the_chosen_one_else_the_release() {
        let dir = folder("shown", &["hills-light.jpg"]);
        assert_eq!(shown(Some("#1f2b48".into()), &dir), "#1f2b48");
        assert_eq!(shown(Some("none".into()), &dir), "none");
        assert_eq!(shown(None, &dir), "hills");
    }

    #[test]
    fn a_value_is_none_a_colour_a_picture_or_a_name() {
        assert_eq!(kind("none"), Kind::None);
        assert_eq!(kind("#1f2b48"), Kind::Colour("#1f2b48".into()));
        assert_eq!(
            kind("/home/ali/sea.jpg"),
            Kind::Picture(PathBuf::from("/home/ali/sea.jpg"))
        );
        assert_eq!(kind("hills"), Kind::Pair("hills".into()));
    }

    #[test]
    fn a_value_reads_as_its_name_or_its_colour() {
        assert_eq!(title_of("hills"), "Hills");
        assert_eq!(title_of("none"), "None");
        assert_eq!(title_of("#1f2b48"), "#1f2b48");
        assert_eq!(title_of("/home/ali/sea.jpg"), "sea.jpg");
    }

    #[test]
    fn a_picture_is_a_jpeg_or_a_png() {
        assert_eq!(
            picture_value(Path::new("/home/ali/Sea.JPG")),
            Ok("/home/ali/Sea.JPG".to_string())
        );
        assert!(picture_value(Path::new("/home/ali/sea.gif")).is_err());
        assert!(picture_value(Path::new("/home/ali/sea")).is_err());
    }
}
