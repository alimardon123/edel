//! Notifications (M5.9b): apps tell the person something by calling
//! `Notify` on `org.freedesktop.Notifications`, which shell-ui serves on
//! the session's D-Bus. It runs on the connection and the thread zbus
//! already has for the settings portal and the tray (`portal.rs`,
//! `tray.rs`) and opens no connection and no thread of its own: a call
//! turns into a [`Msg`] on a calloop channel, as the tray's news does, and
//! the event loop shows a banner (`banner.rs`) and keeps the notification
//! in the list the notification centre shows (`centre.rs`).
//!
//! This file is the data and the server, all plain and tested without a
//! display: [`Notification`] (text only: a call's pictures are never
//! kept, and every text is cut to a size that keeps fifty of them small),
//! [`List`] (at most [`MOST`], newest first), [`banner_wanted`] (do not
//! disturb) and [`banner_ms`] (how long a banner stays), the capabilities
//! the server announces and the two signals it emits.
//!
//! Persistence: the server says it has the `persistence` capability, so a
//! banner that goes away after its few seconds is not a closed
//! notification; it stays in the list until a person dismisses it, an app
//! recalls it with `CloseNotification` or the list is full, and only those
//! emit `NotificationClosed`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;

use crate::messages;

/// The name apps find the server by.
pub const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// The list keeps this many notifications; a new one past it drops the
/// oldest.
pub const MOST: usize = 50;
/// How long a banner stays when the app asks for the default, in
/// milliseconds, and the shortest it ever stays.
pub const BANNER_MS: u64 = 5000;
pub const BANNER_LEAST_MS: u64 = 1000;
/// The longest a summary and a body are kept, in characters.
pub const SUMMARY_MOST: usize = 160;
pub const BODY_MOST: usize = 600;
/// An app's name and icon are cut to this.
pub const NAME_MOST: usize = 80;
/// The most buttons an app may add, besides the `default` action.
pub const ACTIONS_MOST: usize = 3;
/// The action a click on a notification's body invokes, as the
/// specification names it.
pub const DEFAULT_ACTION: &str = "default";

/// How pressing a notification is, from the `urgency` hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    /// Stays on screen until a person deals with it, and shows through
    /// do not disturb.
    Critical,
}

impl Urgency {
    /// The hint's value: 0 low, 1 normal, 2 critical; anything else, or
    /// no hint, is normal.
    pub fn from_hint(hint: Option<u8>) -> Urgency {
        match hint {
            Some(0) => Urgency::Low,
            Some(2) => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

/// One notification, as the list keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u32,
    /// The app's name, `notify-send`
    pub app: String,
    /// Its icon: a name in the icon themes or a path; may be empty.
    pub icon: String,
    pub summary: String,
    pub body: String,
    /// The buttons and the `default` action, as pairs of the key an app
    /// is told and the label shown.
    pub actions: Vec<(String, String)>,
    /// The app's wish, in milliseconds: -1 is the default, 0 never.
    pub timeout: i32,
    pub urgency: Urgency,
}

impl Notification {
    /// Whether a click on its body does something: the app gave a
    /// `default` action.
    pub fn has_default(&self) -> bool {
        self.actions.iter().any(|(key, _)| key == DEFAULT_ACTION)
    }

    /// The buttons: every action but `default`, with a label to show.
    pub fn buttons(&self) -> impl Iterator<Item = &(String, String)> {
        self.actions
            .iter()
            .filter(|(key, label)| key != DEFAULT_ACTION && !label.is_empty())
    }

    /// What a screen reader says of it: `Notification from APP: SUMMARY,
    /// BODY`, without the parts that are empty.
    pub fn spoken(&self) -> String {
        spoken(&self.app, &self.summary, &self.body)
    }
}

/// `Notification from APP: SUMMARY, BODY`; the app, the summary and the
/// body each left out when empty.
pub fn spoken(app: &str, summary: &str, body: &str) -> String {
    use edel::i18n::trf;
    let text = [summary, body]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    match (app.is_empty(), text.is_empty()) {
        (true, true) => edel::i18n::tr("Notification").to_string(),
        (true, false) => trf("Notification: {text}", &[("text", &text)]),
        (false, true) => trf("Notification from {app}", &[("app", app)]),
        (false, false) => trf(
            "Notification from {app}: {text}",
            &[("app", app), ("text", &text)],
        ),
    }
}

/// Why a notification was closed, the number `NotificationClosed` sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// A person dismissed it: its close button, Clear all, or the action
    /// that answered it.
    Dismissed = 2,
    /// The app took it back with `CloseNotification`.
    Recalled = 3,
    /// Pushed out of a full list, or any other reason.
    Undefined = 4,
}

/// What the server tells the event loop.
#[derive(Debug)]
pub enum Msg {
    /// A call to `Notify`: a new notification, or the one `id` replaced.
    Notify(Box<Notification>),
    /// A call to `CloseNotification`.
    Close(u32),
}

// ---- Turning a call into a notification ----

/// `text` as plain text, no longer than `most` characters (a longer one
/// is cut with an ellipsis): markup tags are dropped and its five
/// entities read, and each run of white space and control characters is
/// one space, as the server does not announce `body-markup` and an app
/// that sends it anyway should still read well.
pub fn plain(text: &str, most: usize) -> String {
    let mut out = String::new();
    let mut tag = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' if chars
                .peek()
                .is_some_and(|n| n.is_ascii_alphabetic() || *n == '/') =>
            {
                tag = true;
            }
            '>' if tag => tag = false,
            _ if tag => {}
            '&' => {
                let rest: String = chars.clone().take(6).collect();
                let entity = [
                    ("amp;", '&'),
                    ("lt;", '<'),
                    ("gt;", '>'),
                    ("quot;", '"'),
                    ("apos;", '\''),
                ]
                .into_iter()
                .find(|(name, _)| rest.starts_with(name));
                match entity {
                    Some((name, replacement)) => {
                        out.push(replacement);
                        for _ in 0..name.len() {
                            chars.next();
                        }
                    }
                    None => out.push('&'),
                }
            }
            c if c.is_whitespace() || c.is_control() => {
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
            }
            c => out.push(c),
        }
    }
    let mut out = out.trim_end().to_string();
    if out.chars().count() > most {
        out = out.chars().take(most.saturating_sub(1)).collect();
        out = format!("{}\u{2026}", out.trim_end());
    }
    out
}

/// The pairs an `actions` array holds, key then label, the way the
/// specification writes it; a pair with no key, a lone last item and
/// all but the first [`ACTIONS_MOST`] buttons are dropped (the `default`
/// action is kept besides).
pub fn pairs(actions: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut buttons = 0;
    for pair in actions.chunks_exact(2) {
        let (key, label) = (pair[0], pair[1]);
        if key.is_empty() || out.iter().any(|(k, _)| k == key) {
            continue;
        }
        if key != DEFAULT_ACTION {
            if buttons == ACTIONS_MOST {
                continue;
            }
            buttons += 1;
        }
        out.push((plain(key, NAME_MOST), plain(label, NAME_MOST)));
    }
    out
}

/// The `urgency` hint as a number, whatever whole type the app used.
pub fn urgency_hint(hints: &HashMap<&str, Value<'_>>) -> Option<u8> {
    match hints.get("urgency")? {
        Value::U8(n) => Some(*n),
        Value::U16(n) => u8::try_from(*n).ok(),
        Value::U32(n) => u8::try_from(*n).ok(),
        Value::I16(n) => u8::try_from(*n).ok(),
        Value::I32(n) => u8::try_from(*n).ok(),
        Value::I64(n) => u8::try_from(*n).ok(),
        Value::U64(n) => u8::try_from(*n).ok(),
        _ => None,
    }
}

/// What a call to `Notify` carries that shell-ui keeps.
pub struct Call<'a> {
    pub app: &'a str,
    pub icon: &'a str,
    pub summary: &'a str,
    pub body: &'a str,
    pub actions: &'a [&'a str],
    pub urgency: Option<u8>,
    pub timeout: i32,
}

/// The id a call gets: the one it asks to replace, else the next one,
/// never 0, which the specification keeps for "no notification".
pub fn assign(next: &AtomicU32, replaces: u32) -> u32 {
    if replaces != 0 {
        return replaces;
    }
    loop {
        let id = next.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

/// The notification a call makes, with `id`.
pub fn received(id: u32, call: &Call) -> Notification {
    Notification {
        id,
        app: plain(call.app, NAME_MOST),
        // A name or a path: kept as sent, only cut.
        icon: call.icon.chars().take(NAME_MOST * 3).collect(),
        summary: plain(call.summary, SUMMARY_MOST),
        body: plain(call.body, BODY_MOST),
        actions: pairs(call.actions),
        timeout: call.timeout,
        urgency: Urgency::from_hint(call.urgency),
    }
}

/// What the server says it can do: the body, buttons and `persistence`.
pub fn capabilities() -> Vec<&'static str> {
    vec!["body", "actions", "persistence"]
}

/// The server's name, vendor, version and the specification's version.
pub fn information() -> (String, String, String, String) {
    (
        "Edel".into(),
        "Edel OS".into(),
        env!("CARGO_PKG_VERSION").into(),
        "1.2".into(),
    )
}

// ---- The list ----

/// The notifications, newest first, at most [`MOST`].
#[derive(Debug, Default)]
pub struct List {
    items: Vec<Notification>,
}

impl List {
    /// Adds `n` at the front; one with the same id is the one it
    /// replaces, and a full list drops its oldest. The ids dropped.
    pub fn add(&mut self, n: Notification) -> Vec<u32> {
        self.items.retain(|old| old.id != n.id);
        self.items.insert(0, n);
        let mut dropped = Vec::new();
        while self.items.len() > MOST {
            if let Some(old) = self.items.pop() {
                dropped.push(old.id);
            }
        }
        dropped
    }

    /// Takes `id` out; whether it was there.
    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|n| n.id != id);
        self.items.len() != before
    }

    /// Takes everything out; the ids that were there, newest first.
    pub fn clear(&mut self) -> Vec<u32> {
        self.items.drain(..).map(|n| n.id).collect()
    }

    pub fn items(&self) -> &[Notification] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Whether `n` gets a banner: always, unless do not disturb is on, and
/// then only a critical one.
pub fn banner_wanted(n: &Notification, do_not_disturb: bool) -> bool {
    !do_not_disturb || n.urgency == Urgency::Critical
}

/// How long `n`'s banner stays, in milliseconds; none when it stays until
/// a person deals with it: a critical one, and one whose app said it never
/// expires (0). The app's own time is kept at least [`BANNER_LEAST_MS`].
pub fn banner_ms(n: &Notification) -> Option<u64> {
    if n.urgency == Urgency::Critical {
        return None;
    }
    match n.timeout {
        0 => None,
        t if t < 0 => Some(BANNER_MS),
        t => Some(u64::from(t.unsigned_abs()).max(BANNER_LEAST_MS)),
    }
}

/// Whether do not disturb is on, from the machine's and the person's
/// settings files' text: the person's over the machine's, off when
/// neither says.
pub fn do_not_disturb(machine: Option<&str>, person: Option<&str>) -> bool {
    edel::settings::flag(edel::settings::DO_NOT_DISTURB, machine, person).unwrap_or(false)
}

/// What to write in the person's file to turn do not disturb `on`, or
/// nothing to take the key out: writers never write a default, so when
/// what applies without the person's file (the machine's, else off) is
/// already what was asked, the key goes.
pub fn to_write(machine: Option<&str>, on: bool) -> Option<&'static str> {
    (do_not_disturb(machine, None) != on).then_some(if on { "true" } else { "false" })
}

// ---- The server ----

/// The interface apps call.
struct Server {
    events: Sender<Msg>,
    next: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&'static str> {
        capabilities()
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let id = assign(&self.next, replaces_id);
        let call = Call {
            app: app_name,
            icon: app_icon,
            summary,
            body,
            actions: &actions,
            urgency: urgency_hint(&hints),
            timeout: expire_timeout,
        };
        let _ = self.events.send(Msg::Notify(Box::new(received(id, &call))));
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.events.send(Msg::Close(id));
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        information()
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

/// Serves the interface on `connection` (the portal's and the tray's),
/// its calls going to `events`; false, with a line saying why, when
/// another notification daemon already has the name. Nothing is read or
/// kept until an app calls.
pub fn serve(connection: &zbus::blocking::Connection, events: Sender<Msg>) -> bool {
    let server = Server {
        events,
        next: Arc::new(AtomicU32::new(1)),
    };
    let served = connection
        .object_server()
        .at(PATH, server)
        .and_then(|_| connection.request_name(NAME));
    if let Err(e) = served {
        eprintln!("edel-shell-ui: {}", messages::notifications_not_served(e));
        return false;
    }
    eprintln!("edel-shell-ui: serving notifications as {NAME}");
    true
}

/// Tells apps `id` was closed, for `reason`, without waiting.
pub fn closed(connection: &zbus::blocking::Connection, id: u32, reason: Reason) {
    emit(connection, "NotificationClosed", move |emitter| {
        Box::pin(async move { Server::notification_closed(&emitter, id, reason as u32).await })
    });
}

/// Tells the app that sent `id` its action `key` was invoked, without
/// waiting.
pub fn invoked(connection: &zbus::blocking::Connection, id: u32, key: &str) {
    let key = key.to_string();
    emit(connection, "ActionInvoked", move |emitter| {
        Box::pin(async move { Server::action_invoked(&emitter, id, &key).await })
    });
}

type Said = std::pin::Pin<Box<dyn std::future::Future<Output = zbus::Result<()>> + Send>>;

/// Runs `say` with an emitter on zbus's executor, as the tray's calls
/// are; a failure is one line.
fn emit(
    connection: &zbus::blocking::Connection,
    signal: &'static str,
    say: impl FnOnce(SignalEmitter<'static>) -> Said + Send + 'static,
) {
    let connection = connection.inner().clone();
    let executor = connection.executor().clone();
    let task = async move {
        let said = match SignalEmitter::new(&connection, PATH) {
            Ok(emitter) => say(emitter).await,
            Err(e) => Err(e),
        };
        if let Err(e) = said {
            eprintln!(
                "edel-shell-ui: {}",
                messages::notification_not_said(signal, e)
            );
        }
    };
    executor.spawn(task, "notification signal").detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, summary: &str) -> Notification {
        received(
            id,
            &Call {
                app: "mail",
                icon: "mail",
                summary,
                body: "",
                actions: &[],
                urgency: None,
                timeout: -1,
            },
        )
    }

    #[test]
    fn markup_and_entities_become_plain_text_in_one_line() {
        assert_eq!(plain("<b>Hi</b> &amp; <i>bye</i>", 100), "Hi & bye");
        assert_eq!(
            plain("a &lt; b &gt; c &quot;q&quot; &apos;s&apos;", 100),
            "a < b > c \"q\" 's'"
        );
        // A lone ampersand and an angle that opens no tag stay.
        assert_eq!(plain("fish & chips, 1 < 2", 100), "fish & chips, 1 < 2");
        assert_eq!(
            plain("line one\nline\ttwo \u{7}  three  ", 100),
            "line one line two three"
        );
        assert_eq!(plain("", 10), "");
        // Cut with an ellipsis at a character, not a byte.
        assert_eq!(plain("abcdefghij", 5), "abcd\u{2026}");
        assert_eq!(plain("\u{e9}\u{e9}\u{e9}\u{e9}", 3), "\u{e9}\u{e9}\u{2026}");
        assert_eq!(plain("abcde", 5), "abcde");
    }

    #[test]
    fn actions_are_pairs_with_the_default_kept_and_the_buttons_limited() {
        let got = pairs(&["default", "", "yes", "Yes", "no", "No"]);
        assert_eq!(
            got,
            [
                ("default".to_string(), String::new()),
                ("yes".into(), "Yes".into()),
                ("no".into(), "No".into())
            ]
        );
        // A lone last item, an empty key and a repeated key go.
        assert_eq!(
            pairs(&["a", "A", "", "x", "a", "again", "z"]),
            [("a".to_string(), "A".to_string())]
        );
        // Three buttons at most; the default is not one of them.
        let many = pairs(&["1", "1", "2", "2", "3", "3", "4", "4", "default", "d"]);
        let keys: Vec<&str> = many.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["1", "2", "3", "default"]);
        assert!(pairs(&[]).is_empty());
    }

    #[test]
    fn a_notification_keeps_text_only_and_knows_its_buttons() {
        let n = received(
            7,
            &Call {
                app: "<b>Mail</b>",
                icon: "mail-unread",
                summary: "New mail",
                body: "From Ali: hello &amp; welcome",
                actions: &["default", "Open", "reply", "Reply"],
                urgency: Some(2),
                timeout: 0,
            },
        );
        assert_eq!(
            (n.app.as_str(), n.body.as_str()),
            ("Mail", "From Ali: hello & welcome")
        );
        assert_eq!(n.urgency, Urgency::Critical);
        assert!(n.has_default());
        let buttons: Vec<&str> = n.buttons().map(|(_, l)| l.as_str()).collect();
        assert_eq!(buttons, ["Reply"]);
        assert_eq!(
            n.spoken(),
            "Notification from Mail: New mail, From Ali: hello & welcome"
        );
        // Long texts are cut, so fifty of them stay small.
        let long = "x".repeat(5000);
        let n = received(
            1,
            &Call {
                app: "a",
                icon: &long,
                summary: &long,
                body: &long,
                actions: &[],
                urgency: None,
                timeout: -1,
            },
        );
        assert_eq!(n.summary.chars().count(), SUMMARY_MOST);
        assert_eq!(n.body.chars().count(), BODY_MOST);
        assert!(n.icon.chars().count() <= NAME_MOST * 3);
    }

    #[test]
    fn what_is_spoken_leaves_out_what_is_missing() {
        assert_eq!(
            spoken("Mail", "New mail", ""),
            "Notification from Mail: New mail"
        );
        assert_eq!(spoken("", "New mail", "Hi"), "Notification: New mail, Hi");
        assert_eq!(spoken("Mail", "", ""), "Notification from Mail");
        assert_eq!(spoken("", "", ""), "Notification");
    }

    #[test]
    fn urgency_comes_from_the_hint_whatever_its_type() {
        assert_eq!(Urgency::from_hint(None), Urgency::Normal);
        assert_eq!(Urgency::from_hint(Some(0)), Urgency::Low);
        assert_eq!(Urgency::from_hint(Some(1)), Urgency::Normal);
        assert_eq!(Urgency::from_hint(Some(2)), Urgency::Critical);
        assert_eq!(Urgency::from_hint(Some(9)), Urgency::Normal);
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        assert_eq!(urgency_hint(&hints), None);
        hints.insert("urgency", Value::U8(2));
        assert_eq!(urgency_hint(&hints), Some(2));
        hints.insert("urgency", Value::I32(1));
        assert_eq!(urgency_hint(&hints), Some(1));
        hints.insert("urgency", Value::I32(-1));
        assert_eq!(urgency_hint(&hints), None);
        hints.insert("urgency", Value::from("critical"));
        assert_eq!(urgency_hint(&hints), None);
    }

    #[test]
    fn ids_count_up_from_one_and_a_replacement_keeps_its_own() {
        let next = AtomicU32::new(1);
        assert_eq!(assign(&next, 0), 1);
        assert_eq!(assign(&next, 0), 2);
        assert_eq!(assign(&next, 41), 41, "the id asked to replace");
        assert_eq!(assign(&next, 0), 3, "a replacement uses no new id");
        // Never 0, even when the counter wraps.
        let wrapped = AtomicU32::new(u32::MAX);
        assert_eq!(assign(&wrapped, 0), u32::MAX);
        assert_eq!(assign(&wrapped, 0), 1);
    }

    #[test]
    fn the_server_says_what_it_can_do() {
        assert_eq!(capabilities(), ["body", "actions", "persistence"]);
        let (name, vendor, _, spec) = information();
        assert_eq!(
            (name.as_str(), vendor.as_str(), spec.as_str()),
            ("Edel", "Edel OS", "1.2")
        );
    }

    #[test]
    fn the_list_is_newest_first_replaces_by_id_and_holds_fifty() {
        let mut list = List::default();
        assert!(list.is_empty());
        for id in 1..=3 {
            assert!(list.add(note(id, &format!("n{id}"))).is_empty());
        }
        let ids = |l: &List| l.items().iter().map(|n| n.id).collect::<Vec<_>>();
        assert_eq!(ids(&list), [3, 2, 1]);
        // The same id replaces and comes to the front.
        list.add(note(1, "again"));
        assert_eq!(ids(&list), [1, 3, 2]);
        assert_eq!(
            list.items().iter().find(|n| n.id == 1).unwrap().summary,
            "again"
        );
        assert!(list.remove(3));
        assert!(!list.remove(3));
        assert_eq!(ids(&list), [1, 2]);
        assert_eq!(list.clear(), [1, 2]);
        assert!(list.is_empty());
        // Fifty hold; the fifty-first pushes the oldest out.
        for id in 1..=MOST as u32 {
            assert!(list.add(note(id, "x")).is_empty());
        }
        assert_eq!(list.len(), MOST);
        let dropped = list.add(note(100, "new"));
        assert_eq!(dropped, [1]);
        assert_eq!(list.len(), MOST);
        assert_eq!(list.items()[0].id, 100);
        assert!(list.items().iter().all(|n| n.id != 1));
        // Two at once drop two.
        assert_eq!(list.add(note(101, "y")), [2]);
    }

    #[test]
    fn do_not_disturb_keeps_banners_away_but_not_critical_ones() {
        let normal = note(1, "hello");
        let mut critical = note(2, "battery");
        critical.urgency = Urgency::Critical;
        assert!(banner_wanted(&normal, false));
        assert!(!banner_wanted(&normal, true));
        assert!(banner_wanted(&critical, true));
        // Listed whatever it does to banners.
        let mut list = List::default();
        list.add(normal);
        list.add(critical);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn a_banner_stays_five_seconds_or_what_the_app_asks_or_until_dealt_with() {
        let mut n = note(1, "x");
        assert_eq!(banner_ms(&n), Some(5000));
        n.timeout = 8000;
        assert_eq!(banner_ms(&n), Some(8000));
        n.timeout = 50;
        assert_eq!(banner_ms(&n), Some(BANNER_LEAST_MS));
        n.timeout = 0;
        assert_eq!(banner_ms(&n), None, "never expires");
        n.timeout = -1;
        n.urgency = Urgency::Critical;
        assert_eq!(banner_ms(&n), None, "critical stays until clicked");
        n.timeout = 3000;
        assert_eq!(banner_ms(&n), None);
    }

    #[test]
    fn the_key_is_read_from_the_files_and_written_without_defaults() {
        let on = "format = 1\n[notifications]\ndo_not_disturb = true\n";
        let off = "format = 1\n[notifications]\ndo_not_disturb = false\n";
        assert!(!do_not_disturb(None, None));
        assert!(do_not_disturb(Some(on), None));
        assert!(
            !do_not_disturb(Some(on), Some(off)),
            "the person's over the machine's"
        );
        assert!(do_not_disturb(None, Some(on)));
        // No machine setting: true is written, false is the absence of the key.
        assert_eq!(to_write(None, true), Some("true"));
        assert_eq!(to_write(None, false), None);
        // A machine that keeps banners away: off must be said, on is the default.
        assert_eq!(to_write(Some(on), false), Some("false"));
        assert_eq!(to_write(Some(on), true), None);
        assert_eq!(
            edel::settings::DO_NOT_DISTURB,
            "notifications.do_not_disturb"
        );
    }

    /// A session bus (`dbus-run-session -- cargo test -p edel-shell-ui
    /// notify`) gives a real call: an app notifies, closes and gets the
    /// signal; without one this has nothing to check and passes.
    #[test]
    fn a_call_on_the_bus_reaches_the_loop_and_a_closed_signal_reaches_the_app() {
        use smithay_client_toolkit::reexports::calloop;
        use std::time::{Duration, Instant};
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("no session bus: the notification server is not tried");
            return;
        }
        let (sender, channel) = calloop::channel::channel();
        let host = zbus::blocking::connection::Builder::session()
            .unwrap()
            .build()
            .unwrap();
        assert!(serve(&host, sender));
        let mut events = calloop::EventLoop::<Vec<String>>::try_new().unwrap();
        events
            .handle()
            .insert_source(channel, |event, _, seen| {
                if let calloop::channel::Event::Msg(msg) = event {
                    seen.push(match msg {
                        Msg::Notify(n) => format!(
                            "notify {} {} {} {:?} {}",
                            n.id,
                            n.app,
                            n.summary,
                            n.urgency,
                            n.actions.len()
                        ),
                        Msg::Close(id) => format!("close {id}"),
                    });
                }
            })
            .unwrap();
        let wait =
            |events: &mut calloop::EventLoop<'_, Vec<String>>, seen: &mut Vec<String>, n: usize| {
                let until = Instant::now() + Duration::from_secs(10);
                while seen.len() < n && Instant::now() < until {
                    events.dispatch(Duration::from_millis(100), seen).unwrap();
                }
            };
        let app = zbus::blocking::Connection::session().unwrap();
        macro_rules! call {
            ($method:expr, $body:expr $(,)?) => {
                app.call_method(Some(NAME), PATH, Some(NAME), $method, $body)
                    .unwrap()
            };
        }
        let capabilities: Vec<String> = call!("GetCapabilities", &()).body().deserialize().unwrap();
        assert_eq!(capabilities, ["body", "actions", "persistence"]);
        let info: (String, String, String, String) = call!("GetServerInformation", &())
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(info.0, "Edel");
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("urgency", Value::U8(2));
        let id: u32 = call!(
            "Notify",
            &(
                "notify-send",
                0u32,
                "",
                "hello",
                "<b>there</b>",
                vec!["default", ""],
                hints,
                -1i32,
            ),
        )
        .body()
        .deserialize()
        .unwrap();
        assert_eq!(id, 1);
        let mut seen = Vec::new();
        wait(&mut events, &mut seen, 1);
        assert_eq!(seen, ["notify 1 notify-send hello Critical 1"]);
        // The second call gets the next id; a replacement keeps its own.
        let next: u32 = call!(
            "Notify",
            &(
                "x",
                0u32,
                "",
                "two",
                "",
                Vec::<&str>::new(),
                HashMap::<&str, Value<'_>>::new(),
                -1i32,
            ),
        )
        .body()
        .deserialize()
        .unwrap();
        assert_eq!(next, 2);
        // The app listens for the closed signal, then takes the first back.
        let mut closed_seen = zbus::blocking::MessageIterator::for_match_rule(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(NAME)
                .unwrap()
                .member("NotificationClosed")
                .unwrap()
                .build(),
            &app,
            None,
        )
        .unwrap();
        let _: () = call!("CloseNotification", &(1u32,))
            .body()
            .deserialize()
            .unwrap();
        wait(&mut events, &mut seen, 3);
        assert_eq!(seen.last().unwrap(), "close 1");
        // The loop answers a close by saying so, as the shell does.
        closed(&host, 1, Reason::Recalled);
        let message = closed_seen.next().unwrap().unwrap();
        let (id, reason): (u32, u32) = message.body().deserialize().unwrap();
        assert_eq!((id, reason), (1, 3));
    }
}
