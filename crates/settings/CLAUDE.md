# Settings (`edel-settings`)

Settings, the app for every setting (ADR-008, roadmap M5.6): GTK4 and libadwaita, as ADR-004 chose for our apps, the one app of ours on them. Edition 2024, `rust-version = "1.85"`, GPL-3.0-or-later, `publish = false`; not a default member, so `cargo run` builds only `edel`.

## What belongs here

- A page per section of the settings file, named and ordered by `edel::system::PAGES`, the table `edel settings` prints, so the app and the command never disagree (ADR-008's same names decision). A page is added in the step that brings its section (M5.7 to M5.10, M5.12); About closes the list.
- Every change is one key of the person's settings file (`edel::places::person_settings`, no privilege needed), written with `edel::system::set` and `unset`, the functions `edel settings set` and `reset` use, so a refused value reads the same in a row and on the command line, and the file is byte for byte what the command would write. A choice that is what would apply anyway is taken out of the file, never written (ADR-008: writers never write a default).
- Nothing of its own runs in the background; the desktop follows the file it writes (the compositor watches it, M4.5).

## Layout

- `main.rs`: the application (`APP_ID`, which `features/settings/usr/share/applications/APP_ID.desktop` is named after; a test holds them together), the window: an `AdwNavigationSplitView`, a sidebar listing `pages()` and the chosen page with a header bar; below 600sp (`NARROW`) the split view collapses and pages open one at a time (ADR-004's size classes).
- `files.rs`: `Files` (the machine's file and the person's), `layout()` (the person's `[layout]` over the machine's, then the preset), `choose_preset` and `choose_tiling` (written, or taken out when the default), `set`, `title` (a preset's name as people read it). Tested on files in a temp directory: never a default written, the file equal to `edel settings set`'s, a refused value's text equal to the command's.
- `layout.rs`: the Layout page: a row per preset (`edel::presets::NAMES`) and the Tile windows switch, which follows the chosen preset's policy unless `layout.tiling` is set.
- `about.rs`: the About page: the release from `/usr/lib/os-release` (`PRETTY_NAME`, `VERSION_ID`, `EDEL_CHANNEL`).

## Checks

The root CLAUDE.md's Rust checks cover it (the runner needs `libgtk-4-dev libadwaita-1-dev`); `ci/build.sh` builds it in Alpine with `gtk4.0-dev libadwaita-dev`, and the `settings` feature ships it. desktop-test's `settings` case opens it, checks its colours and clicks Mac-like (`ci/CLAUDE.md`). To see it without an image: run `edel-compositor` nested (it uses winit when `WAYLAND_DISPLAY` or `DISPLAY` is set, as under `Xvfb`) and start `edel-settings` on its socket; GTK 4.14 on Ubuntu warns about the light block of `gtk.css`, which 4.22 in the image reads.

## Coming (roadmap)

6b: each row says where its value comes from (`Automatic`, `Set by this machine`, `Set by profile NAME`) with Reset and Copy as command, `rows.rs` and `features/NAME.rs` pages hidden without their feature; 6c: `docs/layout-guide.md`. Make each change only in its step.
