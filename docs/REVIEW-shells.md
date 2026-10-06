# Review: shells that look beautiful and feel instant

**Date:** 2026-10-03
**Asked by:** Alimardon, choosing shell-ui's toolkit (option B, our own drawing): "learn from other shell UIs to ... develop a better looking really beautiful designs, for example Hyprland, and others ... that feels instant but really beautiful".
**Reviewed:** Hyprland's [animation docs](https://wiki.hypr.land/0.54.0/Configuring/Animations/), niri's [animation docs](https://github.com/niri-wm/niri/wiki/Configuration:-Animations), [Quickshell](https://quickshell.org/about) and the shells built on it ([Caelestia](https://github.com/JohannesPertl/caelestia-shell), end-4's "illogical-impulse"), and, from memory, GNOME Shell, macOS, Windows 11, KDE Plasma and COSMIC. Facts from memory are marked so; the steps that use them check them first.

The question is narrow: what makes a desktop look beautiful, what makes it feel instant, and which of those we can have within the principles (Instant ranks above Beautiful) and the budgets. COSMIC and Plasma stay rejected as Edel's shell (ADR-002); looking at how they draw is not adopting them.

## What each does well

### Hyprland

- **Motion is configured as curves and speeds.** Every animation (windows, their open and close, fades, borders, workspaces, layers) takes a speed in tenths of a second and a cubic Bézier curve; a window can open with `popin`, growing from a percentage of its size, or `slide`. The well-liked configurations use short, overshoot-free ease-out curves of about 0.3 to 0.5 s (from memory).
- **Decoration makes the depth:** rounded corners, soft drop shadows, gaps between tiles, gradient borders on the focused window, and blur behind translucent windows and panels (from memory: a dual Kawase blur, drawn only where something translucent is, with an option to blur the wallpaper alone).
- **Input never waits.** Focus, typing and the next layout apply at once; animations only draw the way there.
- **Cost:** live blur and many effects cost a lot on old GPUs, which is why ADR-002 asks for the same feel with tiers. Its dozens of animation knobs are the opposite of "presets, not options".

### niri

- **Springs, not fixed durations, for motion.** Workspace switches, window movement and resizing are critically damped springs (damping 1.0, stiffness 800 to 1000), which settle in about a quarter of a second, never bounce, and keep their speed when interrupted; small notices use a bouncier spring (damping 0.6). Fades use plain easing.
- **Gestures are one to one.** A touchpad swipe moves the workspace under the fingers, and on release a spring carries on with the fingers' speed.
- **Closing draws a snapshot** of the window's last frame, the same thing M5.11b had to do.
- **Everything can be slowed, sped up or turned off** with one factor.

### Quickshell and the shells built on it (Caelestia, illogical-impulse)

- **Surfaces morph.** Menus, the calendar and quick settings grow out of the bar instead of popping up beside it; the bar's ends meet the screen with rounded fillets, so bar and screen read as one shape.
- **One colour system.** Material-style colours are generated from the wallpaper, so the panel, the accent and the menus always match it.
- **One radius, one spacing, one motion language** across every widget.
- **Cost:** each is a QML program on Qt Quick. A person's setup is code, which is the theme engine and extension API ADR-002 rules out (a script can break the desktop after an update), and Qt Quick is a full GPU toolkit in the same weight class as GTK 4 (54 to 182 MiB proportional for one GTK 4 window, M5.1b's notes; Qt not measured). Quickshell itself can still run on Edel as an ordinary program, since our compositor speaks the layer shell it uses (M5.1a); widgets that talk to Hyprland's own IPC would not work.

### GNOME Shell (from memory)

- **Calm, typographic, little chrome:** generous spacing, one accent, careful type sizes; the overview zooms out of the windows instead of switching to another screen.
- **Gestures one to one** for the overview and workspaces.
- **Cost:** JavaScript extensions break with each release, the lesson ADR-002 already took.

### macOS (from memory)

- **Depth from shadows, not borders:** large, soft, low-opacity window shadows, stronger for the focused window.
- **Materials used sparingly:** translucency and blur only on transient surfaces (menus, the Dock, sidebars), tinted by what is behind.
- **Physics-based motion:** springs everywhere, and windows that grow from where they came from (the Dock icon).
- **Consistent corner radius** on windows, menus and controls.

### Windows 11 (from memory)

- **Mica:** a window's backdrop is the wallpaper, blurred and tinted, computed once rather than every frame: most of the look of blur for almost none of its cost. ADR-002's Balanced tier already describes exactly this.
- **Acrylic** (live blur) only for transient surfaces such as menus and flyouts.
- **8 px rounded corners**, square again when a window is maximized or snapped.
- **Snap layouts** offered from the maximize button.

### KDE Plasma and COSMIC (from memory, as design references only)

- **Plasma:** a polished Breeze look and capable effects, and the anti-lesson of options for everything.
- **COSMIC:** a Rust shell with rounded, flat, calm surfaces, an accent with good contrast, and automatic tiling that keeps title bars.

## What makes it feel instant

1. **The target state applies at once.** Focus, input and the layout go to where the animation ends; the animation only draws the way there. M5.11b already works this way.
2. **Every animation can be interrupted** and starts from where things are drawn, with their speed: springs do this naturally.
3. **Short and without overshoot** for anything a person waits on: about 150 to 250 ms. Bounce only where nothing waits (a notice arriving).
4. **Gestures one to one,** then a spring at the fingers' speed.
5. **Animate cheap things:** opacity, position and scale of what is already drawn, never asking a client to redraw at each step. A resizing window animates a snapshot and swaps to the real one at the end.
6. **Expensive looks computed once:** the wallpaper's blur when the wallpaper changes, shadows as a prepared image, not per frame.
7. **No first-frame stalls:** CI caught llvmpipe compiling a shader variant mid-animation (M5.11b); shaders an effect needs are prepared before its first use.

## What makes it beautiful

1. **One radius, one spacing scale, one type scale,** used by the compositor, shell-ui and Settings alike: our design tokens (M5.5).
2. **Depth from soft shadows** rather than lines.
3. **Translucency only where it helps:** transient surfaces (launcher, quick settings, menus) and the panel; never under text people read for long.
4. **Colour that matches the wallpaper,** as a choice: the accent taken from the wallpaper.
5. **Surfaces that grow from where they belong:** a menu from its panel button, a window from its launcher icon.
6. **Typography first:** Inter at the token sizes (ADR-008), with real text shaping so every script looks right.

## What we take

| Take | Where | Tier |
|---|---|---|
| Springs (critically damped, interruptible, carrying speed) for slides, workspaces and gestures; easing stays for fades | M5.14 | Full and Balanced; Lite keeps short fades |
| Windows and menus grow from where they came from | M5.3, M5.9 | Full and Balanced |
| Rounded corners, soft shadows, blur: rounded corners and shadows on Full and Balanced, the wallpaper's blur computed once on Balanced, live blur behind transient surfaces on Full, square corners when maximized or tiled flush | M5.14 | as ADR-002's table |
| Shaders and prepared images ready before an effect's first frame | M5.14 | all |
| Radius, spacing, type, shadow, blur and motion as tokens | M5.5 | all |
| shell-ui's surfaces morph out of the panel; rounded fillets where the panel meets the screen | M5.1b, M5.9 | Full and Balanced; Lite draws them flat |
| The accent taken from the wallpaper, as `appearance.accent = "wallpaper"` | M5.12 | all |
| Workspace swipes one to one on touchpads | M5.2 | all |
| Quickshell left free to run as an ordinary program | already, through the layer shell (M5.1a) | |

## What we skip

- **Scripted shells** (QML, JavaScript extensions): a theme engine and extension API by another name (ADR-002).
- **Per-animation knobs:** one `appearance.animations` (full, reduced, off) and the presets, not dozens of curves.
- **Live blur everywhere,** wobbly windows and other effects that cost frames on old hardware for little.
- **Bouncy motion** for anything a person waits on.

## Principles check

- **Instant** decided what we take: everything in "What makes it feel instant" serves the next frame; every look we take is either cheap (opacity, transform, prepared images) or limited to the Full tier with the deadline monitor ready to drop it.
- **Beautiful** is served through tokens and a few effects done well rather than many: one radius, soft shadows, translucency where it helps, colour from the wallpaper.
- **Simple:** no new process, no scripting, one tokens file; the effects live in the compositor and shell-ui we already have.
- **Versatile:** looks change through presets and `[appearance]`, not code.
- **Traded off:** live blur is limited to transient surfaces on Full; people who want a Hyprland-style riced desktop can run Quickshell themselves, outside what Edel supports.
