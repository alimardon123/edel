#!/bin/sh
# Prints the "### Packages" section of the run summary (roadmap M3.1): for
# each image, the packages added, removed or changed against the preview
# release, as a record, never a gate. Always exits 0.
set -u

base=https://github.com/alimardon123/edel/releases/download/preview
echo "### Packages"
for list in out/*.packages; do
	[ -e "$list" ] || continue
	name=$(basename "$list")
	if curl -fsSL "$base/$name" -o "out/preview-$name" 2>/dev/null; then
		echo "#### $name against the preview"
		echo '```diff'
		diff "out/preview-$name" "$list" | sed -n 's/^</-/p; s/^>/+/p' | grep . || echo "(no change)"
		echo '```'
	else
		echo "- $name: $(wc -l <"$list") packages; no preview to compare with yet"
	fi
done
exit 0
