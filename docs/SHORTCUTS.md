# Shortcuts

Generated from `ACTIONS` in `crates/edel/src/shortcuts.rs` (roadmap M5.13); a cargo test holds this file to it. Change a shortcut with `edel settings set shortcuts.ACTION=KEYS`, such as `edel settings set shortcuts.close_window=Super+X`, or in `[shortcuts]` of the settings file; `""` unbinds an action, and `edel settings reset shortcuts.ACTION` brings its default back. Modifiers are `Super`, `Ctrl`, `Alt` and `Shift`; the other key is a letter, a digit or one of `Return`, `Tab`, `Space`, `Escape`, `Print`, `BackSpace`, `Delete`, `Insert`, `Home`, `End`, `Page_Up`, `Page_Down`, `Up`, `Down`, `Left`, `Right` and `F1` to `F12`; one modifier alone, such as the launcher's `Super`, is that key tapped with nothing else. A way out (close, launcher, lock) always keeps keys, and no two actions share keys. An action whose step has not landed yet leaves its keys to the app that has the keyboard.

| Action | Keys | What it does | Way out |
|---|---|---|---|
| `close_window` | `Super+Q` | Close the focused window | yes |
| `open_launcher` | `Super` | Open or close the launcher, Super tapped alone | yes |
| `switch_windows` | `Alt+Tab` | Switch windows, the most recently used first, while the modifiers are held; with Shift, back |  |
| `toggle_tiling` | `Super+T` | Switch this workspace between floating and tiling |  |
| `open_terminal` | `Ctrl+Alt+T` | Open a terminal |  |
| `take_screenshot` | `Print` | Take a screenshot (M6.3) |  |
| `lock_screen` | `Super+L` | Lock the screen (M5.10) | yes |
| `go_to_workspace_1` | `Super+1` | Go to workspace 1 |  |
| `go_to_workspace_2` | `Super+2` | Go to workspace 2 |  |
| `go_to_workspace_3` | `Super+3` | Go to workspace 3 |  |
| `go_to_workspace_4` | `Super+4` | Go to workspace 4 |  |
| `go_to_workspace_5` | `Super+5` | Go to workspace 5 |  |
| `go_to_workspace_6` | `Super+6` | Go to workspace 6 |  |
| `go_to_workspace_7` | `Super+7` | Go to workspace 7 |  |
| `go_to_workspace_8` | `Super+8` | Go to workspace 8 |  |
| `go_to_workspace_9` | `Super+9` | Go to workspace 9 |  |
| `move_to_workspace_1` | `Super+Shift+1` | Move the focused window to workspace 1 |  |
| `move_to_workspace_2` | `Super+Shift+2` | Move the focused window to workspace 2 |  |
| `move_to_workspace_3` | `Super+Shift+3` | Move the focused window to workspace 3 |  |
| `move_to_workspace_4` | `Super+Shift+4` | Move the focused window to workspace 4 |  |
| `move_to_workspace_5` | `Super+Shift+5` | Move the focused window to workspace 5 |  |
| `move_to_workspace_6` | `Super+Shift+6` | Move the focused window to workspace 6 |  |
| `move_to_workspace_7` | `Super+Shift+7` | Move the focused window to workspace 7 |  |
| `move_to_workspace_8` | `Super+Shift+8` | Move the focused window to workspace 8 |  |
| `move_to_workspace_9` | `Super+Shift+9` | Move the focused window to workspace 9 |  |
| `toggle_fullscreen` | `Super+F` | Make the focused window fill its screen, or leave fullscreen (M5.20) |  |
| `next_keyboard_layout` | `Super+Space` | Switch to the next keyboard layout of region.keyboard (M5.21) |  |
| `minimize_window` | `Super+H` | Minimize the focused window; the window list or Alt+Tab brings it back (M5.18a) |  |
| `toggle_maximize` | `Super+M` | Maximize the focused window, or give it its size back (M5.18a) |  |
| `focus_window_left` | `Super+Left` | Focus the window to the left of the focused one (M5.16a) |  |
| `focus_window_right` | `Super+Right` | Focus the window to the right of the focused one (M5.16a) |  |
| `focus_window_up` | `Super+Up` | Focus the window above the focused one (M5.16a) |  |
| `focus_window_down` | `Super+Down` | Focus the window below the focused one (M5.16a) |  |
| `move_window_left` | `Super+Shift+Left` | Swap the focused window with the one to its left, when tiled (M5.16a) |  |
| `move_window_right` | `Super+Shift+Right` | Swap the focused window with the one to its right, when tiled (M5.16a) |  |
| `move_window_up` | `Super+Shift+Up` | Swap the focused window with the one above it, when tiled (M5.16a) |  |
| `move_window_down` | `Super+Shift+Down` | Swap the focused window with the one below it, when tiled (M5.16a) |  |
| `cycle_column_width` | `Super+R` | Make the focused column a third, a half or two thirds of the screen wide, in the scroll style (M5.16c) |  |
| `show_tray` | `Super+B` | Open or close the tray's apps behind its arrow, with the keyboard on them (M5.9h) |  |
| `show_workspaces` | `Super+W` | Show every workspace side by side with its windows, or leave the overview (M5.2j) |  |
| `next_workspace` | `Super+Ctrl+Right` | Show the workspace after the shown one; stops at the last (M5.2i) |  |
| `previous_workspace` | `Super+Ctrl+Left` | Show the workspace before the shown one; stops at the first (M5.2i) |  |
| `move_to_next_workspace` | `Super+Ctrl+Shift+Right` | Move the focused window to the workspace after the shown one, and follow it (M5.2i) |  |
| `move_to_previous_workspace` | `Super+Ctrl+Shift+Left` | Move the focused window to the workspace before the shown one, and follow it (M5.2i) |  |
