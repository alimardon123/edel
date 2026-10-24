//! The base's own tools (roadmap M6.17, ADR-010): the commands of Alpine,
//! busybox and OpenRC that Edel OS runs, each named here and nowhere else,
//! so the base stays replaceable as `features/base.toml`, the image builder
//! and `edel::places` keep it (ADR-010). Each function gives the command
//! with its arguments; the caller runs it and says what failed.
//! `ci/seams.sh` fails a module that names one of these tools itself.

use std::process::Command;

/// The program that restarts the machine, run as root.
pub const REBOOT: &str = "/sbin/reboot";
/// The program that shuts the machine down, run as root.
pub const POWEROFF: &str = "/sbin/poweroff";
/// The program that suspends the machine to memory, run as root (M5.10c).
pub const SUSPEND: &str = "/usr/sbin/zzz";
/// The folder where OpenRC keeps a mark for each service it has started,
/// one file named after it (M8.14: the firewall's facts read it).
pub const STARTED_SERVICES: &str = "/run/openrc/started";
/// The one program the base's small tools are links to (busybox's
/// applets). A package an add-on brings may replace such a link with a
/// program of its own; the add-on then keeps the base's (M7.2a).
pub const APPLETS: &str = "busybox";

/// Whether OpenRC has started `service` and not stopped it since (M8.14).
pub fn is_started(service: &str) -> bool {
    is_started_in(std::path::Path::new(STARTED_SERVICES), service)
}

/// [`is_started`] with the marks in `dir`.
pub fn is_started_in(dir: &std::path::Path, service: &str) -> bool {
    dir.join(service).exists()
}

/// Where OpenRC reads each service's settings (`NAME` and
/// `NAME.RUNLEVEL`), beside the folder its script lies in.
pub const SERVICE_CONF_DIR: &str = "/etc/conf.d";

/// Puts `service` into `runlevel`, so it starts at every boot.
pub fn add_to_runlevel(service: &str, runlevel: &str) -> Command {
    let mut command = Command::new("rc-update");
    command.args(["add", service, runlevel]);
    command
}

/// Reads the services again, as OpenRC caches them before `/etc` and `/usr`
/// get their layers from the data partition (developer mode, M7.1).
pub fn reread_services() -> Command {
    let mut command = Command::new("rc-update");
    command.arg("--update");
    command
}

/// Takes `service` out of `runlevel`.
pub fn remove_from_runlevel(service: &str, runlevel: &str) -> Command {
    let mut command = Command::new("rc-update");
    command.args(["del", service, runlevel]);
    command
}

/// Starts `service` now.
pub fn start_service(service: &str) -> Command {
    service_now(service, "start")
}

/// Stops `service` now.
pub fn stop_service(service: &str) -> Command {
    service_now(service, "stop")
}

fn service_now(service: &str, verb: &str) -> Command {
    let mut command = Command::new("rc-service");
    command.args([service, verb]);
    command
}

/// The command a person runs to see why `service` did not start.
pub fn start_service_line(service: &str) -> String {
    format!("rc-service {service} start")
}

/// Adds the person `name` with the login shell `shell` and no password.
pub fn add_person(name: &str, shell: &str) -> Command {
    let mut command = Command::new("adduser");
    command.args(["-D", "-s", shell, name]);
    command
}

/// Adds the system account `name` (a stick's live person) with the login
/// shell `shell`, the home `home` and no password.
pub fn add_system_person(name: &str, shell: &str, home: &str) -> Command {
    let mut command = Command::new("adduser");
    command.args(["-S", "-D", "-s", shell, "-h", home, name]);
    command
}

/// Adds the system group `group`.
pub fn add_group(group: &str) -> Command {
    let mut command = Command::new("addgroup");
    command.args(["-S", group]);
    command
}

/// Puts `name` into `group`.
pub fn add_to_group(name: &str, group: &str) -> Command {
    let mut command = Command::new("addgroup");
    command.args([name, group]);
    command
}

/// Takes `name` out of `group`.
pub fn remove_from_group(name: &str, group: &str) -> Command {
    let mut command = Command::new("delgroup");
    command.args([name, group]);
    command
}

/// The program [`hash_password`] runs, for messages.
pub const PASSWORD_HASHER: &str = "cryptpw";

/// Hashes the password on standard input as `/etc/shadow` keeps it
/// (SHA-512 crypt), printing the hash, so the password is never on a
/// command line.
pub fn hash_password() -> Command {
    let mut command = Command::new(PASSWORD_HASHER);
    command.args(["-m", "sha512", "-P", "0"]);
    command
}

/// Sets the passwords of the `NAME:HASH` lines on standard input.
pub fn set_hashed_passwords() -> Command {
    let mut command = Command::new("chpasswd");
    command.arg("-e");
    command
}

/// Makes the device files the kernel lists and `/dev` lacks.
pub fn scan_devices() -> Command {
    let mut command = Command::new("mdev");
    command.arg("-s");
    command
}

/// udev's control socket, there while udevd runs (the `udev` feature); an
/// image with mdev has none.
pub const DEVICE_EVENTS_SOCKET: &str = "/run/udev/control";

/// Has udev read its rules again, as it read them before the add-ons'
/// layer was over `/usr` (M7.6b).
pub fn reread_device_rules() -> Command {
    let mut command = Command::new("udevadm");
    command.args(["control", "--reload"]);
    command
}

/// Has udev announce every device again as a change, which sets their
/// owners and modes by the rules it has now and runs no rule kept for a
/// device's arrival (M7.6b).
pub fn replay_device_events() -> Command {
    let mut command = Command::new("udevadm");
    command.args(["trigger", "--action=change"]);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_is_started_while_its_mark_is_there() {
        let dir = std::env::temp_dir().join(format!("edel-started-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("edel-firewall"), "").unwrap();
        assert!(is_started_in(&dir, "edel-firewall"));
        assert!(!is_started_in(&dir, "sshd"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn line(command: &Command) -> Vec<String> {
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn each_tool_gets_the_arguments_the_base_takes() {
        assert_eq!(
            line(&add_to_runlevel("sshd", "default")),
            ["rc-update", "add", "sshd", "default"]
        );
        assert_eq!(
            line(&remove_from_runlevel("sshd", "default")),
            ["rc-update", "del", "sshd", "default"]
        );
        assert_eq!(line(&reread_services()), ["rc-update", "--update"]);
        assert_eq!(
            line(&start_service("sshd")),
            ["rc-service", "sshd", "start"]
        );
        assert_eq!(line(&stop_service("sshd")), ["rc-service", "sshd", "stop"]);
        assert_eq!(start_service_line("sshd"), "rc-service sshd start");
        assert_eq!(
            line(&add_person("ali", "/bin/bash")),
            ["adduser", "-D", "-s", "/bin/bash", "ali"]
        );
        assert_eq!(
            line(&add_system_person("live", "/bin/bash", "/home/live")),
            [
                "adduser",
                "-S",
                "-D",
                "-s",
                "/bin/bash",
                "-h",
                "/home/live",
                "live"
            ]
        );
        assert_eq!(line(&add_group("wheel")), ["addgroup", "-S", "wheel"]);
        assert_eq!(
            line(&add_to_group("ali", "wheel")),
            ["addgroup", "ali", "wheel"]
        );
        assert_eq!(
            line(&remove_from_group("ali", "wheel")),
            ["delgroup", "ali", "wheel"]
        );
        assert_eq!(
            line(&hash_password()),
            ["cryptpw", "-m", "sha512", "-P", "0"]
        );
        assert_eq!(line(&set_hashed_passwords()), ["chpasswd", "-e"]);
        assert_eq!(line(&scan_devices()), ["mdev", "-s"]);
        assert_eq!(
            line(&reread_device_rules()),
            ["udevadm", "control", "--reload"]
        );
        assert_eq!(
            line(&replay_device_events()),
            ["udevadm", "trigger", "--action=change"]
        );
    }
}
