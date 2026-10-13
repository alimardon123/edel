# Review: the whole project, 2026-10-13

**Date:** 2026-10-13
**Reviewed:** this repository at `main` `da86609c` (the code, CI and docs), read offline; no other sources

Asked by Alimardon on 2026-10-13: a review of the whole project "as a third person architect and software lead", against the principles, modularity, reuse and flexibility while staying simple, light, fast and versatile, with the base and the desktop able to become independent of each other. Four Opus reviewers each read one area offline (architecture, the reliability core, the desktop's code, process and docs); the session checked their main claims in the code and marks below what it checked. The fixes are planned as M6.17, after the rest of M6.

Code reviewed: `main` at `da86609c`, about 144,000 lines of Rust (shell-ui 51k, the `edel` tool and library 42k, Settings 27k, the compositor 22k, the test client 2k), 9,500 lines of CI shell, 442 crates in `Cargo.lock`.

## Verdict

**Verdict: the engineering is good; the proportions are wrong.** The core is careful work: signed and checked updates that a power cut cannot break, tests that boot real virtual machines, and parts kept apart by checks in CI. But the project grows faster than it is proven: nobody outside has run it, and the plan, the docs and the desktop tests have grown heavy enough to slow every change.

## Against the principles

| Principle | Standing | Evidence |
|---|---|---|
| Reliable | Strong core, three holes | Updates checked before and after the write; the three holes are under "Found and checked" |
| Instant | Good, one cost | Frame p99 budget 4.4 ms, no idle frames, slow work off the drawing thread; window shadows repainted at full size on every resize frame |
| Simple | Weakest | 144,000 lines in about 12 days; the lock screen alone about 19,000; 1 MB of rule docs; a 7,200-line test script |
| Efficient | Mostly good | Container 7.8 MB, VM root 110 MB, VM boot 7.7 s and 76 MiB, shell-ui 1.8 MiB of its own; against: a 700 MB desktop image, the compositor's 169 MiB, the winit development backend shipped in the compositor, a network poll every minute |
| Beautiful | Strong intent | Tokens and mockups everywhere; but three ways to draw text and corners (compositor, shell-ui, Settings) |
| Functional | Mostly there | The basics checklist runs in CI; no touch input (planned in M9.4), quick settings and notifications not reachable by keyboard or screen reader |
| Powerful | Not yet | Developer mode (M7.1) is still to come |
| Scalable | Strong | One base makes the container, VM, server and desktop images; one settings file copies a machine |
| Versatile | Strong, past need in places | Presets, tokens, themes, editable panels, own presets, every setting three ways; the lock screen's editor goes past what a first release needs |
| Modular and reusable | Good between parts, weak inside them | See the two independence goals below |

## The two independence goals

**The base stays replaceable (ADR-010).** ADR-010 puts Alpine's details in `features/base.toml`, the image builder and `edel::places`. `doas` is one named fact there (`places::AS_ROOT`), and apps are Flatpaks, so they do not care about the base. But Alpine's tools (`rc-update`, `rc-service`, busybox's `adduser`, `cryptpw`, `chpasswd`, `mdev`, `zzz`) are named in 14 of the tool's modules (`machine.rs`, `data.rs`, `installer.rs`, `first_boot.rs`, `system_power.rs`, `guard.rs` and others), and no check keeps them in one place. Swapping the base would mean editing all of them.

**The desktop stays separable (ADR-002, M5.15).** Between crates it holds: no desktop part depends on another (`ci/seams.sh`), and the desktop builds and runs on Ubuntu without Edel OS's folders (`ci/elsewhere-test.sh`). At run time it does not yet: shell-ui's power menu runs `edel restart`, `shutdown` and `suspend` (`shell-ui/src/power.rs`), which another distribution lacks; Settings starts `edel-shell-ui --look-picture` and calls `org.edel.Shell` over the session bus, which `ci/seams.sh` does not see; and the shared `edel` library mixes the settings format, desktop-only models (`lock_screen`, `themes`, `panel_edit`) and the OS's own work (users, services), so whoever takes the desktop takes all three.

## Found and checked

Each was read in the code by the session on 2026-10-13.

1. **The guard can restart a machine every 2 minutes for ever.** `guard.rs`'s `on_timeout` looks only at whether a watchdog exists, never at GRUB's environment; when every slot used its tries, `grub.cfg` (`boot.rs`) starts one anyway, and the guard restarts again. It happens when both slots fail their health, as a stick does on graphics the compositor cannot start; that stick never writes the hardware report it exists for.
2. **An install onto a USB disk logs in by itself and may install without a password.** `install::is_removable` counts any disk on USB as removable, the installer offers such disks, `live.rs` logs `live` in on a removable disk at every boot, and `privilege::live_may_install` lets `live` run `edel install` with no password while `from_a_stick()` is true. So a machine installed onto an external SSD gives anyone at its keyboard a session that can wipe its internal disk.
3. **A shell-ui that keeps crashing can lock a person out.** A lock outlives its client on purpose (`compositor/src/lock.rs`), but after 5 failures in 10 s the compositor stops starting shell-ui (`shellui.rs`), so the lock stays with nothing to unlock it until a restart. One such crash: with no font at all, cosmic-text panics with `no default font found` (shown on 2026-10-09).
4. **A look change restarts shell-ui.** `compositor/src/state.rs` restarts it when the preset, colour scheme, accent, colours, text, icons or language change; the restart loses the notifications (`persistence` is advertised), the tray and open popups.
5. **No touch input at all.** The compositor's seat has a keyboard and a pointer only and handles no touch events; shell-ui has no `wl_touch`. Planned in M9.4; a laptop's touchscreen is dead until then.
6. **Drawing written twice.** `rounded` and `shadow` exist in both `compositor/src/glance_look.rs` and `shell-ui/src/paint.rs`, the corners with different curves, and again in Settings' cairo code.
7. **The docs drift.** Examples fixed with this review: Super+T named as the tiling switch, Settings pages called "not built yet" that exist, a key name (`layout.tray_in_panel`) that is `panels.tray` now, the roadmap's "Where we are" still at Phase 0, the laptop image named in M3's goal.
8. **CI is slow and often red.** A green run takes 28 to 34 minutes, some an hour (14 on 2026-10-04); of the last 100 pull request runs 51 passed, 13 failed and 34 were cancelled, most failures in the desktop tests' one 7,200-line script, which has 205 fixed waits.

## Found, not checked by the session

Each needs its own check before a fix (M6.17's second line).

- `edel report` run through doas reads people's `session.log` with `read_to_string`, following a link a person planted (to `/etc/shadow`, say).
- `doas edel settings check FILE` reads any file as root; a parse error may quote a line of it.
- A release list without `expires` never expires; `version.rs` may order `1-rc1` after `1`.
- One `grub.cfg` serves both slots and changes only after a slot is confirmed, so a release that changes the kernel's modules or command line could fail its first boot.
- A hang before the guard starts (no root found, a frozen kernel with no hardware watchdog) has no watchdog.
- "Healthy" means the greeter drew a frame; an update that breaks the session after login is kept.
- Shadows: a maximized window's shadow is a bitmap of its whole size, repainted on every size change; a notification icon given as a path is cached without a limit; the panel reads its status by running `nmcli`, `wpctl` and `upower` every minute even with the netlink watch on.

## What is solid

- The update's write order (`update.rs`): the signature is checked over the bytes then parsed, the target slot is switched off and synced first, the image is hashed while it streams and again from disk, `e2fsck -fn` must pass, and only then do `OK` and `ORDER` change.
- Release lists: channel, expiry, plain http refused, downgrades refused, two keys.
- `edel::places`, the tokens and the seams: outside `places.rs` two hard-coded paths were found; the compositor and shell-ui hold no colour literal.
- Almost no `unwrap` or `expect` on the long-running processes' paths; lenient readers; redraws only on change.
- The tests that matter for an operating system: real boots, A/B fallback, a fresh install, signed updates and budgets, on every change.

## Recommendations, and what each does to complexity

| Recommendation | Complexity | When |
|---|---|---|
| The three dangers (1 to 3 above) | A little more: tens of lines each | M6.17, first |
| Check and fix the unchecked findings that hold | A little more | M6.17 |
| `edel::base`: Alpine's tools in one module, plain functions, and a check that no other module names them | Neutral: code moves | M6.17 |
| Split the library: the settings format, the desktop's models, the OS's own work | Neutral: code moves | M6.17 |
| The desktop's run-time ties: the power menu works without `edel`, `ci/seams.sh` also checks the programs a part runs | A little more | M6.17 |
| One drawing module for the compositor and shell-ui (corners, shadows, colour mixing) | Less: three copies become one | M6.17 |
| shell-ui follows a look change without restarting | Less in the end: one path for every change | M6.17 |
| The winit backend behind a development feature | Less in the product | M6.17 |
| The roadmap's finished steps moved out of it, the CLAUDE.md files cut to rules | Less: deletes text | M6.17 |
| The desktop tests split into parallel parts, a failed case run once more and marked flaky | Neutral in code, about a third of the wall time | M6.17 |
| A shared surface type in shell-ui | More at first | Only if it pays for itself; not planned |

## Waiting for Alimardon

- Whether to cut the path to the first release: the reviewers propose moving the "devices as one" steps (M7.10a to M7.10m), M8.19, M8.20 and M8.22 after it, and freezing new lock screen and theme features until M6 and M8 are done. Not planned; their call.
- Branch protection for `main` with the four checks required (M2.5), and the release key for the first preview (M3.4).

## Principles check

Reliable drove the order: the three dangers come first in M6.17 and before any preview. Simple and Efficient chose the rest, each line removing or moving code rather than adding a layer; the one that adds a layer (a surface type) is left out. Traded off: M6.17 delays M7 by about a week of work, and moving code between modules touches many files at once.
