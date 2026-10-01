#!/bin/sh
# Prints image sizes as a Markdown table for the CI summary. These are the
# first measurements behind the size budgets in the design principles.
set -eu

echo "### Image sizes"
echo
echo "| What | Size |"
echo "|---|---|"
for f in out/*.tar.gz out/*.vmlinuz out/*.initramfs; do
	[ -e "$f" ] && echo "| $(basename "$f") | $(du -h "$f" | cut -f1) |"
done
for d in out/work/*/rootfs; do
	[ -d "$d" ] && echo "| $(basename "$(dirname "$d")") installed files | $(sudo du -sh "$d" | cut -f1) |"
done
exit 0
