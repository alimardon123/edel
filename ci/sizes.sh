#!/bin/sh
# Checks the budgets in ci/budgets.toml (roadmap M3.2) and writes them, with
# the image sizes, as Markdown tables to the run summary. Boot time, memory
# in use and root size are the median of three boots of the one image
# (images/edel.toml, M8.17) with no screen (`-vga none`) and without the
# add-ons its disk carries (M8.17c), read from the line
# edel-boot-ok prints; the image sizes are the container's, the update
# image's and the stick's (M8.17f). Fails the job when a measurement is
# over its budget; CI runs it even after a failure, and then it only
# reports what was built.
#
#   sh ci/sizes.sh        everything
#   sh ci/sizes.sh sizes  the sizes only, as the build job runs it
#   sh ci/sizes.sh boots  the boots only, as the system lane runs it first,
#                         alone on its runner, so the lanes need not wait
#                         for the boots to start
set -eu
part=${1:-all}
case "$part" in
all | sizes | boots) ;;
*)
	echo "sizes.sh: there is no part $part; the parts are sizes and boots" >&2
	exit 2
	;;
esac
. ci/vm.sh
. ci/budgets.sh

summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
container=out/edel-container-x86_64.tar.gz
vm=out/edel-x86_64.img
update=out/edel-x86_64.ext4.gz

# median: the middle of three numbers on standard input.
median() {
	sort -n | sed -n 2p
}

over=0
rows=''
# check WHAT MEASURED KEY UNIT: one row of the budget table.
check() {
	limit=$(budget "$3")
	if awk -v m="$2" -v b="$limit" 'BEGIN { exit !(m <= b) }'; then
		verdict=ok
	else
		verdict='**over**'
		over=1
	fi
	echo "budget $3: $2 $4, budget $limit $4, $verdict"
	rows="$rows| $1 | $2 $4 | $limit $4 | $verdict |
"
}

if [ "$part" != boots ]; then
	if [ -e "$container" ]; then
		check 'Container image (tar.gz)' "$(awk -v b="$(stat -c %s "$container")" 'BEGIN { printf "%.1f", b / 1048576 }')" container_mib MiB
		# What a person downloads to update the system (M5.10c4c): its slot's
		# image, compressed, the wallpapers and the fonts included.
		if [ -e "$update" ]; then
			check 'Update image (ext4.gz)' "$(awk -v b="$(stat -c %s "$update")" 'BEGIN { printf "%.1f", b / 1048576 }')" update_image_mib MiB
		fi
		# What a person downloads to write a stick (M8.17f).
		if [ -e "$vm.gz" ]; then
			check 'The stick (img.gz)' "$(awk -v b="$(stat -c %s "$vm.gz")" 'BEGIN { printf "%.1f", b / 1048576 }')" stick_mib MiB
		fi
	else
		echo "sizes not checked: the images were not built"
	fi
fi
if [ "$part" != sizes ]; then
	if [ -e "$vm" ]; then
		rm -f out/budgets.txt
		# The slot alone (M8.17c): a copy of the disk whose data partition
		# (partition 4) no longer holds the add-ons it carries, as a server's
		# has none, so the budgets measure what a server runs.
		. ci/names.sh
		alone=out/budget-alone.img
		cp --sparse=always "$vm" "$alone"
		partition_out "$alone" 4 out/budget-data.img
		for addon in $(debugfs -R "ls -p $addons_dir_on_data" out/budget-data.img 2>/dev/null | awk -F/ '$6 != "." && $6 != ".." && $6 != "" { print $6 }'); do
			for file in $(debugfs -R "ls -p $addons_dir_on_data/$addon" out/budget-data.img 2>/dev/null | awk -F/ '$6 ~ /\.ext4$/ { print $6 }'); do
				debugfs -w -R "rm $addons_dir_on_data/$addon/$file" out/budget-data.img
				echo "budget boots: without the carried add-on $addon/$file"
			done
		done
		partition_in out/budget-data.img "$alone" 4
		rm -f out/budget-data.img
		for n in 1 2 3; do
			vm_stamp=1 run_vm "out/budget-boot-$n.log" 'Started in [0-9.]+ s' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
				-vga none -drive if=none,id=disk0,format=raw,file="$alone" \
				-device virtio-blk-pci,drive=disk0,bootindex=0
			line=$(tr -d '\r' <"out/budget-boot-$n.log" |
				sed -n 's/.*Started in \([0-9.]*\) s, \([0-9]*\) MiB of memory in use, \([0-9]*\) MiB used on the root.*/\1 \2 \3/p' | tail -n 1)
			if [ -z "$line" ]; then
				echo "FAIL: boot $n printed no \"Started in\" line; see out/budget-boot-$n.log"
				exit 1
			fi
			echo "boot $n: $line"
			echo "$line" >>out/budgets.txt
		done
		rm -f "$alone"
		check 'One image with no screen: root used' "$(cut -d' ' -f3 out/budgets.txt | median)" headless_root_mib MiB
		check 'One image with no screen: boot to confirmed slot' "$(cut -d' ' -f1 out/budgets.txt | median)" headless_boot_seconds s
		check 'One image with no screen: memory in use after boot' "$(cut -d' ' -f2 out/budgets.txt | median)" headless_memory_mib MiB
	else
		echo "boots not measured: the image was not built"
	fi
fi
if [ -n "$rows" ]; then
	{
		echo "### Budgets"
		echo
		echo "| What | Measured | Budget | |"
		echo "|---|---|---|---|"
		printf '%s' "$rows"
		echo
	} >>"$summary"
fi

[ "$part" = boots ] || {
	echo "### Image sizes"
	echo
	echo "| What | Size on disk |"
	echo "|---|---|"
	for f in out/*.tar.gz out/*.ext4 out/*.img out/*.ext4.gz out/*.img.gz out/*.qcow2; do
		[ -e "$f" ] && echo "| $(basename "$f") | $(du -h "$f" | cut -f1) |"
	done
	for d in out/work/*/rootfs; do
		[ -d "$d" ] || continue
		image=$(basename "$(dirname "$d")")
		echo "| $image installed files | $(sudo du -sh "$d" | cut -f1) |"
		# What the firmware and the fonts take of it (M5.30).
		for sub in lib/firmware usr/share/fonts; do
			[ -d "$d/$sub" ] && echo "| $image /$sub | $(sudo du -sh "$d/$sub" | cut -f1) |"
		done
		# What the data partition carries beside the slot (M8.17f): each
		# add-on's image and the default apps (M6.2).
		data="$(dirname "$d")/data"
		[ -d "$data" ] || continue
		for addon in "$data"/addons/*/*.ext4; do
			[ -e "$addon" ] && echo "| $image add-on $(basename "$addon" .ext4), on the data partition | $(sudo du -h "$addon" | cut -f1) |"
		done
		echo "| $image default apps, on the data partition | $(sudo du -sh --exclude=addons "$data" | cut -f1) |"
	done
} >>"$summary"

if [ "$over" = 1 ]; then
	echo "FAIL: over budget; see the Budgets table in the run summary"
	exit 1
fi
exit 0
