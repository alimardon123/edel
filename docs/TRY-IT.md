# Try Edel OS

**This is a preview.** It makes no promise yet: it is a text console with updates, rollback, the system file and the installer, and no desktop. Since milestone 1, `/home`, `/var` and the system file live on the data partition and survive every update and rollback, but a preview can still break, so never install it on a disk holding anything you need. Each merge to `main` replaces the preview.

## The files

Each release has these files:

| File | What |
|---|---|
| `edel-vm-x86_64.img.gz` | A disk for a virtual machine (UEFI) |
| `edel-laptop-x86_64.img.gz` | A disk to write to a USB stick of 4 GB or more and start a laptop from |
| `edel-container-x86_64.tar.gz` | The base as a container image (`docker import`) |
| `*.ext4.gz`, `release.toml`, `release.toml.sig` | The update images and their signed list, which `edel update` reads |
| `*.packages` | Every package inside each image |

## In a virtual machine

On Linux with QEMU and OVMF (`sudo apt install qemu-system-x86 ovmf` on Debian or Ubuntu):

```sh
gunzip edel-vm-x86_64.img.gz
qemu-system-x86_64 -machine q35,accel=kvm -m 1024 -smp 2 \
  -bios /usr/share/ovmf/OVMF.fd \
  -drive file=edel-vm-x86_64.img,format=raw,if=virtio \
  -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:2222-:22 \
  -nographic
```

The image has no user and no password: you bring them in a system file. Make one with your ssh key, and give it to the first boot on a small disk labelled `EDEL-SEED`:

```sh
cat >system.toml <<EOF
format = 1

[users.me]
admin = true
ssh_keys = ["$(cat ~/.ssh/id_ed25519.pub)"]

[users.root]
ssh_keys = ["$(cat ~/.ssh/id_ed25519.pub)"]
EOF
mkfs.vfat -C seed.img 1024 -n EDEL-SEED && mcopy -i seed.img system.toml ::/
```

Add `-drive file=seed.img,format=raw,if=virtio` to the QEMU line, start it, and log in with `ssh -p 2222 me@127.0.0.1`; `edel system export` works as `me`. Commands that change the machine, such as `edel update status`, `edel update install` and `edel system set network.hostname=mine`, need root until `doas` arrives (roadmap M6.5): log in for them with `ssh -p 2222 root@127.0.0.1`.

## On a laptop

1. Write the laptop image to a USB stick. This erases the stick. On Linux: `gunzip -c edel-laptop-x86_64.img.gz | sudo dd of=/dev/sdX bs=4M conv=fsync` with `sdX` the stick (check with `lsblk`); on Windows or macOS, a tool such as balenaEtcher writes `.img.gz` files directly.
2. Turn Secure Boot off in the laptop's firmware settings (Edel OS is not signed for it yet), and start the laptop from the stick.
3. Wait for the `edel login:` prompt. There is no user to log in as yet, and that is fine: the stick has already written a report of your hardware.
4. Turn the laptop off, put the stick into any computer, and open its small FAT partition. Attach `EFI/edel/report.toml` to an issue at <https://github.com/alimardon123/edel/issues>, with the laptop's model.

The report holds the release, the kernel, how long the start took, the memory in use, every PCI device with its driver, and the kernel log. It never holds your files.

To install from the stick onto the laptop's disk you need a login, which arrives with a later preview.

## Updates

A machine checks a release with `edel update check URL` and installs it with `edel update install URL`, where URL is a `release.toml`; the preview's is `https://github.com/alimardon123/edel/releases/download/preview/release.toml`. The update goes into the other slot and is confirmed on the next start; a start that fails three times, hangs or freezes comes back on the previous slot by itself. `edel update rollback` goes back by hand.
