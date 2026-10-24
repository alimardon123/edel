//! What the Updates page knows and says (M5.8d), with no toolkit: the
//! output of `edel status` and `edel update --check` read into plain
//! values, and from them the one view the page shows, a headline, a line
//! under it and the buttons that fit. Every state of the page is a value
//! here, so every state is tested; `updates.rs` only draws it.
//!
//! The words are a familiar updates page's: Check for updates, Install the
//! update, Restart now, and only the Details at the page's foot say "slot".

use edel::i18n::{tr, trf};
use edel::version::compare;

/// One of the two root slots, as `edel status` prints it
/// (`A: ok=1 try=0 /dev/vda2 2026.10.3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub name: String,
    /// Whether the machine started this slot and found it good.
    pub confirmed: bool,
    /// The version it holds, none for an empty slot.
    pub version: Option<String>,
}

/// What `edel status` says about the slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The slot running now, `A` or `B`.
    pub running: String,
    /// The slot the next start takes.
    pub next: String,
    pub slots: Vec<Slot>,
}

/// An update waiting for a restart, or a rollback doing so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub version: String,
    /// Whether it is newer than the running version, else it is going back.
    pub newer: bool,
}

/// A slot's line without its name, `ok=1 try=0 /dev/vda2 2026.10.3`: whether
/// it was confirmed, and its version, none when the slot is empty; none
/// at all when the line has another shape.
pub fn slot_line(value: &str) -> Option<(bool, Option<String>)> {
    let words: Vec<&str> = value.split_whitespace().collect();
    let ok = words.iter().find_map(|w| w.strip_prefix("ok="))?;
    let version = words.last().filter(|_| words.len() >= 4)?;
    let version = (*version != "empty").then(|| version.to_string());
    Some((ok == "1", version))
}

impl Status {
    /// The status in `edel status`' output, none when it has no `running:`
    /// line, which is what a refusal looks like.
    pub fn parse(text: &str) -> Option<Status> {
        let mut running = None;
        let mut next = None;
        let mut slots = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "running" => running = Some(value.to_string()),
                "order" => next = value.split_whitespace().next().map(str::to_string),
                name @ ("A" | "B") => {
                    if let Some((confirmed, version)) = slot_line(value) {
                        slots.push(Slot {
                            name: name.to_string(),
                            confirmed,
                            version,
                        });
                    }
                }
                _ => {}
            }
        }
        let running = running?;
        Some(Status {
            next: next.unwrap_or_else(|| running.clone()),
            running,
            slots,
        })
    }

    fn running_slot(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.name == self.running)
    }

    fn other_slot(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.name != self.running)
    }

    /// The version running now.
    pub fn running_version(&self) -> Option<&str> {
        self.running_slot()?.version.as_deref()
    }

    /// The version the next start takes when it is not the running one:
    /// a new version installed, or the old one after a rollback.
    pub fn pending(&self) -> Option<Pending> {
        if self.next == self.running {
            return None;
        }
        let version = self.other_slot()?.version.clone()?;
        let newer = self
            .running_version()
            .is_none_or(|r| compare(&version, r).is_gt());
        Some(Pending { version, newer })
    }

    /// The version Roll back would start: the other slot's, when it was
    /// confirmed, is older than the running one, and nothing waits for a
    /// restart (`edel rollback` refuses a slot that is switched off).
    pub fn rollback_to(&self) -> Option<&str> {
        if self.pending().is_some() {
            return None;
        }
        let other = self.other_slot().filter(|s| s.confirmed)?;
        let version = other.version.as_deref()?;
        let running = self.running_version()?;
        compare(version, running).is_lt().then_some(version)
    }
}

/// What `edel update --check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub version: String,
    /// Whether it is newer than the running version.
    pub newer: bool,
    /// The release's date, `YYYY-MM-DD`, when the list has one.
    pub released: Option<String>,
    /// This system's image in the release, in bytes, when the list has it.
    pub size: Option<u64>,
}

/// The result in `edel update --check`' output (`running:`, `available:`,
/// and the `newer:`, `released:` and `size:` lines), none when it has no
/// `available:` line.
pub fn parse_check(text: &str) -> Option<Found> {
    let (mut running, mut available, mut newer) = (None, None, None);
    let (mut released, mut size) = (None, None);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "running" => running = Some(value),
            "available" => available = Some(value),
            "newer" => newer = Some(value == "yes"),
            "released" if !value.is_empty() => released = Some(value.to_string()),
            "size" => size = value.parse().ok(),
            _ => {}
        }
    }
    let available = available?;
    let version = available.split_whitespace().next()?.to_string();
    // An older `edel` does not print `newer:`: compare the versions.
    let newer = newer.unwrap_or_else(|| running.is_some_and(|r| compare(&version, r).is_gt()));
    Some(Found {
        version,
        newer,
        released,
        size,
    })
}

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Check,
    Install,
    Restart,
}

/// What was being done when something failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    Check,
    Install,
    GoBack,
    Restart,
}

/// Where the page is in what it is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Nothing checked yet: the page runs nothing by itself.
    Idle,
    Checking,
    /// Checked, and this system is the newest, at a time of day.
    UpToDate {
        at: String,
    },
    /// Checked, and a newer version waits.
    Available(Found),
    Installing,
    /// An update was written to the other slot.
    Installed {
        version: Option<String>,
    },
    GoingBack,
    /// Restart now was pressed.
    Restarting,
    /// A command failed, with what `edel` said and its exit status (none
    /// when it did not start or was killed): the status tells the cause
    /// in any language (`edel::exit`).
    Failed {
        doing: Doing,
        message: String,
        code: Option<i32>,
    },
}

/// What the page knows when it draws.
pub struct Inputs<'a> {
    pub phase: &'a Phase,
    /// `edel status`, none when it could not be read.
    pub status: Option<&'a Status>,
    /// The version running (os-release's `VERSION_ID`).
    pub version: Option<&'a str>,
    /// The channel followed, as a person reads it (`Stable`).
    pub channel: &'a str,
    /// Whether Settings runs as root (a test or an appliance does).
    pub root: bool,
}

/// The button that leads the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Primary {
    pub action: Action,
    pub label: String,
}

/// Everything the page shows at the top, for one state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// An icon of `design/icons/`.
    pub icon: &'static str,
    /// Something is running: a spinner instead of the icon, buttons off.
    pub busy: bool,
    /// The icon and the line read as a problem.
    pub problem: bool,
    pub title: String,
    pub sub: String,
    pub primary: Primary,
    /// The version Roll back goes to, none when there is nothing to go back to.
    pub rollback: Option<String>,
    /// The newer version, for the What's new card.
    pub news: Option<Found>,
}

fn primary(action: Action, label: &str) -> Primary {
    Primary {
        action,
        label: label.to_string(),
    }
}

/// The view for what the page knows.
pub fn view(i: &Inputs) -> View {
    let version = i.version.or(i.status.and_then(Status::running_version));
    let rollback = i.status.and_then(Status::rollback_to).map(str::to_string);
    let pending = i.status.and_then(Status::pending);
    let base = View {
        icon: "status-ok",
        busy: false,
        problem: false,
        title: String::new(),
        sub: String::new(),
        primary: primary(Action::Check, tr("Check for updates")),
        rollback,
        news: None,
    };
    match i.phase {
        Phase::Checking => View {
            busy: true,
            title: tr("Checking for updates...").into(),
            sub: crate::rows::version_line(version, Some(i.channel)),
            ..base
        },
        Phase::Installing => View {
            busy: true,
            title: tr("Installing the update...").into(),
            sub: tr(
                "This takes a few minutes. You can keep using the computer; the new version starts after a restart.",
            )
            .into(),
            primary: primary(Action::Install, tr("Install the update")),
            ..base
        },
        Phase::GoingBack => View {
            busy: true,
            title: tr("Going back...").into(),
            sub: tr("The version before the last update is being made ready.").into(),
            ..base
        },
        Phase::Restarting => View {
            busy: true,
            title: tr("Restarting...").into(),
            sub: tr("The computer restarts in a moment.").into(),
            primary: primary(Action::Restart, tr("Restart now")),
            rollback: None,
            ..base
        },
        Phase::Failed {
            doing,
            message,
            code,
        } if *doing != Doing::Check || pending.is_none() => {
            failed(*doing, message, *code, i.root, base)
        }
        _ if pending.is_some() || matches!(i.phase, Phase::Installed { .. }) => {
            let (target, newer) = match (&pending, i.phase) {
                (Some(p), _) => (Some(p.version.clone()), p.newer),
                (None, Phase::Installed { version }) => (version.clone(), true),
                _ => (None, true),
            };
            let sub = match (&target, newer) {
                (Some(v), true) => trf(
                    "Version {version} is installed and starts after a restart. If it does not start, the computer goes back by itself.",
                    &[("version", v)],
                ),
                (Some(v), false) => trf(
                    "Version {version} starts after a restart.",
                    &[("version", v)],
                ),
                (None, _) => tr(
                    "The new version starts after a restart. If it does not start, the computer goes back by itself.",
                )
                .into(),
            };
            View {
                icon: "status-restart",
                title: if newer {
                    tr("Restart to finish updating").into()
                } else {
                    tr("Restart to go back").into()
                },
                sub,
                primary: primary(Action::Restart, tr("Restart now")),
                rollback: None,
                ..base
            }
        }
        Phase::Available(found) if found.newer => View {
            icon: "status-new",
            title: tr("An update is available").into(),
            sub: match found.size {
                Some(size) => trf(
                    "Version {version} is ready to install ({size})",
                    &[
                        ("version", &found.version),
                        ("size", &crate::rows::bytes_text(size)),
                    ],
                ),
                None => trf(
                    "Version {version} is ready to install",
                    &[("version", &found.version)],
                ),
            },
            primary: primary(Action::Install, tr("Install the update")),
            news: Some(found.clone()),
            ..base
        },
        Phase::UpToDate { at } => View {
            title: tr("Edel OS is up to date").into(),
            sub: match version {
                Some(version) => trf(
                    "Version {version} · {channel} · Checked at {time}",
                    &[("version", version), ("channel", i.channel), ("time", at)],
                ),
                None => trf(
                    "{channel} · Checked at {time}",
                    &[("channel", i.channel), ("time", at)],
                ),
            },
            ..base
        },
        // Nothing checked yet, or the list's version is not newer.
        _ => View {
            icon: "status-sync",
            title: match version {
                Some(version) => trf("Edel OS {version}", &[("version", version)]),
                None => tr("Edel OS").into(),
            },
            sub: trf("{channel} · Not checked yet", &[("channel", i.channel)]),
            ..base
        },
    }
}

/// The view of a failure: what could not be done, why in plain words, and
/// the button that tries again.
fn failed(doing: Doing, message: &str, code: Option<i32>, root: bool, base: View) -> View {
    let (title, retry) = match doing {
        Doing::Check => (
            tr("Couldn't check for updates"),
            primary(Action::Check, tr("Try again")),
        ),
        Doing::Install => (
            tr("Couldn't install the update"),
            primary(Action::Install, tr("Try again")),
        ),
        Doing::GoBack => (
            tr("Couldn't go back"),
            primary(Action::Check, tr("Check for updates")),
        ),
        Doing::Restart => (
            tr("Couldn't restart"),
            primary(Action::Restart, tr("Try again")),
        ),
    };
    View {
        icon: "status-problem",
        problem: true,
        title: title.into(),
        sub: plain_failure(doing, message, code, root),
        primary: retry,
        ..base
    }
}

/// Whether a failure is a refusal of rights: `edel`'s exit status for it
/// (a person outside the admin group, M6.5), or doas's words, which doas
/// does not translate.
fn refused_for_rights(message: &str, code: Option<i32>) -> bool {
    code == Some(i32::from(edel::exit::REFUSED)) || message.contains("Operation not permitted")
}

/// What a failure says to a person: what `edel` said as a sentence, or,
/// for the two causes people meet, a sentence of its own. The command's
/// own words stay in the page's Details.
pub fn plain_failure(doing: Doing, message: &str, code: Option<i32>, root: bool) -> String {
    // A person who is not an administrator is refused by doas or by edel
    // itself: the words say what to do about that, and any other failure
    // (the network, a release that is not newer) keeps the command's own
    // message (M6.5).
    if !root && doing != Doing::Check && refused_for_rights(message, code) {
        return match doing {
            Doing::Install => tr(
                "Installing needs administrator rights, and only an administrator can change the system. Ask an administrator to install the update.",
            ),
            Doing::GoBack => tr(
                "Going back needs administrator rights, and only an administrator can change the system. Ask an administrator to go back.",
            ),
            _ => tr(
                "Restarting needs administrator rights, or the power buttons of the login screen. Restart from the power button.",
            ),
        }
        .to_string();
    }
    let message = unprefixed(message);
    if code == Some(i32::from(edel::exit::OFFLINE)) {
        return tr("Can't reach the update server. Check your internet connection and try again.")
            .to_string();
    }
    sentence(message)
}

/// `message` without the `edel update: ` that `edel` starts a failure's line
/// with, which a person reading a sentence does not need
/// (`docs/MESSAGES.md`).
pub fn unprefixed(message: &str) -> &str {
    let message = message.trim();
    message
        .strip_prefix("edel ")
        .and_then(|rest| rest.split_once(": "))
        .filter(|(word, _)| !word.contains(char::is_whitespace))
        .map_or(message, |(_, rest)| rest)
}

/// `text` with its first letter a capital, as a row's value shows a word
/// the command prints in lower case, with no full stop (M8.14).
pub fn capital(text: &str) -> String {
    let mut letters = text.trim().chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

/// `message` as a sentence: its first letter a capital and a full stop at
/// the end, as Settings shows what the command prints in lower case
/// (`docs/MESSAGES.md`).
pub fn sentence(message: &str) -> String {
    let mut text = capital(message);
    if !text.is_empty() && !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }
    text
}

/// What `edel` printed for a failure as Settings shows it: each line a
/// sentence without the command's prefix, as `docs/MESSAGES.md` asks, so a
/// page never shows `edel install: ...` in lower case. A sentence stays as
/// it is.
pub fn said(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| sentence(unprefixed(line)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "running: A\norder: A B\nA: ok=1 try=0 /dev/vda2 2026.10.5\n\
                          B: ok=1 try=0 /dev/vda3 2026.10.4\neffects: lite\n";

    fn status(text: &str) -> Status {
        Status::parse(text).unwrap()
    }

    fn inputs<'a>(phase: &'a Phase, status: Option<&'a Status>) -> Inputs<'a> {
        Inputs {
            phase,
            status,
            version: Some("2026.10.5"),
            channel: "Stable",
            root: true,
        }
    }

    fn found(newer: bool) -> Found {
        Found {
            version: "2026.10.6".into(),
            newer,
            released: Some("2026-10-08".into()),
            size: Some(412_000_000),
        }
    }

    #[test]
    fn status_is_read_into_slots() {
        let s = status(STATUS);
        assert_eq!((s.running.as_str(), s.next.as_str()), ("A", "A"));
        assert_eq!(s.running_version(), Some("2026.10.5"));
        assert_eq!(s.slots.len(), 2);
        assert!(s.slots[1].confirmed);
        assert_eq!(s.slots[1].version.as_deref(), Some("2026.10.4"));
        assert_eq!(s.pending(), None);
        assert_eq!(s.rollback_to(), Some("2026.10.4"));
        assert_eq!(Status::parse("edel status: permission denied"), None);
        assert_eq!(Status::parse(""), None);
    }

    #[test]
    fn a_new_version_waiting_for_a_restart_is_pending_and_hides_roll_back() {
        let s = status(
            "running: A\norder: B A\nA: ok=1 try=0 /dev/vda2 2026.10.5\n\
             B: ok=0 try=0 /dev/vda3 2026.10.6\n",
        );
        assert_eq!(
            s.pending(),
            Some(Pending {
                version: "2026.10.6".into(),
                newer: true
            })
        );
        assert_eq!(s.rollback_to(), None);
    }

    #[test]
    fn after_a_rollback_the_pending_version_is_older() {
        let s = status(STATUS.replace("order: A B", "order: B A").as_str());
        assert_eq!(
            s.pending(),
            Some(Pending {
                version: "2026.10.4".into(),
                newer: false
            })
        );
    }

    #[test]
    fn there_is_nothing_to_go_back_to_in_an_empty_unconfirmed_or_newer_slot() {
        let go = |b: &str| {
            status(&format!(
                "running: A\norder: A B\nA: ok=1 try=0 /dev/vda2 2026.10.5\nB: {b}\n"
            ))
            .rollback_to()
            .map(str::to_string)
        };
        assert_eq!(
            go("ok=1 try=0 /dev/vda3 2026.10.4"),
            Some("2026.10.4".into())
        );
        assert_eq!(go("ok=0 try=0 /dev/vda3 empty"), None, "empty");
        assert_eq!(go("ok=0 try=3 /dev/vda3 2026.10.4"), None, "switched off");
        assert_eq!(go("ok=1 try=0 /dev/vda3 2026.10.6"), None, "newer");
        assert_eq!(go("ok=1 try=0 /dev/vda3 2026.10.5"), None, "the same");
    }

    #[test]
    fn a_check_is_read_with_or_without_the_extra_lines() {
        let full = "edel update: checking https://x/release.toml\n\
                    running: 2026.10.5\navailable: 2026.10.6 (newer)\nnewer: yes\n\
                    released: 2026-10-08\nsize: 412000000\n";
        assert_eq!(parse_check(full), Some(found(true)));
        let same = "running: 2026.10.6\navailable: 2026.10.6 (not newer)\nnewer: no\n";
        assert_eq!(
            parse_check(same),
            Some(Found {
                version: "2026.10.6".into(),
                newer: false,
                released: None,
                size: None
            })
        );
        // An `edel` that prints only the two lines.
        let old = "running: 2026.10.5\navailable: 2026.10.6 (newer)\n";
        assert!(parse_check(old).unwrap().newer);
        assert_eq!(parse_check("edel: could not download x"), None);
    }

    #[test]
    fn up_to_date_says_so_with_the_version_and_the_time() {
        let phase = Phase::UpToDate { at: "14:32".into() };
        let v = view(&inputs(&phase, None));
        assert_eq!(v.title, "Edel OS is up to date");
        assert_eq!(v.sub, "Version 2026.10.5 · Stable · Checked at 14:32");
        assert_eq!(v.icon, "status-ok");
        assert_eq!(v.primary.label, "Check for updates");
        assert_eq!(v.primary.action, Action::Check);
        assert!(!v.busy && !v.problem && v.rollback.is_none() && v.news.is_none());
    }

    #[test]
    fn before_any_check_the_page_does_not_claim_to_be_up_to_date() {
        let s = status(STATUS);
        let v = view(&inputs(&Phase::Idle, Some(&s)));
        assert_eq!(v.title, "Edel OS 2026.10.5");
        assert_eq!(v.sub, "Stable · Not checked yet");
        assert_eq!(v.primary.label, "Check for updates");
        assert_eq!(v.rollback.as_deref(), Some("2026.10.4"));
    }

    #[test]
    fn a_newer_version_leads_to_update_now_with_its_size() {
        let phase = Phase::Available(found(true));
        let v = view(&inputs(&phase, None));
        assert_eq!(v.title, "An update is available");
        assert_eq!(v.sub, "Version 2026.10.6 is ready to install (412 MB)");
        assert_eq!(v.icon, "status-new");
        assert_eq!(v.primary.label, "Install the update");
        assert_eq!(v.primary.action, Action::Install);
        assert_eq!(v.news, Some(found(true)));
        // Not newer: the page is as it was.
        let phase = Phase::Available(found(false));
        assert_eq!(view(&inputs(&phase, None)).title, "Edel OS 2026.10.5");
    }

    #[test]
    fn checking_and_installing_are_busy_with_their_buttons_held() {
        let v = view(&inputs(&Phase::Checking, None));
        assert!(v.busy);
        assert_eq!(v.title, "Checking for updates...");
        let v = view(&inputs(&Phase::Installing, None));
        assert!(v.busy);
        assert_eq!(v.title, "Installing the update...");
        assert_eq!(v.primary.label, "Install the update");
    }

    #[test]
    fn a_restart_is_asked_for_after_an_install_and_when_status_says_so() {
        let phase = Phase::Installed {
            version: Some("2026.10.6".into()),
        };
        let v = view(&inputs(&phase, None));
        assert_eq!(v.title, "Restart to finish updating");
        assert_eq!(v.primary.label, "Restart now");
        assert_eq!(v.primary.action, Action::Restart);
        assert!(
            v.sub.starts_with("Version 2026.10.6 is installed"),
            "{}",
            v.sub
        );
        let s = status(
            "running: A\norder: B A\nA: ok=1 try=0 /dev/vda2 2026.10.5\n\
             B: ok=0 try=0 /dev/vda3 2026.10.6\n",
        );
        let v = view(&inputs(&Phase::Idle, Some(&s)));
        assert_eq!(v.title, "Restart to finish updating");
        assert_eq!(
            v.rollback, None,
            "nothing to go back to while a restart waits"
        );
        // A rollback waits for its restart too.
        let back = status(STATUS.replace("order: A B", "order: B A").as_str());
        let v = view(&inputs(&Phase::Idle, Some(&back)));
        assert_eq!(v.title, "Restart to go back");
        assert_eq!(v.sub, "Version 2026.10.4 starts after a restart.");
    }

    #[test]
    fn a_failed_check_says_why_in_plain_words() {
        let phase = Phase::Failed {
            doing: Doing::Check,
            message: "could not download https://x/release.toml; check the network".into(),
            code: Some(69),
        };
        let v = view(&inputs(&phase, None));
        assert_eq!(v.title, "Couldn't check for updates");
        assert_eq!(
            v.sub,
            "Can't reach the update server. Check your internet connection and try again."
        );
        assert!(v.problem);
        assert_eq!(v.icon, "status-problem");
        assert_eq!(v.primary.label, "Try again");
        // Any other message is shown as a sentence.
        let phase = Phase::Failed {
            doing: Doing::Check,
            message: "refused: signature: no key signed this".into(),
            code: None,
        };
        assert_eq!(
            view(&inputs(&phase, None)).sub,
            "Refused: signature: no key signed this."
        );
    }

    #[test]
    fn a_failure_that_is_not_a_refusal_keeps_its_own_words_for_a_person() {
        let phase = Phase::Failed {
            doing: Doing::Install,
            message: "could not download the release: the network is down".into(),
            code: Some(69),
        };
        let mut i = inputs(&phase, None);
        i.root = false;
        assert_eq!(
            view(&i).sub,
            "Can't reach the update server. Check your internet connection and try again."
        );
        let phase = Phase::Failed {
            doing: Doing::GoBack,
            message: "refused: no version is kept ready".into(),
            code: None,
        };
        let mut i = inputs(&phase, None);
        i.root = false;
        assert_eq!(view(&i).sub, "Refused: no version is kept ready.");
    }

    #[test]
    fn changing_the_machine_as_a_person_says_what_is_missing() {
        // edel's refusal is found by its exit status, whatever language it is in.
        let phase = Phase::Failed {
            doing: Doing::Install,
            message: "edel install: impossible d'écrire /run/edel".into(),
            code: Some(i32::from(edel::exit::REFUSED)),
        };
        let mut i = inputs(&phase, None);
        i.root = false;
        let v = view(&i);
        assert_eq!(v.title, "Couldn't install the update");
        assert!(v.sub.contains("administrator rights"), "{}", v.sub);
        assert!(v.sub.contains("Ask an administrator"), "{}", v.sub);
        let phase = Phase::Failed {
            doing: Doing::GoBack,
            message: "edel update: guest n'est pas dans le groupe admin".into(),
            code: Some(i32::from(edel::exit::REFUSED)),
        };
        let mut i = inputs(&phase, None);
        i.root = false;
        assert!(view(&i).sub.starts_with("Going back needs administrator"));
        // doas's own words are not translated, so they still mean a refusal.
        let phase = Phase::Failed {
            doing: Doing::Restart,
            message: "doas: Operation not permitted".into(),
            code: Some(1),
        };
        let mut i = inputs(&phase, None);
        i.root = false;
        assert!(view(&i).sub.starts_with("Restarting needs administrator"));
        // As root the command's own message is the reason.
        let phase = Phase::Failed {
            doing: Doing::Install,
            message: "refused: version 2026.10.5 is not newer".into(),
            code: None,
        };
        assert_eq!(
            view(&inputs(&phase, None)).sub,
            "Refused: version 2026.10.5 is not newer."
        );
    }

    #[test]
    fn a_failed_check_does_not_hide_a_waiting_restart() {
        let s = status(
            "running: A\norder: B A\nA: ok=1 try=0 /dev/vda2 2026.10.5\n\
             B: ok=0 try=0 /dev/vda3 2026.10.6\n",
        );
        let phase = Phase::Failed {
            doing: Doing::Check,
            message: "x".into(),
            code: None,
        };
        assert_eq!(
            view(&inputs(&phase, Some(&s))).title,
            "Restart to finish updating"
        );
    }

    #[test]
    fn a_failure_is_shown_as_sentences() {
        assert_eq!(
            said("edel install: \"mount\" failed with exit status: 32\n"),
            "\"mount\" failed with exit status: 32."
        );
        assert_eq!(
            said("warning: x\nedel settings: no such key"),
            "Warning: x.\nNo such key."
        );
        assert_eq!(
            said("Wrong password. Type it again."),
            "Wrong password. Type it again."
        );
        assert_eq!(said(""), "");
    }

    #[test]
    fn the_commands_prefix_is_not_part_of_the_sentence() {
        assert_eq!(
            unprefixed("edel update: could not download x: refused"),
            "could not download x: refused"
        );
        assert_eq!(unprefixed("edel status: x"), "x");
        assert_eq!(
            unprefixed("could not read edel: x"),
            "could not read edel: x"
        );
        let phase = Phase::Failed {
            doing: Doing::Check,
            message: "edel update: could not download https://x/release.toml: refused".into(),
            code: Some(i32::from(edel::exit::OFFLINE)),
        };
        assert!(view(&inputs(&phase, None)).sub.starts_with("Can't reach"));
        let phase = Phase::Failed {
            doing: Doing::Check,
            message: "edel update: refused: expired: x".into(),
            code: None,
        };
        assert_eq!(view(&inputs(&phase, None)).sub, "Refused: expired: x.");
    }

    #[test]
    fn a_translated_download_failure_is_found_by_its_exit_status() {
        let phase = Phase::Failed {
            doing: Doing::Check,
            message: "edel update: impossible de télécharger https://x/release.toml".into(),
            code: Some(i32::from(edel::exit::OFFLINE)),
        };
        assert_eq!(
            view(&inputs(&phase, None)).sub,
            "Can't reach the update server. Check your internet connection and try again."
        );
    }

    #[test]
    fn sizes_and_sentences_read_plainly() {
        assert_eq!(
            sentence("could not read x; try again"),
            "Could not read x; try again."
        );
        assert_eq!(sentence("Done."), "Done.");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn a_slot_line_gives_its_state_and_version() {
        assert_eq!(
            slot_line("ok=1 try=0 /dev/vda2 2026.10.3"),
            Some((true, Some("2026.10.3".into())))
        );
        assert_eq!(slot_line("ok=0 try=0 /dev/vda3 empty"), Some((false, None)));
        assert_eq!(slot_line("unexpected"), None);
    }
}
