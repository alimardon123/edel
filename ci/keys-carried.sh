#!/bin/sh
# Checks that every bootable image carries every public key in images/keys/
# (at least two, so one can replace the other), or its machines could never
# take an update signed with the release key (M1.6). It runs in the build
# job, which has the images' files; the sign job, which has the key, never
# sees them (M3.7). Until Alimardon adds the release's public keys
# (docs/RELEASE.md, step 1), there is nothing to check.
set -eu
count=$(find images/keys -name '*.pub' 2>/dev/null | wc -l)
if [ "$count" = 0 ]; then
	echo "no release public keys in images/keys/ yet, so nothing to check (docs/RELEASE.md, step 1)"
	exit 0
fi
if [ "$count" -lt 2 ]; then
	echo "FAIL: images/keys/ holds $count public key; two are needed (docs/RELEASE.md, step 1)"
	exit 1
fi
. ci/names.sh
for def in images/*.toml; do
	grep -q '^kernel = ' "$def" || continue
	name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$def")
	arch=$(sed -n 's/^arch = "\(.*\)"$/\1/p' "$def")
	for pub in images/keys/*.pub; do
		cmp -s "$pub" "out/work/$name-$arch/rootfs$share_dir/keys/$(basename "$pub")" || {
			echo "FAIL: $name does not carry $pub, so its machines could never take an update; name it in $def's [release] public_keys"
			exit 1
		}
	done
done
echo "PASS: every bootable image carries the $count public keys in images/keys/"
