#!/bin/sh
# Boots the VM disk the way a real machine would (UEFI firmware, GRUB, slot
# A) and waits for the Edel OS login prompt on the serial console. The disk
# is opened in snapshot mode, so the tested image stays exactly as built.
# Then boots a copy of the disk made 3 GiB larger and checks that the data
# partition grew to fill it (roadmap M1.2). Then boots it once more with
# the virtual machine's clock set to 2020 and checks that edel-clock sets it
# from the network, so serial shows 2026 or later (roadmap M1.13). The
# desktop image is booted as a USB stick by desktop-test.sh live (M3.6).
set -eu
. ci/vm.sh

# boot LOG IMAGE [DEVICE ARGUMENTS...]: the disk is drive disk0, on virtio
# unless the arguments attach it otherwise.
boot() {
	log_file=$1 image=$2
	shift 2
	[ "$#" -gt 0 ] || set -- -device virtio-blk-pci,drive=disk0,bootindex=0
	run_vm "$log_file" 'edel login:' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
		-drive if=none,id=disk0,format=raw,file="$image" "$@"
	cat "$log_file"
}

boot out/boot.log out/edel-vm-x86_64.img
if [ "$found" = 1 ] && grep -q 'Welcome to Edel OS' out/boot.log &&
	grep -q 'edel update: slot A confirmed' out/boot.log &&
	grep -q 'edel-data: mounted /data' out/boot.log &&
	grep -q "Edel OS ${EDEL_VERSION:-0.1}, channel " out/boot.log &&
	grep -q 'Starting sshd' out/boot.log &&
	grep -q 'Generating ed25519 SSH host key' out/boot.log &&
	! grep -q 'generating new host keys' out/boot.log &&
	grep -q 'edel guard: using the hardware watchdog' out/boot.log; then
	echo "PASS: slot A booted Edel OS ${EDEL_VERSION:-0.1} to the login prompt, mounted /data, made only an ed25519 host key, started sshd, guarded the boot with the hardware watchdog and was confirmed in ${waited}s"
else
	echo "FAIL: no confirmed boot of Edel OS ${EDEL_VERSION:-0.1} to the login prompt with /data mounted"
	exit 1
fi

grown=out/grown.img
cp --sparse=always out/edel-vm-x86_64.img "$grown"
truncate -s +3G "$grown"
boot out/boot-grown.log "$grown"
rm -f "$grown"
# busybox df -m: Filesystem 1M-blocks Used Available Use% Mounted on
size=$(tr -d '\r' <out/boot-grown.log | awk '$NF == "/data" && $2 ~ /^[0-9]+$/ { print $2 }' | tail -n 1)
if [ "$found" = 1 ] && [ "${size:-0}" -gt 2048 ]; then
	echo "PASS: on a disk 3 GiB larger, /data grew to ${size} MiB"
else
	echo "FAIL: /data did not grow past 2048 MiB (saw '${size:-nothing}')"
	exit 1
fi

# A machine whose clock battery died starts in the past. edel-clock runs in
# the background once the network is up (QEMU's user network reaches the
# runner's), after the login prompt, so wait for its whole line (it ends with the earlier
# time), not the prompt.
run_vm out/boot-clock.log 'edel-clock: .*(it said [0-9: -]* UTC\)|check the network)' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
	-rtc base=2020-01-01 \
	-drive if=none,id=disk0,format=raw,file=out/edel-vm-x86_64.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0
cat out/boot-clock.log
clock_line=$(tr -d '\r' <out/boot-clock.log | grep 'edel-clock: ' | tail -n 1 || true)
# The year 2026 or later: 2026 to 2099.
if [ "$found" = 1 ] &&
	echo "$clock_line" | grep -qE 'edel-clock: set the clock from .*: 20(2[6-9]|[3-9][0-9])-[0-9][0-9]-[0-9][0-9] '; then
	echo "PASS: with the clock set to 2020, edel-clock set it from the network in ${waited}s: ${clock_line}"
else
	echo "FAIL: expected a line 'edel-clock: set the clock from SERVER: YYYY-MM-DD HH:MM UTC' with a year of 2026 or later on serial within ${BOOT_TIMEOUT:-300}s after booting with -rtc base=2020-01-01, saw '${clock_line:-no edel-clock line}'"
	exit 1
fi
