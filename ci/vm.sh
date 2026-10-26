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
# With vm_console=SOCKET the serial console is a unix socket QEMU serves
# and waits on before starting (M6.6b), so a typist (ci/serial.py) can
# answer the console; the caller starts it with its output going to LOG,
# which run_vm then leaves in place.
run_vm() {
	log=$1 pattern=$2 timeout_s=$3
	shift 3
	vars=$(mktemp)
	cp "$ovmf_vars" "$vars"
	[ -n "${vm_console:-}" ] || rm -f "$log"
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
	console="file:$serial"
	if [ -n "${vm_console:-}" ]; then
		console=chardev:console
		set -- -chardev socket,id=console,path="$vm_console",server=on,wait=on "$@"
	fi
	echo "booting with $accel, waiting up to ${timeout_s}s for: $pattern"
	qemu-system-x86_64 \
		-machine q35,accel="$accel" -m 512 -smp 2 \
		-display none -monitor none -serial "$console" \
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

# partition_out DISK N FILE: partition N of the disk image DISK, copied
# into the sparse file FILE (for debugfs); returns 1 when DISK has no
# partition N. Copied in 4 MiB blocks: 512 bytes at a time, a data
# partition's 6.6 GB took 50 s a copy, 4 MiB blocks take 4.
partition_out() {
	part_place=$(sfdisk -d "$1" 2>/dev/null | sed -n "s/^[^ ]*[^0-9]$2 : start= *\([0-9]*\), size= *\([0-9]*\),.*/\1 \2/p")
	[ -n "$part_place" ] || return 1
	dd if="$1" of="$3" bs=4M iflag=skip_bytes,count_bytes skip=$((${part_place% *} * 512)) \
		count=$((${part_place#* } * 512)) conv=sparse status=none
}

# partition_in FILE DISK N: FILE written back over partition N of DISK, in
# place and whole (no block skipped, as the partition's old bytes may not
# be zero where FILE's are); returns 1 when DISK has no partition N.
partition_in() {
	part_place=$(sfdisk -d "$2" 2>/dev/null | sed -n "s/^[^ ]*[^0-9]$3 : start= *\([0-9]*\), size= *\([0-9]*\),.*/\1 \2/p")
	[ -n "$part_place" ] || return 1
	dd if="$1" of="$2" bs=4M oflag=seek_bytes seek=$((${part_place% *} * 512)) conv=notrunc status=none
}

# main_image FILE: the gzipped desktop image (edel-x86_64.img.gz) of the
# newest push to main, which CI keeps 14 days as the artifact
# edel-try-x86_64 (docs/TRY-IT.md), into FILE; false when GH_TOKEN is
# unset or none is kept. On a push to main the run's own build has kept
# its artifact before the lanes start, so this run's (GITHUB_RUN_ID) is
# passed over: it is no older than the build it would be updated to. The upgrade test (M3.5) updates it until a
# preview exists, and desktop-memory.sh measures it.
main_image() {
	[ -n "${GH_TOKEN:-}" ] || return 1
	main_api="https://api.github.com/repos/${GITHUB_REPOSITORY:-alimardon123/edel}/actions/artifacts"
	main_id=$(curl -fsS -H "Authorization: Bearer $GH_TOKEN" "$main_api?name=edel-try-x86_64&per_page=20" | python3 -c '
import json, sys
kept = [a for a in json.load(sys.stdin)["artifacts"]
        if not a["expired"] and a["workflow_run"]["head_branch"] == "main"
        and str(a["workflow_run"]["id"]) != sys.argv[1]]
kept.sort(key=lambda a: a["created_at"], reverse=True)
print(kept[0]["id"] if kept else "")' "${GITHUB_RUN_ID:-}") || return 1
	[ -n "$main_id" ] || return 1
	curl -fsSL -H "Authorization: Bearer $GH_TOKEN" -o "$1.zip" "$main_api/$main_id/zip" || return 1
	python3 -c 'import sys, zipfile, shutil
with zipfile.ZipFile(sys.argv[1]) as z, z.open("edel-x86_64.img.gz") as src, open(sys.argv[2], "wb") as out:
    shutil.copyfileobj(src, out, 4 << 20)' "$1.zip" "$1"
	main_ok=$?
	rm -f "$1.zip"
	return "$main_ok"
}
