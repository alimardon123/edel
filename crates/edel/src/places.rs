//! Where Edel OS keeps its files (ADR-010): every part names Edel OS's
//! places through this module, so a path or a file's name is written once
//! and a change to it is one line here. M5.25a starts it with the settings
//! file, the one file that describes a machine (ADR-006, ADR-008's same
//! names decision); M5.27 brings Edel OS's other places here.
//!
//! CI's shell scripts read the same names from `ci/names.sh`, which a test
//! holds equal to this module, and a test fails when the settings file's
//! name is written anywhere else in the code, CI or the features.

use std::path::{Path, PathBuf};

/// The settings file's name. A rename, such as to `edel.toml`, changes
/// this line and adds the old name to [`FORMER_SETTINGS`].
pub const SETTINGS: &str = "settings.toml";

/// Names the settings file had before, newest first. Readers still read a
/// file by one of them when none has the new name, and `apply` renames it
/// on disk, so a machine set up before a rename keeps its settings.
pub const FORMER_SETTINGS: &[&str] = &["system.toml"];

// Each root below is written once, in its macro; every place under it is
// built from the macro, so moving a root is one line.
macro_rules! data {
    ($path:literal) => {
        concat!("/data", $path)
    };
}
macro_rules! run {
    ($path:literal) => {
        concat!("/run/edel", $path)
    };
}
macro_rules! share {
    ($path:literal) => {
        concat!("/usr/share/edel", $path)
    };
}

/// Where the data partition, shared by both slots, is mounted.
pub const DATA_MOUNT: &str = data!("");
/// Edel OS's directory on the data partition.
pub const DATA_DIR: &str = data!("/edel");
/// What this machine changed in `/etc`: a file here differs from the
/// slot's (the overlay's upper directory, M1.4).
pub const ETC_UPPER: &str = data!("/etc/upper");
/// When `edel update --check` or `edel update` last verified the channel's
/// list, and the version it offered (M8.14): written as root only, read by
/// `edel::security`, so the status can say whether the machine is up to date.
pub const LAST_CHECK: &str = data!("/edel/last-check.toml");
/// The default apps (M6.2): a Flatpak system installation on the data
/// partition, so the apps are paid for once rather than in each slot, and
/// survive every update and rollback. `edel image build` fills it on the
/// stick, `edel install` copies it, `edel update` updates it.
pub const DEFAULT_APPS: &str = data!("/edel/apps");
/// The default apps the slot's image was built with and those a release
/// dropped (M6.2), written by `edel image build`; `edel update` follows it.
pub const DEFAULT_APPS_LIST: &str = share!("/default-apps.toml");
/// Flathub's remote file (M6.4), fetched by `edel image build` from
/// `apps::FLATHUB_REMOTE`, from which each person's session adds Flathub
/// offline, so the store works on a first start without the network.
pub const FLATHUB_REPO_FILE: &str = share!("/flathub.flatpakrepo");
/// Where Flatpak finds its installations besides its own two; the image
/// build writes the default apps' file here (`apps::installation_conf`).
pub const FLATPAK_INSTALLATIONS: &str = "/etc/flatpak/installations.d";
/// The fonts' cache, baked into the slot by `edel image build` (M6.2a).
/// Flatpak hands every app the first of `VAR_FONT_CACHE` and this folder
/// that exists as `/run/host/fonts-cache`, so no app rebuilds it on its
/// first start.
pub const FONT_CACHE_DIR: &str = "/usr/lib/fontconfig/cache";
/// Fontconfig's own cache folder, on the data partition. The slot's own
/// cache replaces it (M6.2a), so an update never leaves a stale one.
pub const VAR_FONT_CACHE: &str = "/var/cache/fontconfig";
/// The fontconfig file that names `FONT_CACHE_DIR` as a cache folder
/// (M6.2a).
pub const FONT_CACHE_CONF: &str = "/etc/fonts/conf.d/05-edel-font-cache.conf";
/// Developer mode is on while this file exists (ADR-007).
pub const DEVELOPER_FLAG: &str = data!("/edel/developer");
/// Developer mode's layer over `/usr` (M7.1): the overlay's upper
/// directory, exactly what this machine changed there, and its work
/// directory beside it on the same file system.
pub const DEVELOPER_LAYER: &str = data!("/developer/usr");
pub const DEVELOPER_WORK: &str = data!("/developer/work");
/// The layer is held back while this mark exists: a start found no slot
/// with a try left (M7.1), and only a reset or switching off ends it.
pub const DEVELOPER_HELD: &str = data!("/developer/held");
/// A reset waits here for the next start, which deletes the layer before
/// mounting anything (M7.1).
pub const DEVELOPER_RESET: &str = data!("/developer/reset");
/// A factory reset waits here for the next start (M8.38): `edel boot
/// mount-data` erases the data partition but the default apps, the
/// add-ons' images and [`INSTALLED`] before mounting anything from it, and
/// deletes this last, so a start cut short erases the rest at the next.
pub const FACTORY_RESET: &str = data!("/edel/factory-reset");
/// What a factory reset moved aside, deleted by a process of its own
/// after the move, so the guard's watchdog never waits on a long erase;
/// a start that finds it deletes it again (M8.38).
pub const FACTORY_RESET_TRASH: &str = data!("/.edel-erasing");
/// What developer mode is in this boot, written by `edel boot
/// mount-data`: `on` or `held`; absent is off (M7.1).
pub const DEVELOPER_NOW: &str = run!("/developer");
/// The last fallback from a slot that did not start, for shell-ui to show
/// once (M1.1, M5.9).
pub const LAST_FALLBACK: &str = data!("/edel/last-fallback.toml");
/// The hashed passwords `edel install` asked for (M6.6b), one
/// `NAME:HASH` line each, root's only: the new machine's first
/// `edel settings apply` gives them to the people the settings file makes
/// and deletes the file, so no password is ever in the settings file.
pub const FIRST_PASSWORDS: &str = data!("/edel/first-passwords");
/// Written by `edel install` on the new disk's data partition: the machine
/// was installed, so a removable disk that holds it is no stick, which
/// logs `live` in by itself and installs without a password (M6.17).
pub const INSTALLED: &str = data!("/edel/installed");
/// Each person's keyring key (M7.18), one file per person, root's alone:
/// made once by `edel boot keyring` as their first session opens, and
/// gone with the accounts in a factory reset.
pub const KEYRING_KEYS: &str = data!("/edel/keyring");
/// Kept on a stick that installed itself (M7.3): the disk's id under
/// `/dev/disk/by-id/` and the date, so starting from the stick again in
/// the same machine never installs twice.
pub const INSTALLED_TO: &str = data!("/edel/installed-to");
/// This device's own identity key (M6.12, ADR-011): an ed25519 secret key
/// in hex, root's only, made once at the first boot, for pairing devices
/// (M7.10b) and passkeys (M9.11) later; nothing reads it yet.
pub const DEVICE_KEY: &str = data!("/edel/device.key");
/// The public half of [`DEVICE_KEY`], in hex, readable by everyone.
pub const DEVICE_KEY_PUB: &str = data!("/edel/device.pub");
/// The login screen's own directory, made at boot owned by greetd by the
/// login feature's `edel-live` (M5.10c3b).
pub const GREETER_DIR: &str = data!("/edel/greeter");
/// Each person's desktop for their login, written by the login screen
/// (M5.10c3b): `[people]` with a login name and the desktop file's id, the
/// file name without `.desktop`. A default is never written.
pub const GREETER_SESSIONS: &str = data!("/edel/greeter/sessions.toml");
/// The pictures of themes `edel theme import` adds to the machine's settings
/// file (M5.12b6), one folder per theme, so every person on the machine sees
/// them; Settings imports into a person's own `person_themes_dir`.
pub const MACHINE_THEMES_DIR: &str = data!("/edel/themes");
/// The add-ons this machine has (M7.2a), one folder per add-on, each the
/// same add-on as the image's and kept on the data partition, so it
/// survives every update and rollback (ADR-007).
pub const ADDONS_DIR: &str = data!("/addons");
/// The folder, inside a version's add-ons folder, of the add-ons fetched
/// by `edel settings apply` and not yet started (M7.2b): the next boot
/// tries them, and the guard's confirmation moves them beside the others.
pub const ADDONS_PENDING: &str = "pending";
/// The one-shot mark in [`ADDONS_PENDING`] that lets the next boot try
/// the pending add-ons (M7.2b); that boot takes it away before it mounts
/// them, so a boot that hangs comes back without them.
pub const ADDONS_TRY: &str = "try";
/// The list, in a version's add-ons folder, of the add-ons whose first
/// boot did not finish (M7.2b), one name a line; apply fetches none of
/// them again until it is taken out of `addons.installed`.
pub const ADDONS_FAILED: &str = "failed";
/// Where `edel update` got the release list of the version it installed
/// (M7.2b), so `edel settings apply` fetches that version's add-ons from
/// the same list.
pub const RELEASE_FROM: &str = data!("/edel/release-from.toml");

/// Edel OS's directory for this boot only.
pub const RUN_DIR: &str = run!("");
/// What the person's session leaves for root (M4.8).
pub const SESSION_DIR: &str = run!("/session");
/// This boot's copies of the keyring keys of the people logged in (M7.18),
/// one per user id, each readable by its person alone, taken out as the
/// session closes.
pub const KEYRING_RUN: &str = run!("/keyring");
/// What the compositor shows: windows, layers, the effect tier (M4.3).
pub const STATE_FILE: &str = run!("/session/state.toml");
/// The desktop's health: the compositor's first frame (M4.8).
pub const READY_FILE: &str = run!("/session/ready");
/// Where a feature's own health checks leave their files, one per name,
/// such as a server's `network` (M1.11).
pub const HEALTH_DIR: &str = run!("/health");
/// The default runlevel's health (M1.5).
pub const DEFAULT_REACHED: &str = run!("/default-reached");
/// The fonts the compositor's title bars and shell-ui fall back to when
/// the interface font cannot be found (M6.17: shell-ui loads them itself
/// when fontconfig finds no font at all).
pub const FALLBACK_FONTS: [&str; 2] = [
    "/usr/share/fonts/inter/InterVariable.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
];
/// Written once the guard confirmed this boot's slot (M1.5).
pub const CONFIRMED: &str = run!("/confirmed");
/// Written when the slot is not healthy and no slot has a try left, so the
/// guard keeps the machine running rather than restarting it for ever;
/// `edel status` shows it (M6.17).
pub const NO_HEALTHY_SLOT: &str = run!("/no-healthy-slot");
/// There while `edel boot mount-data` waits for the data's passphrase to
/// be typed, so the guard's clock waits for the person (M8.2).
pub const UNLOCK_WAITING: &str = run!("/unlock-waiting");
/// Whether this boot has a screen, `yes` or `no`, decided once by the
/// boot's `edel settings apply` (M8.17), so the guard, the login screen's
/// live session and the console all follow the one answer the services did.
pub const SCREEN: &str = run!("/screen");
/// The boot's "Started in" line, for `edel report` (M3.2).
pub const STARTED: &str = run!("/started");
/// Written by `edel update` and `edel rollback` once a slot waits for a
/// restart, and gone at the next boot, as `/run` is memory (M5.10c): the
/// lock screen's power menu marks Restart while it exists.
pub const RESTART_PENDING: &str = run!("/restart-pending");
/// Written by `edel power charge-full` for a trip (M7.16e): battery care's
/// limit is off while it exists, and `edel boot battery-care` takes it away
/// once the computer is unplugged; gone at the next boot, as `/run` is memory.
pub const CHARGE_FULL_ONCE: &str = run!("/charge-full-once");
/// One updater at a time (M1.1).
pub const UPDATE_LOCK: &str = run!("/update.lock");
/// The account a stick logs in by itself (M3.6), which may also run
/// `edel install` there (M6.6a); never a person's login name.
pub const LIVE_USER: &str = "live";
/// Its id, the same on every stick, as `edel::users::ADMIN_GID` is.
pub const LIVE_UID: u32 = 990;
/// greetd's config with the live session (M3.6).
pub const GREETD_LIVE: &str = run!("/greetd.toml");
/// Who the session at the screen belongs to, the one person the
/// controllers and `/dev/uinput` are given to (M8.37): written by
/// `edel-seat` when greetd opens a session and taken out when it closes.
pub const SEAT_OWNER: &str = run!("/seat");
/// Where a seed stick or partition is mounted while it is read (M2.2).
pub const SEED_MOUNT: &str = run!("/seed");
/// Where the installer mounts the new disk's partitions (M2.4).
pub const INSTALL_DIR: &str = run!("/install");
/// The machine's certificate authorities, one PEM file (Alpine's
/// `ca-certificates-bundle`), which a settings file fetched over https is
/// checked against (M6.6b).
pub const CA_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";
/// The answers to the installer's questions as the new machine's settings
/// file, before `edel install` copies it to the new disk (M6.6b).
pub const INSTALL_ANSWERS: &str = run!("/install-answers.toml");
/// The settings file an unattended install writes to the new disk: the
/// machine's own without its `[install]` table (M7.3).
pub const INSTALL_UNATTENDED: &str = run!("/install-unattended.toml");
/// The links to whole disks and partitions by their hardware ids.
pub const DISKS_BY_ID: &str = "/dev/disk/by-id";
/// Where an add-on is mounted while the slot's root is built or started
/// (M7.2a), one folder per add-on.
pub const ADDONS_RUN: &str = run!("/addons");
/// The hardware packs this machine's devices match and it has not chosen,
/// written at each start (M7.11): one line per pack, its name, a tab and its
/// summary.
pub const PACKS_FILE: &str = run!("/hardware-packs");
/// The pending add-ons this boot tried (M7.2b), one name a line, which the
/// guard's confirmation moves beside the others.
pub const ADDONS_TRIED: &str = run!("/addons-tried");

/// Where a slot keeps Edel OS's own files.
pub const SHARE_DIR: &str = share!("");
/// The image's feature files (M4.0).
pub const FEATURES_DIR: &str = share!("/features");
/// The feature files of the add-ons built beside this image (M7.2b), so
/// Settings and `edel addon list` offer them without the network.
pub const OFFERED_ADDONS_DIR: &str = share!("/addons");
/// The public keys releases are checked against (M1.6).
pub const KEYS_DIR: &str = share!("/keys");
/// Every package in the slot, one per line (M3.1).
pub const PACKAGES_FILE: &str = share!("/packages");
/// The design tokens a slot may carry in place of the built-in ones.
pub const TOKENS_FILE: &str = share!("/design/tokens.toml");
/// foot's font and colours from the tokens (M6.13), which every person's
/// foot config includes.
pub const FOOT_INI: &str = share!("/foot.ini");
/// The wallpapers the `wallpapers` feature carries, and their `CREDITS`
/// (M5.10c4b): `NAME-light.jpg` and `NAME-dark.jpg` for a pair.
pub const WALLPAPERS_DIR: &str = share!("/wallpapers");
/// Speaker tuning profiles and the tables that match them to models
/// (M6.15, `edel::speakers`).
pub const SPEAKERS_DIR: &str = share!("/speakers");
/// The PipeWire drop-in that gives sound little delay (M7.6b), in the
/// slot; the compositor links it into the person's own drop-in folder
/// ([`low_latency_link`]) while `sound.low_latency` is on.
pub const LOW_LATENCY: &str = share!("/sound/low-latency.conf");
/// Where each part's words in each language are (M5.24): `LANG/PART.po`.
pub const LOCALE_DIR: &str = share!("/locale");
/// Alpine's initramfs `init` with our one change, which finds a root
/// named by its partition (`root=PARTUUID=`, M1.12); `mkinitfs -i` uses it.
pub const INITRAMFS_INIT: &str = share!("/initramfs-init");
/// These places as shell variables, for the slot's own scripts (M5.27).
pub const PLACES_SH: &str = share!("/places.sh");

/// Where people's home directories are.
pub const HOMES: &str = "/home";
/// A person's state directory for Edel OS inside their home, where
/// `XDG_STATE_HOME` is not set: their session's log (M5.28a).
pub const STATE_IN_HOME: &str = ".local/state/edel";
/// A person's configuration folder in their home, when `XDG_CONFIG_HOME` is not set.
pub const CONFIG_IN_HOME: &str = ".config";
/// The person's link to [`LOW_LATENCY`] in their configuration folder:
/// PipeWire's own drop-in folder for them, read when their PipeWire starts.
pub const LOW_LATENCY_IN_CONFIG: &str = "pipewire/pipewire.conf.d/90-edel-low-latency.conf";
/// The desktop session's log, in a person's state directory: the
/// compositor and what it starts write to it (M5.28a).
pub const SESSION_LOG: &str = "session.log";
/// The session before's log, beside it: two sessions are kept.
pub const SESSION_LOG_BEFORE: &str = "session.old.log";
/// Each game's smoothness, the last time it played (M7.7d,
/// `edel::smoothness`), in a person's state directory: the compositor
/// writes it when a game ends, `edel report` reads it.
pub const GAMES: &str = "games.toml";
/// foot's theme in a person's state directory, `dark` or `light` (M6.13):
/// shell-ui writes it, and foot's config includes it, so a new window
/// starts in the desktop's scheme.
pub const FOOT_THEME: &str = "foot-theme.ini";
/// The system log busybox's syslogd writes, on the data partition.
pub const SYSTEM_LOG: &str = "/var/log/messages";
/// Where the system's password helper may lie, the first that exists
/// taken: Linux-PAM's `unix_chkpwd`, which checks a person's own password
/// for a program they run, as the lock screen does (M5.10). Alpine puts it
/// in `/sbin`; other systems in `/usr/sbin`, so the desktop runs there too.
pub const PASSWORD_CHECKERS: &[&str] = &["/sbin/unix_chkpwd", "/usr/sbin/unix_chkpwd"];
/// The program that runs a command as root for the people its rules allow
/// (M6.5): OpenDoas, whose rules the machine and login features keep in
/// `/etc/doas.d/`; the machine feature also answers to `sudo` through
/// doas-sudo-shim. A base with another such program changes this,
/// [`AS_ROOT_USER`] and those rule files, and nothing else (ADR-010).
pub const AS_ROOT: &str = "doas";
/// The variable [`AS_ROOT`] sets to the login name of the person who ran it.
pub const AS_ROOT_USER: &str = "DOAS_USER";
/// Where the `edel` command is installed on a machine, which the rules of
/// the login feature (`etc/doas.d/`) name when a person's `edel` runs it as
/// root (M7.8).
pub const EDEL_BIN: &str = "/usr/bin/edel";

/// The boot loader the slot carries, which rides along with updates (M1.8).
pub const SLOT_BOOT_DIR: &str = "/usr/lib/edel/boot";
/// Edel OS's directory on the EFI system partition: GRUB, its counters and
/// a seed settings file.
pub const ESP_DIR: &str = "/EFI/edel";
/// GRUB's config in [`ESP_DIR`], copied from the slot's [`SLOT_BOOT_DIR`].
pub const GRUB_CFG: &str = "grub.cfg";
/// GRUB's environment block in [`ESP_DIR`]: the slots' order and counters.
pub const GRUBENV: &str = "grubenv";
/// The boot loader's version file in [`ESP_DIR`] and the slot's boot
/// folder, which says whether GRUB rides along with an update.
pub const LOADER_TOML: &str = "loader.toml";

/// `file` in [`ESP_DIR`] on the EFI system partition mounted at `esp`
/// (`/boot/efi/EFI/edel/grubenv`), or [`ESP_DIR`] itself for an empty
/// `file`.
pub fn on_esp(esp: &Path, file: &str) -> PathBuf {
    let dir = esp.join(ESP_DIR.trim_start_matches('/'));
    if file.is_empty() { dir } else { dir.join(file) }
}

/// The desktop sessions installed, one `.desktop` file each, which the
/// login screen offers (M5.10c3b). The standard place, not Edel OS's.
pub const WAYLAND_SESSIONS: &str = "/usr/share/wayland-sessions";

/// The folders cursor and icon themes are installed in, in the order they
/// are read (M5.12b5): the system's, then the local administrator's. A
/// cursor theme is a folder in one of them with a `cursors` folder in it,
/// as the xcursor standard lays them out.
pub const ICON_THEME_DIRS: &[&str] = &["/usr/share/icons", "/usr/local/share/icons"];

/// Where admins look first: `/etc/edel/` holds a link to the machine's
/// settings file.
pub const ETC_DIR: &str = "/etc/edel";
/// The time zones' files, from tzdata (the shell feature ships it, M5.10c7d).
pub const ZONEINFO_DIR: &str = "/usr/share/zoneinfo";
/// The link to the time zone the machine keeps its clock in; no link is UTC.
pub const LOCALTIME: &str = "/etc/localtime";
/// The time zone's name, beside [`LOCALTIME`], for programs that read the name.
pub const TIMEZONE_FILE: &str = "/etc/timezone";
/// The place of this computer, the static source GeoClue reads when it is
/// asked (M6.14c): `edel settings apply` writes it from `location.place`.
pub const GEOLOCATION: &str = "/etc/geolocation";
/// The drop-in that turns GeoClue's lookup of nearby Wi-Fi on, written from
/// `location.online` (M6.14c); GeoClue reads it when it starts.
pub const GEOCLUE_ONLINE: &str = "/etc/geoclue/conf.d/60-edel-online.conf";
/// The system's default apps, written by `edel settings apply` from
/// `[default_apps]` (M6.10): freedesktop's `mimeapps.list`, read by every
/// desktop app and by `xdg-open`. Only a file that starts with
/// `apps::MIME_COMMENT` is ours to change.
pub const MIME_DEFAULTS: &str = "/etc/xdg/mimeapps.list";
/// The link `edel boot speakers` makes to this machine's speaker profile
/// (M6.15), in PipeWire's own drop-in folder, so the session's PipeWire
/// loads it; none when no profile matches.
pub const SPEAKER_PROFILE: &str = "/etc/pipewire/pipewire.conf.d/80-edel-speakers.conf";
/// The folder an add-on's init scripts are copied to in the slot, run
/// at boot by the slot's own init (M7.2a).
pub const ADDON_INIT_DIR: &str = "/usr/lib/edel/init.d";
/// A link to OpenRC's `edel::base::SERVICE_CONF_DIR` beside
/// `ADDON_INIT_DIR` (M8.17c): OpenRC follows `/etc/init.d`'s link to an
/// add-on's script and looks for its `conf.d` beside the script, so an
/// add-on's service reads the machine's settings as a slot's does.
pub const ADDON_CONF_DIR: &str = "/usr/lib/edel/conf.d";
/// The folder of an add-on's own `/etc` files, one folder per add-on, so
/// the files it brings never clash with the slot's (M7.2a).
pub const ADDON_ETC_DIR: &str = "/usr/lib/edel/etc";
/// The folder of an add-on's own `/var` files, one folder per add-on
/// (M7.2a).
pub const ADDON_VAR_DIR: &str = "/usr/lib/edel/var";
/// The folder of an add-on's account lines (`passwd`, `group`, `shadow`),
/// one folder per add-on, merged into the slot's files at boot (M7.2a).
pub const ADDON_ACCOUNTS_DIR: &str = "/usr/lib/edel/accounts";
/// The firewall's rules, which edel-firewall loads when the machine starts
/// (M8.14): plain nft text, so it names [`FIREWALL_DIR`] itself, and a
/// test checks the two agree. Under `/usr`, so an update brings it.
pub const FIREWALL_RULES: &str = "/usr/lib/edel/firewall/edel.nft";
/// The folder of the firewall's drop-ins, one `NAME.nft` a feature that
/// listens on the network ships, which the rules include (M8.14).
pub const FIREWALL_DIR: &str = "/usr/lib/edel/firewall.d";

/// The places shell scripts use, as `NAME=VALUE` lines: `ci/names.sh` for
/// CI and `/usr/share/edel/places.sh` in every image, both written from
/// here (M5.27).
pub fn shell_vars() -> Vec<(&'static str, String)> {
    vec![
        ("settings_name", SETTINGS.to_string()),
        ("data_dir", DATA_DIR.to_string()),
        ("run_dir", RUN_DIR.to_string()),
        ("session_dir", SESSION_DIR.to_string()),
        ("state_file", STATE_FILE.to_string()),
        ("ready_file", READY_FILE.to_string()),
        ("default_reached", DEFAULT_REACHED.to_string()),
        ("health_dir", HEALTH_DIR.to_string()),
        ("confirmed_file", CONFIRMED.to_string()),
        ("no_healthy_slot", NO_HEALTHY_SLOT.to_string()),
        ("screen_file", SCREEN.to_string()),
        ("started_file", STARTED.to_string()),
        ("greetd_live", GREETD_LIVE.to_string()),
        ("seat_owner", SEAT_OWNER.to_string()),
        ("last_fallback", LAST_FALLBACK.to_string()),
        ("addons_dir", ADDONS_DIR.to_string()),
        // The same folder on the data partition itself (M8.17c), for a
        // script that reads the partition with debugfs.
        (
            "addons_dir_on_data",
            ADDONS_DIR
                .strip_prefix(DATA_MOUNT)
                .unwrap_or(ADDONS_DIR)
                .to_string(),
        ),
        ("first_passwords", FIRST_PASSWORDS.to_string()),
        ("installed_mark", INSTALLED.to_string()),
        ("default_apps", DEFAULT_APPS.to_string()),
        ("factory_reset", FACTORY_RESET.to_string()),
        ("factory_reset_trash", FACTORY_RESET_TRASH.to_string()),
        ("greeter_dir", GREETER_DIR.to_string()),
        ("greeter_sessions", GREETER_SESSIONS.to_string()),
        ("share_dir", SHARE_DIR.to_string()),
        ("keys_dir", KEYS_DIR.to_string()),
        ("firewall_rules", FIREWALL_RULES.to_string()),
        ("keyring_keys", KEYRING_KEYS.to_string()),
        ("keyring_run", KEYRING_RUN.to_string()),
        ("esp_dir", ESP_DIR.to_string()),
        // Inside a person's home (M5.28a).
        ("home_session_log", format!("{STATE_IN_HOME}/{SESSION_LOG}")),
        ("system_log", SYSTEM_LOG.to_string()),
        ("font_cache_dir", FONT_CACHE_DIR.to_string()),
        ("flathub_repo_file", FLATHUB_REPO_FILE.to_string()),
        ("low_latency", LOW_LATENCY.to_string()),
        (
            "home_low_latency_link",
            format!("{CONFIG_IN_HOME}/{LOW_LATENCY_IN_CONFIG}"),
        ),
    ]
}

/// `shell_vars` as a sourced shell file, with a header saying what wrote
/// it.
pub fn shell_file(what: &str) -> String {
    let mut text = format!(
        "# {what}.\n\
         # Written by edel::places (crates/edel/src/places.rs); never edit it\n\
         # by hand. Sourced, not run.\n\
         # shellcheck disable=SC2034\n"
    );
    for (name, value) in shell_vars() {
        text.push_str(&format!("{name}={value}\n"));
    }
    text
}

/// The machine's settings file, on the data partition.
pub fn machine_settings() -> PathBuf {
    Path::new(DATA_DIR).join(SETTINGS)
}

/// The slot's own settings file, which seeds a first boot when no stick
/// or EFI system partition holds one.
pub fn slot_settings() -> PathBuf {
    Path::new(SHARE_DIR).join(SETTINGS)
}

/// The link to the machine's settings file in `/etc/edel/`.
pub fn etc_settings() -> PathBuf {
    Path::new(ETC_DIR).join(SETTINGS)
}

/// The settings file of the person whose home is `home`, as their own
/// session finds it without `XDG_CONFIG_HOME`.
pub fn person_settings_in(home: &Path) -> PathBuf {
    home.join(CONFIG_IN_HOME).join("edel").join(SETTINGS)
}

/// A person's own directory for Edel OS: `$XDG_CONFIG_HOME/edel`, else
/// `~/.config/edel`; none without either variable.
pub fn person_dir() -> Option<PathBuf> {
    config_home().map(|config| config.join("edel"))
}

/// The person's link to [`LOW_LATENCY`] (M7.6b); none without
/// `XDG_CONFIG_HOME` or `HOME`.
pub fn low_latency_link() -> Option<PathBuf> {
    config_home().map(|config| config.join(LOW_LATENCY_IN_CONFIG))
}

/// A person's configuration folder: `$XDG_CONFIG_HOME`, else `~/.config`;
/// none without either variable.
fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(CONFIG_IN_HOME))
        })
}

/// A person's GTK 4 stylesheet, `$XDG_CONFIG_HOME/gtk-4.0/gtk.css`, else
/// `~/.config/gtk-4.0/gtk.css`, which every GTK 4 and libadwaita app reads
/// at its start and shell-ui writes from the tokens (M6.18); a Flatpak app
/// reads it through the read-only permission shell-ui keeps in the person's
/// global override (`app_permissions::GTK_CSS_FILESYSTEM`).
/// None without either variable.
pub fn person_gtk_css() -> Option<PathBuf> {
    config_home().map(|config| config.join("gtk-4.0").join("gtk.css"))
}

/// A person's state directory for Edel OS: `$XDG_STATE_HOME/edel`, else
/// [`STATE_IN_HOME`] in their home; none without either variable.
pub fn person_state_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    match var("XDG_STATE_HOME") {
        Some(state) => Some(PathBuf::from(state).join("edel")),
        None => var("HOME").map(|home| PathBuf::from(home).join(STATE_IN_HOME)),
    }
}

/// Where shell-ui keeps the screenshot an app asked for (M8.21 part 21f):
/// `$XDG_RUNTIME_DIR/edel/screenshots`, in the person's own runtime folder
/// (mode 0700, made by pam_rundir), so no other person can read it; none
/// without the variable.
pub fn screenshots_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(|runtime| PathBuf::from(runtime).join("edel").join("screenshots"))
}

/// The record of which app used the camera, the microphone or the screen
/// and when (M6.14b, `edel::app_use`), in a person's state directory
/// ([`person_state_dir`]): shell-ui writes it, Settings' app pages read it.
pub const APP_USE: &str = "app-use";

/// The stamp Settings' Welcome leaves in a person's state directory
/// ([`person_state_dir`]) when it closes on a stick (M6.12), so the stick's
/// desktop opens it once; `edel::first_start` reads it.
pub const WELCOME_SEEN: &str = "welcome-seen";

/// A person's own themes' pictures (M5.12b6): `$XDG_DATA_HOME/edel/themes`,
/// else `~/.local/share/edel/themes`, one folder per imported theme, named
/// by the theme. A theme's values are in the settings file; these are only
/// the pictures an imported theme brought, so it keeps working without its
/// `.edeltheme` file. None without either variable.
pub fn person_themes_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    match var("XDG_DATA_HOME") {
        Some(data) => Some(PathBuf::from(data).join("edel").join("themes")),
        None => var("HOME").map(|home| PathBuf::from(home).join(".local/share/edel/themes")),
    }
}

/// The folder of a person's own desktop files (M6.4), where a web app's
/// desktop file goes: `$XDG_DATA_HOME/applications` (an absolute value
/// only, as the spec says), else `~/.local/share/applications`. None
/// without a home folder.
pub fn person_applications_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    match var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
    {
        Some(data) => Some(data.join("applications")),
        None => var("HOME").map(|home| PathBuf::from(home).join(".local/share/applications")),
    }
}

/// The folder of a person's Flatpak override files (M6.14a), one file per
/// app id, which Flatpak lays over what the app asked for when it runs:
/// `$XDG_DATA_HOME/flatpak/overrides` (an absolute value only), else
/// `~/.local/share/flatpak/overrides`. None without a home folder.
pub fn person_flatpak_overrides_dir() -> Option<PathBuf> {
    person_applications_dir()
        .and_then(|apps| apps.parent().map(|data| data.join("flatpak/overrides")))
}

/// The folder a web app's Firefox profile is kept in (M6.4): inside
/// Firefox's own Flatpak data folder, which its sandbox can write, one
/// folder per web app. None without a home folder.
pub fn web_apps_profiles_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".var/app")
            .join(crate::web_apps::BROWSER)
            .join("edel-web-apps"),
    )
}

/// The folder of a person's lock screen pictures (M5.10c7c): one PNG for
/// each look Settings' Lock screen page shows, drawn by `edel-shell-ui
/// --look-picture` and named by a hash of what the picture shows. Settings
/// deletes the pictures no look uses, so the folder stays small. Nothing
/// else reads it, and none of it is a setting.
pub fn look_pictures_dir() -> Option<PathBuf> {
    person_state_dir().map(|dir| dir.join("look-pictures"))
}

/// Where one of Edel OS's shared places, such as [`TOKENS_FILE`] or
/// [`FEATURES_DIR`], is found at run time (M5.15a). On Edel OS it is where
/// the constant says. On another system the desktop may be installed
/// elsewhere, so the same place under the first `edel` folder of the XDG
/// data directories that has it is used: `$XDG_DATA_HOME` (else
/// `~/.local/share`), then each of `$XDG_DATA_DIRS` (else
/// `/usr/local/share` and `/usr/share`). When none has it, the constant
/// comes back, so a message names where it was looked for.
pub fn found_shared(place: &str) -> PathBuf {
    if Path::new(place).exists() {
        return PathBuf::from(place);
    }
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let mut dirs: Vec<PathBuf> = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .into_iter()
        .collect();
    match var("XDG_DATA_DIRS") {
        Some(list) => dirs.extend(std::env::split_paths(&list)),
        None => dirs.extend(["/usr/local/share", "/usr/share"].map(PathBuf::from)),
    }
    shared_in(place, &dirs)
}

/// [`found_shared`] with the data directories given.
fn shared_in(place: &str, dirs: &[PathBuf]) -> PathBuf {
    let within = place
        .strip_prefix(SHARE_DIR)
        .unwrap_or(place)
        .trim_start_matches('/');
    dirs.iter()
        .map(|dir| dir.join("edel").join(within))
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(place))
}

/// `part`'s words in `language`, such as `de` or `pt_BR` (M5.24).
pub fn catalogue(language: &str, part: &str) -> PathBuf {
    Path::new(LOCALE_DIR)
        .join(language)
        .join(format!("{part}.po"))
}

/// A person's own settings file, with the same keys as the machine's; for
/// the keys the desktop reads, its values win.
pub fn person_settings() -> Option<PathBuf> {
    person_dir().map(|dir| dir.join(SETTINGS))
}

/// The settings file named `name` inside `dir`, such as on a seed stick or
/// the EFI system partition.
pub fn settings_in(dir: &Path) -> PathBuf {
    dir.join(SETTINGS)
}

/// Whether `name` is the settings file's name, or one it had before; a
/// watcher follows a file by either.
pub fn is_settings_name(name: &str) -> bool {
    name == SETTINGS || FORMER_SETTINGS.contains(&name)
}

/// `place`, a path on the data partition, where that partition is mounted
/// at `root` instead of [`DATA_MOUNT`]: a disk being installed, an image
/// being built. The result is always under `root`, never `place` itself,
/// which would name the running machine's own file.
pub fn on_data(root: &Path, place: &Path) -> PathBuf {
    let below = place
        .strip_prefix(DATA_MOUNT)
        .or_else(|_| place.strip_prefix("/"))
        .unwrap_or(place);
    root.join(below)
}

/// `path`, a settings file, as it is found: itself when it exists, else
/// the first file beside it by a former name that does, else `path`.
pub fn found(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    FORMER_SETTINGS
        .iter()
        .map(|old| path.with_file_name(old))
        .find(|old| old.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// The names to look for in a directory, the settings file's own first.
pub fn settings_names() -> impl Iterator<Item = &'static str> {
    std::iter::once(SETTINGS).chain(FORMER_SETTINGS.iter().copied())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_data_place_on_another_mount_stays_under_it() {
        let root = Path::new("/mnt/new");
        assert_eq!(
            on_data(root, Path::new(DEFAULT_APPS)),
            Path::new("/mnt/new/edel/apps")
        );
        assert_eq!(
            on_data(root, Path::new("/etc/x")),
            Path::new("/mnt/new/etc/x")
        );
    }

    #[test]
    fn a_shared_place_is_found_in_the_xdg_data_dirs_elsewhere() {
        let root = std::env::temp_dir().join(format!("edel-shared-{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(second.join("edel/design")).unwrap();
        fs::write(second.join("edel/design/tokens.toml"), "").unwrap();
        let dirs = [first, second.clone()];
        assert_eq!(
            shared_in(TOKENS_FILE, &dirs),
            second.join("edel/design/tokens.toml")
        );
        // A place no data directory has stays Edel OS's own.
        assert_eq!(shared_in(FEATURES_DIR, &dirs), PathBuf::from(FEATURES_DIR));
        fs::remove_dir_all(root).unwrap();
    }

    // M5.15a: the desktop finds Edel OS's places only through this module,
    // so it can ship on another distribution.
    #[test]
    fn the_desktop_names_no_place_of_edel_os_itself() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let roots = [SHARE_DIR, RUN_DIR, DATA_DIR];
        let mut found = Vec::new();
        for part in ["compositor", "shell-ui"] {
            let mut files = vec![crates.join(part).join("src")];
            while let Some(path) = files.pop() {
                if path.is_dir() {
                    files.extend(fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    let code = line.trim_start();
                    if code == "#[cfg(test)]" {
                        break;
                    }
                    if code.starts_with("//") {
                        continue;
                    }
                    if roots.iter().any(|root| code.contains(&format!("\"{root}"))) {
                        found.push(format!("{}:{}: {}", path.display(), n + 1, code));
                    }
                }
            }
        }
        assert!(
            found.is_empty(),
            "name these through edel::places instead:\n{}",
            found.join("\n")
        );
    }

    #[test]
    fn a_former_name_is_found_when_the_new_one_is_missing() {
        let dir = std::env::temp_dir().join(format!("edel-places-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = settings_in(&dir);
        assert_eq!(found(&path), path);
        fs::write(dir.join(FORMER_SETTINGS[0]), "format = 1\n").unwrap();
        assert_eq!(found(&path), dir.join(FORMER_SETTINGS[0]));
        fs::write(&path, "format = 1\n").unwrap();
        assert_eq!(found(&path), path);
        fs::remove_dir_all(&dir).unwrap();
        assert!(is_settings_name(SETTINGS) && is_settings_name(FORMER_SETTINGS[0]));
        assert!(!is_settings_name("state.toml"));
    }

    /// The firewall's rules include the drop-ins by the path
    /// [`FIREWALL_DIR`] names, and the drop-ins of ssh and printing sit in
    /// that folder (M8.14). The rules are plain nft text, so they cannot
    /// read this module and the test is what keeps the two equal.
    #[test]
    fn the_firewall_rules_include_the_drop_ins_folder() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rules_path = root
            .join("features/firewall")
            .join(FIREWALL_RULES.trim_start_matches('/'));
        let rules = fs::read_to_string(&rules_path).unwrap();
        let include = format!("include \"{FIREWALL_DIR}/*.nft\"");
        assert!(
            rules.lines().any(|line| line.trim() == include),
            "{} must include {include}",
            rules_path.display()
        );
        let folder = FIREWALL_DIR.trim_start_matches('/');
        for feature in ["ssh", "printing"] {
            let drop_in = root
                .join("features")
                .join(feature)
                .join(folder)
                .join(format!("{feature}.nft"));
            assert!(drop_in.is_file(), "{} is missing", drop_in.display());
        }
    }

    /// `ci/names.sh` says what this module says, for CI's shell scripts;
    /// with EDEL_WRITE_DOCS set, the test writes it.
    #[test]
    fn ci_names_are_these() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ci/names.sh");
        let want = shell_file("The places CI's scripts share with the code (ADR-010, M5.27)");
        if std::env::var_os("EDEL_WRITE_DOCS").is_some() {
            fs::write(&path, &want).unwrap();
        }
        assert!(
            fs::read_to_string(&path).unwrap_or_default() == want,
            "ci/names.sh differs from edel::places; run EDEL_WRITE_DOCS=1 cargo test -p edel ci_names"
        );
    }

    /// The settings file's name, and every name it had, is written here
    /// and in `ci/names.sh` only: code, CI and features take it from
    /// them, and docs say "the settings file" (ADR-010).
    #[test]
    fn the_settings_file_is_named_in_one_place() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let allowed = [
            root.join("crates/edel/src/places.rs"),
            root.join("ci/names.sh"),
        ];
        let mut found = Vec::new();
        for top in ["crates", "ci", "features", "images", "presets"] {
            walk(&root.join(top), &mut |path| {
                if allowed.iter().any(|a| same(a, path))
                    || path.extension().is_some_and(|e| e == "md")
                    || path.components().any(|c| c.as_os_str() == "target")
                {
                    return;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let text = fs::read_to_string(path).unwrap_or_default();
                // A feature file is named after its feature: the settings
                // feature (M5.6a) is features/settings.toml, not this file.
                let feature = path
                    .parent()
                    .is_some_and(|p| same(p, &root.join("features")));
                for named in settings_names() {
                    if (name == named && !feature) || text.contains(named) {
                        found.push(format!("{} names {named}", path.display()));
                    }
                }
            });
        }
        assert!(
            found.is_empty(),
            "the settings file's name belongs in edel::places and ci/names.sh only:\n{}",
            found.join("\n")
        );
    }

    fn same(a: &Path, b: &Path) -> bool {
        fs::canonicalize(a).ok() == fs::canonicalize(b).ok()
    }

    fn walk(dir: &Path, visit: &mut impl FnMut(&Path)) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, visit);
            } else {
                visit(&path);
            }
        }
    }
}
