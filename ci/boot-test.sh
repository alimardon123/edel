#!/bin/sh
# Boots the one image (images/edel.toml) the way a real machine would (UEFI
# firmware, GRUB, slot A) with no screen (`-vga none`, so the machine has no
# graphics card and starts no desktop) and waits for the Edel OS login prompt
# on the serial console. The disk is opened in snapshot mode, so the tested
# image stays exactly as built. Then boots a copy of the disk made 3 GiB
# larger and checks that the data partition grew to fill it (roadmap M1.2).
# Then boots it once more with the virtual machine's clock set to 2020 and
# checks that edel-clock sets it from the network, so serial shows 2026 or
# later (roadmap M1.13). desktop-test.sh boots the same image with a screen.
# The image has no seed, so nobody can log in, and its serial console asks
# for the first person before the login prompt (M6.6b): the first boot
# answers over a serial socket with ci/serial.py, giving `ci` a fresh ssh
# key, and ci then logs in over ssh. The other boots stop at that question.
set -eu
. ci/vm.sh

# boot LOG PATTERN IMAGE: boots IMAGE on virtio until PATTERN.
boot() {
	log_file=$1 boot_pattern=$2 image=$3
	run_vm "$log_file" "$boot_pattern" "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot -vga none \
		-drive if=none,id=disk0,format=raw,file="$image" \
		-device virtio-blk-pci,drive=disk0,bootindex=0
	cat "$log_file"
}

key=out/keys/boot-ssh
mkdir -p out/keys
rm -f "$key" "$key.pub"
ssh-keygen -q -t ed25519 -N '' -C ci@boot-test -f "$key"
console=out/boot-console.sock
rm -f out/boot.log "$console"
python3 ci/serial.py "$console" 'Username: =ci' 'ssh key instead: =' \
	"on one line: =$(cat "$key.pub")" >out/boot.log 2>&1 &
typist=$!
vm_console=$console vm_net=,hostfwd=tcp:127.0.0.1:2223-:22 keep_vm=1
boot out/boot.log 'edel login:' out/edel-x86_64.img
vm_console='' vm_net='' keep_vm=0
kill "$typist" 2>/dev/null || :
logged_in=''
if [ "$found" = 1 ]; then
	tries=0
	until logged_in=$(ssh -i "$key" -p 2223 -o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR \
		-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null ci@127.0.0.1 \
		'id -un; id -Gn; edel settings get users.ci.admin' 2>out/boot-ssh.log); do
		tries=$((tries + 1))
		[ "$tries" -lt 10 ] || break
		sleep 3
	done
fi
stop_vm
if [ "$found" = 1 ]; then
	echo "ssh as ci: $logged_in"
	case $logged_in in
	ci*admin*true*) ;;
	*)
		cat out/boot-ssh.log 2>/dev/null || :
		echo "FAIL: the person the console made, ci, could not log in over ssh as an administrator"
		exit 1
		;;
	esac
fi
grep -q 'Nobody can log in to this computer yet' out/boot.log ||
	{ echo "FAIL: the seedless image's console did not ask for its first person"; exit 1; }
grep -q 'ci can log in now' out/boot.log ||
	{ echo "FAIL: the console did not say ci can log in"; exit 1; }
if [ "$found" = 1 ] && grep -q 'Welcome to Edel OS' out/boot.log &&
	grep -q 'edel update: slot A confirmed' out/boot.log &&
	grep -q 'edel-data: mounted /data' out/boot.log &&
	grep -q "edel-data: made this device's identity key" out/boot.log &&
	grep -q "Edel OS ${EDEL_VERSION:-0.1}, channel " out/boot.log &&
	grep -q 'Starting sshd' out/boot.log &&
	grep -q 'Generating ed25519 SSH host key' out/boot.log &&
	! grep -q 'generating new host keys' out/boot.log &&
	grep -q 'edel guard: using the hardware watchdog' out/boot.log &&
	grep -q 'edel guard: this machine has no screen' out/boot.log &&
	! grep -q 'Starting greetd' out/boot.log; then
	echo "PASS: slot A of the one image booted Edel OS ${EDEL_VERSION:-0.1} with no screen and started no desktop, mounted /data, made this device's identity key, made only an ed25519 host key, started sshd, guarded the boot with the hardware watchdog and was confirmed; its console asked for the first person, made ci an administrator with the key typed there, reached the login prompt in ${waited}s, and ci logged in over ssh"
else
	echo "FAIL: no confirmed boot of Edel OS ${EDEL_VERSION:-0.1} to the login prompt with /data mounted"
	exit 1
fi

grown=out/grown.img
cp --sparse=always out/edel-x86_64.img "$grown"
truncate -s +3G "$grown"
boot out/boot-grown.log 'Username: ' "$grown"
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
	-vga none -rtc base=2020-01-01 \
	-drive if=none,id=disk0,format=raw,file=out/edel-x86_64.img \
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
