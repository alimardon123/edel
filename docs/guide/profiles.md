# Profiles

A profile is what one kind of work needs, ready made: a few apps, an add-on or two, and a value or two in the settings file. Choose one when you install (More options, Profile) or later in Settings (System, Profile). Nothing else changes: your own choices stay yours.

| Profile | For | What it brings |
|---|---|---|
| Office | documents and printing | [profiles/office.toml](https://github.com/alimardon123/edel/blob/main/profiles/office.toml) |
| Developer | containers and code | [profiles/developer.toml](https://github.com/alimardon123/edel/blob/main/profiles/developer.toml) |
| Creative | drawing and photos | [profiles/creative.toml](https://github.com/alimardon123/edel/blob/main/profiles/creative.toml) |
| Gaming | games | [profiles/gaming.toml](https://github.com/alimardon123/edel/blob/main/profiles/gaming.toml) |

Each file is a few lines of the settings file and says in its comment what it brings; the installer and Settings say it in a line under the choice.

Pens and screen capture work on every desktop, with or without a profile.

## In the settings file

The settings file names the profiles, never what is in them:

```toml
[system]
profiles = ["developer"]
```

The same on the command line, as an administrator:

```sh
edel settings set system.profiles='["developer"]'
edel settings apply
```

`edel settings reset system.profiles` takes the profile away again. A name this release has no profile for is refused, with the names there are:

```text
edel settings: system.profiles: there is no profile "chess"; the profiles are office, developer, creative, gaming
```

Because only the name is written, a profile a later release renews (a better editor, one more add-on) reaches every machine that names it with the next update. Each profile is one file in the repository, `profiles/NAME.toml`, a few lines of the settings file.

## How a profile and your own values meet

A value your settings file sets wins over the profile's. A list your file has, such as `apps.installed`, keeps its items and gains the profile's. A machine may name more than one profile; where two set the same value, the first named wins. In full, a key's value comes from your own file, then the machine's, then its profiles in the order named, then the layout preset, then the release ([ADR-008](../ADR-008-features-defaults-and-install.md)).

Apps a profile names are installed from Flathub when the machine is online, as `apps.installed` always is; its add-ons arrive with the next update and are listed by `edel addon list`.
