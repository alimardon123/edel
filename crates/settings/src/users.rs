//! The Users page (roadmap M5.8b, drawn as the approved board of 2026-10-11,
//! M5.32): who has an account on this computer. The person at the machine
//! comes first, centred on a card: a round avatar with their first letter,
//! their name large, and under it their login name and whether they are an
//! administrator. Your account is their Administrator row, a key of the
//! settings file (`users.NAME.admin`), in a card under it. Other people
//! follow, each as a row with a small avatar, and Details, closed, holds the
//! login name, user id, home folder and login shell for people who need them.
//! A password is never shown, read or touched: the page reads `/etc/passwd`
//! and `/etc/group` (`edel::users`) and nothing else.
//!
//! Adding and removing people is not on this page yet, so it shows no such
//! buttons. Seam (ADR-008): doas (M6.5) lets the page run `edel settings
//! apply` through `cmd.rs`, as the Updates page runs `edel update`, and the
//! buttons come back as rows of the same kind.

use std::rc::Rc;

use gtk::prelude::*;

use edel::i18n::{n_, tr, trf};
use edel::users::{self, Person};

use crate::{card, rows, widgets};

const INTRO: &str = n_("The people who use this computer.");

/// What a person is: an administrator can change the system.
fn role(person: &Person) -> &'static str {
    if person.admin {
        tr("Administrator")
    } else {
        tr("Standard user")
    }
}

/// The line under a person's name: the name they log in with, when their
/// full name hides it, then what they are.
fn line(person: &Person) -> String {
    if person.full_name.is_empty() || person.full_name == person.name {
        role(person).to_string()
    } else {
        trf(
            "{name} · {role}",
            &[("name", &person.name), ("role", role(person))],
        )
    }
}

/// Whether the settings file's `users` keys reach this account: apply
/// leaves system accounts, such as a stick's `live`, alone.
fn in_the_file(person: &Person) -> bool {
    (1000..65534).contains(&person.uid)
}

/// A round avatar with the first letter of `name`: the person's on the
/// card, or a smaller one in a row of other people.
fn avatar(name: &str, small: bool) -> gtk::Label {
    let letter: String = name
        .chars()
        .next()
        .map_or(String::new(), |c| c.to_uppercase().collect());
    let classes: &[&str] = if small {
        &["edel-avatar", "edel-avatar-small"]
    } else {
        &["edel-avatar"]
    };
    gtk::Label::builder()
        .label(letter)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(classes)
        .build()
}

/// Another person as a row: a small avatar, the name and the line under it.
fn person_row(group: &gtk::Box, person: &Person) {
    let row = gtk::Box::builder()
        .spacing(12)
        .css_classes(["edel-row"])
        .build();
    row.append(&avatar(person.shown(), true));
    let words = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    words.append(
        &gtk::Label::builder()
            .label(person.shown())
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-row-name"])
            .build(),
    );
    words.append(
        &gtk::Label::builder()
            .label(line(person))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["edel-row-subtitle"])
            .build(),
    );
    row.append(&words);
    group.append(&row);
}

pub fn page() -> gtk::Widget {
    let (page, content, _) = widgets::page(tr("Users"), tr(INTRO));
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    content.append(&body);
    // The accounts are read when the page is shown, not on a timer: who
    // has one changes seldom, and the file is small.
    let announced = std::cell::Cell::new(false);
    let announced = Rc::new(announced);
    page.connect_map(move |_| {
        let people = users::read(users::current());
        card::clear(&body);
        fill(&body, &people);
        if let (Some(you), false) = (people.first(), announced.replace(true)) {
            // CI reads this line to know who the page showed first.
            eprintln!("edel-settings: users page shows \"{}\"", you.shown());
        }
    });
    page
}

/// Draws the page for `people`, the person at the machine first.
fn fill(body: &gtk::Box, people: &[Person]) {
    let Some(you) = people.first() else {
        let group = widgets::group(body);
        widgets::note_row(
            &group,
            tr("Settings could not find your account in /etc/passwd, so there is no one to show."),
        );
        return;
    };

    // The person at the machine, centred as the board draws them: a round
    // avatar with the first letter, the name large, the line under it.
    let card = widgets::hero_card(body);
    card.set_spacing(6);
    card.append(&avatar(you.shown(), false));
    card.append(
        &gtk::Label::builder()
            .label(you.shown())
            .xalign(0.5)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .css_classes(["edel-person-name"])
            .build(),
    );
    card.append(
        &gtk::Label::builder()
            .label(line(you))
            .xalign(0.5)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .css_classes(["edel-person-line"])
            .build(),
    );
    card.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}. {}",
        you.shown(),
        line(you)
    ))]);

    if in_the_file(you) {
        let group = widgets::group(body);
        let switch = gtk::Switch::builder().active(you.admin).build();
        switch.set_sensitive(false);
        let row = widgets::row(&group, tr("Administrator"), switch.upcast_ref());
        row.subtitle.set_label(tr(
            "Can change the system. Changing it needs administrator rights.",
        ));
        row.reset.set_visible(false);
        // The command that turns it the other way, as an administrator
        // would run it (then `edel settings apply`).
        let key = format!("users.{}.admin", you.name);
        let command = rows::command(&[(&key, (!you.admin).to_string())]);
        row.copy.connect_clicked(move |button| {
            button.clipboard().set_text(&command);
            widgets::copied(button);
        });
    }

    widgets::heading(body, tr("Other people"));
    let group = widgets::group(body);
    let others = &people[1..];
    if others.is_empty() {
        widgets::note_row(&group, tr("No one else has an account on this computer."));
    }
    for person in others {
        person_row(&group, person);
    }

    // Add a person and Remove a person are not on this page yet; see above.
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let details = widgets::group(&inner);
    widgets::value_row(&details, tr("Username"), &you.name);
    widgets::value_row(&details, tr("User ID"), &you.uid.to_string());
    widgets::value_row(&details, tr("Home folder"), &you.home);
    widgets::value_row(&details, tr("Login shell"), &you.shell);
    card::fold(body, tr("Details"), &inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, full: &str, admin: bool, uid: u32) -> Person {
        Person {
            name: name.into(),
            full_name: full.into(),
            uid,
            home: format!("/home/{name}"),
            shell: "/bin/ash".into(),
            admin,
        }
    }

    #[test]
    fn a_person_is_an_administrator_or_a_standard_user() {
        assert_eq!(line(&person("bob", "", false, 1001)), "Standard user");
        assert_eq!(line(&person("ali", "ali", true, 1000)), "Administrator");
        assert_eq!(
            line(&person("ali", "Ali Karimov", true, 1000)),
            "ali · Administrator",
            "the username, when the full name hides it"
        );
    }

    #[test]
    fn the_settings_file_reaches_people_and_not_the_systems_own_accounts() {
        assert!(in_the_file(&person("ali", "", false, 1000)));
        assert!(!in_the_file(&person("live", "", false, 102)));
        assert!(!in_the_file(&person("root", "", true, 0)));
    }

    #[test]
    fn the_keys_the_page_speaks_of_are_in_the_table() {
        for key in ["users.*.admin", "users.*.login_shell", "users.*.ssh_keys"] {
            assert!(
                edel::settings::KEYS
                    .iter()
                    .any(|k| k.path == key && k.supported),
                "{key} is not a supported key"
            );
        }
    }
}
