#!/bin/sh
# Signs the release lists ci/build.sh made, out/release/release.toml and
# out/channel/release.toml (roadmap M3.4, M3.7), in the "sign" job of
# .github/workflows/ci.yml: a fresh runner, after the build, with only the
# two lists from the build and the signer built there from the locked
# sources, so nothing the build produced runs while the key is present.
# Only that job of a push to main or a tag is given EDEL_RELEASE_KEY.
#
#   EDEL=path/to/edel sh ci/sign.sh
#
# With the key, both lists are signed with it and checked against the
# public keys in images/keys/ (at least two; the build job checks that
# every image carries them, ci/keys-carried.sh); without it, a throwaway
# key signs, and out/channel/signer says which, so ci/release.sh never
# publishes a throwaway signature.
set -eu
edel=${EDEL:?set EDEL to the edel this job built}

if [ -n "${EDEL_RELEASE_KEY:-}" ]; then
	count=$(find images/keys -name '*.pub' 2>/dev/null | wc -l)
	if [ "$count" -lt 2 ]; then
		echo "FAIL: EDEL_RELEASE_KEY is set but images/keys/ holds $count public key(s); two are needed (docs/RELEASE.md, step 1)"
		exit 1
	fi
	mkdir -p out/keys
	# The secret never outlives this script, even when signing fails.
	trap 'rm -f out/keys/release.key' EXIT
	(umask 077 && printf '%s\n' "$EDEL_RELEASE_KEY" >out/keys/release.key)
	key=out/keys/release.key keys=images/keys signer=release
	# The build ran untrusted code (cargo, apk), so the real key signs only
	# the version and channel this run's ref gives (the review of
	# 2026-10-25): a tag's own version on stable, else the preview.
	if [ "${GITHUB_REF_TYPE:-}" = tag ]; then
		want_channel=stable want_version=${GITHUB_REF_NAME#v}
	else
		want_channel=preview want_version=
	fi
	for manifest in out/release/release.toml out/channel/release.toml; do
		channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$manifest" | head -n 1)
		version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$manifest" | head -n 1)
		if [ "$channel" != "$want_channel" ] || { [ -n "$want_version" ] && [ "$version" != "$want_version" ]; }; then
			echo "FAIL: $manifest is version $version on channel $channel, but this run signs ${want_version:-a preview} on $want_channel; nothing was signed"
			exit 1
		fi
	done
else
	# Its own directory: verify reads every *.pub in it.
	$edel release keygen out/keys/throwaway ci-release
	key=out/keys/throwaway/ci-release.key keys=out/keys/throwaway signer=throwaway
fi
for manifest in out/release/release.toml out/channel/release.toml; do
	# Each list expires 60 days after it is signed (M3.8, the default row),
	# renewed by every publish and monthly rebuild.
	$edel release sign --key "$key" --expires-in 60 "$manifest"
	$edel release verify --keys "$keys" "$manifest"
done
echo "$signer" >out/channel/signer
echo "PASS: both release lists signed with the $signer key and verified"
