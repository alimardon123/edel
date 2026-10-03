# Shared by the VM tests (sourced, not run): uses KVM when it can, finds the
# UEFI firmware, and runs QEMU until a line shows up on the serial console.

accel=tcg
[ -w /dev/kvm ] && accel=kvm

# OVMF is the UEFI firmware for QEMU; distributions keep it in different places.
ovmf_code=''
for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd \
	/usr/share/ovmf/x64/OVMF_CODE.fd; do
	if [ -f "$f" ]; then
		ovmf_code=$f
		break
	fi
done
if [ -z "$ovmf_code" ]; then
	echo "no OVMF firmware found; install the ovmf package" >&2
	exit 1
fi
ovmf_vars=$(echo "$ovmf_code" | sed 's/CODE/VARS/')

# run_vm LOG PATTERN TIMEOUT [QEMU ARGUMENTS...]
# Boots a VM with its serial console written to LOG until PATTERN (an
# extended regex) appears, QEMU exits, or TIMEOUT seconds pass. Sets found
# to 1 if PATTERN appeared, and waited to the seconds it took. vm_net adds
# options to the user network, such as ",hostfwd=tcp:127.0.0.1:2222-:22";
# with keep_vm=1 a VM that showed PATTERN keeps running until stop_vm.
# With vm_stamp=1 each finished line of LOG starts with the seconds since
# QEMU started, taken on the host as the line arrives, so the services of
# a boot can be timed without changing the image (the budgets use it).
run_vm() {
	log=$1 pattern=$2 timeout_s=$3
	shift 3
	vars=$(mktemp)
	cp "$ovmf_vars" "$vars"
	rm -f "$log"
	serial=$log stamper=''
	if [ "${vm_stamp:-0}" = 1 ]; then
		serial=$(mktemp -u)
		mkfifo "$serial"
		python3 -u -c 'import sys, time
start = time.monotonic()
for line in sys.stdin.buffer:
    sys.stdout.buffer.write(b"%8.3f " % (time.monotonic() - start) + line)
    sys.stdout.buffer.flush()' <"$serial" >"$log" &
		stamper=$!
	fi
	echo "booting with $accel, waiting up to ${timeout_s}s for: $pattern"
	qemu-system-x86_64 \
		-machine q35,accel="$accel" -m 512 -smp 2 \
		-display none -monitor none -serial file:"$serial" \
		-drive if=pflash,format=raw,readonly=on,file="$ovmf_code" \
		-drive if=pflash,format=raw,file="$vars" \
		-netdev user,id=net0"${vm_net:-}" -device virtio-net-pci,netdev=net0,romfile= \
		-device i6300esb -action watchdog=reset \
		"$@" &
	qemu=$!
	found=0
	waited=0
	while [ "$waited" -lt "$timeout_s" ]; do
		if grep -qE "$pattern" "$log" 2>/dev/null; then
			found=1
			break
		fi
		kill -0 "$qemu" 2>/dev/null || break
		sleep 1
		waited=$((waited + 1))
	done
	# QEMU may have printed it and exited between two looks.
	if [ "$found" = 0 ] && grep -qE "$pattern" "$log" 2>/dev/null; then
		found=1
	fi
	if [ "${keep_vm:-0}" = 1 ] && [ "$found" = 1 ]; then
		return 0
	fi
	stop_vm
}

# stop_vm: stops the VM run_vm started.
stop_vm() {
	kill "$qemu" 2>/dev/null || true
	wait "$qemu" 2>/dev/null || true
	rm -f "$vars"
	if [ -n "$stamper" ]; then
		# QEMU closed the pipe, so the stamper reads its last line and
		# ends; one still waiting to open it (QEMU never started) is
		# stopped after 2 s.
		(sleep 2 && kill "$stamper" 2>/dev/null) &
		wait "$stamper" 2>/dev/null || true
		rm -f "$serial"
	fi
}
