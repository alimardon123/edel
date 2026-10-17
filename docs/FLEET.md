# Fleets

A fleet is many machines that run the same signed Edel OS and differ only in their settings file (ADR-006, ADR-009). There is no fleet agent, no server to run and no telemetry: a machine's whole setup is one `settings.toml`, and a fleet image carries one inside it.

## Build a fleet image

```sh
edel image build images/vm.toml --settings fleet.toml --settings room-12.toml --out out/room-12
```

`--settings FILE` may be given more than once. The files are merged in order, a later file winning where two set the same key (a table is merged key by key; any other value, a list included, is replaced whole), the result is checked as strictly as `edel settings check` does, and it is written to the image's `/usr/share/edel/settings.toml`. That file seeds the machine's own at its first boot, unless an `EDEL-SEED` volume or the EFI system partition holds one first (`docs/settings.md`). A file with any problem stops the build before anything is built.

A typical fleet keeps one shared file and one small file per device:

```toml
# fleet.toml: what every machine shares
format = 1

[users.admin]
admin = true
ssh_keys = ["ssh-ed25519 AAAA... admin@fleet"]

[region]
timezone = "Europe/Berlin"
```

```toml
# room-12.toml: this device alone
format = 1

[network]
hostname = "room-12"
```

## Install by itself

A fleet stick can install itself onto the machine it starts on, with nobody at the keyboard. Give the settings file an `[install]` table:

```toml
[install]
disk = "auto"          # or a /dev/disk/by-id/ name
erase = "empty-only"   # or "yes"
then = "poweroff"      # or "reboot"
```

| Key | Values | Without it |
|---|---|---|
| `install.disk` | `auto`: the one disk `edel install --list` offers that is not removable, not the stick and large enough; or a `/dev/disk/by-id/` name | `auto` |
| `install.erase` | `empty-only`: only a disk with no partitions and nothing in its first MiB; `yes`: any disk, after a ten second countdown on the console, and only when `disk` is a `/dev/disk/by-id/` name | `empty-only` |
| `install.then` | `poweroff` or `reboot` once the disk is written | `poweroff` |

At every start the `edel-install` service runs `edel install --unattended`, which does nothing when the settings file has no `[install]` table. With one, it waits until the stick's own slot is confirmed (so a stick that cannot start well never writes a disk), picks the disk, installs as `edel install DISK --settings FILE --yes` would, writes the disk's id and the date to the stick's `/data/edel/installed-to`, and powers off or restarts. A stick that finds the disk its `installed-to` names in the machine says `already installed to ID` and leaves it alone, so starting from the stick again never installs twice.

The installed machine's settings file is the stick's without `[install]`: only `edel install` reads that table, and `edel settings export` never writes it. A file that leaves nobody able to log in (people named, and none of them with an ssh key) is refused, both by the image build and by the install, because an unattended machine has nobody to answer a password question.

A refusal names its cause on the console and changes nothing, such as `install refused: /dev/vdb is not empty`.

## What it is not

- No configuration language, templates or variables: a device file is a settings file.
- No fleet server: a stick, a seed volume or an https address (`edel install --settings https://...`, [Install](INSTALL.md)) carries the file.
- A machine's drift from its file is `edel settings diff` (ADR-009).

## Principles check

- **Reliable:** the stick installs only from a confirmed slot, an unattended install erases only an empty disk unless a by-id name and `erase = "yes"` say otherwise, and a disk installed once is never installed again from the same stick.
- **Simple:** a fleet image is the same image with one more file; the merge is the settings file's own keys, and the service is one command of `edel`.
- **Scalable:** one shared file and one small file per device describe any number of machines.
- Traded: no Settings page shows `[install]`, as it is an instruction to a stick, not a setting of a machine; Settings' installer page is the same install by hand.
