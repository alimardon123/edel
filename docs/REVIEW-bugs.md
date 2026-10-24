# Other projects' bugs, checked against ours (M8.11)

Roadmap step M8.11 reads the bug trackers of the projects nearest each part, sorts what they show into kinds of mistake, and checks each kind against our code and tests. Each kind we are open to gets a test and a fix. This file records the sources read, the kinds and what each led to. It is a first pass of 2026-10-24: the step stays open until every kind names its test, or why it does not apply to us, and the trackers still to read are read.

## Sources

Each tracker was read on 2026-10-24 as its 25 issues with the most comments (open and closed), the issues people meet most.

| Part | Read | Not read yet, and why |
|---|---|---|
| Compositor | niri, cosmic-comp, Sway, Hyprland | smithay (no issue matched its bug label; read its open issues next), wlroots (on freedesktop.org's GitLab) |
| shell-ui | Waybar, Cinnamon | GNOME Shell (its GitLab page gave no issue list to a plain fetch), the COSMIC panel, the Plasma panel |
| Settings | | GNOME Settings, Plasma's System Settings |
| Base, installer, updates | RAUC, bootc | Alpine's aports (its GitLab answered 403), postmarketOS, AerynOS, Fedora Atomic, Calamares (archived; its issues moved) |
| Tools | Flatpak | apk-tools (on Alpine's GitLab) |

## Kinds of mistake

| Kind | Seen in | Ours | What it led to |
|---|---|---|---|
| The clock and time zones | Waybar #3575 (the bar shows UTC while everything else is local) | Every clock in shell-ui read jiff's system zone, which jiff keeps for five minutes, so after a person picked another zone in Settings, the panel, the lock screen and the greeter showed the old one for up to five minutes, and the automatic dark mode's words did too | Fixed: `edel::timezones::now` and `system` read `/etc/localtime` again as soon as the link changes, and every clock uses them; test `timezones::tests::the_zone_follows_its_link_at_once` |
| A full disk | from memory: settings and state files cut short when the disk fills | `edel::settings::write_text` writes a new file and renames it over the old one, so the old file survives, but a write cut short by a full disk left the half-written `settings.toml.edel-new` behind, filling the disk more | Fixed: the half file is taken out when the write or the rename fails; test `settings::tests::a_failed_write_keeps_the_file_and_leaves_no_half_file`. The update writes into the other slot, not the data partition, and says when an image is too big for it (`update::too_big`) |
| User and group ids that drift between builds | bootc #673, bootc #1127 (no login after a reinstall) | A newer slot's accounts are added to the machine's own `/etc/passwd` at boot (`data::merge_accounts`, tested), and one whose id this machine already gave someone else is left out with a warning, because Alpine hands out system ids per build | Open: an account left out leaves its service without its user. Fixed ids for the accounts our packages make would close it; that touches the base and is written here for a step of its own, not changed in a review |
| A new base or a reinstall on the same data | bootc #1127, bootc #673 | People's apps, data and settings live only on the data partition | Tested: the desktop case `another-base` (M8.13a) starts a new root on the first system's data partition and finds mpv, its setting and an empty diff |
| Screens plugged in and out, docks, lids | Sway #7062 (layout not reapplied after a dock), cosmic-comp #906 (crash on reconnect), niri #798 and #996 (lid) | The compositor keeps each screen's settings by name in the settings file | Tested: `outputs`, `one-screen` (a screen turned off) and `lid`. Not tested: a screen unplugged and plugged back while windows are on it, which needs QEMU to hot-unplug a virtio screen; to look at next |
| Sleep and resume | Hyprland #804 (crash on wake), Waybar #3223 (segfault after resume), Sway #5759 (a screen that does not wake), Sway #2706 (lock screen slow after resume), niri #3391 | | Tested: `suspend` maps a new window after the wakeup and `lid` suspends again. Not tested: the lock screen taking a password right after a wakeup; to add to `suspend` |
| Fractional scaling and popups | Sway #6426 (popups wrong at scale other than 1), Sway #4924 (subsurfaces past the parent) | | Tested: `scale` at 2 and 2.5. Popups at a fractional scale are not tested; to look at next |
| Memory and file descriptors that grow over time | cosmic-comp #1179 and #2073, niri #1869, Cinnamon #6850, Sway #6310 (too many open files) | | Tested: `memory` (M7.15) and the compositor's and shell-ui's memory budgets. File descriptors are not counted; to add a count before and after the cases that open and close windows |
| Busy when idle | cosmic-comp #972, Waybar #846 and #1464 | | Tested: `wakeups` (M7.16a), the wakeups a second with nothing on screen |
| Input methods | niri #454 (fcitx5 popups in GTK 4), Flatpak #675 | | Tested: `input-method` (M6.8) types through fcitx5 in foot. A popup in a GTK 4 app is not tested |
| Keyboard state: Num Lock, Caps Lock, layouts | niri #1501 (Num Lock works only after a modifier), niri #1236, Waybar #2215 | No code of ours sets Num Lock at login | Open: to check what the compositor's keyboard starts with, and whether a key binding on the number row follows the layout (`keyboard` tests `de`) |
| The tray's watcher going away | Waybar #3468 and #3597 | shell-ui hosts the watcher | Tested: `tray`. Not tested: an app that registered before shell-ui restarts; to add to `respawn` |
| The screen handed from the greeter to the session | cosmic-comp #2690 (EBUSY on the DRM device), cosmic-comp #913 | The greeter is our compositor too | Tested in CI only on the virtual GPU (`live`, `fresh`); the laptop walk (M8.15b) covers real hardware |
| Switching between direct scanout and drawing | niri #310 (a freeze of several seconds) | | Tested in part: `fullscreen` and `smoothness`; the VM cannot scan out directly (`game_direct_percent_floor` is 0 there) |
| Clipboard between Wayland and X11 apps | Sway #4007, Hyprland #6247 | | Tested: `clipboard` (M7.13) between Wayland apps. Not tested with an Xwayland app |
| A cursor or a theme an app does not follow | Flatpak #709 and #740, Cinnamon #12994 (Firefox ignores the dark mode) | | Tested: `theme`. The cursor's size inside a Flatpak app is not tested |
| An update cut by power loss | RAUC #437 (slot busy), RAUC #360 (boot order not changed) | | Tested: `ab-test` (an update that never boots falls back) and the guard's cases; the GRUB variables keep RAUC's names |
| Very short-lived windows | Cinnamon #340, Sway #7303 (crash on close) | | Not tested: a window that maps and closes within one frame; to add a test client mode for it |

## Principles check

- **Reliable:** each kind is a way a person was left with a broken session elsewhere; the two fixed here showed a wrong time and filled a disk.
- **Simple and modular:** each fix is in the one place that owns it (`edel::timezones`, `edel::settings::write_text`), and every clock reads the same function.
- **Traded off:** the kinds marked "to look at next" wait for the rest of the step, so this pass did not grow the desktop tests' running time.
