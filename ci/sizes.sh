#!/bin/sh
# Checks the budgets in ci/budgets.toml (roadmap M3.2) and writes them, with
# the image sizes, as Markdown tables to the run summary. Boot time, memory
# in use and root size are the median of three boots of the VM image, read
# from the line edel-boot-ok prints. Fails the job when a measurement is
# over its budget; CI runs it even after a failure, and then it only
# reports what was built.
set -eu
. ci/vm.sh

summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
container=out/edel-container-x86_64.tar.gz
vm=out/edel-vm-x86_64.img

# budget KEY: the number KEY has in ci/budgets.toml.
budget() {
	awk -F= -v key="$1" '{ k = $1; gsub(/ /, "", k) } k == key { v = $2; sub(/#.*/, "", v); gsub(/ /, "", v); print v }' ci/budgets.toml
}

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

if [ -e "$container" ] && [ -e "$vm" ]; then
	rm -f out/budgets.txt
	for n in 1 2 3; do
		vm_stamp=1 run_vm "out/budget-boot-$n.log" 'Started in [0-9.]+ s' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
			-drive if=none,id=disk0,format=raw,file="$vm" \
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
	check 'Container image (tar.gz)' "$(awk -v b="$(stat -c %s "$container")" 'BEGIN { printf "%.1f", b / 1048576 }')" container_mib MiB
	check 'VM root used' "$(cut -d' ' -f3 out/budgets.txt | median)" vm_root_mib MiB
	check 'VM boot to confirmed slot' "$(cut -d' ' -f1 out/budgets.txt | median)" vm_boot_seconds s
	check 'VM memory in use after boot' "$(cut -d' ' -f2 out/budgets.txt | median)" vm_memory_mib MiB
	{
		echo "### Budgets"
		echo
		echo "| What | Measured | Budget | |"
		echo "|---|---|---|---|"
		printf '%s' "$rows"
		echo
	} >>"$summary"
else
	echo "budgets not checked: the images were not built"
fi

{
	echo "### Image sizes"
	echo
	echo "| What | Size on disk |"
	echo "|---|---|"
	for f in out/*.tar.gz out/*.ext4 out/*.img out/*.ext4.gz out/*.img.gz; do
		[ -e "$f" ] && echo "| $(basename "$f") | $(du -h "$f" | cut -f1) |"
	done
	for d in out/work/*/rootfs; do
		[ -d "$d" ] || continue
		image=$(basename "$(dirname "$d")")
		echo "| $image installed files | $(sudo du -sh "$d" | cut -f1) |"
		# What the firmware and the fonts take of it (M5.30).
		for part in lib/firmware usr/share/fonts; do
			[ -d "$d/$part" ] && echo "| $image /$part | $(sudo du -sh "$d/$part" | cut -f1) |"
		done
	done
} >>"$summary"

if [ "$over" = 1 ]; then
	echo "FAIL: over budget; see the Budgets table in the run summary"
	exit 1
fi
exit 0
