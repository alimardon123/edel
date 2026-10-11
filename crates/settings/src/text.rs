//! The Appearance page's Text group (M5.12b5): the interface font, the
//! monospace font and the text size, each one key of the person's settings
//! file (`appearance.font`, `appearance.monospace_font` and
//! `appearance.text_size`). The two fonts are drop-downs of the families this
//! system has (pango's list, sorted, the release's first and marked
//! "(default)"), each name drawn in its own face; the size is a slider from
//! 80 to 150 percent, shown as "Default" at 100. A sample sentence under the
//! rows shows the chosen font at the chosen size and is drawn again on every
//! change. Each choice is written with `Files::choose_over`, so the release's
//! value is taken out, never written (ADR-008), and a drag of the slider
//! writes once it settles, as the Shape group's sliders do. Asked for by one
//! of its keys, the page scrolls the group to the top and gives the keyboard
//! to that control; once two frames agree it logs where the controls lie for
//! CI.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use gtk::pango;

use edel::i18n::{tr, trf};
use edel::settings::{self, Source};
use edel::tokens::{MONOSPACE, Tokens};

use crate::files::Files;
use crate::{rows, widgets};

/// How long a slider waits for the last move before it writes, as the
/// Shape group's sliders do.
const SETTLE: Duration = Duration::from_millis(250);

/// The width of the size slider, and of the words that show its value.
const SLIDER: i32 = 220;
const VALUE: i32 = 96;

/// The sentence the sample shows, in the chosen font at the chosen size.
const SAMPLE: &str = "The quick brown fox jumps over the lazy dog";

/// The release's text size in percent, which the size slider shows as
/// "Default" and takes out of the file.
const DEFAULT_PERCENT: u32 = 100;

/// One font row: its key, the release's family, the families its drop-down
/// lists (the model's strings) and the row's parts.
struct Family {
    key: &'static str,
    release: String,
    model: gtk::StringList,
    dropdown: gtk::DropDown,
    row: widgets::Row,
}

/// The size row: its slider, the words at its end and the write it waits
/// for while a drag goes on.
struct Size {
    scale: gtk::Scale,
    words: gtk::Label,
    row: widgets::Row,
    pending: RefCell<Option<glib::SourceId>>,
}

/// The Text group on the Appearance page.
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    /// The page's line for a problem.
    problem: gtk::Label,
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    interface: Family,
    monospace: Family,
    size: Size,
    /// The sample sentence, drawn again on every change.
    sample: gtk::Label,
    /// Set while `show` moves a control, so that is not a choice.
    quiet: Cell<bool>,
}

/// The release's interface font, from the tokens.
fn interface_release() -> String {
    Tokens::built_in().font
}

/// The release's monospace font.
fn monospace_release() -> String {
    MONOSPACE.to_string()
}

/// The families a drop-down lists: the release's first, then the others
/// this system has, sorted and without repeats; only monospace ones when
/// `monospace` is set.
fn faces(widget: &gtk::Widget, release: &str, monospace: bool) -> Vec<String> {
    let mut names: Vec<String> = widget
        .pango_context()
        .list_families()
        .iter()
        .filter(|family| !monospace || family.is_monospace())
        .map(|family| family.name().to_string())
        .filter(|name| name != release)
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names.insert(0, release.to_string());
    names
}

/// The attributes that draw a name in its own face.
fn face(family: &str) -> pango::AttrList {
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrString::new_family(family));
    attrs
}

/// The words the size slider shows for `value`: "Default" at 100, else the
/// percent.
pub fn words(value: u32) -> String {
    if value == DEFAULT_PERCENT {
        tr("Default").to_string()
    } else {
        trf("{n} %", &[("n", &value.to_string())])
    }
}

/// The index of `name` in `model`, appended first when the list lacks it
/// (a family the file names that this system does not list).
fn index_of(model: &gtk::StringList, name: &str) -> u32 {
    (0..model.n_items())
        .find(|i| model.string(*i).is_some_and(|s| s == name))
        .unwrap_or_else(|| {
            model.append(name);
            model.n_items() - 1
        })
}

/// The name a drop-down's model holds at `index`.
fn name_at(model: &gtk::StringList, index: u32) -> Option<String> {
    model.string(index).map(|s| s.to_string())
}

/// Builds one font row in `card`: a drop-down of `faces`, each name drawn
/// in its own face, marked "(default)" for the release's.
fn family(
    card: &gtk::Box,
    widget: &gtk::Widget,
    key: &'static str,
    title: &str,
    release: String,
    monospace: bool,
) -> Family {
    let names = faces(widget, &release, monospace);
    let model = gtk::StringList::new(&[]);
    for name in &names {
        model.append(name);
    }
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(&gtk::Label::builder().xalign(0.0).build()));
        }
    });
    let default = release.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = item.child().and_downcast::<gtk::Label>();
        let string = item.item().and_downcast::<gtk::StringObject>();
        let (Some(label), Some(string)) = (label, string) else {
            return;
        };
        let name = string.string().to_string();
        let shown = if name == default {
            trf("{name} (default)", &[("name", &name)])
        } else {
            name.clone()
        };
        label.set_label(&shown);
        label.set_attributes(Some(&face(&name)));
    });
    let dropdown = gtk::DropDown::builder()
        .model(&model)
        .enable_search(true)
        .valign(gtk::Align::Center)
        .build();
    dropdown.set_factory(Some(&factory));
    dropdown.set_list_factory(Some(&factory));
    dropdown.update_property(&[gtk::accessible::Property::Label(title)]);
    let row = widgets::row(card, title, dropdown.upcast_ref());
    Family {
        key,
        release,
        model,
        dropdown,
        row,
    }
}

/// Builds the Text group in `content`: the section with its source line,
/// Reset and Copy as command, the sample, then the three rows.
pub fn group(content: &gtk::Box, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Text"));
    let card = widgets::group(content);

    // The sample first, so a person sees the font and size at once.
    let sample = gtk::Label::builder()
        .label(SAMPLE)
        .xalign(0.0)
        .wrap(true)
        .build();
    sample.update_property(&[gtk::accessible::Property::Label(tr(
        "A sample of the text in the chosen font and size",
    ))]);
    let sample_row = gtk::Box::builder()
        .css_classes(["edel-row", "edel-preview-row"])
        .build();
    sample_row.append(&sample);
    sample_row.set_can_target(false);
    card.append(&sample_row);

    let interface = family(
        &card,
        content.upcast_ref(),
        settings::FONT,
        tr("Interface font"),
        interface_release(),
        false,
    );
    let monospace = family(
        &card,
        content.upcast_ref(),
        settings::MONOSPACE_FONT,
        tr("Monospace font"),
        monospace_release(),
        true,
    );

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 80.0, 150.0, 1.0);
    scale.set_digits(0);
    scale.set_draw_value(false);
    scale.set_valign(gtk::Align::Center);
    scale.set_size_request(SLIDER, -1);
    let title = tr("Text size");
    scale.update_property(&[gtk::accessible::Property::Label(title)]);
    let words = gtk::Label::builder()
        .width_request(VALUE)
        .xalign(1.0)
        .valign(gtk::Align::Center)
        .css_classes(["edel-value"])
        .build();
    let control = gtk::Box::builder().spacing(10).build();
    control.append(&scale);
    control.append(&words);
    let row = widgets::row(&card, title, control.upcast_ref());

    let group = Rc::new_cyclic(|me| Group {
        me: me.clone(),
        problem: problem.clone(),
        source,
        interface,
        monospace,
        size: Size {
            scale: scale.clone(),
            words,
            row,
            pending: RefCell::new(None),
        },
        sample,
        quiet: Cell::new(false),
    });
    wire(&group);
    log_places(
        &card,
        [
            group.interface.dropdown.clone().upcast(),
            group.monospace.dropdown.clone().upcast(),
            scale.upcast(),
        ],
    );
    group.show();
    group
}

impl Group {
    /// Shows what the files say now: each control at its value, the value's
    /// words, each row's line and Reset, the group's source line and the
    /// sample.
    pub fn show(&self) {
        let files = Files::here();
        self.quiet.set(true);
        self.show_family(&self.interface, &files);
        self.show_family(&self.monospace, &files);
        let percent = self.size_value(&files);
        if self.size.pending.borrow().is_none() {
            self.size.scale.set_value(f64::from(percent));
        }
        self.size.words.set_label(&words(percent));
        let source = files.source(settings::TEXT_SIZE);
        self.size.row.reset.set_visible(rows::resettable(&source));
        self.size.row.subtitle.set_label(&note_line(&source));
        self.size.row.subtitle.set_visible(true);
        self.quiet.set(false);
        self.sample_now();
        self.source.label.set_label(&self.source_text(&files));
        self.source.reset.set_visible(
            [
                settings::FONT,
                settings::MONOSPACE_FONT,
                settings::TEXT_SIZE,
            ]
            .iter()
            .any(|key| matches!(files.source(key), Source::Person(_))),
        );
    }

    /// One font row: the family the files choose (else the release's) on
    /// its drop-down, its Reset and its line.
    fn show_family(&self, family: &Family, files: &Files) {
        let chosen = files
            .chosen(family.key)
            .unwrap_or_else(|| family.release.clone());
        let index = index_of(&family.model, &chosen);
        if family.dropdown.selected() != index {
            family.dropdown.set_selected(index);
        }
        let source = files.source(family.key);
        family.row.reset.set_visible(rows::resettable(&source));
        family.row.subtitle.set_label(&note_line(&source));
        family.row.subtitle.set_visible(true);
    }

    /// The text size the files choose, else the release's 100.
    fn size_value(&self, files: &Files) -> u32 {
        files
            .chosen(settings::TEXT_SIZE)
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_PERCENT)
    }

    /// The group's source line: your choice when the person sets any of the
    /// three, else this machine's, else the release's.
    fn source_text(&self, files: &Files) -> String {
        let sources = [
            settings::FONT,
            settings::MONOSPACE_FONT,
            settings::TEXT_SIZE,
        ]
        .map(|key| files.source(key));
        if sources.iter().any(|s| matches!(s, Source::Person(_))) {
            tr("Your choice").to_string()
        } else if sources.iter().any(|s| matches!(s, Source::Machine(_))) {
            tr("Set by this machine").to_string()
        } else {
            trf("Automatic ({current})", &[("current", tr("the release"))])
        }
    }

    /// The interface font the drop-down shows now.
    fn interface_name(&self) -> Option<String> {
        name_at(&self.interface.model, self.interface.dropdown.selected())
    }

    /// The monospace font the drop-down shows now.
    fn monospace_name(&self) -> Option<String> {
        name_at(&self.monospace.model, self.monospace.dropdown.selected())
    }

    /// Draws the sample in the font and size the controls show now.
    fn sample_now(&self) {
        let family = self.interface_name().unwrap_or_else(interface_release);
        let percent = self.size.scale.value().round() as u32;
        // The release's 13 px text at the chosen percent, in Pango's units.
        let px = f64::from(Tokens::built_in().text_size) * f64::from(percent) / 100.0;
        let attrs = face(&family);
        attrs.insert(pango::AttrSize::new_size_absolute(
            (px * f64::from(pango::SCALE)).round() as i32,
        ));
        self.sample.set_attributes(Some(&attrs));
    }

    /// A family drop-down moved: its line and the sample follow, and the
    /// value is written at once, unless the page moved it.
    fn picked(&self, family: &Family) {
        if self.quiet.get() {
            return;
        }
        let Some(name) = name_at(&family.model, family.dropdown.selected()) else {
            return;
        };
        self.sample_now();
        self.write(family.key, &name, &family.release);
    }

    /// The size slider moved: its words and the sample follow at once, and
    /// its value is written once it settles, unless the page moved it.
    fn size_moved(&self, value: u32) {
        self.size.words.set_label(&words(value));
        self.sample_now();
        if self.quiet.get() {
            return;
        }
        let mut pending = self.size.pending.borrow_mut();
        if let Some(id) = pending.take() {
            id.remove();
        }
        let weak = self.me.clone();
        *pending = Some(glib::timeout_add_local_once(SETTLE, move || {
            if let Some(group) = weak.upgrade() {
                group.size.pending.borrow_mut().take();
                group.write_size();
            }
        }));
    }

    /// Writes the slider's value, or takes the key out when it is the
    /// release's.
    fn write_size(&self) {
        let value = self.size.scale.value().round() as u32;
        self.write(
            settings::TEXT_SIZE,
            &value.to_string(),
            &DEFAULT_PERCENT.to_string(),
        );
    }

    /// Chooses `value` for `key` against the release's `release`, logs what
    /// was done for CI and shows the result; a refusal is in the problem line.
    fn write(&self, key: &str, value: &str, release: &str) {
        match Files::here().choose_over(key, value, Some(release)) {
            Ok(()) => {
                self.problem.set_visible(false);
                let written = matches!(Files::here().source(key), Source::Person(_));
                let what = if written { "written" } else { "taken out" };
                eprintln!("edel-settings: text {key} {value} {what}");
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

/// A row's line: where its value comes from, as the other rows say it.
fn note_line(source: &Source) -> String {
    rows::note(source, false).unwrap_or_default().to_string()
}

/// The font row for `key`, the interface's or the monospace one's.
fn family_of<'a>(group: &'a Group, key: &str) -> &'a Family {
    if key == settings::FONT {
        &group.interface
    } else {
        &group.monospace
    }
}

/// Connects the drop-downs, the slider, each row's Reset and Copy as command,
/// and the group's Reset and Copy as command.
fn wire(group: &Rc<Group>) {
    for key in [settings::FONT, settings::MONOSPACE_FONT] {
        let weak = Rc::downgrade(group);
        family_of(group, key)
            .dropdown
            .connect_selected_notify(move |_| {
                if let Some(group) = weak.upgrade() {
                    group.picked(family_of(&group, key));
                }
            });
        let weak = Rc::downgrade(group);
        family_of(group, key).row.reset.connect_clicked(move |_| {
            if let Some(group) = weak.upgrade() {
                group.set(key, None);
            }
        });
        let weak = Rc::downgrade(group);
        family_of(group, key)
            .row
            .copy
            .connect_clicked(move |button| {
                let Some(group) = weak.upgrade() else {
                    return;
                };
                let chosen = family_of(&group, key);
                if let Some(name) = name_at(&chosen.model, chosen.dropdown.selected()) {
                    button.clipboard().set_text(&rows::command(&[(key, name)]));
                    widgets::copied(button);
                }
            });
    }
    let scale = group.size.scale.clone();
    let weak = Rc::downgrade(group);
    scale.connect_value_changed(move |scale| {
        if let Some(group) = weak.upgrade() {
            group.size_moved(scale.value().round() as u32);
        }
    });
    let weak = Rc::downgrade(group);
    group.size.row.reset.connect_clicked(move |_| {
        if let Some(group) = weak.upgrade() {
            group.set(settings::TEXT_SIZE, None);
        }
    });
    let weak = Rc::downgrade(group);
    group.size.row.copy.connect_clicked(move |button| {
        let Some(group) = weak.upgrade() else {
            return;
        };
        let value = group.size.scale.value().round() as u32;
        button
            .clipboard()
            .set_text(&rows::command(&[(settings::TEXT_SIZE, value.to_string())]));
        widgets::copied(button);
    });
    let weak = Rc::downgrade(group);
    group.source.reset.connect_clicked(move |_| {
        let Some(group) = weak.upgrade() else {
            return;
        };
        let files = Files::here();
        let mut result = Ok(());
        for key in [
            settings::FONT,
            settings::MONOSPACE_FONT,
            settings::TEXT_SIZE,
        ] {
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
            (settings::FONT, group.interface_name().unwrap_or_default()),
            (
                settings::MONOSPACE_FONT,
                group.monospace_name().unwrap_or_default(),
            ),
            (
                settings::TEXT_SIZE,
                (group.size.scale.value().round() as u32).to_string(),
            ),
        ];
        let pairs: Vec<(&str, String)> = pairs.into_iter().collect();
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}

/// Logs where the controls lie in the window, once the card is laid out, for
/// CI, as `edel-settings: text group places font X+Y+WxH, monospace_font ...,
/// text_size ...` (logical pixels). Asked for by one of the keys, the page
/// first scrolls the group to the top and gives the keyboard to that control;
/// it logs once two frames agree.
fn log_places(card: &gtk::Box, controls: [gtk::Widget; 3]) {
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
            let keys = [
                settings::FONT,
                settings::MONOSPACE_FONT,
                settings::TEXT_SIZE,
            ];
            let asked =
                widgets::asked_row().and_then(|row| keys.iter().position(|key| *key == row));
            if let Some(index) = asked {
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
        let places: Vec<String> = [
            settings::FONT,
            settings::MONOSPACE_FONT,
            settings::TEXT_SIZE,
        ]
        .iter()
        .zip(&controls)
        .map(|(key, control)| {
            let name = key.trim_start_matches("appearance.");
            format!("{name} {}", widgets::place(control.compute_bounds(&window)))
        })
        .collect();
        let line = format!("edel-settings: text group places {}", places.join(", "));
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
    fn the_size_words_say_default_at_100_and_the_percent_else() {
        assert_eq!(words(100), tr("Default"));
        assert_eq!(words(150), "150 %");
        assert_eq!(words(80), "80 %");
    }

    #[test]
    fn the_release_fonts_are_the_tokens_and_the_monospace_one() {
        assert_eq!(interface_release(), Tokens::built_in().font);
        assert_eq!(monospace_release(), MONOSPACE);
    }

    #[test]
    fn the_size_slider_takes_the_key_range_and_the_key_names_are_the_rows() {
        assert_eq!(settings::whole_range(settings::TEXT_SIZE), Some((80, 150)));
        for key in [
            settings::FONT,
            settings::MONOSPACE_FONT,
            settings::TEXT_SIZE,
        ] {
            assert!(
                rows::ROWS.iter().any(|row| row.key == key),
                "{key} has no row"
            );
        }
    }
}
