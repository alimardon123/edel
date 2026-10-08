//! The Displays page (M5.7a): the screens side by side in a picture drawn
//! from the tokens, and for each screen its resolution, scale, position
//! and whether it is on. Each choice is one line of the person's settings
//! file, `displays.NAME.KEY`, which the compositor follows at once, and
//! each row says where its value comes from, with Reset for the person's
//! own choice and Copy as command (ADR-008).
//!
//! The screens are what the compositor reports in the state file and what
//! the files' `[displays]` add (`screens::list`), read again whenever
//! either changes; nothing here assumes how many screens there are or
//! that they are all plugged into this machine (ADR-011, M7.10d). A
//! screen's own default (the size it prefers, the scale the compositor
//! worked out from its size) is taken out of the file rather than written,
//! and the last screen that is on cannot be turned off.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};

use crate::files::Files;
use crate::screens::{self, Screen};
use crate::style::Theme;
use crate::{preview, rows, widgets};

const INTRO: &str = n_(
    "Each screen's size, scale and place. Changes apply at once. Each choice is also a line \
     of your settings file and an edel settings set command.",
);

/// The page's own state, so every change shows everywhere on it.
struct Ui {
    /// What the picture draws and the rows show.
    shown: Rc<RefCell<Vec<Screen>>>,
    picture: gtk::DrawingArea,
    /// Where each screen's rows go, below the picture.
    holder: gtk::Box,
    problem: gtk::Label,
    /// The rows now in `holder`, one set per screen.
    rows: RefCell<Vec<ScreenRows>>,
    /// What the rows were built from: they are built again only when it
    /// changes, as a screen appears, goes or gains a size.
    shape: RefCell<Vec<Shape>>,
    /// Set while the page itself moves a control, so that is not a choice.
    quiet: Cell<bool>,
    /// Kept so the page follows the files and the state while it is open.
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

/// What decides which rows a screen has.
#[derive(Clone, Debug, PartialEq)]
struct Shape {
    name: String,
    sizes: Vec<String>,
    scales: Vec<f64>,
    can_place: bool,
}

/// One screen's rows.
struct ScreenRows {
    name: String,
    detail: gtk::Label,
    resolution: Rc<widgets::Choice>,
    sizes: Vec<String>,
    scale: Rc<widgets::Segments>,
    scales: Vec<f64>,
    x: gtk::SpinButton,
    y: gtk::SpinButton,
    on: gtk::Switch,
    resolution_row: widgets::Row,
    scale_row: widgets::Row,
    position_row: widgets::Row,
    on_row: widgets::Row,
}

fn key(name: &str, field: &str) -> String {
    format!("displays.{name}.{field}")
}

fn shape_of(screen: &Screen) -> Shape {
    Shape {
        name: screen.name.clone(),
        sizes: screens::mode_choices(screen),
        scales: screens::scale_choices(screen.scale),
        can_place: screen.place.is_some(),
    }
}

pub fn page(theme: &Rc<Theme>) -> gtk::Widget {
    let (page, content, problem) = widgets::page(tr("Displays"), tr(INTRO));
    let shown = Rc::new(RefCell::new(Vec::new()));
    let picture = preview::arrangement(theme, shown.clone());
    content.append(&picture);
    let holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    content.append(&holder);
    let ui = Rc::new(Ui {
        shown,
        picture,
        holder,
        problem,
        rows: RefCell::new(Vec::new()),
        shape: RefCell::new(Vec::new()),
        quiet: Cell::new(false),
        monitors: RefCell::new(Vec::new()),
    });
    ui.update();
    let files = Files::here();
    let mut monitors = Vec::new();
    for path in std::iter::once(&files.machine)
        .chain(files.person.as_ref())
        .map(|p| p.as_path())
        .chain(std::iter::once(std::path::Path::new(
            edel::places::STATE_FILE,
        )))
    {
        let file = gio::File::for_path(path);
        if let Ok(monitor) =
            file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            let weak = Rc::downgrade(&ui);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(ui) = weak.upgrade() {
                    ui.update();
                }
            });
            monitors.push(monitor);
        }
    }
    *ui.monitors.borrow_mut() = monitors;
    // The page's state lives as long as the page does.
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

impl Ui {
    /// Reads the files and the state and shows them: the picture, and the
    /// rows, built again only when the screens or their choices changed.
    fn update(self: &Rc<Self>) {
        let files = Files::here();
        let state = std::fs::read_to_string(edel::places::STATE_FILE).ok();
        let list = screens::list(state.as_deref(), &files.displays());
        let shape: Vec<Shape> = list.iter().map(shape_of).collect();
        let rebuild = *self.shape.borrow() != shape;
        if rebuild || *self.shown.borrow() != list {
            *self.shown.borrow_mut() = list.clone();
            self.picture.queue_draw();
        }
        if rebuild {
            *self.shape.borrow_mut() = shape;
            self.build(&list);
        }
        self.quiet.set(true);
        for (rows, screen) in self.rows.borrow().iter().zip(&list) {
            show(rows, screen, &files);
        }
        self.quiet.set(false);
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

    /// The rows of every screen in `list`, in place of those before.
    fn build(self: &Rc<Self>, list: &[Screen]) {
        while let Some(child) = self.holder.first_child() {
            self.holder.remove(&child);
        }
        let mut all = Vec::new();
        if list.is_empty() {
            widgets::text_row(&widgets::group(&self.holder), tr(NONE));
        }
        for screen in list {
            all.push(self.screen_rows(screen));
        }
        *self.rows.borrow_mut() = all;
    }

    fn screen_rows(self: &Rc<Self>, screen: &Screen) -> ScreenRows {
        let name = screen.name.clone();
        let heading = widgets::heading(&self.holder, &name);
        let detail = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["edel-source"])
            .build();
        heading.append(&detail);
        let group = widgets::group(&self.holder);

        // Scale first: it is what most people come here to change.
        let scales = screens::scale_choices(screen.scale);
        let scale = widgets::Segments::new(
            &scales
                .iter()
                .map(|s| screens::percent(*s))
                .collect::<Vec<_>>(),
        );
        let scale_row = widgets::row(&group, rows::title("displays.*.scale"), &scale.widget());

        let sizes = screens::mode_choices(screen);
        let labels: Vec<String> = if sizes.is_empty() {
            vec![tr("Automatic").to_string()]
        } else {
            sizes.clone()
        };
        let resolution = widgets::Choice::new(labels);
        let resolution_row = widgets::row(
            &group,
            rows::title("displays.*.resolution"),
            &resolution.widget(),
        );

        let spin = |label: &str| {
            let spin = gtk::SpinButton::with_range(-16384.0, 16384.0, 10.0);
            spin.set_css_classes(&["edel-spin"]);
            spin.set_width_chars(6);
            spin.update_property(&[gtk::accessible::Property::Label(label)]);
            spin
        };
        let (x, y) = (spin(tr("Across")), spin(tr("Down")));
        let place = gtk::Box::builder().spacing(6).build();
        for (text, spin) in [("X", &x), ("Y", &y)] {
            place.append(
                &gtk::Label::builder()
                    .label(text)
                    .css_classes(["edel-source"])
                    .build(),
            );
            place.append(spin);
        }
        let position_row = widgets::row(
            &group,
            rows::title("displays.*.position"),
            &place.upcast::<gtk::Widget>(),
        );

        let on = gtk::Switch::new();
        let on_row = widgets::row(
            &group,
            rows::title("displays.*.enabled"),
            &on.clone().upcast::<gtk::Widget>(),
        );

        let rows = ScreenRows {
            name: name.clone(),
            detail,
            resolution,
            sizes,
            scale,
            scales,
            x,
            y,
            on,
            resolution_row,
            scale_row,
            position_row,
            on_row,
        };
        self.wire(&rows);
        rows
    }

    /// What to call when a control of `name`'s changes: `f` writes the
    /// choice, any refusal is shown, and the page shows the files again.
    fn on_change(
        self: &Rc<Self>,
        name: &str,
        f: impl Fn(&str, &Screen, &Files) -> Result<(), String> + 'static,
    ) -> impl Fn() + 'static {
        let (ui, name) = (Rc::downgrade(self), name.to_string());
        move || {
            let Some(ui) = ui.upgrade() else { return };
            if ui.quiet.get() {
                return;
            }
            let screen = ui.shown.borrow().iter().find(|s| s.name == name).cloned();
            if let Some(screen) = screen {
                ui.report(f(&name, &screen, &Files::here()));
            }
            ui.update();
        }
    }

    /// A change in `rs`' controls is one line of the person's file; its
    /// Reset takes the line out and its Copy gives the command.
    fn wire(self: &Rc<Self>, rs: &ScreenRows) {
        let (sizes, resolution) = (rs.sizes.clone(), rs.resolution.clone());
        rs.resolution
            .connect_changed(self.on_change(&rs.name, move |name, screen, files| {
                let Some(size) = sizes.get(resolution.selected()) else {
                    return Ok(());
                };
                files.choose_over(&key(name, "resolution"), size, screen.preferred.as_deref())
            }));

        let (scales, scale) = (rs.scales.clone(), rs.scale.clone());
        rs.scale
            .connect_changed(self.on_change(&rs.name, move |name, screen, files| {
                let Some(picked) = scales.get(scale.selected()) else {
                    return Ok(());
                };
                files.choose_over(
                    &key(name, "scale"),
                    &screens::scale_value(*picked),
                    screen.auto_scale.map(screens::scale_value).as_deref(),
                )
            }));

        for spin in [&rs.x, &rs.y] {
            let (x, y) = (rs.x.clone(), rs.y.clone());
            let changed = self.on_change(&rs.name, move |name, _, files| {
                let pair = format!("[{}, {}]", x.value() as i64, y.value() as i64);
                files.set(&key(name, "position"), Some(&pair))
            });
            spin.connect_value_changed(move |_| changed());
        }

        let on = rs.on.clone();
        let changed = self.on_change(&rs.name, move |name, screen, files| {
            let wanted = on.is_active();
            let state = std::fs::read_to_string(edel::places::STATE_FILE).ok();
            let list = screens::list(state.as_deref(), &files.displays());
            if !wanted && screen.on && screens::on_count(&list) <= 1 {
                return Err(trf(
                    "{name} cannot be turned off: it is the only screen that is on. \
                         Turn another screen on first.",
                    &[("name", name)],
                ));
            }
            files.choose_over(&key(name, "enabled"), &wanted.to_string(), Some("true"))
        });
        rs.on.connect_active_notify(move |_| changed());

        // Reset and Copy as command, one pair for each row.
        let fields = [
            ("resolution", &rs.resolution_row),
            ("scale", &rs.scale_row),
            ("position", &rs.position_row),
            ("enabled", &rs.on_row),
        ];
        for (field, row) in fields {
            let (ui, name) = (Rc::downgrade(self), rs.name.clone());
            row.reset.connect_clicked(move |_| {
                if let Some(ui) = ui.upgrade() {
                    ui.report(Files::here().set(&key(&name, field), None));
                    ui.update();
                }
            });
            let (ui, name) = (Rc::downgrade(self), rs.name.clone());
            row.copy.connect_clicked(move |button| {
                let Some(ui) = ui.upgrade() else { return };
                let screens = ui.shown.borrow();
                let Some(screen) = screens.iter().find(|s| s.name == name) else {
                    return;
                };
                let value = current(screen, field);
                button
                    .clipboard()
                    .set_text(&rows::command(&[(&key(&name, field), value)]));
                widgets::copied(button);
            });
        }
    }
}

const NONE: &str = n_(
    "No screen is reported yet. Screens show here once the desktop is running, \
     or once your settings file names them under [displays].",
);

/// What `field` of `screen` is now, as `edel settings set` takes it.
fn current(screen: &Screen, field: &str) -> String {
    match field {
        "resolution" => screen.mode.clone().unwrap_or_default(),
        "scale" => screens::scale_value(screen.scale),
        "position" => {
            let place = screen.place.unwrap_or(screens::Place {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            });
            format!("[{}, {}]", place.x, place.y)
        }
        _ => screen.on.to_string(),
    }
}

/// Shows `screen` in `rows`: each control's value, where each comes from,
/// and what can be changed.
fn show(rows: &ScreenRows, screen: &Screen, files: &Files) {
    let place = screen.place;
    rows.detail.set_label(&if !screen.on {
        tr("Off").to_string()
    } else if screen.lit {
        place.map_or_else(String::new, |p| {
            trf(
                "{width} x {height} in the layout",
                &[("width", &p.w.to_string()), ("height", &p.h.to_string())],
            )
        })
    } else {
        tr("Not connected or not reported").to_string()
    });

    let at = screen
        .mode
        .as_ref()
        .and_then(|m| rows.sizes.iter().position(|s| s == m))
        .unwrap_or(0);
    rows.resolution.set_selected(at);
    rows.resolution
        .widget()
        .set_sensitive(!rows.sizes.is_empty());
    let at = rows
        .scales
        .iter()
        .position(|s| screens::same(*s, screen.scale))
        .unwrap_or(0);
    rows.scale.set_selected(at);
    let (px, py) = place.map_or((0, 0), |p| (p.x, p.y));
    rows.x.set_value(px as f64);
    rows.y.set_value(py as f64);
    for spin in [&rows.x, &rows.y] {
        spin.set_sensitive(place.is_some());
    }
    rows.on.set_active(screen.on);

    let line = |what: String, field: &str| {
        let source = files.source(&key(&screen.name, field));
        match (what.as_str(), rows::note(&source, false)) {
            (what, Some(note)) if !what.is_empty() => format!("{what} · {note}"),
            ("", Some(note)) => note.to_string(),
            (what, _) => what.to_string(),
        }
    };
    let set = |row: &widgets::Row, field: &str, what: String| {
        let text = line(what, field);
        row.subtitle.set_label(&text);
        row.subtitle.set_visible(!text.is_empty());
        let source = files.source(&key(&screen.name, field));
        row.reset.set_visible(rows::resettable(&source));
    };
    set(
        &rows.resolution_row,
        "resolution",
        match &screen.preferred {
            Some(own) => trf("The screen's own is {size}", &[("size", own)]),
            None => String::new(),
        },
    );
    set(
        &rows.scale_row,
        "scale",
        screen
            .mode
            .as_deref()
            .and_then(|m| screens::logical(m, screen.scale))
            .map_or_else(String::new, |(w, h)| {
                trf(
                    "Looks like {width} x {height}",
                    &[("width", &w.to_string()), ("height", &h.to_string())],
                )
            }),
    );
    set(
        &rows.position_row,
        "position",
        tr("Where this screen's top left corner lies in the layout").to_string(),
    );
    set(
        &rows.on_row,
        "enabled",
        tr("A screen that is off stays dark").to_string(),
    );
}
