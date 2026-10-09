# Mockups of the target look

Pictures of where the Edel shell's look is heading. They are not
screenshots and not binding: the real values are design tokens
(`design/tokens.toml`, M5.5), and a person can change every one of them
(M5.12). This is the third round, from 2026-10-03, after two reviews by
Alimardon: a little sharper, calm and polished rather than playful, and
welcoming to everyone by default; the menu button's four squares, round
workspace buttons on the right, one floating or tiling button, thinner
title bars and app icons drawn as objects came with the second review.
The fourth round, on 2026-10-08, added the status area, quick settings,
the calendar and notifications for M5.9, as Alimardon asked.

| Picture | What it shows |
|---|---|
| [classic.jpg](classic.jpg) | Classic, the default, light: a panel along the bottom with the menu and the windows, then the workspace buttons, the floating or tiling button, status and clock |
| [classic-dark.jpg](classic-dark.jpg) | The same in the dark scheme |
| [classic-menus.jpg](classic-menus.jpg) | The launcher, quick settings and a notification, over the Settings app's Layout page |
| [quick-settings.jpg](quick-settings.jpg) | The status area in full (tray arrow, keyboard layout, one pill of status icons, the clock) and the quick settings above it, with the tray overflow open beside it |
| [quick-settings-dark.jpg](quick-settings-dark.jpg) | The clock's popup in the dark scheme: the calendar under the notification centre, and the volume overlay |
| [mac-like.jpg](mac-like.jpg) | Mac-like: a bar on top, a dock, window buttons on the left |
| [windows-like.jpg](windows-like.jpg) | Windows-like, dark: a taskbar with search and centred apps |
| [tiling.jpg](tiling.jpg) | Tiling, dark: gaps, and a title bar with its close button on every tile |
| [tablet.jpg](tablet.jpg) | Tablet: windows tile by default, with larger targets |
| [tiers.jpg](tiers.jpg) | The effect tiers Full, Balanced and Lite side by side |
| [phone.jpg](phone.jpg) | Phone: Settings, and the card switcher |
| [phone-quick.jpg](phone-quick.jpg) | Phone: quick settings pulled down at touch size, and notifications on the lock screen |

What the look is made of, each a token:

- corners a little rounded, never pill-shaped: windows 10 px, menus 12 px,
  buttons and fields 7 px, square on the Lite tier;
- soft layered shadows from one light above, and hairline edges;
- one calm blue accent, light and dark both first-class;
- Inter in sentence case, tabular figures for the clock;
- the workspace switcher: round buttons, the shown workspace a wider
  accent pill, at most three at once (the shown one and its neighbours),
  the rest a scroll away behind a faded edge, sliding as it scrolls;
- one floating or tiling button, its icon changing and its background
  filling when the workspace tiles;
- title bars 28 px high, as the compositor draws them;
- app icons drawn as objects, a folder, a globe, a gear, each lit from
  above, never a glyph on a coloured tile;
- frosted panels and menus on Full, the wallpaper blurred once on
  Balanced, solid colours on Lite.

## Fifth round (2026-10-09): the defaults to build toward

Alimardon reviewed quick settings, notifications, the tray and the
workspace switcher over several days of comments on a design canvas and
on 2026-10-09 called the result "my default current mockups to go
towards". The pictures in [shell/](shell/) are those defaults. They
replace the fourth round's quick settings and notifications where the
two differ. They are still defaults, not limits: every tile, shelf,
place and size below is a setting a person can change in Settings, with
`edel settings` and in the settings file, and later rounds (the design
review of M8.18 first) may revisit them for better taste.

| Picture | What it shows |
|---|---|
| [shell/laptop.jpg](shell/laptop.jpg) | Classic on a laptop: quick settings over the status area, the player its own card above it, brightness and volume on the bottom shelf, edit, Settings and power in the footer beside the battery |
| [shell/tablet.jpg](shell/tablet.jpg) | Tablet: tiled under a top bar; quick settings at touch size in the top right corner, the player below it |
| [shell/phone.jpg](shell/phone.jpg) | Phone: quick settings pulled down, the player below; the volume pop-up beside the volume keys |
| [shell/notifications-laptop.jpg](shell/notifications-laptop.jpg) | A click on the clock: the notifications card, with Do not disturb as a moon button in its title row and Clear all, the calendar card below |
| [shell/notifications-phone.jpg](shell/notifications-phone.jpg) | Phone: a swipe down from the top left opens a full shade, with no card and no calendar, the battery shown |
| [shell/notifications-tablet.jpg](shell/notifications-tablet.jpg) | Tablet: the same shade over the dimmed screen |
| [shell/banners.jpg](shell/banners.jpg) | New notifications stack by the status area, newest at the bottom, each with a close button on hover, gone after 5 s |
| [shell/details.jpg](shell/details.jpg) | Going deeper: a pill's chevron opens Wi-Fi or Sound in place of the grid |
| [shell/states.jpg](shell/states.jpg) | Hover, pressed, on and off, focus, and how each part moves |
| [shell/tray.jpg](shell/tray.jpg) | The tray: one arrow, as on Windows, opening a frosted grid |
| [shell/settings.jpg](shell/settings.jpg) | Every setting these pictures need: its choices, its default, its proposed key, and whether it is decided or still the person's to pick |
| [shell/options.jpg](shell/options.jpg) | The other layouts a person can switch to: the shelf at the top, both shelves, the player inside the grid, the sliders standing in it |

The rules under them:

- **One square grid.** Every cell is a square and every gap the same,
  across and down; a round toggle fills a cell with its title in the gap
  below; a pill (Wi-Fi, the network) is two cells merged; every radius
  is half a cell. At most four columns on every device; on a laptop
  64 px toggles, two rows a page, more pages by swiping.
- **Shelves stay put.** A hairline, then items that do not move while
  the grid's pages swipe: brightness and volume lying full width on the
  bottom shelf by default. A shelf can be at the top, the bottom or both,
  and can hold sliders, the player or any tile at any width.
- **The player is its own card** on the side away from the edge quick
  settings hangs from: above it on a laptop with a bottom panel, below it
  on a tablet or phone.
- **Edit, Settings and power** sit on the anchored edge: the footer on a
  bottom panel, the header's second line when the sheet hangs from the
  top. A chevron on a pill opens its details in place.
- **Notifications** open from the clock on a laptop, apart from the
  calendar card below; on a phone or tablet from a swipe down at the top
  left, as a full shade. Do not disturb is a moon button that fills with
  the accent when on, and a toggle in quick settings.
- **Title bars** in the pictures are the compositor's own: 28 px, square,
  a 1 px edge, buttons flush right, as `crates/compositor/src/frame.rs`
  draws them.
- **Light on every machine.** Blur and shadows are the Full tier's;
  Balanced and Lite draw the same shapes in solid colours, so the look
  costs no frames on old hardware.

The canvas these came from is Alimardon's design artifact (private). The
pictures here are the copy every session and agent reads.

`edel-mockups.html` draws them all; `node render.js` writes the pictures
(Playwright with Chromium, and Inter installed for the text).
