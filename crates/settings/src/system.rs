//! The System page (roadmap M5.8c): the whole machine as one file
//! (ADR-006). Export saves what `edel settings export` prints, to copy
//! this machine to another; Apply takes a settings file, shows what
//! `edel settings diff FILE` says it would change, and only then offers
//! to apply it with `edel settings import FILE` (the file also becomes
//! this machine's own, so the next boot keeps it). Both run the command
//! on a thread of its own and show its result or its message as they are
//! (`cmd.rs`): changing the machine runs as root through doas (M6.5), and
//! a click that lacks the right reads `edel`'s refusal. The page runs
//! nothing in the background.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};

use crate::{actions, cmd, rows, widgets};

const INTRO: &str = n_("Copy this machine to another, or set it up from a file.");

/// The machine key developer mode is, and the switch that sets it (M7.1).
const DEVELOPER_KEY: &str = "system.developer_mode";

/// How long a first click on Reset developer mode waits for the second,
/// as Updates' Roll back does (M7.1): resetting deletes the changes made
/// under /usr, so it is asked twice.
const CONFIRM: Duration = Duration::from_secs(5);

/// The line under developer mode's switch: what this boot is, from the
/// flag (is it on now), the boot's own mark `places::DEVELOPER_NOW`
/// (trimmed, `None` when it is not there) and whether the reset mark
/// `places::DEVELOPER_RESET` is there (M7.1). A reset is said first, as it
/// wins at the next start (`edel`'s `developer_state` says the same
/// order); nothing is said while it is on and running as set, or off and
/// nothing is pending.
fn state_line(flag: bool, now: Option<&str>, reset: bool) -> Option<&'static str> {
    if reset {
        return Some(tr("Its changes are deleted when the computer next starts."));
    }
    match (flag, now) {
        (true, Some("held")) => Some(tr(
            "Held back: a start found no slot with a try left, so your changes are off. Switch developer mode off and on again to keep them, or reset it.",
        )),
        (true, Some(_)) => None,
        (true, None) => Some(tr("On after a restart.")),
        (false, Some(_)) => Some(tr("Off after a restart.")),
        (false, None) => None,
    }
}

/// What `edel settings diff FILE` said, ready to show.
#[derive(Debug, PartialEq, Eq)]
pub struct Diff {
    /// The changes, one to a line, and any notes the command added; or its
    /// message when it failed.
    pub text: String,
    /// How many changes applying the file would make.
    pub changes: usize,
    /// Whether the command failed rather than found changes or none.
    pub failed: bool,
}

/// Reads the command's answer: it prints one `change: ...` line for each
/// change and exits with 1 when there are some, 0 when there are none,
/// and 1 with a message on standard error when it could not read the file,
/// so the lines decide which of the two a 1 is.
pub fn diff(run: &cmd::Outcome) -> Diff {
    let changes: Vec<&str> = run
        .out
        .lines()
        .filter_map(|line| line.strip_prefix("change: "))
        .collect();
    if run.ok || !changes.is_empty() {
        let notes = run.out.lines().filter(|l| !l.starts_with("change: "));
        let text = if changes.is_empty() {
            tr("This machine already matches this file; there is nothing to apply.").to_string()
        } else {
            changes
                .iter()
                .copied()
                .chain(notes)
                .collect::<Vec<_>>()
                .join("\n")
        };
        return Diff {
            text,
            changes: changes.len(),
            failed: false,
        };
    }
    Diff {
        text: run.shown(),
        changes: 0,
        failed: true,
    }
}

struct Ui {
    page: gtk::Widget,
    export_out: gtk::Label,
    chosen_row: widgets::Row,
    diff_out: gtk::Label,
    apply: gtk::Button,
    apply_out: gtk::Label,
    /// The file picked to apply.
    file: RefCell<Option<PathBuf>>,
}

pub fn page() -> gtk::Widget {
    let (page, content, _) = widgets::page(tr("System"), tr(INTRO));
    // What this release did not understand comes first, hidden when there
    // is nothing (M6.11).
    crate::unknown::group(&content);

    widgets::heading(&content, tr("Copy this machine"));
    let group = widgets::group(&content);
    let export = widgets::action(tr("Save as..."), true);
    let export_row = widgets::row(
        &group,
        tr("Save this machine's settings"),
        export.upcast_ref(),
    );
    export_row.subtitle.set_label(tr(
        "Writes one file that describes this machine, without your files or any secret.",
    ));
    export_row.reset.set_visible(false);
    let export_out = widgets::output_row(&group);

    widgets::heading(&content, tr("Set this machine up from a file"));
    let group = widgets::group(&content);
    let choose = widgets::action(tr("Choose file..."), false);
    let chosen_row = widgets::row(&group, tr("Settings file"), choose.upcast_ref());
    chosen_row.subtitle.set_label(tr("No file chosen yet."));
    chosen_row.reset.set_visible(false);
    chosen_row.copy.set_visible(false);
    let diff_out = widgets::output_row(&group);
    let apply = widgets::action(tr("Apply"), true);
    apply.set_sensitive(false);
    let apply_row = widgets::row(&group, tr("Apply the file"), apply.upcast_ref());
    apply_row.subtitle.set_label(tr(
        "Makes the file this machine's settings. Choose a file first to see what it changes.",
    ));
    apply_row.reset.set_visible(false);
    let apply_out = widgets::output_row(&group);

    // Developer mode (M7.1), after the file: the switch, its line and the
    // reset, which is shown only while developer mode has changes to delete.
    let dev = developer(&content);

    let ui = Rc::new(Ui {
        page: page.clone(),
        export_out,
        chosen_row,
        diff_out,
        apply: apply.clone(),
        apply_out,
        file: RefCell::new(None),
    });

    export_row.copy.connect_clicked(|button| {
        button.clipboard().set_text(&format!(
            "{} > {}",
            cmd::line(actions::EXPORT.args),
            edel::places::SETTINGS
        ));
        widgets::copied(button);
    });
    let weak = Rc::downgrade(&ui);
    apply_row.copy.connect_clicked(move |button| {
        let Some(ui) = weak.upgrade() else { return };
        let file = ui.file.borrow().clone();
        let path = file.map_or(edel::places::SETTINGS.to_string(), |p| {
            p.display().to_string()
        });
        button
            .clipboard()
            .set_text(&cmd::line(&with_file(&actions::IMPORT, &path)));
        widgets::copied(button);
    });
    let weak = Rc::downgrade(&ui);
    export.connect_clicked(move |button| {
        if let Some(ui) = weak.upgrade() {
            ui.export(button);
        }
    });
    let weak = Rc::downgrade(&ui);
    choose.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.choose();
        }
    });
    let weak = Rc::downgrade(&ui);
    apply.connect_clicked(move |button| {
        if let Some(ui) = weak.upgrade() {
            ui.apply(button);
        }
    });
    let keep = (ui.clone(), dev);
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
}

/// The Developer mode group (M7.1): a switch for `system.developer_mode`
/// with its line and what this boot is, and Reset developer mode, shown
/// while developer mode has changes under /usr. The switch's state is the
/// flag file's; a change runs as root through `edel` (as Region's machine
/// keys do), then applies, on a thread of its own. Turning it on asks
/// first, and Cancel puts the switch back off.
fn developer(content: &gtk::Box) -> Rc<Developer> {
    widgets::heading(content, tr("For developers"));
    let group = widgets::group(content);
    let switch = gtk::Switch::new();
    let row = widgets::row(&group, rows::title(DEVELOPER_KEY), switch.upcast_ref());
    row.subtitle.set_label(tr(
        "Change Edel OS's own files under /usr and install packages with apk. Takes effect when the computer next starts.",
    ));
    // The reset below is the way back, so the row's Reset would repeat it.
    row.reset.set_visible(false);
    let copy_switch = switch.clone();
    row.copy.connect_clicked(move |button| {
        let value = if copy_switch.is_active() {
            "true"
        } else {
            "false"
        };
        button
            .clipboard()
            .set_text(&rows::command(&[(DEVELOPER_KEY, value.to_string())]));
        widgets::copied(button);
    });
    let state = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-row", "edel-row-subtitle"])
        .build();
    group.append(&state);

    // The reset is the group's second row, there only while the layer is.
    let reset = widgets::action(tr("Reset"), false);
    let reset_row = widgets::row(&group, tr("Reset developer mode"), reset.upcast_ref());
    let reset_shown = group.last_child().expect("the row just added");
    reset_row.subtitle.set_label(tr(
        "Deletes the changes developer mode made under /usr when the computer next starts, and takes its key out of the settings file.",
    ));
    reset_row.reset.set_visible(false);
    reset_row.copy.connect_clicked(|button| {
        button
            .clipboard()
            .set_text(&cmd::line(actions::RESET_DEVELOPER.args));
        widgets::copied(button);
    });
    // What `edel` refused, in its words, under the switch and the reset.
    let problem = widgets::output_row(&group);

    let dev = Rc::new(Developer {
        switch: switch.clone(),
        state,
        reset_shown,
        reset: reset.clone(),
        problem,
        quiet: Cell::new(false),
        armed: Cell::new(false),
    });
    dev.update();

    let weak = Rc::downgrade(&dev);
    switch.connect_active_notify(move |switch| {
        let Some(dev) = weak.upgrade() else { return };
        if dev.quiet.get() {
            return;
        }
        if switch.is_active() {
            let weak = Rc::downgrade(&dev);
            ask_turn_on(switch.upcast_ref(), move |confirmed| {
                let Some(dev) = weak.upgrade() else { return };
                if confirmed {
                    dev.run(set_words(true), "true");
                } else {
                    dev.set_switch(false);
                }
            });
        } else {
            dev.run(set_words(false), "false");
        }
    });

    let weak = Rc::downgrade(&dev);
    reset.connect_clicked(move |button| {
        let Some(dev) = weak.upgrade() else { return };
        if !dev.armed.get() {
            // Deleting the changes is not undone: ask twice.
            dev.armed.set(true);
            button.set_label(tr("Click again"));
            let weak = weak.clone();
            glib::timeout_add_local_once(CONFIRM, move || {
                if let Some(dev) = weak.upgrade() {
                    dev.disarm();
                }
            });
            return;
        }
        dev.disarm();
        dev.run(reset_words(), "reset");
    });

    let weak = Rc::downgrade(&dev);
    content.connect_map(move |_| {
        if let Some(dev) = weak.upgrade() {
            dev.update();
        }
    });
    dev
}

/// The Developer mode group's controls and what they show (M7.1).
struct Developer {
    switch: gtk::Switch,
    state: gtk::Label,
    reset_shown: gtk::Widget,
    reset: gtk::Button,
    problem: gtk::Label,
    /// Set while the page itself moves the switch, so that is no choice.
    quiet: Cell<bool>,
    /// Set after the first click on Reset developer mode.
    armed: Cell<bool>,
}

impl Developer {
    /// Shows what the files and the boot say now: the switch, the line
    /// under it and whether the reset is there.
    fn update(&self) {
        let flag = Path::new(edel::places::DEVELOPER_FLAG).exists();
        let now = fs::read_to_string(edel::places::DEVELOPER_NOW).ok();
        let reset = Path::new(edel::places::DEVELOPER_RESET).exists();
        let layer = Path::new(edel::places::DEVELOPER_LAYER).exists();
        self.set_switch(flag);
        match state_line(flag, now.as_deref().map(str::trim), reset) {
            Some(text) => widgets::say(&self.state, text, false),
            None => widgets::say(&self.state, "", false),
        }
        self.reset_shown.set_visible(layer);
    }

    /// Moves the switch without asking, as the page's own change.
    fn set_switch(&self, on: bool) {
        self.quiet.set(true);
        self.switch.set_active(on);
        self.quiet.set(false);
    }

    fn disarm(&self) {
        self.armed.set(false);
        self.reset.set_label(tr("Reset"));
    }

    /// Runs `args` as root, then `edel settings apply` when that worked, on
    /// a thread of its own. The controls wait meanwhile; a refusal is shown
    /// as `edel` said it, and the line and the switch are read again after.
    fn run(self: &Rc<Self>, args: Vec<String>, value: &'static str) {
        self.switch.set_sensitive(false);
        self.reset.set_sensitive(false);
        widgets::say(&self.problem, "", false);
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || {
                let words: Vec<&str> = args.iter().map(String::as_str).collect();
                let set = cmd::edel_as_root(&words, None);
                let apply = set.ok.then(|| cmd::edel_as_root(actions::APPLY.args, None));
                (set, apply)
            })
            .await;
            let Some(dev) = weak.upgrade() else { return };
            if let Ok((set, apply)) = done {
                eprintln!(
                    "edel-settings: system {DEVELOPER_KEY} {value} asked, exit {}",
                    exit_text(set.code)
                );
                let refused = if set.ok {
                    apply.and_then(|run| {
                        eprintln!(
                            "edel-settings: system apply asked, exit {}",
                            exit_text(run.code)
                        );
                        (!run.ok).then(|| run.shown())
                    })
                } else {
                    Some(set.shown())
                };
                match refused {
                    Some(text) => widgets::say(&dev.problem, &text, true),
                    None => widgets::say(&dev.problem, "", false),
                }
            }
            dev.switch.set_sensitive(true);
            dev.reset.set_sensitive(true);
            dev.update();
        });
    }
}

/// Asks before developer mode is turned on: Cancel is the default, and
/// `done` gets whether the person chose Turn on.
fn ask_turn_on(near: &gtk::Widget, done: impl FnOnce(bool) + 'static) {
    let parent = near.root().and_downcast::<gtk::Window>();
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(tr("Turn on developer mode?"))
        .detail(tr(
            "Developer mode lets you change Edel OS's own files and install packages with apk. If a change stops the computer from starting, Edel OS starts without your changes until you switch developer mode off and on again or reset it.",
        ))
        .buttons(vec![tr("Cancel"), tr("Turn on")])
        .cancel_button(0)
        .default_button(0)
        .build();
    dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
        done(matches!(answer, Ok(1)));
    });
}

/// The words that set developer mode: the Region page's `set` words, then
/// the assignment (M7.1).
fn set_words(on: bool) -> Vec<String> {
    actions::SET
        .args
        .iter()
        .map(|word| word.to_string())
        .chain(std::iter::once(format!("{DEVELOPER_KEY}={on}")))
        .collect()
}

/// The words that reset developer mode, as the action names them.
fn reset_words() -> Vec<String> {
    actions::RESET_DEVELOPER
        .args
        .iter()
        .map(|word| word.to_string())
        .collect()
}

/// An exit status as the log shows it: its number, or `none` when there
/// was none (a signal).
fn exit_text(code: Option<i32>) -> String {
    code.map_or_else(|| "none".to_string(), |code| code.to_string())
}

impl Ui {
    /// Asks where to save, then writes what `edel settings export` prints.
    fn export(self: &Rc<Self>, button: &gtk::Button) {
        let (ui, button) = (self.clone(), button.clone());
        glib::spawn_future_local(async move {
            let Some(path) = widgets::pick_file(
                &ui.page,
                tr("Save this machine's settings"),
                Some(edel::places::SETTINGS),
            )
            .await
            else {
                return;
            };
            button.set_sensitive(false);
            widgets::say(&ui.export_out, tr("Working..."), false);
            let target = path.clone();
            let done = gio::spawn_blocking(move || save(&target)).await;
            button.set_sensitive(true);
            match done {
                Ok(Ok(())) => widgets::say(
                    &ui.export_out,
                    &trf("Saved to {path}", &[("path", &path.display().to_string())]),
                    false,
                ),
                Ok(Err(message)) => widgets::say(&ui.export_out, &message, true),
                Err(_) => widgets::say(&ui.export_out, "", false),
            }
        });
    }

    /// Asks for a file and shows what applying it would change.
    fn choose(self: &Rc<Self>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let Some(path) = widgets::pick_file(&ui.page, tr("Choose a settings file"), None).await
            else {
                return;
            };
            let shown = path.display().to_string();
            ui.chosen_row.subtitle.set_label(&shown);
            *ui.file.borrow_mut() = Some(path);
            ui.apply.set_sensitive(false);
            widgets::say(&ui.apply_out, "", false);
            widgets::say(&ui.diff_out, tr("Working..."), false);
            let done =
                gio::spawn_blocking(move || cmd::edel(&with_file(&actions::DIFF, &shown))).await;
            let Ok(run) = done else { return };
            let seen = diff(&run);
            widgets::say(&ui.diff_out, &seen.text, seen.failed);
            ui.apply.set_sensitive(seen.changes > 0);
        });
    }

    /// Applies the chosen file, which the person has just seen the diff of.
    fn apply(self: &Rc<Self>, button: &gtk::Button) {
        let Some(path) = self.file.borrow().clone() else {
            return;
        };
        button.set_sensitive(false);
        widgets::say(&self.apply_out, tr("Working..."), false);
        let (ui, button) = (self.clone(), button.clone());
        glib::spawn_future_local(async move {
            let shown = path.display().to_string();
            let done =
                gio::spawn_blocking(move || cmd::edel(&with_file(&actions::IMPORT, &shown))).await;
            match done {
                Ok(run) => {
                    widgets::say(&ui.apply_out, &run.shown(), !run.ok);
                    // Applied, there is nothing left to apply.
                    button.set_sensitive(!run.ok);
                }
                Err(_) => widgets::say(&ui.apply_out, "", false),
            }
        });
    }
}

/// An action's `edel` arguments with the file a person picked added last,
/// such as `settings import FILE`.
fn with_file<'a>(action: &actions::Action, file: &'a str) -> Vec<&'a str> {
    action.args.iter().copied().chain([file]).collect()
}

/// Writes what `edel settings export` prints to `path`; the message when
/// it cannot.
fn save(path: &std::path::Path) -> Result<(), String> {
    let run = cmd::edel(actions::EXPORT.args);
    if !run.ok {
        return Err(run.shown());
    }
    std::fs::write(path, format!("{}\n", run.out)).map_err(|why| {
        trf(
            "could not write {path}: {why}; choose a folder you can write to",
            &[
                ("path", &path.display().to_string()),
                ("why", &why.to_string()),
            ],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ok: bool, out: &str, err: &str) -> cmd::Outcome {
        cmd::Outcome {
            ok,
            code: Some(i32::from(!ok)),
            out: out.into(),
            err: err.into(),
        }
    }

    #[test]
    fn changes_are_listed_without_the_word_change() {
        let seen = diff(&run(
            false,
            "edel settings: note\nchange: set hostname to lab-1\nchange: turn ssh on",
            "",
        ));
        assert_eq!(
            seen,
            Diff {
                text: "set hostname to lab-1\nturn ssh on\nedel settings: note".into(),
                changes: 2,
                failed: false,
            }
        );
    }

    #[test]
    fn a_file_that_changes_nothing_says_so() {
        let seen = diff(&run(true, "", ""));
        assert_eq!(seen.changes, 0);
        assert!(!seen.failed);
        assert!(seen.text.starts_with("This machine already matches"));
    }

    #[test]
    fn a_running_developer_mode_says_nothing() {
        assert_eq!(state_line(true, Some("on"), false), None);
        assert_eq!(state_line(false, None, false), None);
    }

    #[test]
    fn developer_mode_turned_on_or_off_waits_for_a_restart() {
        assert_eq!(state_line(true, None, false), Some("On after a restart."));
        assert_eq!(
            state_line(false, Some("on"), false),
            Some("Off after a restart.")
        );
    }

    #[test]
    fn a_held_layer_is_said_with_what_to_do() {
        assert_eq!(
            state_line(true, Some("held"), false),
            Some(
                "Held back: a start found no slot with a try left, so your changes are off. Switch developer mode off and on again to keep them, or reset it."
            )
        );
    }

    #[test]
    fn a_pending_reset_is_said_first() {
        let deleted = Some("Its changes are deleted when the computer next starts.");
        assert_eq!(state_line(true, Some("on"), true), deleted);
        assert_eq!(state_line(false, None, true), deleted);
        assert_eq!(state_line(true, Some("held"), true), deleted);
    }

    #[test]
    fn a_refusal_is_shown_as_it_is() {
        let seen = diff(&run(
            false,
            "",
            "edel settings: could not read /x.toml: No such file or directory (os error 2)",
        ));
        assert!(seen.failed);
        assert_eq!(seen.changes, 0);
        assert_eq!(
            seen.text,
            "edel settings: could not read /x.toml: No such file or directory (os error 2)"
        );
    }
}
