# The system file

**Date:** 2026-10-02

One TOML file describes a whole machine (ADR-006): `/data/edel/system.toml`. It holds only what a person chose; an absent key means the release decides (ADR-008). It never holds personal files or secrets. `edel system check FILE` checks one strictly, `edel system apply [FILE]` makes the machine match it, `edel system diff [FILE]` shows what apply would change (exit 1 when anything would), `edel system set KEY=VALUE` and `unset KEY` change one key of the machine's file in place (unsetting a user's last key keeps `[users.NAME]`, so apply takes back what the key gave), and `edel system export` prints the machine as one, with the `/etc` files it changed listed in comments; every part reads it with the one parser in `edel::system` (`crates/edel/src/system.rs`), whose key table this page follows. The reading rules are in [FORMATS.md](FORMATS.md).

```toml
format = 1

[network]
hostname = "ali-laptop"

[users.ali]
admin = true
ssh_keys = ["ssh-ed25519 AAAA... ali@desk"]

[appearance]
color_scheme = "dark"
```

On the first boot, `edel system apply` seeds the file from the first of: a volume labelled `EDEL-SEED` holding `system.toml`, `/EFI/edel/system.toml` on the EFI system partition, and the slot's `/usr/share/edel/system.toml`. Without any, nothing is applied. The `edel-system` service applies it at every boot, before the hostname is set; `apply FILE` applies another file and makes it the machine's own. An absent key gets the release's value at the next apply: an absent `network.hostname` removes the machine's own `/etc/hostname`, so the slot's shows again.

"From" names the roadmap step that first acts on the key. Until then `edel system check` refuses the key with "not supported yet" and the boot apply skips it and says so. `NAME` is any name: a user, an output, an action, a feature. A user name has up to 32 lowercase letters, digits, `-` and `_`, and starts with a letter or `_`.

## `[system]`

| Key | Value | From |
|---|---|---|
| `system.channel` | text, such as `"stable"` | M3.4 |
| `system.version` | text, a release to stay on; absent follows the channel | M3.4 |
| `system.variant` | `container`, `server`, `desktop` or `phone` | M7.3 |
| `system.developer` | `true` or `false`: developer mode (ADR-007) | M2.2 |
| `system.profiles` | list of profile names, applied in order | M7.6 |

## `[users.NAME]`

| Key | Value | From |
|---|---|---|
| `users.*.admin` | `true` or `false`: member of the `admin` group; absent is `false` | M2.2 |
| `users.*.ssh_keys` | list of public keys for `~/.ssh/authorized_keys`; absent leaves the file as it is | M2.2 |
| `users.*.shell` | a login shell's full path, such as `/bin/ash`, with no `:`; absent is `/bin/sh`. Apply leaves the shell as it is, and says so, when the path is not a program on the machine | M2.2 |

On a desktop (an image with the `seat` feature), apply also puts every person in `[users]` and the greeter's account in the `seat` group, so they may use the screen and input; there is no key for it (M4.2b).

## `[network]` and `[locale]`

| Key | Value | From |
|---|---|---|
| `network.hostname` | a hostname: letters, digits and hyphens, up to 63 | M2.2 |
| `locale.language` | text, such as `"en_GB"` | M6.6a |
| `locale.keyboard` | text, an XKB layout such as `"us"` | M6.6a |
| `locale.timezone` | text, such as `"Europe/London"` | M6.6a |

## `[shell]` and `[outputs.NAME]`

The compositor follows these keys at once (M4.5, M4.6): it reads the machine's file and the person's `~/.config/edel/system.toml`, where a key set wins over the machine's, and reads both again whenever either is written. `edel system apply` leaves them to it.

| Key | Value | From |
|---|---|---|
| `shell.preset` | `classic`, `mac-like`, `windows-like`, `tiling`, `tablet` or `phone` | M5.4 |
| `shell.tiling` | `true` or `false` | M4.5 |
| `shell.title_bars` | `always` or `floating-only` | M4.5 |
| `shell.form_factor` | `desktop`, `tablet` or `phone`; absent detects it | M9.3 |
| `outputs.*.position` | two whole numbers, `[x, y]` | M4.6 |
| `outputs.*.scale` | a number, such as `1.25` | M4.6 |
| `outputs.*.mode` | text, such as `"1920x1080@60"` | M4.6 |
| `outputs.*.enabled` | `true` or `false` | M4.6 |
| `outputs.*.transform` | `0`, `90`, `180` or `270` | M9.8 |

## `[appearance]`

| Key | Value | From |
|---|---|---|
| `appearance.wallpaper` | text, a file path | M5.12 |
| `appearance.color_scheme` | `light`, `dark` or `auto` | M5.12 |
| `appearance.accent` | text, a token accent name or a hex colour | M5.12 |
| `appearance.font` | text, a font family | M5.12 |
| `appearance.font_size` | a number, in points | M5.12 |
| `appearance.cursor_size` | a whole number, in pixels | M5.12 |
| `appearance.icon_size` | a whole number, in pixels | M5.12 |
| `appearance.motion` | `full`, `reduced` (fades only) or `off`; absent is `full`. The compositor follows it at once | M5.11b |

## Behaviour: `[shortcuts]`, `[defaults]`, `[startup]`, `[power]`, `[services]`

| Key | Value | From |
|---|---|---|
| `shortcuts.*` | text, keys for the action `NAME`, such as `close = "Super+Q"` | M5.13 |
| `defaults.browser` | text, a `.desktop` id | M6.10 |
| `defaults.files` | text, a `.desktop` id | M6.10 |
| `defaults.editor` | text, a `.desktop` id | M6.10 |
| `defaults.terminal` | text, a `.desktop` id | M6.10 |
| `defaults.mail` | text, a `.desktop` id | M6.10 |
| `startup.apps` | list of `.desktop` ids started with the session | M6.10 |
| `power.lid` | `suspend`, `lock`, `nothing` or `poweroff` | M7.8 |
| `power.idle` | a whole number of minutes before the screen locks; `0` never | M7.8 |
| `power.power_button` | `suspend`, `poweroff`, `ask` or `nothing` | M7.8 |
| `power.on_battery` | `power-saver`, `balanced` or `performance` | M7.8 |
| `services.*` | `true` or `false`: the switchable feature `NAME` on or off | M6.10 |

## `[updates]`, `[apps]` and `[addons]`

| Key | Value | From |
|---|---|---|
| `updates.auto` | `off`, `check`, `install` or `boot` | M7.5 |
| `updates.window` | text, hours for unattended restarts, such as `"02:00-04:00"` | M7.5 |
| `apps.flatpak` | list of Flathub app ids, installed per admin user | M6.1 |
| `addons.add` | list of add-on names (ADR-007) | M7.2b |

## Adding a key

Add it to `KEYS` and the structs in `system.rs`, append it to `crates/edel/tests/keys.txt`, add its row here, and set `supported` in the step that acts on it. A key is never removed or renamed within a format (ADR-008).
