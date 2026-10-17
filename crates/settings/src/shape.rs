//! The Appearance page's Shape group (M5.12b2c): the corners, the gaps
//! between windows and at the screen's edge, the border, the title bar's
//! height and the panels' opacity, each a slider over its key's range
//! (`settings::whole_range`) with its value at the end, and a picture of a
//! small desktop with two tiled windows drawn with them, first in the group
//! so each change shows without scrolling. Each slider is one key of the
//! person's settings file: it starts at the value the files give (the
//! person's over the machine's, else the release's from the tokens), and
//! the value the release has is taken out, never written
//! (`Files::choose_over`, ADR-008). A drag shows its value at once and
//! writes the key once it settles, as the Sound page's volume does, so a
//! drag is one write. The six rows are one grid: the sliders start at one
//! x and their values end at one x, and below the page's narrow breakpoint
//! each slider goes under its title at full width. `show` follows the
//! files; asked for by one of its keys, the group scrolls to the top and
//! gives the keyboard to that slider, and once two frames agree it logs
//! where the sliders lie for CI.

use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::cairo::Context;
use gtk::glib;
use gtk::pango;

use edel::i18n::{tr, trf};
use edel::settings::{self, Source};
use edel::tokens::{Colour, Shape as Keys, Tokens};

use crate::files::Files;
use crate::style::Theme;
use crate::{colours, preview, rows, widgets};

/// How long after the last move of a slider its value is written.
const SETTLE: Duration = Duration::from_millis(250);
/// The picture's height in logical pixels: the desktop with its room.
pub const PICTURE_HEIGHT: i32 = 180;
/// The slider's width where the grid is wide (below the narrow breakpoint
/// the slider takes the width its row gives it).
const SLIDER: i32 = 220;
/// The value column's width; the value is right-aligned in it.
const VALUE: i32 = 96;
/// The room round the picture's desktop, in logical pixels.
const ROOM: f64 = 16.0;
/// The panel strip along the desktop's foot, in logical pixels (not scaled).
const PANEL: f64 = 14.0;
/// The height the picture's sizes are scaled over: a value is drawn at
/// `height / SKETCH` of its size, so 8 px reads as 3 px at 150 px; half a
/// real screen's height, so the gaps and the border show at a glance.
const SKETCH: f64 = 400.0;
/// The space at a row's ends.
const PAD: i32 = 14;
/// The grid's columns when it is wide: title, slider, value, actions.
const COLUMNS: i32 = 4;

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
pub(crate) fn read_file(path: &Path) -> Option<settings::SettingsFile> {
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

/// The scale the picture draws its sizes at, for a picture `height` tall.
fn scale_of(height: f64) -> f64 {
    height / SKETCH
}

/// A gap or border drawn at `scale`: at least 1 px when its value is above 0.
fn px(value: u32, scale: f64) -> f64 {
    if value == 0 {
        0.0
    } else {
        (f64::from(value) * scale).max(1.0)
    }
}

/// A title bar drawn at `scale`: at least 6 px.
fn bar_px(value: u32, scale: f64) -> f64 {
    (f64::from(value) * scale).max(6.0)
}

/// The desktop's width and the height above its panel, for a picture
/// `width` by `height` logical pixels: the room is round the desktop.
pub fn area_of(width: f64, height: f64) -> (f64, f64) {
    let desk_w = (width - 2.0 * ROOM).max(0.0);
    let area_h = (height - 2.0 * ROOM - PANEL).max(0.0);
    (desk_w, area_h)
}

/// A frame of the picture: its place in logical pixels from its top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The two frames in an area `width` by `height` (above the panel), the
/// left one focused: the gaps at the edge around them and the gap between
/// them, each drawn at `scale`, as the tiling places windows.
pub fn frames(width: f64, height: f64, values: &Shown, scale: f64) -> [Frame; 2] {
    let edge = px(values[GAPS_OUTER], scale);
    let gap = px(values[GAPS], scale);
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

/// A colour with its alpha scaled by `alpha`.
fn rgba(cr: &Context, c: Colour, alpha: f64) {
    cr.set_source_rgba(
        f64::from(c.r),
        f64::from(c.g),
        f64::from(c.b),
        f64::from(c.a) * alpha,
    );
}

/// Draws the picture: the box with its corners and hairline, the desktop
/// in the overview's backdrop inside its room, two windows in the desktop's
/// area above the panel, and the panel strip at the desktop's foot.
fn draw(cr: &Context, width: f64, height: f64, t: &Tokens, values: &Shown) {
    let scale = scale_of(height);
    preview::rounded(
        cr,
        0.5,
        0.5,
        width - 1.0,
        height - 1.0,
        f64::from(t.radius_menu),
    );
    preview::set(cr, t.card);
    let _ = cr.fill_preserve();
    preview::set(cr, t.line);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    let (desk_w, area_h) = area_of(width, height);
    let desk_h = (height - 2.0 * ROOM).max(0.0);
    let _ = cr.save();
    cr.translate(ROOM, ROOM);
    // The desktop as the overview draws it without a wallpaper, the
    // backdrop's gradient, so light windows stand out in either scheme.
    let desktop = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, desk_h);
    for (stop, c) in [(0.0, t.backdrop), (1.0, t.backdrop_deep)] {
        desktop.add_color_stop_rgba(
            stop,
            f64::from(c.r),
            f64::from(c.g),
            f64::from(c.b),
            f64::from(c.a),
        );
    }
    let _ = cr.set_source(&desktop);
    cr.rectangle(0.0, 0.0, desk_w, desk_h);
    let _ = cr.fill();

    let radius = f64::from(values[RADIUS]) * scale;
    let border = px(values[BORDER], scale);
    let bar = bar_px(values[TITLE_BAR], scale);
    for (index, frame) in frames(desk_w, area_h, values, scale)
        .into_iter()
        .enumerate()
    {
        draw_window(cr, frame, index == 0, radius, border, bar, t);
    }

    let panel = (desk_h - area_h).max(0.0);
    rgba(cr, t.panel, f64::from(values[OPACITY]) / 100.0);
    cr.rectangle(0.0, area_h, desk_w, panel);
    let _ = cr.fill();
    let _ = cr.restore();
}

/// One window: its body in `window`, its title bar (`title_bar_focused`
/// for the focused one) with three small buttons at its right, its corners
/// rounded, and its border inside the frame in `window_border`, or for the
/// focused one in `window_border_focused`, in the gradient to its end when
/// the two differ.
fn draw_window(
    cr: &Context,
    frame: Frame,
    focused: bool,
    radius: f64,
    border: f64,
    bar: f64,
    t: &Tokens,
) {
    let Frame { x, y, w, h } = frame;
    let bar = bar.min(h);
    let _ = cr.save();
    preview::rounded(cr, x, y, w, h, radius);
    cr.clip();
    preview::set(cr, t.window);
    let _ = cr.paint();
    preview::set(
        cr,
        if focused {
            t.title_bar_focused
        } else {
            t.title_bar
        },
    );
    cr.rectangle(x, y, w, bar);
    let _ = cr.fill();
    buttons(cr, x, y, w, bar, t);
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
        let (start, end) = (t.window_border_focused, t.window_border_focused_end);
        if focused && start.hex() != end.hex() {
            let gradient = gtk::cairo::LinearGradient::new(x, 0.0, x + w, 0.0);
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

/// Three small dots for a window's buttons, at the right of its title bar,
/// drawn only when the window is wide enough to hold them.
fn buttons(cr: &Context, x: f64, y: f64, w: f64, bar: f64, t: &Tokens) {
    let r = (bar * 0.2).max(1.0);
    if w < 12.0 * r {
        return;
    }
    rgba(cr, t.title_text, 0.5);
    for i in 0..3 {
        let cx = x + w - 2.5 * r - f64::from(i) * 3.0 * r;
        cr.arc(cx, y + bar / 2.0, r, 0.0, TAU);
        let _ = cr.fill();
    }
}

/// A grid cell's part of a row: the title with its line, the slider, the
/// value at its end, the Reset and Copy as command at the row's end, and
/// the line above a row, from the second row on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Title,
    Slider,
    Value,
    Actions,
    Line,
}

/// Where a cell lies in the grid: the row's index in its group's keys,
/// its column and grid row, and how many columns it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub cell: Part,
    pub index: usize,
    pub column: i32,
    pub row: i32,
    pub span: i32,
}

/// Where every cell of `rows` rows lies. Wide: one line per row, the title
/// in column 0, the slider in column 1 (`SLIDER` px), the value in column 2
/// and the actions in column 3. Narrow: the title and its actions on one
/// line, and the slider under the title at the full width with the value
/// at its end.
pub fn plan(narrow: bool, rows: usize) -> Vec<Place> {
    let mut places = Vec::new();
    for index in 0..rows {
        let place = |cell, column, row, span| Place {
            cell,
            index,
            column,
            row,
            span,
        };
        if narrow {
            let top = 3 * index as i32;
            if index > 0 {
                places.push(place(Part::Line, 0, top - 1, COLUMNS));
            }
            places.push(place(Part::Title, 0, top, 2));
            places.push(place(Part::Actions, 2, top, 1));
            places.push(place(Part::Slider, 0, top + 1, 2));
            places.push(place(Part::Value, 2, top + 1, 1));
        } else {
            let top = 2 * index as i32;
            if index > 0 {
                places.push(place(Part::Line, 0, top - 1, COLUMNS));
            }
            places.push(place(Part::Title, 0, top, 1));
            places.push(place(Part::Slider, 1, top, 1));
            places.push(place(Part::Value, 2, top, 1));
            places.push(place(Part::Actions, 3, top, 1));
        }
    }
    places
}

/// The figures of a value in tabular form, so the values line up.
pub(crate) fn tabular() -> pango::AttrList {
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontFeatures::new("tnum=1"));
    attrs
}

/// One slider: its scale, its value at the end, its row, the parts of the
/// row the grid places, and the write it waits for while a drag goes on.
pub(crate) struct Slider {
    pub(crate) scale: gtk::Scale,
    pub(crate) words: gtk::Label,
    pub(crate) row: widgets::Row,
    title: gtk::Box,
    actions: gtk::Box,
    line: gtk::Box,
    pub(crate) pending: RefCell<Option<glib::SourceId>>,
}

/// The slider of `key`, over its key's whole range, with its title and the
/// line under it, its value at the end, and Reset and Copy as command. The
/// Shape and Depth groups both build their rows this way.
pub(crate) fn slider(key: &str) -> Slider {
    let (least, most) = settings::whole_range(key).expect("a slider's key has a whole range");
    let scale =
        gtk::Scale::with_range(gtk::Orientation::Horizontal, least as f64, most as f64, 1.0);
    scale.set_digits(0);
    scale.set_draw_value(false);
    scale.set_valign(gtk::Align::Center);
    let title = rows::title(key);
    scale.update_property(&[gtk::accessible::Property::Label(title)]);
    let words = gtk::Label::builder()
        .width_request(VALUE)
        .xalign(1.0)
        .valign(gtk::Align::Center)
        .css_classes(["edel-value"])
        .build();
    words.set_attributes(Some(&tabular()));

    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-title"])
        .build();
    let keys = gtk::Box::builder()
        .spacing(3)
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    let heading = gtk::Box::builder().spacing(8).build();
    heading.append(&title_label);
    heading.append(&keys);
    let subtitle = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["edel-row-subtitle"])
        .build();
    let title_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    title_box.append(&heading);
    title_box.append(&subtitle);

    let widgets::Source { reset, copy, .. } = widgets::source();
    let actions = gtk::Box::builder()
        .spacing(4)
        .valign(gtk::Align::Center)
        .css_classes(["edel-shape-actions"])
        .build();
    actions.append(&reset);
    actions.append(&copy);
    let line = gtk::Box::builder()
        .hexpand(true)
        .css_classes(["edel-shape-line"])
        .build();
    Slider {
        scale,
        words,
        row: widgets::Row::of(subtitle, keys, reset, copy),
        title: title_box,
        actions,
        line,
        pending: RefCell::new(None),
    }
}

/// Puts the cells of the six rows in `grid` as `plan(narrow, settings::SHAPE_KEYS.len())` places them.
/// The grid is emptied first, so it is laid out again at each breakpoint
/// without any cell being built again.
pub(crate) fn lay_out(grid: &gtk::Grid, sliders: &[Slider], narrow: bool) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    for slider in sliders {
        slider.scale.set_hexpand(narrow);
        slider
            .scale
            .set_size_request(if narrow { -1 } else { SLIDER }, -1);
    }
    for place in plan(narrow, sliders.len()) {
        let slider = &sliders[place.index];
        let widget: gtk::Widget = match place.cell {
            Part::Title => slider.title.clone().upcast(),
            Part::Slider => slider.scale.clone().upcast(),
            Part::Value => slider.words.clone().upcast(),
            Part::Actions => slider.actions.clone().upcast(),
            Part::Line => slider.line.clone().upcast(),
        };
        let (top, bottom) = match place.cell {
            Part::Line => (0, 0),
            Part::Slider if narrow => (0, 9),
            _ => (9, 9),
        };
        let (start, end) = match place.cell {
            Part::Line => (0, 0),
            Part::Actions => (0, PAD),
            Part::Value if narrow => (0, PAD),
            _ if place.column == 0 => (PAD, 0),
            _ => (0, 0),
        };
        widget.set_margin_top(top);
        widget.set_margin_bottom(bottom);
        widget.set_margin_start(start);
        widget.set_margin_end(end);
        grid.attach(&widget, place.column, place.row, place.span, 1);
    }
}

/// Shows a row's Copy as command while the pointer is over any of its
/// title, slider or value, as `widgets::row` does for the other rows.
pub(crate) fn reveal_copy(slider: &Slider) {
    let cells: [gtk::Widget; 3] = [
        slider.title.clone().upcast(),
        slider.scale.clone().upcast(),
        slider.words.clone().upcast(),
    ];
    for cell in cells {
        let motion = gtk::EventControllerMotion::new();
        let (on, off) = (slider.actions.clone(), slider.actions.clone());
        motion.connect_enter(move |_, _, _| on.add_css_class("edel-hover"));
        motion.connect_leave(move |_| off.remove_css_class("edel-hover"));
        cell.add_controller(motion);
    }
}

/// The Shape group on the Appearance page.
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    /// The page's line for a problem.
    problem: gtk::Label,
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    sliders: Rc<Vec<Slider>>,
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
        for (value, slider) in values.iter_mut().zip(self.sliders.iter()) {
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

/// Builds the Shape group in `content`, after the Colours group: the
/// section with its source line, Reset and Copy as command, the picture,
/// then the six rows as one grid of sliders.
pub fn group(content: &gtk::Box, theme: &Rc<Theme>, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Shape"));
    let card = widgets::group(content);

    // The picture first, so a person sees what each slider does at once.
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

    let sliders: Vec<Slider> = settings::SHAPE_KEYS.iter().map(|key| slider(key)).collect();
    let sliders = Rc::new(sliders);

    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .hexpand(true)
        .build();
    lay_out(&grid, &sliders, false);
    // The page's narrow width, as the window's: the card is narrower than
    // the window there, so a window at the breakpoint folds the grid.
    let (g, s) = (grid.clone(), sliders.clone());
    let bin = widgets::folding(&grid, move |narrow| lay_out(&g, &s, narrow));
    card.append(&bin);
    for slider in sliders.iter() {
        reveal_copy(slider);
    }

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
    log_places(&card, "shape", settings::SHAPE_KEYS, scales, None);
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
/// CI, as `edel-settings: NAME group places radius X+Y+WxH, gaps ...`
/// (logical pixels), with `reset X+Y+WxH` last when a `reset` is given. The
/// sliders are `keys`' in order. Asked for by one of the keys, the page
/// first scrolls the group to the top and gives the keyboard to that
/// slider; it logs once two frames agree.
pub(crate) fn log_places(
    card: &gtk::Box,
    name: &'static str,
    keys: &'static [&'static str],
    controls: Vec<impl IsA<gtk::Widget>>,
    reset: Option<gtk::Widget>,
) {
    let scrolled = Cell::new(false);
    // Asked for by one of its keys, the group scrolls to the top again on
    // each frame until the places settle: the pictures above it (the theme
    // card's) load after the first frame and push it down (M8.21 part g).
    let asked_here = Cell::new(false);
    let last = RefCell::new(String::new());
    card.add_tick_callback(move |card, _| {
        let Some(window) = card.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !card.is_mapped() || card.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        if !scrolled.replace(true) {
            let asked =
                widgets::asked_row().and_then(|row| keys.iter().position(|key| *key == row));
            if let Some(index) = asked {
                asked_here.set(true);
                widgets::scroll_to_top(card.upcast_ref());
                let page = std::iter::successors(
                    Some(card.clone().upcast::<gtk::Widget>()),
                    gtk::Widget::parent,
                )
                .find(|w| w.has_css_class(widgets::ASKED));
                if let Some(page) = page {
                    widgets::take_asked(&page, &controls[index]);
                }
            }
            return glib::ControlFlow::Continue;
        }
        if asked_here.get() {
            widgets::scroll_to_top(card.upcast_ref());
        }
        let mut places: Vec<String> = keys
            .iter()
            .zip(&controls)
            .map(|(key, control)| {
                let name = key.trim_start_matches("appearance.");
                format!("{name} {}", widgets::place(control.compute_bounds(&window)))
            })
            .collect();
        if let Some(reset) = &reset {
            places.push(format!(
                "reset {}",
                widgets::place(reset.compute_bounds(&window))
            ));
        }
        let line = format!("edel-settings: {name} group places {}", places.join(", "));
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
    use edel::tokens::Scheme;

    /// The release's values and the largest ones the keys allow, and the
    /// smallest of the rest.
    fn extremes() -> [Shown; 4] {
        let release = values_of(&Tokens::built_in());
        [
            [0, 0, 0, 0, 0, 100],
            release,
            [24, 64, 64, 10, 48, 100],
            [0, 64, 0, 10, 24, 20],
        ]
    }

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
    fn the_picture_keeps_both_windows_in_the_area_above_the_panel() {
        // A Compact card's width, the page's, and a wide window's, at the
        // picture's height.
        for width in [268.0, 568.0, 704.0] {
            let (area_w, area_h) = area_of(width, f64::from(PICTURE_HEIGHT));
            let scale = scale_of(f64::from(PICTURE_HEIGHT));
            for values in extremes() {
                let [left, right] = frames(area_w, area_h, &values, scale);
                let edge = px(values[GAPS_OUTER], scale);
                let gap = px(values[GAPS], scale);
                assert!(left.x >= 0.0 && left.y >= 0.0, "{values:?}");
                assert!((left.x - edge).abs() < 1e-9, "the edge gap, {values:?}");
                assert!(right.x + right.w <= area_w + 1e-9, "{values:?} {area_w}");
                assert!(left.y + left.h <= area_h + 1e-9, "{values:?} {area_h}");
                assert!(right.y + right.h <= area_h + 1e-9, "{values:?} {area_h}");
                assert!(
                    (right.x - (left.x + left.w) - gap).abs() < 1e-9,
                    "the gap between is the scaled gaps, {values:?}"
                );
                assert!(
                    (left.w - right.w).abs() < 1e-9,
                    "the frames are equal in width"
                );
                assert!(
                    bar_px(values[TITLE_BAR], scale) >= 6.0,
                    "the title bar is at least 6 px, {values:?}"
                );
            }
        }
    }

    #[test]
    fn the_picture_scales_its_sizes_to_its_height() {
        // 8 px reads as 3 px at 150 px tall, and a value above 0 is always
        // at least 1 px.
        let scale = scale_of(150.0);
        assert!((px(8, scale) - 3.0).abs() < 1e-9);
        assert_eq!(px(0, scale), 0.0);
        assert_eq!(px(1, scale_of(100.0)), 1.0);
    }

    #[test]
    fn every_row_has_each_of_its_cells_once() {
        for narrow in [false, true] {
            let places = plan(narrow, settings::SHAPE_KEYS.len());
            for index in 0..settings::SHAPE_KEYS.len() {
                for cell in [Part::Title, Part::Slider, Part::Value, Part::Actions] {
                    let count = places
                        .iter()
                        .filter(|p| p.index == index && p.cell == cell)
                        .count();
                    assert_eq!(count, 1, "row {index} {cell:?} narrow {narrow}");
                }
            }
        }
    }

    #[test]
    fn the_sliders_start_at_one_x_and_their_values_end_at_one_x_when_wide() {
        let places = plan(false, settings::SHAPE_KEYS.len());
        let columns = |cell| {
            let found: Vec<i32> = places
                .iter()
                .filter(|p| p.cell == cell)
                .map(|p| p.column)
                .collect();
            assert_eq!(found.len(), settings::SHAPE_KEYS.len(), "{cell:?}");
            found
        };
        // The title column grows with the card, so every slider starts at
        // the same x, whatever each title's length.
        assert!(columns(Part::Slider).iter().all(|&c| c == 1));
        assert!(columns(Part::Title).iter().all(|&c| c == 0));
        assert!(columns(Part::Value).iter().all(|&c| c == 2));
        assert!(
            places
                .iter()
                .filter(|p| p.cell == Part::Slider)
                .all(|p| p.span == 1)
        );
    }

    #[test]
    fn narrow_the_slider_takes_the_row_under_its_title() {
        let places = plan(true, settings::SHAPE_KEYS.len());
        for place in places.iter().filter(|p| p.cell == Part::Slider) {
            assert_eq!((place.column, place.span), (0, 2));
            let title = places
                .iter()
                .find(|p| p.cell == Part::Title && p.index == place.index)
                .expect("a title");
            assert_eq!(place.row, title.row + 1, "under its title");
        }
        for place in places.iter().filter(|p| p.cell == Part::Value) {
            assert_eq!(place.column, 2, "the value at the end");
        }
    }

    #[test]
    fn the_picture_is_written_to_a_file_when_asked() {
        // Gated by EDEL_SETTINGS_PNG, the folder the pictures go in. Cairo
        // here has no PNG writer, so each picture is written as a PAM file
        // (RGB with alpha, un-premultiplied), which ImageMagick reads.
        let Some(dir) = std::env::var_os("EDEL_SETTINGS_PNG") else {
            return;
        };
        let (w, h) = (600, PICTURE_HEIGHT);
        let light = Tokens::built_in_scheme(Scheme::Light);
        let dark = Tokens::built_in_scheme(Scheme::Dark);
        let largest = [24, 64, 64, 10, 48, 100];
        for (name, t, values) in [
            ("settings-shape.pam", &light, values_of(&light)),
            ("settings-shape-largest.pam", &light, largest),
            ("settings-shape-dark.pam", &dark, values_of(&dark)),
        ] {
            let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, w, h)
                .expect("a surface");
            {
                let cr = Context::new(&surface).expect("a context");
                draw(&cr, f64::from(w), f64::from(h), t, &values);
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().expect("the pixels");
            let mut out = format!(
                "P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
            )
            .into_bytes();
            for y in 0..h as usize {
                for x in 0..w as usize {
                    // ARGB32 in memory is B, G, R, A per pixel, premultiplied.
                    let at = y * stride + x * 4;
                    let (b, g, r, a) = (data[at], data[at + 1], data[at + 2], data[at + 3]);
                    let unpremultiply = |c: u8| -> u8 {
                        if a == 0 {
                            0
                        } else {
                            (u32::from(c) * 255 / u32::from(a)).min(255) as u8
                        }
                    };
                    out.extend_from_slice(&[
                        unpremultiply(r),
                        unpremultiply(g),
                        unpremultiply(b),
                        a,
                    ]);
                }
            }
            drop(data);
            let path = Path::new(&dir).join(name);
            std::fs::write(&path, out).expect("a file");
        }
    }
}
