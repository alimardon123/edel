# Layout guide for apps

How an app fits every screen Edel OS runs on, from a phone to a desktop, with one package (ADR-004). It is for anyone writing an app for Edel OS, and our own apps follow it first: Settings is the reference (M5.6).

## Three size classes

An app picks its layout from its window's width, never from the device. The window's width changes when a person resizes it, docks a phone or unfolds a foldable, and the app follows.

| Size class | Width, in logical pixels | Typical place | Layout |
|---|---|---|---|
| Compact | below 600 | a phone, a narrow window | one pane at a time, full-width pages, navigation at the bottom or a back button at the top |
| Medium | 600 to 839 | a tablet, a window beside another | two panes, or a sidebar that folds away |
| Expanded | 840 and wider | a laptop, a desktop, a docked phone | sidebar, content and details side by side |

These are Android's window size classes, so developers who know them need to learn nothing new. Logical pixels are the screen's pixels divided by its scale (`displays.NAME.scale`), the unit GTK and Qt already use. In libadwaita, write the cutoffs as breakpoints: `max-width: 600sp` for Compact, `max-width: 840sp` for Medium. Settings folds its sidebar at the first.

## Touch targets

| Input | Smallest target | Space between targets |
|---|---|---|
| Pointer (mouse, touchpad) | 24 px square | 4 px |
| Touch mode | 44 px square | 8 px |

A row, a button or a switch counts with its padding: the area that takes the click, not the drawn shape. In touch mode an app grows its targets and spacing to these sizes; it does not change what it shows.

## Touch mode

Touch mode is a desktop-wide setting that says a person is using fingers rather than a pointer: on a tablet, on a phone, or on a 2-in-1 folded into tablet mode, when it changes by itself (M9.6). Apps read it through the standard settings portal (`org.freedesktop.portal.Settings`):

| Namespace | Key | Type | Values |
|---|---|---|---|
| `io.github.alimardon123.edel` | `touch-mode` | boolean (`b`) | `true` in touch mode, else `false` |

Read it once with `ReadOne`, and follow `SettingChanged` for the same namespace and key, which the portal sends whenever it flips. An app that cannot read it (an older portal, a sandbox without it) assumes `false`. shell-ui serves it from M9.6, as it serves the colour scheme and the accent today (M5.5).

## Live resize, never restart

A size change is just a new size. When the width crosses a cutoff, the app moves its content between panes, and it keeps everything: what a person typed, the scroll position, the selection, the page they were on and anything playing. It never restarts, reloads or asks to. A foldable opening, a phone docked to a monitor and a window snapped beside another all arrive as plain resizes.

## What Edel OS does for you

- The compositor sets every window's size, so a phone's apps open full screen and a tablet's tile (ADR-002).
- Title bars are the compositor's: an app that leaves them to us gets the person's own buttons on their side. A window keeps the middle of its title bar free to drag; Super with a drag moves a window from anywhere in it.
- Colours, light and dark and the accent come through the settings portal and GTK's named colours from the design tokens (M5.5), so an app that uses them follows the person's look.

## Principles check

- **Reliable:** live resize keeps a person's work through every change of size; nothing restarts.
- **Simple:** three classes known from Android, one portal key, no new toolkit or API.
- **Functional:** the touch sizes match what fingers need, the pointer sizes what a mouse can hit.
- **Versatile:** one package serves a phone, a tablet and a desktop.
- **Traded off:** fixed cutoffs fit most screens but not every one perfectly; an app may add its own breakpoints within a class.
