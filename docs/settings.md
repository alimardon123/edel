# The settings file

**Date:** 2026-10-02, amended 2026-10-06 (M5.25: the Settings app's names)

One TOML file describes a whole machine (ADR-006): the settings file, `settings.toml`, kept on the data partition and linked from `/etc/edel/settings.toml`. It holds only what a person chose; a missing key means the release decides (ADR-008). It never holds personal files or passwords. Its sections are the pages of the Settings app and its keys their rows, named the way the app names them (ADR-008's same names decision), so whoever knows the app can read the file and the command line, and back.

```toml
format = 1

[layout]
preset = "mac-like"

[appearance]
mode = "dark"

[network]
hostname = "ali-laptop"

[users.ali]
admin = true
ssh_keys = ["ssh-ed25519 AAAA... ali@desk"]
```

## The command

`edel settings` is the command line's Settings app ([Commands](guide/commands.md)):

| Command | What it does |
|---|---|
| `edel settings` | Lists the pages, in the app's order |
| `edel settings get [KEY or PAGE]` | Shows settings with their values and where each comes from; `--toml` prints the file's own TOML |
| `edel settings set KEY=VALUE...` | Changes one or more settings, keeping every other byte of the file |
| `edel settings reset KEY...` | Gives settings back to the release, as the app's Reset does |
| `edel settings diff [FILE]` | Shows what apply would change; exits 1 when anything would |
| `edel settings apply` | Makes the machine match its file |
| `edel settings import FILE` | Applies FILE and keeps it as the machine's file |
| `edel settings export` | Prints the machine as a settings file, to import on another one |
| `edel settings check FILE` | Checks a file strictly |

A mistyped key or value is answered with the nearest one: `edel settings set layout.presset=hive` says "did you mean layout.preset?". Every part reads the file with the one parser in `edel::settings` (`crates/edel/src/settings.rs`), whose key table this page follows; the reading rules are in [FORMATS.md](FORMATS.md).

## Where it lives

On the first boot, `edel settings apply` seeds the machine's file from the first of: a volume labelled `EDEL-SEED` holding a settings file, the EFI system partition's `/EFI/edel/`, and the slot's own in `/usr/share/edel/`. Without any, nothing is applied. The `edel-settings` service applies it at every boot, before the hostname is set. A person's own file in `~/.config/edel/` has the same keys, and for the pages the desktop follows at once (Layout, Displays, Appearance, Shortcuts) its values win over the machine's. The file's name is written once, in `edel::places`; a file by a name it had before is still read, and the next apply renames it.

"From" names the roadmap step that first acts on a key. Until then `edel settings check` and `set` refuse the key with "not supported yet", and the boot apply skips it and says so. `NAME` is any name: a person, a screen's connector, an action, a feature. A person's name has up to 32 lowercase letters, digits, `-` and `_`, and starts with a letter or `_`.

## Layout: `[layout]`

The desktop follows these keys at once (M4.5): it reads the machine's file and the person's, and reads both again whenever either is written.

| Key | Value | From |
|---|---|---|
| `layout.preset` | a preset this release has: `classic`, `hive` (M5.4a), `windows-like` (M5.4c) or `mac-like` (M5.4d), with `tablet` and `phone` to come with M9; missing is `classic`. A changed preset re-lays out the windows and restarts the panel at once; a name this release lacks gives Classic, and `edel settings diff` says so | M5.4a |
| `layout.tiling` | `true` or `false` | M4.5 |
| `layout.title_bars` | `always` or `floating-only` | M4.5 |
| `layout.tiling_style` | `stack`, `split` or `scroll`: how tiling lays windows out, one main window on the left and the rest stacked on the right (`stack`), each new window taking half of the focused one's space, cut across its longer side, as Hyprland tiles (`split`), or each window a column half the screen wide on a strip that scrolls to the focused one, as niri tiles (`scroll`, M5.16c); missing is `stack`. It never switches a workspace between floating and tiling: a floating one takes the style when it next tiles. The panel's layout button offers the same choice on a right click (M5.16b) | M5.16a |
| `layout.device_type` | `desktop`, `tablet` or `phone`; missing detects it | M9.3 |
| `layout.window_buttons` | `left` or `right`: the side of the title bars where close, minimize and maximize sit, close outermost; missing is the preset's (right in Classic and Hive) | M5.4b |
| `layout.close_button` | `true` or `false`: whether every title bar shows its close button; missing shows it. Super+Q closes a window whatever is shown | M5.18a |
| `layout.minimize_button` | `true` or `false`: whether every title bar shows its minimize button; missing shows it | M5.18a |
| `layout.maximize_button` | `true` or `false`: whether every title bar shows its maximize button; missing shows it. A hidden button's room goes to the title | M5.18a |
| `layout.panels` | panels in place of the preset's, each as a preset writes one: `edge` (`top` or `bottom`), `style` (`bar`, the default, or `dock`), for a dock `hide` (`never`, the default, or `covered`: it then keeps no space and hides while a window covers it, M5.4f) and the widgets by name in `start`, `centre` and `end`, at most one panel along each edge; as `[[layout.panels]]` tables, or on the command line `edel settings set 'layout.panels=[{ edge = "bottom", end = ["clock"] }]'`; missing is the preset's. A change restarts the panel at once | M5.4e |

## Displays: `[displays.NAME]`

Each screen by its connector's name, such as `eDP-1` for a laptop's own screen; the desktop follows these at once (M4.6). Settings' Displays page (M5.7a) lists the screens the compositor reports and draws them side by side; a scale of 100, 125, 150, 175 or 200 percent, a resolution from the screen's own list, a position and an on switch each write one of these keys, and a choice that is what the screen would get anyway (its preferred resolution, the scale worked out from its size) is taken out of the file, never written. `edel-settings --page displays` opens the app on that page. The last screen that is on cannot be turned off from the page.

| Key | Value | From |
|---|---|---|
| `displays.*.position` | two whole numbers, `[x, y]` | M4.6 |
| `displays.*.scale` | a number, such as `1.25` | M4.6 |
| `displays.*.resolution` | `WIDTHxHEIGHT`, such as `"1920x1080"`; missing is the screen's own | M4.6 |
| `displays.*.refresh_rate` | a number of Hz, such as `60`; missing is the resolution's best | M4.6 |
| `displays.*.enabled` | `true` or `false` | M4.6 |
| `displays.*.rotation` | `0`, `90`, `180` or `270` | M9.8 |

## Sound: no section

The Sound page (M5.7b) has no key. The volume, the mute switch and the device in use are the sound system's own live state, which WirePlumber remembers for each person, so a line of the file would only be a second copy that could disagree. The page and the command line are the same level all the same: each row copies the `wpctl` command that does what it does (`wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%`, `wpctl set-mute @DEFAULT_AUDIO_SINK@ 1`, `wpctl set-default ID`), and `edel settings get sound` says where to look.

## Appearance: `[appearance]`

| Key | Value | From |
|---|---|---|
| `appearance.wallpaper` | text, a file path | M5.12 |
| `appearance.mode` | `light`, `dark` or `auto`: the colours of the title bars, the panels and, through the settings portal, the apps; `auto` is the release's choice, dark in this one, and missing is `auto`. A change takes effect at once | M5.5c |
| `appearance.accent` | text, a token accent name or a hex colour | M5.12 |
| `appearance.font` | text, a font family | M5.12 |
| `appearance.font_size` | a number, in points | M5.12 |
| `appearance.cursor_size` | a whole number, in pixels | M5.12 |
| `appearance.icon_size` | a whole number, in pixels | M5.12 |
| `appearance.animations` | `full`, `reduced` (fades only) or `off`; missing is `full`. The desktop follows it at once | M5.11b |

## Shortcuts: `[shortcuts]`

| Key | Value | From |
|---|---|---|
| `shortcuts.*` | keys for the action `*`, such as `close_window = "Super+W"`, or `""` for none; the actions, their default keys and the key names are in [Keyboard shortcuts](SHORTCUTS.md). Check and set refuse an unknown action, two actions on one key and a way out (close a window, the launcher, the lock) without keys. The desktop follows them at once | M5.13a |

## Region: `[region]`

| Key | Value | From |
|---|---|---|
| `region.language` | the language shell-ui and Settings show their words in: a catalogue's name, such as `"de"` or `"pt_BR"` (`pt_BR` falls back to `pt`); missing is English, as is a word the catalogue lacks. The desktop restarts shell-ui in it at once; Settings takes it when next opened | M5.24a |
| `region.keyboard` | keyboard layouts as xkb names them: one, such as `"de"`, or up to four, such as `"us,ru"`, each with a variant if wanted, such as `"de(nodeadkeys)"`; missing is US. Check and set refuse a layout or variant xkeyboard-config lacks where its list is on the machine. The desktop follows it at once, and Super+Space (`shortcuts.next_keyboard_layout`) goes to the next layout | M5.21a |
| `region.timezone` | text, such as `"Europe/London"` | M6.6a |
| `region.time_servers` | the time servers the clock is set from, one to four host names, such as `["pool.ntp.org"]` or a cloud provider's own, `["169.254.169.123"]` (letters, digits, hyphens and dots; check and set refuse anything else); missing is `pool.ntp.org`. The `edel-clock` service of every machine image asks them once, in the background, each time the machine starts (busybox `ntpd`, up to 30 s, never holding the boot up), prints one line on the console and in the system log and writes the hardware clock; nothing keeps running afterwards. The next start uses a changed list. Its Settings row, "Time servers", comes with the Settings app's Region page, not built yet | M1.13 |

## Users: `[users.NAME]`

| Key | Value | From |
|---|---|---|
| `users.*.admin` | `true` or `false`: member of the `admin` group; missing is `false` | M2.2 |
| `users.*.ssh_keys` | list of public keys for `~/.ssh/authorized_keys`; missing leaves the file as it is | M2.2 |
| `users.*.login_shell` | a login shell's full path, such as `/bin/ash`, with no `:`; missing is `/bin/sh`. Apply leaves the shell as it is, and says so, when the path is not a program on the machine | M2.2 |

On a desktop (an image with the `seat` feature), apply also puts every person in `[users]` and the greeter's account in the `seat` group, so they may use the screen and input; there is no key for it (M4.2b). `reset users.NAME.admin` keeps `[users.NAME]`, so apply takes back what the key gave.

## Network: `[network]`

| Key | Value | From |
|---|---|---|
| `network.hostname` | the row "Device name (hostname)": letters, digits and hyphens, up to 63. Missing at the next apply removes the machine's own `/etc/hostname`, so the slot's shows again | M2.2 |

## Default apps, Startup, Power and Services

| Key | Value | From |
|---|---|---|
| `default_apps.browser` | text, a `.desktop` id | M6.10 |
| `default_apps.files` | text, a `.desktop` id | M6.10 |
| `default_apps.editor` | text, a `.desktop` id | M6.10 |
| `default_apps.terminal` | text, a `.desktop` id | M6.10 |
| `default_apps.mail` | text, a `.desktop` id | M6.10 |
| `startup.apps` | list of `.desktop` ids started with the session | M6.10 |
| `power.lid_close` | `suspend`, `lock`, `nothing` or `poweroff` | M7.8 |
| `power.lock_after_minutes` | a whole number of minutes without input before the screen locks; `0` never | M7.8 |
| `power.power_button` | `suspend`, `poweroff`, `ask` or `nothing` | M7.8 |
| `power.on_battery` | `power-saver`, `balanced` or `performance` | M7.8 |
| `services.*` | `true` or `false`: the switchable feature `NAME` on or off | M6.10 |

## Updates, Apps and Add-ons

| Key | Value | From |
|---|---|---|
| `updates.channel` | the channel whose release lists `edel update` takes, such as `"stable"` or `"preview"`: lowercase letters, digits and hyphens, up to 32; missing is the channel the image was built for. `edel update` with no release named takes this channel's list, and a list for another channel is refused unless `edel update --channel NAME` asks for it once. Settings' Updates page has it as the Channel row | M3.8, M5.8c |
| `updates.version` | text, a release to stay on; missing follows the channel | M3.4 |
| `updates.automatic` | `off`, `check`, `install` or `install-and-restart` | M7.5 |
| `updates.restart_window` | text, hours for unattended restarts, such as `"02:00-04:00"` | M7.5 |
| `apps.installed` | list of Flathub app ids, installed per admin user | M6.1 |
| `addons.installed` | list of add-on names (ADR-007) | M7.2b |

## System: `[system]`

| Key | Value | From |
|---|---|---|
| `system.variant` | `container`, `server`, `desktop` or `phone` | M7.3 |
| `system.developer_mode` | `true` or `false`: developer mode (ADR-007) | M2.2 |
| `system.profiles` | list of profile names, applied in order | M7.6 |

## Adding a key

Name it after its row in Settings: the label in lowercase with `_` for spaces, its section the page's name, its values the options' labels in lowercase with `-` for spaces (ADR-008's same names decision). Add it to `KEYS` and the structs in `settings.rs`, append it to `crates/edel/tests/keys.txt`, add its row here, and set `supported` in the step that acts on it. A key is never removed or renamed within a format (ADR-008).
