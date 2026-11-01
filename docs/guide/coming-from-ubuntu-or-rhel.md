# Coming from Ubuntu or RHEL

Edel OS does the same jobs with fewer, different tools. Its system is read-only and updates as a whole, so most habits move from changing the system to choosing what runs on top of it ([How Edel OS works](how-it-works.md)).

| You used to | On Edel OS |
|---|---|
| `systemctl start NAME`, `stop`, `status` | `rc-service NAME start`, `stop`, `status` (OpenRC) |
| `systemctl enable NAME` | `edel settings set services.FEATURE=true` for a switchable feature such as `ssh`, or Settings; the settings file keeps it ([the settings file](../settings.md)) |
| `journalctl` | the system log, `/var/log/messages`; the desktop's own log is `~/.local/state/edel/session.log`; `edel report` gathers both ([Messages and logs](../MESSAGES.md)) |
| `apt install` or `dnf install` an app | Flatpak from Flathub, or `edel settings set apps.installed+=ID` for everyone ([Commands](commands.md)) |
| `apt install` a server | an OCI container with Podman (the `containers` add-on, or the Developer profile) |
| `apt install` build tools | a dev container, so each project has its own tools and the system stays clean |
| a vendor's `.deb` or `.rpm`, an AppImage, Homebrew | the dev box (the `containers` add-on): `distrobox create` makes an Ubuntu LTS box, `distrobox enter` opens it (or Dev in the launcher), and `distrobox-export --app NAME` or `--bin PATH` puts what you install there in the launcher and on your `PATH` |
| `apt upgrade`, `dnf upgrade` | `edel update`, which installs the next release beside this one; `edel rollback` goes back |
| `/etc` files by hand | one settings file, `edel settings set`, `diff`, `export` and `apply`; a file copies a machine to another |
| `sudo` | `doas`; an administrator runs `edel` through it with no password, and `edel` asks for one before a change that could break or open the machine (an install, an update) |
| `ufw` or `firewalld` | the `firewall` feature: on where there is a screen, off without one; `services.firewall = true` or `false` sets it either way |
| a PPA, a changed kernel, your own packages | developer mode (`system.developer_mode`), which keeps the system's own parts yours to change and undoes them with one reset |

Everything Settings does, `edel` does too, with the same names: `edel settings` lists the pages, and `edel settings get KEY` says where a value comes from.
