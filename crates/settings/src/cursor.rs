//! The Appearance page's Cursor and icons group (M5.12b5 part A): the
//! cursor's size (`appearance.cursor_size`, 16 to 96 logical pixels), its
//! theme (`appearance.cursor_theme`, one of the xcursor themes this system
//! has, `default` first, as a `widgets::Choice`), the apps' icon theme
//! (`appearance.icon_theme`, one of the icon themes this system has, `Edel`
//! first, M8.21 part g) and the icons' size (`appearance.icon_size`, 75 to
//! 150 percent). Both theme rows are one `Theme` each, built alike. The two
//! sliders are built and laid out as the Depth group's are (`shape::slider`, `shape::lay_out`),
//! their values at the end. Each is one key of the person's settings file,
//! read from the value that applies now, and a drag writes its key once it
//! settles (250 ms). The release's value is taken out, never written
//! (`Files::choose_over`, ADR-008). Reset and Copy as command are per row and
//! for the group. `show` follows the files; asked for by one of its keys, the
//! group scrolls to the top and gives the keyboard to that control, and once
//! two frames agree it logs where the controls lie for CI.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use edel::i18n::{n_, tr, trf};
use edel::settings::{self, Source};

use crate::files::Files;
use crate::shape::{self, Slider};
use crate::{rows, widgets};

/// How long after the last move of a slider its value is written.
const SETTLE: Duration = Duration::from_millis(250);

/// The two sliders, in [`KEYS`]' order.
const CURSOR: usize = 0;
const ICONS: usize = 1;

/// The two theme rows, in [`Group::themes`]' order.
const CURSOR_THEME_ROW: usize = 0;
const ICON_THEME_ROW: usize = 1;

/// The keys of the group in the order the places log names them: the two
/// sliders, then the two themes.
const KEYS: &[&str] = &[
    settings::CURSOR_SIZE,
    settings::ICON_SIZE,
    settings::CURSOR_THEME,
    settings::ICON_THEME,
];

/// The release's values of the two sliders, as the files take them out.
const RELEASE: [u32; 2] = [settings::CURSOR_SIZE_DEFAULT, settings::ICON_SIZE_DEFAULT];

/// What a slider's end shows for its value: `24 px` for the cursor, and
/// `As made` at 100 percent for the icons, `75 %` and `150 %` at the ends.
pub fn words(index: usize, value: u32) -> String {
    let number = value.to_string();
    match index {
        CURSOR => trf("{n} px", &[("n", &number)]),
        _ if value == settings::ICON_SIZE_DEFAULT => tr("As made").to_string(),
        _ => trf("{n} %", &[("n", &number)]),
    }
}

/// The line under a slider's title: what the row does.
fn line(index: usize) -> &'static str {
    match index {
        CURSOR => tr("The pointer's height. Apps already open keep theirs"),
        _ => tr("The panel's icons and the app icons in the bar and the dock"),
    }
}

/// A theme's row: its choice of the themes this system has for one key, and
/// the row's parts. The cursor's and the icons' rows are two of these.
struct Theme {
    /// The key the row writes, `appearance.cursor_theme` or
    /// `appearance.icon_theme`.
    key: &'static str,
    /// The release's theme, which is taken out of the file when chosen.
    release: &'static str,
    /// The line under the row's title, which says what the theme changes.
    line: &'static str,
    choice: Rc<widgets::Choice>,
    /// The themes the choice lists, in its order; the release's first.
    names: Vec<String>,
    row: widgets::Row,
}

impl Theme {
    /// Shows what the files say now: the choice, the row's Reset and its line
    /// with where the value comes from.
    fn show(&self, files: &Files) {
        let chosen = files
            .chosen(self.key)
            .unwrap_or_else(|| self.release.to_string());
        let at = self
            .names
            .iter()
            .position(|name| *name == chosen)
            .unwrap_or(0);
        if self.choice.selected() != at {
            self.choice.set_selected(at);
        }
        let source = files.source(self.key);
        self.row.reset.set_visible(rows::resettable(&source));
        let mut text = vec![tr(self.line).to_string()];
        if let Some(note) = rows::note(&source, false) {
            text.push(note.to_string());
        }
        self.row.subtitle.set_label(&text.join(" · "));
    }

    /// The theme the choice shows now.
    fn name(&self) -> String {
        self.names
            .get(self.choice.selected())
            .cloned()
            .unwrap_or_else(|| self.release.to_string())
    }
}

/// The Cursor and icons group on the Appearance page.
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    /// The page's line for a problem.
    problem: gtk::Label,
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    sliders: Rc<Vec<Slider>>,
    /// The cursor's and the icons' theme rows, in that order.
    themes: Vec<Theme>,
    /// Set while `show` moves a control, so that is not a choice.
    quiet: Cell<bool>,
}

impl Group {
    /// Shows what the files say now: each slider at its value (unless the
    /// person is dragging it), its value at the end, the line under it and
    /// its Reset, the two themes, then the group's source line.
    pub fn show(&self) {
        let files = Files::here();
        self.quiet.set(true);
        for (index, slider) in self.sliders.iter().enumerate() {
            let key = KEYS[index];
            let value = files
                .chosen(key)
                .and_then(|value| value.parse().ok())
                .unwrap_or(RELEASE[index]);
            if slider.pending.borrow().is_none() {
                slider.scale.set_value(f64::from(value));
            }
            let shown = slider.scale.value().round() as u32;
            slider.words.set_label(&words(index, shown));
            let source = files.source(key);
            slider.row.reset.set_visible(rows::resettable(&source));
            let mut text = vec![line(index).to_string()];
            if let Some(note) = rows::note(&source, false) {
                text.push(note.to_string());
            }
            slider.row.subtitle.set_label(&text.join(" · "));
            slider.row.subtitle.set_visible(true);
        }
        for theme in &self.themes {
            theme.show(&files);
        }
        self.quiet.set(false);
        self.source.label.set_label(&self.source_text(&files));
        self.source.reset.set_visible(
            KEYS.iter()
                .any(|key| matches!(files.source(key), Source::Person(_))),
        );
    }

    /// The group's source line: your choice when the person sets any of the
    /// three, else this machine's, else the release's.
    fn source_text(&self, files: &Files) -> String {
        let sources: Vec<Source> = KEYS.iter().map(|key| files.source(key)).collect();
        if sources.iter().any(|s| matches!(s, Source::Person(_))) {
            tr("Your choice").to_string()
        } else if sources.iter().any(|s| matches!(s, Source::Machine(_))) {
            tr("Set by this machine").to_string()
        } else {
            trf("Automatic ({current})", &[("current", tr("the release"))])
        }
    }

    /// A slider moved: its end follows at once, and its value is written once
    /// it settles, unless the group moved it.
    fn moved(&self, index: usize, value: u32) {
        self.sliders[index].words.set_label(&words(index, value));
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
                group.write_slider(index);
            }
        }));
    }

    /// Writes slider `index`'s value, or takes the key out when it is the
    /// release's.
    fn write_slider(&self, index: usize) {
        let value = self.sliders[index].scale.value().round() as u32;
        self.write(KEYS[index], &value.to_string(), &RELEASE[index].to_string());
    }

    /// Theme row `index` was picked: it is written, or taken out when it is
    /// the release's.
    fn picked(&self, index: usize) {
        if self.quiet.get() {
            return;
        }
        let theme = &self.themes[index];
        self.write(theme.key, &theme.name(), theme.release);
    }

    /// Chooses `value` for `key` against the release's `release`, logs what
    /// was done for CI and shows the result; a refusal is in the problem line.
    fn write(&self, key: &str, value: &str, release: &str) {
        match Files::here().choose_over(key, value, Some(release)) {
            Ok(()) => {
                self.problem.set_visible(false);
                let written = matches!(Files::here().source(key), Source::Person(_));
                let what = if written { "written" } else { "taken out" };
                eprintln!("edel-settings: cursor {key} {value} {what}");
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

/// Builds one theme row in `card`: its choice of `names`, under `key`'s title.
fn theme_row(
    card: &gtk::Box,
    key: &'static str,
    release: &'static str,
    line: &'static str,
    names: Vec<String>,
) -> Theme {
    let choice = widgets::Choice::new(names.clone());
    let row = widgets::row(card, rows::title(key), &choice.widget());
    Theme {
        key,
        release,
        line,
        choice,
        names,
        row,
    }
}

/// Builds the Cursor and icons group in `content`, after the Text group: the
/// section with its line and source line, Reset and Copy as command, then one
/// card with the two themes' rows and the two sliders.
pub fn group(content: &gtk::Box, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Cursor and icons"));
    let card = widgets::group(content);
    widgets::note_row(
        &card,
        tr("The pointer and the icons. Apps already open keep their cursor until they start again"),
    );

    let cursor = theme_row(
        &card,
        settings::CURSOR_THEME,
        settings::CURSOR_THEME_DEFAULT,
        n_("The pointer's look in every app that starts after this"),
        settings::cursor_themes(),
    );
    let icons = theme_row(
        &card,
        settings::ICON_THEME,
        settings::ICON_THEME_DEFAULT,
        n_("The apps' icons in the panel, the dock, the launcher and the title bars"),
        settings::icon_themes(),
    );

    let sliders: Vec<Slider> = [settings::CURSOR_SIZE, settings::ICON_SIZE]
        .iter()
        .map(|key| shape::slider(key))
        .collect();
    let sliders = Rc::new(sliders);
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .hexpand(true)
        .build();
    shape::lay_out(&grid, &sliders, false);
    // The page's narrow width folds each slider under its title, as the
    // Depth group's do.
    let (g, s) = (grid.clone(), sliders.clone());
    let bin = widgets::folding(&grid, move |narrow| shape::lay_out(&g, &s, narrow));
    card.append(&bin);
    for slider in sliders.iter() {
        shape::reveal_copy(slider);
    }

    let mut controls: Vec<gtk::Widget> = sliders.iter().map(|s| s.scale.clone().upcast()).collect();
    controls.extend([cursor.choice.widget(), icons.choice.widget()]);
    let reset: gtk::Widget = source.reset.clone().upcast();
    let group = Rc::new_cyclic(|me| Group {
        me: me.clone(),
        problem: problem.clone(),
        source,
        sliders,
        themes: vec![cursor, icons],
        quiet: Cell::new(false),
    });
    wire(&group);
    shape::log_places(&card, "cursor", KEYS, controls, Some(reset));
    group.show();
    group
}

/// Connects each slider's moves, the theme's picks, each row's Reset and Copy
/// as command and the group's Reset and Copy as command.
fn wire(group: &Rc<Group>) {
    for (index, slider) in group.sliders.iter().enumerate() {
        let weak = Rc::downgrade(group);
        slider.scale.connect_value_changed(move |scale| {
            if let Some(group) = weak.upgrade() {
                group.moved(index, scale.value().round() as u32);
            }
        });
        let weak = Rc::downgrade(group);
        let key = KEYS[index];
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
    for (index, theme) in group.themes.iter().enumerate() {
        let weak = Rc::downgrade(group);
        theme.choice.connect_changed(move || {
            if let Some(group) = weak.upgrade() {
                group.picked(index);
            }
        });
        let weak = Rc::downgrade(group);
        let key = theme.key;
        theme.row.reset.connect_clicked(move |_| {
            if let Some(group) = weak.upgrade() {
                group.set(key, None);
            }
        });
        let weak = Rc::downgrade(group);
        theme.row.copy.connect_clicked(move |button| {
            let Some(group) = weak.upgrade() else {
                return;
            };
            let name = group.themes[index].name();
            button.clipboard().set_text(&rows::command(&[(key, name)]));
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
        for key in KEYS {
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
        let pairs = [
            (
                settings::CURSOR_SIZE,
                (group.sliders[CURSOR].scale.value().round() as u32).to_string(),
            ),
            (
                settings::ICON_SIZE,
                (group.sliders[ICONS].scale.value().round() as u32).to_string(),
            ),
            (
                settings::CURSOR_THEME,
                group.themes[CURSOR_THEME_ROW].name(),
            ),
            (settings::ICON_THEME, group.themes[ICON_THEME_ROW].name()),
        ];
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_read_as_pixels_and_as_made_at_the_icons_default() {
        assert_eq!(words(CURSOR, 24), "24 px");
        assert_eq!(words(CURSOR, 96), "96 px");
        assert_eq!(words(ICONS, 100), "As made");
        assert_eq!(words(ICONS, 75), "75 %");
        assert_eq!(words(ICONS, 150), "150 %");
    }

    #[test]
    fn the_sliders_take_their_ranges_and_the_keys_are_rows() {
        assert_eq!(settings::whole_range(settings::CURSOR_SIZE), Some((16, 96)));
        assert_eq!(settings::whole_range(settings::ICON_SIZE), Some((75, 150)));
        for key in KEYS {
            assert!(
                rows::ROWS.iter().any(|row| row.key == *key),
                "{key} has no row"
            );
        }
    }

    #[test]
    fn the_release_values_are_the_defaults_the_key_table_gives() {
        assert_eq!(RELEASE, [24, 100]);
        assert_eq!(settings::CURSOR_THEME_DEFAULT, "default");
        assert_eq!(settings::ICON_THEME_DEFAULT, "Edel");
    }
}
