//! Game mode's rule (roadmap M7.7), pure and tested: when game mode is on.
//! `power.game_mode` says when (`plugged-in`, the default, `always` or
//! `off`), and Super+G turns it on or off over the key until the next
//! login or until the key changes. In game mode a fullscreen window gets
//! variable refresh where its screen offers it (`games.rs` in the binary,
//! `drm.rs`). The gaming profile (M7.6) sets the key to `always`.

/// `power.game_mode`: when game mode is on by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum When {
    /// While the machine is plugged in, or has no battery.
    #[default]
    PluggedIn,
    Always,
    Off,
}

impl When {
    /// The key's value as the settings file writes it; anything else is
    /// the default, as a lenient reader keeps it.
    pub fn parse(value: Option<&str>) -> When {
        match value {
            Some("always") => When::Always,
            Some("off") => When::Off,
            _ => When::PluggedIn,
        }
    }
}

/// Game mode: the key's rule and Super+G's choice over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameMode {
    when: When,
    /// What Super+G chose, until the next login or the key's change.
    toggled: Option<bool>,
}

impl GameMode {
    /// Follows the key; a new value drops Super+G's choice, as the person
    /// chose anew.
    pub fn set_when(&mut self, when: When) {
        if self.when != when {
            self.when = when;
            self.toggled = None;
        }
    }

    /// Whether game mode is on; `on_battery` is asked only when the key's
    /// rule needs it.
    pub fn on(&self, on_battery: impl FnOnce() -> bool) -> bool {
        self.toggled.unwrap_or_else(|| match self.when {
            When::Always => true,
            When::Off => false,
            When::PluggedIn => !on_battery(),
        })
    }

    /// Super+G: game mode the other way round; gives whether it is on now.
    pub fn toggle(&mut self, on_battery: impl FnOnce() -> bool) -> bool {
        let now = !self.on(on_battery);
        self.toggled = Some(now);
        now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_reads_leniently() {
        assert_eq!(When::parse(None), When::PluggedIn);
        assert_eq!(When::parse(Some("always")), When::Always);
        assert_eq!(When::parse(Some("off")), When::Off);
        assert_eq!(When::parse(Some("turbo")), When::PluggedIn);
    }

    #[test]
    fn plugged_in_is_on_off_the_battery() {
        let game = GameMode::default();
        assert!(game.on(|| false));
        assert!(!game.on(|| true));
    }

    #[test]
    fn always_and_off_never_ask_for_the_battery() {
        let mut game = GameMode::default();
        game.set_when(When::Always);
        assert!(game.on(|| unreachable!()));
        game.set_when(When::Off);
        assert!(!game.on(|| unreachable!()));
    }

    #[test]
    fn super_g_turns_it_the_other_way_until_the_key_changes() {
        let mut game = GameMode::default();
        assert!(!game.toggle(|| false));
        assert!(!game.on(|| false));
        assert!(game.toggle(|| unreachable!()));
        assert!(game.on(|| unreachable!()));
        game.set_when(When::PluggedIn);
        assert!(
            game.on(|| unreachable!()),
            "the same value keeps the choice"
        );
        game.set_when(When::Off);
        assert!(!game.on(|| unreachable!()));
    }
}
