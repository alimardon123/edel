//! Keyboard shortcuts on screen (roadmap M5.13a): `[shortcuts]` from both
//! settings files, laid over the defaults by `edel::shortcuts::resolve`, the
//! one table `edel settings check` and Settings use, and turned into the
//! keysyms the keyboard reports. Only actions whose step has landed are
//! bound here; the keys of the others (screenshot, lock) reach
//! the app that has the keyboard until then. Keys match on the Latin
//! layout's keysym, whatever the layout. A modifier alone, such as the
//! launcher's `Super` (M5.3b), is a tap: pressed and let go with nothing
//! else between, which `tapped` matches on the release.

use std::collections::BTreeMap;

use smithay::input::keyboard::{ModifiersState, xkb};

use edel::shortcuts::{Keys, resolve};

/// What a shortcut does in the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Close,
    Tiling,
    Terminal,
    /// Go to workspace N, from 0 (M5.2a).
    Workspace(usize),
    /// Move the focused window to workspace N, from 0.
    MoveTo(usize),
    /// Show or hide shell-ui's launcher (M5.3b).
    Launcher,
    /// The next window in the switcher, held open while the keys'
    /// modifiers are (M5.3c); with Shift as well, the one before.
    Switcher,
    SwitcherBack,
}

impl Act {
    /// The action called `name`, if it does anything yet.
    fn of(name: &str) -> Option<Act> {
        let number = |rest: &str| match rest.parse::<usize>() {
            Ok(n @ 1..=9) => Some(n - 1),
            _ => None,
        };
        match name {
            "close" => Some(Act::Close),
            "launcher" => Some(Act::Launcher),
            "switcher" => Some(Act::Switcher),
            "tiling" => Some(Act::Tiling),
            "terminal" => Some(Act::Terminal),
            _ => {
                if let Some(rest) = name.strip_prefix("move_to_workspace_") {
                    number(rest).map(Act::MoveTo)
                } else {
                    number(name.strip_prefix("workspace_")?).map(Act::Workspace)
                }
            }
        }
    }
}

/// An action on keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub act: Act,
    pub keys: Keys,
    /// The key with the modifiers; none for a modifier tapped alone.
    sym: Option<xkb::Keysym>,
}

/// The bindings `[shortcuts]` (action to keys, as written) gives, and
/// notes on what was left out.
pub fn bind(file: &BTreeMap<String, String>) -> (Vec<Binding>, Vec<String>) {
    let (resolved, mut notes) = resolve(file);
    let mut bindings = Vec::new();
    for (action, keys) in resolved {
        let (Some(act), Some(keys)) = (Act::of(action.name), keys) else {
            continue;
        };
        let Some(name) = &keys.key else {
            // A modifier tapped alone: one modifier, as two cannot be let
            // go of at once.
            let held = [keys.logo, keys.ctrl, keys.alt, keys.shift];
            if held.iter().filter(|h| **h).count() == 1 {
                bindings.push(Binding {
                    act,
                    keys,
                    sym: None,
                });
            } else {
                notes.push(format!(
                    "shortcuts.{}: only one modifier can be tapped alone",
                    action.name
                ));
            }
            continue;
        };
        let sym = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
        if sym.raw() == xkb::keysyms::KEY_NoSymbol {
            notes.push(format!(
                "shortcuts.{}: no key is called {name}",
                action.name
            ));
            continue;
        }
        bindings.push(Binding {
            act,
            keys,
            sym: Some(sym),
        });
    }
    (bindings, notes)
}

/// The action the pressed key `latin` (its Latin layout's keysym) does
/// with `modifiers` held, if any.
pub fn find(
    bindings: &[Binding],
    modifiers: &ModifiersState,
    latin: Option<xkb::Keysym>,
) -> Option<Act> {
    let latin = latin?;
    find_exact(bindings, modifiers, latin).or_else(|| {
        // The switcher's keys with Shift as well go back.
        let without = ModifiersState {
            shift: false,
            ..*modifiers
        };
        (modifiers.shift && find_exact(bindings, &without, latin) == Some(Act::Switcher))
            .then_some(Act::SwitcherBack)
    })
}

fn find_exact(bindings: &[Binding], modifiers: &ModifiersState, latin: xkb::Keysym) -> Option<Act> {
    bindings
        .iter()
        .find(|b| {
            b.sym == Some(latin)
                && b.keys.logo == modifiers.logo
                && b.keys.ctrl == modifiers.ctrl
                && b.keys.alt == modifiers.alt
                && b.keys.shift == modifiers.shift
        })
        .map(|b| b.act)
}

/// Whether the switcher's modifiers are still held: it stays open until
/// they are all let go. Keys without modifiers hold it for one press.
pub fn holds_switcher(bindings: &[Binding], modifiers: &ModifiersState) -> bool {
    bindings
        .iter()
        .find(|b| b.act == Act::Switcher)
        .is_some_and(|b| {
            (b.keys.logo && modifiers.logo)
                || (b.keys.ctrl && modifiers.ctrl)
                || (b.keys.alt && modifiers.alt)
                || (b.keys.shift && modifiers.shift)
        })
}

/// Whether `sym` is a modifier that can be tapped alone.
pub fn is_tap_key(sym: xkb::Keysym) -> bool {
    use xkb::keysyms as k;
    matches!(
        sym.raw(),
        k::KEY_Super_L
            | k::KEY_Super_R
            | k::KEY_Control_L
            | k::KEY_Control_R
            | k::KEY_Alt_L
            | k::KEY_Alt_R
            | k::KEY_Shift_L
            | k::KEY_Shift_R
    )
}

/// The action the modifier `sym`, tapped alone, does, if any.
pub fn tapped(bindings: &[Binding], sym: xkb::Keysym) -> Option<Act> {
    use xkb::keysyms as k;
    let alone = |logo, ctrl, alt, shift| Keys {
        logo,
        ctrl,
        alt,
        shift,
        key: None,
    };
    let keys = match sym.raw() {
        k::KEY_Super_L | k::KEY_Super_R => alone(true, false, false, false),
        k::KEY_Control_L | k::KEY_Control_R => alone(false, true, false, false),
        k::KEY_Alt_L | k::KEY_Alt_R => alone(false, false, true, false),
        k::KEY_Shift_L | k::KEY_Shift_R => alone(false, false, false, true),
        _ => return None,
    };
    bindings
        .iter()
        .find(|b| b.sym.is_none() && b.keys == keys)
        .map(|b| b.act)
}

/// What changed from `old` to `new`, one line per action, for the log.
pub fn changes(old: &[Binding], new: &[Binding]) -> Vec<String> {
    let keys_of = |list: &[Binding], act: Act| {
        list.iter()
            .find(|b| b.act == act)
            .map(|b| b.keys.to_string())
    };
    edel::shortcuts::ACTIONS
        .iter()
        .filter_map(|action| {
            let act = Act::of(action.name)?;
            let (before, after) = (keys_of(old, act), keys_of(new, act));
            (before != after).then(|| match after {
                Some(keys) => format!("shortcut {} is {keys}", action.name),
                None => format!("shortcut {} has no keys", action.name),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(logo: bool, ctrl: bool, alt: bool, shift: bool) -> ModifiersState {
        ModifiersState {
            logo,
            ctrl,
            alt,
            shift,
            ..Default::default()
        }
    }

    fn sym(name: &str) -> Option<xkb::Keysym> {
        Some(xkb::keysym_from_name(name, 0))
    }

    #[test]
    fn the_defaults_close_tile_and_open_a_terminal() {
        let (bindings, notes) = bind(&BTreeMap::new());
        assert!(notes.is_empty(), "{notes:?}");
        let super_ = held(true, false, false, false);
        assert_eq!(find(&bindings, &super_, sym("q")), Some(Act::Close));
        assert_eq!(find(&bindings, &super_, sym("t")), Some(Act::Tiling));
        let ctrl_alt = held(false, true, true, false);
        assert_eq!(find(&bindings, &ctrl_alt, sym("t")), Some(Act::Terminal));
        // Other modifiers, or none, leave the key to the app.
        assert_eq!(
            find(&bindings, &held(true, false, false, true), sym("q")),
            None
        );
        assert_eq!(
            find(&bindings, &held(false, false, false, false), sym("q")),
            None
        );
        // Super+N goes to workspace N, Super+Shift+N takes the window.
        assert_eq!(find(&bindings, &super_, sym("1")), Some(Act::Workspace(0)));
        assert_eq!(find(&bindings, &super_, sym("9")), Some(Act::Workspace(8)));
        let super_shift = held(true, false, false, true);
        assert_eq!(
            find(&bindings, &super_shift, sym("2")),
            Some(Act::MoveTo(1))
        );
        // Alt+Tab steps the switcher, with Shift back; Alt alone holds it.
        let alt = held(false, false, true, false);
        assert_eq!(find(&bindings, &alt, sym("Tab")), Some(Act::Switcher));
        assert_eq!(
            find(&bindings, &held(false, false, true, true), sym("Tab")),
            Some(Act::SwitcherBack)
        );
        assert_eq!(
            find(&bindings, &held(false, true, true, false), sym("Tab")),
            None
        );
        assert!(holds_switcher(&bindings, &alt));
        assert!(!holds_switcher(&bindings, &held(false, false, false, true)));
    }

    #[test]
    fn super_tapped_alone_opens_the_launcher() {
        let (bindings, _) = bind(&BTreeMap::new());
        let tap = |name: &str| tapped(&bindings, xkb::keysym_from_name(name, 0));
        assert_eq!(tap("Super_L"), Some(Act::Launcher));
        assert_eq!(tap("Super_R"), Some(Act::Launcher));
        assert_eq!(tap("Alt_L"), None);
        assert_eq!(tap("q"), None);
        assert!(is_tap_key(xkb::keysym_from_name("Super_L", 0)));
        assert!(!is_tap_key(xkb::keysym_from_name("a", 0)));
        // Moved to Super+Space, it is a key like any other; two modifiers
        // alone cannot be tapped.
        let file = BTreeMap::from([("launcher".to_string(), "Super+Space".to_string())]);
        let (moved, notes) = bind(&file);
        assert!(notes.is_empty(), "{notes:?}");
        let super_ = held(true, false, false, false);
        assert_eq!(find(&moved, &super_, sym("space")), Some(Act::Launcher));
        assert_eq!(tapped(&moved, xkb::keysym_from_name("Super_L", 0)), None);
        let file = BTreeMap::from([("launcher".to_string(), "Ctrl+Alt".to_string())]);
        let (_, notes) = bind(&file);
        assert!(
            notes.iter().any(|n| n.contains("only one modifier")),
            "{notes:?}"
        );
    }

    #[test]
    fn a_rebound_close_moves_and_the_log_says_so() {
        let (old, _) = bind(&BTreeMap::new());
        let file = BTreeMap::from([("close".to_string(), "Super+W".to_string())]);
        let (new, notes) = bind(&file);
        assert!(notes.is_empty(), "{notes:?}");
        let super_ = held(true, false, false, false);
        assert_eq!(find(&new, &super_, sym("w")), Some(Act::Close));
        assert_eq!(
            find(&new, &super_, sym("q")),
            None,
            "Super+Q no longer closes"
        );
        assert_eq!(changes(&old, &new), ["shortcut close is Super+W"]);
        assert!(changes(&new, &new).is_empty());
        let unbound = BTreeMap::from([("terminal".to_string(), String::new())]);
        assert_eq!(
            changes(&old, &bind(&unbound).0),
            ["shortcut terminal has no keys"]
        );
    }
}
