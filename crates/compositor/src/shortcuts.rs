//! Keyboard shortcuts on screen (roadmap M5.13a): `[shortcuts]` from both
//! system files, laid over the defaults by `edel::shortcuts::resolve`, the
//! one table `edel system check` and Settings use, and turned into the
//! keysyms the keyboard reports. Only actions whose step has landed are
//! bound here; the keys of the others (launcher, switcher, workspaces,
//! screenshot, lock) reach the app that has the keyboard until then.
//! Keys match on the Latin layout's keysym, whatever the layout.

use std::collections::BTreeMap;

use smithay::input::keyboard::{ModifiersState, xkb};

use edel::shortcuts::{Keys, resolve};

/// What a shortcut does in the compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Close,
    Tiling,
    Terminal,
}

impl Act {
    /// The action called `name`, if it does anything yet.
    fn of(name: &str) -> Option<Act> {
        match name {
            "close" => Some(Act::Close),
            "tiling" => Some(Act::Tiling),
            "terminal" => Some(Act::Terminal),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Act::Close => "close",
            Act::Tiling => "tiling",
            Act::Terminal => "terminal",
        }
    }
}

/// An action on keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub act: Act,
    pub keys: Keys,
    sym: xkb::Keysym,
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
        // A modifier tapped alone (the launcher's Super) waits for M5.3.
        let Some(name) = &keys.key else {
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
        bindings.push(Binding { act, keys, sym });
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
    bindings
        .iter()
        .find(|b| {
            b.sym == latin
                && b.keys.logo == modifiers.logo
                && b.keys.ctrl == modifiers.ctrl
                && b.keys.alt == modifiers.alt
                && b.keys.shift == modifiers.shift
        })
        .map(|b| b.act)
}

/// What changed from `old` to `new`, one line per action, for the log.
pub fn changes(old: &[Binding], new: &[Binding]) -> Vec<String> {
    let keys_of = |list: &[Binding], act: Act| {
        list.iter()
            .find(|b| b.act == act)
            .map(|b| b.keys.to_string())
    };
    [Act::Close, Act::Tiling, Act::Terminal]
        .into_iter()
        .filter_map(|act| {
            let (before, after) = (keys_of(old, act), keys_of(new, act));
            (before != after).then(|| match after {
                Some(keys) => format!("shortcut {} is {keys}", act.name()),
                None => format!("shortcut {} has no keys", act.name()),
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
        // Actions still to come (switcher, workspaces) bind nothing yet.
        assert_eq!(
            find(&bindings, &held(false, false, true, false), sym("Tab")),
            None
        );
        assert_eq!(find(&bindings, &super_, sym("1")), None);
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
