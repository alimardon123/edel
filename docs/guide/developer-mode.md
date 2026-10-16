# Developer mode

Edel OS's own files are read-only, so every machine runs exactly the system that was signed, and an update or a rollback swaps all of it at once. Developer mode is the one way past that, for tinkerers and people who work on the system itself (ADR-007, level 3). It is off unless you turn it on.

## Turning it on

In Settings, System, switch Developer mode on and confirm the warning. On the command line:

```sh
edel settings set system.developer_mode=true
edel settings apply
```

or the line `developer_mode = true` under `[system]` in the settings file. It takes effect when the computer next starts. `edel status` then says `developer mode: on`, and the panel shows Developer mode.

## What it allows

- `/usr`, where all of the system lives (`/bin`, `/sbin` and `/lib` lead there), becomes writable. What you change is kept apart, on the data partition, in a layer over the system: the system itself is never written.
- `apk add` installs Alpine packages for the release's Alpine branch into that layer.
- Your own OpenRC services go in `/usr/local/etc/init.d`, inside the layer, and are added with `rc-update add NAME default`. A service written to `/etc/init.d` instead is not part of the layer, so a safe start (below) would still run it.

## When a change stops the computer starting

The guard restarts a start that does not finish, and each slot has a few tries. Once no slot has a try left, the computer starts safely: GRUB adds `edel.safe` to the command line, and the developer layer is held back, on that start and every one after it, so your changes are off but kept. `edel status` says so. To end it:

- switch developer mode off and on again, to try your changes once more, or
- reset it (below), to delete them.

## Reset

```sh
edel settings reset system.developer_mode
edel settings apply
```

turns developer mode off and, after you type `yes` (or with `--yes`), deletes every change it made under `/usr` when the computer next starts, with apk's list of wanted packages. Settings' Reset developer mode row does the same. Files that packages wrote to `/etc` stay, as `/etc` is this machine's own (ADR-006); `edel settings export` lists them. Switching developer mode off instead keeps the layer, ready for switching it on again.

## Updates

An update keeps the layer. A file you changed hides the new release's copy of it, apk's database included, so after an update apk may not know what the release changed. When something breaks after an update, reset is the fix.

## Moving to another machine

`edel settings export` carries `developer_mode = true` and says how many files the layer changed, but not the files themselves. Software that should reach other machines is an add-on (roadmap M7.2a), not a developer mode change.
