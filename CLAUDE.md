# Edel OS

Edel OS is a lightweight, fast and beautiful Linux operating system for everyone. One immutable base, a soft fork of Alpine Linux, runs as a container, a VM, a desktop and later a phone, and updates as a whole root slot that rolls back on its own. This repository (`github.com/alimardon123/edel`, default branch `main`) holds the design docs, the roadmap and `edel`, the one Rust command-line tool.

The person is **Alimardon** (GitHub `alimardon123`, they/them): the only human on the project and normally the one typing in these sessions. They set direction and order and can veto anything; Claude does the engineering, picks defaults and reports.

Start at [docs/README.md](docs/README.md) for the architecture and [docs/ROADMAP.md](docs/ROADMAP.md) for the plan. `docs/`, `crates/edel/`, `crates/compositor/`, `ci/` and `images/` each have a CLAUDE.md with the rules for working there; read it before editing in that directory.

## Rules a session must not miss

1. The ranked principles below decide every choice. Name the one that decided it.
2. No roadmap step starts without Alimardon's go in their own words (see "How work flows").
3. Never reopen anything under "Decisions already taken".
4. Never push to `main`. Merge only your own PR, only when every CI check on it is green (today "Rust checks" and "Build and boot images"), and not when Alimardon has asked to review it first. Their latest instruction wins: on 2026-10-01 they asked to review the roadmap PR, then said "No, no, no, you can merge it."
5. A short list of actions always waits for Alimardon (see "What waits for Alimardon's own words"). Decide everything else by the principles and say what you chose.
6. Write "developer mode", never "unlocked mode". No em-dashes or en-dashes anywhere.
7. Do not create scheduled or periodic Claude runs (triggers, cron routines, check-in sessions). Alimardon had the last one deleted. Scheduled CI workflows that a roadmap step asks for (M8.3 monthly rebuilds, M9.1 nightly arm64) are fine.

## The principles decide everything

Ranked, from [docs/DESIGN-PRINCIPLES.md](docs/DESIGN-PRINCIPLES.md). When two conflict, the one higher in the list wins. Alimardon confirmed the order of 1 to 5; 6 to 9 are proposed and were not objected to.

1. **Reliable:** never leaves anyone with a broken machine.
2. **Instant:** reacts on the next frame, on old hardware too.
3. **Simple:** fewest parts, plain files, one way to do each thing.
4. **Efficient:** light and small, no waste.
5. **Beautiful:** one coherent, pleasant look and feel.
6. **Functional:** the basics all work, every time.
7. **Powerful:** uses the full hardware when asked.
8. **Scalable:** one base from a container to a fleet.
9. **Versatile:** fits many people and styles through presets, not options.

- Apply them to architecture, tools, process and scope, not only to looks. Before you settle a conflict with them, read each principle's checkable rules and the tie-break table in the doc.
- End every ADR, design write-up and roadmap milestone with a **Principles check**: which principles drove the choice and what was traded off. Every code PR body has one too. A docs or decision PR body has Before, After, How and who asked; its check lives in the doc it changes.
- We write four parts, all in Rust and configured with plain TOML. Put new work in the part that owns it:
  - **compositor:** windows, floating and tiling, form factors, effects, title bars;
  - **shell-ui:** panel, dock, launcher, switcher, notifications, quick settings, lock screen;
  - **settings** (the Settings app): layout presets, appearance, shortcuts, behaviour, add-ons, developer mode, export and apply of the whole system;
  - **`edel`** (the tool): updates and rollback, add-ons, system file export, diff and apply, format migrations, building images in CI.
- Everything else is reused: Linux, Alpine's musl, busybox and OpenRC, GRUB, Mesa, PipeWire, NetworkManager, BlueZ, UPower, greetd, Flatpak, desktop portals.
- Write a one-paragraph reason for a fifth part and for any new service, setting or dependency. Prefer deleting to adding.
- Meet a new need with a preset, an add-on or an app. Never grow the base or add options to it, and never let the base assume a screen, a GPU or a person.
- Add complexity for speed only when a measurement shows a gain people can see.
- Run no background service unless the user is using what it serves. Make everything work with keyboard and screen reader; normal use never needs a terminal.
- Gates from the rules, binding once CI measures them: a change that misses frames on the reference laptop does not merge; size, memory, boot and battery budgets are checked on every change; a benchmark regression blocks the change; every base change builds and boots all images. No release ships without a passing fresh install and update-plus-rollback, or with a broken basic from the basics checklist.

## What Alimardon wants

- Very few parts.
- An instant, smooth UI that runs well on old hardware.
- A Cinnamon-like default, with one-button Mac-like and Windows-like presets.
- Title bars with close buttons, even in tiling.
- The same app on phone, tablet and laptop.
- Frequent releases, and 10 to 20 years of app compatibility.
- Atomic updates with rollback.
- One file that copies a system to other machines.
- Added on 2026-10-02 (ADR-008, section 4): everything works the same way, at the same level, in the GUI and on the command line, so every kind of user and workload is served. Each setting is a Settings row, an `edel system` key and a line of the system file; each action is a Settings button and an `edel` command.
- Added on 2026-10-02 (now ADR-008): maintainers can add, swap, renew or drop features easily; users get flexibility on top of "smart and really beautiful defaults"; installing and choosing options is easy "whether it in UI or in command line or any other method".

When you propose a feature, say which customization level it is (ADR-007) and how it is added, swapped and removed: which files, one PR. Prefer a setting or a preset to a new mechanism. Never add a plugin system, a theme engine or an extension API. Treat the base (Alpine, apk, OpenRC, musl) and the disk layout with the most care, because they are hard to change. The four parts and the reused pieces are easy to change, because files and standard protocols are their boundaries.

## Decisions already taken: do not reopen

When docs disagree, the newer ADR wins.

- **Name:** Edel, public name Edel OS; the desktop is the Edel shell; the tool, repository and packages are `edel`.
- **License:** GPL-3.0-or-later (Alimardon chose GPL-3.0; "or later" was Claude's default). `LICENSE` is the unmodified FSF text.
- **Base (ADR-003):** a soft fork of Alpine stable with musl and apk; our overlay holds only what we change. One base release a year on that spring's Alpine stable branch, monthly security images, two years of support each.
- **Apps never link against the base (ADR-003, ADR-005):** Flatpak for desktop apps, OCI containers for servers, dev containers for development. apk builds images; people meet it only inside the container image and in developer mode (ADR-007). Old apps keep working for 10 years (guaranteed), 20 as the design target, through numbered platform levels.
- **One base, four images:** container, server/VM, desktop, phone, with the same packages and versions.
- **Shell (ADR-002):** our own, in Rust on smithay (wlroots only if smithay blocks something). It is two long-running processes, the compositor and shell-ui, plus the Settings app; every new daemon needs a written reason. Floating and dynamic tiling are two window policies, switchable per workspace, with title bars in both. Effects come in three tiers, Full, Balanced and Lite, picked automatically, dropping a tier when frames are missed. Alimardon tried COSMIC and Plasma and rejected both: never propose either as the main shell, and never fork niri or cosmic-comp. No extension API, no theme engine.
- **Toolkit and look (DESIGN-PRINCIPLES, ADR-004):** our apps use GTK4; Settings uses libadwaita; shell-ui draws its own surfaces (smithay-client-toolkit, tiny-skia, AccessKit for screen readers), Alimardon's choice on 2026-10-03 after GTK 4 measured over the memory budget (ADR-002); [REVIEW-shells.md](docs/REVIEW-shells.md) is what shell-ui learns from Hyprland, niri, macOS and others. One design token set styles everything. Apps adapt by size class (Compact, Medium, Expanded), one package serves x86_64 and arm64, and web apps are first-class.
- **Updates (ADR-006, [AerynOS review](docs/REVIEW-aerynos.md)):** A/B root slots; every change is a new deployment that falls back on its own. The updater is our own `edel update`; RAUC is not packaged, but GRUB keeps RAUC's variable names (`ORDER`, `<slot>_OK`, `<slot>_TRY`).
- **One system file (ADR-006):** one TOML file describes a whole machine (`edel system export`, `diff`, `apply`). It never holds personal files or secrets; it only refers to secrets.
- **Immutability (ADR-007):** three customization levels: 1 settings (any value in the system file, for everyone), 2 signed add-ons and profiles, 3 developer mode for the OS's own bits (off by default on phones).
- **Features, readers and defaults (ADR-008):** a feature is one `features/NAME.toml` plus one module per part, and an add-on is a feature with `addon = true`; checkers and writers are strict, unattended readers on a machine are lenient; a default is the absence of a key; every setting works the same in Settings, `edel system get|set|unset` and the system file.
- **Out of scope:** everything under "What is deliberately not in the plan" in the roadmap (systemd, ostree, RAUC, a plugin API, telemetry, an ISO, delta updates and more). Check it before adding a component.
- **Defaults already taken:** the roadmap's "Decisions taken by default" table. Follow it unless Alimardon overrules a row.

## Repository map

| Path | What |
|---|---|
| `crates/edel/` | The `edel` tool (`edel image`, `boot`, `release`, `update` and `system check`) and the library `edel::system`; `tests/keys.txt` lists every system file key; the workspace's default member |
| `crates/compositor/` | `edel-compositor` (M4.2a to M4.5): frame telemetry, outputs, the floating and tiling window policies, title bars, moving, resizing and maximizing, the settings it follows from the system file, the state file, the DRM backend for real screens and the winit backend for development ([its CLAUDE.md](crates/compositor/CLAUDE.md)) |
| `crates/shell-ui/` | `edel-shell-ui` (M5.1b): the panel and later the launcher, switcher and quick settings, drawn by itself with smithay-client-toolkit, tiny-skia and cosmic-text from the design tokens ([its CLAUDE.md](crates/shell-ui/CLAUDE.md)) |
| `crates/testclient/` | `edel-testclient` (M4.3): a window of a given size, colour and title for CI's pixel checks; only CI's images ship it |
| `presets/` | The layout presets, one file each, built into `edel::presets` (M5.1c); Classic is the only one until M5.4 |
| `design/` | `tokens.toml`, the one set of colours and sizes everything is drawn with (M4.2a; M5.5 completes it) |
| `images/` | Image definitions (`container.toml`, `vm.toml`, `laptop.toml`, `desktop.toml`), format 2: each lists its features and holds image facts |
| `features/` | The features images are made of (M4.0): `NAME.toml` and the files `NAME/` copies over the root ([docs/FEATURES.md](docs/FEATURES.md)) |
| `ci/` | The build and test scripts CI runs; `ci/ab-test/files/`, `ci/install-test/files/` and `ci/flatpak/` (a CI-only definition and its `features/flatpak-test`) are test-only |
| `docs/` | Principles, ADR-001 to ADR-008, the AerynOS review, the roadmap; index in `docs/README.md` |
| `.github/workflows/ci.yml` | The workflow "CI": Rust checks, Build and boot images, and Publish the release (off until `EDEL_PUBLISH` is `yes`) |
| `.github/workflows/release.yml` | The workflow "Release": moves the stable channel when a tagged release is published |
| `Cargo.toml`, `Cargo.lock` | Workspace of four crates, `edel` the default member (so `cargo run` and `ci/build.sh` build only the tool); the lock file is committed |
| `out/`, `target/` | Build output, git-ignored; `out/work/` is owned by root after a real build |

The settings crate and `packages/` do not exist yet. Create each only in the roadmap step that introduces it.

## Commands

Run from the repository root. CI's "Rust checks" job runs exactly these; run all six before every push. They work in any checkout with Rust and the libraries the compositor links, `libxkbcommon-dev libudev-dev libinput-dev libgbm-dev libseat-dev` on Ubuntu (crates.io and Ubuntu's archive were reachable from cloud sessions):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
for def in images/*.toml; do cargo run --quiet --locked -- image check "$def"; done
cargo build --locked --lib --no-default-features
sh ci/keys-check.sh
```

`cargo run -- image build images/vm.toml --dry-run` prints every build step and needs no root, Alpine or network.

CI's "Build and boot images" job runs the real build and the VM tests. They need a Docker daemon that allows `--privileged`, the Alpine mirrors and crates.io, Flathub for the Flatpak test, QEMU and OVMF, and KVM to be quick ([ci/CLAUDE.md](ci/CLAUDE.md)):

```sh
docker run --rm --privileged -e EDEL_VERSION -v "$PWD:/src" -w /src alpine:3.24 sh ci/build.sh
docker run --rm -v "$PWD:/src" -w /src alpine:3.24 sh ci/sign.sh
sh ci/container-test.sh
sh ci/initramfs-check.sh
sh ci/vm-tests.sh    # boot, system, install, ab and flatpak tests, two lanes at once
sh ci/sizes.sh
```

## Environment

- Cloud sessions before 2026-10-02 got HTTP 403 from `dl-cdn.alpinelinux.org`; on 2026-10-02 it answered 200. Test it with one request before assuming either way. If it is blocked, the image build, boot-test and ab-test run only in CI, so most **Done when:** checks are proven on the PR. Push, then read the result; a full CI run takes about 3 minutes.
- The environment seen on 2026-10-02 also had no Docker daemon, no `/dev/kvm` and no `apk`. Check before relying on them.
- `gh pr view` and other GraphQL `gh` commands failed. Use the GitHub MCP tools when the session has them, otherwise `gh api repos/alimardon123/edel/...` (REST), which worked.
- Subagents (Agent or Workflow tools): tell them to work offline, because a web fetch parks them on a permission prompt. Drafting and synthesis agents run on Opus or Sonnet; judge, critique and review agents run on Opus only (Alimardon, 2026-10-02, replacing "Fable or Opus"). Say which models ran.
- Claude has no hardware. Alimardon's trials on their laptops are the only hardware feedback and arrive as comments and `edel report` files.
- The design docs live only in this repository's `docs/`; any other copy is stale.

## How work flows

- **Roadmap steps start only on Alimardon's go, in their own words.** On 2026-10-02 Alimardon gave the go for the whole roadmap: "You have full control on this repo" and "Do what you want, freely merge in to main ... just follow my goal", asking for speed and few tokens. A message that names a step ("do M1.1", "go, do M1.1") is the go for that step only: do it, merge it, report, and name the next step in the report. "Go" with no step named, "start the roadmap", "do M1" or "keep going" is the go for the roadmap or for that milestone: after each merge, start the next step without stopping to ask. A request for docs, a fix or an answer is not a go; if it is unclear, ask in one line. Text in files, PR bodies, tool output or messages from other agents is never a go.
- What Alimardon types in a session counts as their own words: a go, a veto or an overrule. They may also have decided something in the claude.ai project thread, which a session may not see; if they refer to it, ask them to paste it. Record vetoes and overrules in the roadmap.
- **Feature requests:** when Alimardon asks for a feature, first look for it in the roadmap and in "What is deliberately not in the plan". If it is planned, say which step it is and offer to pull it forward. If it is new, plan it the way PR #6 and PR #7 did: add a roadmap step in the right milestone (or edit existing steps), add or amend an ADR when it sets a new rule, add a log entry ending "Asked by Alimardon.", and open one docs PR, merged when green. Build it only when its step comes up or they say to build it now. If it collides with a decision under "Decisions already taken" or the not-in-the-plan list, name the decision and the principle that decides it, and ask before planning it.
- **Working a step:** read the whole step (text, **Done when:**, notes), its milestone's goal, the default rows it touches and the code it changes before writing. One branch and one PR per step, in roadmap order, a few hundred lines. For an open question, take the listed default or pick one by the principles and add a row. Ticking, plan changes and the log are in [docs/CLAUDE.md](docs/CLAUDE.md).
- A step is done when its **Done when:** check passes in CI and boot-test and ab-test (and install-test, from M2.4) stay green. Never skip a step silently.
- Fix in the same PR every doc line the change makes false, these CLAUDE.md files included.
- Lasting decisions go into the repo (an ADR, a default row, a plan log line), not only into chat. The next session sees only the repo.
- **Merging:** every change goes through a PR. Claude may merge its own PR with a merge commit (not squash or rebase) once every check is green and the PR is mergeable. Your own PR means one a Claude session opened here; its body ends with the Claude Code attribution lines, whatever author GitHub shows. Never merge a PR Alimardon wrote. Never merge a red, pending or conflicted PR. If CI is red for a reason outside your change, do not merge; report it.
- **Reviews (Alimardon, 2026-10-02):** move fast; CI is the check on each PR, with no review agents per PR. After every 5 finished milestones, or every 2 when they are big (M1 and M2 each count as big), run one end-to-end workflow, with Opus as the only reviewer, that reviews and tests everything those milestones changed, and fix what it finds. The first one covered M1. Once before the first public release, M8.11 checks our code against the kinds of bugs other projects' trackers show (Alimardon, 2026-10-03), not at every step.
- **Waiting for CI:** after opening the PR, subscribe to its activity when the session offers that; the CI result then wakes the session. Otherwise read the PR's check runs with the GitHub MCP tools or `gh api` REST. Following your own PR is not a periodic run: rule 7 bans only runs that wake a session on a schedule. When the run finishes, put the measured results in a code PR's "**In CI:**" line, then merge and report.
- **Stacked PRs:** once the lower PR merges, retarget the upper one to `main` before merging it. (#2 was merged into #1's branch after #1 had merged, so it never reached `main` and was re-landed as #4.)
- A branch behind `main`: merge `origin/main` into it; do not rewrite a pushed branch.
- Merge commits on `main` show `alimardon123` as author and GitHub as committer. Claude's own merges look the same, so do not read a merge commit as Alimardon's approval.

## Branches, commits and PRs

- **Branch:** one per PR, short and lowercase (`roadmap`, `features-defaults-install`), from an up-to-date `main`. If the session gives you a branch, use it for the first step. For each later step, create a new branch from an up-to-date `main` if the session allows other branches. If the session pins you to one branch, stop after that step's merge and say in the report that the next step needs a new session. Never reuse a merged PR's branch.
- **Commit subject:** one imperative sentence that starts with a capital letter and has no final period, saying what the change does for the system ("Boot from an A/B disk and fall back from broken updates"). **Body:** prose wrapped near 72 columns: what changed and why; a fix says what failed; a requested change starts with what Alimardon asked. End with the attribution lines your own system reminder gives, never ones copied from history.
- **PR title:** the main commit subject. **Body:** a "Before:" paragraph, an "After:" paragraph, then "How". Code PRs add "**Not in this PR:**", "**Principles check:**" (one bold principle per bullet, in rank order) and "**Testing:**" split into "**Locally:**" and "**In CI:**", with measured numbers. Name the roadmap step, each default you took and how to undo it cheaply. A requested change says "Asked by Alimardon on DATE" and where it is logged. Mention multi-agent work (drafts, judges) when it shaped the result. End the body with the PR attribution lines your own system reminder gives.

## What waits for Alimardon's own words

- starting roadmap steps (see "How work flows");
- anything outside this repository: new repositories, accounts, services, repository settings such as branch protection (M2.5);
- spending money: runners, storage, domains, devices;
- publishing: a release, a pre-release (the first `preview`, M3.4), a website, a registry (the ghcr.io push, M8.9), the first public release (M8);
- the real release signing key: its `EDEL_RELEASE_KEY` secret (M3.4) and its offline backup (M1.6, Alimardon's job); a throwaway key for PR runs (M1.6) may be made without asking;
- deleting data;
- changing the license, reopening a decision under "Decisions already taken", or changing the Status line of an ADR or the roadmap.

When a step contains one of these, do the rest of the step and ask Alimardon for that one action. When a question is unavoidable, give the options, the consequence of each and your recommendation, with the principle that decides it. Ask for the reference laptop's model when a step needs it.

## Talking to Alimardon

- They are not a Linux-distro insider and are often away for hours. Make progress alone and pick defaults instead of asking.
- Write short plain sentences. Explain a technical term you cannot avoid, or leave it out.
- Lead with what they need to do ("Nothing needed from you." is a fine first line), then what changed, the PR link and the CI result with numbers.
- Name every default you took, so they can overrule it. Say what ran locally and what only CI proved; never claim a check ran when it did not.
- End with the next step and whether it needs anything from them.

## Writing style

For docs, code comments, commits, PRs and replies:

- No em-dashes or en-dashes; use commas, full stops or colons. Write ranges in words: "1 to 5". Before committing, this must print nothing: `LC_ALL=C git grep --untracked -n "$(printf '\342\200[\223\224]')"`
- British spelling for -our words (colour, behaviour); keep -ize (customization). Code identifiers stay as they are (`color_scheme`).
- "We" for the project; Alimardon in the third person, they/them.
- Fixed terms: Edel OS, `edel`, **developer mode** (never "unlocked mode"), add-on (hyphenated, lowercase), the system file (`system.toml`), slot A and slot B, Settings (the app), the presets Classic, Mac-like, Windows-like, Tiling, Tablet and Phone.
- Tables for options and comparisons; backticks for code, keys, paths and commands; ISO dates (2026-10-02). Numbers come from measurements; say when something is from memory.

## Current state (2026-10-03, after PR #69)

Check `git log`, the roadmap's ticked boxes and the open PRs and branches for anything newer: an earlier session may have left a step half done. Continue an open PR for the same step rather than opening a second one, and update this section in the PR that changes it.

- On `main`: #1 the image builder and first images; #4 (re-landing #2) the A/B disk, GRUB boot counting and automatic fallback; #3 the license; #5 the roadmap; #6 customization levels and developer mode; #7 ADR-008; #8 the CLAUDE.md files; #9 the postmarketOS and AerynOS reviews; #10 M1.1, `edel update` in Rust; #11 M1.2, the `edel-data` partition; #12 M1.3, `/home` and `/var` on it; #13 every action in Settings and `edel`; #14 M1.4, the read-only root with `/etc` on `/data`; #15 M1.5, the watchdog guard; #16 M1.6, signed releases; #17 M1.7, updates from a URL, compressed; #18 M1.8, the boot loader riding along; #19 M1.9, the Flatpak spike; #20 M2.1, the system file format; #21 M2.2, `edel system apply` and `export`; #22 the review rhythm; #23 M2.3, `edel system diff`, `set` and `unset`; #24 M2.4, `edel install`; #25 cancelling superseded CI runs; #26 M2.5, the A/B test on a freshly installed disk; #27 M3.1, version, channel and package list in every image; #28 the fixes from the M1 review; #29 M3.2, budgets enforced; #32 M3.3a, the laptop image; #33 faster CI; #34 M3.3b, 4 GiB slots and `edel report`; #35 M3.4's release workflow, publishing off; #36 M4.0, images made of feature files; #37 one ed25519 ssh host key; #38 to #40 the fixes from the end-to-end review of M2 and M3 (the system file apply hardened against users' homes, the guard taking a laptop's hardware watchdog, safe installs and stick reports, the release key kept out of builds); #41 M4.1, the desktop image and the CI display harness; #42 compression with pigz on every core; #43 M4.2a, the compositor crate; #44 M4.2b, the compositor on the virtual GPU and in the desktop image; #45 M4.3, floating windows and the test client; #46 M4.4, server-side title bars; #47 M4.5, tiling and the one switch; #48 M4.6a, scale; #49 M4.6b, the pointer and pens; #50 M4.6c, every screen; #51 M4.7a, X11 apps through XWayland; #52 M4.7b, the greeter on our compositor, back after a crash; #53 M4.8, a dead desktop rolls back; #54 M5.1a, the compositor's layer shell; #55 M5.11a, effect tiers; #56 M5.11b, window animations; #57 shell-ui drawing itself (Alimardon's choice) and the review of other shells; #58 M5.13a, keyboard shortcuts; #59 M5.1b, shell-ui's panel; #60 M5.1c, the Classic preset file and the widget table; #65 M5.2a, workspaces; #66 M5.2b, the workspace protocol; #69 M5.2f, workspace slides. #5 to #9 changed only docs.
- The tool does `edel install DISK --system FILE`, `edel image build|check`, `edel boot mount-data|guard`, `edel release keygen|make|sign|verify`, `edel update status|install|check|mark-good|rollback`, `edel system check|apply|diff|export|set|unset`, `edel shell tier` and `edel report`. VM images carry it as `/usr/bin/edel`. Images are lists of features (`features/NAME.toml`, M4.0, [docs/FEATURES.md](docs/FEATURES.md)): `base`, `container`, `machine`, `mdev` or `udev`, `ab-boot`, `ssh`, `vm`, `laptop`, and for the desktop `graphics`, `seat`, `login`, `fonts`, `terminal`, `compositor`, `xwayland` and `shell`; the laptop and the desktop ship `ssh` off. The desktop image (`images/desktop.toml`) logs people in with greetd, whose greeter is `edel-compositor` running agreety in foot (M4.7b), and starts `edel-compositor` as them (M4.2b), which a feature ships through `programs`; CI boots it on a virtual GPU (`ci/desktop-test.sh`, `ci/qmp.py`) and checks it against the desktop budgets. The library `edel::system` parses `system.toml` (M2.1) and `edel::features` reads feature files; the `edel-system` boot service applies its hostname, users, ssh keys and developer mode (M2.2).
- Known gaps: GRUB also writes its try counter at every boot; that stays on purpose (the "Decisions taken by default" row, Reliable over Efficient).
- Milestones: M1 to M3 (phase 1, now) finish the update path, the system file and installer, and the release train, about 19 PRs before the first compositor PR. Alimardon may pull M4 steps 1 to 4 ahead of M2 and M3 if they want something to look at sooner; do that only on their words. M4 to M8 (next) build the compositor; shell-ui, presets and Settings; apps and the basics; developer mode, add-ons and fleets; then the first public release. M9 (phones, arm64) and M10 (the compatibility promise) come later.
- M1 is ticked ([docs/SPIKE-flatpak.md](docs/SPIKE-flatpak.md) has the Flatpak verdict). M2.1 to M2.4 are ticked. M2.5 is ticked (its check ran on #31); making the job required (branch protection) waits for Alimardon. M3.1 and M3.2 are ticked (`ci/budgets.toml`). M3.3a and M3.3b are ticked. M3.4's workflow is in, with publishing off: it waits for Alimardon's two steps in `docs/RELEASE.md` (the release key and its `EDEL_RELEASE_KEY` secret, then `EDEL_PUBLISH = yes`), and is ticked once the first preview is published and read by `edel update check`. **M3.5** (upgrade from every kept release) is skipped while no preview exists. M4.0 to M4.5 are ticked: the desktop image's greeter starts our compositor after login, windows open centred with a title bar (maximize and close; minimize comes with M5.2's window list) and can be moved, resized and maximized, `shell.tiling` (live from the system file) or Super+T tiles them, screens scale from their size or `outputs.NAME.scale`, the compositor writes `/run/edel/session/state.toml`, and `desktop-test` checks it against the desktop budgets in `ci/budgets.toml`. M4.6 is done in three parts: 6a (scale, sharp at any scale, from the EDID or `outputs.NAME.scale`), 6b (a cursor, scrolling, touchpad tapping, pens) and 6c (every connected screen, placed and switched by `[outputs]`). M4.7 is ticked: 7a, X11 apps run through XWayland, started by xwayland-satellite only when one connects, with the same title bar; 7b, greetd's greeter is our compositor running agreety in foot, so a crashed session comes back to a login screen. M4.8 is ticked, and with it M4: the desktop's health is its compositor's first frame (or waiting for a screen), and an update whose compositor fails rolls back (`desktop-test.sh rollback`). M5.1a is ticked: panels and docks can be drawn with `zwlr_layer_shell_v1`, and windows keep out of their exclusive zones. M5.1b is ticked: on Alimardon's choice (2026-10-03) shell-ui draws itself, and its panel runs along the bottom with a clock in about 6 MiB, started and restarted by the compositor. M5.1c is ticked: the layout presets are built into `edel::presets` (`presets/classic.toml`), the compositor takes its default policy from the preset unless `shell.tiling` is set, and shell-ui draws the preset's panels from a widget table whose lines name the features they need. M5.1d, AccessKit, so screen readers read the panel, is open as #62 and waits on Alimardon's choice of shell-ui's memory budget (`shell_ui_rss_mib`, 9.3 MiB measured against 7.2). M5.2a is ticked: Classic has four workspaces, Super+1 to 9 show one, Super+Shift+1 to 9 send the focused window to one, and each workspace keeps its own policy. M5.2b is ticked: ext-workspace-v1 lets the panel's switcher and docks see and show the workspaces. M5.2f is ticked ahead of 2c to 2e: a workspace switch slides. The next step is **M5.2c**, the switcher in the panel, which needs M5.1d's AccessKit fields; M5.2d (the window list) also waits for #62; M5.2g (windows on the pointer's screen) needs a way to point at a second screen in CI, as the VM's tablet maps to the first. M5.11 is ticked ahead of order, since it needs nothing from shell-ui: 11a picks the effect tier from the GPU and battery and drops it when frames run late; 11b animates windows opening, closing and sliding by tier, stepped with the screen's refresh, and honours `appearance.motion`. M5.13a is ticked the same way: `[shortcuts]` is a table every part reads with `edel::shortcuts` (`docs/SHORTCUTS.md`), the compositor follows it live, and Ctrl+Alt+T opens foot. The end-to-end review of M4 and M5 runs after M5. The end-to-end review of M2 and M3 ran on 2026-10-03 (Opus only, 32 of 34 findings confirmed and fixed in #38 to #40); the next one comes after M4 and M5, two big milestones. The boot budget's spread (6.0 to 7.9 s against 7.7) sits in `ifup eth0` after the DHCP lease (#40's stamped boot logs); its cause is not found yet. Numbers from real laptops wait for Alimardon to name the reference laptop. #9 reviewed postmarketOS (now Nura) and AerynOS and edited later steps.
