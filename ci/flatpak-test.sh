#!/bin/sh
# Roadmap M1.9: boots the CI-only Flatpak image (ci/flatpak/vm.toml) on a
# copy of its disk made 6 GiB larger, so the data partition has room for
# the runtimes, and waits for the spike's verdict on the serial console.
# The newest runtime must run; the oldest is reported. Flatpak must be
# 1.15 or later, which marks its apps with a security context (M5.22).
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
if ! grep -q 'marks its apps with a security context: yes' "$log"; then
	echo "FAIL: the image's Flatpak is older than 1.15, so its apps would see the shell's protocols (M5.22)"
	exit 1
fi
if [ "$found" = 1 ] && grep -q 'FLATPAK-TEST: PASS' "$log"; then
	echo "PASS: a glibc Flatpak runtime runs on the musl base, and Flatpak marks its apps with a security context (${waited}s)"
else
	cat "$log"
	echo "FAIL: see the serial log above"
	exit 1
fi
