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
the calendar and notifications for M5.9, as Alimardon asked; the fifth
round below replaced all of it but the status area, and on 2026-10-10
the fourth round's pictures of quick settings, notifications and the
phone's lock screen were deleted, so no session builds from them.

**One rule for every round:** a picture that a newer round replaces is
deleted in the same pull request (git keeps it), never kept beside the
new one. What is in this folder is what to build toward.

| Picture | What it shows |
|---|---|
| [classic.jpg](classic.jpg) | Classic, the default, light: a panel along the bottom with the menu and the windows, then the workspace buttons, the floating or tiling button, status and clock |
| [classic-dark.jpg](classic-dark.jpg) | The same in the dark scheme |
| [classic-menus.jpg](classic-menus.jpg) | The launcher over the Settings app's Layout page |
| [mac-like.jpg](mac-like.jpg) | Mac-like: a bar on top, a dock, window buttons on the left |
| [windows-like.jpg](windows-like.jpg) | Windows-like, dark: a taskbar with search and centred apps |
| [tiling.jpg](tiling.jpg) | Tiling, dark: gaps, and a title bar with its close button on every tile |
| [tablet.jpg](tablet.jpg) | Tablet: windows tile by default, with larger targets |
| [tiers.jpg](tiers.jpg) | The effect tiers Full, Balanced and Lite side by side |
| [phone.jpg](phone.jpg) | Phone: Settings, and the card switcher |

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
- frosted panels and menus from the wallpaper blurred once, on every
  tier, and live blur behind short-lived surfaces on Full only
  (ADR-002's glass decision).

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
| [shell/tablet.jpg](shell/tablet.jpg) | Tablet: tiled under a top bar; quick settings at touch size in the top right corner, with the phone's header, the player below it |
| [shell/phone.jpg](shell/phone.jpg) | Phone: quick settings pulled down, its header a large clock with the battery at the right, then the date with the year and the edit, Settings and power buttons (Alimardon, 2026-10-10), the player below; the volume pop-up beside the volume keys |
| [shell/notifications-laptop.jpg](shell/notifications-laptop.jpg) | A click on the clock: the notifications card, with Do not disturb as a moon button in its title row and Clear all, the calendar card below |
| [shell/notifications-phone.jpg](shell/notifications-phone.jpg) | Phone: a swipe down from the top left opens a full shade, with no card and no calendar, the battery shown |
| [shell/notifications-tablet.jpg](shell/notifications-tablet.jpg) | Tablet: the same shade over the dimmed screen |
| [shell/banners.jpg](shell/banners.jpg) | New notifications stack by the status area, newest at the bottom, each with a close button on hover, gone after 5 s |
| [shell/details.jpg](shell/details.jpg) | Going deeper: a pill's chevron opens Wi-Fi or Sound in place of the grid |
| [shell/states.jpg](shell/states.jpg) | Hover, pressed, on and off, focus, and how each part moves |
| [shell/tray.jpg](shell/tray.jpg) | The tray: one arrow, as on Windows, opening a frosted grid |
| [shell/settings.jpg](shell/settings.jpg) | Every setting these pictures need: its choices, its default and its proposed key; Alimardon's decisions, and four defaults Claude picked by the principles (how many workspaces show, the ends of their strip, the volume pop-up on, new tray apps behind the arrow), which they can overrule |
| [shell/standing-slider.jpg](shell/standing-slider.jpg) | How Sound is reached from a standing volume slider: six ways compared, and the one chosen, no button, a right click or a still hold |
| [shell/player.jpg](shell/player.jpg) | The player card (M5.9i, chosen by Alimardon on 2026-10-10 as option 2A): a pill, its app's icon and name small at the top centre; with several players each is a page, switched by scrolling sideways, a swipe or a click on tiny dots under the controls, the shown dot a short accent pill |
| [shell/overview.jpg](shell/overview.jpg) | The overview, Super+W (M5.2j-b, approved by Alimardon on 2026-10-10): every workspace small in a strip on a frosted tray, a frame to add one, search, and the shown workspace's windows large and labelled; a workspace dragged along the strip reorders them; the strip down the left by default, or on any side with `workspaces.overview_strip`; a click on the switcher's lit workspace opens it too |
| [shell/lock.jpg](shell/lock.jpg) | The lock screen (M5.10c, proposed 2026-10-10): the wallpaper blurred and dimmed, the date over a large light clock, how many notifications came and from which apps, never their words, the person and a frosted password field with a Caps Lock warning; in the bottom right corner the keyboard layout, accessibility and power, its menu open: Suspend, Restart (an update waits) and Shut down |
| [shell/lock-media.jpg](shell/lock-media.jpg) | The lock screen while music plays (M5.10c): the player as a frosted card in the bottom left corner, across from the buttons, with the cover, the time played and the controls |
| [shell/lock-media-options.jpg](shell/lock-media-options.jpg) | Five places for the lock screen's player (2026-10-10): Alimardon chose the bottom left corner (2, `lock-media.jpg`) as the default for now, shown only while something plays; every place stays a choice in Settings, and a later round may redesign it |
| [shell/greeter.jpg](shell/greeter.jpg) | The login screen (M5.10c): the same design, the people of the machine to pick and Other to type a name |
| [shell/greeter-sessions.jpg](shell/greeter-sessions.jpg) | The login screen with a second session installed (M5.10c, Alimardon 2026-10-10): a chip in the corner names the session and opens the frosted "Desktop for this login" menu; Console stands for any session an add-on or developer mode adds, and with one session the chip is not there |
| [shell/lock-phone.jpg](shell/lock-phone.jpg) | A phone's lock screen (M9): the player under the clock, then the notifications as frosted cards saying only what arrived, the torch and camera; then the passcode on round frosted keys |
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
  on a tablet or phone. It is a pill, its corners half its height; its
  app's icon and name sit small at its top centre, and with several
  players each is a page of the same card, under tiny dots that show only
  then (Alimardon, 2026-10-10, `shell/player.jpg`).
- **Edit, Settings and power** sit on the anchored edge: the footer on a
  bottom panel; when the sheet hangs from the top (phone, tablet), the
  header, as 36 px rounded squares on the date's row, under a large
  clock with the battery at its right (Alimardon, 2026-10-10). A chevron on a pill opens its
  details in place.
- **A slider is only a slider.** One lying on a shelf has its chevron
  beside it; one standing in the grid has no button at all: a right
  click on a laptop, or a still hold of half a second on touch, opens
  Sound (Displays for brightness). The slider follows only a moving
  finger, so a hold is never mistaken for a drag (Alimardon,
  2026-10-09).
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
pictures here are the copy every session and agent reads, and their
boards, as HTML to change, are in `docs/mockups/shell/canvas/`; `canvas/player.html` draws `player.jpg`, `canvas/overview.html` `overview.jpg` and `canvas/lock.html` `lock.jpg`, `lock-media.jpg`, `greeter.jpg`, `greeter-sessions.jpg` and `lock-phone.jpg`, and `canvas/lock-media-options.html` the five boards of `lock-media-options.jpg` (put side by side with ImageMagick's `montage`) (`node render.js lock.html` there) (the same Playwright and Chromium, at twice the size, as JPEG).

## Settings (2026-10-11): Appearance with themes

| Picture | What it shows |
|---|---|
| [settings/appearance.jpg](settings/appearance.jpg) | The Appearance page with themes (M5.12b6, approved by Alimardon on 2026-10-11): the theme in use on one line, its pictures and what changed, with Reset, Keep as new and a More menu; under it three equal pictures of the theme, Light, Dark and Automatic, which are the style; then the themes in one row, each its light and dark side split diagonally with a Built in badge, New last; then Accent, Text, Shape and Motion, open as before; the page's icon a paint palette |

Its board is `settings/canvas/appearance.html`, plain HTML drawn from the design canvas board "Appearance with themes"; the canvas is Alimardon's private design artifact, and this is the copy every session reads.

## Settings (2026-10-11): Power, Updates, Users and the lock looks

| Picture | What it shows |
|---|---|
| [settings/power-updates-users.jpg](settings/power-updates-users.jpg) | Power, Updates and Users (M5.32, approved by Alimardon on 2026-10-11): Power's battery with its last day, the power modes and what the lid, the power button and the screen do; Updates' calm hero, the version kept ready to roll back to, what the release brought and the channel; Users' person with their picture, others to add and logging in without a password |
| [settings/lock-looks.jpg](settings/lock-looks.jpg) | The lock looks (M5.10c7, approved by Alimardon on 2026-10-11): the Lock screen page's looks with In use and Built in, the card with Customize and its More menu; the looks side by side on the lock (Super+L); the editor with its bar, wallpaper panel, widget card and strip, and Preview |

Their boards are `settings/canvas/power-updates-users.html` and `settings/canvas/lock-looks.html`, drawn from the design canvas as the Appearance board is.

`edel-mockups.html` draws them all; `node render.js` writes the pictures
(Playwright with Chromium, and Inter installed for the text).
