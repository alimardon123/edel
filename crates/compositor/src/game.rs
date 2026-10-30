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

/// How many times larger a game is shown than it is drawn (M8.28), when
/// that is a whole number of at least 2, the same across and down: such a
/// game is scaled pixel for pixel, sharp. `drawn` is the buffer's part
/// shown, in its own pixels; `shown` the size it covers on the screen, in
/// the screen's pixels. Anything else is scaled smoothly, as other windows
/// are.
pub fn whole_upscale(drawn: (f64, f64), shown: (f64, f64)) -> Option<u32> {
    if drawn.0 < 1.0 || drawn.1 < 1.0 {
        return None;
    }
    let (across, down) = (shown.0 / drawn.0, shown.1 / drawn.1);
    let times = across.round();
    ((across - down).abs() < 0.01 && (across - times).abs() < 0.01 && times >= 2.0)
        .then_some(times as u32)
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
    fn the_values_are_the_keys_own() {
        use edel::settings::{GAME_MODE, GAME_MODE_DEFAULT, choices};
        assert_eq!(choices(GAME_MODE), ["plugged-in", "always", "off"]);
        assert_eq!(When::parse(Some(GAME_MODE_DEFAULT)), When::default());
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

    #[test]
    fn a_game_drawn_at_half_size_is_scaled_by_whole_pixels() {
        assert_eq!(whole_upscale((640.0, 400.0), (1280.0, 800.0)), Some(2));
        assert_eq!(whole_upscale((427.0, 240.0), (1280.0, 720.0)), Some(3));
        assert_eq!(
            whole_upscale((1280.0, 800.0), (1280.0, 800.0)),
            None,
            "drawn at full size"
        );
        assert_eq!(
            whole_upscale((854.0, 480.0), (1280.0, 720.0)),
            None,
            "one and a half times"
        );
        assert_eq!(
            whole_upscale((640.0, 480.0), (1280.0, 800.0)),
            None,
            "stretched out of shape"
        );
        assert_eq!(whole_upscale((0.0, 0.0), (1280.0, 800.0)), None);
    }
}
