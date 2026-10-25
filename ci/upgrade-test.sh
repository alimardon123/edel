#!/bin/sh
# The upgrade test (roadmap M3.5): the update real people run, from the
# bits they have. For every release ci/levels.toml keeps and for the
# preview, it downloads the release's disk, edel-x86_64.img.gz, boots it
# as a computer's own disk with no screen, lets that old release's own
# edel update it to this build, served from out/upgrade-test/CHANNEL/
# (ci/build.sh), and checks that slot B starts and is confirmed, the boot
# loader of the new slot taking over the old disk's EFI system partition
# when it differs (M1.8); then it restarts once more and checks slot B
# starts again under that loader.
#
# Into the old slot A it writes only two files, with debugfs (growing its
# file system to the partition when it has no room): this run's
# throwaway public key, as the old release trusts only the keys it was
# built with, and ci/upgrade-test/edel-upgrade-test, the service that
# runs the update. Until a preview is published (M3.4), the old disk is
# the newest desktop image a push to main kept (the artifact
# edel-try-x86_64, docs/TRY-IT.md), when GH_TOKEN can read it; with
# neither, the test says so and passes.
#
#   sh ci/upgrade-test.sh   logs in out/upgrade-test-N.log, one per disk
set -eu
. ci/vm.sh
. ci/names.sh

dir=out/upgrade-test
image=edel-x86_64.img.gz
repo=${GITHUB_REPOSITORY:-alimardon123/edel}

fail() {
	echo "FAIL: $*"
	exit 1
}

# sources: one line per old disk to update, "NAME URL", the kept releases
# first, then the preview.
sources() {
	awk -F'"' '/^version = / { v = $2 } /^vm_image = / { print "release-" v, $2 }' ci/levels.toml
	preview=https://github.com/$repo/releases/download/preview/$image
	if curl -fsIL -o /dev/null "$preview" 2>/dev/null; then
		echo "preview $preview"
	fi
}

# main_disk FILE: the newest desktop image a push to main kept, into
# FILE; false when GH_TOKEN is unset or none is kept.
main_disk() {
	[ -n "${GH_TOKEN:-}" ] || return 1
	api="https://api.github.com/repos/$repo/actions/artifacts"
	id=$(curl -fsS -H "Authorization: Bearer $GH_TOKEN" "$api?name=edel-try-x86_64&per_page=20" | python3 -c '
import json, sys
kept = [a for a in json.load(sys.stdin)["artifacts"]
        if not a["expired"] and a["workflow_run"]["head_branch"] == "main"]
kept.sort(key=lambda a: a["created_at"], reverse=True)
print(kept[0]["id"] if kept else "")') || return 1
	[ -n "$id" ] || return 1
	curl -fsSL -H "Authorization: Bearer $GH_TOKEN" -o "$dir/main.zip" "$api/$id/zip"
	python3 -c 'import sys, zipfile, shutil
with zipfile.ZipFile(sys.argv[1]) as z, z.open(sys.argv[2]) as src, open(sys.argv[3], "wb") as out:
    shutil.copyfileobj(src, out, 4 << 20)' "$dir/main.zip" "$image" "$1"
	rm -f "$dir/main.zip"
}

# os_value SLOT KEY: KEY's value in the slot's os-release.
os_value() {
	debugfs -R "cat /usr/lib/os-release" "$1" 2>/dev/null | sed -n "s/^$2=\"\{0,1\}\([^\"]*\)\"\{0,1\}$/\1/p" | head -n 1
}

# upgrade N NAME GZ: updates the old disk GZ to this build, logs in
# out/upgrade-test-N.log.
upgrade() {
	n=$1 name=$2 gz=$3
	disk=$dir/disk.img slot=$dir/slot-a.ext4 log=out/upgrade-test-$n.log
	echo "== $name"
	gunzip -c "$gz" | dd of="$disk" bs=4M conv=sparse status=none
	partition_out "$disk" 2 "$slot" || fail "$name: the disk has no slot A"
	channel=$(os_value "$slot" EDEL_CHANNEL)
	old=$(os_value "$slot" VERSION_ID)
	[ -f "$dir/$channel/release.toml" ] ||
		fail "$name is version $old on channel '$channel', which this build makes no update list for (ci/build.sh)"
	new=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$dir/$channel/release.toml" | head -n 1)
	[ "$(printf '%s\n%s\n' "$old" "$new" | sort -V | tail -n 1)" = "$new" ] && [ "$old" != "$new" ] ||
		fail "$name is version $old, not older than this build's $new, so no release would update it (set EDEL_VERSION for ci/build.sh)"
	# A slot is shrunk to its files (M2.5); two more need a little room.
	free=$(dumpe2fs -h "$slot" 2>/dev/null | sed -n 's/^Free blocks: *//p')
	if [ "${free:-0}" -lt 256 ]; then resize2fs -f "$slot" >/dev/null 2>&1; fi
	debugfs -w -f - "$slot" >/dev/null 2>&1 <<-EOF
		mkdir $keys_dir
		write out/keys/ci-1.pub $keys_dir/upgrade-test.pub
		sif $keys_dir/upgrade-test.pub uid 0
		sif $keys_dir/upgrade-test.pub gid 0
		write ci/upgrade-test/edel-upgrade-test /etc/init.d/edel-upgrade-test
		sif /etc/init.d/edel-upgrade-test mode 0100755
		sif /etc/init.d/edel-upgrade-test uid 0
		sif /etc/init.d/edel-upgrade-test gid 0
		symlink /etc/runlevels/default/edel-upgrade-test /etc/init.d/edel-upgrade-test
	EOF
	debugfs -R "stat /etc/init.d/edel-upgrade-test" "$slot" 2>/dev/null | grep -q 'Mode: *0755' ||
		fail "$name: the test's service could not be written into slot A"
	partition_in "$slot" "$disk" 2
	rm -f "$slot"

	python3 -m http.server 8000 --bind 127.0.0.1 --directory "$dir/$channel" >"$dir/http.log" 2>&1 &
	server=$!
	# Slot A starts, updates and restarts; slot B starts and is confirmed.
	run_vm "$log" 'edel update: slot B confirmed|UPGRADE-TEST: FAIL' "${UPGRADE_TEST_TIMEOUT:-900}" -m 1024 -vga none \
		-drive if=none,id=disk0,format=raw,file="$disk" \
		-device virtio-blk-pci,drive=disk0,bootindex=0
	kill "$server" 2>/dev/null || true
	cat "$log"
	log_has() {
		tr -d '\r' <"$log" | grep -q "$1"
	}
	log_has 'UPGRADE-TEST: installed, restarting' || fail "$name ($old, $channel) did not install this build"
	{ [ "$found" = 1 ] && log_has 'edel update: slot B confirmed'; } || fail "$name: this build did not start and confirm in slot B after the update"
	loader=$(tr -d '\r' <"$log" | sed -n 's/.*edel update: \(boot loader updated to .*\)$/\1/p' | head -n 1)

	# Once more, under the loader slot B left on the old disk.
	run_vm "$log.2" 'edel update: slot B confirmed' "${UPGRADE_TEST_TIMEOUT:-900}" -m 1024 -vga none \
		-drive if=none,id=disk0,format=raw,file="$disk" \
		-device virtio-blk-pci,drive=disk0,bootindex=0
	cat "$log.2" >>"$log"
	rm -f "$log.2" "$disk"
	[ "$found" = 1 ] || fail "$name: slot B did not start again after the update"
	echo "PASS: $name ($old, $channel) updated to this build, slot B started and was confirmed twice; ${loader:-the boot loader was the same}"
}

mkdir -p "$dir"
list=$(sources)
if [ -z "$list" ]; then
	if main_disk "$dir/main.img.gz"; then
		list="main-build $dir/main.img.gz"
	else
		echo "SKIP: no release is kept and no preview is published (M3.4), and no desktop image of main could be read (GH_TOKEN), so there is nothing to upgrade from"
		exit 0
	fi
fi
n=0
echo "$list" | while read -r name url; do
	n=$((n + 1))
	case "$url" in
	https://*)
		curl -fsSL -o "$dir/old.img.gz" "$url" || fail "$name: could not download $url"
		upgrade "$n" "$name" "$dir/old.img.gz"
		;;
	*) upgrade "$n" "$name" "$url" ;;
	esac
	rm -f "$dir/old.img.gz" "$dir/main.img.gz"
done
echo "PASS: every kept release updated to this build"
