#!/bin/sh
# Roadmap M1.9: boots the CI-only Flatpak image (ci/flatpak/vm.toml) on a
# copy of its disk made 6 GiB larger, so the data partition has room for
# the runtimes, and waits for the spike's verdict on the serial console.
# The newest runtime must run; the oldest is reported.
set -eu
. ci/vm.sh

log=out/flatpak-test.log
grown=out/flatpak/grown.img
cp --sparse=always out/flatpak/edel-flatpak-x86_64.img "$grown"
truncate -s +6G "$grown"
run_vm "$log" 'FLATPAK-TEST: (PASS|FAIL)' "${FLATPAK_TEST_TIMEOUT:-1800}" -no-reboot -m 2048 \
	-drive if=none,id=disk0,format=raw,file="$grown" \
	-device virtio-blk-pci,drive=disk0,bootindex=0
rm -f "$grown"
grep 'FLATPAK-TEST' "$log" | tr -d '\r' || true
if [ "$found" = 1 ] && grep -q 'FLATPAK-TEST: PASS' "$log"; then
	echo "PASS: a glibc Flatpak runtime runs on the musl base (${waited}s)"
else
	cat "$log"
	echo "FAIL: see the serial log above"
	exit 1
fi
