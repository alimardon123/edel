# Install Edel OS

Every way into Edel OS ends in one settings file, `settings.toml`, that describes the new machine: its people, its name, its language and its look (ADR-006, ADR-008). The ways differ only in who writes that file and when. Its keys are in [the settings file](settings.md).

**This is a preview.** Never install it on a disk that holds anything you need: installing erases the whole disk. [Try it](TRY-IT.md) from a USB stick first; the stick never writes to the computer's own disk.

## 1. Check the download

Each release lists its files with their sha256 in `release.toml`, and `release.toml.sig` signs that list, so a file whose sha256 matches the signed list is the one we built. Until the first preview is published, CI's `edel-try-x86_64` artifact carries `SHA256SUMS` instead ([Try it](TRY-IT.md)).

| On | Check the sha256 |
|---|---|
| Linux | `sha256sum edel-x86_64.img.gz` |
| macOS | `shasum -a 256 edel-x86_64.img.gz` |
| Windows | `certutil -hashfile edel-x86_64.img.gz SHA256` |

Compare the line it prints with the file's `sha256` in `release.toml`, or its line in `SHA256SUMS`. On a machine that already runs Edel OS, `edel release verify release.toml` checks the list's signature against the keys the system carries.

## 2. Write the USB stick

Writing erases the stick, which needs 4 GB or more.

- **Linux:** `gunzip -c edel-x86_64.img.gz | sudo dd of=/dev/sdX bs=4M conv=fsync`, with `sdX` the stick (`lsblk` lists the disks; pick the stick's, never a partition such as `sdX1`).
- **Windows and macOS:** we name a stick writer here only once CI or Alimardon has tried it. Until then, see [Try it](TRY-IT.md), which names the ones people use, from memory.

## 3. Start from the stick

1. Turn Secure Boot off in the computer's firmware settings: Edel OS is not signed for it yet. If Windows on that computer uses BitLocker or device encryption, have its recovery key at hand first.
2. Open the computer's list of boot devices at power on and pick the stick. The usual keys, from memory: HP F9 (F10 for the settings), Dell F12, Lenovo F12, Acer F12, ASUS F8 or Esc, MSI F11.
3. The desktop starts by itself as `live`, a person only the stick has.

## 4. Install

Each of these ends in the new disk's own `settings.toml`.

| Way | How | The settings file comes from |
|---|---|---|
| **Questions on a terminal** | Open a terminal (Super+T) and run `doas edel install`. It asks your language, keyboard, username and password, shows the disks, then the plan, and erases only once you type the disk's name. | Your answers, and only them: anything you leave at its suggestion that is the release's default is left out of the file. Your password is never in it. |
| **A file you have** | `doas edel install sda --settings my-machine.toml` | That file, checked first: a file with any problem is refused before the disk is touched. |
| **A file on a web server** | `doas edel install sda --settings https://example.org/fleet.toml` | That address, over https only: a settings file can make administrators and add ssh keys and is not signed, so `http://` is refused. The server is checked against the machine's certificate authorities, so a fleet adds its own authority to `/etc/ssl/certs/ca-certificates.crt` on the stick. |
| **Installer page in Settings** | Start from the stick: Settings opens on Install Edel OS. It asks the same questions, shows the disk as `edel install --list` sees it, then the plan, and erases only when Erase and install is pressed twice. Return moves to the next question, so the keyboard alone does it. | Your answers, as on a terminal. Your password goes to `edel install` on its standard input, never in the file. |
| **A file on another stick, with the installer page** | Put your settings file, `settings.toml`, at the top of any USB stick (FAT, as any computer writes it), plug it in beside the Edel OS stick and open Settings: the page finds it and offers Use it, then asks only what the file leaves open, such as the password. `edel install --find-settings` shows what it would find. | That file, with your answers to the rest added to it; its own lines and comments stay. |
| **A virtual machine image** | No install: start `edel-x86_64.img` and give its first boot a file on a small disk labelled `EDEL-SEED` ([Try it](TRY-IT.md)). | The seed disk's file, kept on the data partition. |

`edel install --list` shows every disk it could install on, with its model, size and partitions, and marks the one running; it changes nothing. `--plan` shows what an install would erase and write, and changes nothing. `edel settings check FILE` checks a file, or an `https://` address, before you use it.

After an install, take the stick out and restart: the new disk starts with its settings applied.

## Principles check

- **Reliable:** nothing is erased before the plan is shown and the disk named; a settings file with a problem, or one over plain http, is refused before anything changes.
- **Simple:** every way ends in the same one file, and one command, `edel install`, takes a file, an address or your answers.
- **Scalable:** one file on a web server sets up a whole fleet the same way.
- **Traded off:** no stick writer for Windows or macOS is recommended until one is tried, and Secure Boot must be off until Edel OS is signed for it.
