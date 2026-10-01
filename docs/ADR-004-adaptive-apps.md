# ADR-004: One app on every device (adaptive apps)

**Status:** Proposed
**Date:** 2026-10-01
**Deciders:** Alimardon
**Extends:** ADR-002 (which covers how *windows* adapt; this covers how *apps* adapt)

## Context

The same app should run on a phone, a tablet, a foldable and a laptop, with a layout that fits each screen, so nothing looks crowded or awkward. ADR-002 handles the compositor side (form factors, window policies, switchers). That is only half of it: the compositor can resize a window, but only the app can rearrange its own buttons, sidebars and menus.

Three things are needed:

1. **The same app package** must run on both phone and laptop chips.
2. **The app** must change its layout by size and input type.
3. **The system** must tell the app what it needs to know (size, touch or mouse, posture) and keep the app running while that changes.

## Decision

### 1. One package, both chip types

Apps ship for both x86_64 (most laptops today) and arm64 (phones, tablets and the newer ARM laptops). Flathub already builds both. The user installs "the app", not "the phone version". Settings and data are the same on every device.

### 2. Apps adapt by size class

We publish a short layout guide with three size classes:

| Size class | Typical device | Typical layout |
|------------|----------------|----------------|
| Compact | Phone, folded foldable | One pane, bottom navigation, full-screen pages |
| Medium | Tablet, unfolded foldable, small window | Two panes or a collapsible sidebar |
| Expanded | Laptop, desktop, docked phone | Sidebar, content and details side by side, toolbars |

The exact width cutoffs are tuned during development; Android's window size classes use the same three-step idea and are a good starting point.

**Recommended toolkit: GTK4 with libadwaita.** It already has breakpoints and adaptive building blocks (split views that collapse, bottom sheets, view switchers that move from the header to the bottom bar). Qt with Kirigami works too. Our own app (settings) and any default tools we add use GTK4, which matches the shell-ui choice in ADR-002 and gives one look across the system.

### 3. The system provides the signals

- **Size class:** apps get it from their window size, which the compositor sets per form factor.
- **Touch mode:** a system setting, published through the standard settings portal under our own name, that apps read to enlarge touch targets and spacing. It switches automatically when a keyboard is detached or a 2-in-1 folds into tablet mode.
- **Posture (later):** folded, unfolded or tent, for foldables.
- **Live resize, never restart:** when a foldable opens or a phone is docked to a monitor, the window just gets a new size. Apps must keep their state through that. This is a rule in the layout guide.

### 4. Apps that don't adapt

Many existing Linux desktop apps (for example GIMP or LibreOffice) were never designed for a phone screen. They still install and run everywhere, but:

- on a compact screen they open in a zoomable "desktop window" instead of being cut off,
- the store labels them so people know what to expect (next point),
- they shine when a foldable is open or the phone is docked.

### 5. The store shows where each app works well

App metadata already has a standard way (AppStream) to declare the screen sizes and input types an app supports. The app store (an existing Flatpak store front at first, per the fewest-parts map) reads it and shows simple badges: **Phone**, **Tablet**, **Desktop**. No new format to invent.

### 6. Web apps are first-class

Many everyday services are web-based and already adaptive. Users can install a web app so it gets its own icon, window and switcher entry like any other app.

## About "laptop power on the phone"

The phone runs the *same* app, with the same features. It does not get laptop *speed*: a phone chip is slower and has less room to stay cool. Light work feels the same; heavy work (video export, big games) is slower on the phone. That limit is hardware, not software.

A later idea that softens this: **handoff**, where you start something on the phone and continue it on the laptop with the same state.

## Options Considered

| Option | Verdict |
|--------|---------|
| A. Adaptive apps on an adaptive toolkit | **Recommended.** One app, one codebase, every device. |
| B. Separate phone and desktop apps (the Android vs macOS model) | Rejected. Double the work for every developer. |
| C. Compositor scales desktop apps, apps unchanged | Kept only as the fallback in section 4. Scaling a desktop UI onto a phone is exactly the "crowded" result to avoid. |
| D. Web apps only | A complement, not the base. |

## Consequences

- **Easier:** one app everywhere, one look, a store that tells the truth about each app.
- **Harder:** app developers must follow the layout guide; older apps stay desktop-first.
- **Revisit:** posture signals once a foldable is on the hardware list; handoff after the phone image exists.

## Action Items

1. [ ] Write the layout guide (size classes, touch targets, live resize rule).
2. [ ] Define the touch-mode setting and publish it through the settings portal.
3. [ ] Build the settings app as the first adaptive reference app.
4. [ ] Make sure the chosen store front shows AppStream-based device badges.
