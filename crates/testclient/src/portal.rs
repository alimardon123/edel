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
//! the standard output, which CI's desktop test reads back.

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

/// How long a portal may take to answer one request.
const WAIT: Duration = Duration::from_secs(20);

/// Prints `line` and flushes, as the output is a file that is read back.
fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

/// A request's answer: its response code (0 for success) and its results.
type Answer = (u32, HashMap<String, OwnedValue>);

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
    ) -> Result<Answer, String> {
        let rx = self.listen(token).map_err(|e| format!("{e:#}"))?;
        make(&self.connection).map_err(|e| format!("calling {method}: {e}"))?;
        Self::answer(&rx)
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
