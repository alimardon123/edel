//! The power keys, the lid and the idle ladder (M7.8, M7.16b): the shell's
//! side of `power_rules.rs`. The lid and the power button act here; a time
//! without input dims the light, turns the screens off, locks the screen
//! (`edel-shell-ui: idle for N min, locking`) and on battery sleeps, each
//! step through the compositor's ext-idle-notify, and the power profile the charger and
//! the settings want is asked of `edel power profile`. The commands run on
//! threads of their own, never the drawing one.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
};

use crate::Shell;
use crate::messages;
use crate::power;
use crate::power_rules::{self, Do, Stage};
use crate::status::Cmd;

/// Whether the power profile is kept (M7.8): `edel` is here and the kernel
/// has a profile file. The status is read then, even on a panel without the
/// status area, because a change of the charger reaches shell-ui through it.
pub fn profile_wanted() -> bool {
    crate::on_path(crate::EDEL) && Path::new(edel::profile::PLATFORM_PROFILE).exists()
}

/// Binds the compositor's idle notifier (M7.8); none without it, and the
/// screen then locks by no idle time.
pub fn bind_idle(globals: &GlobalList, qh: &QueueHandle<Shell>) -> Option<ExtIdleNotifierV1> {
    globals
        .bind(qh, 1..=1, IdleEvents)
        .inspect_err(|_| {
            eprintln!(
                "edel-shell-ui: the compositor offers no ext_idle_notifier_v1, so no idle lock"
            )
        })
        .ok()
}

/// The user data of the idle notifier, whose events reach `Shell` (M7.8).
/// Each idle notification has its own, `IdleStage`.
pub struct IdleEvents;

impl Dispatch2<ExtIdleNotifierV1, Shell> for IdleEvents {
    fn event(
        &self,
        _: &mut Shell,
        _: &ExtIdleNotifierV1,
        _: ext_idle_notifier_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
    }
}

/// The user data of one idle notification, which says which step of the
/// idle ladder it is (M7.16b).
pub struct IdleStage(pub Stage);

impl Dispatch2<ExtIdleNotificationV1, Shell> for IdleStage {
    fn event(
        &self,
        shell: &mut Shell,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        // Input came back: whatever dimmed the light gives it back, at any
        // step. A notification destroyed before its resume sends none, so
        // `follow_idle` restores the light itself first.
        if let ext_idle_notification_v1::Event::Idled = event {
            shell.idled(self.0);
        } else {
            shell.undim();
        }
    }
}

/// The light the idle dim turned down, as the kernel counts it, so that
/// `undim` sets back exactly what was there (M7.16b).
pub struct Dimmed {
    /// The screen's raw level.
    pub screen: u32,
    /// Each keyboard light and its raw level.
    pub keyboards: Vec<(PathBuf, u32)>,
}

impl Shell {
    /// The lid moved (M7.8): what `power.lid_close` says happens on a close,
    /// with the number of screens lit; an open lid changes nothing.
    pub fn lid(&mut self, closed: bool) {
        if !closed {
            crate::log_line("edel-shell-ui: lid opened");
            return;
        }
        let screens = self.outputs.outputs().count();
        let action = power_rules::on_lid(&self.rules, screens, crate::on_path(crate::EDEL));
        crate::log_line(&format!("edel-shell-ui: lid closed: {}", action.name()));
        self.act_power(action);
    }

    /// The power button, the `power` media key (M7.8): what
    /// `power.power_button` says happens.
    pub fn power_button(&mut self) {
        let action = power_rules::on_button(&self.rules, crate::on_path(crate::EDEL));
        crate::log_line(&format!("edel-shell-ui: power button: {}", action.name()));
        self.act_power(action);
    }

    /// Does what the rules asked. A suspend locks first and suspends once the
    /// lock is drawn; with the screen locked already it suspends at once.
    fn act_power(&mut self, action: Do) {
        match action {
            Do::Nothing => {}
            Do::Lock => self.lock_screen(),
            Do::Suspend => {
                if self.lock.as_ref().is_some_and(|card| card.locked) {
                    self.run_power(power::Choice::Suspend);
                } else {
                    self.suspend_after_lock = true;
                    self.lock_screen();
                }
            }
            Do::PowerOff => self.run_power(power::Choice::ShutDown),
            Do::Ask => {
                self.lock_screen();
                self.set_power_menu(true);
            }
        }
    }

    /// Carries `choice` out with `power::spawn`, closing Power's menu first
    /// when it is open; a suspend or a shut down holds the others off for
    /// 3 s (M7.8).
    pub(crate) fn run_power(&mut self, choice: power::Choice) {
        if self.lock.as_ref().is_some_and(|card| card.menu) {
            self.set_power_menu(false);
        }
        power::spawn(choice, &self.suspending, "");
    }

    /// One step of the idle ladder ran out (M7.8, M7.16b): the light dims,
    /// the screens go off, the screen locks (unless it is locked already),
    /// or on battery the computer sleeps as the lid's suspend does.
    fn idled(&mut self, stage: Stage) {
        match stage {
            Stage::Dim => self.dim(),
            Stage::ScreensOff => {
                eprintln!("edel-shell-ui: idle, screens off");
                self.link.screens_off();
            }
            Stage::Lock => {
                // The USB stick's own account has no password, so being
                // away locks nothing there; the screens just stay off.
                if self.lock.is_some() || crate::lock::user() == edel::places::LIVE_USER {
                    return;
                }
                eprintln!(
                    "edel-shell-ui: idle for {} min, locking",
                    self.rules.lock_after()
                );
                self.lock_screen();
            }
            Stage::Sleep => {
                if crate::on_path(crate::EDEL) {
                    eprintln!("edel-shell-ui: idle on battery, sleeping");
                    self.act_power(Do::Suspend);
                }
            }
        }
    }

    /// Turns the backlight down to a third of its level (at least one
    /// percent) and every keyboard light off, 30 s before the screens go off
    /// (M7.16b). The raw levels are kept for `undim`. A machine with no
    /// backlight does nothing. The levels are written on the status thread,
    /// as the brightness is, never on the one that draws.
    fn dim(&mut self) {
        if self.dimmed.is_some() {
            return;
        }
        let Some(screen) = edel::backlight::find() else {
            return;
        };
        let keyboards: Vec<(PathBuf, u32)> = edel::backlight::keyboard()
            .into_iter()
            .filter_map(|led| edel::backlight::keyboard_level(&led).map(|level| (led, level)))
            .collect();
        let dim_level = edel::backlight::level_for(screen.max, (screen.percent() / 3).max(1));
        let off: Vec<(PathBuf, u32)> = keyboards.iter().map(|(led, _)| (led.clone(), 0)).collect();
        eprintln!("edel-shell-ui: idle, dimming");
        self.dimmed = Some(Dimmed {
            screen: screen.now,
            keyboards,
        });
        self.run_and_read(
            Some(Cmd::Lights {
                screen: Some(dim_level),
                keyboards: off,
            }),
            false,
        );
    }

    /// Sets back the light `dim` turned down, when it did (M7.16b): input
    /// came back, at any idle step. Nothing when nothing was dimmed.
    pub fn undim(&mut self) {
        let Some(saved) = self.dimmed.take() else {
            return;
        };
        eprintln!("edel-shell-ui: light back");
        self.run_and_read(
            Some(Cmd::Lights {
                screen: Some(saved.screen),
                keyboards: saved.keyboards,
            }),
            false,
        );
    }

    /// Makes the idle notifications the idle ladder asks for, on the seat,
    /// in place of the ones before (M7.8, M7.16b): at start, when a seat
    /// comes, when the rules change, when the charger changes and when a
    /// game starts or ends. The light comes back first, as a destroyed
    /// notification sends no resume.
    pub fn follow_idle(&mut self) {
        self.undim();
        for (_, old) in self.idle_notifications.drain(..) {
            old.destroy();
        }
        let (Some(notifier), Some(seat)) = (&self.idle_notifier, self.seat.seats().next()) else {
            return;
        };
        let stages = power_rules::idle_stages(&self.rules, self.live.status.on_battery, self.game);
        for (ms, stage) in stages {
            let notification =
                notifier.get_idle_notification(ms, &seat, &self.qh, IdleStage(stage));
            self.idle_notifications.push((stage, notification));
        }
    }

    /// The settings files changed (M7.8): the rules are read again, the idle
    /// ladder follows them and the profile follows the rules.
    pub fn power_changed(&mut self, machine: Option<&str>, person: Option<&str>) {
        let rules = power_rules::Rules::read(machine, person);
        if rules == self.rules {
            return;
        }
        self.rules = rules;
        self.follow_saver();
        self.follow_idle();
        self.follow_profile();
    }

    /// Decides the battery saver again (M8.31) from the rules and the last
    /// status: when it changes, says so, tells the compositor, which draws
    /// at most the Lite tier while it is on, and follows the profile. The
    /// panels and an open quick settings card follow it too.
    pub fn follow_saver(&mut self) {
        let on = power_rules::saver(
            &self.rules,
            self.live.status.on_battery,
            self.live.status.battery.as_ref().map(|b| b.percent),
            self.saver_latched,
        );
        self.saver_latched = on && self.rules.saver_auto();
        if on == self.saver {
            if !self.saver_told {
                self.saver_told = true;
                self.link.saver(on);
            }
            return;
        }
        self.saver = on;
        self.saver_told = true;
        eprintln!(
            "edel-shell-ui: battery saver {}",
            if on { "on" } else { "off" }
        );
        self.link.saver(on);
        if on && crate::on_path(crate::EDEL) && !edel::profile::offers_any() {
            eprintln!("edel-shell-ui: {}", messages::no_profile_for_saver());
        }
        self.draw_all();
        if let Some(card) = &mut self.quick {
            card.state.saver = on;
        }
        self.draw_quick();
        self.follow_profile();
    }

    /// A game plays on some screen, or none does any more (M7.7b): on the
    /// charger the profile becomes Performance while it plays, and the
    /// rules' profile comes back when it ends. A machine with no profile
    /// file says so once per game, as a VM does. The idle ladder follows too:
    /// while a game plays only the lock is kept (M7.16b).
    pub fn game_changed(&mut self, on: bool) {
        if self.game == on {
            return;
        }
        self.game = on;
        self.follow_idle();
        if on
            && self.live.status.on_battery != Some(true)
            && crate::on_path(crate::EDEL)
            && !edel::profile::offers_any()
        {
            eprintln!("edel-shell-ui: {}", messages::no_profile_for_game());
        }
        self.follow_profile();
    }

    /// Asks `edel power profile` for the profile the rules want now, when
    /// `edel` is here and the computer's profile, processor hint or trip
    /// charge is not settled yet (M7.8, M7.16c). Each change of the wanted
    /// profile or of the charger is asked once, and the command writes the
    /// files as root through doas. Called when a status is read and when the
    /// settings change; a machine with nothing to set does nothing.
    pub fn follow_profile(&mut self) {
        if !crate::on_path(crate::EDEL) {
            return;
        }
        let want = power_rules::profile(
            &self.rules,
            self.live.status.on_battery,
            self.game,
            self.saver,
        )
        .to_string();
        if !edel::profile::needs_apply(&want) {
            self.profile_asked = None;
            return;
        }
        let asked = (want.clone(), self.live.status.on_battery);
        if self.profile_asked.as_ref() == Some(&asked) {
            return;
        }
        eprintln!("edel-shell-ui: power profile {want} asked");
        self.profile_asked = Some(asked);
        let spawned = std::thread::Builder::new()
            .name("power-profile".into())
            .spawn(move || {
                // A refusal stays in the log, as `edel` tells it, so a
                // missing rule is found; the profile is asked again only
                // when the wanted one changes.
                match Command::new(crate::EDEL)
                    .args(["power", "profile", &want])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .output()
                {
                    Ok(out) if out.status.success() => {}
                    Ok(out) => eprintln!(
                        "edel-shell-ui: {}",
                        messages::profile_not_set(String::from_utf8_lossy(&out.stderr).trim())
                    ),
                    Err(e) => eprintln!("edel-shell-ui: {}", messages::profile_not_set(e)),
                }
            });
        if let Err(e) = spawned {
            self.profile_asked = None;
            eprintln!("edel-shell-ui: {}", messages::profile_not_set(e));
        }
    }
}
