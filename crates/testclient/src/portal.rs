//! `--screenshot` and `--screencast` (roadmap M6.3): an app's two requests
//! to xdg-desktop-portal, made as an app makes them. Each request names a
//! handle token, so the portal answers on a request object whose path the
//! client knows before it calls: the client listens for that object's
//! `Response` signal first, calls, then waits up to 20 s for the answer.
//! `--screenshot` prints `screenshot URI file://...` (the picture the
//! portal wrote) or `screenshot failed CODE`. `--screencast` creates a
//! session, selects the monitors, starts the stream and closes the session
//! again: it prints `screencast stream NODE_ID WxH` for the first stream,
//! or `screencast failed STEP CODE`. Both draw nothing and print through
//! the standard output, which CI's desktop test reads back. `--location`
//! (roadmap M6.14c) draws nothing either: it asks the Location portal for
//! the device's position, as an app does, and prints
//! `location LAT LON ACCURACY` from the first update, or `location failed
//! STEP ERROR`.

use std::collections::HashMap;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{MatchRule, Message, message::Type};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SCREENSHOT: &str = "org.freedesktop.portal.Screenshot";
const SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
const SESSION: &str = "org.freedesktop.portal.Session";
const LOCATION: &str = "org.freedesktop.portal.Location";

/// How long a portal may take to answer one request.
const WAIT: Duration = Duration::from_secs(20);

/// Prints `line` and flushes, as the output is a file that is read back.
fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

/// A request's answer: its response code (0 for success) and its results.
type Answer = (u32, HashMap<String, OwnedValue>);

/// Why a call failed: the text to print, and the name the portal gave the
/// error (`org.freedesktop.portal.Error.NotAllowed`), or the text again
/// when there was no D-Bus error.
struct Failure {
    name: String,
    text: String,
}

impl Failure {
    fn plain(text: String) -> Failure {
        Failure {
            name: text.clone(),
            text,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

/// A session on the bus and the handle tokens it has used.
struct Portal {
    connection: Connection,
    tokens: u32,
}

impl Portal {
    fn connect() -> Result<Portal> {
        let connection = Connection::session().context("connecting to the session's bus")?;
        Ok(Portal {
            connection,
            tokens: 0,
        })
    }

    /// A handle token nobody has used in this process.
    fn token(&mut self) -> String {
        self.tokens += 1;
        format!("edeltest{}", self.tokens)
    }

    /// The path the portal answers a request with `token` on: the sender's
    /// unique name without its colon, its dots as underscores.
    fn request_path(&self, token: &str) -> Result<ObjectPath<'static>> {
        let sender = self
            .connection
            .unique_name()
            .map(|name| name.as_str().trim_start_matches(':').replace('.', "_"))
            .context("the bus gave no unique name")?;
        ObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/request/{sender}/{token}"
        ))
        .context("making the request's path")
    }

    /// Listens for the `Response` of the request with `token`, before the
    /// request is made, so an answer that comes at once is not missed.
    fn listen(&self, token: &str) -> Result<mpsc::Receiver<Option<Message>>> {
        let path = self.request_path(token)?;
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(REQUEST)?
            .member("Response")?
            .path(path)?
            .build();
        let mut signals = MessageIterator::for_match_rule(rule, &self.connection, None)
            .context("listening for the portal's answer")?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let first = signals.find_map(Result::ok);
            let _ = tx.send(first);
        });
        Ok(rx)
    }

    /// Listens for the answer to `token`, makes the call with `make`, and
    /// waits for the answer: its code and results, or why there is none.
    fn call(
        &self,
        method: &str,
        token: &str,
        make: impl FnOnce(&Connection) -> zbus::Result<Message>,
    ) -> Result<Answer, Failure> {
        let rx = self
            .listen(token)
            .map_err(|e| Failure::plain(format!("{e:#}")))?;
        if let Err(e) = make(&self.connection) {
            let text = format!("calling {method}: {e}");
            let name = match &e {
                zbus::Error::MethodError(name, _, _) => name.as_str().to_string(),
                _ => text.clone(),
            };
            return Err(Failure { name, text });
        }
        Self::answer(&rx).map_err(Failure::plain)
    }

    /// The answer the listener heard, within `WAIT`.
    fn answer(rx: &mpsc::Receiver<Option<Message>>) -> Result<Answer, String> {
        match rx.recv_timeout(WAIT) {
            Ok(Some(message)) => message
                .body()
                .deserialize::<Answer>()
                .map_err(|e| format!("reading the answer: {e}")),
            Ok(None) => Err("no answer".into()),
            Err(_) => Err("timeout".into()),
        }
    }
}

/// The text of a string or an object path value.
fn text(value: &OwnedValue) -> Option<String> {
    match &**value {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

/// `--screenshot`: asks for a picture of the screen, non-interactively.
pub fn screenshot() -> Result<()> {
    let mut portal = Portal::connect()?;
    let token = portal.token();
    let body = (
        "",
        HashMap::from([
            ("interactive", Value::from(false)),
            ("handle_token", Value::from(token.as_str())),
        ]),
    );
    match portal.call("Screenshot", &token, |c| {
        c.call_method(Some(DESKTOP), PATH, Some(SCREENSHOT), "Screenshot", &body)
    }) {
        Ok((0, results)) => match results.get("uri").and_then(text) {
            Some(uri) => say(&format!("screenshot URI {uri}")),
            None => say("screenshot failed 0 no URI"),
        },
        Ok((code, _)) => say(&format!("screenshot failed {code}")),
        Err(why) => say(&format!("screenshot failed {why}")),
    }
    Ok(())
}

/// `--screencast`: shares the first monitor, then closes the session.
pub fn screencast() -> Result<()> {
    let mut portal = Portal::connect()?;
    let token = portal.token();
    let session_token = portal.token();
    let body = (HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        ("session_handle_token", Value::from(session_token.as_str())),
    ]),);
    let session = match portal.call("CreateSession", &token, |c| {
        c.call_method(
            Some(DESKTOP),
            PATH,
            Some(SCREENCAST),
            "CreateSession",
            &body,
        )
    }) {
        Ok((0, results)) => match results.get("session_handle").and_then(text) {
            Some(session) => session,
            None => {
                say("screencast failed CreateSession no session");
                return Ok(());
            }
        },
        Ok((code, _)) => {
            say(&format!("screencast failed CreateSession {code}"));
            return Ok(());
        }
        Err(why) => {
            say(&format!("screencast failed CreateSession {why}"));
            return Ok(());
        }
    };
    let session_path =
        ObjectPath::try_from(session.as_str()).context("the portal's session path")?;

    // Monitors only (type 1, the portal's bit for a whole screen).
    let token = portal.token();
    let body = (
        session_path.clone(),
        HashMap::from([
            ("handle_token", Value::from(token.as_str())),
            ("types", Value::from(1u32)),
        ]),
    );
    match portal.call("SelectSources", &token, |c| {
        c.call_method(
            Some(DESKTOP),
            PATH,
            Some(SCREENCAST),
            "SelectSources",
            &body,
        )
    }) {
        Ok((0, _)) => {}
        Ok((code, _)) => {
            say(&format!("screencast failed SelectSources {code}"));
            return Ok(());
        }
        Err(why) => {
            say(&format!("screencast failed SelectSources {why}"));
            return Ok(());
        }
    }

    let token = portal.token();
    let body = (
        session_path.clone(),
        "",
        HashMap::from([("handle_token", Value::from(token.as_str()))]),
    );
    match portal.call("Start", &token, |c| {
        c.call_method(Some(DESKTOP), PATH, Some(SCREENCAST), "Start", &body)
    }) {
        Ok((0, results)) => match first_stream(results.get("streams")) {
            Some((node, width, height)) => {
                say(&format!("screencast stream {node} {width}x{height}"));
            }
            None => say("screencast failed Start no stream"),
        },
        Ok((code, _)) => say(&format!("screencast failed Start {code}")),
        Err(why) => say(&format!("screencast failed Start {why}")),
    }

    // The session ends here: the portal stops the stream with it.
    let _ =
        portal
            .connection
            .call_method(Some(DESKTOP), session.as_str(), Some(SESSION), "Close", &());
    Ok(())
}

/// The node id and size of the first of `streams`, `a(ua{sv})`: the node
/// is what PipeWire names the stream, the size comes from its `size`.
fn first_stream(streams: Option<&OwnedValue>) -> Option<(u32, i32, i32)> {
    let streams = streams?.clone();
    let streams = Vec::<(u32, HashMap<String, OwnedValue>)>::try_from(streams).ok()?;
    let (node, properties) = streams.into_iter().next()?;
    let size = properties.get("size")?.clone();
    let (width, height) = <(i32, i32)>::try_from(size).ok()?;
    Some((node, width, height))
}

/// `--location`: asks for the device's position, then closes the session.
pub fn location() -> Result<()> {
    let mut portal = Portal::connect()?;
    let Some(session) = create_location_session(&mut portal) else {
        return Ok(());
    };
    ObjectPath::try_from(session.as_str()).context("the portal's session path")?;

    // Listens before Start, so an update that comes at once is not missed.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(LOCATION)?
        .member("LocationUpdated")?
        .path(PATH)?
        .build();
    let signals = MessageIterator::for_match_rule(rule, &portal.connection, None)
        .context("listening for the location updates")?;
    let (tx, rx) = mpsc::channel();
    let wanted = session.clone();
    std::thread::spawn(move || {
        // The first update for this session, its location: `a{sv}`.
        let first = signals.filter_map(Result::ok).find_map(|message| {
            let body = message.body();
            let (for_session, location) = body
                .deserialize::<(ObjectPath, HashMap<String, OwnedValue>)>()
                .ok()?;
            (for_session.as_str() == wanted).then_some(location)
        });
        let _ = tx.send(first);
    });

    if start_location(&mut portal, &session) {
        match rx.recv_timeout(WAIT) {
            Ok(Some(location)) => say(&location_line(&location)),
            _ => say("location failed update timeout"),
        }
    }

    // The session ends here: the portal stops the updates with it.
    let _ =
        portal
            .connection
            .call_method(Some(DESKTOP), session.as_str(), Some(SESSION), "Close", &());
    Ok(())
}

/// Creates the location session with the accuracy an app asks for, and
/// returns its path, or prints why the portal did not make one.
fn create_location_session(portal: &mut Portal) -> Option<String> {
    let token = portal.token();
    let session_token = portal.token();
    let body = (HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        ("session_handle_token", Value::from(session_token.as_str())),
        ("distance-threshold", Value::from(0u32)),
        ("time-threshold", Value::from(0u32)),
        ("accuracy", Value::from(5u32)),
    ]),);
    match portal.call("CreateSession", &token, |c| {
        c.call_method(Some(DESKTOP), PATH, Some(LOCATION), "CreateSession", &body)
    }) {
        Ok((0, results)) => match results.get("session_handle").and_then(text) {
            Some(session) => Some(session),
            None => {
                say("location failed create no session");
                None
            }
        },
        Ok((code, _)) => {
            say(&format!("location failed create {code}"));
            None
        }
        Err(why) => {
            say(&format!("location failed create {}", why.name));
            None
        }
    }
}

/// Starts the updates of `session`, and says whether they started.
fn start_location(portal: &mut Portal, session: &str) -> bool {
    let session_path = match ObjectPath::try_from(session) {
        Ok(path) => path,
        Err(_) => {
            say("location failed start no session");
            return false;
        }
    };
    let token = portal.token();
    let body = (
        session_path,
        "",
        HashMap::from([("handle_token", Value::from(token.as_str()))]),
    );
    match portal.call("Start", &token, |c| {
        c.call_method(Some(DESKTOP), PATH, Some(LOCATION), "Start", &body)
    }) {
        Ok((0, _)) => true,
        Ok((code, _)) => {
            // 1 is denied: the app is set to Never, or the person said no.
            say(&format!("location failed start {code}"));
            false
        }
        Err(why) => {
            say(&format!("location failed start {}", why.name));
            false
        }
    }
}

/// The line `--location` prints for `location`: the latitude and longitude
/// in degrees to 4 decimals, and the accuracy in whole metres.
fn location_line(location: &HashMap<String, OwnedValue>) -> String {
    let number = |key: &str| {
        location
            .get(key)
            .and_then(|value| f64::try_from(value.clone()).ok())
    };
    match (number("Latitude"), number("Longitude")) {
        (Some(latitude), Some(longitude)) => {
            let accuracy = match number("Accuracy") {
                Some(metres) => format!("{metres:.0}"),
                None => "unknown".to_string(),
            };
            format!("location {latitude:.4} {longitude:.4} {accuracy}")
        }
        _ => "location failed update no-coordinates".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: f64) -> OwnedValue {
        OwnedValue::try_from(Value::from(value)).unwrap()
    }

    #[test]
    fn location_line_prints_degrees_and_whole_metres() {
        let location = HashMap::from([
            ("Latitude".to_string(), number(51.50742)),
            ("Longitude".to_string(), number(-0.12776)),
            ("Accuracy".to_string(), number(12.4)),
        ]);
        assert_eq!(location_line(&location), "location 51.5074 -0.1278 12");
    }

    #[test]
    fn location_line_says_when_accuracy_is_missing() {
        let location = HashMap::from([
            ("Latitude".to_string(), number(48.85)),
            ("Longitude".to_string(), number(2.35)),
        ]);
        assert_eq!(location_line(&location), "location 48.8500 2.3500 unknown");
    }

    #[test]
    fn location_line_says_when_there_are_no_coordinates() {
        let location = HashMap::from([("Accuracy".to_string(), number(5.0))]);
        assert_eq!(
            location_line(&location),
            "location failed update no-coordinates"
        );
    }
}
