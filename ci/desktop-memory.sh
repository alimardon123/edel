#!/bin/sh
# Desktop memory beside other systems (roadmap M8.36, the release
# readiness audit of 2026-10-25): the live desktop of one system, booted
# from its own download in the same QEMU machine as the others, at 1.5 GiB
# and at 1 GiB of memory, in the manner of Bryan Quigley's 2026 live-stick
# test. Run by the workflow "Desktop memory" when someone starts it, never
# on a schedule or a change.
#
#   sh ci/desktop-memory.sh SYSTEM   edel (main's newest desktop image, read
#                                    with GH_TOKEN), ubuntu or fedora
#
# Every system gets the same machine: q35 with KVM, 2 processors, the
# standard VGA screen drawn in software, UEFI, a user network and a
# virtio balloon device, through which the guest's own kernel reports its
# memory; nothing is installed or changed inside any guest. Memory in use
# is MemTotal less MemAvailable, the number Edel OS's own memory budgets
# use. For each size the VM runs 420 s with nobody using it but a press of
# Shift every 10 s from second 200, which keeps every screen awake and
# unlocked (the desktop's idle number is the median of the last minute);
# then Firefox opens on a page that plays a video served from this host,
# the way a person opens it on each system (below), and the median of the
# last minute of 240 s
# more is the number with Firefox. Screenshots of each moment show whether
# the desktop, the browser and the video really came; a number without its
# picture is not a result. Results in out/desktop-memory/: SYSTEM-SIZE.png
# (idle), SYSTEM-SIZE-opened.png, SYSTEM-SIZE-firefox.png,
# SYSTEM-SIZE.samples and a table line each in
# results.md.
set -eu
. ci/vm.sh

system=${1:?say which system: edel, ubuntu or fedora}
dir=out/desktop-memory
ubuntu=https://releases.ubuntu.com/26.04
fedora=https://download.fedoraproject.org/pub/fedora/linux/releases/44/Workstation/x86_64/iso
mkdir -p "$dir/www"

fail() {
	echo "FAIL: $*"
	exit 1
}

# fetch URL FILE SUM: URL into FILE, which must hash to SUM.
fetch() {
	curl -fsSL --retry 3 -o "$2" "$1" || fail "could not download $1"
	echo "$3  $2" | sha256sum -c --quiet - || fail "$1 does not match its published sha256"
}

# The system's start medium: QEMU arguments in media, its name in label.
case "$system" in
edel)
	main_image "$dir/edel.img.gz" || fail "no desktop image of main could be read (GH_TOKEN)"
	gunzip -c "$dir/edel.img.gz" | dd of="$dir/edel.img" bs=4M conv=sparse status=none
	rm "$dir/edel.img.gz"
	slot=$dir/slot.ext4
	partition_out "$dir/edel.img" 2 "$slot"
	label="Edel OS $(debugfs -R 'cat /usr/lib/os-release' "$slot" 2>/dev/null | sed -n 's/^VERSION_ID="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p')"
	rm -f "$slot"
	# As a USB stick, which starts the live session (M3.6); snapshot, so
	# both sizes start it new.
	media="-drive if=none,id=stick,format=raw,file=$dir/edel.img,snapshot=on -device usb-storage,bus=xhci.0,drive=stick,removable=on,bootindex=0"
	;;
ubuntu)
	line=$(curl -fsS "$ubuntu/SHA256SUMS" | grep 'desktop-amd64\.iso$' | sort -V -k2 | tail -n 1)
	file=${line##*\*}
	fetch "$ubuntu/$file" "$dir/ubuntu.iso" "${line%% *}"
	label="Ubuntu ${file#ubuntu-}"
	label=${label%-desktop-amd64.iso}
	media="-drive if=none,id=cd,media=cdrom,readonly=on,file=$dir/ubuntu.iso -device ide-cd,drive=cd,bootindex=0"
	;;
fedora)
	sums=$(curl -fsSL "$fedora/" | grep -o 'Fedora-Workstation-[^"]*-CHECKSUM' | head -n 1)
	[ -n "$sums" ] || fail "no CHECKSUM file at $fedora"
	line=$(curl -fsSL "$fedora/$sums" | grep '^SHA256 (Fedora-Workstation-Live-.*\.iso)')
	file=$(echo "$line" | sed 's/^SHA256 (\(.*\)) = .*/\1/')
	fetch "$fedora/$file" "$dir/fedora.iso" "${line##* }"
	label="Fedora Workstation ${file#Fedora-Workstation-Live-}"
	label=${label%.x86_64.iso}
	media="-drive if=none,id=cd,media=cdrom,readonly=on,file=$dir/fedora.iso -device ide-cd,drive=cd,bootindex=0"
	;;
*) fail "there is no system $system; the systems are edel, ubuntu and fedora" ;;
esac

# The page Firefox opens: a video looping, made here so every system
# plays the same bytes.
if [ ! -f "$dir/www/video.webm" ]; then
	ffmpeg -loglevel error -f lavfi -i testsrc2=size=1280x720:rate=30 -t 20 \
		-c:v libvpx-vp9 -deadline realtime -b:v 1M "$dir/www/video.webm"
	printf '<!doctype html><title>video</title><video src="video.webm" autoplay muted loop style="width:100%%"></video>\n' >"$dir/www/index.html"
fi
python3 -m http.server 8000 --directory "$dir/www" >"$dir/http.log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT

# median FILE FROM TO: the median in-use number of FILE's samples (lines
# of "SECONDS IN_USE TOTAL") from second FROM to TO, or "none".
median() {
	awk -v from="$2" -v to="$3" '$1 >= from && $1 <= to && $2 != "none" { print $2 }' "$1" | sort -n |
		awk '{ v[NR] = $1 } END { if (NR == 0) print "none"; else print v[int((NR + 1) / 2)] }'
}

# measure MIB: one run at MIB of memory.
measure() {
	size=$1 run=$dir/$system-$1
	export QMP="$dir/qmp.sock"
	rm -f "$QMP" "$run.samples"
	cp "$ovmf_vars" "$dir/vars.fd"
	# shellcheck disable=SC2086 # media holds several arguments
	qemu-system-x86_64 -machine q35,accel="$accel" -m "$size" -smp 2 \
		-display none -vga std \
		-drive if=pflash,format=raw,readonly=on,file="$ovmf_code" \
		-drive if=pflash,format=raw,file="$dir/vars.fd" \
		-netdev user,id=net0 -device virtio-net-pci,netdev=net0,romfile= \
		-device virtio-balloon-pci,id=balloon0 -device qemu-xhci,id=xhci \
		-qmp unix:"$QMP",server=on,wait=off -serial file:"$run.serial" \
		$media &
	qemu=$!
	start=$(date +%s)
	# sample UNTIL: a sample every 10 s until second UNTIL of the run;
	# from second 200, when every desktop is up, each after a press of
	# Shift, as a person reading the screen would make, so no system dims,
	# blanks or locks its screen (none is pressed while a system starts).
	sample() {
		while [ $(($(date +%s) - start)) -lt "$1" ]; do
			sleep 10
			if [ $(($(date +%s) - start)) -ge 200 ]; then
				python3 ci/qmp.py key shift >/dev/null 2>&1 || true
			fi
			echo "$(($(date +%s) - start)) $(python3 ci/qmp.py memory 2>/dev/null || echo none)" >>"$run.samples"
		done
	}
	sample 420
	python3 ci/qmp.py screendump "$run.png" || true
	# Firefox, the way a person opens it on each system: GNOME's Alt+F2
	# runs it with the page, as its search lost the keys to the welcome
	# windows Ubuntu and Fedora open over a live session; Edel OS's
	# launcher opens it, Tab and Return take Firefox's first-run notice,
	# and the address bar the page.
	case "$system" in
	edel)
		python3 ci/qmp.py key meta_l || true
		sleep 3
		python3 ci/qmp.py type firefox || true
		sleep 5
		python3 ci/qmp.py key ret || true
		sample 540
		python3 ci/qmp.py screendump "$run-opened.png" || true
		python3 ci/qmp.py key tab || true
		sleep 1
		python3 ci/qmp.py key ret || true
		sleep 5
		python3 ci/qmp.py key ctrl-l || true
		sleep 2
		python3 ci/qmp.py type 'http://10.0.2.2:8000/\n' || true
		;;
	*)
		python3 ci/qmp.py key alt-f2 || true
		sleep 5
		python3 ci/qmp.py type 'firefox http://10.0.2.2:8000/\n' || true
		sample 540
		python3 ci/qmp.py screendump "$run-opened.png" || true
		;;
	esac
	sample 660
	python3 ci/qmp.py screendump "$run-firefox.png" || true
	state=$(python3 ci/qmp.py status 2>/dev/null || echo stopped)
	kill "$qemu" 2>/dev/null || true
	wait "$qemu" 2>/dev/null || true
	idle=$(median "$run.samples" 360 425)
	firefox=$(median "$run.samples" 600 665)
	peak=$(awk '$2 != "none" { print $2 }' "$run.samples" | sort -n | tail -n 1)
	echo "| $label | $size MiB | $idle | $firefox | ${peak:-none} | $state |" | tee -a "$dir/results.md"
}

measure 1536
measure 1024
rm -f "$dir/edel.img" "$dir/ubuntu.iso" "$dir/fedora.iso"
