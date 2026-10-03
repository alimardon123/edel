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
  - `layout.rs`: the `WindowPolicy` trait (`open`, `close`, `moved`, `arrange`), `Floating` (M4.3: centred, cascading `CASCADE` px off a window with the same centre, kept inside the area; the CI pixel checks depend on this rule) and `OutputLayout` (mode, scale snapped to 1/120, logical size). Policies place frames, not windows.
  - `frame.rs` (M4.4): server-side frames. `Insets` (the bar above, the border round the rest) turns a window's place into its frame's and back; `hit` says which part of a frame a point is on (title, maximize, close, an edge or corner within `GRIP` px); `paint` draws a bar described by a `Look` into ARGB pixels; `Text` is the title font (`fontdue`, the first of `FONTS` that loads) with its letters cached.
- `main.rs`: arguments (`--bench`), the tokens from `/usr/share/edel/design/tokens.toml`, else the built-in ones, and the backend: winit when `WAYLAND_DISPLAY` or `DISPLAY` is set, else DRM.
- `state.rs`: `Edel`, the Wayland state: compositor, shm, xdg-shell (offering maximize only), xdg-decoration, seat, data device, outputs, presentation time, one `Space` and the floating policy; `dirty` says something changed. A new toplevel waits in `unplaced` until its first buffer, then the policy places its frame and it takes the keyboard; `placed`, `outputs_changed` and `state_changed` keep the policy, the space and the state file in step, and a window that resizes itself or changes its title writes the state again; a window that drops its buffer leaves the screen and goes back to `unplaced`, to be placed anew when it draws again. `dragging` is set while a `WindowGrab` runs: never ask the pointer `is_grabbed` from code a grab calls, as it waits on its own lock. `listen` opens the socket.
- `decoration.rs` (M4.4): xdg-decoration (a window that asks for our bar or leaves the choice gets it, one that asks to draw its own keeps it), each window's `FrameData` in its user data (the bar's texture, redrawn only when its `Look` changes, the border buffers, the frame to go back to after maximizing), maximize and unmaximize, close, and `load_text`, which loads the font on a thread and redraws the bars when it arrives.
- `render.rs` (M4.4): `Element` and `Edel::elements`, what both backends draw: the windows from the top, each with its popups, bar and border.
- `input.rs`: keyboard and pointer for both backends (relative and absolute motion, click to focus and raise; `under` finds the frame part or surface at a point; a bar drags, double-clicks to maximize and its buttons act on release; edges resize; Super with the left or right button starts a move or resize; Super+Q closes) and the terminal Ctrl+Alt+F1 to F12 asks for.
- `grabs.rs`: `WindowGrab`, one pointer grab for moving and resizing (`resized` keeps the opposite edges and a 96x64 minimum; a maximized window lets go after 8 px); it hands the final place to `Edel::placed` when the button is released.
- `statefile.rs`: the writer thread of `/run/edel/session/state.toml` (only outside a session it does nothing); the newest state wins, and its new file is created under a fresh name, never opened through a link.
- `drm.rs` (M4.2b): the backend for real screens. A libseat session opens the primary GPU (`primary_gpu`, else the first); GBM, EGL and `GlesRenderer`; one `DrmCompositor` on the first connected connector at its preferred mode; libinput on the session. A frame is drawn when `dirty` is set and the last one's vblank has come; on the vblank, presentation feedback goes to clients and the first one writes `/run/edel/session/ready` and logs `edel-compositor: output NAME WxH ready`. A timer armed by the first frame after a quiet spell logs the telemetry once the screen has been still for 2 s, so nothing wakes while idle. Pausing the session (another VT) pauses DRM and libinput; activating resets the compositor's state and redraws.
- `winit.rs`: the development backend. A frame is drawn only when `dirty` is set (a commit, a new or closed window, input that focuses, a resize), so an idle desktop draws nothing; `--bench` stops after 5 s and prints the telemetry.

## Running it

On a desktop with X11 or Wayland: `cargo run -p edel-compositor`, then `WAYLAND_DISPLAY=wayland-N foot` with the socket it prints. Without a display (a cloud session), `xvfb-run -a cargo run -p edel-compositor -- --bench` works with Xvfb, Mesa (`libegl1`, `libgl1-mesa-dri`) and `libxkbcommon-x11-0` installed; a client such as `weston-simple-shm` makes it draw. The binary links libxkbcommon, libudev, libinput, libgbm and libseat (`libxkbcommon-dev libudev-dev libinput-dev libgbm-dev libseat-dev` on Ubuntu; `eudev-dev libinput-dev libseat-dev libxkbcommon-dev mesa-dev pkgconf` on Alpine); EGL is loaded at run time. In the desktop image greetd starts it; CI's `desktop-test` runs it under llvmpipe with `LIBGL_ALWAYS_SOFTWARE=1 LP_NUM_THREADS=0`, which virtio-vga needs (M4.1).

## Coming changes (roadmap)

`crates/testclient` is `edel-testclient` (M4.3): one window of `--size WxH`, `--colour RRGGBB` and `--title T`, drawn once and again only on resize, for CI's pixel checks; it exits when the compositor goes away. Only CI's images ship it.

Make each change only in its step: M4.5 tiling and `shell.title_bars`; M4.6 outputs, scale (the bar's texture is drawn at scale 1 until then) and input settings; M4.7 XWayland; M4.8 rollback of a dead desktop; M5.2 the minimize button.
