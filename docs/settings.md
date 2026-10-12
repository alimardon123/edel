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
| `layout.window_buttons` | `left` or `right`: the side of the title bars where close, minimize and maximize sit, close outermost; missing is the preset's (right in Classic and Hive) | M5.4b |
| `layout.close_button` | `true` or `false`: whether every title bar shows its close button; missing shows it. Super+Q closes a window whatever is shown | M5.18a |
| `layout.minimize_button` | `true` or `false`: whether every title bar shows its minimize button; missing shows it | M5.18a |
| `layout.maximize_button` | `true` or `false`: whether every title bar shows its maximize button; missing shows it. A hidden button's room goes to the title | M5.18a |

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
| `appearance.mode` | `light`, `dark` or `auto`: the colours of the title bars, the panels and, through the settings portal, the apps; `auto` is the release's choice, dark in this one, and missing is `auto`. A change takes effect at once | M5.5c |
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
| `appearance.animation_curve` | `smooth`, `snappy` (quicker, settles sooner) or `springy` (overshoots a little, then rests); missing is `smooth`. One curve for every animation, windows and popups alike. The desktop follows it at once | M5.12b4 |
| `appearance.switcher_look` | `numbers` or `button`: `numbers` shows round buttons for the workspaces around the shown one, numbered as Super+1 to Super+9 are, the shown one a wider pill, with a workspace's name in its pill (the default); `button` shows the word Workspaces over an underline split into one segment per workspace, the shown one in the accent, and a click on a segment shows its workspace, a scroll the next or previous one. Missing is `numbers`. `edel settings set appearance.switcher_look=button` switches the look at once | M5.2m |
| `appearance.switcher_shown` | a whole number from 1 to 9: how many numbers the `numbers` look shows at once, the shown workspace and its neighbours; missing is 3. `edel settings set appearance.switcher_shown=10` is refused with "expected a whole number from 1 to 9, not 10" | M5.2m |
| `appearance.switcher_ends` | `fade`, `arrows` or `counts`: where more workspaces lie than show, an arrow at each end, dark where more lie that way and faint where none do (`arrows`), the next one peeking in, faded (`fade`), or with how many lie beyond it, as `+2` (`counts`); a click on an end shows the view one step towards it. Missing is `arrows`. `edel settings set appearance.switcher_ends=dots` is refused with "unknown value \"dots\"; use fade, arrows or counts" | M5.2m |

## Lock screen: `[lock_screen]`

The lock and login screens are made of parts (M5.10c5), each placed by an anchor and an offset, so one arrangement fits a phone, a laptop and a 4K screen. Missing is the classic arrangement, the mockup's.

| Key | Value | From |
|---|---|---|
| `lock_screen.parts` | a list of parts, each a table: `part` (`clock`, `date`, `count`, `player`, `person`, `message`, `picture` or `battery`), `at` (where it stands: `top_left`, `top`, `top_right`, `left`, `centre`, `right`, `bottom_left`, `bottom` or `bottom_right`), and optionally `x` and `y` (an offset, a share of the screen's width and height, from -1 to 1), `text` (a message's words, 1 to 120 characters on one line) and `path` (a picture's file, an absolute path ending `.png`, `.jpg` or `.jpeg`). The person stands exactly once, so nobody is locked out; each other part at most once, except the clock, of which up to 3 stand, and at most 3 messages and 3 pictures. Parts at one anchor stack, one below the other. A clock also takes `face` (`digital`, `stacked`, `analog` or `minimal`; absent is `digital`), `hours` (12 or 24; absent is 24, and 12 writes `2:06`, with no AM or PM), `size` (`small`, `medium` or `large`; absent is `large`, the token's size, `medium` two thirds of it and `small` a half) and `weight` (100 to 900 in steps of 100; absent is the token `font.lock_clock_weight`), `zone` (an IANA time zone's name, such as `Asia/Tokyo`, `Europe/London` or `UTC`; absent is this machine's time, and a zone the machine has no zone files for shows this machine's time too) and `label` (1 to 24 characters on one line, drawn under the clock; absent with a zone is the zone's city, so `Asia/Tokyo` reads `Tokyo`). Each clock has its own options, so two clocks can show two zones: `{ part = "clock", at = "top_right", face = "analog", size = "small", zone = "Asia/Tokyo" }` beside the classic clock. A date takes `show` (a list of `weekday`, `day`, `month` and `year`, each at most once, shown in that order whatever order is written; absent is weekday, day and month) and `style` (`long`, `short` or `numeric`; absent is `long`, as `Saturday 3 October 2026`, `Sat 3 Oct 2026` or `Sat 03.10.2026`). Each option is refused on another part. An example: `[[lock_screen.parts]]` tables with `part = "clock"`, `at = "centre"`, `face = "analog"`, `hours = 12` and `part = "date"`, `at = "top"`, `style = "short"`; on the command line `edel settings set 'lock_screen.parts=[{ part = "clock", at = "top", face = "stacked", hours = 12, size = "medium" }, { part = "date", at = "top", show = ["weekday", "day"], style = "short" }, { part = "person", at = "centre" }]'`. On the command line `edel settings set 'lock_screen.parts=[{ part = "person", at = "centre" }]'`; `edel settings set lock_screen.parts=[{ part = "person", at = "top" }]` is refused with "the lock screen needs the person and their password field" | M5.10c5, M5.10c5b |
| `lock_screen.look` | a look's name: `classic` (the mockup's, the default), `magazine` (a large clock at the left with the date under it and the person at the right), `minimal` (the time alone), or one of your own in `looks`. A name that is neither is refused, and the lock screen falls back to classic if the file is read leniently. Set with `edel settings set lock_screen.look=minimal` | M5.10c5c |
| `lock_screen.background` | what lies behind the parts: `backdrop` (the navy scene, classic's default), `blurred` (the wallpaper that applies, blurred for glass, every other look's default), `sharp` (the wallpaper, sharp), a colour such as `"#1f2b48"`, or a picture's full path such as `"/home/you/lock.png"` (ending `.png`, `.jpg` or `.jpeg`). Missing is the look's. Set with `edel settings set 'lock_screen.background="#1f2b48"'`; `edel settings set lock_screen.background=lock.png` is refused, as a picture needs its full path | M5.10c5c |
| `lock_screen.dim` | how much the background is darkened, in percent, from 0 to 80, so the white words read. Missing is the look's own: 30 for a blurred or colour background and 40 for a sharp one or a picture (the built-in amounts, `DIM` and `DIM_SHARP` in `edel::lock_screen`). Set with `edel settings set lock_screen.dim=40`; a value of 81 is refused. The Dim slider on the Lock screen page, and in the lock editor's wallpaper panel, set it | M5.10c7g |
| `lock_screen.notifications` | `count`, which shows how many notifications are unread, or `none`, which hides the count. Missing is `count`. `edel settings set lock_screen.notifications=some` is refused with the values it takes | M5.10c5c |
| `lock_screen.looks` | the person's own looks, each a table named by the look (`[lock_screen.looks.home]`) with `parts`, a list of parts checked as `lock_screen.parts` is (so each has the person), and `background`, as `lock_screen.background` takes it (absent is the wallpaper, blurred); choosing the look brings both, and `lock_screen.parts` and `lock_screen.background` set beside it are changes over it. A name is letters, digits and hyphens and not a built-in look's, and it is chosen with `lock_screen.look`. Made by Save as on the Lock screen page. An example: `[lock_screen.looks.home]` with `parts = [{ part = "clock", at = "top", y = 0.1 }, { part = "person", at = "centre" }]`, then `look = "home"` under `[lock_screen]`; on the command line `edel settings set 'lock_screen.looks={ home = { parts = [{ part = "person", at = "centre" }] } }'`; a look without the person is refused | M5.10c5c |
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
| `region.language` | the language shell-ui and Settings show their words in: a catalogue's name, such as `"de"` or `"pt_BR"` (`pt_BR` falls back to `pt`); missing is English, as is a word the catalogue lacks. The desktop restarts shell-ui in it at once; Settings takes it when next opened | M5.24a |
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

## Default apps, Startup, Power and Services

| Key | Value | From |
|---|---|---|
| `default_apps.browser` | text, a `.desktop` id | M6.10 |
| `default_apps.files` | text, a `.desktop` id | M6.10 |
| `default_apps.editor` | text, a `.desktop` id | M6.10 |
| `default_apps.terminal` | text, a `.desktop` id | M6.10 |
| `default_apps.mail` | text, a `.desktop` id | M6.10 |
| `startup.apps` | list of `.desktop` ids started with the session | M6.10 |
| `power.lid_close` | `suspend`, `lock`, `nothing` or `poweroff` | M7.8 |
| `power.lock_after_minutes` | a whole number of minutes without input before the screen locks; `0` never | M7.8 |
| `power.power_button` | `suspend`, `poweroff`, `ask` or `nothing` | M7.8 |
| `power.on_battery` | `power-saver`, `balanced` or `performance` | M7.8 |
| `services.*` | `true` or `false`: the switchable feature `NAME` on or off | M6.10 |

## Updates, Apps and Add-ons

| Key | Value | From |
|---|---|---|
| `updates.channel` | the channel whose release lists `edel update` takes, such as `"stable"` or `"preview"`: lowercase letters, digits and hyphens, up to 32; missing is the channel the image was built for. `edel update` with no release named takes this channel's list, and a list for another channel is refused unless `edel update --channel NAME` asks for it once. Settings' Updates page has it as the Channel row | M3.8, M5.8c |
| `updates.version` | text, a release to stay on; missing follows the channel | M3.4 |
| `updates.automatic` | `off`, `check`, `install` or `install-and-restart` | M7.5 |
| `updates.restart_window` | text, hours for unattended restarts, such as `"02:00-04:00"` | M7.5 |
| `apps.installed` | a list of Flatpak app ids from Flathub, such as `["io.mpv.Mpv"]` (an id is reverse-DNS: three or more parts joined by dots, the last with no `-`). For each admin person, apply adds Flathub if they have no remote and installs, with `flatpak --user` as that person, each id they lack; the boot leaves the installs to an apply, and says so. Taking an id out leaves its app installed, as the store (M6.4) removes apps. On the command line `edel settings set 'apps.installed=["io.mpv.Mpv"]'`, then `edel settings apply`; the file line is `apps.installed = ["io.mpv.Mpv"]` under `[apps]`. No Settings row yet: the store (M6.4) is this key's page | M6.1 |
| `addons.installed` | list of add-on names (ADR-007) | M7.2b |

## System: `[system]`

| Key | Value | From |
|---|---|---|
| `system.variant` | `container`, `server`, `desktop` or `phone` | M7.3 |
| `system.developer_mode` | `true` or `false`: developer mode (ADR-007) | M2.2 |
| `system.profiles` | list of profile names, applied in order | M7.6 |

## Adding a key

Name it after its row in Settings: the label in lowercase with `_` for spaces, its section the page's name, its values the options' labels in lowercase with `-` for spaces (ADR-008's same names decision). Add it to `KEYS` and the structs in `settings.rs`, append it to `crates/edel/tests/keys.txt`, add its row here, and set `supported` in the step that acts on it. A key is never removed or renamed within a format (ADR-008).
