//! greetd's IPC (M5.10b), the greeter's side: messages of JSON objects
//! with string values, each sent as its length in four bytes of the
//! machine's own order and the JSON, on the socket `GREETD_SOCK` names.
//! A login is `create_session` with the user name, then the answer to
//! each prompt (`post_auth_message_response`) until `success`, then
//! `start_session` with the command; a wrong password is an `error`,
//! after which the session is cancelled and asked for again. Only the
//! few messages the greeter sends and the string fields it reads are
//! written and read here, so no JSON library is needed. Tested.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result, bail};

/// What greetd answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Success,
    /// A prompt: whether the answer is secret (a password), and its words.
    Prompt {
        secret: bool,
        words: String,
    },
    /// A message to show, needing no answer but an empty one.
    Info(String),
    /// Refused: `auth_error` for a wrong user or password, else another.
    Error {
        auth: bool,
        words: String,
    },
}

/// `text` as a JSON string, quotes included.
pub fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The messages the greeter sends.
pub fn create_session(user: &str) -> String {
    format!(
        "{{\"type\":\"create_session\",\"username\":{}}}",
        quoted(user)
    )
}

pub fn answer(response: Option<&str>) -> String {
    match response {
        Some(r) => format!(
            "{{\"type\":\"post_auth_message_response\",\"response\":{}}}",
            quoted(r)
        ),
        None => "{\"type\":\"post_auth_message_response\"}".to_string(),
    }
}

pub fn start_session(cmd: &[&str]) -> String {
    let cmd: Vec<String> = cmd.iter().map(|c| quoted(c)).collect();
    format!(
        "{{\"type\":\"start_session\",\"cmd\":[{}],\"env\":[]}}",
        cmd.join(",")
    )
}

pub const CANCEL: &str = "{\"type\":\"cancel_session\"}";

/// The string fields of a flat JSON object, in order; other values are
/// skipped. None when it is not such an object.
pub fn fields(json: &str) -> Option<Vec<(String, String)>> {
    let mut chars = json.trim().chars().peekable();
    let mut out = Vec::new();
    if chars.next()? != '{' {
        return None;
    }
    loop {
        skip_space(&mut chars);
        match chars.peek()? {
            '}' => return Some(out),
            ',' => {
                chars.next();
                continue;
            }
            '"' => {}
            _ => return None,
        }
        let key = string(&mut chars)?;
        skip_space(&mut chars);
        if chars.next()? != ':' {
            return None;
        }
        skip_space(&mut chars);
        if *chars.peek()? == '"' {
            let value = string(&mut chars)?;
            out.push((key, value));
        } else {
            // A number, a boolean, null, a list or an object: skipped, as
            // greetd's replies the greeter reads hold none it needs.
            let mut depth = 0i32;
            let mut in_string = false;
            while let Some(&c) = chars.peek() {
                if in_string {
                    chars.next();
                    if c == '\\' {
                        chars.next();
                    } else if c == '"' {
                        in_string = false;
                    }
                    continue;
                }
                match c {
                    '"' => in_string = true,
                    '[' | '{' => depth += 1,
                    ']' | '}' if depth > 0 => depth -= 1,
                    ',' | '}' if depth == 0 => break,
                    _ => {}
                }
                chars.next();
            }
        }
    }
}

fn skip_space(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

/// A JSON string at the cursor, its quotes taken off and escapes read.
fn string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
}

/// greetd's answer, read from its JSON.
pub fn reply(json: &str) -> Option<Reply> {
    let fields = fields(json)?;
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    Some(match get("type")? {
        "success" => Reply::Success,
        "auth_message" => {
            let words = get("auth_message").unwrap_or_default().to_string();
            match get("auth_message_type")? {
                "secret" => Reply::Prompt {
                    secret: true,
                    words,
                },
                "visible" => Reply::Prompt {
                    secret: false,
                    words,
                },
                _ => Reply::Info(words),
            }
        }
        "error" => Reply::Error {
            auth: get("error_type") == Some("auth_error"),
            words: get("description").unwrap_or_default().to_string(),
        },
        _ => return None,
    })
}

/// A reply as the log says it: its kind, and an error's words.
fn said(reply: &Reply) -> String {
    match reply {
        Reply::Success => "success".to_string(),
        Reply::Prompt { secret: true, .. } => "secret prompt".to_string(),
        Reply::Prompt { secret: false, .. } => "visible prompt".to_string(),
        Reply::Info(_) => "message".to_string(),
        Reply::Error { auth, words } => format!(
            "{} error: {words}",
            if *auth { "authentication" } else { "other" }
        ),
    }
}

/// A connection to greetd.
pub struct Greetd(UnixStream);

impl Greetd {
    /// Connects to the socket `GREETD_SOCK` names.
    pub fn connect() -> Result<Greetd> {
        let path = std::env::var_os("GREETD_SOCK")
            .context("GREETD_SOCK is not set: the greeter runs only under greetd")?;
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("connecting to greetd at {}", path.to_string_lossy()))?;
        Ok(Greetd(stream))
    }

    /// Sends `message` and reads greetd's reply.
    pub fn ask(&mut self, message: &str) -> Result<Reply> {
        send(&mut self.0, message)?;
        let json = receive(&mut self.0)?;
        reply(&json).with_context(|| format!("greetd's reply was not understood: {json}"))
    }
}

/// Writes one message: its length, then its bytes.
pub fn send(to: &mut impl Write, message: &str) -> Result<()> {
    let len = u32::try_from(message.len()).context("the message is too long")?;
    to.write_all(&len.to_ne_bytes())?;
    to.write_all(message.as_bytes())?;
    to.flush()?;
    Ok(())
}

/// Reads one message.
pub fn receive(from: &mut impl Read) -> Result<String> {
    let mut len = [0u8; 4];
    from.read_exact(&mut len)
        .context("greetd closed the connection")?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > 1 << 20 {
        bail!("greetd's reply says it is {len} bytes long");
    }
    let mut body = vec![0u8; len];
    from.read_exact(&mut body)?;
    String::from_utf8(body).context("greetd's reply is not UTF-8")
}

/// What a login comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// The session starts: the greeter ends.
    Started,
    /// The user name or password was wrong.
    Refused,
    /// Something else failed: why.
    Failed(String),
}

/// Logs `user` in with `password` and starts `cmd`, answering every
/// secret prompt with the password and any other with nothing; a refusal
/// cancels the session, so the next try starts afresh.
pub fn log_in(greetd: &mut Greetd, user: &str, password: &str, cmd: &[&str]) -> Login {
    let mut next = create_session(user);
    // At most a few prompts: PAM asks once for a password, maybe once
    // more for a message.
    for _ in 0..8 {
        let answer = greetd.ask(&next);
        // What greetd said to each step, for the greeter's log; never
        // the password.
        let step = fields(&next)
            .and_then(|f| f.into_iter().next())
            .map(|(_, v)| v)
            .unwrap_or_default();
        match &answer {
            Ok(reply) => eprintln!("edel-shell-ui: greetd: {step}: {}", said(reply)),
            Err(e) => eprintln!("edel-shell-ui: greetd: {step}: {e:#}"),
        }
        match answer {
            Ok(Reply::Success) if next.contains("\"start_session\"") => return Login::Started,
            Ok(Reply::Success) => next = start_session(cmd),
            Ok(Reply::Prompt { secret: true, .. }) => next = answer(Some(password)),
            Ok(Reply::Prompt { secret: false, .. }) => next = answer(Some(user)),
            Ok(Reply::Info(_)) => next = answer(None),
            Ok(Reply::Error { auth, words }) => {
                let _ = greetd.ask(CANCEL);
                return if auth {
                    Login::Refused
                } else {
                    Login::Failed(words)
                };
            }
            Err(e) => {
                let _ = greetd.ask(CANCEL);
                return Login::Failed(format!("{e:#}"));
            }
        }
    }
    let _ = greetd.ask(CANCEL);
    Login::Failed("greetd kept asking".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_written_as_greetd_reads_them() {
        assert_eq!(
            create_session("ci"),
            r#"{"type":"create_session","username":"ci"}"#
        );
        assert_eq!(
            answer(Some("a\"b\\c")),
            r#"{"type":"post_auth_message_response","response":"a\"b\\c"}"#
        );
        assert_eq!(
            start_session(&["/bin/sh", "-c", "edel-session"]),
            r#"{"type":"start_session","cmd":["/bin/sh","-c","edel-session"],"env":[]}"#
        );
        assert_eq!(quoted("tab\there\u{1}"), "\"tab\\there\\u0001\"");
    }

    #[test]
    fn replies_are_read_with_their_fields() {
        assert_eq!(reply(r#"{"type":"success"}"#), Some(Reply::Success));
        assert_eq!(
            reply(
                r#"{"type": "auth_message", "auth_message_type": "secret", "auth_message": "Password: "}"#
            ),
            Some(Reply::Prompt {
                secret: true,
                words: "Password: ".into()
            })
        );
        assert_eq!(
            reply(r#"{"type":"auth_message","auth_message_type":"info","auth_message":"Hi é"}"#),
            Some(Reply::Info("Hi \u{e9}".into()))
        );
        assert_eq!(
            reply(
                r#"{"type":"error","error_type":"auth_error","description":"pam_authenticate: AUTH_ERR"}"#
            ),
            Some(Reply::Error {
                auth: true,
                words: "pam_authenticate: AUTH_ERR".into()
            })
        );
        // Other values are skipped, whatever they hold.
        assert_eq!(
            fields(r#"{"n":1,"list":["a,}",{"x":[1]}],"type":"success","b":true}"#).unwrap(),
            vec![("type".to_string(), "success".to_string())]
        );
        assert_eq!(reply("not json"), None);
        assert_eq!(reply(r#"{"type":"unknown"}"#), None);
    }

    #[test]
    fn a_message_goes_out_and_back_with_its_length() {
        let mut wire = Vec::new();
        send(&mut wire, "{\"type\":\"success\"}").unwrap();
        assert_eq!(&wire[..4], &18u32.to_ne_bytes());
        assert_eq!(
            receive(&mut wire.as_slice()).unwrap(),
            "{\"type\":\"success\"}"
        );
    }

    /// A greetd that knows ci's password `open` and checks the order of
    /// the messages, over a socket pair.
    fn fake_greetd(server: UnixStream) -> std::thread::JoinHandle<Vec<String>> {
        std::thread::spawn(move || {
            let mut s = server;
            let mut seen = Vec::new();
            while let Ok(m) = receive(&mut s) {
                seen.push(m.clone());
                let fields = fields(&m).unwrap();
                let get = |k: &str| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone());
                let out = match get("type").as_deref() {
                    Some("create_session") => {
                        r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#
                    }
                    Some("post_auth_message_response")
                        if get("response").as_deref() == Some("open") =>
                    {
                        r#"{"type":"success"}"#
                    }
                    Some("post_auth_message_response") => {
                        r#"{"type":"error","error_type":"auth_error","description":"no"}"#
                    }
                    Some("start_session") | Some("cancel_session") => r#"{"type":"success"}"#,
                    _ => r#"{"type":"error","error_type":"error","description":"?"}"#,
                };
                send(&mut s, out).unwrap();
            }
            seen
        })
    }

    #[test]
    fn a_login_answers_the_password_and_starts_the_session_or_cancels() {
        let (client, server) = UnixStream::pair().unwrap();
        let fake = fake_greetd(server);
        let mut greetd = Greetd(client);
        assert_eq!(
            log_in(&mut greetd, "ci", "shut", &["edel-session"]),
            Login::Refused
        );
        assert_eq!(
            log_in(&mut greetd, "ci", "open", &["edel-session"]),
            Login::Started
        );
        drop(greetd);
        let seen = fake.join().unwrap();
        let kinds: Vec<String> = seen
            .iter()
            .map(|m| fields(m).unwrap()[0].1.clone())
            .collect();
        assert_eq!(
            kinds,
            [
                "create_session",
                "post_auth_message_response",
                "cancel_session",
                "create_session",
                "post_auth_message_response",
                "start_session"
            ]
        );
    }
}
