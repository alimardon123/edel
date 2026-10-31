//! Updates by themselves on the Updates page (roadmap M7.5b): the rows of
//! `updates.automatic` and `updates.restart_window`, the machine's keys,
//! under Channel in its card. A choice runs `edel settings set` and then
//! `apply` as root through doas (`cmd::set_and_apply_as_root`), as the
//! Region page's machine keys do, so the `updates` feature's crond starts
//! or stops at once; the release's value is written as a reset, never as a
//! line. Restart between shows only while updates install and restart. The
//! rows are there only where the image has the `updates` feature.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use edel::i18n::{tr, trf};
use edel::settings::Source;

use crate::files::Files;
use crate::widgets::{self, exit_text, monitors, reset_words, root_command, set_words};
use crate::{cmd, rows};

const MODE: &str = "updates.automatic";
const WINDOW: &str = "updates.restart_window";
/// The mode the release gives, the absence of the key.
const OFF: &str = "off";
const AND_RESTART: &str = "install-and-restart";
/// The spans Restart between offers, after Any hour (the absence of the
/// key); a span the file names otherwise is listed too.
const WINDOWS: &[&str] = &["02:00-05:00", "00:00-06:00", "22:00-06:00"];

/// What a mode means, under its row.
fn mode_line(mode: &str) -> &'static str {
    match mode {
        "check" => tr("Looks for an update once an hour and installs nothing."),
        "install" => tr("Installs updates by itself; each starts at your next restart."),
        AND_RESTART => tr(
            "Installs updates and restarts in the hours below; a computer with a screen asks you to restart.",
        ),
        _ => tr("Edel OS updates when you choose."),
    }
}

/// The spans Restart between lists: Any hour (empty), the offered spans,
/// then `current` when it is another.
pub fn windows(current: &str) -> Vec<String> {
    let mut list: Vec<String> = std::iter::once("")
        .chain(WINDOWS.iter().copied())
        .map(String::from)
        .collect();
    if !list.iter().any(|w| w == current) {
        list.push(current.to_string());
    }
    list
}

/// A span as the row shows it: `02:00 to 05:00`, or Any hour.
pub fn window_label(span: &str) -> String {
    match span.split_once('-') {
        Some((start, end)) => trf("{start} to {end}", &[("start", start), ("end", end)]),
        None => tr("Any hour").to_string(),
    }
}

/// The words that write `key` as `value`: a reset when it is the
/// release's own value, else a set.
pub fn words(key: &str, value: &str) -> Vec<String> {
    if value.is_empty() || (key == MODE && value == OFF) {
        reset_words(key)
    } else {
        set_words(key, value)
    }
}

struct Ui {
    mode_row: widgets::Row,
    mode: Rc<widgets::Choice>,
    modes: &'static [&'static str],
    window_row: widgets::Row,
    holder: gtk::Box,
    window: RefCell<Option<Rc<widgets::Choice>>>,
    spans: RefCell<Vec<String>>,
    /// A refused change's message, by key, shown until the next change.
    problems: RefCell<Vec<(&'static str, String)>>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

/// Adds the two rows to `group` where the image updates by itself.
pub fn rows(group: &gtk::Box) {
    if !edel::places::found_shared(edel::features::DIR)
        .join("updates.toml")
        .exists()
    {
        return;
    }
    let modes = rows::values(MODE);
    let mode = widgets::Choice::new(modes.iter().map(|m| rows::label(m)).collect());
    mode.set_width(widgets::CONTROL_WIDTH);
    let mode_row = widgets::row(group, rows::title(MODE), &mode.widget());
    let holder = gtk::Box::builder().build();
    let window_row = widgets::row(group, rows::title(WINDOW), holder.upcast_ref());
    let ui = Rc::new(Ui {
        mode_row,
        mode,
        modes,
        window_row,
        holder,
        window: RefCell::new(None),
        spans: RefCell::new(Vec::new()),
        problems: RefCell::new(Vec::new()),
        monitors: RefCell::new(Vec::new()),
    });
    let weak = Rc::downgrade(&ui);
    ui.mode.connect_changed(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(mode) = ui.modes.get(ui.mode.selected()) {
            change(&ui, MODE, words(MODE, mode));
        }
    });
    for (key, row) in [(MODE, &ui.mode_row), (WINDOW, &ui.window_row)] {
        let weak = Rc::downgrade(&ui);
        row.reset.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                change(&ui, key, reset_words(key));
            }
        });
        row.copy.connect_clicked(move |button| {
            button
                .clipboard()
                .set_text(&root_command(&[(key, value(key))]));
            widgets::copied(button);
        });
    }
    let files = Files::here();
    let watched = monitors(&files);
    for monitor in &watched {
        let weak = Rc::downgrade(&ui);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ui) = weak.upgrade() {
                show(&ui);
            }
        });
    }
    *ui.monitors.borrow_mut() = watched;
    show(&ui);
    // The rows hold their state while the group lives.
    let keep = ui.clone();
    group.connect_destroy(move |_| {
        let _ = &keep;
    });
}

/// The machine's value of `key`, the release's when it has none.
fn value(key: &str) -> String {
    edel::settings::on_machine(key).unwrap_or_else(|| {
        if key == MODE {
            OFF.to_string()
        } else {
            String::new()
        }
    })
}

/// Shows both rows as the files say them now.
fn show(ui: &Rc<Ui>) {
    let files = Files::here();
    let mode = value(MODE);
    let at = ui.modes.iter().position(|m| *m == mode).unwrap_or(0);
    ui.mode.set_selected(at);
    let span = value(WINDOW);
    let spans = windows(&span);
    if *ui.spans.borrow() != spans {
        while let Some(child) = ui.holder.first_child() {
            ui.holder.remove(&child);
        }
        let choice = widgets::Choice::new(spans.iter().map(|s| window_label(s)).collect());
        choice.set_width(widgets::CONTROL_WIDTH);
        let weak = Rc::downgrade(ui);
        let picked = Rc::downgrade(&choice);
        choice.connect_changed(move || {
            let (Some(ui), Some(choice)) = (weak.upgrade(), picked.upgrade()) else {
                return;
            };
            let span = ui.spans.borrow().get(choice.selected()).cloned();
            if let Some(span) = span {
                change(&ui, WINDOW, words(WINDOW, &span));
            }
        });
        ui.holder.append(&choice.widget());
        *ui.window.borrow_mut() = Some(choice);
        *ui.spans.borrow_mut() = spans.clone();
    }
    if let Some(choice) = ui.window.borrow().as_ref() {
        choice.set_selected(spans.iter().position(|s| *s == span).unwrap_or(0));
    }
    let window_help = tr("When an update may restart this computer by itself, in local time.");
    for (key, row, words) in [
        (MODE, &ui.mode_row, mode_line(&mode)),
        (WINDOW, &ui.window_row, window_help),
    ] {
        let source = files.source(key);
        let problem = ui
            .problems
            .borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, p)| p.clone());
        match problem {
            Some(problem) => {
                row.subtitle.add_css_class("edel-error");
                row.subtitle.set_label(&crate::status::sentence(&problem));
            }
            None => {
                row.subtitle.remove_css_class("edel-error");
                row.subtitle.set_label(&match rows::note(&source, false) {
                    Some(note) => format!("{words} · {note}"),
                    None => words.to_string(),
                });
            }
        }
        row.reset
            .set_visible(!matches!(source, Source::Release | Source::Profile(_)));
    }
    // The hours matter only while updates restart by themselves.
    if let Some(row) = ui.holder.parent() {
        row.set_visible(mode == AND_RESTART);
    }
}

/// Runs `words` and then `apply` as root, then shows the rows again, with
/// what refused the change under its row.
fn change(ui: &Rc<Ui>, key: &'static str, words: Vec<String>) {
    ui.problems.borrow_mut().retain(|(k, _)| *k != key);
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let done = gio::spawn_blocking(move || cmd::set_and_apply_as_root(words)).await;
        let Some(ui) = weak.upgrade() else { return };
        if let Ok((set, apply)) = done {
            eprintln!(
                "edel-settings: updates {key} asked, exit {}",
                exit_text(set.code)
            );
            let refused = if set.ok {
                apply.and_then(|run| (!run.ok).then(|| run.shown()))
            } else {
                Some(set.shown())
            };
            if let Some(why) = refused {
                let why = crate::status::unprefixed(why.lines().last().unwrap_or("")).to_string();
                ui.problems.borrow_mut().push((key, why));
            }
        }
        show(&ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_value_is_a_reset_and_any_other_a_set() {
        assert_eq!(words(MODE, "off"), ["settings", "reset", MODE]);
        assert_eq!(
            words(MODE, "install"),
            ["settings", "set", "updates.automatic=install"]
        );
        assert_eq!(words(WINDOW, ""), ["settings", "reset", WINDOW]);
        assert_eq!(
            words(WINDOW, "02:00-05:00"),
            ["settings", "set", "updates.restart_window=02:00-05:00"]
        );
    }

    #[test]
    fn the_hours_list_any_hour_the_offered_spans_and_the_files_own() {
        assert_eq!(windows("")[0], "");
        assert_eq!(windows("02:00-05:00").len(), 1 + WINDOWS.len());
        assert_eq!(windows("03:00-04:00").last().unwrap(), "03:00-04:00");
        assert_eq!(window_label(""), "Any hour");
        assert_eq!(window_label("02:00-05:00"), "02:00 to 05:00");
        for span in WINDOWS {
            assert!(edel::settings::hours(span).is_some(), "{span}");
        }
    }

    #[test]
    fn every_mode_is_a_value_of_the_key() {
        for mode in [OFF, "check", "install", AND_RESTART] {
            assert!(rows::values(MODE).contains(&mode), "{mode}");
        }
    }
}
