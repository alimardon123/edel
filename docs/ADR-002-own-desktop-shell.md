# ADR-002: Build our own lightweight desktop shell

**Status:** Proposed
**Date:** 2026-10-01, amended 2026-10-02 (sharing presets, ADR-008; power-profiles-daemon dropped, roadmap M7.8)
**Deciders:** Alimardon
**Supersedes:** the shell decision in ADR-001 (COSMIC vs Plasma spike)

## Context

Alimardon tried COSMIC and Plasma and rejected both: COSMIC feels buggy and is split into many hard-to-handle parts, Plasma feels heavy. The goal is a shell that is simple in architecture, because a simple architecture means fewer bugs and easier maintenance, while still being beautiful and capable.

Requirements carried over from the first brief:

- Traditional, Cinnamon-like default layout.
- One-button switch to other layouts (macOS-like, Windows-like, others), plus settings to fine-tune.
- Apps adapt to phone, tablet and laptop sizes, with a matching app switcher for each.
- One switch between floating windows and dynamic tiling.
- Lightweight enough for low-memory machines and, later, phones.

The key insight: a "desktop environment" is mostly things you should not write (audio, networking, Bluetooth, file manager, text editor, browser). The part that makes it feel like *your* DE is the **shell**: window management, layouts, panel, dock, launcher, switcher and settings. Build that, reuse the rest.

## Decision

Build our own shell as **two long-running processes plus one settings app**:

```
+----------------------------------------------------------+
|  Apps (GTK4/libadwaita by default, any Wayland or X11)    |
+----------------------------------------------------------+
|  shell-ui  (one process, layer-shell client)              |
|  panel - dock - launcher - switcher - notifications -    |
|  quick settings - on-screen display - lock screen         |
+----------------------------------------------------------+
|  compositor  (Rust, built on the smithay library)         |
|  - window policy: Floating | Dynamic tiling (swappable)   |
|  - form factor: Phone | Tablet | Desktop                   |
|  - preset loader (reads one layout file)                  |
|  - input, outputs, scaling, animations, XWayland          |
+----------------------------------------------------------+
|  Reused system services: PipeWire, NetworkManager,       |
|  BlueZ, UPower, greetd, portals    |
+----------------------------------------------------------+
```

Why two processes and not one: if the panel crashes, the compositor and all open windows survive and the panel just restarts. If everything lives in one process (as GNOME Shell does), a panel bug takes down the whole session. Two processes is the smallest split that keeps that safety.

### 1. Layouts are data, not code

A layout preset is one small, readable config file that describes:

- panels (which edge, size, which widgets),
- dock (on or off, position, auto-hide),
- launcher style (menu, full-screen grid, or search bar),
- window button side and style,
- workspace model and hot corners,
- default window policy (floating or tiling).

Built-in presets: **Classic** (Cinnamon-like, the default), **Mac-like**, **Windows-like**, **Tiling**, **Tablet**, **Phone**. The one-button switch just loads a different file. The settings app edits the same file, so there is one source of truth and nothing to get out of sync. Users share a layout as the `[shell]`, `[appearance]` and `[shortcuts]` tables of an exported system file; presets themselves ship in the slot (ADR-008).

### 2. Floating and tiling are two implementations of one interface

The compositor has a single "window policy" interface (place a window, resize, move, close, re-layout). Floating and dynamic tiling are two implementations of it. The switch swaps the policy and re-lays out open windows. It can be set per workspace, so one workspace can tile while another floats.

### 3. Form factors drive the window policy

The compositor decides the current form factor from screen size and density, the tablet-mode switch reported by the input stack, whether a keyboard is attached, and later the fold sensor on foldables.

| Form factor | Window behavior | Switcher |
|-------------|-----------------|----------|
| Phone | One app full screen, gesture navigation | Card stack |
| Tablet | Full screen or side-by-side tiles | Grid of cards |
| Desktop | Floating or tiling, per the user's switch | Alt-Tab list or overview |

Apps that adapt (GTK4/libadwaita, Qt Kirigami) reflow on their own when the compositor gives them a new size. Apps that don't adapt get a scaled "desktop window" fallback on small screens instead of being cut off.

### 4. Simplicity rules (these keep the bug count down)

1. Two long-running processes. Every new daemon needs a written reason.
2. Configuration is plain files. No hidden databases.
3. Communication is Wayland protocols plus one small, versioned settings interface.
4. **No extension API in version 1.** GNOME extensions break on nearly every release; presets and settings cover most of what people use extensions for.
5. Set a memory and startup budget before writing code, using Sway or niri on the same hardware as the baseline, and check it in CI on every change.
6. Keep a written "not building" list (below) and defend it.

### 5. Instant and smooth on any hardware

Alimardon's reference for smoothness is Hyprland, which looks great but leans on live blur and heavy animations that cost a lot on older GPUs. The goal here is the same feel at full frame rate on a laptop from about 2016 with integrated graphics, not only on new hardware. How:

- **Only redraw what changed** (damage tracking). An idle desktop draws nothing.
- **Direct scanout.** A full-screen game or video goes straight to the display without being composited, which saves GPU time and latency.
- **Render as late as possible before each screen refresh,** so input-to-screen delay stays low.
- **Nothing slow on the compositor's main loop.** No disk, network or service calls there; the panel and launcher live in shell-ui, so they can't stall a frame.
- **Animations are short, interruptible and tied to the display clock.** A gesture can reverse an animation mid-way. Reduce-motion is respected.
- **Automatic effect tiers.** The compositor picks a tier from the GPU and battery state, and drops a tier on its own if frames start missing their deadline:

| Tier | Effects |
|------|---------|
| Full | Live blur, shadows, rounded corners, full animations |
| Balanced | Blur of the wallpaper only (computed once, not every frame), shadows, rounded corners |
| Lite | Flat surfaces, short fades |

  The look stays consistent across tiers because all three share the same colors, shapes and spacing; only the expensive effects change.
- **Games:** variable refresh rate and optional tearing for full-screen games that ask for it.
- **Measured in CI** on the old reference laptop: frame times while opening, moving and closing windows, and input latency. A change that makes frames miss the refresh deadline does not merge.

### 6. Title bars and the X button, in tiling too

Wayland apps either draw their own title bar (GTK and libadwaita apps do, with their own close button) or ask the compositor to draw one. Hyprland doesn't draw compositor title bars by default, which is why the X button is missing there.

Here, the compositor draws a slim title bar with close, minimize and maximize for every app that asks for one, **in both floating and tiling mode**. Apps that draw their own keep theirs. Keyboard users can hide title bars in tiling mode with one setting, and close always works from the keyboard too.

### Not building (reuse instead)

Audio (PipeWire), networking (NetworkManager), Bluetooth (BlueZ), login screen (greetd plus a small greeter), file manager, terminal, text editor, browser, office suite, store front (start with an existing Flatpak front end).

**shell-ui toolkit decision (2026-10-03):** shell-ui draws its own surfaces, with no GUI toolkit: layer-shell surfaces through smithay-client-toolkit, drawing with tiny-skia into shared-memory buffers, text through a shaping library, and AccessKit for screen readers (AT-SPI on Linux). Alimardon chose this (option B of three: GTK 4 with a raised memory budget, our own drawing, or Slint) on 2026-10-03, after a minimal GTK 4 window measured 54 to 182 MiB proportional memory against about 30 MiB left in the desktop budget. Settings and our apps keep GTK 4 and libadwaita (ADR-004). Alimardon also asked that shell-ui learn from shells that look beautiful and feel instant, such as Hyprland: [REVIEW-shells.md](REVIEW-shells.md) records what we take and what we skip. Asked by Alimardon.

**Separable desktop decision (2026-10-04):** the compositor and shell-ui stay a desktop that could ship on its own for other distributions, should Edel OS ever want that. They reach Edel OS only through standard Wayland protocols, plain files whose places one module names (`edel::places`, falling back to the XDG base directories where Edel OS's own are absent), and the `edel` library's readers of those files; no code in them assumes Alpine, musl, OpenRC, greetd or Edel OS's disk layout, and the session's start (greetd's greeter, the health file, rollback) stays outside them. Places are found once at start, so this costs no frame and no memory that matters; on Edel OS every path stays as it is. Shipping the desktop for other distributions is a later roadmap step (M5.15b) that starts only on Alimardon's word. Principles: Simple and Versatile (files and standard protocols as the only boundary) with nothing taken from Instant or Efficient. Asked by Alimardon.

## Options Considered

### Option A: Own shell on smithay (Rust), recommended

| Dimension | Assessment |
|-----------|------------|
| Complexity | High, but bounded by the two-process scope above |
| Fit to wishes | Full control of looks, presets, tiling, form factors |
| Maintainability | Good: one language, small surface, memory-safe |
| References | niri and cosmic-comp are both built on smithay, so there is working code to learn from |

### Option B: Own shell on wlroots (C)

Mature and used by Sway, but C means more memory-safety bugs to chase. Choose only if smithay blocks something.

### Option C: Fork niri or cosmic-comp

Saves early work, but you inherit someone else's design and their bugs, which is the thing you want to avoid. Better as reading material than as a base.

### Option D: Keep using COSMIC or Plasma

Rejected by Alimardon after trying both.

## Shell UI toolkit

The shell-ui process needs a toolkit for panels and menus. Because it is a separate process, this choice can be changed later without touching the compositor.

| Toolkit | For | Against |
|---------|-----|---------|
| GTK4 via gtk4-rs plus gtk4-layer-shell | Mature, accessible (screen readers), input methods, CSS theming | Heavier than the Rust-native options |
| iced | Rust-native, light | Same toolkit as COSMIC; accessibility still limited as far as I know |
| Slint | Very light, declarative, made for embedded too | I have not verified its layer-shell support |

Lean: GTK4 for version 1, because accessibility and input methods matter for "every user on the planet" and come for free. Revisit after measuring.

**Decided 2026-10-03, after measuring:** shell-ui draws itself (see Decision, "shell-ui toolkit decision"). A minimal GTK 4 window took 54 to 182 MiB proportional memory under our compositor with llvmpipe, against about 30 MiB left in the desktop budget, so GTK 4 stays for Settings and our apps only.

## Trade-off Analysis

- **Control vs effort.** This is the largest single piece of work in the whole project. The two-process scope and the "not building" list are what keep it finishable.
- **Light vs complete.** The hidden costs are screen sharing, multi-monitor, fractional scaling, input methods for non-Latin languages, accessibility and XWayland for older apps and games. They are not optional for a consumer OS, so they belong in the plan from the start.
- **No extensions vs flexibility.** Users lose some customization; you gain releases that don't break.

## Consequences

- **Easier:** the look, the presets and the tiling behave exactly as you want; fewer moving parts to debug.
- **Harder:** you own window-management bugs that GNOME or KDE would have fixed for you; screen sharing and accessibility take real work.
- **Revisit:** toolkit choice after the first memory measurements; whether to add a limited extension API after version 1.

## Action Items

1. [ ] Measure Sway and niri on the reference laptop to set the memory and startup budget.
2. [ ] Compositor spike on smithay: floating policy, one output, XWayland.
3. [ ] Add the dynamic tiling policy and the switch between them.
4. [ ] Define the preset file format; ship Classic first.
5. [ ] shell-ui spike: panel and launcher, drawn by shell-ui itself on the layer shell (amended 2026-10-03: was gtk4-layer-shell; roadmap M5.1b).
6. [ ] Plan screen sharing (portal backend) and input methods before version 1.
