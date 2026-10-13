//! The password dialog of the Updates page (roadmap M6.5): before
//! `edel update` or `edel rollback` runs as the administrator through doas,
//! the person at the machine types their own password. It is built like
//! the Shortcuts page's Change dialog, from our widgets and tokens. The
//! password goes to `edel --password-stdin` and nowhere else, and `edel`
//! checks it before it changes anything. A wrong one keeps the dialog open
//! and says so; any other result is given to the page once, and the
//! dialog closes. CI reads the line `edel-settings: update asked, exit N`
//! (and `rollback asked` for going back).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use edel::i18n::{tr, trf};
use edel::users;

use crate::{cmd, widgets};

/// One dialog and the `edel` command it runs as the administrator.
pub struct Ask {
    /// The window's title, such as "Install the update" (`n_` marked).
    pub title: &'static str,
    /// The sentence above the password entry (`n_` marked).
    pub prompt: &'static str,
    /// The button that runs the command (`n_` marked).
    pub button: &'static str,
    /// `edel`'s arguments, such as `["update"]`.
    pub args: &'static [&'static str],
}

/// What the dialog calls when the command starts running (`true`) and when
/// a wrong password sends the person back to the entry (`false`).
type Running = Box<dyn Fn(bool)>;

/// What the dialog calls with the command's outcome, once.
type Done = Box<dyn FnOnce(cmd::Outcome)>;

/// Shows the dialog beside `near`. When the password is typed and the
/// command starts, `running(true)`; when the command has run, `done` is
/// called with what it gave, once. A wrong password is said in the dialog,
/// which stays open for another try, with `running(false)`.
pub fn ask(
    near: &gtk::Widget,
    what: &'static Ask,
    running: impl Fn(bool) + 'static,
    done: impl FnOnce(cmd::Outcome) + 'static,
) {
    let user = users::read(users::current())
        .first()
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let window = gtk::Window::builder()
        .title(tr(what.title))
        .modal(true)
        .resizable(false)
        .default_width(380)
        .build();
    if let Some(parent) = near.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&parent));
    }
    let prompt = gtk::Label::builder()
        .label(tr(what.prompt))
        .xalign(0.0)
        .wrap(true)
        .build();
    let who = gtk::Label::builder()
        .label(trf("Password for {user}", &[("user", &user)]))
        .xalign(0.0)
        .css_classes(["edel-source"])
        .build();
    let entry = gtk::PasswordEntry::new();
    entry.set_show_peek_icon(false);
    let status = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .visible(false)
        .css_classes(["edel-error"])
        .build();
    let cancel = widgets::action(tr("Cancel"), false);
    let run = widgets::action(tr(what.button), true);
    let buttons = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    buttons.append(&cancel);
    buttons.append(&run);
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .build();
    body.append(&prompt);
    body.append(&who);
    body.append(&entry);
    body.append(&status);
    body.append(&buttons);
    window.set_child(Some(&body));

    let done: Rc<RefCell<Option<Done>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let running: Rc<Running> = Rc::new(Box::new(running));
    let submit = {
        let (window, entry, status, run, cancel, done, running) = (
            window.clone(),
            entry.clone(),
            status.clone(),
            run.clone(),
            cancel.clone(),
            done.clone(),
            running.clone(),
        );
        Rc::new(move || {
            // The entry's text is copied once, for the command, and the
            // entry is cleared at once.
            let password = entry.text().to_string();
            entry.set_text("");
            if password.is_empty() {
                status.set_label(tr("Type your password first."));
                status.set_visible(true);
                return;
            }
            entry.set_sensitive(false);
            run.set_sensitive(false);
            cancel.set_sensitive(false);
            running(true);
            let (window, entry, status, run, cancel, done, running) = (
                window.clone(),
                entry.clone(),
                status.clone(),
                run.clone(),
                cancel.clone(),
                done.clone(),
                running.clone(),
            );
            let args = what.args;
            glib::spawn_future_local(async move {
                let Ok(outcome) =
                    gio::spawn_blocking(move || cmd::edel_as_admin(args, password)).await
                else {
                    window.close();
                    return;
                };
                // CI clicks through the dialog and reads this line.
                eprintln!(
                    "edel-settings: {} asked, exit {}",
                    args.first().copied().unwrap_or_default(),
                    outcome.code.map_or("none".into(), |c| c.to_string())
                );
                entry.set_sensitive(true);
                run.set_sensitive(true);
                cancel.set_sensitive(true);
                if !outcome.ok && outcome.err.contains("wrong password") {
                    running(false);
                    status.set_label(tr("Wrong password. Type it again."));
                    status.set_visible(true);
                    entry.grab_focus();
                    return;
                }
                window.close();
                if let Some(done) = done.borrow_mut().take() {
                    done(outcome);
                }
            });
        })
    };
    {
        let submit = submit.clone();
        run.connect_clicked(move |_| submit());
    }
    {
        let submit = submit.clone();
        entry.connect_activate(move |_| submit());
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    let keys = gtk::EventControllerKey::new();
    {
        let window = window.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                window.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    window.add_controller(keys);
    window.present();
    entry.grab_focus();
}
