# The compositor

`edel-compositor`: windows, floating and tiling, title bars, outputs and effects (ADR-002), one of the two long-running processes of the Edel shell. Edition 2024, `rust-version = "1.85"`, GPL-3.0-or-later, `publish = false`. Built on smithay, pinned to `=0.7.0` with only the features in use.

## What belongs here

- Window management, outputs, input routing, title bars, effects and the frame loop. The panel, dock, launcher and notifications belong to shell-ui (M5); settings pages to Settings.
- No extension API, no theme engine, no second renderer (GLES only), no IPC daemon (ADR-002). Floating and tiling are two implementations of `layout::WindowPolicy`, chosen per workspace.
- Colours and sizes come only from `design/tokens.toml` through `tokens::Tokens`; never hard-code one in drawing code.
- Readers of files other releases write (tokens, later `system.toml`) are lenient: unknown keys and bad values keep the built-in value and are reported, never fatal (ADR-008). `tokens::check` is the strict reader for tests and builds.

## Layout

- `lib.rs`: the parts that need no display, tested with `cargo test`:
  - `tokens.rs`: `Tokens::read` (lenient, starting from the built-in `design/tokens.toml`) and `check` (strict); `Colour::parse` takes `#rrggbb` and `#rrggbbaa`.
  - `telemetry.rs`: render times and idle frames; `percentile` is nearest rank, the same rule as CI's desktop test (M4.1); `Summary` prints `frames N, render p50 X ms, p99 Y ms, idle frames Z`.
  - `layout.rs`: the `WindowPolicy` trait and `OutputLayout` (mode, scale snapped to 1/120, logical size).
- `main.rs`: arguments (`--bench`) and the tokens from `/usr/share/edel/design/tokens.toml`, else the built-in ones.
- `state.rs`: `Edel`, the Wayland state: compositor, shm, xdg-shell, seat, data device, outputs, one `Space`; `dirty` says something changed.
- `winit.rs`: the development backend. A frame is drawn only when `dirty` is set (a commit, a new or closed window, input that focuses, a resize), so an idle desktop draws nothing; `--bench` stops after 5 s and prints the telemetry.

## Running it

On a desktop with X11 or Wayland: `cargo run -p edel-compositor`, then `WAYLAND_DISPLAY=wayland-N foot` with the socket it prints. Without a display (a cloud session), `xvfb-run -a cargo run -p edel-compositor -- --bench` works with Xvfb, Mesa (`libegl1`, `libgl1-mesa-dri`) and `libxkbcommon-x11-0` installed; a client such as `weston-simple-shm` makes it draw. The binary links `libxkbcommon` (`libxkbcommon-dev` to build).

## Coming changes (roadmap)

Make each change only in its step: M4.2b the DRM, GBM, libinput, udev and libseat backend on the virtual GPU, the health file `/run/edel/session/ready` and the image; M4.3 the floating policy and a test client; M4.4 title bars; M4.5 tiling; M4.6 outputs and input settings; M4.7 XWayland; M4.8 rollback of a dead desktop.
