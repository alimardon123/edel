//! The Appearance page's Shape group (M5.12b2c): the corners, the gaps
//! between windows and at the screen's edge, the border, the title bar's
//! height and the panels' opacity, each a slider over its key's range
//! (`settings::whole_range`) with its value at the end, and a picture of
//! two windows drawn with them. Each slider is one key of the person's
//! settings file: it starts at the value the files give (the person's over
//! the machine's, else the release's from the tokens), and the value the
//! release has is taken out, never written (`Files::choose_over`, ADR-008).
//! A drag shows its value at once and writes the key once it settles, as
//! the Sound page's volume does, so a drag is one write. `show` follows the
//! files; asked for by one of its keys, the group scrolls to the top and
//! gives the keyboard to that slider, and once two frames agree it logs
//! where the sliders lie for CI.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::settings::{self, Source};
use edel::tokens::{Shape as Keys, Tokens};

use crate::files::Files;
use crate::style::Theme;
use crate::{colours, preview, rows, widgets};

/// How long after the last move of a slider its value is written.
const SETTLE: Duration = Duration::from_millis(250);
/// The picture's height in logical pixels.
const PICTURE_HEIGHT: i32 = 150;
/// The width of each slider's control, as Sound's volume slider has it.
const SLIDER_WIDTH: i32 = 190;

/// The six values, in the order `settings::SHAPE_KEYS` lists their keys.
const RADIUS: usize = 0;
const GAPS: usize = 1;
const GAPS_OUTER: usize = 2;
const BORDER: usize = 3;
const TITLE_BAR: usize = 4;
const OPACITY: usize = 5;

/// The six values a slider shows, in `settings::SHAPE_KEYS`' order.
pub type Shown = [u32; 6];

/// The release's six values, from the tokens (`Tokens::built_in()`).
fn values_of(t: &Tokens) -> Shown {
    [
        t.radius_window,
        t.gap,
        t.gap_outer,
        t.border,
        t.title_bar_height,
        t.panel_opacity,
    ]
}

/// The values that apply now, with the shape the files set on top of the
/// release, and the keys the files set.
fn now(files: &Files) -> Shown {
    let machine = read_file(&files.machine);
    let person = files.person.as_deref().and_then(read_file);
    let keys = Keys::between(machine.as_ref(), person.as_ref());
    values_of(&Tokens::built_in().with_shape(&keys).0)
}

/// A settings file read leniently, as the desktop reads it; none when it
/// is missing or not TOML.
fn read_file(path: &Path) -> Option<settings::SettingsFile> {
    let text = std::fs::read_to_string(path).ok()?;
    settings::read(&text).ok().map(|read| read.file)
}

/// What a slider's end shows for its value: `10 px`, `Square` for corners
/// of 0, `None` for a border of 0, `Same as gaps` while the gaps at the
/// edge are unset, and `100 %` for the panels' opacity.
pub fn words(index: usize, value: u32, unset: bool) -> String {
    let number = value.to_string();
    match index {
        RADIUS if value == 0 => tr("Square").to_string(),
        BORDER if value == 0 => tr("None").to_string(),
        GAPS_OUTER if unset => tr("Same as gaps").to_string(),
        OPACITY => trf("{n} %", &[("n", &number)]),
        _ => trf("{n} px", &[("n", &number)]),
    }
}

/// A frame of the picture: its place in logical pixels from its top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The two frames of the picture in a `width` by `height` area, the left
/// one focused: `gaps_outer` from the area's edges and `gaps` between them,
/// as the tiling places windows, so the gap between them is `gaps`.
pub fn frames(width: f64, height: f64, values: &Shown) -> [Frame; 2] {
    let edge = f64::from(values[GAPS_OUTER]);
    let gap = f64::from(values[GAPS]);
    let w = ((width - 2.0 * edge - gap) / 2.0).max(0.0);
    let h = (height - 2.0 * edge).max(0.0);
    [
        Frame {
            x: edge,
            y: edge,
            w,
            h,
        },
        Frame {
            x: edge + w + gap,
            y: edge,
            w,
            h,
        },
    ]
}

/// Draws the picture: the background, then the two frames with their
/// corners, title bars and borders, the focused one's border in the
/// gradient from `window_border_focused` to its end when they differ.
fn draw(cr: &gtk::cairo::Context, width: f64, height: f64, t: &Tokens, values: &Shown) {
    preview::set(cr, t.background);
    let _ = cr.paint();
    let radius = f64::from(values[RADIUS]);
    let border = f64::from(values[BORDER]);
    let title = f64::from(values[TITLE_BAR]);
    for (index, frame) in frames(width, height, values).into_iter().enumerate() {
        let focused = index == 0;
        let Frame { x, y, w, h } = frame;
        let _ = cr.save();
        preview::rounded(cr, x, y, w, h, radius);
        cr.clip();
        preview::set(cr, t.window);
        let _ = cr.paint();
        let bar = if focused {
            t.title_bar_focused
        } else {
            t.title_bar
        };
        preview::set(cr, bar);
        cr.rectangle(x, y, w, title.min(h));
        let _ = cr.fill();
        let _ = cr.restore();
        if border > 0.0 {
            // The stroke lies inside the frame, half its width in.
            let inset = border / 2.0;
            preview::rounded(
                cr,
                x + inset,
                y + inset,
                w - border,
                h - border,
                (radius - inset).max(0.0),
            );
            cr.set_line_width(border);
            if focused && t.window_border_focused.hex() != t.window_border_focused_end.hex() {
                let gradient = gtk::cairo::LinearGradient::new(x, 0.0, x + w, 0.0);
                let start = t.window_border_focused;
                let end = t.window_border_focused_end;
                gradient.add_color_stop_rgba(
                    0.0,
                    f64::from(start.r),
                    f64::from(start.g),
                    f64::from(start.b),
                    f64::from(start.a),
                );
                gradient.add_color_stop_rgba(
                    1.0,
                    f64::from(end.r),
                    f64::from(end.g),
                    f64::from(end.b),
                    f64::from(end.a),
                );
                let _ = cr.set_source(&gradient);
            } else {
                let colour = if focused {
                    t.window_border_focused
                } else {
                    t.window_border
                };
                preview::set(cr, colour);
            }
            let _ = cr.stroke();
        }
    }
}

/// The value that applies without the key `index`, which `choose_over`
/// takes it out at: the release's, and for the gaps at the edge the gaps
/// that apply now.
fn release_of(index: usize, files: &Files) -> u32 {
    if index == GAPS_OUTER {
        now(files)[GAPS]
    } else {
        values_of(&Tokens::built_in())[index]
    }
}

/// One slider: its scale, its value at the end, its row and the write it
/// waits for while a drag goes on.
struct Slider {
    scale: gtk::Scale,
    words: gtk::Label,
    row: widgets::Row,
    pending: RefCell<Option<glib::SourceId>>,
}

/// The Shape group on the Appearance page.
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    /// The page's line for a problem.
    problem: gtk::Label,
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    sliders: Vec<Slider>,
    /// The values the picture draws, redrawn on every move.
    shown: Rc<Cell<Shown>>,
    picture: gtk::DrawingArea,
    /// Set while `show` moves a slider, so that is not a choice.
    quiet: Cell<bool>,
}

impl Group {
    /// Shows what the files say now: each slider at its value (unless the
    /// person is dragging it), its value at the end, the line under it and
    /// its Reset, then the group's source line and the picture.
    pub fn show(&self) {
        let files = Files::here();
        let values = now(&files);
        self.quiet.set(true);
        for (index, slider) in self.sliders.iter().enumerate() {
            let key = settings::SHAPE_KEYS[index];
            if slider.pending.borrow().is_none() {
                slider.scale.set_value(f64::from(values[index]));
            }
            let value = slider.scale.value().round() as u32;
            let unset = files.chosen(key).is_none();
            slider.words.set_label(&words(index, value, unset));
            let source = files.source(key);
            slider.row.reset.set_visible(rows::resettable(&source));
            let mut line = Vec::new();
            if index == BORDER {
                line.push(trf(
                    "Its colours are in Colours: {name}",
                    &[("name", &colours::readable("window_border_focused"))],
                ));
            }
            if let Some(note) = rows::note(&source, false) {
                line.push(note.to_string());
            }
            slider.row.subtitle.set_label(&line.join(" · "));
            slider.row.subtitle.set_visible(!line.is_empty());
        }
        self.quiet.set(false);
        self.shown.set(self.scale_values());
        self.picture.queue_draw();
        self.source.label.set_label(&self.source_text(&files));
        self.source.reset.set_visible(
            settings::SHAPE_KEYS
                .iter()
                .any(|key| matches!(files.source(key), Source::Person(_))),
        );
    }

    /// The group's source line: your choice when the person sets any of the
    /// six, else this machine's, else the release's.
    fn source_text(&self, files: &Files) -> String {
        let sources: Vec<Source> = settings::SHAPE_KEYS
            .iter()
            .map(|key| files.source(key))
            .collect();
        if sources.iter().any(|s| matches!(s, Source::Person(_))) {
            tr("Your choice").to_string()
        } else if sources.iter().any(|s| matches!(s, Source::Machine(_))) {
            tr("Set by this machine").to_string()
        } else {
            trf("Automatic ({current})", &[("current", tr("the release"))])
        }
    }

    /// The six values the sliders are at now.
    fn scale_values(&self) -> Shown {
        let mut values = [0u32; 6];
        for (value, slider) in values.iter_mut().zip(&self.sliders) {
            *value = slider.scale.value().round() as u32;
        }
        values
    }

    /// A slider moved: its end and the picture follow at once, and its
    /// value is written once it settles, unless the page moved it.
    fn moved(&self, index: usize, value: u32) {
        self.sliders[index]
            .words
            .set_label(&words(index, value, false));
        self.shown.set(self.scale_values());
        self.picture.queue_draw();
        if self.quiet.get() {
            return;
        }
        let mut pending = self.sliders[index].pending.borrow_mut();
        if let Some(id) = pending.take() {
            id.remove();
        }
        let weak = self.me.clone();
        *pending = Some(glib::timeout_add_local_once(SETTLE, move || {
            if let Some(group) = weak.upgrade() {
                group.sliders[index].pending.borrow_mut().take();
                group.write(index);
            }
        }));
    }

    /// Writes slider `index`'s value to the person's file, or takes the key
    /// out when it is what applies without it, and logs what was done.
    fn write(&self, index: usize) {
        let key = settings::SHAPE_KEYS[index];
        let value = self.sliders[index].scale.value().round() as u32;
        let files = Files::here();
        let release = release_of(index, &files);
        match files.choose_over(key, &value.to_string(), Some(&release.to_string())) {
            Ok(()) => {
                self.problem.set_visible(false);
                let written = matches!(Files::here().source(key), Source::Person(_));
                let what = if written { "written" } else { "taken out" };
                eprintln!("edel-settings: shape {key} {value} {what}");
            }
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
        self.show();
    }

    /// Sets `key` to `value` in the person's file, or takes it out with
    /// none, and shows the result; a refusal is shown in the problem line.
    fn set(&self, key: &str, value: Option<&str>) {
        match Files::here().set(key, value) {
            Ok(()) => self.problem.set_visible(false),
            Err(e) => {
                self.problem.set_label(&e);
                self.problem.set_visible(true);
            }
        }
        self.show();
    }
}

/// Builds the Shape group in `content`, after the Colours group: the
/// section with its source line, Reset and Copy as command, the six rows
/// with their sliders, and the picture under them.
pub fn group(content: &gtk::Box, theme: &Rc<Theme>, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Shape"));
    let card = widgets::group(content);
    let mut sliders = Vec::new();
    for key in settings::SHAPE_KEYS {
        let (least, most) = settings::whole_range(key).expect("a shape key has a whole range");
        let scale =
            gtk::Scale::with_range(gtk::Orientation::Horizontal, least as f64, most as f64, 1.0);
        scale.set_digits(0);
        scale.set_draw_value(false);
        scale.set_width_request(SLIDER_WIDTH);
        scale.set_hexpand(true);
        let title = rows::title(key);
        scale.update_property(&[gtk::accessible::Property::Label(title)]);
        let words = gtk::Label::builder()
            .width_chars(9)
            .xalign(1.0)
            .css_classes(["edel-value"])
            .build();
        let control = gtk::Box::builder().spacing(10).build();
        control.append(&scale);
        control.append(&words);
        let row = widgets::row(&card, title, control.upcast_ref());
        sliders.push(Slider {
            scale,
            words,
            row,
            pending: RefCell::new(None),
        });
    }

    // The picture, a row of the card below the sliders.
    let picture = gtk::DrawingArea::builder()
        .content_height(PICTURE_HEIGHT)
        .hexpand(true)
        .build();
    picture.update_property(&[gtk::accessible::Property::Label(tr(
        "How the windows will look with these shapes",
    ))]);
    let shown = Rc::new(Cell::new([0u32; 6]));
    let drawn = theme.clone();
    let values = shown.clone();
    picture.set_draw_func(move |_, cr, w, h| {
        draw(
            cr,
            f64::from(w),
            f64::from(h),
            &drawn.tokens(),
            &values.get(),
        );
    });
    let weak = picture.downgrade();
    theme.watch(move || {
        if let Some(picture) = weak.upgrade() {
            picture.queue_draw();
        }
    });
    let picture_row = gtk::Box::builder()
        .css_classes(["edel-row", "edel-preview-row"])
        .build();
    picture_row.append(&picture);
    picture_row.set_can_target(false);
    card.append(&picture_row);

    let scales: Vec<gtk::Scale> = sliders.iter().map(|s| s.scale.clone()).collect();
    let group = Rc::new_cyclic(|me| Group {
        me: me.clone(),
        problem: problem.clone(),
        source,
        sliders,
        shown,
        picture,
        quiet: Cell::new(false),
    });
    wire(&group);
    log_places(&card, scales);
    group.show();
    group
}

/// Connects each slider's moves, the group's Reset and Copy as command and
/// each row's Reset and Copy as command.
fn wire(group: &Rc<Group>) {
    for (index, slider) in group.sliders.iter().enumerate() {
        let weak = Rc::downgrade(group);
        slider.scale.connect_value_changed(move |scale| {
            if let Some(group) = weak.upgrade() {
                group.moved(index, scale.value().round() as u32);
            }
        });
        let weak = Rc::downgrade(group);
        let key = settings::SHAPE_KEYS[index];
        slider.row.reset.connect_clicked(move |_| {
            if let Some(group) = weak.upgrade() {
                group.set(key, None);
            }
        });
        let weak = Rc::downgrade(group);
        slider.row.copy.connect_clicked(move |button| {
            let Some(group) = weak.upgrade() else {
                return;
            };
            let value = group.sliders[index].scale.value().round() as u32;
            button
                .clipboard()
                .set_text(&rows::command(&[(key, value.to_string())]));
            widgets::copied(button);
        });
    }
    let weak = Rc::downgrade(group);
    group.source.reset.connect_clicked(move |_| {
        let Some(group) = weak.upgrade() else {
            return;
        };
        let files = Files::here();
        let mut result = Ok(());
        for key in settings::SHAPE_KEYS {
            if matches!(files.source(key), Source::Person(_)) {
                result = result.and(files.set(key, None));
            }
        }
        match result {
            Ok(()) => group.problem.set_visible(false),
            Err(e) => {
                group.problem.set_label(&e);
                group.problem.set_visible(true);
            }
        }
        group.show();
    });
    let weak = Rc::downgrade(group);
    group.source.copy.connect_clicked(move |button| {
        let Some(group) = weak.upgrade() else {
            return;
        };
        let values = group.scale_values();
        let pairs: Vec<(&str, String)> = settings::SHAPE_KEYS
            .iter()
            .zip(values)
            .map(|(key, value)| (*key, value.to_string()))
            .collect();
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}

/// Logs where the sliders lie in the window, once the card is laid out, for
/// CI, as `edel-settings: shape group places radius X+Y+WxH, gaps ...`
/// (logical pixels). Asked for by one of the keys, the page first scrolls
/// the group to the top and gives the keyboard to that slider; it logs once
/// two frames agree.
fn log_places(card: &gtk::Box, scales: Vec<gtk::Scale>) {
    let scrolled = Cell::new(false);
    let last = RefCell::new(String::new());
    card.add_tick_callback(move |card, _| {
        let Some(window) = card.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !card.is_mapped() || card.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            let asked = widgets::asked_row()
                .and_then(|row| settings::SHAPE_KEYS.iter().position(|key| *key == row));
            if let Some(index) = asked {
                widgets::scroll_to_top(card.upcast_ref());
                let page = std::iter::successors(
                    Some(card.clone().upcast::<gtk::Widget>()),
                    gtk::Widget::parent,
                )
                .find(|w| w.has_css_class(widgets::ASKED));
                if let Some(page) = page {
                    widgets::take_asked(&page, &scales[index]);
                }
            }
            return glib::ControlFlow::Continue;
        }
        let places: Vec<String> = settings::SHAPE_KEYS
            .iter()
            .zip(&scales)
            .map(|(key, scale)| {
                let name = key.trim_start_matches("appearance.");
                format!("{name} {}", widgets::place(scale.compute_bounds(&window)))
            })
            .collect();
        let line = format!("edel-settings: shape group places {}", places.join(", "));
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

    #[test]
    fn the_values_read_as_words_at_their_ends() {
        assert_eq!(words(RADIUS, 0, false), "Square");
        assert_eq!(words(RADIUS, 24, false), "24 px");
        assert_eq!(words(RADIUS, 10, false), "10 px");
        assert_eq!(words(GAPS, 0, false), "0 px");
        assert_eq!(words(GAPS, 64, false), "64 px");
        assert_eq!(words(GAPS_OUTER, 8, true), "Same as gaps");
        assert_eq!(words(GAPS_OUTER, 0, false), "0 px");
        assert_eq!(words(BORDER, 0, false), "None");
        assert_eq!(words(BORDER, 10, false), "10 px");
        assert_eq!(words(TITLE_BAR, 24, false), "24 px");
        assert_eq!(words(TITLE_BAR, 48, false), "48 px");
        assert_eq!(words(OPACITY, 20, false), "20 %");
        assert_eq!(words(OPACITY, 100, false), "100 %");
    }

    #[test]
    fn the_sliders_take_their_ranges_from_the_key_table() {
        let expected = [(0, 24), (0, 64), (0, 64), (0, 10), (24, 48), (20, 100)];
        for (key, want) in settings::SHAPE_KEYS.iter().zip(expected) {
            assert_eq!(settings::whole_range(key), Some(want), "{key}");
        }
    }

    #[test]
    fn the_picture_keeps_both_frames_inside_with_the_gap_between_them() {
        for width in [328.0, 400.0, 600.0, 704.0] {
            for height in [PICTURE_HEIGHT as f64, 200.0] {
                for gaps in [0, 8, 64] {
                    for gaps_outer in [0, 8, 64] {
                        let values: Shown = [10, gaps, gaps_outer, 1, 28, 100];
                        let [left, right] = frames(width, height, &values);
                        let gap = f64::from(gaps);
                        let edge = f64::from(gaps_outer);
                        // Only an area too small for the edges is skipped:
                        // the picture is taller than two edges at 64 px.
                        if width < 2.0 * edge + gap || height < 2.0 * edge {
                            continue;
                        }
                        assert!(left.x >= 0.0 && left.y >= 0.0, "{values:?}");
                        assert!(right.x + right.w <= width + 1e-9, "{values:?} {width}");
                        assert!(left.y + left.h <= height + 1e-9, "{values:?} {height}");
                        assert!(right.y + right.h <= height + 1e-9, "{values:?} {height}");
                        assert!(
                            (right.x - (left.x + left.w) - gap).abs() < 1e-9,
                            "{values:?} {width}"
                        );
                        assert!(
                            (left.w - right.w).abs() < 1e-9,
                            "the frames are equal in width"
                        );
                    }
                }
            }
        }
    }
}
