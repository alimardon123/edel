# shell-ui

`edel-shell-ui`: the Edel shell's panels and menus (ADR-002), the second of its two long-running processes. Edition 2024, `rust-version = "1.85"`, GPL-3.0-or-later, `publish = false`.

## What belongs here

- What people use to find and switch things: the panel (M5.1b) and its workspace switcher (M5.2c), and later the launcher and switcher (M5.3), the window list and tray (M5.2), notifications and quick settings (M5.9), the lock screen and greeter (M5.10). Windows, title bars and effects belong to the compositor; settings pages to Settings.
- It draws its own surfaces (ADR-002's shell-ui toolkit decision of 2026-10-03): layer-shell surfaces through smithay-client-toolkit, drawing with tiny-skia into shared memory, text shaped by cosmic-text. No GTK, no GLib, no scripting, no theme engine. [docs/REVIEW-shells.md](../../docs/REVIEW-shells.md) is what its look and motion learn from.
- Colours and sizes come only from the design tokens (`edel::tokens`); never hard-code one in drawing code.
- Redraw only what changed, and only when it changed: an idle panel draws nothing between minutes.

## Layout

- `main.rs`: the Wayland connection; the preset (`shell.preset` from the machine's and the person's system file through `edel::system`, else Classic, read once at start through `edel::presets`) and one layer surface per panel in it (namespace `edel-panel`, along its edge, exclusive zone `size.panel`, a `size.radius` strip on its inner side for the fillets that is neither opaque nor clickable); the calloop loop with the minute timer that redraws the panels; buffers from a `SlotPool`; each panel's scale from `preferred_buffer_scale`; the tier from the compositor's state file (fillets unless Lite); a pointer from the seat, whose left clicks and scrolls go to the widget under them (`Shell::input`, by the places `paint` returned), and the `Live` state widgets show, kept by `workspaces_changed`; each change of where widgets lie is logged as `panel places NAME X+W, ...` in logical pixels, which CI reads to click them. It exits when the compositor goes away or takes a panel away; the compositor starts it again (`shellui.rs` there).
- `paint.rs`: `Look` (everything a panel shows at one scale, what each widget shows included), `Row` (the panel's widgets, from its start, in its centre and towards its end) and `paint`, which draws a panel into a tiny-skia pixmap (its colour, the fillets on either edge and the widgets laid out in their groups) and returns where each widget lies; `Text` (cosmic-text's fonts and glyph cache, loaded once) shapes and draws a `Line`; `rounded` is a rounded rectangle path; `to_argb` gives `wl_shm`'s byte order. Tested without a display.
- `widgets/mod.rs` (M5.1c): the widget table. A widget is one module here and one line in `TABLE`: its name (what presets write), the feature it needs if any, and four functions: what it shows now, from `Live` (the panel redraws only when that changes), its width and how it draws, on a `Canvas`, both from what it shows, and what an `Input` (a click, a scroll) on it does, an `Action` (show a workspace, move the switcher's view) or nothing (`no_input`). `usable` keeps the widgets a preset names that this machine can show and notes the rest: a name the table lacks, or a widget whose feature has no `/usr/share/edel/features/NAME.toml`. The tests fail when a line needs a feature with no file under `features/` or a built-in preset names a widget the table lacks.
- `widgets/menu.rs`: the menu button's icon, four rounded squares in a square as tall as the panel (the launcher it opens comes with M5.3).
- `widgets/workspaces.rs` (M5.2c): the workspace switcher. It shows `FIRST;NAME,NAME*,...`: at most three round buttons (`SHOWN`), the shown workspace a wider pill in `colour.accent`, the others filled with the panel text's colour at 14 percent, and a dot at a side where more lie; its width stays the same as the shown workspace changes. A click shows the workspace under it, a scroll moves the view, kept in sight. Tested. Its AccessKit node joins with M5.1d.
- `workspaces.rs` (M5.2c): the ext-workspace-v1 client, through sctk's `Dispatch2` user data (`Manager`, `Group`, `Handle`): the workspaces by name with the shown one marked, `show` (activate, then commit), and `workspaces_changed` at each `done`; without the protocol the switcher shows nothing.
- `widgets/clock.rs`: the clock, `HH:MM` with room on both sides, and the time until the next minute. Tested.
- `features/NAME.rs` (none yet): code that talks to an OS feature, such as NetworkManager for `network` (M5.9); its widget's line says `needs: Some("NAME")`.

## Running it

Inside a running compositor: `WAYLAND_DISPLAY=wayland-N cargo run -p edel-shell-ui`. The compositor starts it by itself when `edel-shell-ui` is on the `PATH`, as in the desktop image (the `shell` feature).
