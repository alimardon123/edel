# Shortcuts

Generated from `ACTIONS` in `crates/edel/src/shortcuts.rs` (roadmap M5.13); a cargo test holds this file to it. Change a shortcut with `edel system set shortcuts.ACTION=KEYS`, such as `edel system set shortcuts.close=Super+W`, or in `[shortcuts]` of the system file; `""` unbinds an action, and `edel system unset shortcuts.ACTION` brings its default back. Modifiers are `Super`, `Ctrl`, `Alt` and `Shift`; the other key is a letter, a digit or one of `Return`, `Tab`, `Space`, `Escape`, `Print`, `BackSpace`, `Delete`, `Insert`, `Home`, `End`, `Page_Up`, `Page_Down`, `Up`, `Down`, `Left`, `Right` and `F1` to `F12`; one modifier alone, such as the launcher's `Super`, is that key tapped with nothing else. A way out (close, launcher, lock) always keeps keys, and no two actions share keys. An action whose step has not landed yet leaves its keys to the app that has the keyboard.

| Action | Keys | What it does | Way out |
|---|---|---|---|
| `close` | `Super+Q` | Close the focused window | yes |
| `launcher` | `Super` | Open or close the launcher, Super tapped alone | yes |
| `switcher` | `Alt+Tab` | Switch windows, the most recently used first, while the modifiers are held; with Shift, back |  |
| `tiling` | `Super+T` | Switch this workspace between floating and tiling |  |
| `terminal` | `Ctrl+Alt+T` | Open a terminal |  |
| `screenshot` | `Print` | Take a screenshot (M6.3) |  |
| `lock` | `Super+L` | Lock the screen (M5.10) | yes |
| `workspace_1` | `Super+1` | Go to workspace 1 |  |
| `workspace_2` | `Super+2` | Go to workspace 2 |  |
| `workspace_3` | `Super+3` | Go to workspace 3 |  |
| `workspace_4` | `Super+4` | Go to workspace 4 |  |
| `workspace_5` | `Super+5` | Go to workspace 5 |  |
| `workspace_6` | `Super+6` | Go to workspace 6 |  |
| `workspace_7` | `Super+7` | Go to workspace 7 |  |
| `workspace_8` | `Super+8` | Go to workspace 8 |  |
| `workspace_9` | `Super+9` | Go to workspace 9 |  |
| `move_to_workspace_1` | `Super+Shift+1` | Move the focused window to workspace 1 |  |
| `move_to_workspace_2` | `Super+Shift+2` | Move the focused window to workspace 2 |  |
| `move_to_workspace_3` | `Super+Shift+3` | Move the focused window to workspace 3 |  |
| `move_to_workspace_4` | `Super+Shift+4` | Move the focused window to workspace 4 |  |
| `move_to_workspace_5` | `Super+Shift+5` | Move the focused window to workspace 5 |  |
| `move_to_workspace_6` | `Super+Shift+6` | Move the focused window to workspace 6 |  |
| `move_to_workspace_7` | `Super+Shift+7` | Move the focused window to workspace 7 |  |
| `move_to_workspace_8` | `Super+Shift+8` | Move the focused window to workspace 8 |  |
| `move_to_workspace_9` | `Super+Shift+9` | Move the focused window to workspace 9 |  |
