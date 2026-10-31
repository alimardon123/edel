#!/bin/sh
# How light Edel OS is beside other light systems (roadmap M10.7): one
# system's server image, booted from its own download in the same QEMU
# machine as the others, with no screen. Run by the workflow "Lightness"
# when someone starts it or a release is published, never on a change.
#
#   sh ci/lightness.sh SYSTEM   edel (main's newest image, read with
#                               GH_TOKEN), alpine (Alpine's virt image
#                               of its latest stable branch) or debian
#                               (Debian 13's nocloud image)
#
# Every system gets the same machine: q35 with KVM, 2 processors, 1 GiB,
# UEFI, a user network, no screen (so Edel OS starts as a server and asks
# for its first person on the console) and a virtio balloon device,
# through which the guest's own kernel reports its memory. Nothing is
# installed or changed inside any guest. The numbers: the download in
# MiB; the seconds from QEMU's start to the console's first prompt
# (Edel OS's "Username:", the others' "login:"); and memory in use
# (MemTotal less MemAvailable, the number Edel OS's own budgets use), the
# median of the minute after two minutes at that prompt. Results in
# out/lightness/: SYSTEM.serial, SYSTEM.samples and a table line in
# results.md.
set -eu
. ci/vm.sh

system=${1:?say which system: edel, alpine or debian}
dir=out/lightness
alpine=https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64
debian=https://cloud.debian.org/images/cloud/trixie/latest
mkdir -p "$dir"

fail() {
	echo "FAIL: $*"
	exit 1
}

# fetch URL FILE SUM TOOL: URL into FILE, which must hash to SUM with TOOL.
fetch() {
	curl -fsSL --retry 3 -o "$2" "$1" || fail "could not download $1"
	echo "$3  $2" | "$4" -c --quiet - || fail "$1 does not match its published sum"
}

mib() {
	echo "$(($(stat -c %s "$1") / 1048576))"
}

# The system's start medium: QEMU arguments in media, its name in label,
# its download's size in download.
case "$system" in
edel)
	main_image "$dir/edel.img.gz" || fail "no image of main could be read (GH_TOKEN)"
	download=$(mib "$dir/edel.img.gz")
	gunzip -c "$dir/edel.img.gz" | dd of="$dir/edel.img" bs=4M conv=sparse status=none
	rm "$dir/edel.img.gz"
	slot=$dir/slot.ext4
	partition_out "$dir/edel.img" 2 "$slot"
	label="Edel OS $(debugfs -R 'cat /usr/lib/os-release' "$slot" 2>/dev/null | sed -n 's/^VERSION_ID="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p')"
	rm -f "$slot"
	media="-drive if=none,id=disk0,format=raw,file=$dir/edel.img,snapshot=on -device virtio-blk-pci,drive=disk0,bootindex=0"
	prompt='Username:'
	;;
alpine)
	file=$(curl -fsS "$alpine/latest-releases.yaml" | sed -n 's/^ *iso: *\(alpine-virt-[^ ]*-x86_64\.iso\)$/\1/p' | head -n 1)
	[ -n "$file" ] || fail "no virt image named in $alpine/latest-releases.yaml"
	fetch "$alpine/$file" "$dir/alpine.iso" "$(curl -fsS "$alpine/$file.sha256" | cut -d ' ' -f 1)" sha256sum
	download=$(mib "$dir/alpine.iso")
	label="Alpine $(echo "$file" | sed 's/^alpine-virt-\(.*\)-x86_64\.iso$/\1/') (virt)"
	media="-drive if=none,id=cd,media=cdrom,readonly=on,file=$dir/alpine.iso -device ide-cd,drive=cd,bootindex=0"
	prompt='login:'
	;;
debian)
	file=debian-13-nocloud-amd64.qcow2
	fetch "$debian/$file" "$dir/debian.qcow2" "$(curl -fsS "$debian/SHA512SUMS" | awk -v f="$file" '$2 == f { print $1 }')" sha512sum
	download=$(mib "$dir/debian.qcow2")
	label="Debian 13 (nocloud)"
	media="-drive if=none,id=disk0,format=qcow2,file=$dir/debian.qcow2,snapshot=on -device virtio-blk-pci,drive=disk0,bootindex=0"
	prompt='login:'
	;;
*) fail "there is no system $system; the systems are edel, alpine and debian" ;;
esac

# median FILE FROM TO: the median in-use number of FILE's samples (lines
# of "SECONDS IN_USE TOTAL") from second FROM to TO, or "none".
median() {
	awk -v from="$2" -v to="$3" '$1 >= from && $1 <= to && $2 != "none" { print $2 }' "$1" | sort -n |
		awk '{ v[NR] = $1 } END { if (NR == 0) print "none"; else print v[int((NR + 1) / 2)] }'
}

run=$dir/$system
export QMP="$dir/qmp.sock"
rm -f "$QMP" "$run.samples" "$run.serial"
cp "$ovmf_vars" "$dir/vars.fd"
start=$(date +%s)
# shellcheck disable=SC2086 # media holds several arguments
qemu-system-x86_64 -machine q35,accel="$accel" -m 1024 -smp 2 \
	-display none -vga none \
	-drive if=pflash,format=raw,readonly=on,file="$ovmf_code" \
	-drive if=pflash,format=raw,file="$dir/vars.fd" \
	-netdev user,id=net0 -device virtio-net-pci,netdev=net0,romfile= \
	-device virtio-balloon-pci,id=balloon0 \
	-qmp unix:"$QMP",server=on,wait=off -serial file:"$run.serial" \
	$media &
qemu=$!
trap 'kill "$qemu" 2>/dev/null || true' EXIT
boot=none
while [ $(($(date +%s) - start)) -lt 300 ]; do
	if tr -d '\r' <"$run.serial" 2>/dev/null | grep -q "$prompt"; then
		boot=$(($(date +%s) - start))
		break
	fi
	sleep 1
done
[ "$boot" != none ] || echo "$system: no $prompt on the console in 300 s; see $run.serial"
at=$(date +%s)
while [ $(($(date +%s) - at)) -lt 180 ]; do
	sleep 10
	echo "$(($(date +%s) - at)) $(python3 ci/qmp.py memory 2>/dev/null || echo none)" >>"$run.samples"
done
kill "$qemu" 2>/dev/null || true
wait "$qemu" 2>/dev/null || true
idle=$(median "$run.samples" 120 185)
echo "| $label | $download | $boot | $idle |" | tee -a "$dir/results.md"
rm -f "$dir/edel.img" "$dir/alpine.iso" "$dir/debian.qcow2"
