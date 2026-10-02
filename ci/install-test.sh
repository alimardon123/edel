#!/bin/sh
# Roadmap M2.4: boots the install test image (ci/install-test/files) with a
# blank second disk. Its test service runs `edel install` without --yes
# (the plan, exit code 3, the disk unchanged) and then with --yes. Then the
# installed disk boots alone: it must reach the login prompt under the
# system file's hostname with slot A confirmed, and with an ssh host key
# that differs from the live image's.
set -eu
. ci/vm.sh

target=out/install-target.img
rm -f "$target"
truncate -s 3G "$target"
log=out/install-test.log
fail() {
	cat "$1"
	echo "FAIL: $2"
	exit 1
}

run_vm "$log" 'INSTALL-TEST: (installed|FAIL)' "${INSTALL_TEST_TIMEOUT:-600}" -no-reboot \
	-drive if=none,id=disk0,format=raw,file=out/install-test/edel-vm-x86_64.img,snapshot=on \
	-device virtio-blk-pci,drive=disk0,bootindex=0 \
	-drive if=none,id=disk1,format=raw,file="$target" \
	-device virtio-blk-pci,drive=disk1
grep -q 'INSTALL-TEST: installed' "$log" || fail "$log" "the live image did not install"
grep -q 'Install Edel OS on /dev/vdb' "$log" || fail "$log" "install showed no plan"
grep -q 'INSTALL-TEST: exit code 3 without --yes, disk unchanged' "$log" ||
	fail "$log" "install without --yes did not exit 3 with the disk unchanged"
live_key=$(tr -d '\r' <"$log" | sed -n 's/.*INSTALL-TEST: host key //p' | head -n 1)

boot_log=out/install-boot.log
run_vm "$boot_log" 'ci-installed login:' "${BOOT_TIMEOUT:-300}" -no-reboot \
	-drive if=none,id=disk0,format=raw,file="$target",snapshot=on \
	-device virtio-blk-pci,drive=disk0,bootindex=0
rm -f "$target"
[ "$found" = 1 ] || fail "$boot_log" "the installed disk did not reach ci-installed login:"
grep -q 'edel update: slot A confirmed' "$boot_log" || fail "$boot_log" "slot A was not confirmed"
installed_key=$(tr -d '\r' <"$boot_log" | sed -n 's/.*INSTALL-TEST: host key //p' | head -n 1)
[ -n "$live_key" ] && [ -n "$installed_key" ] && [ "$live_key" != "$installed_key" ] ||
	fail "$boot_log" "host keys: live '$live_key', installed '$installed_key'"
cat "$log" "$boot_log"
echo "PASS: installed onto a blank disk, which booted as ci-installed with slot A confirmed and its own host key (${waited}s)"
