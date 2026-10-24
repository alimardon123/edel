//! The Appearance page's Motion group (M5.12b4): how much things move
//! (`appearance.animations`, a segmented choice of Full, Reduced and Off),
//! how fast (`appearance.animation_speed`, a slider over its key's range,
//! its words at the end: `Slower · 0.5x`, `As made · 1x`, `Faster · 2x`) and
//! with what feel (`appearance.animation_curve`, Smooth, Snappy or Springy).
//! Each is one key of the person's settings file, with Reset for the
//! person's own value and Copy as command, as the Shape group does
//! (ADR-008): the release's value is taken out, never written. Speed and
//! feel are insensitive while the motion is off. A drag writes its key once
//! it settles, as the Shape group's sliders do, so a drag is one write.
//! `show` follows the files; once two frames agree the group logs where its
//! controls lie for CI.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use edel::i18n::tr;
use edel::settings::{self, Source};

use crate::files::Files;
use crate::{rows, widgets};

const SETTLE: Duration = Duration::from_millis(250);

/// The width of the speed slider, narrower than the Shape group's: the
/// row also holds the segments of the other two rows' choices.
const SLIDER: i32 = 160;

/// The width of the speed's words at the slider's end.
const WORDS: i32 = 150;

/// The factor `value` percent of the release's speed is, as the words show
/// it: `2x`, `0.5x`, `1.5x`; one decimal only when it needs one.
pub fn factor(value: u32) -> String {
    let times = f64::from(value) / f64::from(settings::ANIMATION_SPEED_DEFAULT);
    let text = format!("{times:.1}");
    text.strip_suffix(".0").unwrap_or(&text).to_string()
}

/// What the speed's end shows for `value` percent: the word for its speed
/// (`Slower` under 100, `As made` at 100, `Faster` above) and its factor.
pub fn words(value: u32) -> String {
    let word = match value.cmp(&settings::ANIMATION_SPEED_DEFAULT) {
        std::cmp::Ordering::Less => tr("Slower"),
        std::cmp::Ordering::Equal => tr("As made"),
        std::cmp::Ordering::Greater => tr("Faster"),
    };
    format!("{word} · {}x", factor(value))
}

/// The Motion group on the Appearance page.
pub struct Group {
    /// This group as the closures it hands out refer to it.
    me: Weak<Group>,
    /// The page's line for a problem.
    problem: gtk::Label,
    /// The group's source line, Reset and Copy as command.
    source: widgets::Source,
    motion: Rc<widgets::Segments>,
    motion_row: widgets::Row,
    speed: gtk::Scale,
    speed_words: gtk::Label,
    speed_row: widgets::Row,
    feel: Rc<widgets::Segments>,
    feel_row: widgets::Row,
    /// The write the speed waits for while a drag goes on.
    pending: RefCell<Option<glib::SourceId>>,
    /// Set while `show` moves the speed, so that is not a choice.
    quiet: Cell<bool>,
}

impl Group {
    /// Shows what the files say now: each choice at its value, the speed's
    /// words, the sensitivity of speed and feel, each row's line and Reset,
    /// then the group's source line.
    pub fn show(&self) {
        let files = Files::here();
        let motion = files
            .chosen(settings::ANIMATIONS)
            .unwrap_or_else(|| settings::ANIMATIONS_DEFAULT.to_string());
        let speed = files
            .chosen(settings::ANIMATION_SPEED)
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(settings::ANIMATION_SPEED_DEFAULT);
        let feel = files
            .chosen(settings::ANIMATION_CURVE)
            .unwrap_or_else(|| settings::ANIMATION_CURVE_DEFAULT.to_string());
        self.quiet.set(true);
        let at =
            |values: &[&str], value: &str| values.iter().position(|v| *v == value).unwrap_or(0);
        self.motion
            .set_selected(at(rows::values(settings::ANIMATIONS), &motion));
        self.feel
            .set_selected(at(rows::values(settings::ANIMATION_CURVE), &feel));
        // A drag the person is making is not moved back under them.
        if self.pending.borrow().is_none() {
            self.speed.set_value(f64::from(speed));
        }
        self.quiet.set(false);
        self.speed_words.set_label(&words(speed));
        let off = motion == "off";
        self.speed.set_sensitive(!off);
        self.feel.widget().set_sensitive(!off);
        for (key, row) in [
            (settings::ANIMATIONS, &self.motion_row),
            (settings::ANIMATION_SPEED, &self.speed_row),
            (settings::ANIMATION_CURVE, &self.feel_row),
        ] {
            let source = files.source(key);
            row.reset.set_visible(rows::resettable(&source));
            let note = rows::note(&source, false);
            row.subtitle.set_label(note.unwrap_or(""));
            row.subtitle.set_visible(note.is_some());
        }
        self.source.label.set_label(&self.source_text(&files));
        self.source.reset.set_visible(
            [
                settings::ANIMATIONS,
                settings::ANIMATION_SPEED,
                settings::ANIMATION_CURVE,
            ]
            .iter()
            .any(|key| matches!(files.source(key), Source::Person(_))),
        );
    }

    /// The group's source line: your choice when the person sets any of the
    /// three, else this machine's, else the release's.
    fn source_text(&self, files: &Files) -> String {
        let sources: Vec<Source> = [
            settings::ANIMATIONS,
            settings::ANIMATION_SPEED,
            settings::ANIMATION_CURVE,
        ]
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

    /// The speed moved: its words follow at once, and its value is written
    /// once it settles, unless the group moved it.
    fn moved(&self, value: u32) {
        self.speed_words.set_label(&words(value));
        if self.quiet.get() {
            return;
        }
        let mut pending = self.pending.borrow_mut();
        if let Some(id) = pending.take() {
            id.remove();
        }
        let weak = self.me.clone();
        *pending = Some(glib::timeout_add_local_once(SETTLE, move || {
            if let Some(group) = weak.upgrade() {
                group.pending.borrow_mut().take();
                let value = group.speed.value().round() as u32;
                group.write(
                    settings::ANIMATION_SPEED,
                    &value.to_string(),
                    &settings::ANIMATION_SPEED_DEFAULT.to_string(),
                );
            }
        }));
    }

    /// Writes `key` to `value` in the person's file, or takes it out when
    /// it is what applies without it (`release`), and logs what was done.
    fn write(&self, key: &str, value: &str, release: &str) {
        match Files::here().choose_over(key, value, Some(release)) {
            Ok(()) => {
                self.problem.set_visible(false);
                let written = matches!(Files::here().source(key), Source::Person(_));
                let what = if written { "written" } else { "taken out" };
                eprintln!("edel-settings: motion {key} {value} {what}");
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

    /// The value the speed shows now, as a whole number of percent.
    fn speed_value(&self) -> u32 {
        self.speed.value().round() as u32
    }

    /// The value the motion choice shows now.
    fn motion_value(&self) -> &'static str {
        rows::values(settings::ANIMATIONS)
            .get(self.motion.selected())
            .copied()
            .unwrap_or(settings::ANIMATIONS_DEFAULT)
    }

    /// The value the feel choice shows now.
    fn feel_value(&self) -> &'static str {
        rows::values(settings::ANIMATION_CURVE)
            .get(self.feel.selected())
            .copied()
            .unwrap_or(settings::ANIMATION_CURVE_DEFAULT)
    }
}

/// Builds the Motion group in `content`, after the Shape group: the
/// section with its source line, Reset and Copy as command, then one card
/// of three rows.
pub fn group(content: &gtk::Box, problem: &gtk::Label) -> Rc<Group> {
    let source = widgets::section(content, tr("Motion"));
    let card = widgets::group(content);

    let motion = widgets::Segments::new(
        &rows::values(settings::ANIMATIONS)
            .iter()
            .map(|v| rows::label(v))
            .collect::<Vec<String>>(),
    );
    let motion_row = widgets::row(&card, rows::title(settings::ANIMATIONS), &motion.widget());

    let (least, most) =
        settings::whole_range(settings::ANIMATION_SPEED).expect("the speed has a whole range");
    let speed =
        gtk::Scale::with_range(gtk::Orientation::Horizontal, least as f64, most as f64, 1.0);
    speed.set_digits(0);
    speed.set_draw_value(false);
    speed.set_valign(gtk::Align::Center);
    speed.set_size_request(SLIDER, -1);
    let speed_words = gtk::Label::builder()
        .width_request(WORDS)
        .xalign(1.0)
        .valign(gtk::Align::Center)
        .css_classes(["edel-value"])
        .build();
    let speed_box = gtk::Box::builder().spacing(12).build();
    speed_box.append(&speed);
    speed_box.append(&speed_words);
    let speed_row = widgets::row(
        &card,
        rows::title(settings::ANIMATION_SPEED),
        speed_box.upcast_ref(),
    );

    let feel = widgets::Segments::new(
        &rows::values(settings::ANIMATION_CURVE)
            .iter()
            .map(|v| rows::label(v))
            .collect::<Vec<String>>(),
    );
    let feel_row = widgets::row(
        &card,
        rows::title(settings::ANIMATION_CURVE),
        &feel.widget(),
    );

    let group = Rc::new_cyclic(|me| Group {
        me: me.clone(),
        problem: problem.clone(),
        source,
        motion,
        motion_row,
        speed: speed.clone(),
        speed_words,
        speed_row,
        feel,
        feel_row,
        pending: RefCell::new(None),
        quiet: Cell::new(false),
    });
    wire(&group);
    log_places(&card, &group);
    group.show();
    group
}

/// Connects the three choices' picks, the speed's moves, the group's Reset
/// and Copy as command and each row's Reset and Copy as command.
fn wire(group: &Rc<Group>) {
    let weak = Rc::downgrade(group);
    group.motion.connect_changed(move || {
        if let Some(group) = weak.upgrade() {
            let value = group.motion_value();
            group.write(settings::ANIMATIONS, value, settings::ANIMATIONS_DEFAULT);
        }
    });
    let weak = Rc::downgrade(group);
    group.feel.connect_changed(move || {
        if let Some(group) = weak.upgrade() {
            let value = group.feel_value();
            group.write(
                settings::ANIMATION_CURVE,
                value,
                settings::ANIMATION_CURVE_DEFAULT,
            );
        }
    });
    let weak = Rc::downgrade(group);
    group.speed.connect_value_changed(move |scale| {
        if let Some(group) = weak.upgrade() {
            group.moved(scale.value().round() as u32);
        }
    });
    let keyed = [
        (settings::ANIMATIONS, &group.motion_row),
        (settings::ANIMATION_SPEED, &group.speed_row),
        (settings::ANIMATION_CURVE, &group.feel_row),
    ];
    for (key, row) in keyed {
        let weak = Rc::downgrade(group);
        row.reset.connect_clicked(move |_| {
            if let Some(group) = weak.upgrade() {
                group.set(key, None);
            }
        });
        let weak = Rc::downgrade(group);
        row.copy.connect_clicked(move |button| {
            let Some(group) = weak.upgrade() else {
                return;
            };
            let value = match key {
                settings::ANIMATIONS => group.motion_value().to_string(),
                settings::ANIMATION_SPEED => group.speed_value().to_string(),
                _ => group.feel_value().to_string(),
            };
            button.clipboard().set_text(&rows::command(&[(key, value)]));
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
        for key in [
            settings::ANIMATIONS,
            settings::ANIMATION_SPEED,
            settings::ANIMATION_CURVE,
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
            (settings::ANIMATIONS, group.motion_value().to_string()),
            (settings::ANIMATION_SPEED, group.speed_value().to_string()),
            (settings::ANIMATION_CURVE, group.feel_value().to_string()),
        ];
        button.clipboard().set_text(&rows::command(&pairs));
        widgets::copied(button);
    });
}

/// Logs where the group's three choices lie in the window, once the card is
/// laid out, for CI, as `edel-settings: motion group places animations
/// X+Y+WxH, animation_speed ..., animation_curve ...` (logical pixels). It
/// logs once two frames agree, as the view settles over a few frames.
fn log_places(card: &gtk::Box, group: &Rc<Group>) {
    let weak = Rc::downgrade(group);
    let last = RefCell::new(String::new());
    card.add_tick_callback(move |card, _| {
        let Some(window) = card.root().and_downcast::<gtk::Window>() else {
            return glib::ControlFlow::Continue;
        };
        if !card.is_mapped() || card.width() == 0 {
            return glib::ControlFlow::Continue;
        }
        let Some(group) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let line = format!(
            "edel-settings: motion group places animations {}, animation_speed {}, animation_curve {}",
            widgets::place(group.motion.widget().compute_bounds(&window)),
            widgets::place(group.speed.compute_bounds(&window)),
            widgets::place(group.feel.widget().compute_bounds(&window)),
        );
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
    fn the_speed_words_say_the_speed_and_its_factor() {
        assert_eq!(words(50), "Slower · 0.5x");
        assert_eq!(words(100), "As made · 1x");
        assert_eq!(words(200), "Faster · 2x");
        assert_eq!(factor(150), "1.5");
        assert_eq!(factor(400), "4");
    }

    #[test]
    fn the_speed_range_is_the_key_tables() {
        assert_eq!(
            settings::whole_range(settings::ANIMATION_SPEED),
            Some((25, 400))
        );
        assert_eq!(
            rows::values(settings::ANIMATION_CURVE),
            ["smooth", "snappy", "springy"]
        );
    }

    #[test]
    fn the_motion_rows_are_on_the_appearance_page() {
        for key in [
            settings::ANIMATIONS,
            settings::ANIMATION_SPEED,
            settings::ANIMATION_CURVE,
        ] {
            assert!(
                rows::on_page("appearance").any(|row| row.key == key),
                "{key}"
            );
        }
    }
}
