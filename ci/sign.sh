#!/bin/sh
# Signs the release ci/build.sh made in out/release/ and out/channel/
# (roadmap M3.4). It runs in a container of its own, after the build, and
# only it is given EDEL_RELEASE_KEY, and only on a push to main or a tag:
# cargo, apk and the image build run code from the network, which must
# never see the key.
#
#   docker run --rm -e EDEL_RELEASE_KEY -v "$PWD:/src" -w /src alpine:3.24 sh ci/sign.sh
#
# With the key, every bootable image must carry every public key in
# images/keys/ (at least two, so one can replace the other) and both lists
# are checked against them; without it, a throwaway key signs, and
# out/channel/signer says which, so ci/release.sh never publishes a
# throwaway signature.
set -eu

# The edel Alpine's cargo built links against libgcc_s.
apk add --no-cache --quiet libgcc
edel=./target/release/edel

if [ -n "${EDEL_RELEASE_KEY:-}" ]; then
	count=$(find images/keys -name '*.pub' 2>/dev/null | wc -l)
	if [ "$count" -lt 2 ]; then
		echo "FAIL: EDEL_RELEASE_KEY is set but images/keys/ holds $count public key(s); two are needed (docs/RELEASE.md, step 1)"
		exit 1
	fi
	for def in images/*.toml; do
		grep -q '^kernel = ' "$def" || continue
		name=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$def")
		arch=$(sed -n 's/^arch = "\(.*\)"$/\1/p' "$def")
		for pub in images/keys/*.pub; do
			cmp -s "$pub" "out/work/$name-$arch/rootfs/usr/share/edel/keys/$(basename "$pub")" || {
				echo "FAIL: $name does not carry $pub, so its machines could never take an update; name it in $def's [release] public_keys"
				exit 1
			}
		done
	done
	# The secret never outlives this script, even when signing fails.
	trap 'rm -f out/keys/release.key' EXIT
	(umask 077 && printf '%s\n' "$EDEL_RELEASE_KEY" >out/keys/release.key)
	key=out/keys/release.key keys=images/keys signer=release
else
	# Its own directory: verify reads every *.pub in it, and out/keys also
	# holds the test keys and an ssh key.
	$edel release keygen out/keys/throwaway ci-release
	key=out/keys/throwaway/ci-release.key keys=out/keys/throwaway signer=throwaway
fi
for manifest in out/release/release.toml out/channel/release.toml; do
	$edel release sign --key "$key" "$manifest"
	$edel release verify --keys "$keys" "$manifest"
done
echo "$signer" >out/channel/signer

# Root made these; the host reads and deletes them.
owner=$(stat -c %u:%g .)
chown -R "$owner" out/release out/channel out/keys
