//! The power keys, the lid and the idle lock (M7.8): the shell's side of
//! `power_rules.rs`. The lid and the power button act here, a time without
//! input locks the screen (`edel-shell-ui: idle for N min, locking`) through
//! the compositor's ext-idle-notify, and the power profile the charger and
//! the settings want is asked of `edel power profile`. The commands run on
//! threads of their own, never the drawing one.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

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
use crate::power_rules::{self, Do};

/// How long a suspend or a shut down holds the others off after its command
/// returns: the computer wakes or goes down, and the lid's and the button's
/// news that came with it is not acted on again.
const GUARD: Duration = Duration::from_secs(3);

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

/// The user data of the idle notifier and of each idle notification, whose
/// events reach `Shell` (M7.8).
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

impl Dispatch2<ExtIdleNotificationV1, Shell> for IdleEvents {
    fn event(
        &self,
        shell: &mut Shell,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &Connection,
        _: &QueueHandle<Shell>,
    ) {
        // Resumed needs nothing: the lock waits for the next idle time.
        if let ext_idle_notification_v1::Event::Idled = event {
            shell.idled();
        }
    }
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

    /// Carries `choice` out on a thread of its own, closing Power's menu
    /// first when it is open. A suspend or a shut down holds the others off
    /// for `GUARD` after its command returns (M7.8).
    pub(crate) fn run_power(&mut self, choice: power::Choice) {
        if self.lock.as_ref().is_some_and(|card| card.menu) {
            self.set_power_menu(false);
        }
        let guarded = matches!(choice, power::Choice::Suspend | power::Choice::ShutDown);
        if guarded && self.suspending.swap(true, Ordering::SeqCst) {
            eprintln!(
                "edel-shell-ui: power: {} ignored, one is already under way",
                choice.name()
            );
            return;
        }
        eprintln!("edel-shell-ui: power: {}", choice.name());
        let suspending = Arc::clone(&self.suspending);
        let spawned = std::thread::Builder::new()
            .name("power".into())
            .spawn(move || {
                if let Err(e) = power::run(choice) {
                    eprintln!("edel-shell-ui: {e}");
                }
                if guarded {
                    std::thread::sleep(GUARD);
                    suspending.store(false, Ordering::SeqCst);
                }
            });
        if let Err(e) = spawned {
            if guarded {
                self.suspending.store(false, Ordering::SeqCst);
            }
            eprintln!(
                "edel-shell-ui: {}",
                messages::power_failed(choice.name(), e)
            );
        }
    }

    /// The idle time ran out (M7.8): the screen locks, unless it is locked.
    fn idled(&mut self) {
        if self.lock.is_some() {
            return;
        }
        eprintln!(
            "edel-shell-ui: idle for {} min, locking",
            self.rules.lock_after()
        );
        self.lock_screen();
    }

    /// Makes the idle notification the lock time asks for, on the seat, in
    /// place of the one before (M7.8): at start, when a seat comes and when
    /// the lock time changes. None for never.
    pub fn follow_idle(&mut self) {
        if let Some(old) = self.idle_notification.take() {
            old.destroy();
        }
        let (Some(notifier), Some(ms), Some(seat)) = (
            &self.idle_notifier,
            power_rules::idle_ms(&self.rules),
            self.seat.seats().next(),
        ) else {
            return;
        };
        self.idle_notification =
            Some(notifier.get_idle_notification(ms, &seat, &self.qh, IdleEvents));
    }

    /// The settings files changed (M7.8): the rules are read again, the idle
    /// notification follows a new lock time and the profile follows the rules.
    pub fn power_changed(&mut self, machine: Option<&str>, person: Option<&str>) {
        let rules = power_rules::Rules::read(machine, person);
        if rules == self.rules {
            return;
        }
        let idle = rules.lock_after() != self.rules.lock_after();
        self.rules = rules;
        if idle {
            self.follow_idle();
        }
        self.follow_profile();
    }

    /// Asks `edel power profile` for the profile the rules want now, when
    /// `edel` is here, the kernel offers one of ours and it is not set yet
    /// (M7.8). Each change is asked once, and the command writes the file as
    /// root through doas. Called when a status is read and when the settings
    /// change; a machine with no profile file does nothing.
    pub fn follow_profile(&mut self) {
        if !crate::on_path(crate::EDEL) {
            return;
        }
        let Some((now, offered)) = edel::profile::now() else {
            return;
        };
        let want = power_rules::profile(&self.rules, self.live.status.on_battery).to_string();
        let names: Vec<&str> = offered.iter().map(String::as_str).collect();
        let Some(kernel) = edel::profile::kernel_profile(&want, &names) else {
            return;
        };
        if kernel == now {
            self.profile_asked = None;
            return;
        }
        if self.profile_asked.as_deref() == Some(want.as_str()) {
            return;
        }
        eprintln!("edel-shell-ui: power profile {want} asked");
        self.profile_asked = Some(want.clone());
        let spawned = std::thread::Builder::new()
            .name("power-profile".into())
            .spawn(move || {
                let _ = Command::new(crate::EDEL)
                    .args(["power", "profile", &want])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            });
        if let Err(e) = spawned {
            self.profile_asked = None;
            eprintln!("edel-shell-ui: {}", messages::profile_not_set(e));
        }
    }
}
