#!/bin/sh
# Roadmap M7.3: fleet images and a stick that installs itself
# (docs/FLEET.md). build.sh made out/fleet-a and out/fleet-b, the VM image
# with ci/fleet/base.toml under device-a.toml and device-b.toml, and
# out/fleet-seed.img, an EDEL-SEED volume holding ci/fleet/erase.toml.
# 1. Device A boots to its own hostname.
# 2. Device B's stick, whose [install] says disk = "auto", installs onto a
#    blank second disk with nobody at it: the slot confirmed before the
#    first write, then it powers off.
# 3. That disk boots alone to device B's hostname, its settings file
#    without [install].
# 4. A fresh stick refuses a partitioned disk, which stays as it was.
# 5. A fresh stick seeded with erase.toml (the disk by its by-id name,
#    erase = "yes", then = "reboot") counts down, erases the installed disk
#    and restarts; started again it says it already installed there, and
#    the disk stays as it was.
set -eu
. ci/vm.sh
. ci/names.sh

dir=out/fleet
rm -rf "$dir"
mkdir -p "$dir"
fail() {
	cat "$1"
	echo "FAIL: $2"
	exit 1
}
# The installed disk's serial gives it the by-id name virtio-fleet-target.
target_drive() {
	echo "-drive if=none,id=disk1,format=raw,file=$1,discard=unmap,detect-zeroes=unmap -device virtio-blk-pci,drive=disk1,serial=fleet-target"
}
stick_drive() {
	echo "-drive if=none,id=disk0,format=raw,file=$1 -device virtio-blk-pci,drive=disk0,bootindex=0"
}
fresh_stick() {
	rm -f "$dir/stick.img"
	cp --sparse=always out/fleet-b/edel-vm-x86_64.img "$dir/stick.img"
}
# The data partition's settings file of the disk $1.
settings_of() {
	part=$(sfdisk -d "$1" 2>/dev/null | sed -n 's/^[^ ]*4 : start= *\([0-9]*\), size= *\([0-9]*\),.*/\1 \2/p')
	[ -n "$part" ] || return 1
	dd if="$1" of="$dir/data.img" bs=512 skip="${part% *}" count="${part#* }" conv=sparse status=none
	e2fsck -fy "$dir/data.img" >/dev/null 2>&1 || true
	debugfs -R "cat ${data_dir#/data}/$settings_name" "$dir/data.img" 2>/dev/null
}
# The first line of $1 that holds $2, by number; empty when none does.
line_of() {
	tr -d '\r' <"$1" | grep -n -F "$2" | head -n 1 | cut -d: -f1
}

# 1. Device A.
log=out/fleet-a.log
run_vm "$log" 'fleet-a login:' "${BOOT_TIMEOUT:-300}" -no-reboot \
	-drive if=none,id=disk0,format=raw,file=out/fleet-a/edel-vm-x86_64.img,snapshot=on \
	-device virtio-blk-pci,drive=disk0,bootindex=0
[ "$found" = 1 ] || fail "$log" "device A did not boot to its hostname fleet-a"
echo "device A booted to fleet-a (${waited}s)"

# 2. Device B's stick installs itself onto a blank disk.
target="$dir/target.img"
truncate -s 10G "$target"
fresh_stick
log=out/fleet-install.log
# shellcheck disable=SC2046
run_vm "$log" 'installed to virtio-fleet-target; powering off' "${INSTALL_TEST_TIMEOUT:-600}" -no-reboot \
	$(stick_drive "$dir/stick.img") $(target_drive "$target")
[ "$found" = 1 ] || fail "$log" "device B's stick did not install itself"
confirmed=$(line_of "$log" 'slot A confirmed')
writing=$(line_of "$log" 'installing onto /dev/vdb (virtio-fleet-target)')
[ -n "$confirmed" ] && [ -n "$writing" ] && [ "$confirmed" -lt "$writing" ] ||
	fail "$log" "the slot was not confirmed before the install began (lines $confirmed and $writing)"
installed=$(settings_of "$target") || fail "$log" "the installed disk has no data partition"
case $installed in
*'hostname = "fleet-b"'*) ;;
*) fail "$log" "the installed settings file lacks the hostname fleet-b: $installed" ;;
esac
case $installed in
*install*) fail "$log" "the installed settings file still holds [install]: $installed" ;;
esac
echo "device B's stick installed itself after its slot was confirmed (${waited}s)"

# 3. The installed disk boots alone.
log=out/fleet-installed.log
run_vm "$log" 'fleet-b login:' "${BOOT_TIMEOUT:-300}" -no-reboot \
	-drive if=none,id=disk0,format=raw,file="$target",snapshot=on \
	-device virtio-blk-pci,drive=disk0,bootindex=0
[ "$found" = 1 ] || fail "$log" "the installed disk did not boot to fleet-b"
if tr -d '\r' <"$log" | grep -q 'edel install: installing'; then
	fail "$log" "the installed machine installed itself again"
fi
echo "the installed disk booted alone to fleet-b (${waited}s)"

# 4. A partitioned disk is refused and left alone.
other="$dir/partitioned.img"
truncate -s 10G "$other"
echo 'type=83' | sfdisk -q "$other"
before=$(sha256sum "$other" | cut -d' ' -f1)
fresh_stick
log=out/fleet-refused.log
# shellcheck disable=SC2046
run_vm "$log" 'install refused: /dev/vdb is not empty' "${BOOT_TIMEOUT:-300}" -no-reboot \
	$(stick_drive "$dir/stick.img") $(target_drive "$other")
[ "$found" = 1 ] || fail "$log" "the stick did not refuse a partitioned disk"
[ "$(sha256sum "$other" | cut -d' ' -f1)" = "$before" ] || fail "$log" "the refused disk changed"
rm -f "$other"
echo "a partitioned disk was refused and left as it was (${waited}s)"

# 5. erase = "yes" erases the installed disk once, by its by-id name.
fresh_stick
log=out/fleet-erase.log
# shellcheck disable=SC2046
run_vm "$log" 'installed to virtio-fleet-target; restarting' "${INSTALL_TEST_TIMEOUT:-600}" -no-reboot \
	$(stick_drive "$dir/stick.img") $(target_drive "$target") \
	-drive if=none,id=seed,format=raw,file=out/fleet-seed.img,snapshot=on -device virtio-blk-pci,drive=seed
[ "$found" = 1 ] || fail "$log" "the seeded stick did not erase and install the disk"
grep -q 'erasing /dev/vdb (virtio-fleet-target) in 10 s' "$log" || fail "$log" "no countdown before erasing"
installed=$(settings_of "$target") || fail "$log" "the erased disk has no data partition"
case $installed in
*'hostname = "fleet-erased"'*) ;;
*) fail "$log" "the erased disk's settings file is not the seed's: $installed" ;;
esac
before=$(sha256sum "$target" | cut -d' ' -f1)
log=out/fleet-again.log
# shellcheck disable=SC2046
run_vm "$log" 'already installed to virtio-fleet-target' "${BOOT_TIMEOUT:-300}" -no-reboot \
	$(stick_drive "$dir/stick.img") $(target_drive "$target")
[ "$found" = 1 ] || fail "$log" "the stick did not say it already installed the disk"
[ "$(sha256sum "$target" | cut -d' ' -f1)" = "$before" ] || fail "$log" "the disk changed after the stick started again"
rm -rf "$dir"
echo "PASS: device A booted to fleet-a; device B's stick installed itself onto a blank disk once its slot was confirmed, and that disk booted alone to fleet-b without [install]; a partitioned disk was refused unchanged; erase = \"yes\" counted down, erased and reinstalled the disk by its by-id name, and the stick started again left it alone"
