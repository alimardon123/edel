//! The Appearance page's Depth group (M5.12b3): how see-through the focused
//! window is and the others are (`appearance.window_opacity` and
//! `appearance.window_opacity_unfocused`, 20 to 100 percent) and the shadow
//! under the windows (`appearance.shadow_size`, its blur from 0 to 64 px,
//! and `appearance.shadow_darkness`, 0 to 100 percent), and the glass behind
//! see-through panels (`appearance.blur`, 0 to 100 percent). Five sliders over
//! their keys' ranges (`settings::whole_range`), built and laid out as the
//! Shape group's are (`shape::slider`, `shape::lay_out`), with their values
//! at the end. Each is one key of the person's settings file, read from the
//! value that applies now (the files over the release's tokens), and a drag
//! writes its key once it settles (250 ms). The release's value is taken
//! out, never written (`Files::choose_over`, ADR-008). Reset and Copy as
//! command are per row and for the group. `show` follows the files; asked
//! for by one of its keys, the group scrolls to the top and gives the
//! keyboard to that slider, and once two frames agree it logs where the
//! sliders lie for CI.

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use edel::i18n::{tr, trf};
use edel::settings::{self, Source};
use edel::tokens::{Shape as Keys, Tokens};

use crate::files::Files;
use crate::shape::{self, Slider};
use crate::{rows, widgets};

/// How long after the last move of a slider its value is written.
const SETTLE: Duration = Duration::from_millis(250);

/// The five sliders, in `settings::DEPTH_KEYS`' order.
const FOCUSED: usize = 0;
const UNFOCUSED: usize = 1;
const SIZE: usize = 2;
const DARKNESS: usize = 3;
const BLUR: usize = 4;

/// The five values a slider shows, in `settings::DEPTH_KEYS`' order.
pub type Shown = [u32; 5];

/// The release's five values, from the tokens (`Tokens::built_in()`); the
/// darkness is the shadow colour's alpha as a percent, rounded.
fn values_of(t: &Tokens) -> Shown {
    [
        t.window_opacity,
        t.window_opacity_unfocused,
        t.shadow_blur,
        (t.shadow.a * 100.0).round() as u32,
        t.blur,
    ]
}

/// The values that apply now, with the shape the files set on top of the
/// release.
fn now(files: &Files) -> Shown {
    let machine = shape::read_file(&files.machine);
    let person = files.person.as_deref().and_then(shape::read_file);
    let keys = Keys::between(machine.as_ref(), person.as_ref());
    values_of(&Tokens::built_in().with_shape(&keys).0)
}

/// What a slider's end shows for its value: `Opaque` for a window at 100
/// percent and `90 %` below it, `None` for no shadow and `24 px` for its
/// blur, `None` or `45 %` for its darkness, `Sharp` or `50 %` for the glass.
pub fn words(index: usize, value: u32) -> String {
    let number = value.to_string();
    match index {
        FOCUSED | UNFOCUSED if value == 100 => tr("Opaque").to_string(),
        FOCUSED | UNFOCUSED => trf("{n} %", &[("n", &number)]),
        SIZE if value == 0 => tr("None").to_string(),
        SIZE => trf("{n} px", &[("n", &number)]),
        DARKNESS if value == 0 => tr("None").to_string(),
        DARKNESS => trf("{n} %", &[("n", &number)]),
        BLUR if value == 0 => tr("Sharp").to_string(),
        BLUR => trf("{n} %", &[("n", &number)]),
        _ => unreachable!("a depth slider has one of five indices"),
    }
}

/// The line under a row's title: what the row does.
fn line(index: usize) -> &'static str {
    match index {
        FOCUSED => tr("The window you are using, with its title bar"),
        UNFOCUSED => tr("Every other window"),
        SIZE => tr("How far the shadow spreads. Full and Balanced effects only"),
        DARKNESS => tr("How dark the shadow is"),
        _ => tr("How much the wallpaper blurs behind see-through panels"),
    }
}

/// The Depth group on the Appearance page.
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    /// The page's line for a problem.
    problem: gtk::Label,
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    sliders: Rc<Vec<Slider>>,
    /// Set while `show` moves a slider, so that is not a choice.
    quiet: Cell<bool>,
}

impl Group {
    /// Shows what the files say now: each slider at its value (unless the
    /// person is dragging it), its value at the end, the line under it and
    /// its Reset, then the group's source line.
    pub fn show(&self) {
        let files = Files::here();
        let values = now(&files);
        self.quiet.set(true);
        for (index, slider) in self.sliders.iter().enumerate() {
            let key = settings::DEPTH_KEYS[index];
            if slider.pending.borrow().is_none() {
                slider.scale.set_value(f64::from(values[index]));
            }
            let value = slider.scale.value().round() as u32;
            slider.words.set_label(&words(index, value));
            let source = files.source(key);
            slider.row.reset.set_visible(rows::resettable(&source));
            let mut text = vec![line(index).to_string()];
            if let Some(note) = rows::note(&source, false) {
                text.push(note.to_string());
            }
            slider.row.subtitle.set_label(&text.join(" · "));
            slider.row.subtitle.set_visible(true);
        }
        self.quiet.set(false);
        self.source.label.set_label(&self.source_text(&files));
        self.source.reset.set_visible(
            settings::DEPTH_KEYS
                .iter()
                .any(|key| matches!(files.source(key), Source::Person(_))),
        );
    }

    /// The group's source line: your choice when the person sets any of the
    /// five, else this machine's, else the release's.
    fn source_text(&self, files: &Files) -> String {
        let sources: Vec<Source> = settings::DEPTH_KEYS
            .iter()
            .map(|key| files.source(key))
            .collect();
        if sources.iter().any(|s| matches!(s, Source::Person(_))) {
            tr("Your choice").to_string()
        } else if sources.iter().any(|s| matches!(s, Source::Machine(_))) {
            tr("Set by this machine").to_string()
        } else {
            rows::release_choice()
        }
    }

    /// The five values the sliders are at now.
    fn scale_values(&self) -> Shown {
        let mut values = [0u32; 5];
        for (value, slider) in values.iter_mut().zip(self.sliders.iter()) {
            *value = slider.scale.value().round() as u32;
        }
        values
    }

    /// A slider moved: its end follows at once, and its value is written
    /// once it settles, unless the group moved it.
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
                group.write(index);
            }
        }));
    }

    /// Writes slider `index`'s value to the person's file, or takes the key
    /// out when it is what applies without it, and logs what was done.
    fn write(&self, index: usize) {
        let key = settings::DEPTH_KEYS[index];
        let value = self.sliders[index].scale.value().round() as u32;
        let release = values_of(&Tokens::built_in())[index];
        match Files::here().choose_over(key, &value.to_string(), Some(&release.to_string())) {
            Ok(()) => {
                self.problem.set_visible(false);
                let written = matches!(Files::here().source(key), Source::Person(_));
                let what = if written { "written" } else { "taken out" };
                eprintln!("edel-settings: depth {key} {value} {what}");
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

/// Builds the Depth group in `content`, after the Shape group: the section
/// with its line and source line, Reset and Copy as command, then one card
/// of five sliders.
pub fn group(content: &gtk::Box, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Depth"));
    let card = widgets::group(content);
    widgets::note_row(
        &card,
        tr("How see-through windows are and the shadow under them"),
    );

    let sliders: Vec<Slider> = settings::DEPTH_KEYS
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
    // Shape group's do.
    let (g, s) = (grid.clone(), sliders.clone());
    let bin = widgets::folding(&grid, move |narrow| shape::lay_out(&g, &s, narrow));
    card.append(&bin);
    for slider in sliders.iter() {
        shape::reveal_copy(slider);
    }

    let scales: Vec<gtk::Scale> = sliders.iter().map(|s| s.scale.clone()).collect();
    let reset: gtk::Widget = source.reset.clone().upcast();
    let group = Rc::new_cyclic(|me| Group {
        me: me.clone(),
        problem: problem.clone(),
        source,
        sliders,
        quiet: Cell::new(false),
    });
    wire(&group);
    shape::log_places(&card, "depth", settings::DEPTH_KEYS, scales, Some(reset));
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
        let key = settings::DEPTH_KEYS[index];
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
        for key in settings::DEPTH_KEYS {
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
        let pairs: Vec<(&str, String)> = settings::DEPTH_KEYS
            .iter()
            .zip(values)
            .map(|(key, value)| (*key, value.to_string()))
            .collect();
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_values_read_as_words_at_their_ends_and_between() {
        assert_eq!(words(FOCUSED, 100), "Opaque");
        assert_eq!(words(FOCUSED, 90), "90 %");
        assert_eq!(words(FOCUSED, 20), "20 %");
        assert_eq!(words(UNFOCUSED, 100), "Opaque");
        assert_eq!(words(UNFOCUSED, 70), "70 %");
        assert_eq!(words(SIZE, 0), "None");
        assert_eq!(words(SIZE, 24), "24 px");
        assert_eq!(words(SIZE, 64), "64 px");
        assert_eq!(words(DARKNESS, 0), "None");
        assert_eq!(words(DARKNESS, 45), "45 %");
        assert_eq!(words(DARKNESS, 100), "100 %");
        assert_eq!(words(BLUR, 0), "Sharp");
        assert_eq!(words(BLUR, 50), "50 %");
        assert_eq!(words(BLUR, 100), "100 %");
    }

    #[test]
    fn the_sliders_take_their_ranges_from_the_key_table() {
        let expected = [(20, 100), (20, 100), (0, 64), (0, 100), (0, 100)];
        for (key, want) in settings::DEPTH_KEYS.iter().zip(expected) {
            assert_eq!(settings::whole_range(key), Some(want), "{key}");
        }
    }

    #[test]
    fn the_release_values_are_the_tokens_the_sliders_start_at() {
        assert_eq!(values_of(&Tokens::built_in()), [100, 100, 24, 45, 50]);
    }

    #[test]
    fn the_depth_rows_are_on_the_appearance_page() {
        for key in settings::DEPTH_KEYS {
            assert!(
                rows::on_page("appearance").any(|row| row.key == *key),
                "{key}"
            );
        }
    }
}
