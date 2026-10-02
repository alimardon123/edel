#!/bin/sh
# Boots the VM disk the way a real machine would (UEFI firmware, GRUB, slot
# A) and waits for the Edel OS login prompt on the serial console. The disk
# is opened in snapshot mode, so the tested image stays exactly as built.
# Then boots a copy of the disk made 2 GiB larger and checks that the data
# partition grew to fill it (roadmap M1.2).
set -eu
. ci/vm.sh

boot() {
	run_vm "$1" 'edel login:' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
		-drive if=none,id=disk0,format=raw,file="$2" \
		-device virtio-blk-pci,drive=disk0,bootindex=0
	cat "$1"
}

boot out/boot.log out/edel-vm-x86_64.img
if [ "$found" = 1 ] && grep -q 'Welcome to Edel OS' out/boot.log &&
	grep -q 'edel update: slot A confirmed' out/boot.log &&
	grep -q 'edel-data: mounted /data' out/boot.log; then
	echo "PASS: slot A booted to the Edel OS login prompt, mounted /data and was confirmed in ${waited}s"
else
	echo "FAIL: no confirmed boot to the Edel OS login prompt with /data mounted"
	exit 1
fi

grown=out/grown.img
cp --sparse=always out/edel-vm-x86_64.img "$grown"
truncate -s +2G "$grown"
boot out/boot-grown.log "$grown"
rm -f "$grown"
# busybox df -m: Filesystem 1M-blocks Used Available Use% Mounted on
size=$(tr -d '\r' <out/boot-grown.log | awk '$NF == "/data" && $2 ~ /^[0-9]+$/ { print $2 }' | tail -n 1)
if [ "$found" = 1 ] && [ "${size:-0}" -gt 2048 ]; then
	echo "PASS: on a disk 2 GiB larger, /data grew to ${size} MiB"
else
	echo "FAIL: /data did not grow past 2048 MiB (saw '${size:-nothing}')"
	exit 1
fi
