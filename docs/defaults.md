# Defaults

This file is written by `EDEL_WRITE_DOCS=1 cargo test -p edel key_help` from `crates/edel/src/key_help.rs` and must not be edited by hand. It gives the value every settings key has while the settings file does not set it, and who decides that value. [The settings file](settings.md) says what each key does.

| Key | Default | Decided by |
|---|---|---|
| `system.variant` | the image's | image |
| `system.developer_mode` | `false` | release |
| `system.profiles` | `[]` | release |
| `users.*.admin` | `false` | release |
| `users.*.ssh_keys` | the file as it is | release |
| `users.*.login_shell` | `"/bin/sh"` | release |
| `network.hostname` | `"edel"` | image |
| `region.language` | English | release |
| `region.keyboard` | `"us"` | release |
| `region.timezone` | `"UTC"` | release |
| `region.time_servers` | `["pool.ntp.org"]` | release |
| `layout.preset` | `"classic"` | release |
| `layout.tiling` | the preset's | preset |
| `layout.title_bars` | the preset's | preset |
| `layout.device_type` | detected | hardware |
| `layout.window_buttons` | the preset's | preset |
| `layout.close_button` | `true` | release |
| `layout.minimize_button` | `true` | release |
| `layout.maximize_button` | `true` | release |
| `layout.tiling_style` | the preset's | preset |
| `panels.list` | the preset's | preset |
| `panels.tray` | `[]` | release |
| `panels.pinned` | the preset's | preset |
| `workspaces.count` | the preset's | preset |
| `workspaces.dynamic` | `false` | release |
| `workspaces.per_screen` | `false` | release |
| `workspaces.names` | `[]` | release |
| `workspaces.wallpapers` | `[]` | release |
| `workspaces.apps` | `{}` | release |
| `workspaces.overview_strip` | `"left"` | release |
| `displays.*.position` | each screen's own | hardware |
| `displays.*.scale` | each screen's own | hardware |
| `displays.*.resolution` | each screen's own | hardware |
| `displays.*.refresh_rate` | the resolution's best | hardware |
| `displays.*.enabled` | `true` | hardware |
| `displays.*.rotation` | each screen's own | hardware |
| `appearance.theme` | `"edel"` | release |
| `appearance.wallpaper` | `"hills"` | release |
| `appearance.wallpaper_dark` | the wallpaper's | release |
| `appearance.pictures` | the theme's | release |
| `appearance.switcher_look` | `"numbers"` | release |
| `appearance.switcher_shown` | `3` | release |
| `appearance.switcher_ends` | `"arrows"` | release |
| `appearance.mode` | `"auto"` | token |
| `appearance.contrast` | `"normal"` | release |
| `appearance.accent` | `"blue"` | token |
| `appearance.colours.*` | the token's | token |
| `appearance.colours.light.*` | the token's | token |
| `appearance.colours.dark.*` | the token's | token |
| `appearance.radius` | the token size.radius_window | token |
| `appearance.gaps` | the token size.gap | token |
| `appearance.gaps_outer` | the token size.gap_outer | token |
| `appearance.border_width` | the token size.border | token |
| `appearance.title_bar_height` | the token size.title_bar | token |
| `appearance.panel_opacity` | the token size.panel_opacity | token |
| `appearance.window_opacity` | the token size.window_opacity | token |
| `appearance.window_opacity_unfocused` | the token size.window_opacity_unfocused | token |
| `appearance.shadow_size` | the token size.shadow_blur | token |
| `appearance.shadow_darkness` | the alpha of colour.shadow | token |
| `appearance.blur` | the token size.blur | token |
| `appearance.font` | `"Inter Variable"` | token |
| `appearance.monospace_font` | `"Noto Sans Mono"` | release |
| `appearance.text_size` | `100` | release |
| `appearance.cursor_size` | `24` | release |
| `appearance.cursor_theme` | `"default"` | release |
| `appearance.icon_theme` | `"Edel"` | release |
| `appearance.icon_size` | `100` | release |
| `appearance.animations` | `"full"` | release |
| `appearance.animation_speed` | `100` | release |
| `appearance.animation_curve` | `"smooth"` | release |
| `appearance.widgets` | none | release |
| `lock_screen.widgets` | the look's | release |
| `lock_screen.look` | `"classic"` | release |
| `lock_screen.wallpaper` | the look's | release |
| `lock_screen.dim` | 30, or 40 for a sharp wallpaper or picture | release |
| `lock_screen.notifications` | `"count"` | release |
| `lock_screen.looks` | none | release |
| `themes.*` | none | release |
| `presets.*` | none | release |
| `shortcuts.*` | each action's own keys, docs/SHORTCUTS.md | release |
| `clipboard.history` | `true` | release |
| `default_apps.browser` | the first installed browser | image |
| `default_apps.files` | the first installed file manager | image |
| `default_apps.editor` | the first installed editor | image |
| `default_apps.terminal` | the first installed terminal, else foot | image |
| `default_apps.mail` | the first installed mail app | image |
| `startup.apps` | `[]` | release |
| `power.lid_close` | suspend | release |
| `power.lock_after_minutes` | `5` | release |
| `power.screen_off_minutes` | `5` | release |
| `power.sleep_after_minutes` | `15` | release |
| `power.power_button` | suspend | release |
| `power.on_battery` | balanced | release |
| `power.game_mode` | plugged-in | release |
| `power.battery_care` | `false` | release |
| `services.*` | the image's | image |
| `updates.channel` | the image's | image |
| `updates.version` | follows the channel | release |
| `updates.automatic` | not set yet | release |
| `updates.restart_window` | none | release |
| `apps.installed` | `[]` | release |
| `apps.camera` | `{}` | release |
| `apps.microphone` | `{}` | release |
| `apps.screenshots` | `{}` | release |
| `apps.notifications` | `{}` | release |
| `apps.files` | `{}` | release |
| `addons.installed` | `[]` | release |
| `notifications.do_not_disturb` | `false` | release |
| `notifications.position` | `"auto"` | release |
| `notifications.badge` | `"dot"` | release |
| `notifications.list` | `"button"` | release |
| `region.input_method` | `"none"` | release |
| `apps.location` | `{}` | release |
| `location.enabled` | `true` | release |
| `location.place` | the time zone's city | release |
| `location.online` | `false` | release |
| `install.disk` | `"auto"` | release |
| `install.erase` | `"empty-only"` | release |
| `install.then` | `"poweroff"` | release |
| `system.profiles_without` | `[]` | release |
| `sound.low_latency` | `false` | release |
