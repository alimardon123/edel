//! The System page (roadmap M5.8c): the whole machine as one file
//! (ADR-006). Export saves what `edel settings export` prints, to copy
//! this machine to another; Apply takes a settings file, shows what
//! `edel settings diff FILE` says it would change, and only then offers
//! to apply it with `edel settings import FILE` (the file also becomes
//! this machine's own, so the next boot keeps it). Both run the command
//! on a thread of its own and show its result or its message as they are
//! (`cmd.rs`): changing the machine needs root until `doas` arrives
//! (M6.5), and until then a click that lacks the right reads `edel`'s
//! refusal. The page runs nothing in the background.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};

use crate::{cmd, widgets};

const INTRO: &str = n_(
    "A machine is one settings file. Save this one to set up another the same way, or pick a \
     file to see what it would change here before anything is changed.",
);

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
            cmd::line(&["settings", "export"]),
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
            .set_text(&cmd::line(&["settings", "import", &path]));
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
    let keep = ui.clone();
    page.connect_destroy(move |_| {
        let _ = &keep;
    });
    page
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
            let done = gio::spawn_blocking(move || cmd::edel(&["settings", "diff", &shown])).await;
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
                gio::spawn_blocking(move || cmd::edel(&["settings", "import", &shown])).await;
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

/// Writes what `edel settings export` prints to `path`; the message when
/// it cannot.
fn save(path: &std::path::Path) -> Result<(), String> {
    let run = cmd::edel(&["settings", "export"]);
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
