#!/bin/sh
# Tests A/B updates end to end, across several restarts of one VM: an update
# installs into slot B and starts; then a broken update installs into slot
# A, and GRUB must go back to slot B on its own. The steps run inside the VM
# (ci/ab-test/files); this script boots it with the update image as a second
# disk and reads the verdict from the serial console. Snapshot mode keeps
# both images exactly as built.
set -eu
. ci/vm.sh

dir=out/ab-test
log=out/ab-test.log
run_vm "$log" 'AB-TEST: (PASS|FAIL)' "${AB_TEST_TIMEOUT:-900}" -snapshot \
	-drive if=none,id=disk0,format=raw,file="$dir/edel-vm-x86_64.img" \
	-device virtio-blk-pci,drive=disk0,bootindex=0 \
	-drive if=none,id=update,format=raw,file="$dir/edel-vm-x86_64.ext4" \
	-device virtio-blk-pci,drive=update

cat "$log"
if [ "$found" = 1 ] && grep -q 'AB-TEST: PASS' "$log"; then
	echo "PASS: update, restart and automatic fallback work (${waited}s)"
else
	echo "FAIL: see the serial log above"
	exit 1
fi
