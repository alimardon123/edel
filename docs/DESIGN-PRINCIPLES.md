# Design principles

**Status:** Order of principles 1 to 5 confirmed by Alimardon on 2026-10-01; principles 6 to 9 (the split of "Capable") proposed.
**Date:** 2026-10-01

These are Alimardon's general principles, and they are the rules for **every decision** in the project, not only visual design: architecture, technology and tool choices, process, and what to build or cut, from the kernel configuration to the corner radius of a button. Each principle comes with a rule that can be checked, so it isn't just a nice word.

Every ADR and design write-up ends with a short **Principles check** naming which principles drove the choice and what was traded off.

When two principles conflict, **the one higher in the list wins.**

| # | Principle | In one line |
|---|-----------|-------------|
| 1 | Reliable | Never leaves anyone with a broken machine |
| 2 | Instant | Reacts on the next frame, on old hardware too |
| 3 | Simple | Fewest parts, plain files, one way to do each thing |
| 4 | Efficient | Light and small, no waste |
| 5 | Beautiful | One coherent, pleasant look and feel |
| 6 | Functional | The basics all work, every time |
| 7 | Powerful | Uses the full hardware when asked |
| 8 | Scalable | One base from a container to a fleet |
| 9 | Versatile | Fits many people and styles through presets, not options |

## The principles

### 1. Reliable
The system never leaves anyone with a broken machine.
- Every change (update, add-on, configuration) is atomic and rolls back on failure (ADR-006).
- No release ships without CI passing **both a fresh install and an update-plus-rollback** on every image and on the reference hardware. (Lesson from AerynOS, whose fresh installs broke once while updates were fine.)

### 2. Instant
Everything reacts on the next frame, on old hardware as well as new.
- Every frame arrives in time at the display's refresh rate on the old reference laptop (about 2016, integrated graphics). Measured in CI; a change that misses frames does not merge.
- Opening the launcher, switching apps and moving windows have no visible delay.
- Complexity added for speed is allowed **only** when a measurement shows a gain people can actually see.

### 3. Simple
As few parts as possible, plain files, one way to do each thing.
- The whole project is built from the four parts in the map below. A fifth part needs a written reason.
- Every new service, setting or dependency needs a one-paragraph written reason. Deleting beats adding.

### 4. Efficient (light and small)
- Budgets for base image size, idle memory, boot time and idle battery drain, set from measurements on the reference hardware and checked in CI on every change.
- No background service runs unless the user is using what it serves.

### 5. Beautiful and pleasant
- One set of design tokens (colors, spacing, corner radius, type, motion) generates the shell theme, the GTK theme and the icon palette, so everything looks like one system in every preset and every effect tier.
- The same core keyboard shortcuts work in every preset.
- Everything works with keyboard and screen reader; normal use never needs a terminal.

### 6. Functional
The everyday basics work completely before anything new is added.
- A **basics checklist** must pass on every reference device before each release: Wi-Fi, Bluetooth audio, sleep and wake, external displays, printing, camera, battery reporting, input methods for non-Latin languages and accessibility.
- A release with a broken basic does not ship, whatever new features it has.

### 7. Powerful
When the user asks for it, the system uses the hardware to its limit.
- Current kernel and graphics drivers, hardware video decoding in the browser, game mode (variable refresh rate, direct scanout), and full performance on AC power.
- Hot libraries are also built for newer CPUs, alongside the baseline builds, so new machines run faster without dropping old ones.
- A benchmark suite (games, video export, code compiles) runs on the reference hardware; a regression blocks the change.

### 8. Scalable
One base serves everything from a tiny container to a fleet of thousands.
- Every base change builds and boots **all** images (container, server/VM, desktop, phone) in CI.
- Nothing in the base may assume a screen, a GPU or a person in front of it; anything that does belongs in the desktop or phone image.
- The same system file (ADR-006) deploys one machine or ten thousand.

### 9. Versatile
Developers, creative workers, gamers, office users and servers each get a setup that fits them.
- New needs are met with a **preset**, an **add-on** or an **app**, never by growing the base or adding options to it.
- Presets cover layouts (Classic, Mac-like, Windows-like, Tiling, Tablet, Phone) and, later, user profiles (for example a gaming profile that turns on game mode and installs Steam).

## Why this order

- **Reliable first:** a beautiful, fast system that breaks loses people's trust for good.
- **Instant before simple:** speed is what people feel every second. But the "only with a visible measured gain" rule stops speed from becoming an excuse for complexity.
- **Simple before efficient:** simple systems are easy to make efficient later; clever optimizations make systems hard to maintain.
- **Beautiful before the capability group:** a coherent look is what people notice every day, and every extra capability is something more to design, test and keep beautiful.
- **Functional before powerful:** half-working Bluetooth drives people away faster than a missing benchmark win.
- **Powerful before scalable:** gamers, creative workers and developers judge the system on what their own machine can do. Scalability is mostly protected by the architecture itself (one base, several images), so it rarely needs to win a conflict.
- **Scalable before versatile:** the one-base design is hard to repair once broken, while presets are easy to add later.

## Tie-break examples

| Situation | Winner | Result |
|-----------|--------|--------|
| Live blur drops frames on an old laptop | Instant over Beautiful | The compositor drops to the Balanced effect tier; same look, no live blur |
| A new background service would make apps launch faster | Simple vs Instant | Allowed only if the launch is visibly faster in a measurement and it can't live inside an existing part |
| A custom updater would be faster than an established one | Reliable over Instant | Use the established updater |
| A gaming tweak speeds up desktops but drains phone batteries | Efficient over Powerful | It turns on only in game mode or on AC power |
| A new feature is ready but sleep and wake broke on one device | Functional over everything below it | The release waits for the fix |
| A server-only service would make the base bigger for everyone | Efficient and Scalable over Versatile | It goes in the server image only |
| Someone asks for 15 new panel options | Simple over Versatile | Make it a preset instead |
| A newer slot wrote a key the rollback slot does not know | Reliable over Simple | The boot reader keeps it, applies the rest and reports it; only checkers and writers refuse unknown keys (ADR-008) |

## The fewest-parts map

**We write four parts.** All in Rust, all configured with plain TOML files.

| Part | What it does |
|------|--------------|
| **compositor** | Windows, floating and tiling, form factors, effects, title bars |
| **shell-ui** | Panel, dock, launcher, switcher, notifications, quick settings, lock screen |
| **settings** | The settings app, including layout presets, appearance, shortcuts, behaviour, add-ons, developer mode and one-button export/apply of the whole system |
| **edel** (command-line tool) | Updates and rollback, add-ons, system file export/diff/apply, format migrations, and building images in CI |

**One of each, everywhere:**

| Concern | The one choice |
|---------|----------------|
| Language for our code | Rust |
| Configuration format | TOML (presets, system file, shell settings), versioned from version 1 |
| How users get apps | Flatpak, plus installable web apps |
| How servers get apps | Containers |
| Base packages | apk, used only to build images; users never see it |
| How the system changes | New deployment with automatic rollback |
| How the base is extended | Signed add-ons (ADR-007) |
| UI toolkit for our apps | GTK4 |
| Look | One design token set |

**Everything else is reused, not written:** Linux kernel, Alpine's musl and OpenRC base, Mesa graphics drivers, PipeWire (audio), NetworkManager, BlueZ (Bluetooth), UPower (battery), greetd (login), Flatpak, desktop portals, and an established A/B updater if the spike in ADR-006 works out.
