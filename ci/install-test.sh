#!/bin/sh
# Roadmap M2.4: boots the install test image (ci/install-test/files) with a
# blank second disk. Its test service runs `edel install` without --yes
# (the plan, exit code 3, the disk unchanged) and then with --yes. The
# installed disk, out/install-target.img, is what ci/ab-test.sh boots next
# (M2.5); this script saves the live image's ssh host key for it.
set -eu
. ci/vm.sh

target=out/install-target.img
rm -f "$target"
# Two 4096 MiB slots, the ESP and room for data (M3.3b). Zeros written to
# it stay holes, so the copied slot costs the runner no disk.
truncate -s 10G "$target"
log=out/install-test.log
fail() {
	cat "$1"
	echo "FAIL: $2"
	exit 1
}

run_vm "$log" 'INSTALL-TEST: (installed|FAIL)' "${INSTALL_TEST_TIMEOUT:-600}" -no-reboot \
	-drive if=none,id=disk0,format=raw,file=out/install-test/edel-vm-x86_64.img,snapshot=on \
	-device virtio-blk-pci,drive=disk0,bootindex=0 \
	-drive if=none,id=disk1,format=raw,file="$target",discard=unmap,detect-zeroes=unmap \
	-device virtio-blk-pci,drive=disk1
grep -q 'INSTALL-TEST: installed' "$log" || fail "$log" "the live image did not install"
grep -q 'Install Edel OS on /dev/vdb' "$log" || fail "$log" "install showed no plan"
grep -q 'INSTALL-TEST: exit code 3 without --yes, disk unchanged' "$log" ||
	fail "$log" "install without --yes did not exit 3 with the disk unchanged"
tr -d '\r' <"$log" | sed -n 's/.*INSTALL-TEST: host key //p' | head -n 1 >out/install-live-key
[ -s out/install-live-key ] || fail "$log" "the live image printed no ssh host key"
cat "$log"
echo "PASS: install showed its plan and exited 3 without --yes, then installed onto the blank disk (${waited}s)"
