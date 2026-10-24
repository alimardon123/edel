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
    screen_off: u32,
    sleep_after: u32,
    on_battery: String,
    saver: String,
}

/// The minutes a whole-minute key of the settings files gives, checked
/// against its range; its default when the files lack it or give a number
/// out of range (M7.8, M7.16b).
fn minutes(key: &str, default: u32, machine: Option<&str>, person: Option<&str>) -> u32 {
    let (least, most) = settings::whole_range(key).unwrap_or((0, i64::from(u32::MAX)));
    settings::chosen(key, machine, person)
        .and_then(|text| text.parse::<i64>().ok())
        .filter(|minutes| (least..=most).contains(minutes))
        .and_then(|minutes| u32::try_from(minutes).ok())
        .unwrap_or(default)
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
        Rules {
            lid: one_of(settings::LID_CLOSE, settings::LID_CLOSE_DEFAULT),
            button: one_of(settings::POWER_BUTTON, settings::POWER_BUTTON_DEFAULT),
            lock_after: minutes(
                settings::LOCK_AFTER_MINUTES,
                settings::LOCK_AFTER_MINUTES_DEFAULT,
                machine,
                person,
            ),
            screen_off: minutes(
                settings::SCREEN_OFF_MINUTES,
                settings::SCREEN_OFF_MINUTES_DEFAULT,
                machine,
                person,
            ),
            sleep_after: minutes(
                settings::SLEEP_AFTER_MINUTES,
                settings::SLEEP_AFTER_MINUTES_DEFAULT,
                machine,
                person,
            ),
            on_battery: one_of(settings::ON_BATTERY, settings::ON_BATTERY_DEFAULT),
            saver: one_of(settings::SAVER, settings::SAVER_DEFAULT),
        }
    }

    /// The minutes without input before the screen locks; 0 for never.
    pub fn lock_after(&self) -> u32 {
        self.lock_after
    }

    /// Whether `power.saver` is `auto`, so a saver that is on came from the
    /// battery and stays on until the charger (M8.31).
    pub fn saver_auto(&self) -> bool {
        self.saver == "auto"
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

/// Whether the battery saver is on now (M8.31). `on` and `off` are the
/// person's choice whatever the battery does. `auto`, the default, is on
/// while the computer runs on battery at [`settings::SAVER_AT_PERCENT`] or
/// less, and once on it stays on until the charger is plugged in (`was_on`,
/// the last decision), so 20 and 21 percent cannot flap it. With no battery
/// (`None`, `None`) it is off.
pub fn saver(rules: &Rules, on_battery: Option<bool>, percent: Option<u32>, was_on: bool) -> bool {
    match rules.saver.as_str() {
        "on" => true,
        "off" => false,
        _ => {
            on_battery == Some(true)
                && (was_on || percent.is_some_and(|p| p <= settings::SAVER_AT_PERCENT))
        }
    }
}

/// The power profile the machine should use now: `saver` (M8.31) is
/// power-saver first, before the battery and a game, as the person chose to
/// save power (`auto` is never on at the charger, so only an explicit `on`
/// beats a game). Else on battery the rules' choice; plugged in,
/// Performance while a game plays (`game`, M7.7b) or when that is the
/// choice, else Balanced. With no word on the power, the machine counts as
/// plugged in.
pub fn profile(rules: &Rules, on_battery: Option<bool>, game: bool, saver: bool) -> &str {
    match on_battery {
        _ if saver => "power-saver",
        Some(true) => &rules.on_battery,
        _ if game || rules.on_battery == "performance" => "performance",
        _ => "balanced",
    }
}

/// One step of the idle ladder (M7.16b): the backlight dims, the screens go
/// off, the screen locks, or the computer sleeps on battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Dim,
    ScreensOff,
    Lock,
    Sleep,
}

/// How long before the screens go off the backlight dims, in milliseconds.
const DIM_BEFORE_MS: u32 = 30_000;

/// The steps idle time brings and when, in milliseconds, in order (M7.16b):
/// dimming 30 s before the screens go off (at once when that is under 30 s),
/// the screens off, the lock, and on battery the sleep. Nothing that is 0
/// (never). While a game plays only the lock is kept.
pub fn idle_stages(rules: &Rules, on_battery: Option<bool>, game: bool) -> Vec<(u32, Stage)> {
    let ms = |minutes: u32| minutes.saturating_mul(60_000);
    let lock = (rules.lock_after > 0).then(|| (ms(rules.lock_after), Stage::Lock));
    if game {
        return lock.into_iter().collect();
    }
    let mut stages = Vec::new();
    if rules.screen_off > 0 {
        let off = ms(rules.screen_off);
        stages.push((off.saturating_sub(DIM_BEFORE_MS), Stage::Dim));
        stages.push((off, Stage::ScreensOff));
    }
    stages.extend(lock);
    if on_battery == Some(true) && rules.sleep_after > 0 {
        stages.push((ms(rules.sleep_after), Stage::Sleep));
    }
    stages.sort_by_key(|(at, _)| *at);
    stages
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
        assert_eq!(rules.saver, "auto");
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
        assert_eq!(profile(&balanced, Some(true), false, false), "balanced");
        assert_eq!(profile(&balanced, Some(false), false, false), "balanced");
        assert_eq!(profile(&balanced, None, false, false), "balanced");
        let saver = rules("[power]\non_battery = \"power-saver\"\n");
        assert_eq!(profile(&saver, Some(true), false, false), "power-saver");
        assert_eq!(profile(&saver, Some(false), false, false), "balanced");
        let fast = rules("[power]\non_battery = \"performance\"\n");
        assert_eq!(profile(&fast, Some(true), false, false), "performance");
        assert_eq!(
            profile(&fast, Some(false), false, false),
            "performance",
            "plugged in, Performance when chosen"
        );
    }

    #[test]
    fn a_game_on_the_charger_asks_for_full_power_and_battery_still_decides() {
        let balanced = Rules::read(None, None);
        assert_eq!(profile(&balanced, Some(false), true, false), "performance");
        assert_eq!(
            profile(&balanced, None, true, false),
            "performance",
            "a machine with no battery counts as plugged in"
        );
        assert_eq!(profile(&balanced, Some(true), true, false), "balanced");
        let saver = rules("[power]\non_battery = \"power-saver\"\n");
        assert_eq!(profile(&saver, Some(true), true, false), "power-saver");
        assert_eq!(profile(&saver, Some(false), true, false), "performance");
        assert_eq!(
            profile(&saver, Some(false), false, false),
            "balanced",
            "the rules' profile comes back when the game ends"
        );
    }

    #[test]
    fn the_idle_lock_is_minutes_in_milliseconds_or_none_for_never() {
        let lock_ms = |text: &str| {
            idle_stages(&rules(text), Some(false), false)
                .into_iter()
                .find(|(_, stage)| *stage == Stage::Lock)
                .map(|(ms, _)| ms)
        };
        assert_eq!(lock_ms(""), Some(300_000));
        assert_eq!(lock_ms("[power]\nlock_after_minutes = 0\n"), None);
        assert_eq!(lock_ms("[power]\nlock_after_minutes = 1\n"), Some(60_000));
        assert_eq!(
            lock_ms("[power]\nlock_after_minutes = 1440\n"),
            Some(86_400_000)
        );
    }

    #[test]
    fn on_the_charger_the_idle_ladder_dims_then_turns_the_screens_off_and_locks() {
        let stages = idle_stages(&Rules::read(None, None), Some(false), false);
        assert_eq!(
            stages,
            [
                (270_000, Stage::Dim),
                (300_000, Stage::ScreensOff),
                (300_000, Stage::Lock),
            ],
            "no sleep on the charger"
        );
    }

    #[test]
    fn on_battery_the_ladder_adds_the_sleep_last() {
        let stages = idle_stages(&Rules::read(None, None), Some(true), false);
        assert_eq!(stages.last(), Some(&(900_000, Stage::Sleep)));
        assert_eq!(stages.len(), 4);
        assert!(
            idle_stages(&Rules::read(None, None), None, false)
                .iter()
                .all(|(_, stage)| *stage != Stage::Sleep)
        );
    }

    #[test]
    fn a_zero_leaves_its_step_out_and_no_screen_off_drops_the_dim_too() {
        let off = rules("[power]\nscreen_off_minutes = 0\nlock_after_minutes = 0\n");
        assert_eq!(idle_stages(&off, Some(false), false), []);
        let sleepless = rules("[power]\nsleep_after_minutes = 0\n");
        assert!(
            idle_stages(&sleepless, Some(true), false)
                .iter()
                .all(|(_, stage)| *stage != Stage::Sleep)
        );
        let no_dim = rules("[power]\nscreen_off_minutes = 0\n");
        assert_eq!(
            idle_stages(&no_dim, Some(false), false),
            [(300_000, Stage::Lock)]
        );
    }

    #[test]
    fn a_game_keeps_only_the_lock() {
        let battery = rules("[power]\nlock_after_minutes = 2\n");
        assert_eq!(
            idle_stages(&battery, Some(true), true),
            [(120_000, Stage::Lock)]
        );
    }

    #[test]
    fn the_saver_is_auto_by_the_battery_at_twenty_percent_and_latches_until_the_charger() {
        let auto = Rules::read(None, None);
        assert!(
            saver(&auto, Some(true), Some(20), false),
            "20 percent on battery"
        );
        assert!(
            !saver(&auto, Some(true), Some(21), false),
            "21 percent on battery"
        );
        assert!(
            !saver(&auto, Some(false), Some(5), false),
            "5 percent on the charger"
        );
        assert!(
            saver(&auto, Some(true), Some(21), true),
            "latched at 21 percent"
        );
        assert!(
            !saver(&auto, Some(false), Some(21), true),
            "the charger ends the latch"
        );
        assert!(!saver(&auto, None, None, false), "no battery is off");
        assert!(
            !saver(&auto, None, None, true),
            "no battery is off even when latched"
        );
    }

    #[test]
    fn the_saver_on_and_off_ignore_the_battery() {
        let on = rules("[power]\nsaver = \"on\"\n");
        assert!(saver(&on, Some(false), Some(90), false), "on the charger");
        let off = rules("[power]\nsaver = \"off\"\n");
        assert!(
            !saver(&off, Some(true), Some(5), false),
            "5 percent on battery"
        );
        assert!(
            !saver(&off, Some(true), Some(5), true),
            "off ends any latch"
        );
    }

    #[test]
    fn the_saver_s_profile_is_power_saver_before_the_battery_and_a_game() {
        let auto = Rules::read(None, None);
        assert_eq!(
            profile(&auto, Some(false), false, true),
            "power-saver",
            "on the charger"
        );
        assert_eq!(
            profile(&auto, Some(true), false, true),
            "power-saver",
            "on battery"
        );
        assert_eq!(
            profile(&auto, Some(false), true, true),
            "power-saver",
            "with a game"
        );
        let fast = rules("[power]\non_battery = \"performance\"\n");
        assert_eq!(profile(&fast, Some(true), false, true), "power-saver");
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
