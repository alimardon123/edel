#!/bin/sh
# Tests developer mode end to end (roadmap M7.1), across several restarts
# of one VM: off, /usr is read-only and apk fails; on, a file written
# under /usr and a package apk added survive a restart; a service in the
# layer that hangs uses up the last try, the next start is a safe one
# (edel.safe) without the layer; a reset deletes the layer and turns it
# off; then a factory reset erases a person's file and a setting but
# keeps the default apps and the add-ons (M8.38). The steps run inside the VM (ci/developer-test/files); this script
# reads the verdict from the serial console. Snapshot mode keeps the
# image as it was built. apk add needs Alpine's mirror from the VM.
set -eu
. ci/vm.sh

log=out/developer-test.log
run_vm "$log" 'DEVELOPER-TEST: (PASS|FAIL)' "${DEVELOPER_TEST_TIMEOUT:-900}" -snapshot \
	-drive if=none,id=disk0,format=raw,file=out/developer-test/edel-vm-x86_64.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0
cat "$log"
log_has() {
	tr -d '\r' <"$log" | grep -q "$1"
}
# The hang's start: the guard restarted once more, as the layer was on.
if ! log_has 'edel guard: health not reached after'; then
	echo "FAIL: the hanging service in developer mode's layer did not keep the slot from being healthy"
	exit 1
fi
if ! log_has 'Edel OS: no slot has tries left; starting slot A anyway, safely'; then
	echo "FAIL: GRUB did not start safely once no slot had a try left"
	exit 1
fi
if ! log_has 'the factory reset is done'; then
	echo "FAIL: the start after edel factory-reset did not say the reset was done"
	exit 1
fi
if [ "$found" = 1 ] && log_has 'DEVELOPER-TEST: PASS'; then
	echo "PASS: developer mode keeps /usr changes and apk's packages, a safe start holds them back, and a reset deletes them; a factory reset erases a person's file and the settings, keeping the default apps and the add-ons (${waited}s)"
else
	echo "FAIL: see the serial log above"
	exit 1
fi
