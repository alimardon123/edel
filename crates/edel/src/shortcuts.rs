//! Keyboard shortcuts (roadmap M5.13, ADR-008): `[shortcuts]` in the
//! settings file maps an action to keys, such as `close_window = "Super+W"`. An
//! action the file leaves out keeps its default from [`ACTIONS`]; `""`
//! unbinds one. Checkers refuse an unknown action, two actions on one key
//! and a rescue action (close, launcher, lock) left without keys, so a
//! person can always get out of a window; readers on a machine keep what
//! they can and say what they dropped. `docs/SHORTCUTS.md` is generated
//! from [`ACTIONS`] by [`markdown`], and a test holds it to the table.

use std::collections::BTreeMap;
use std::fmt;

/// One action keys can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// Its name in `[shortcuts]`.
    pub name: &'static str,
    /// Its keys when the file says nothing.
    pub default: &'static str,
    /// Whether it must always have keys: a way out of anything on screen.
    pub rescue: bool,
    /// What it does, for `docs/SHORTCUTS.md` and Settings.
    pub what: &'static str,
}

const fn action(
    name: &'static str,
    default: &'static str,
    rescue: bool,
    what: &'static str,
) -> Action {
    Action {
        name,
        default,
        rescue,
        what,
    }
}

/// Every action, with the Classic preset's keys (the defaults until the
/// presets ship their own, M5.4). Append only: a name is a key of the
/// settings file.
pub const ACTIONS: &[Action] = &[
    action("close_window", "Super+Q", true, "Close the focused window"),
    action(
        "open_launcher",
        "Super",
        true,
        "Open or close the launcher, Super tapped alone",
    ),
    action(
        "switch_windows",
        "Alt+Tab",
        false,
        "Switch windows, the most recently used first, while the modifiers are held; with Shift, back",
    ),
    action(
        "toggle_tiling",
        "Super+T",
        false,
        "Switch this workspace between floating and tiling",
    ),
    action("open_terminal", "Ctrl+Alt+T", false, "Open a terminal"),
    action(
        "take_screenshot",
        "Print",
        false,
        "Take a screenshot (M6.3)",
    ),
    action("lock_screen", "Super+L", true, "Lock the screen (M5.10)"),
    action("go_to_workspace_1", "Super+1", false, "Go to workspace 1"),
    action("go_to_workspace_2", "Super+2", false, "Go to workspace 2"),
    action("go_to_workspace_3", "Super+3", false, "Go to workspace 3"),
    action("go_to_workspace_4", "Super+4", false, "Go to workspace 4"),
    action("go_to_workspace_5", "Super+5", false, "Go to workspace 5"),
    action("go_to_workspace_6", "Super+6", false, "Go to workspace 6"),
    action("go_to_workspace_7", "Super+7", false, "Go to workspace 7"),
    action("go_to_workspace_8", "Super+8", false, "Go to workspace 8"),
    action("go_to_workspace_9", "Super+9", false, "Go to workspace 9"),
    action(
        "move_to_workspace_1",
        "Super+Shift+1",
        false,
        "Move the focused window to workspace 1",
    ),
    action(
        "move_to_workspace_2",
        "Super+Shift+2",
        false,
        "Move the focused window to workspace 2",
    ),
    action(
        "move_to_workspace_3",
        "Super+Shift+3",
        false,
        "Move the focused window to workspace 3",
    ),
    action(
        "move_to_workspace_4",
        "Super+Shift+4",
        false,
        "Move the focused window to workspace 4",
    ),
    action(
        "move_to_workspace_5",
        "Super+Shift+5",
        false,
        "Move the focused window to workspace 5",
    ),
    action(
        "move_to_workspace_6",
        "Super+Shift+6",
        false,
        "Move the focused window to workspace 6",
    ),
    action(
        "move_to_workspace_7",
        "Super+Shift+7",
        false,
        "Move the focused window to workspace 7",
    ),
    action(
        "move_to_workspace_8",
        "Super+Shift+8",
        false,
        "Move the focused window to workspace 8",
    ),
    action(
        "move_to_workspace_9",
        "Super+Shift+9",
        false,
        "Move the focused window to workspace 9",
    ),
    action(
        "toggle_fullscreen",
        "Super+F",
        false,
        "Make the focused window fill its screen, or leave fullscreen (M5.20)",
    ),
    action(
        "next_keyboard_layout",
        "Super+Space",
        false,
        "Switch to the next keyboard layout of region.keyboard (M5.21)",
    ),
    action(
        "minimize_window",
        "Super+H",
        false,
        "Minimize the focused window; the window list or Alt+Tab brings it back (M5.18a)",
    ),
    action(
        "toggle_maximize",
        "Super+M",
        false,
        "Maximize the focused window, or give it its size back (M5.18a)",
    ),
    action(
        "focus_window_left",
        "Super+Left",
        false,
        "Focus the window to the left of the focused one (M5.16a)",
    ),
    action(
        "focus_window_right",
        "Super+Right",
        false,
        "Focus the window to the right of the focused one (M5.16a)",
    ),
    action(
        "focus_window_up",
        "Super+Up",
        false,
        "Focus the window above the focused one (M5.16a)",
    ),
    action(
        "focus_window_down",
        "Super+Down",
        false,
        "Focus the window below the focused one (M5.16a)",
    ),
    action(
        "move_window_left",
        "Super+Shift+Left",
        false,
        "Swap the focused window with the one to its left, when tiled (M5.16a)",
    ),
    action(
        "move_window_right",
        "Super+Shift+Right",
        false,
        "Swap the focused window with the one to its right, when tiled (M5.16a)",
    ),
    action(
        "move_window_up",
        "Super+Shift+Up",
        false,
        "Swap the focused window with the one above it, when tiled (M5.16a)",
    ),
    action(
        "move_window_down",
        "Super+Shift+Down",
        false,
        "Swap the focused window with the one below it, when tiled (M5.16a)",
    ),
    action(
        "cycle_column_width",
        "Super+R",
        false,
        "Make the focused column a third, a half or two thirds of the screen wide, in the scroll style (M5.16c)",
    ),
    action(
        "show_tray",
        "Super+B",
        false,
        "Open or close the tray's apps behind its arrow, with the keyboard on them (M5.9h)",
    ),
    action(
        "next_workspace",
        "Super+Ctrl+Right",
        false,
        "Show the workspace after the shown one; stops at the last (M5.2i)",
    ),
    action(
        "previous_workspace",
        "Super+Ctrl+Left",
        false,
        "Show the workspace before the shown one; stops at the first (M5.2i)",
    ),
    action(
        "move_to_next_workspace",
        "Super+Ctrl+Shift+Right",
        false,
        "Move the focused window to the workspace after the shown one, and follow it (M5.2i)",
    ),
    action(
        "move_to_previous_workspace",
        "Super+Ctrl+Shift+Left",
        false,
        "Move the focused window to the workspace before the shown one, and follow it (M5.2i)",
    ),
];

/// The action called `name`.
pub fn find(name: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.name == name)
}

/// Keys a person presses together: modifiers and at most one other key.
/// Only modifiers, such as `Super`, is a tap of that modifier alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keys {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The other key's name as xkbcommon knows it, case aside: `Q`, `1`,
    /// `Return`, `F5`; `None` for modifiers alone.
    pub key: Option<String>,
}

/// Keys other than letters and digits that a shortcut may use.
const NAMED: &[&str] = &[
    "Return",
    "Tab",
    "Space",
    "Escape",
    "Print",
    "BackSpace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "Page_Up",
    "Page_Down",
    "Up",
    "Down",
    "Left",
    "Right",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
];

/// What keys text such as `"Super+Q"` names: `Ok(None)` for `""`, no
/// keys. Modifiers are `Super`, `Ctrl`, `Alt` and `Shift`, in any order and
/// case; the other key is a letter, a digit or one of `Return`, `Tab`,
/// `Space`, `Escape`, `Print`, `BackSpace`, `Delete`, `Insert`, `Home`,
/// `End`, `Page_Up`, `Page_Down`, the arrows and `F1` to `F12`.
pub fn parse(text: &str) -> Result<Option<Keys>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut keys = Keys {
        logo: false,
        ctrl: false,
        alt: false,
        shift: false,
        key: None,
    };
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    for (i, part) in parts.iter().enumerate() {
        let modifier = match part.to_ascii_lowercase().as_str() {
            "super" => Some(&mut keys.logo),
            "ctrl" | "control" => Some(&mut keys.ctrl),
            "alt" => Some(&mut keys.alt),
            "shift" => Some(&mut keys.shift),
            _ => None,
        };
        match modifier {
            Some(held) if !*held => *held = true,
            Some(_) => return Err(format!("{part} is named twice in {text:?}")),
            None if i + 1 < parts.len() => {
                return Err(format!(
                    "{part:?} is not a modifier; put Super, Ctrl, Alt and Shift before the key, as in \"Super+Q\""
                ));
            }
            None => keys.key = Some(key_name(part).ok_or_else(|| {
                format!("{part:?} is not a key a shortcut can use; use a letter, a digit or a key such as Return, Tab, Print or F5")
            })?),
        }
    }
    Ok(Some(keys))
}

/// The other key's name, spelled as [`NAMED`] does, letters upper case.
fn key_name(part: &str) -> Option<String> {
    let mut chars = part.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return c
            .is_ascii_alphanumeric()
            .then(|| c.to_ascii_uppercase().to_string());
    }
    NAMED
        .iter()
        .find(|n| n.eq_ignore_ascii_case(part))
        .map(|n| (*n).to_string())
}

impl fmt::Display for Keys {
    /// `Super+Ctrl+Alt+Shift+KEY`, the one spelling writers use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        for (held, name) in [
            (self.logo, "Super"),
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ] {
            if held {
                parts.push(name);
            }
        }
        if let Some(key) = &self.key {
            parts.push(key);
        }
        f.write_str(&parts.join("+"))
    }
}

/// Text in `[shortcuts]` in the one spelling writers use, or why it names
/// no keys.
pub fn normalize(text: &str) -> Result<String, String> {
    Ok(parse(text)?.map(|k| k.to_string()).unwrap_or_default())
}

/// Every action and its keys once the file's `[shortcuts]` (action name to
/// keys) is laid over the defaults, the way a reader on a machine must:
/// what it cannot use is left out and noted. Of two actions on one key
/// the rescue one keeps it, else the one the file set, else the first in
/// [`ACTIONS`]; the other loses its keys. A rescue action left without keys
/// gets its default back.
pub fn resolve(
    file: &BTreeMap<String, String>,
) -> (Vec<(&'static Action, Option<Keys>)>, Vec<String>) {
    let mut notes = Vec::new();
    let mut table: Vec<(&'static Action, Option<Keys>, bool)> = ACTIONS
        .iter()
        .map(|a| (a, parse(a.default).ok().flatten(), false))
        .collect();
    for (name, text) in file {
        let Some(row) = table.iter_mut().find(|(a, _, _)| a.name == name) else {
            notes.push(format!(
                "shortcuts.{name}: unknown action; docs/SHORTCUTS.md lists them"
            ));
            continue;
        };
        match parse(text) {
            Ok(keys) => {
                row.1 = keys;
                row.2 = true;
            }
            Err(message) => notes.push(format!("shortcuts.{name}: {message}")),
        }
    }
    for row in table
        .iter_mut()
        .filter(|(a, keys, _)| a.rescue && keys.is_none())
    {
        notes.push(format!(
            "shortcuts.{}: a way out must keep its keys, so it stays {}",
            row.0.name, row.0.default
        ));
        row.1 = parse(row.0.default).ok().flatten();
    }
    // Who keeps a key two actions want: rescue first, then the file's.
    let rank = |(a, _, set): &(&Action, Option<Keys>, bool)| (!a.rescue, !*set);
    let mut order: Vec<usize> = (0..table.len()).collect();
    order.sort_by_key(|&i| rank(&table[i]));
    let mut taken: Vec<Keys> = Vec::new();
    for i in order {
        let Some(keys) = table[i].1.clone() else {
            continue;
        };
        if taken.contains(&keys) {
            notes.push(format!(
                "shortcuts.{}: {keys} belongs to another action, so it has no keys",
                table[i].0.name
            ));
            table[i].1 = None;
        } else {
            taken.push(keys);
        }
    }
    let resolved = table.into_iter().map(|(a, keys, _)| (a, keys)).collect();
    (resolved, notes)
}

/// What a strict checker refuses in `[shortcuts]`, one line per problem:
/// unknown actions, keys it cannot read, two actions on one key and a
/// rescue action without keys.
pub fn check(file: &BTreeMap<String, String>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut keys_of: Vec<(&'static Action, Option<Keys>)> = Vec::new();
    for action in ACTIONS {
        let keys = match file.get(action.name) {
            Some(text) => match parse(text) {
                Ok(keys) => keys,
                Err(message) => {
                    lines.push(format!("shortcuts.{}: {message}", action.name));
                    continue;
                }
            },
            None => parse(action.default).ok().flatten(),
        };
        if action.rescue && keys.is_none() {
            lines.push(format!(
                "shortcuts.{}: a way out must keep its keys; unset it to go back to {}",
                action.name, action.default
            ));
        }
        keys_of.push((action, keys));
    }
    for name in file.keys().filter(|n| find(n).is_none()) {
        lines.push(format!(
            "shortcuts.{name}: unknown action; docs/SHORTCUTS.md lists them"
        ));
    }
    for (i, (action, keys)) in keys_of.iter().enumerate() {
        let Some(keys) = keys else {
            continue;
        };
        if let Some((other, _)) = keys_of[..i].iter().find(|(_, k)| k.as_ref() == Some(keys)) {
            lines.push(format!(
                "shortcuts.{}: {keys} is already {}'s",
                action.name, other.name
            ));
        }
    }
    lines
}

/// `docs/SHORTCUTS.md`, from [`ACTIONS`].
pub fn markdown() -> String {
    let mut text = String::from(
        "# Shortcuts\n\n\
         Generated from `ACTIONS` in `crates/edel/src/shortcuts.rs` (roadmap M5.13); a cargo test holds this file to it. \
         Change a shortcut with `edel settings set shortcuts.ACTION=KEYS`, such as `edel settings set shortcuts.close_window=Super+W`, \
         or in `[shortcuts]` of the settings file; `\"\"` unbinds an action, and `edel settings reset shortcuts.ACTION` brings its default back. \
         Modifiers are `Super`, `Ctrl`, `Alt` and `Shift`; the other key is a letter, a digit or one of `Return`, `Tab`, `Space`, \
         `Escape`, `Print`, `BackSpace`, `Delete`, `Insert`, `Home`, `End`, `Page_Up`, `Page_Down`, `Up`, `Down`, `Left`, `Right` \
         and `F1` to `F12`; one modifier alone, such as the launcher's `Super`, is that key tapped with nothing else. A way out (close, launcher, lock) always keeps keys, and no two actions share keys. \
         An action whose step has not landed yet leaves its keys to the app that has the keyboard.\n\n\
         | Action | Keys | What it does | Way out |\n|---|---|---|---|\n",
    );
    for a in ACTIONS {
        text.push_str(&format!(
            "| `{}` | `{}` | {} | {} |\n",
            a.name,
            a.default,
            a.what,
            if a.rescue { "yes" } else { "" }
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn keys_parse_in_any_order_and_case_and_print_one_way() {
        assert_eq!(normalize("super+q").unwrap(), "Super+Q");
        assert_eq!(
            normalize("Shift + Alt + Ctrl + super + f5").unwrap(),
            "Super+Ctrl+Alt+Shift+F5"
        );
        assert_eq!(normalize("control+alt+t").unwrap(), "Ctrl+Alt+T");
        assert_eq!(normalize("Print").unwrap(), "Print");
        assert_eq!(normalize("page_up").unwrap(), "Page_Up");
        assert_eq!(
            normalize("Super").unwrap(),
            "Super",
            "a modifier tapped alone"
        );
        assert_eq!(normalize("").unwrap(), "", "no keys");
        assert_eq!(parse("Super+1").unwrap().unwrap().key.as_deref(), Some("1"));
    }

    #[test]
    fn keys_that_cannot_be_read_say_why() {
        assert!(parse("Super+Q+W").unwrap_err().contains("not a modifier"));
        assert!(parse("Super+Super+Q").unwrap_err().contains("named twice"));
        assert!(parse("Super+Hyper").unwrap_err().contains("not a key"));
        assert!(parse("Super+é").unwrap_err().contains("not a key"));
    }

    #[test]
    fn every_default_reads_and_no_two_share_keys() {
        let mut seen = Vec::new();
        for a in ACTIONS {
            let keys = parse(a.default).unwrap().unwrap();
            assert_eq!(
                keys.to_string(),
                a.default,
                "{} is written the one way",
                a.name
            );
            assert!(!seen.contains(&keys), "{} shares {keys}", a.name);
            seen.push(keys);
        }
        assert!(check(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn check_refuses_a_conflict_an_unbound_way_out_and_an_unknown_action() {
        let lines = check(&file(&[("open_terminal", "Super+Q")]));
        assert_eq!(
            lines,
            ["shortcuts.open_terminal: Super+Q is already close_window's"]
        );
        let lines = check(&file(&[("close_window", "")]));
        assert_eq!(
            lines,
            [
                "shortcuts.close_window: a way out must keep its keys; unset it to go back to Super+Q"
            ]
        );
        let lines = check(&file(&[("cloze", "Super+W")]));
        assert_eq!(
            lines,
            ["shortcuts.cloze: unknown action; docs/SHORTCUTS.md lists them"]
        );
        // Moving close off Super+Q frees it, and unbinding others is fine.
        assert!(
            check(&file(&[
                ("close_window", "Super+W"),
                ("open_terminal", "Super+Q"),
                ("switch_windows", "")
            ]))
            .is_empty()
        );
    }

    #[test]
    fn a_reader_keeps_a_way_out_and_gives_a_contested_key_to_it() {
        let keys = |resolved: &[(&Action, Option<Keys>)], name: &str| {
            resolved
                .iter()
                .find(|(a, _)| a.name == name)
                .and_then(|(_, k)| k.as_ref().map(Keys::to_string))
        };
        let (resolved, notes) = resolve(&file(&[("close_window", "Super+W")]));
        assert_eq!(keys(&resolved, "close_window").as_deref(), Some("Super+W"));
        assert!(notes.is_empty(), "{notes:?}");
        // The terminal asks for close's keys: close keeps them.
        let (resolved, notes) = resolve(&file(&[("open_terminal", "Super+Q")]));
        assert_eq!(keys(&resolved, "close_window").as_deref(), Some("Super+Q"));
        assert_eq!(keys(&resolved, "open_terminal"), None);
        assert_eq!(
            notes,
            ["shortcuts.open_terminal: Super+Q belongs to another action, so it has no keys"]
        );
        // A way out unbound gets its default back; an action the file set
        // wins a default's keys.
        let (resolved, notes) =
            resolve(&file(&[("lock_screen", ""), ("switch_windows", "Super+T")]));
        assert_eq!(keys(&resolved, "lock_screen").as_deref(), Some("Super+L"));
        assert_eq!(
            keys(&resolved, "switch_windows").as_deref(),
            Some("Super+T")
        );
        assert_eq!(keys(&resolved, "toggle_tiling"), None);
        assert_eq!(notes.len(), 2, "{notes:?}");
        let (_, notes) = resolve(&file(&[
            ("cloze", "Super+W"),
            ("toggle_tiling", "Super+Q+W"),
        ]));
        assert_eq!(notes.len(), 2, "{notes:?}");
    }

    #[test]
    fn the_shortcuts_page_is_generated_from_the_table() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/SHORTCUTS.md");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            std::fs::write(path, markdown()).unwrap();
        }
        let page = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            page == markdown(),
            "docs/SHORTCUTS.md is not what ACTIONS gives; run EDEL_WRITE_DOCS=1 cargo test -p edel shortcuts"
        );
    }
}
