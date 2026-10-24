# Review: everything walked as a newcomer

**Date:** 2026-10-17
**Reviewed:** the paths a person takes, from the stick to an update gone wrong, in CI (M8.15a) and later on the reference laptop (M8.15b)

**Verdict: every path CI can walk is walked on every change, timed by the machine and counted in steps, and fails when it gets slower or longer.**

## Why

On 2026-10-06 Alimardon asked that before the first public release every path a person takes is walked end to end and polished until it is easy, clear and quick (M8.15). The reviewed release plan of 2026-10-16 split the step: the walks CI can take are built now (15a), and the walks on the HP 250 G8, which need Alimardon, come with the release (15b).

## How a walk is measured

A walk has up to two measures, each with its budget in [the budgets](https://github.com/alimardon123/edel/blob/main/ci/budgets.toml), `walk_NAME_seconds` and `walk_NAME_steps`; a change that makes a walk slower or longer than its budget does not merge.

- **Seconds** are what the machine makes a person wait, timed by the machine itself: a boot's seconds from the kernel running `/init` to the `Started in` line it prints (`walk_boot_seconds`; the kernel's part before it is Alpine's, the same for every change, and what a slow runner stretches most), or the guest's own clock around a command and across a restart. The test's polling (screenshots, copies of the data partition, sleeps while a page settles) never counts, as it would measure CI and not Edel OS.
- **Steps** are what a person does: each key, typed word or click, counted by `press` in `ci/budgets.sh`. The test's own detours (opening Firefox on the stick, a screenshot) are not steps.
- **Frames missed** are not a walk's measure in CI: the stick and the fresh disk draw with llvmpipe on a shared runner, so their frames measure the runner. Smoothness is gated where CI measures it honestly: the frames between presentations (`frame_p99_ms`), the animations (`animation_p99_ms`) and a game's slowest frames (M7.7e); the walks' frames are watched on the laptop (M8.15b).

A walk that must grow (a new page in the Welcome, a new question in the installer) changes its budget in a reviewed pull request that says why, and that one step is worth it.

## The walks

| Walk | What a person does | CI case | Measures |
|---|---|---|---|
| `try` | Starts the stick; the live desktop shows | `desktop-test.sh live` | seconds: the kernel running `/init` to the session's first frame |
| `install-page` | Picks Install Edel OS on the Welcome, gives a name and a password twice, confirms the plan, erases and installs | `desktop-test.sh live` | steps |
| `install` | Installs with `edel install DISK --settings FILE --yes` | `install-test.sh` | seconds of `edel install`, steps |
| `first-setup` | On a new computer: language, keyboard, the first account, time zone, layout, style, Start using Edel OS; the machine restarts into the greeter | `desktop-test.sh fresh` | steps; seconds of the restart into the greeter, from `/init` |
| `update` | Runs `edel update`, then restarts into the new system | `ab-test.sh`, steps 1 to 2 | seconds from the command to the new slot confirmed, steps |
| `rollback` | Runs `edel rollback`, then restarts into the old system | `ab-test.sh`, steps 2d to 3 | seconds, steps |
| `broken-update` | Restarts into an update that cannot start; Edel OS comes back to the old system by itself | `ab-test.sh`, steps 3 to 4 | seconds from the restart to the old slot confirmed, steps |

`install-page` has no seconds of its own: it runs the same `edel install` the `install` walk times.

## Left for the laptop (M8.15b)

- **A setting changed three ways** (Settings, `edel settings`, the settings file): CI proves each way works in its own cases (`settings`, `tiling` and the others in [CASES.md](https://github.com/alimardon123/edel/blob/main/ci/CASES.md)), and the desktop follows within a frame or two; what a person feels there is below what a CI clock in a VM measures honestly, so it is walked on the laptop.
- **Every error message a person can meet**, read for whether it says what to do ([MESSAGES.md](MESSAGES.md)).
- **The download and writing the stick**, which happen on another computer.

## First numbers

Measured on 2026-10-18 by the step's pull request, in CI's VMs with KVM; each budget is the number plus 20 percent, rounded up to a whole second, and the steps exactly.

| Walk | Seconds | Steps |
|---|---|---|
| `try` | 11.95 | |
| `install-page` | | 9 |
| `install` | 12 | 1 |
| `first-setup` | 3.88 | 12 |
| `update` | 14 | 2 |
| `rollback` | 12 | 2 |
| `broken-update` | 51 | 1 |

The broken update is the slowest walk: 30 of its 51 seconds are the test image's wait for the new system to prove it is healthy before falling back (`--health-timeout 30`), and two starts make the rest. On 2026-10-24 a fresh lane's walk measured 5.01 s against 5 while its kernel reached `/init` later than usual, so the boot walks are timed from `/init` since then: their budgets lost the kernel's usual part (1.15 s on a disk, the median of the installed lane's 1.11, 1.15 and 1.22 s that day; 1.36 s on the stick, of 1.21, 1.36 and 1.38 s), `first-setup` 5 to 3.85 and `try` 15 to 13.64, so they allow no more than before on a usual runner. A game stand-in's frames measured p99 12.9 ms on one runner and 14.8 to 14.9 ms on another (M7.7e).

## Principles check

- **Reliable:** the update, rollback and broken update walks run on every change that can affect them, so a slower recovery is caught before it ships.
- **Instant:** each walk's wait is a budget, timed by the machine rather than by the test.
- **Simple and modular:** one sourced file owns reading the budgets and checking a walk; each walk is a few lines in the test that already takes the path, with no new VM boot.
- **Efficient:** no CI time is added; the walks are measured on boots the tests already make.
- **Traded off:** steps count a typed word as one step, and the setting changed three ways waits for the laptop.
