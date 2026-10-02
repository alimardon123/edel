#!/bin/sh
# Boots the VM disk the way a real machine would (UEFI firmware, GRUB, slot
# A) and waits for the Edel OS login prompt on the serial console. The disk
# is opened in snapshot mode, so the tested image stays exactly as built.
set -eu
. ci/vm.sh

log=out/boot.log
run_vm "$log" 'edel login:' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
	-drive if=none,id=disk0,format=raw,file=out/edel-vm-x86_64.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0

cat "$log"
if [ "$found" = 1 ] && grep -q 'Welcome to Edel OS' "$log" &&
	grep -q 'edel update: slot A confirmed' "$log"; then
	echo "PASS: slot A booted to the Edel OS login prompt and was confirmed in ${waited}s"
else
	echo "FAIL: no confirmed boot to the Edel OS login prompt"
	exit 1
fi
