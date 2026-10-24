# Profiles

A profile is what one kind of work needs, ready made: a few apps, an add-on or two, and a value or two in the settings file. Choose one when you install (More options, Profile) or later in Settings (System, Profile). Nothing else changes: your own choices stay yours.

| Profile | For | What it brings |
|---|---|---|
| Office | documents, printing and scanning | [profiles/office.toml](https://github.com/alimardon123/edel/blob/main/profiles/office.toml) |
| Developer | containers and code | [profiles/developer.toml](https://github.com/alimardon123/edel/blob/main/profiles/developer.toml) |
| Creative | drawing and photos, and sound with little delay | [profiles/creative.toml](https://github.com/alimardon123/edel/blob/main/profiles/creative.toml) |
| Gaming | games and game controllers | [profiles/gaming.toml](https://github.com/alimardon123/edel/blob/main/profiles/gaming.toml) |

Each file is a few lines of the settings file and says in its comment what it brings; the installer and Settings say it in a line under the choice.

Pens and screen capture work on every desktop, with or without a profile.

Game controllers come with Gaming, as the gaming add-on; `edel settings set addons.installed+=gaming` adds them without the profile. The add-on also puts Edel OS Game on the login screen's session chip: Steam's Big Picture alone, fullscreen and in game mode, with no panel or dock; closing Steam brings the login screen back.

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

## Leaving out something a profile brings

A profile is a starting set. To keep Gaming but not Steam, take Steam out as you would any app, and the profile stops bringing it:

```sh
edel settings set apps.installed-=com.valvesoftware.Steam
```

The same works for an add-on, such as printing, with its switch on the Add-ons page or `addons.installed-=printing`. Edel OS writes what you left out in one key, and Settings' Profile line says it ("Steam, and game mode for every fullscreen game; without Steam"):

```toml
[system]
profiles = ["gaming"]
profiles_without = ["com.valvesoftware.Steam"]
```

Adding it again (`apps.installed+=com.valvesoftware.Steam`) takes it off that list, and choosing another profile keeps only what the new one still brings, so you never edit the key by hand. Everything else the profile brings, including later releases' changes, still comes.

## AI agents

AI agents are an add-on, not a profile, so they come only to people who pick them: the AI agents switch on Settings' Add-ons page, or one line:

```sh
edel settings set addons.installed+=ai-agents
```

It brings two apps from Flathub: Newelle, an assistant that can run commands for you, with a model on your computer or your own account with a provider, and Alpaca, a chat app for models that run on your computer. Neither runs until you start it, and Edel OS sends nothing anywhere; an agent's account and key are yours, kept by the agent. An agent changes the system only as you could, through `edel settings set` and the other `edel` commands, so `edel settings diff` shows what it changed, `edel settings reset` undoes a setting and `edel rollback` an update. Command-line agents run in a dev container, with the Developer profile's Podman. Taking the add-on out (`addons.installed-=ai-agents`) brings no more apps and leaves the ones you have, which you remove as any app. While the add-on is on, it brings both apps back at the next apply; to keep only one, take the add-on out and add that app alone (`apps.installed+=ID`).
