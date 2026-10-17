//! What the lid, the power button, the idle lock and the power profile do
//! (M7.8): the rules the settings files give, read as the rest of shell-ui
//! reads them, and the choice each event makes. Pure: no display, no bus,
//! no command, so the tests run anywhere. Where `edel` is not installed, as
//! on another distribution, the lid and the power button are left to logind
//! and the profile is not touched; the idle lock still works there.

use edel::settings;

/// What an event asks shell-ui to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    /// Nothing.
    Nothing,
    /// Lock the screen.
    Lock,
    /// Suspend: lock the screen first, then suspend once the lock is drawn
    /// (in the greeter, suspend straight).
    Suspend,
    /// Shut down.
    PowerOff,
    /// Lock the screen and open Power's menu on it.
    Ask,
}

impl Do {
    /// The word the log says, as in `edel-shell-ui: lid closed: NAME`.
    pub fn name(self) -> &'static str {
        match self {
            Do::Nothing => "nothing",
            Do::Lock => "lock",
            Do::Suspend => "suspend",
            Do::PowerOff => "shutdown",
            Do::Ask => "ask",
        }
    }
}

/// The power rules the settings files give, each already checked against
/// its key's values; a key the files lack, or give a value that is not one
/// of its values, has its default (ADR-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    lid: String,
    button: String,
    lock_after: u32,
    on_battery: String,
}

impl Rules {
    /// The rules of the machine's and the person's settings files, the
    /// person's first.
    pub fn read(machine: Option<&str>, person: Option<&str>) -> Rules {
        let one_of = |key: &str, default: &str| {
            settings::chosen(key, machine, person)
                .filter(|value| settings::choices(key).contains(&value.as_str()))
                .unwrap_or_else(|| default.to_string())
        };
        let (least, most) =
            settings::whole_range(settings::LOCK_AFTER_MINUTES).unwrap_or((0, i64::from(u32::MAX)));
        let lock_after = settings::chosen(settings::LOCK_AFTER_MINUTES, machine, person)
            .and_then(|text| text.parse::<i64>().ok())
            .filter(|minutes| (least..=most).contains(minutes))
            .and_then(|minutes| u32::try_from(minutes).ok())
            .unwrap_or(settings::LOCK_AFTER_MINUTES_DEFAULT);
        Rules {
            lid: one_of(settings::LID_CLOSE, settings::LID_CLOSE_DEFAULT),
            button: one_of(settings::POWER_BUTTON, settings::POWER_BUTTON_DEFAULT),
            lock_after,
            on_battery: one_of(settings::ON_BATTERY, settings::ON_BATTERY_DEFAULT),
        }
    }

    /// The minutes without input before the screen locks; 0 for never.
    pub fn lock_after(&self) -> u32 {
        self.lock_after
    }
}

/// What closing the lid does. With two or more screens lit (a docked
/// laptop) it does nothing, so the desks keep working.
pub fn on_lid(rules: &Rules, screens: usize, edel_here: bool) -> Do {
    if !edel_here || screens >= 2 {
        return Do::Nothing;
    }
    match rules.lid.as_str() {
        "suspend" => Do::Suspend,
        "lock" => Do::Lock,
        "poweroff" => Do::PowerOff,
        _ => Do::Nothing,
    }
}

/// What the power button does.
pub fn on_button(rules: &Rules, edel_here: bool) -> Do {
    if !edel_here {
        return Do::Nothing;
    }
    match rules.button.as_str() {
        "suspend" => Do::Suspend,
        "poweroff" => Do::PowerOff,
        "ask" => Do::Ask,
        _ => Do::Nothing,
    }
}

/// The power profile the machine should use now: on battery the rules'
/// choice; plugged in, Performance when that is the choice, else Balanced.
/// With no word on the power, the machine counts as plugged in.
pub fn profile(rules: &Rules, on_battery: Option<bool>) -> &str {
    match on_battery {
        Some(true) => &rules.on_battery,
        _ if rules.on_battery == "performance" => "performance",
        _ => "balanced",
    }
}

/// The idle time after which the screen locks, in milliseconds; none for
/// never (0 minutes).
pub fn idle_ms(rules: &Rules) -> Option<u32> {
    (rules.lock_after > 0).then(|| rules.lock_after.saturating_mul(60_000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Rules {
        Rules::read(Some(text), None)
    }

    #[test]
    fn without_settings_every_rule_has_its_default() {
        let rules = Rules::read(None, None);
        assert_eq!(rules.lid, "suspend");
        assert_eq!(rules.button, "suspend");
        assert_eq!(rules.lock_after(), 5);
        assert_eq!(rules.on_battery, "balanced");
    }

    #[test]
    fn the_person_s_file_wins_over_the_machine_s() {
        let machine = "[power]\nlid_close = \"lock\"\n";
        let person = "[power]\nlid_close = \"nothing\"\n";
        let rules = Rules::read(Some(machine), Some(person));
        assert_eq!(rules.lid, "nothing");
        let rules = Rules::read(Some(machine), None);
        assert_eq!(rules.lid, "lock");
    }

    #[test]
    fn a_value_that_is_not_one_of_the_key_s_falls_back() {
        let rules =
            rules("[power]\nlid_close = \"hibernate\"\npower_button = 3\non_battery = \"turbo\"\n");
        assert_eq!(rules.lid, "suspend");
        assert_eq!(rules.button, "suspend");
        assert_eq!(rules.on_battery, "balanced");
    }

    #[test]
    fn the_lock_takes_whole_minutes_from_zero_to_a_day() {
        assert_eq!(rules("[power]\nlock_after_minutes = 0\n").lock_after(), 0);
        assert_eq!(
            rules("[power]\nlock_after_minutes = 1440\n").lock_after(),
            1440
        );
        assert_eq!(
            rules("[power]\nlock_after_minutes = 1441\n").lock_after(),
            5
        );
        assert_eq!(rules("[power]\nlock_after_minutes = -1\n").lock_after(), 5);
        assert_eq!(
            rules("[power]\nlock_after_minutes = \"ten\"\n").lock_after(),
            5
        );
    }

    #[test]
    fn the_lid_closing_suspends_with_one_screen_and_the_rules_say_so() {
        let suspend = Rules::read(None, None);
        assert_eq!(on_lid(&suspend, 1, true), Do::Suspend);
        let lock = rules("[power]\nlid_close = \"lock\"\n");
        assert_eq!(on_lid(&lock, 1, true), Do::Lock);
        let off = rules("[power]\nlid_close = \"poweroff\"\n");
        assert_eq!(on_lid(&off, 1, true), Do::PowerOff);
        let nothing = rules("[power]\nlid_close = \"nothing\"\n");
        assert_eq!(on_lid(&nothing, 1, true), Do::Nothing);
    }

    #[test]
    fn a_docked_lid_does_nothing_and_so_does_a_machine_without_edel() {
        let suspend = Rules::read(None, None);
        assert_eq!(on_lid(&suspend, 2, true), Do::Nothing);
        assert_eq!(on_lid(&suspend, 3, true), Do::Nothing);
        assert_eq!(on_lid(&suspend, 1, false), Do::Nothing);
        assert_eq!(on_button(&suspend, false), Do::Nothing);
    }

    #[test]
    fn the_power_button_suspends_by_default_and_asks_when_told() {
        let suspend = Rules::read(None, None);
        assert_eq!(on_button(&suspend, true), Do::Suspend);
        let ask = rules("[power]\npower_button = \"ask\"\n");
        assert_eq!(on_button(&ask, true), Do::Ask);
        let off = rules("[power]\npower_button = \"poweroff\"\n");
        assert_eq!(on_button(&off, true), Do::PowerOff);
        let nothing = rules("[power]\npower_button = \"nothing\"\n");
        assert_eq!(on_button(&nothing, true), Do::Nothing);
    }

    #[test]
    fn the_profile_follows_the_power_and_the_choice_for_battery() {
        let balanced = Rules::read(None, None);
        assert_eq!(profile(&balanced, Some(true)), "balanced");
        assert_eq!(profile(&balanced, Some(false)), "balanced");
        assert_eq!(profile(&balanced, None), "balanced");
        let saver = rules("[power]\non_battery = \"power-saver\"\n");
        assert_eq!(profile(&saver, Some(true)), "power-saver");
        assert_eq!(profile(&saver, Some(false)), "balanced");
        let fast = rules("[power]\non_battery = \"performance\"\n");
        assert_eq!(profile(&fast, Some(true)), "performance");
        assert_eq!(
            profile(&fast, Some(false)),
            "performance",
            "plugged in, Performance when chosen"
        );
    }

    #[test]
    fn the_idle_lock_is_minutes_in_milliseconds_or_none_for_never() {
        assert_eq!(idle_ms(&Rules::read(None, None)), Some(300_000));
        assert_eq!(idle_ms(&rules("[power]\nlock_after_minutes = 0\n")), None);
        assert_eq!(
            idle_ms(&rules("[power]\nlock_after_minutes = 1\n")),
            Some(60_000)
        );
        assert_eq!(
            idle_ms(&rules("[power]\nlock_after_minutes = 1440\n")),
            Some(86_400_000)
        );
    }

    #[test]
    fn the_action_names_are_the_ones_the_log_says() {
        let names: Vec<&str> = [Do::Nothing, Do::Lock, Do::Suspend, Do::PowerOff, Do::Ask]
            .iter()
            .map(|d| d.name())
            .collect();
        assert_eq!(names, ["nothing", "lock", "suspend", "shutdown", "ask"]);
    }
}
