# Mockups of the target look

Pictures of where the Edel shell's look is heading. They are not
screenshots and not binding: the real values are design tokens
(`design/tokens.toml`, M5.5), and a person can change every one of them
(M5.12). This is the second round, retouched on 2026-10-03 after
Alimardon's review of the first: a little sharper, calm and polished
rather than playful, and welcoming to everyone by default.

| Picture | What it shows |
|---|---|
| [classic.jpg](classic.jpg) | Classic, the default, light: a panel along the bottom with the menu, workspace buttons 1 to 4, the windows, the floating or tiling toggle, status and clock |
| [classic-dark.jpg](classic-dark.jpg) | The same in the dark scheme |
| [classic-menus.jpg](classic-menus.jpg) | The launcher, quick settings and a notification, over the Settings app's Layout page |
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
- app icons in one shape with muted colours and one light from above;
- frosted panels and menus on Full, the wallpaper blurred once on
  Balanced, solid colours on Lite.

`edel-mockups.html` draws them all; `node render.js` writes the pictures
(Playwright with Chromium, and Inter installed for the text).
