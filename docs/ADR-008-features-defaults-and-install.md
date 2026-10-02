# ADR-008: Features, defaults and the way in

**Status:** Proposed
**Date:** 2026-10-02
**Deciders:** Alimardon
**Related:** ADR-002 (presets, no extension API; one line amended here), ADR-003 (same packages in every image), ADR-006 (system file), ADR-007 (customization levels, add-ons, developer mode), [design principles](DESIGN-PRINCIPLES.md), [roadmap](ROADMAP.md)

## Context

On 2026-10-02 Alimardon wrote three wishes:

1. "I want to be able to easily and dynamically replace, change, add, renew or drop some features from the project." This is about us as maintainers: a feature of the OS or the shell should be something we can add, swap for another implementation, refresh or remove without surgery across the codebase.
2. "Users should be able to have some flexibility too, with smart and really beautiful defaults."
3. "Installation and the options part should be really easy and user-friendly, for the best user experience, so everybody gets smooth with it, whether in the UI or in the command line or any other method."

The obvious answer is a feature framework: a registry of features with dependencies, versions and hooks, runtime-loaded modules, and a schema that generates the Settings UI. We reject it. It would be a fifth part in all but name, ADR-002 (no extension API) and ADR-007 (no theme engine) already rule it out, and most of what it would do is already planned: image definitions, presets, the system file, add-ons, profiles, Settings pages and `edel system`. So this ADR asks a narrower question: **what is actually missing?**

**A feature is spread out.** Under the current plan Bluetooth lives in six places: a package in the desktop image, its service in the image's service list, an allowlist hardcoded in `edel` (M6.10), a Settings page (M5.8), a quick-settings tile with its D-Bus code (M5.9) and a basics row (M6.7). Kernel modules sit in every image's `modules=` line. Printing appears twice, as an allowlisted base service and as an add-on (M7.6); the containers package list is written twice (M7.4, M7.6). M3.3 and M4.1 call images "vm plus" and "laptop plus", but `crates/edel/src/def.rs` has no include, so each image would copy the lists below it.

**Three readers meet files written by another release:**
- The old slot's updater reads every future `release.toml`. If it refuses unknown fields, as every loader does today, the first field we add stops all updates.
- After a fallback, the old slot reads a `system.toml` the newer slot wrote. With `deny_unknown_fields` (M2.1) it refuses the whole file, and serde also fails it on one new enum value (`color_scheme = "sepia"`).
- A name we drop (preset, profile, add-on, service) must not stop a machine from booting or updating. M7.2 keeps the slot off when an add-on cannot be fetched, so the first add-on we drop would strand every machine that named it.

The first two cannot be fixed later: the code that must be lenient is the code already on people's machines. That is why part of this ADR lands in phase 1.

## Decision

**Four conventions on files and code we already have, each enforced by a check, plus small additions to planned commands.** No new part, no daemon, no API, no runtime-loaded code. "Dynamic" means, for us, one PR and one A/B update that rolls back; for users, settings that apply at once and add-ons that arrive at the next boot.

### 1. A feature is one file, and one module per part

From M4.0, `features/NAME.toml` is the unit the OS is made of:

```toml
# features/bluetooth.toml; features/bluetooth/, if present, is copied over the root
format     = 1
summary    = "Bluetooth devices and audio"   # Settings, `edel addon list`, release.toml
why        = "Headphones and mice are a basic (principle 6); the switch turns it off for people without any."
packages   = ["bluez", "bluez-openrc"]       # check in CI
modules    = []                              # kernel modules for modules=; `initramfs` lists mkinitfs features
switchable = true                            # services.bluetooth = true | false
addon      = false                           # true: also built as a signed add-on (M7.2a)
health     = []                              # sentinels it writes; the compositor's is ["compositor"]

[services]
default = ["dbus", "bluetooth"]              # dbus may also be listed by network, flatpak, ...
```

- **Image definitions list features** (format 2): `features = ["base", "kernel", "ab-boot", "ssh"]`, `off = ["ssh"]` for switchable features that ship with their services off (written as `EDEL_SERVICES_OFF` in os-release, next to `EDEL_HEALTH`), and image facts only (name, variant, arch, hostname, `[alpine]`, `[vm]` without `modules=`, `[image] health_timeout`, `[release]`). Packages, services, modules and files live only in feature files: one way to do each.
- **`edel image build` merges** the listed features in order: packages, services and modules as sets, so a shared service such as `dbus` is listed by every feature that needs it. It refuses an unknown feature, a package or service without `why` (principle 3's written reason, now a build check), one service in two runlevels, a file path shipped by two features, and an `off` entry that is not switchable. It writes the union of `health` into `EDEL_HEALTH`, so an image cannot carry the compositor without its health check, and copies each feature file to `/usr/share/edel/features/NAME.toml`.
- **An add-on is a feature with `addon = true`.** Its build moves the packages' `/etc/init.d/*` to `/usr/lib/edel/init.d/`, the rest of `/etc` to `/usr/lib/edel/etc/NAME/` and `/var` to `/usr/lib/edel/var/NAME/`, and refuses a tree with anything left outside `/usr`. `edel system apply` links the `/etc` entries and init scripts into the `/etc` overlay and the runlevel and removes its links when the feature is gone, so a rollback leaves no dangling service; `mount-data`'s `/var` top-up (M1.3) copies the `/var` entries. Add-ons are lowerdirs of the one `/usr` overlay, so `/usr/share/edel/features/` lists every feature on the machine with no merge code. M7.2's separate `addons/*.toml` format is never created, the containers list is written once, and M10.1's platform add-on is `features/platform.toml`.
- **The name is the interface; the inside is private.** `[services]` keys are feature names: `services.bluetooth = false` stops the services of the feature `bluetooth` that no other enabled feature lists. The M6.10 allowlist becomes "the features present that are `switchable`". Swapping bluez or NetworkManager never breaks a user's file; the daemon's own state (saved Wi-Fi networks under `/etc`) is converted by the replacing PR or named in its release notes.
- **Code that talks to an OS feature sits in one module per part:** `crates/settings/src/features/NAME.rs` and `crates/shell-ui/src/features/NAME.rs`, each registered by one table line with `needs = "NAME"` and hidden or skipped when the machine lacks that feature's file. A cargo test in each crate reads `features/*.toml` and fails when a module names a feature with no file, and basics-test rows are keyed by feature name. A feature's whole footprint is one directory listing, and dropping it cannot leave dead code behind.
- **What feature files do not have:** dependencies between features (apk resolves packages), versions (the release's is theirs), install scripts, alternatives to choose from, UI declarations. The `apps` feature alone has `flatpak` and `flatpak_dropped`, because a person's data in `~/.var/app` outlives the release and a replaced default app stays for those who used it; a setting key needs no such list, since its value lives in the person's file. Readers on a machine read feature files leniently (an old release reads the platform add-on's file); `edel image build` and `image check` are strict.

**Shell features stay code**, as ADR-002 made them. A window policy or a widget is one module plus one line in a static table in its part. Setting keys are named after what the user sees (`shell.tiling`, never `master_stack`), so another algorithm behind the same trait keeps every user's value, and a cargo test fails when a preset names a widget the table lacks.

### 2. Strict writers, lenient readers

| Who | Unknown key | Unknown value or type | Unknown name (preset, profile, add-on, feature) | Newer format |
|---|---|---|---|---|
| Checkers: `edel system check`, the installer, CI | Refused, naming the key | Refused, listing the allowed values | Refused, listing the known names: features from `/usr/share/edel/features/`, add-ons from the cached manifest, else reported as unverified | Refused |
| Writers: `set`, `unset`, Settings | Refused for the key being written; keys already in the file are kept and reported | Refused for the value being written | As checkers | They change `system.toml.v<N>` and say the change applies to this release only |
| Unattended readers: boot apply, compositor, shell-ui, updater, feature-file readers | Kept, ignored, reported by `edel system diff` and on the System page | The default is used and reported | The default is used and reported; a dropped add-on is skipped with a notice | `system.toml`: read `system.toml.v<N>` beside it, else apply nothing and say so; `release.toml`: a stepping stone |

- **One parser for every row.** `edel::system`, a library target inside `crates/edel` created in M2.1 with no network or signing dependencies, holds the structs, the defaults table and the checks; the compositor (M4.5), shell-ui (M5.1) and Settings (M5.6) link it. The structs drop `deny_unknown_fields`, collect ignored keys with `serde_ignored` (check in CI) and read enum-valued keys leniently (a `#[serde(other)]` variant, or a string mapped after parsing).
- **Writes keep what they do not understand.** `set`, `unset` and Settings edit the file with `toml_edit`, so comments, order and keys a newer slot wrote survive byte for byte. Lenient reading alone would still lose them at the first write after a rollback.
- **`release.toml` readers ignore unknown fields**; the signature still covers them. A breaking manifest change bumps its format, publishes it under a new file name and keeps `release.toml` pointing at a stepping-stone release whose `edel` reads both.
- **A key is never removed or renamed within a format.** From M2.1 a cargo test checks the structs against an append-only `crates/edel/tests/keys.txt`, and CI fails a PR that deletes a line without a format bump; M6.11 extends this to every kept release tag. A dropped key gets `retired = "VERSION"` in the help table: still parsed, reported as no longer used, refused by `set`. At the next bump `edel migrate` deletes it and leaves `system.toml.v<N>` for the slot that still reads format N. An unavoidable rename reads both names, and `set` keeps the name the file already uses.
- **A dropped add-on never strands a machine.** An add-on in `[addons] add` that the new signed manifest does not list is skipped and reported; one that is listed but fails to download still keeps the slot off.
- **Image definitions and presets stay strict:** CI and the slot that ships them are their only readers.

### 3. Defaults: the file holds only what a person chose

- **An absent key means "the release decides".** One order decides every key: user file, then machine file, then profiles in listed order, then preset, then the release default (tokens, the parts' own values, image facts such as the hostname and `EDEL_SERVICES_OFF`, or the hardware: effect tier, form factor, output scale). Writers never write a default: the installer writes `profiles = ["office"]`, not the profile's contents. Apply enforces the release default for every absent key it owns (for a file in the `/etc` overlay, by removing the upper copy), so `unset` really undoes.
- **Defaults can come back and say where they come from.** `edel system unset KEY` removes a key; `edel system get KEY` prints the value and its layer (`shortcuts.close = Super+Q (preset classic)`). A Settings row shows `Automatic (current value)` when its key is absent and has Reset; a value from the machine file or a profile shows `Set by this machine` or `Set by profile office` instead of a Reset that cannot work. Per-user tables (`[shell]`, `[outputs]`, `[appearance]`, `[shortcuts]`, `[defaults]`, `[startup]`) go to the user file when run as a user; machine tables go to the machine file under doas and are refused without it, with the exact command to run.
- **Renewed, and reviewed before it ships.** A redesigned token, preset or profile reaches every machine that did not override it, which is how the look is renewed without a theme engine. The help table gives each key its default and source; `docs/defaults.md` is generated from it and diffed in CI (M6.11); every PR summary shows a "visible defaults changed" section against the last kept tag; the release checklist has a Defaults reviewed box with first-boot screendumps of every preset, light and dark, at scale 1 and 2 (M8.7). `release/*` branches refuse changes under `design/` and `presets/` except `look-fix:` commits (M8.5).

The first out-of-box state; `docs/defaults.md` holds it once generated:

| What | Default | Step |
|---|---|---|
| Layout | Classic (ADR-002) | M5.1 |
| Style and accent | Light; blue, one of nine named token accents (hex also accepted); every text-on-surface pair and accent at WCAG 4.5:1 for text and 3:1 for icons and focus rings, in both schemes, as a cargo test | M5.12 |
| Fonts | Inter; Noto for other scripts (`font-noto-all`, `font-noto-emoji`); `font-noto-cjk` measured against the budget, else a `fonts-cjk` add-on the installer adds for CJK languages (check in CI) | M5.12 |
| Display scale | From the EDID's physical size: about 125 logical pixels per inch for built-in panels and 110 for external ones, rounded to 0.25, clamped to 1 to 3, never written to the file | M4.6 |
| Touchpad | Tap-to-click on; natural scrolling on in Mac-like only | M4.6 |
| Language, keyboard | Asked first at install; a non-Latin layout gets `us` as a second layout | M6.6a |
| Hostname | Prefilled as user and form factor (`ali-laptop`), under More options | M6.6a |
| Apps | Browser, files, editor and store, offline in the slot; a replaced default app stays for every user whose `~/.var/app/<id>` exists | M6.2 |
| Services | ssh off on desktop and laptop (`off = ["ssh"]`), on for vm and server; bluetooth and avahi on where present | M4.0, M6.10 |
| Power | Lid locks then suspends; on AC, idle dims at 5 minutes and locks at 10 | M6.10, M7.8 |
| Updates | Desktops check and flag; servers install in their window | M5.9, M7.5 |
| Users | None in the slot; the installer, a seed or the console first boot creates the first admin; never a default password; the live session's user is created at boot only when `/` is on removable media, never in the slot | M6.6a, M6.6b |

### 4. One way in, behind three doors

- **Every level-1 key is reachable three ways:** a Settings row, `edel system get|set|unset`, and a line in the generated `docs/system-file.md`; a deliberately command-line-only key is listed with its reason. A cargo test over `crates/settings/src/rows.rs` enforces it (M6.11), so principle 5's "normal use never needs a terminal" is checked rather than remembered.
- **Same words, same checks, same timing.** A bad value gives the same message in a Settings row, from `edel system set` and from `edel system check FILE` (with the file position in front); `KEY+=VALUE` and `KEY-=VALUE` edit lists. `set` and Settings apply at once; a hand edit applies at the next boot or `edel system apply`, and `edel system diff` lists what is pending.
- **One question list,** in `edel::system`, rendered in this order by Settings, the terminal and the console first boot: language (it suggests keyboard and timezone), keyboard (applied live before any password is typed), name and password (or an ssh key on vm and server), disk; timezone prefilled; under More options hostname, preset (Classic), profile (none) and, from M8.2, encryption.
- **One disk planner, and the one command that erases a disk shows what it will erase.** `edel install --list` prints every candidate disk (model, size, partitions with labels and filesystems such as `Windows (NTFS, 420 GB)`, removable, the boot medium marked); `edel install DISK --dry-run` prints the plan; a terminal run asks the person to type the disk's name; a run with neither a terminal nor `--yes` exits 3 (M2.4). Settings shows the same list and plan word for word; its Erase and install button stands for typing the name, and only then does it run `doas edel install DISK --system FILE --yes`.
- **System files come from a path or an `https://` URL only.** The file is unsigned and creates admins and ssh keys, so `http://` is refused.
- **Unattended installs are safe by default.** `[install]` takes `disk = "auto"` (the `--list` rule: the one non-removable disk that is not the boot medium), `erase = "empty-only"` unless a `/dev/disk/by-id/` name and a console countdown say otherwise, and `then = "poweroff"` by default. The `edel-install` service runs in the background, like the guard, waits until the running slot is confirmed, then runs `edel install --unattended` (reads `[install]`, implies `--yes`). It writes `/data/edel/installed-to` on the stick and skips while that disk is present; export never writes `[install]` and the installed copy drops it, so neither the stick nor the disk installs twice.

Every install path ends in one `system.toml` and one `edel system apply`:

| Who | How |
|---|---|
| Anyone with a PC | Write the image to a stick (`dd`, or a stick writer CI or Alimardon has tried on Windows and macOS; `docs/INSTALL.md` shows the sha256 check there, the signature check on Linux, turning Secure Boot off and the boot-menu keys), boot it, Settings > Install (M6.6a), or Use a system file (M6.6b) |
| A terminal or a bare-metal server | `edel install` asks the same questions (M6.6b) |
| Scripted | `edel install DISK --system FILE` or `--system https://...` (M2.4, M6.6b) |
| A VM or server image without a seed | The console first boot asks for a user with a password or ssh key, then starts getty (M6.6b) |
| VM, cloud, fleet | `system.toml` on an `EDEL-SEED` volume (M2.2) or baked into a fleet image; with `[install]` the stick installs itself (M7.3) |
| After installing | Settings, `edel system set`, or the file; an add-on chosen today arrives at the next boot (M7.2b) |

### 5. Worked examples

`docs/FEATURES.md` (M4.0) copies these as the maintainer's how-to. Each is one PR and one A/B update that rolls back.

| Change | Files the PR touches | What machines see |
|---|---|---|
| Add printing | `features/printing.toml` (`addon = true`, `switchable = true`) and `features/printing/`; one line in `profiles/office.toml`; `crates/settings/src/features/printing.rs` and its line in `rows.rs`; the `printing` row in `ci/basics-test.sh` | Nothing until someone picks it (`edel system set addons.add+=printing`, a button, or the office profile); then it arrives at the next boot |
| Replace NetworkManager | `features/network.toml` (packages, services); `crates/settings/src/features/network.rs`; `crates/shell-ui/src/features/network.rs` | Every user's keys unchanged; saved networks converted or the notes say so |
| Replace master-stack tiling | One `WindowPolicy` module and its line in the compositor's policy table | `shell.tiling` keeps its meaning and every value |
| Renew the accent | `design/tokens.toml` | Every machine that left `appearance.accent` absent; the PR summary and the release checklist show the change |
| Drop `sway-baseline` | Delete `features/sway-baseline.toml` and `features/sway-baseline/`, one line in `images/desktop.toml`, the `baseline` case in `ci/desktop-test.sh` | Nothing; no key named it |
| Drop a key, preset or profile | A key gets `retired` and goes at the next format bump; a preset or profile file is deleted | The key is reported as no longer used; a dropped name falls back to Classic or no profile, reported |

CI proves every row as it proves everything else: the images build, the boot, ab, install and upgrade tests pass, budgets hold, and the package diff (M3.1) shows what moved.

## Options Considered

| Option | Verdict |
|--------|---------|
| A. Four conventions on files and code we already have; four new roadmap PRs (M4.0, M6.6b, M6.11, M7.2b) | **Recommended** |
| B. A feature framework: manifests with kinds, roles, dependencies and hooks, runtime-loaded modules, a schema that generates Settings | A fifth part in all but name; ruled out by ADR-002 and ADR-007; every hook is a way to break an update |
| C. Change nothing and handle each feature by hand | Works for a dozen features; then copied lists, the hardcoded allowlist and scattered D-Bus code make every change surgery, and strict readers strand machines at the first drop |
| D. A defaults file as its own layer under the machine file | Absence and one resolve order already give the layering; a second file of values is a second place to look |
| E. An append-only retired-keys file with migration verbs | The old slot never sees a file added later; `keys.txt` checked in CI and `retired` in the help table are enough until the first real bump |
| F. A separate installer app, a text-UI installer or an ISO | The Settings page and `edel install` on a terminal cover both, with one question list and one planner |

Not built, and defended: a plugin or extension API; dependencies, versions or alternatives between features; feature flags in shipped images; a schema-generated Settings UI; per-domain verbs that change settings (`edel addon add`; settings change only through `edel system set|unset`); shell completion scripts (busybox ash has no programmable completion); telemetry; a stick writer of our own for Windows or macOS; netboot; a look-change notice (see Revisit).

## Consequences

- **Easier:** adding an OS feature is one file, its modules and one line; dropping one is deleting them, and a cargo test finds what is left. Machines that used a dropped feature keep updating and are told. Replacing a daemon or a tiling algorithm never touches users' files. A rollback never loses a key a newer slot wrote, and the first new manifest field or enum value stops nobody. Better defaults reach everyone who did not override them, are reviewed before each release, and can always be reset. Every option is in Settings, on the command line and in the file, every install path ends in the same file, and an installed desktop does not accept ssh until someone turns it on.
- **Harder:** at boot, a typo in a hand-edited file is reported, not refused (`check` and Settings still refuse it). A manifest format change costs a stepping-stone release, and keys only disappear at a format bump. Every package or service needs a `why`; every module that talks to the OS sits in `features/`.
- **Cost:** about 1,800 lines; four new PRs (M4.0, M6.6b split from M6.6, M6.11, M7.2b), the rest inside planned steps. Phase 1 grows by about 400 to 600 lines with tests in M1.6, M2.1, M2.3 and M2.4, all Reliable work, with no new step and no reordering; `get` with layers and the nearest-key suggestion wait for M6.11.
- **Amends ADR-002:** "Users can share presets as files" becomes sharing a layout as the `[shell]`, `[appearance]` and `[shortcuts]` tables of an exported system file; presets ship in the slot.
- **Revisit:** an alias table the first time a name really must change; per-feature CI tests if image tests stop isolating failures; a look-change notice with Keep my old look (limited to `[appearance]` and `[shortcuts]`) at the first visible redesign after the first public release; user preset files if people ask; Settings search once the help table exists.

## Action Items

1. [ ] `release.toml` readers ignore unknown fields; the stepping-stone rule in `docs/FORMATS.md` (M1.6).
2. [ ] `edel::system`; strict checking and lenient boot reading of `system.toml` (keys, enum values, names), `system.toml.v<N>`, `keys.txt` (M2.1).
3. [ ] `unset`, release defaults enforced by apply, writes with `toml_edit` (M2.3); one plan for dry run, confirmation and exit 3, test service in the background (M2.4).
4. [ ] Feature files, image definitions format 2 with `off`, modules from features, `docs/FEATURES.md`, health from features (M4.0, M4.1, M4.8).
5. [ ] The compositor links `edel::system`, role-named keys (M4.5); scale from the EDID (M4.6).
6. [ ] Feature modules and widget table (M5.1); preset fallback (M5.4); resolve order, Automatic, Reset, `Set by` (M5.6); pages and tiles as feature modules (M5.8, M5.9); defaults, fonts and the contrast test (M5.12).
7. [ ] Default apps kept (M6.2); question list, `--list`, the installer page (M6.6a); terminal, console first boot, https-only system files, serial helper, `docs/INSTALL.md` (M6.6b); basics rows by feature (M6.7); `[services]` by feature (M6.10); help table, `get`, `docs/defaults.md`, the defaults diff (M6.11).
8. [ ] Add-ons as features, files moved under `/usr`, the skip rule (M7.2a); add-ons at the next boot (M7.2b); unattended install (M7.3); containers once (M7.4); profiles by name (M7.6); serial helper reused (M8.2); the `release/*` freeze (M8.5); Defaults reviewed (M8.7); platform as a feature (M10.1).
9. [ ] List ADR-008 in `docs/README.md`; amend ADR-002's line on sharing presets and point ADR-007's add-on format item at feature files; add the boot-reader tie-break to `DESIGN-PRINCIPLES.md`.

## Principles check

- **Reliable** drove where the work lands. Three rules must be on machines before anything is added or dropped: the lenient manifest (M1.6), the lenient system file with its versioned sibling, lenient enum values and byte-preserving writes (M2.1, M2.3), and skipping add-ons the manifest no longer lists (M7.2a). Each goes where its reader is first written. Unknown names fall back instead of failing boot, the installer shows what it will erase, unattended installs wait for a confirmed slot and refuse non-empty disks, a bad add-on at the next boot comes back without it, and system files never travel over plain http.
- **Instant:** nothing new runs on the compositor's main loop; feature presence is one `stat` when a page or widget loads, and scale is computed at hotplug.
- **Simple:** no part, no daemon, no API, no runtime-loaded code. One feature format serves baked features and add-ons and replaces the add-on format M7.2 would have added, so the format count is unchanged. One library parses the system file for all four parts, one directory records what a machine has, the services allowlist moves from code to data, and every install path converges on one file, one question list and one planner.
- **Efficient:** feature files cost nothing at runtime, duplicated package lists disappear, CJK fonts are measured before they ship, and there is no telemetry.
- **Beautiful:** the file holds only choices, so redesigned defaults reach everyone and are reviewed in screendumps before each release; the contrast test and the scale default make the first screen good on every panel.
- **Functional:** fonts for the shipped scripts, a Latin layout beside a non-Latin one, add-ons that arrive without waiting for a release, and "no terminal for normal use" as a CI check.
- **Versatile** is still served only by presets, profiles and add-ons.
- **Traded off:** Versatile loses alternatives, user preset files and convenience verbs; strictness at boot loses to Reliable; Beautiful waits for a look-change notice; Simple pays about 1,800 lines, four PRs, a `why` per package and one module per feature per part.
