# Shortcuts

Generated from `ACTIONS` in `crates/edel/src/shortcuts.rs` (roadmap M5.13); a cargo test holds this file to it. Change a shortcut with `edel system set shortcuts.ACTION=KEYS`, such as `edel system set shortcuts.close=Super+W`, or in `[shortcuts]` of the system file; `""` unbinds an action, and `edel system unset shortcuts.ACTION` brings its default back. Modifiers are `Super`, `Ctrl`, `Alt` and `Shift`; the other key is a letter, a digit or one of `Return`, `Tab`, `Space`, `Escape`, `Print`, `BackSpace`, `Delete`, `Insert`, `Home`, `End`, `Page_Up`, `Page_Down`, `Up`, `Down`, `Left`, `Right` and `F1` to `F12`. A way out (close, launcher, lock) always keeps keys, and no two actions share keys. An action whose step has not landed yet leaves its keys to the app that has the keyboard.

| Action | Keys | What it does | Way out |
|---|---|---|---|
| `close` | `Super+Q` | Close the focused window | yes |
| `launcher` | `Super` | Open the launcher (M5.3) | yes |
| `switcher` | `Alt+Tab` | Switch to the next window (M5.3) |  |
| `tiling` | `Super+T` | Switch this workspace between floating and tiling |  |
| `terminal` | `Ctrl+Alt+T` | Open a terminal |  |
| `screenshot` | `Print` | Take a screenshot (M6.3) |  |
| `lock` | `Super+L` | Lock the screen (M5.10) | yes |
| `workspace_1` | `Super+1` | Go to workspace 1 (M5.2) |  |
| `workspace_2` | `Super+2` | Go to workspace 2 (M5.2) |  |
| `workspace_3` | `Super+3` | Go to workspace 3 (M5.2) |  |
| `workspace_4` | `Super+4` | Go to workspace 4 (M5.2) |  |
| `workspace_5` | `Super+5` | Go to workspace 5 (M5.2) |  |
| `workspace_6` | `Super+6` | Go to workspace 6 (M5.2) |  |
| `workspace_7` | `Super+7` | Go to workspace 7 (M5.2) |  |
| `workspace_8` | `Super+8` | Go to workspace 8 (M5.2) |  |
| `workspace_9` | `Super+9` | Go to workspace 9 (M5.2) |  |
