# The settings file

**Date:** 2026-10-02, amended 2026-10-06 (M5.25: the Settings app's names) and 2026-10-10 (M5.2q: the Panels and Workspaces pages)

One TOML file describes a whole machine (ADR-006): the settings file, `settings.toml`, kept on the data partition and linked from `/etc/edel/settings.toml`. It holds only what a person chose; a missing key means the release decides (ADR-008). It never holds personal files or passwords. Its sections are the pages of the Settings app and its keys their rows, named the way the app names them (ADR-008's same names decision), so whoever knows the app can read the file and the command line, and back.

```toml
format = 1

[layout]
preset = "mac-like"

[appearance]
mode = "dark"

[network]
hostname = "ali-laptop"

[users.ali]
admin = true
ssh_keys = ["ssh-ed25519 AAAA... ali@desk"]
```

## The command

`edel settings` is the command line's Settings app ([Commands](guide/commands.md)):

| Command | What it does |
|---|---|
| `edel settings` | Lists the pages, in the app's order |
| `edel settings get [KEY or PAGE]` | Shows settings with their values and where each comes from; `--toml` prints the file's own TOML |
| `edel settings set KEY=VALUE...` | Changes one or more settings, keeping every other byte of the file |
| `edel settings reset KEY...` | Gives settings back to the release, as the app's Reset does |
| `edel settings diff [FILE]` | Shows what apply would change; exits 1 when anything would |
| `edel settings apply` | Makes the machine match its file |
| `edel settings import FILE` | Applies FILE and keeps it as the machine's file |
| `edel settings export` | Prints the machine as a settings file, to import on another one |
| `edel settings check FILE` | Checks a file strictly |

A list takes `KEY+=VALUE` to add an item and `KEY-=VALUE` to remove one, such as `edel settings set startup.apps+=org.mozilla.firefox`; a list left empty removes its key. Someone who is not root changes their own settings file for the keys the desktop follows; a machine key needs an administrator, and the refusal names the command that gives one. `get` shows the value that applies to a key no file sets, with the layer that decides it: the preset in use, or the release, the image or the hardware. A key a release has stopped using is still read and reported, and `set` refuses it until `reset` takes it out.

A mistyped key or value is answered with the nearest one: `edel settings set layout.presset=hive` says "did you mean layout.preset?". Every part reads the file with the one parser in `edel::settings` (`crates/edel/src/settings.rs`), whose key table this page follows; the reading rules are in [FORMATS.md](FORMATS.md).

## Where it lives

On the first boot, `edel settings apply` seeds the machine's file from the first of: a volume labelled `EDEL-SEED` holding a settings file, the EFI system partition's `/EFI/edel/`, and the slot's own in `/usr/share/edel/`. Without any, nothing is applied. The `edel-settings` service applies it at every boot, before the hostname is set. A person's own file in `~/.config/edel/` has the same keys, and for the pages the desktop follows at once (Layout, Displays, Appearance, Shortcuts) its values win over the machine's. The file's name is written once, in `edel::places`; a file by a name it had before is still read, and the next apply renames it.

"From" names the roadmap step that first acts on a key. Until then `edel settings check` and `set` refuse the key with "not supported yet", and the boot apply skips it and says so. `NAME` is any name: a person, a screen's connector, an action, a feature. A person's name has up to 32 lowercase letters, digits, `-` and `_`, and starts with a letter or `_`.

## Layout: `[layout]`

The desktop follows these keys at once (M4.5): it reads the machine's file and the person's, and reads both again whenever either is written.

| Key | Value | From |
|---|---|---|
| `layout.preset` | a preset this release has: `classic`, `hive` (M5.4a), `windows-like` (M5.4c), `mac-like` (M5.4d) or `zen` (M5.18b), with `tablet` and `phone` to come with M9; or one of your own `[presets.NAME]` tables, such as `"mine"`; missing is `classic`. A changed preset re-lays out the windows and restarts the panel at once; a name this release lacks gives Classic, and `edel settings diff` says so | M5.4a, M5.17 |
| `layout.tiling` | `true` or `false` | M4.5 |
| `layout.title_bars` | `always` (every window has its bar), `floating-only` (only floating windows have one) or `never` (no window has one, in floating or tiling; Super with a drag still moves a window and Super+Q still closes it). Missing is the preset's (`always` in every preset but Zen, which is `never`, M5.18b) | M4.5, `never` M5.18b |
| `layout.tiling_style` | `stack`, `split` or `scroll`: how tiling lays windows out, one main window on the left and the rest stacked on the right (`stack`), each new window taking half of the focused one's space, cut across its longer side, as Hyprland tiles (`split`), or each window a column half the screen wide on a strip that scrolls to the focused one, as niri tiles (`scroll`, M5.16c); missing is the preset's style (Zen's is `split`, M5.18b), else `stack`. It never switches a workspace between floating and tiling: a floating one takes the style when it next tiles. The panel's layout button offers the same choice on a right click (M5.16b) | M5.16a |
| `layout.device_type` | `desktop`, `tablet` or `phone`; missing detects it | M9.3 |
| `layout.window_buttons` | `left` or `right`: the side of the title bars where close, minimize and maximize sit, close outermost; missing is the preset's (right in Classic and Hive); GTK and libadwaita apps' own header bars follow it through the settings portal (M6.16) | M5.4b |
| `layout.close_button` | `true` or `false`: whether every title bar shows its close button; missing shows it. Super+Q closes a window whatever is shown; apps' own header bars follow it through the settings portal (M6.16) | M5.18a |
| `layout.minimize_button` | `true` or `false`: whether every title bar shows its minimize button; missing shows it; apps' own header bars follow it through the settings portal (M6.16) | M5.18a |
| `layout.maximize_button` | `true` or `false`: whether every title bar shows its maximize button; missing shows it. A hidden button's room goes to the title; apps' own header bars follow it through the settings portal (M6.16) | M5.18a |

## Panels: `[panels]`

The Panels page (M5.2q): the desktop follows these keys at once.

| Key | Value | From |
|---|---|---|
| `panels.list` | panels in place of the preset's, each as a preset writes one: `edge` (`top` or `bottom`), `style` (`bar`, the default, or `dock`), for a dock `hide` (`never`, the default, or `covered`: it then keeps no space and hides while a window covers it, M5.4f), `size` (`small`, `medium`, the default, or `large`: the tokens' `size.panel_small`, `size.panel` and `size.panel_large`, M5.31c), `floating` (`false`, the default, or `true`: a bar with a gap, `size.gap`, from its edge and both sides and rounded corners, as a dock has; a dock cannot float, M5.31c), `screens` (`main`, the default, or `every`: one panel on each screen, M5.31c) and the widgets by name in `start`, `centre` and `end`, at most one panel along each edge; as `[[panels.list]]` tables, or on the command line `edel settings set 'panels.list=[{ edge = "bottom", end = ["clock"] }]'`; missing is the preset's. A change restarts the panel at once | M5.4e, M5.31c |
| `panels.tray` | a list of the tray's apps kept in the panel, by the `Id` their tray item gives (`["nm-applet", "edel-testclient"]`), in the panel's order; every other app waits behind the tray's arrow, and an app new to the tray goes behind it too; missing keeps none in the panel. Dragging an icon from the arrow's grid onto the panel adds it, dragging one off the panel takes it out; Settings' Layout page has it as the Tray card, one choice per app, the same choice the drag makes; the panel follows a change at once, whoever made it. `edel settings set panels.tray=nm-applet` is refused with "expected a list of texts in quotes, not ..." (the value must be a list in brackets) | M5.9g |
| `panels.pinned` | the apps the apps widget pins, in order, each an app's id or a role (`files`, `browser`, `mail`, `editor`, `terminal`, `music` or `settings`), those not installed left out; absent is the layout preset's. Dragging an app's icon on the panel pins it, moves it along or takes it off (dragged off the panel); Settings' Layout page lists the apps under Panels, and `edel settings set panels.pinned='["settings", "terminal"]'` sets the same list | M5.31d |

## Workspaces: `[workspaces]`

The Workspaces page (M5.2q): how many there are, their names and the apps that open on their own. The desktop follows these keys at once.

| Key | Value | From |
|---|---|---|
| `workspaces.count` | a whole number from 1 to 9: how many workspaces, one for each of Super+1 to Super+9; missing is the preset's count (Classic has 4). Dynamic workspaces ignore it. `edel settings set workspaces.count=10` is refused with "expected a whole number from 1 to 9, not 10" | M5.2i |
| `workspaces.dynamic` | `true` or `false`: when `true`, an empty workspace always waits at the end, and other empty ones close as they empty, so the count follows the windows (as GNOME and COSMIC do); a workspace with a name stays when it is empty; `workspaces.count` is then ignored. Missing is `false` | M5.2i |
| `workspaces.per_screen` | `true` or `false`: when `true`, each screen shows its own workspace, so Super+1 to Super+9 switch only the screen the pointer is on, and a window's workspace follows its screen; when `false`, every screen switches together (as Cinnamon does). Missing is `false` | M5.2k |
| `workspaces.names` | a list of texts, the first workspace's name first, such as `["Mail", "", "Code"]`; an empty text means no name, so the workspace shows its number. A named workspace stays when empty in dynamic mode. Each name goes to the clients that follow workspaces (ext-workspace-v1) and to the state file's `workspace_names`; the switcher shows each name in its pill or underline (M5.2m) | M5.2i |
| `workspaces.wallpapers` | a list of wallpapers, the first workspace's first, such as `["", "#336699"]`: each entry is a value `appearance.wallpaper` takes (a wallpaper's name, a picture's path or a colour `"#rrggbb"`), and an empty text is the same as Appearance's, so that workspace shows `appearance.wallpaper` (and `appearance.wallpaper_dark` in dark mode). Missing is Appearance's wallpaper on every workspace. Trailing empty texts are dropped. An entry that `appearance.wallpaper` would refuse is refused with its number, such as `edel settings set 'workspaces.wallpapers=["", "Sea"]'` giving "entry 2: \"Sea\" is not a wallpaper; ..."; a name with no picture shows the background colour, as `appearance.wallpaper` does | M5.12b5 |
| `workspaces.apps` | a table of app ids and workspace numbers, such as `{ "org.mozilla.firefox" = 2 }` (on the command line `edel settings set 'workspaces.apps={ "org.mozilla.firefox" = 2 }'`): each app opens on its own workspace, a number from 1 to 9, so Firefox always opens on 2 while you stay where you are; the app id is the one its window gives, and a window of an app not listed opens on the workspace in use. Missing means apps open on the workspace in use. `edel settings set workspaces.apps={ "org.mozilla.firefox" = 10 }` is refused with "org.mozilla.firefox has workspace 10; use a whole number from 1 to 9" | M5.2l |
| `workspaces.overview_strip` | `left`, `right`, `top` or `bottom`: the side of the screen the workspace overview's strip of workspaces lies on (Super+W, or a click on the panel switcher's lit workspace); the windows take the rest. Missing is `left`. `edel settings set workspaces.overview_strip=diagonal` is refused with "unknown value \"diagonal\"; use left, right, top or bottom" | M5.2j-b |

## Displays: `[displays.NAME]`

Each screen by its connector's name, such as `eDP-1` for a laptop's own screen; the desktop follows these at once (M4.6). Settings' Displays page (M5.7a) lists the screens the compositor reports and draws them side by side; a scale of 100, 125, 150, 175 or 200 percent, a resolution from the screen's own list, a position and an on switch each write one of these keys, and a choice that is what the screen would get anyway (its preferred resolution, the scale worked out from its size) is taken out of the file, never written. `edel-settings --page displays` opens the app on that page. The last screen that is on cannot be turned off from the page.

| Key | Value | From |
|---|---|---|
| `displays.*.position` | two whole numbers, `[x, y]` | M4.6 |
| `displays.*.scale` | a number, such as `1.25` | M4.6 |
| `displays.*.resolution` | `WIDTHxHEIGHT`, such as `"1920x1080"`; missing is the screen's own | M4.6 |
| `displays.*.refresh_rate` | a number of Hz, such as `60`; missing is the resolution's best | M4.6 |
| `displays.*.enabled` | `true` or `false` | M4.6 |
| `displays.*.rotation` | `0`, `90`, `180` or `270` | M9.8 |

## Sound: no section

The Sound page (M5.7b) has no key. The volume, the mute switch and the device in use are the sound system's own live state, which WirePlumber remembers for each person, so a line of the file would only be a second copy that could disagree. The page and the command line are the same level all the same: each row copies the `wpctl` command that does what it does (`wpctl set-volume @DEFAULT_AUDIO_SINK@ 30%`, `wpctl set-mute @DEFAULT_AUDIO_SINK@ 1`, `wpctl set-default ID`), and `edel settings get sound` says where to look.

## Appearance: `[appearance]`

| Key | Value | From |
|---|---|---|
| `appearance.theme` | a built-in theme's name (`edel`, `aurora`, `earth` or `ocean`) or one of your own `[themes]`, such as `"my-aurora"`; missing is `edel`, the release's own, which changes nothing. The theme's values apply under the ones written here, and the theme's pictures give `appearance.wallpaper` when it is missing | M5.12b6a |
| `appearance.wallpaper` | a wallpaper's name (`"hills"`, the light or dark picture as the colour scheme says), a picture's path, a colour `"#rrggbb"` or `"none"`; absent is Hills where the `wallpapers` feature carries it, else the background colour | M5.10c4b |
| `appearance.wallpaper_dark` | the same kinds of value as `appearance.wallpaper`, for dark mode when it differs: a person's own light and dark pair is two picture paths, one in each key; absent is the dark side of `appearance.wallpaper` | M5.10c4d |
| `appearance.pictures` | the pictures of the theme in use, on top of its own: a list of wallpapers as `appearance.wallpaper` takes them, a wallpaper's name such as `["aurora", "earth"]`, a picture's full path or a colour; never copies of the files. Absent is the theme's own list, and `[]` shows none; at most 32, none twice. Set in the pictures sheet of Appearance (Add pictures, a picture's menu and drag and drop), or with `edel settings set appearance.pictures='["aurora"]'`. Changing a built-in theme this way makes it Changed; Keep as new saves the person's copy | M5.12b6, part c2 |
| `appearance.mode` | `light`, `dark` or `auto`: the colours of the title bars, the panels and, through the settings portal, the apps; `auto` is the release's choice, light in this one, and missing is `auto`. A change takes effect at once | M5.5c |
| `appearance.contrast` | `normal` or `high`: high contrast gives strong edges and the plainest colours, in both schemes, text at 7 to 1 or more and lines in black or white, over every colour the person chose; missing is `normal`. The desktop follows it at once | M6.9 |
| `appearance.accent` | one of the release's named accents (`blue`, `purple`, `pink`, `red`, `orange`, `yellow`, `green`, `teal`, `graphite`), `wallpaper` for a colour taken from the picture, or a colour `"#rrggbb"`; missing is `blue`. The text on the accent is whichever of the two schemes' reads better. `edel settings set appearance.accent=red` works; `edel settings set appearance.accent=crimson` is refused with "`"crimson"` is not an accent; choose blue, purple, pink, red, orange, yellow, green, teal, graphite, wallpaper or a colour such as "#d0363f"". An accent that leaves its own pairs under their contrast ratio is refused too, with the pair named | M5.12b1 |
| `appearance.colours.*` | a colour `"#rrggbb"` or `"#rrggbbaa"` for any colour token by its name (`background`, `panel`, `title_bar_focused`, ...), in both schemes; missing is the release's. `edel settings set appearance.colours.background=#202020` works; `edel settings set appearance.colours.panl=#ffffff` is refused with `appearance.colours.panl: "panl" is not a colour; did you mean panel?`. `edel settings reset appearance.colours.background` takes it back. A colour that leaves a pair under its ratio is refused with that pair's line, in both schemes | M5.12b1 |
| `appearance.colours.light.*` | the same, for the light scheme alone, over the shared one: `edel settings set appearance.colours.light.panel=#f6f7f9` works | M5.12b1 |
| `appearance.colours.dark.*` | the same, for the dark scheme alone: `edel settings set appearance.colours.dark.panel=#11111b` works. `edel settings set appearance.colours.panel=#1e1e2e` is refused, since the light scheme's text would not read on that panel; the refusal names the pair | M5.12b1 |
| `appearance.radius` | a whole number from 0 to 24 logical pixels: the windows' corner radius; the menus', the controls' and the small corners scale with it, and 0 makes every corner square; missing is the release's 10 | M5.12b2 |
| `appearance.gaps` | a whole number from 0 to 64 logical pixels: the gap between tiled windows, and at the screen's edge too unless `appearance.gaps_outer` is set; missing is the release's 8 | M5.12b2 |
| `appearance.gaps_outer` | a whole number from 0 to 64 logical pixels: the gap between tiled windows and the screen's edge; missing is `appearance.gaps`' value, or the release's 8 when that is missing too | M5.12b2 |
| `appearance.border_width` | a whole number from 0 to 10 logical pixels: the border round each window with a title bar; missing is the release's 1 | M5.12b2 |
| `appearance.title_bar_height` | a whole number from 24 to 48 logical pixels: the title bar's height; missing is the release's 28 | M5.12b2 |
| `appearance.panel_opacity` | a whole number from 20 to 100, in percent: the panels' opacity; missing is 100 | M5.12b2 |
| `appearance.window_opacity` | a whole number from 20 to 100, in percent: the focused window's opacity, its title bar and border with it; missing is 100 | M5.12b3 |
| `appearance.window_opacity_unfocused` | a whole number from 20 to 100, in percent: every other window's opacity; missing is 100 | M5.12b3 |
| `appearance.shadow_size` | a whole number from 0 to 64 logical pixels: the shadow's blur under each window, 0 for none; missing is the release's 24 | M5.12b3 |
| `appearance.shadow_darkness` | a whole number from 0 to 100, in percent: the shadow's darkness in both colour schemes; missing is the scheme's own shadow | M5.12b3 |
| `appearance.blur` | a whole number from 0 to 100, in percent: how strongly the wallpaper blurs behind see-through panels, frosted glass; 0 shows it sharp; missing is the release's 50 | M5.12b3 |
| `appearance.font` | text, the interface font's family as fontconfig names it, such as `"Noto Sans"`; missing is the release's (Inter Variable). The title bars and everything the panels and apps draw follow it at once | M5.12b5 |
| `appearance.monospace_font` | text, the family apps use for code and terminals; missing is `"Noto Sans Mono"` | M5.12b5 |
| `appearance.text_size` | a whole number from 80 to 150, in percent of the release's sizes; missing is 100. `edel settings set appearance.text_size=150` makes every text size a half bigger, and the title bars, panels and apps follow | M5.12b5 |
| `appearance.cursor_size` | a whole number from 16 to 96, the cursor's height in logical pixels; missing is 24. The compositor draws the pointer at it at once, and the programs it starts get it as `XCURSOR_SIZE`; apps already open keep theirs | M5.12b5 |
| `appearance.cursor_theme` | a cursor theme's folder name, one this machine has under `/usr/share/icons` or `/usr/local/share/icons` (one with a `cursors` folder); missing is `default`, the drawn arrow when no such theme is installed. The check refuses a name the machine lacks and lists the ones it has | M5.12b5 |
| `appearance.icon_size` | a whole number from 75 to 150, in percent of the release's icons; missing is 100. The panel's icons and the apps' icons in the bar and the dock follow it, the panel's height does not change | M5.12b5 |
| `appearance.animations` | `full`, `reduced` (fades only) or `off`; missing is `full`. The desktop follows it at once | M5.11b |
| `appearance.animation_speed` | a whole number from 25 to 400: how fast things move, as a percent of the release's speed; 200 halves every length, 50 doubles it; missing is 100. The desktop follows it at once | M5.12b4 |
| `appearance.animation_curve` | `smooth`, `snappy` (quicker, settles sooner) or `springy` (overshoots a little, then rests); missing is `smooth`. A feel, each giving every kind its curve: under smooth, pop-ups are snappy; under springy, a workspace switch is snappy, not swinging past its place (M8.21 part 21e). The desktop follows it at once | M5.12b4 |
| `appearance.switcher_look` | `numbers` or `button`: `numbers` shows round buttons for the workspaces around the shown one, numbered as Super+1 to Super+9 are, the shown one a wider pill, with a workspace's name in its pill (the default); `button` shows the word Workspaces over an underline split into one segment per workspace, the shown one in the accent, and a click on a segment shows its workspace, a scroll the next or previous one. Missing is `numbers`. `edel settings set appearance.switcher_look=button` switches the look at once | M5.2m |
| `appearance.switcher_shown` | a whole number from 1 to 9: how many numbers the `numbers` look shows at once, the shown workspace and its neighbours; missing is 3. `edel settings set appearance.switcher_shown=10` is refused with "expected a whole number from 1 to 9, not 10" | M5.2m |
| `appearance.switcher_ends` | `fade`, `arrows` or `counts`: where more workspaces lie than show, an arrow at each end, dark where more lie that way and faint where none do (`arrows`), the next one peeking in, faded (`fade`), or with how many lie beyond it, as `+2` (`counts`); a click on an end shows the view one step towards it. Missing is `arrows`. `edel settings set appearance.switcher_ends=dots` is refused with "unknown value \"dots\"; use fade, arrows or counts" | M5.2m |
| `appearance.widgets` | a list of widgets the desktop shows over the wallpaper and under the windows, as `[{ widget = "clock", at = "top_right" }]`: only `clock` and `date` stand there, each with an anchor and an offset as on the lock screen; missing or `[]` is none, so the desktop shows nothing. `edel settings set appearance.widgets='[{ widget = "person", at = "centre" }]'` is refused with "the person does not stand on the desktop; the desktop takes clock or date". The Appearance page's Desktop widgets switch sets the clock and the date, and its Edit button moves them. The clock and the date also take `colour` (M5.33d), with the values and the checks of `lock_screen.widgets`' `colour`, over the desktop's wallpaper with no dim: `edel settings set appearance.widgets=...` with a yellow date over a white `appearance.wallpaper` is refused with `appearance.widgets: the date's colour "#ffff00" is #ffff00 on its wallpaper #ffffff and reads at 1.0:1; text needs 4.5:1, such as #7a7a00`. The editor's card shows the colours as swatches, with Use on every widget. Removed with `edel settings reset appearance.widgets` | M8.21 parts 21j1 and 21j2, and 21k1 (M5.33d)

## Lock screen: `[lock_screen]`

The lock and login screens are made of widgets (M5.10c5), each placed by an anchor and an offset, so one arrangement fits a phone, a laptop and a 4K screen. Missing is the classic arrangement, the mockup's.

| Key | Value | From |
|---|---|---|
| `lock_screen.widgets` | a list of widgets, each a table: `widget` (`clock`, `date`, `count`, `player`, `person`, `message`, `picture` or `battery`), `at` (where it stands: `top_left`, `top`, `top_right`, `left`, `centre`, `right`, `bottom_left`, `bottom` or `bottom_right`), and optionally `x` and `y` (an offset, a share of the screen's width and height, from -1 to 1), `text` (a message's words, 1 to 120 characters on one line) and `path` (a picture's file, an absolute path ending `.png`, `.jpg` or `.jpeg`). The person stands exactly once, so nobody is locked out; each other widget at most once, except the clock, of which up to 3 stand, and at most 3 messages and 3 pictures. Widgets at one anchor stack, one below the other. A clock also takes `face` (`digital`, `stacked`, `analog` or `minimal`; absent is `digital`), `hours` (12 or 24; absent is 24, and 12 writes `2:06`, with no AM or PM), `size` (`small`, `medium` or `large`; absent is `large`, the token's size, `medium` two thirds of it and `small` a half) and `weight` (100 to 900 in steps of 100; absent is the token `font.lock_clock_weight`), `zone` (an IANA time zone's name, such as `Asia/Tokyo`, `Europe/London` or `UTC`; absent is this machine's time, and a zone the machine has no zone files for shows this machine's time too) and `label` (1 to 24 characters on one line, drawn under the clock; absent with a zone is the zone's city, so `Asia/Tokyo` reads `Tokyo`). Each clock has its own options, so two clocks can show two zones: `{ widget = "clock", at = "top_right", face = "analog", size = "small", zone = "Asia/Tokyo" }` beside the classic clock. A date takes `show` (a list of `weekday`, `day`, `month` and `year`, each at most once, shown in that order whatever order is written; absent is weekday, day and month) and `style` (`long`, `short` or `numeric`; absent is `long`, as `Saturday 3 October 2026`, `Sat 3 Oct 2026` or `Sat 03.10.2026`). Each option is refused on another widget. An example: `[[lock_screen.widgets]]` tables with `widget = "clock"`, `at = "centre"`, `face = "analog"`, `hours = 12` and `widget = "date"`, `at = "top"`, `style = "short"`; on the command line `edel settings set 'lock_screen.widgets=[{ widget = "clock", at = "top", face = "stacked", hours = 12, size = "medium" }, { widget = "date", at = "top", show = ["weekday", "day"], style = "short" }, { widget = "person", at = "centre" }]'`. On the command line `edel settings set 'lock_screen.widgets=[{ widget = "person", at = "centre" }]'`; `edel settings set lock_screen.widgets=[{ widget = "person", at = "top" }]` is refused with "the lock screen needs the person and their password field" The clock, the date, a message and the battery also take `colour` (M5.33d): absent is automatic (the text colour that reads over the wallpaper); `accent` follows the accent (so the wallpaper too when `appearance.accent` is `wallpaper`); a named accent such as `blue`, in the scheme shown; `wallpaper`, a colour taken from the look's wallpaper as the accent's is; or a colour such as `"#e8b04a"`. Below 3.0:1 for a clock, or 4.5:1 for the others, over the wallpaper it is refused with the pair named: `edel settings set 'lock_screen.widgets=[{ widget = "person", at = "centre" }, { widget = "clock", at = "top_left", colour = "accent" }]'` over the classic look in the light scheme gives `lock_screen.widgets: the clock's colour "accent" is #2d6ae3 on its wallpaper #2f3c5e and reads at 2.2:1; large text needs 3.0:1, such as #5184e8`. A picture is not read at check time, so over one a colour is held to the token `backdrop_deep` under the dim; when drawn, every colour is made light or dark enough to read. The count, player, person and picture take none. The editor's card shows the colours as swatches, with Use on every widget. | M5.10c5, M5.10c5b, M5.33d |
| `lock_screen.look` | a look's name: `classic` (the mockup's, the default), `magazine` (a large clock at the left with the date under it and the person at the right), `minimal` (the time alone), or one of your own in `looks`. A name that is neither is refused, and the lock screen falls back to classic if the file is read leniently. Set with `edel settings set lock_screen.look=minimal` | M5.10c5c |
| `lock_screen.wallpaper` | what lies behind the widgets: `backdrop` (the navy scene, classic's default), `blurred` (the wallpaper that applies, blurred for glass, every other look's default), `sharp` (the wallpaper, sharp), a colour such as `"#1f2b48"`, or a picture's full path such as `"/home/you/lock.png"` (ending `.png`, `.jpg` or `.jpeg`). Missing is the look's. Set with `edel settings set 'lock_screen.wallpaper="#1f2b48"'`; `edel settings set lock_screen.wallpaper=lock.png` is refused, as a picture needs its full path | M5.10c5c |
| `lock_screen.dim` | how much the wallpaper is darkened, in percent, from 0 to 80, so the white words read. Missing is the look's own: 30 for a blurred or colour wallpaper and 40 for a sharp one or a picture (the built-in amounts, `DIM` and `DIM_SHARP` in `edel::lock_screen`). Set with `edel settings set lock_screen.dim=40`; a value of 81 is refused. The Dim slider on the Lock screen page, and in the lock editor's wallpaper panel, set it | M5.10c7g |
| `lock_screen.notifications` | `count`, which shows how many notifications are unread, or `none`, which hides the count. Missing is `count`. `edel settings set lock_screen.notifications=some` is refused with the values it takes | M5.10c5c |
| `lock_screen.looks` | the person's own looks, each a table named by the look (`[lock_screen.looks.home]`) with `widgets`, a list of widgets checked as `lock_screen.widgets` is (so each has the person), and `wallpaper`, as `lock_screen.wallpaper` takes it (absent is the wallpaper, blurred); choosing the look brings both, and `lock_screen.widgets` and `lock_screen.wallpaper` set beside it are changes over it. A name is letters, digits and hyphens and not a built-in look's, and it is chosen with `lock_screen.look`. Made by Save as on the Lock screen page. An example: `[lock_screen.looks.home]` with `widgets = [{ widget = "clock", at = "top", y = 0.1 }, { widget = "person", at = "centre" }]`, then `look = "home"` under `[lock_screen]`; on the command line `edel settings set 'lock_screen.looks={ home = { widgets = [{ widget = "person", at = "centre" }] } }'`; a look without the person is refused | M5.10c5c |
| `themes.*` | your own themes, each a table named by the theme (`[themes.my-aurora]`) with `title` (absent is the name made readable), `pictures`, a list of wallpapers as `appearance.wallpaper` takes them, and the appearance values the theme sets, the keys of the table itself (`accent = "wallpaper"`, `[themes.NAME.colours]`); never `theme`, `mode`, `wallpaper`, `wallpaper_dark` or the switcher's keys, which stay outside a theme. Written by `edel theme save`; a built-in theme's name is refused. A theme goes to another machine as one `NAME.edeltheme` file (`edel theme export NAME`, read back by `edel theme import FILE`): an archive of its table and its JPEG or PNG pictures, nothing that runs, checked before anything is written; an imported theme's pictures are kept in the person's themes folder (`$XDG_DATA_HOME/edel/themes`) | M5.12b6a, M5.12b6 |
| `presets.*` | your own layout presets, each a table named by the preset (`[presets.mine]`) with `preset`, the built-in preset it starts from (absent is `classic`), and the `[layout]` keys it sets as the table's own keys, such as `window_buttons = "left"`; `layout.preset = "mine"` picks it, and each `[layout]` key the file leaves unset is the preset's. A name of a built-in preset is refused, and so is a preset that starts from one of yours. Written by Settings' Save as preset or `edel preset save NAME`, removed by `edel preset delete NAME`; a preset holds the layout only, so its panels and pinned apps are its built-in preset's. A base this release lacks is Classic, which `edel settings diff` says | M5.17 |

## Shortcuts: `[shortcuts]`

| Key | Value | From |
|---|---|---|
| `shortcuts.*` | keys for the action `*`, such as `close_window = "Super+X"`, or `""` for none; the actions, their default keys and the key names are in [Keyboard shortcuts](SHORTCUTS.md). Check and set refuse an unknown action, two actions on one key and a way out (close a window, the launcher, the lock) without keys. The desktop follows them at once | M5.13a |

## Notifications: `[notifications]`

| Key | Value | From |
|---|---|---|
| `notifications.do_not_disturb` | `true` or `false`: whether banners stay away; missing is `false`. Notifications still arrive and are listed in the notification centre, which the clock opens, and a critical one still shows its banner. shell-ui reads the file at each notification, so a change by `edel settings set` applies to the next one. Settings' Notifications page has it as the Do not disturb row, quick settings as its tile, and the centre as its switch row. `edel settings set notifications.do_not_disturb=maybe` is refused with `expected true or false` | M5.9b |
| `notifications.position` | Where banners appear: `auto`, by the status area wherever the panel puts it, the default; `top-right`, `top-left`, `bottom-right` or `bottom-left`, that corner; or `top`, the middle of the top edge. A banner keeps clear of every panel. shell-ui reads it at each banner, so `edel settings set notifications.position=top-right` applies to the next one. Settings' Notifications page has it as the Where notifications appear row. `edel settings set notifications.position=middle` is refused with the values it takes | M5.9j |
| `notifications.badge` | What the notifications button, a panel widget a person adds with Edit panels, shows while notifications are unread: `dot`, a small dot in the accent, the default; `count`, how many; or `none`. Opening the notifications reads them all. Settings' Notifications page has it as the Unread badge row; shell-ui follows a change at once | M5.9k |
| `notifications.list` | Where the notification list opens, by the clock or the notifications button: `button`, hung on the button that opened it, the default; `left` or `right`, a sheet the screen's height along that side, with the calendar at its bottom when the clock opened it. Settings' Notifications page has it as the Notification list row; shell-ui follows a change at once and closes an open list | M5.9l |

## Region: `[region]`

| Key | Value | From |
|---|---|---|
| `region.language` | the language shell-ui and Settings show their words in: a catalogue's name, such as `"de"` or `"pt_BR"` (`pt_BR` falls back to `pt`); missing is English, as is a word the catalogue lacks. The desktop's panels and menus show it at once, without starting again (M6.17); Settings takes it when next opened | M5.24a |
| `region.keyboard` | keyboard layouts as xkb names them: one, such as `"de"`, or up to four, such as `"us,ru"`, each with a variant if wanted, such as `"de(nodeadkeys)"`; missing is US. Check and set refuse a layout or variant xkeyboard-config lacks where its list is on the machine. The desktop follows it at once, and Super+Space (`shortcuts.next_keyboard_layout`) goes to the next layout | M5.21a |
| `region.timezone` | text, such as `"Europe/London"` | M6.6a |
| `region.time_servers` | the time servers the clock is set from, one to four host names, such as `["pool.ntp.org"]` or a cloud provider's own, `["169.254.169.123"]` (letters, digits, hyphens and dots; check and set refuse anything else); missing is `pool.ntp.org`. The `edel-clock` service of every machine image asks them once, in the background, each time the machine starts (busybox `ntpd`, up to three tries of 20 s, never holding the boot up), prints one line on the console and in the system log and writes the hardware clock; nothing keeps running afterwards. The next start uses a changed list. Its Settings row, "Time servers", comes with the Settings app's Region page, not built yet | M1.13 |
| `region.input_method` | the input method for typing languages a keyboard has no keys for: `fcitx5`, or `none`; missing runs none. With `fcitx5` the desktop starts it with the session, once it is on screen, and stops it at once when the key changes; it runs only where the input-method feature is in the image (its Settings row comes with the Settings app's Region page, not built yet) | M6.8 |

## Users: `[users.NAME]`

| Key | Value | From |
|---|---|---|
| `users.*.admin` | `true` or `false`: member of the `admin` group; missing is `false` | M2.2 |
| `users.*.ssh_keys` | list of public keys for `~/.ssh/authorized_keys`; missing leaves the file as it is | M2.2 |
| `users.*.login_shell` | a login shell's full path, such as `/bin/ash`, with no `:`; missing is `/bin/sh`. Apply leaves the shell as it is, and says so, when the path is not a program on the machine | M2.2 |

On a desktop (an image with the `seat` feature), apply also puts every person in `[users]` and the greeter's account in the `seat` group, so they may use the screen and input; there is no key for it (M4.2b). `reset users.NAME.admin` keeps `[users.NAME]`, so apply takes back what the key gave.

## Network: `[network]`

| Key | Value | From |
|---|---|---|
| `network.hostname` | the row "Device name (hostname)": letters, digits and hyphens, up to 63. Missing at the next apply removes the machine's own `/etc/hostname`, so the slot's shows again | M2.2 |

## Default apps: `[default_apps]`

| Key | Value | From |
|---|---|---|
| `default_apps.browser` | text, a desktop file id without `.desktop`, such as `org.mozilla.firefox`; `edel settings apply` writes it as the system's MIME defaults for web pages and `x-scheme-handler/http(s)`, and the `open_browser` key opens it | M6.10 |
| `default_apps.files` | text, a desktop file id without `.desktop`; the folders' MIME type `inode/directory` and the `open_files` key use it | M6.10 |
| `default_apps.editor` | text, a desktop file id without `.desktop`; plain text, `text/plain` | M6.10 |
| `default_apps.terminal` | text, a desktop file id without `.desktop`; there is no MIME type for a terminal; the `open_terminal` shortcut opens it | M6.10 |
| `default_apps.mail` | text, a desktop file id without `.desktop`; `x-scheme-handler/mailto` | M6.10 |

When the key is unset, or names an app that is not installed, the shortcuts open the first installed app of the role's category (FileManager, WebBrowser), and the terminal's falls back to `foot`.

## Startup, Power and Services

| Key | Value | From |
|---|---|---|
| `startup.apps` | list of desktop file ids without `.desktop`, such as `["org.mozilla.firefox"]`, each started once the desktop is on screen, in the order listed; an app not installed is logged and skipped; an empty list is none | M6.10 |
| `power.lid_close` | `suspend`, `lock`, `nothing` or `poweroff` | M7.8 |
| `power.lock_after_minutes` | a whole number of minutes without input before the screen locks; `0` never | M7.8 |
| `power.power_button` | `suspend`, `poweroff`, `ask` or `nothing` | M7.8 |
| `power.on_battery` | `power-saver`, `balanced` or `performance` | M7.8 |
| `services.*` | `true` or `false`: the feature `NAME` on or off, by the feature's name, never a daemon's, and only a feature marked `switchable` (ssh, bluetooth); `edel settings set` and `check` name the ones this machine has. Missing is the image's choice (on, unless the image definition lists it in `off`). `edel settings apply` adds or takes out the feature's services in their runlevels and starts or stops them at once; a service another feature that is on still lists (dbus) stays. At boot only the runlevels change, and the default runlevel then starts what it holds | M6.10 |

## Updates, Apps and Add-ons

| Key | Value | From |
|---|---|---|
| `updates.channel` | the channel whose release lists `edel update` takes, such as `"stable"` or `"preview"`: lowercase letters, digits and hyphens, up to 32; missing is the channel the image was built for. `edel update` with no release named takes this channel's list, and a list for another channel is refused unless `edel update --channel NAME` asks for it once. Settings' Updates page has it as the Channel row | M3.8, M5.8c |
| `updates.version` | text, a release to stay on; missing follows the channel | M3.4 |
| `updates.automatic` | `off`, `check`, `install` or `install-and-restart` | M7.5 |
| `updates.restart_window` | text, hours for unattended restarts, such as `"02:00-04:00"` | M7.5 |
| `apps.installed` | a list of Flatpak app ids from Flathub, such as `["io.mpv.Mpv"]` (an id is reverse-DNS: three or more parts joined by dots, the last with no `-`). For each admin person, apply adds Flathub if they have no remote and installs, with `flatpak --user` as that person, each id they lack; the boot leaves the installs to an apply, and says so. Taking an id out leaves its app installed, as the store (M6.4) removes apps. On the command line `edel settings set 'apps.installed=["io.mpv.Mpv"]'`, then `edel settings apply`; the file line is `apps.installed = ["io.mpv.Mpv"]` under `[apps]`. No Settings row yet: the store (M6.4) is this key's page | M6.1 |
| `apps.camera` | a table of Flatpak app ids and `allow`, `ask` or `never`, such as `{ "org.gnome.Snapshot" = "never" }` (on the command line `edel settings set 'apps.camera={ "org.gnome.Snapshot" = "never" }'`): whether the app may use the camera, is asked every time, or never may. The person's own, set in Settings' Apps page; shell-ui lays it onto the portals' permission store (table `devices`, id `camera`) when the session starts and whenever the file changes. An app not listed keeps the portal's own way: it asks the first time and keeps the answer. `edel settings set 'apps.camera={ "org.gnome.Snapshot" = "maybe" }'` is refused with "\"org.gnome.Snapshot\" has \"maybe\"; choose allow, ask or never" | M6.14a |
| `apps.microphone` | the same table with `allow` or `never`: whether the app may use the microphone; no portal asks, so WirePlumber hides the microphones from an app set to `never` (store table `devices`, id `microphone`, which it reads) | M6.14b |
| `apps.screenshots` | the same table with `allow` or `never`: whether the app may take a screenshot without asking (store table `screenshot`); `ask` is not offered, as the portal keeps the first answer | M6.14a |
| `apps.notifications` | the same table with `allow` or `never`: whether the app's notifications show (store table `notifications`) | M6.14a |
| `apps.files` | the same table with `allow` or `ask`: `ask` takes away the app's own reach into your files (its override file's `filesystems` line gains `!host`, `!home` and each home folder), so it sees only the files you pick in the file chooser, every time; `allow` keeps what it asked for when installed | M6.14a |
| `addons.installed` | list of add-on names (ADR-007) | M7.2b |

## System: `[system]`

| Key | Value | From |
|---|---|---|
| `system.variant` | `container`, `server`, `desktop` or `phone` | M7.3 |
| `system.developer_mode` | `true` or `false`: developer mode (ADR-007) | M2.2 |
| `system.profiles` | list of profile names, applied in order | M7.6 |

## Every key

<!-- written by key_help.rs: EDEL_WRITE_DOCS=1 cargo test -p edel key_help -->
### `appearance`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `appearance.theme` | The theme in use: a built-in one (edel, aurora, earth or ocean) or one of your own. | `"edel"` | release |
| `appearance.wallpaper` | The wallpaper behind the windows: a name such as hills, a picture's path, a colour or none. | `"hills"` | release |
| `appearance.wallpaper_dark` | The wallpaper for dark mode, when it differs from the light one. | the wallpaper's | release |
| `appearance.pictures` | The pictures of the theme in use, shown on top of its own; at most 32, none twice. | the theme's | release |
| `appearance.switcher_look` | How the workspace switcher looks: round numbers, or a button with a line for each workspace. | `"numbers"` | release |
| `appearance.switcher_shown` | How many workspace numbers the switcher shows at once, from 1 to 9. | `3` | release |
| `appearance.switcher_ends` | What the switcher's ends show when more workspaces lie beyond them: fade, arrows or counts. | `"arrows"` | release |
| `appearance.mode` | Light or dark colours for title bars, panels and apps; auto is the release's choice, light in this one. | `"auto"` | token |
| `appearance.contrast` | High contrast gives strong edges and plain colours in both schemes; normal is the usual look. | `"normal"` | release |
| `appearance.accent` | The accent colour for the shown workspace, buttons and marks: a named accent, wallpaper, or a colour. | `"blue"` | token |
| `appearance.colours.*` | One colour of the look, by its token name, in both schemes. | the token's | token |
| `appearance.colours.light.*` | One colour of the light scheme alone, over the colours both schemes share. | the token's | token |
| `appearance.colours.dark.*` | One colour of the dark scheme alone, over the colours both schemes share. | the token's | token |
| `appearance.radius` | The corner radius of windows, in logical pixels from 0 to 24; 0 makes every corner square. | the token size.radius_window | token |
| `appearance.gaps` | The gap between tiled windows, in logical pixels from 0 to 64. | the token size.gap | token |
| `appearance.gaps_outer` | The gap between tiled windows and the screen's edge, in logical pixels from 0 to 64. | the token size.gap_outer | token |
| `appearance.border_width` | The border round each window with a title bar, in logical pixels from 0 to 10. | the token size.border | token |
| `appearance.title_bar_height` | The height of each title bar, in logical pixels from 24 to 48. | the token size.title_bar | token |
| `appearance.panel_opacity` | How solid the panels are, in percent from 20 to 100. | the token size.panel_opacity | token |
| `appearance.window_opacity` | How solid the focused window is, in percent from 20 to 100. | the token size.window_opacity | token |
| `appearance.window_opacity_unfocused` | How solid every other window is, in percent from 20 to 100. | the token size.window_opacity_unfocused | token |
| `appearance.shadow_size` | How far the shadow under each window spreads, in logical pixels from 0 to 64; 0 is none. | the token size.shadow_blur | token |
| `appearance.shadow_darkness` | How dark the shadow under each window is, in percent from 0 to 100. | the alpha of colour.shadow | token |
| `appearance.blur` | How strongly the wallpaper blurs behind see-through panels, in percent; 0 shows it sharp. | the token size.blur | token |
| `appearance.font` | The font for the interface's text, by its family name. | `"Inter Variable"` | token |
| `appearance.monospace_font` | The font apps use for code and terminals, by its family name. | `"Noto Sans Mono"` | release |
| `appearance.text_size` | How big every text is, in percent of the release's size, from 80 to 150. | `100` | release |
| `appearance.cursor_size` | The pointer's height in logical pixels, from 16 to 96. | `24` | release |
| `appearance.cursor_theme` | The pointer's theme, by the folder name of an installed cursor theme; default is the drawn arrow. | `"default"` | release |
| `appearance.icon_theme` | The apps' icon theme, by the folder name of an installed icon theme; the panel and apps draw app icons from it, and the shell's own icons stay Edel's. | `"Edel"` | release |
| `appearance.icon_size` | How big the icons in the panel and the bar are, in percent of the release's, from 75 to 150. | `100` | release |
| `appearance.animations` | How much things move: full, reduced (fades only) or off. | `"full"` | release |
| `appearance.animation_speed` | How fast things move, in percent of the release's speed, from 25 to 400; 200 is twice as fast. | `100` | release |
| `appearance.animation_curve` | How animations feel: smooth, snappy (quicker) or springy (windows and pop-ups overshoot a little). | `"smooth"` | release |
| `appearance.widgets` | The widgets on the desktop, over the wallpaper and under the windows, and where each one stands, as a list of tables. | none | release |
| `themes.*` | One of your own themes: its title, its pictures and the appearance values it sets. | none | release |

### `layout`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `layout.preset` | The layout preset that arranges the panel, windows and workspaces, such as classic, mac-like or zen. | `"classic"` | release |
| `layout.tiling` | Whether windows tile across the screen (true) or float freely (false); the preset decides when this is missing. | the preset's | preset |
| `layout.title_bars` | Where windows get a title bar: always, only on floating windows, or never (Zen has none). | the preset's | preset |
| `layout.device_type` | What kind of device this is: a desktop, a tablet or a phone; this release does not act on it yet. | detected | hardware |
| `layout.window_buttons` | The side of each title bar where close, minimize and maximize sit: left or right. | the preset's | preset |
| `layout.close_button` | Whether each title bar shows its close button; Super+Q closes a window whether or not it shows. | `true` | release |
| `layout.minimize_button` | Whether each title bar shows its minimize button. | `true` | release |
| `layout.maximize_button` | Whether each title bar shows its maximize button; a hidden button's room goes to the title. | `true` | release |
| `layout.tiling_style` | How tiling lays windows out: stack, split (as Hyprland tiles) or scroll (as niri tiles). | the preset's | preset |
| `presets.*` | One of your own layout presets: the built-in preset it starts from and the layout keys it sets. | none | release |

### `panels`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `panels.list` | The panels along the screen's edges, each with its widgets, in place of the preset's. | the preset's | preset |
| `panels.tray` | The tray apps kept in the panel, by their tray id; the other apps wait behind the tray's arrow. | `[]` | release |
| `panels.pinned` | The apps the apps widget pins, in order, by app id or role such as browser. | the preset's | preset |

### `workspaces`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `workspaces.count` | How many workspaces there are, from 1 to 9, one for each of Super+1 to Super+9. | the preset's | preset |
| `workspaces.dynamic` | Whether empty workspaces come and go, so the count follows the windows. | `false` | release |
| `workspaces.per_screen` | Whether each screen shows its own workspace, so Super+1 to Super+9 switch only the screen under the pointer. | `false` | release |
| `workspaces.names` | The names of the workspaces, the first one's first; an empty text leaves a workspace as its number. | `[]` | release |
| `workspaces.wallpapers` | A wallpaper for each workspace, the first one's first; an empty text uses Appearance's wallpaper. | `[]` | release |
| `workspaces.apps` | The apps that always open on one workspace, by app id and number, such as Firefox on 2. | `{}` | release |
| `workspaces.overview_strip` | The side of the screen where the workspace overview's strip lies: left, right, top or bottom. | `"left"` | release |

### `lock_screen`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `lock_screen.widgets` | The widgets on the lock and login screens and where each one stands, as a list of tables. | the look's | release |
| `lock_screen.look` | The look of the lock and login screens: classic, magazine, minimal or one of your own. | `"classic"` | release |
| `lock_screen.wallpaper` | What lies behind the lock screen's widgets: the backdrop, the desktop wallpaper blurred or sharp, a colour or a picture. | the look's | release |
| `lock_screen.dim` | How much the lock screen's wallpaper is darkened, in percent from 0 to 80, so the words read. | 30, or 40 for a sharp wallpaper or picture | release |
| `lock_screen.notifications` | Whether the lock screen shows how many notifications are unread: count, or none to hide it. | `"count"` | release |
| `lock_screen.looks` | Your own lock screen looks, each with its widgets and wallpaper, chosen by name. | none | release |

### `network`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `network.hostname` | The name this computer has on the network: letters, digits and hyphens, up to 63. | `"edel"` | image |

### `bluetooth`

This page has no key in the file.

### `displays`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `displays.*.position` | Where this screen sits among the others, as [x, y] in whole numbers. | each screen's own | hardware |
| `displays.*.scale` | How big things are on this screen, such as 1.25 for 125 percent. | each screen's own | hardware |
| `displays.*.resolution` | The resolution of this screen as WIDTHxHEIGHT, such as 1920x1080. | each screen's own | hardware |
| `displays.*.refresh_rate` | How many times a second this screen redraws, such as 60. | the resolution's best | hardware |
| `displays.*.enabled` | Whether this screen is on; a screen that is off shows nothing. | `true` | hardware |
| `displays.*.rotation` | How far this screen is turned: 0, 90, 180 or 270 degrees; this release does not act on it yet. | each screen's own | hardware |

### `sound`

This page has no key in the file.

### `power`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `power.lid_close` | What closing the lid does: suspend (locking first), lock, nothing or power off; nothing happens while two or more screens are lit. | suspend | release |
| `power.lock_after_minutes` | How many minutes without input before the screen locks, 0 for never. | `5` | release |
| `power.power_button` | What the power button does: suspend (the screen locks first), power off, ask (the screen locks and its power menu opens) or nothing. | suspend | release |
| `power.on_battery` | How hard the machine works on battery: power-saver, balanced or performance; plugged in, it is balanced, or performance when chosen here. | balanced | release |
| `power.game_mode` | When a fullscreen game gets variable refresh: plugged-in (or with no battery), always or off; Super+G turns it on or off until you log out. | plugged-in | release |

### `notifications`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `notifications.do_not_disturb` | Whether banners stay away; notifications still arrive and are listed in the notification centre. | `false` | release |
| `notifications.position` | Where banners appear: by the status area, in a corner, or at the top's middle. | `"auto"` | release |
| `notifications.badge` | What the notifications button shows while some are unread: a dot, the count, or nothing. | `"dot"` | release |
| `notifications.list` | Where the notification list opens: hung on its button, or as a sheet on the left or right. | `"button"` | release |

### `shortcuts`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `shortcuts.*` | The keys for one action, such as Super+X, or empty for none; the Shortcuts page lists the actions. | each action's own keys, docs/SHORTCUTS.md | release |
| `clipboard.history` | Keeps the last texts and pictures you copied, in memory only, for Super+V; false keeps nothing. | `true` | release |

### `region`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `region.language` | The language the desktop's own words and Settings are shown in, such as de or pt_BR. | English | release |
| `region.keyboard` | The keyboard layouts, up to four, such as us or us,ru; Super+Space goes to the next one. | `"us"` | release |
| `region.timezone` | The time zone the clock shows, such as Europe/London. | `"UTC"` | release |
| `region.time_servers` | The servers the clock is set from, one to four host names, such as pool.ntp.org. | `["pool.ntp.org"]` | release |
| `region.input_method` | The input method for typing languages a keyboard has no keys for: fcitx5 or none. | `"none"` | release |

### `default_apps`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `default_apps.browser` | The app that opens web pages and web links, by its desktop file id without .desktop. | the first installed browser | image |
| `default_apps.files` | The app that opens folders, by its desktop file id without .desktop. | the first installed file manager | image |
| `default_apps.editor` | The app that opens plain text files, by its desktop file id without .desktop. | the first installed editor | image |
| `default_apps.terminal` | The app the terminal shortcut opens, by its desktop file id without .desktop. | the first installed terminal, else foot | image |
| `default_apps.mail` | The app that opens mail and mail links, by its desktop file id without .desktop. | the first installed mail app | image |

### `startup`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `startup.apps` | The apps that start once the desktop is on screen, in the order listed, by desktop file id. | `[]` | release |

### `apps`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `apps.installed` | The Flathub apps installed for each administrator, by app id such as io.mpv.Mpv. | `[]` | release |
| `apps.camera` | Which apps may use the camera, by Flatpak app id: allow, ask every time or never. | `{}` | release |
| `apps.microphone` | Which apps may use the microphone, by Flatpak app id: allow or never. | `{}` | release |
| `apps.screenshots` | Which apps may take a screenshot without asking, by Flatpak app id: allow or never. | `{}` | release |
| `apps.notifications` | Which apps may show notifications, by Flatpak app id: allow or never. | `{}` | release |
| `apps.files` | Which apps reach your files by themselves (allow) or only the ones you pick (ask), by Flatpak app id. | `{}` | release |
| `apps.location` | Which apps may know where this computer is, by Flatpak app id: allow, ask every time or never. | `{}` | release |

### `location`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `location.enabled` | Off gives every app nothing, whatever each app may use. | `true` | release |
| `location.place` | The city apps are told, named by its time zone, such as Asia/Tashkent. | the time zone's city | release |
| `location.online` | Find the place from nearby Wi-Fi networks through BeaconDB, a community service, which is sent their hardware addresses. | `false` | release |

### `addons`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `addons.installed` | The add-ons this machine has, by name; one added is downloaded by edel settings apply and starts at the next restart, and each is in edel addon list. | `[]` | release |

### `services`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `services.*` | Turns one switchable feature, such as ssh or bluetooth, on or off by its name. | the image's | image |

### `users`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `users.*.admin` | Whether this person is an administrator, who may change the whole machine. | `false` | release |
| `users.*.ssh_keys` | The public keys that may log in to this person's account over ssh, one line each. | the file as it is | release |
| `users.*.login_shell` | The program this person's login starts, given as a full path such as /bin/ash. | `"/bin/sh"` | release |

### `updates`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `updates.channel` | The release channel whose updates this machine takes, such as stable or preview. | the image's | image |
| `updates.version` | A release to stay on instead of following the channel; this release does not act on it yet. | follows the channel | release |
| `updates.automatic` | Whether updates are checked, installed, or installed and restarted by themselves, or off; this release does not act on it yet. | not set yet | release |
| `updates.restart_window` | The hours when a restart may happen by itself, such as 02:00-04:00; this release does not act on it yet. | none | release |

### `system`

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `system.variant` | What kind of system this is: a container, a server, a desktop or a phone; this release does not act on it yet. | the image's | image |
| `system.developer_mode` | Developer mode, which lets people change the system's own parts, such as its kernel; off unless a person turns it on. | `false` | release |
| `system.profiles` | Ready-made sets of apps and add-ons for one kind of work, such as gaming; a value this file sets wins over a profile's. | `[]` | release |

### Other keys

| Key | What it does | Default | Decided by |
|---|---|---|---|
| `install.disk` | The disk a stick installs itself on: auto, the one fixed disk large enough, or a /dev/disk/by-id/ name. | `"auto"` | release |
| `install.erase` | Whether an unattended install erases only an empty disk, or any disk after a ten second countdown. | `"empty-only"` | release |
| `install.then` | What the machine does once a stick has installed itself: power off or restart. | `"poweroff"` | release |

<!-- end of the keys -->

## Adding a key

Name it after its row in Settings: the label in lowercase with `_` for spaces, its section the page's name, its values the options' labels in lowercase with `-` for spaces (ADR-008's same names decision). Add it to `KEYS` and the structs in `settings.rs`, append it to `crates/edel/tests/keys.txt`, add its row here, and set `supported` in the step that acts on it. A key is never removed or renamed within a format (ADR-008). A new key also gets its line in `crates/edel/src/key_help.rs`, which writes the list above and `docs/defaults.md`.
