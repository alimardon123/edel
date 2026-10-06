# Try Edel OS

**This is a preview, for testing only.** It makes no promise yet. The VM image is a text console with updates, rollback, the settings file and the installer; the desktop image adds our desktop, early and only now meeting real hardware. Since milestone 1, `/home`, `/var` and the settings file live on the data partition and survive every update and rollback, but a preview can still break, so never install it on a disk holding anything you need. Each merge to `main` replaces the preview.

## Get the images

Until the first preview is published (roadmap M3.4), the newest images come from CI (M3.6). Every merge to `main` keeps the desktop image it has just started in its tests, for 14 days:

1. Open [the CI runs on `main`](https://github.com/alimardon123/edel/actions/workflows/ci.yml?query=branch%3Amain) and click the newest run with a green tick. You need to be logged in to GitHub.
2. Under **Artifacts** at the bottom, download `edel-try-x86_64`. It is a zip holding `edel-desktop-x86_64.img.gz` and `SHA256SUMS`, and the run's summary lists its size.
3. Unzip it. Keep the `.img.gz` file as it is for a USB stick; unpack one (with 7-Zip on Windows, `gunzip` elsewhere) for VirtualBox or QEMU.

Once a preview exists, the same files are on its release page.

## The files

Each release has these files:

| File | What |
|---|---|
| `edel-vm-x86_64.img.gz` | A disk for a virtual machine (UEFI) |
| `edel-desktop-x86_64.img.gz` | The desktop, to write to a USB stick of 4 GB or more, or to start in VirtualBox or QEMU |
| `edel-container-x86_64.tar.gz` | The base as a container image (`docker import`) |
| `*.ext4.gz`, `release.toml`, `release.toml.sig` | The update images and their signed list, which `edel update` reads |
| `*.packages` | Every package inside each image |

## The desktop on a laptop, from a USB stick

The stick touches only itself: nothing is written to the laptop's disk.

1. Write `edel-desktop-x86_64.img.gz` to a USB stick of 4 GB or more. This erases the stick.
   - On Windows: [Rufus](https://rufus.ie) or [balenaEtcher](https://etcher.balena.io), both of which write `.img.gz` files as they are (from memory; in Rufus pick the file and keep "DD image" if it asks).
   - On Linux: `gunzip -c edel-desktop-x86_64.img.gz | sudo dd of=/dev/sdX bs=4M conv=fsync`, with `sdX` the stick (check with `lsblk`).
2. If Windows on the laptop uses BitLocker or device encryption, have its recovery key at hand: changing Secure Boot can make Windows ask for it once.
3. Turn Secure Boot off in the laptop's firmware settings (Edel OS is not signed for it yet). On an HP laptop, such as the HP 250 G8, F10 at power on opens the settings and F9 the list of boot devices (from memory).
4. Start the laptop from the stick. After a little while the desktop shows by itself, logged in as a person called `live`: the stick makes `live` on its own data partition, so the desktop needs no login. Super (the Windows key) opens the launcher, Ctrl+Alt+T a terminal, and Super+T tiles the windows.
5. Turn the laptop off and put the stick into any computer: its small FAT partition holds `EFI/edel/report.toml`, a report of your hardware written at every start. Attach it to an issue at <https://github.com/alimardon123/edel/issues> with the laptop's model, and say what worked: the screen, the touchpad, Wi-Fi, sound, sleep.

The report holds the release, the kernel, how long the start took, the memory in use, every PCI device with its driver, and the kernel log. It never holds your files.

## The desktop in VirtualBox

1. Unpack `edel-desktop-x86_64.img.gz` to `edel-desktop-x86_64.img` (7-Zip on Windows).
2. Turn it into a VirtualBox disk: `VBoxManage convertfromraw edel-desktop-x86_64.img edel-desktop.vdi --format VDI` (on Windows, `VBoxManage.exe` is in VirtualBox's folder, usually `C:\Program Files\Oracle\VirtualBox`).
3. Make a new virtual machine: type Linux, version Other Linux (64-bit), 2 GB of memory or more and 2 processors, with `edel-desktop.vdi` as its existing disk.
4. In its settings, under System, turn on **Enable EFI**; under Display, pick the **VMSVGA** graphics controller with 128 MB of video memory. The desktop image carries VMSVGA's driver (`vmwgfx`) and Mesa's for it.
5. Start it. Nobody has an account on the new disk, so the desktop logs `live` in by itself, as on a stick.

## The desktop in QEMU

On Linux: unpack the image, then

```sh
qemu-system-x86_64 -machine q35,accel=kvm -m 2048 -smp 2 \
  -bios /usr/share/ovmf/OVMF.fd -vga std \
  -drive file=edel-desktop-x86_64.img,format=raw,if=virtio
```

## The console in a virtual machine

On Linux with QEMU and OVMF (`sudo apt install qemu-system-x86 ovmf` on Debian or Ubuntu):

```sh
gunzip edel-vm-x86_64.img.gz
qemu-system-x86_64 -machine q35,accel=kvm -m 1024 -smp 2 \
  -bios /usr/share/ovmf/OVMF.fd \
  -drive file=edel-vm-x86_64.img,format=raw,if=virtio \
  -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:2222-:22 \
  -nographic
```

The image has no user and no password: you bring them in a settings file. Make one with your ssh key, and give it to the first boot on a small disk labelled `EDEL-SEED`:

```sh
cat >settings.toml <<EOF
format = 1

[users.me]
admin = true
ssh_keys = ["$(cat ~/.ssh/id_ed25519.pub)"]

[users.root]
ssh_keys = ["$(cat ~/.ssh/id_ed25519.pub)"]
EOF
mkfs.vfat -C seed.img 1024 -n EDEL-SEED && mcopy -i seed.img settings.toml ::/
```

Add `-drive file=seed.img,format=raw,if=virtio` to the QEMU line, start it, and log in with `ssh -p 2222 me@127.0.0.1`; `edel settings export` works as `me`. Commands that change the machine, such as `edel status`, `edel update` and `edel settings set network.hostname=mine`, need root until `doas` arrives (roadmap M6.5): log in for them with `ssh -p 2222 root@127.0.0.1`.

## Updates

A machine checks a release with `edel update --check URL` and installs it with `edel update URL`, where URL is a `release.toml`; the preview's is `https://github.com/alimardon123/edel/releases/download/preview/release.toml`. The update goes into the other slot and is confirmed on the next start; a start that fails three times, hangs or freezes comes back on the previous slot by itself. `edel rollback` goes back by hand.
