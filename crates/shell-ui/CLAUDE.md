# shell-ui

`edel-shell-ui`: the Edel shell's panels and menus (ADR-002), the second of its two long-running processes. Edition 2024, `rust-version = "1.85"`, GPL-3.0-or-later, `publish = false`.

## What belongs here

- What people use to find and switch things: the panel (M5.1b), and later the launcher and switcher (M5.3), the window list and tray (M5.2), notifications and quick settings (M5.9), the lock screen and greeter (M5.10). Windows, title bars and effects belong to the compositor; settings pages to Settings.
- It draws its own surfaces (ADR-002's shell-ui toolkit decision of 2026-10-03): layer-shell surfaces through smithay-client-toolkit, drawing with tiny-skia into shared memory, text shaped by cosmic-text. No GTK, no GLib, no scripting, no theme engine. [docs/REVIEW-shells.md](../../docs/REVIEW-shells.md) is what its look and motion learn from.
- Colours and sizes come only from the design tokens (`edel::tokens`); never hard-code one in drawing code.
- Redraw only what changed, and only when it changed: an idle panel draws nothing between minutes.

## Layout

- `main.rs`: the Wayland connection, the panel's layer surface (namespace `edel-panel`, bottom, exclusive zone `size.panel`, a `size.radius` strip above it for the fillets that is neither opaque nor clickable), the calloop loop with the clock's minute timer, buffers from a `SlotPool`, the scale from `preferred_buffer_scale`, and the tier from the compositor's state file (fillets unless Lite). It exits when the compositor goes away; the compositor starts it again (`shellui.rs` there).
- `paint.rs`: `Look` (everything the panel shows at one scale) and `paint`, which draws it into a tiny-skia pixmap: the panel, the fillets, the menu icon and, with `Text` (cosmic-text's fonts and glyph cache, loaded once), the clock; `rounded` is a rounded rectangle path; `to_argb` gives `wl_shm`'s byte order. Tested without a display.
- `clock.rs`: the clock's text and the time until the next minute. Tested.

## Running it

Inside a running compositor: `WAYLAND_DISPLAY=wayland-N cargo run -p edel-shell-ui`. The compositor starts it by itself when `edel-shell-ui` is on the `PATH`, as in the desktop image (the `shell` feature).
