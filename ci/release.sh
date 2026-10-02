#!/bin/sh
# Publishes the release ci/build.sh made in out/release/ and out/channel/
# (roadmap M3.4), or with dry-run checks it and shows what it would do.
#
#   sh ci/release.sh dry-run|publish TAG
#
# TAG preview replaces the assets of the `preview` pre-release, which every
# merge to main updates in place, unless the preview there is newer. TAG v*
# makes a draft release with the same assets plus channel.toml, the stable
# channel's manifest; publishing that draft is Alimardon's step, and then
# .github/workflows/release.yml copies channel.toml to gh-pages. Publishing
# needs gh with GH_TOKEN allowed to write contents, and a release signed
# with the release key, never a throwaway one (docs/RELEASE.md).
set -eu
mode=${1:-} tag=${2:-}
case "$mode" in
dry-run | publish) ;;
*)
	echo "usage: sh ci/release.sh dry-run|publish preview|vVERSION" >&2
	exit 2
	;;
esac
for file in out/release/release.toml out/release/release.toml.sig out/channel/release.toml out/channel/release.toml.sig out/channel/signer; do
	[ -s "$file" ] || {
		echo "FAIL: $file is missing; ci/build.sh makes it"
		exit 1
	}
done
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' out/release/release.toml)
signer=$(cat out/channel/signer)
if [ "$mode" = publish ] && [ "$signer" != release ]; then
	echo "FAIL: this release is signed with a $signer key; set the EDEL_RELEASE_KEY secret (docs/RELEASE.md)"
	exit 1
fi
cp out/channel/release.toml out/release/channel.toml
cp out/channel/release.toml.sig out/release/channel.toml.sig
assets=$(cd out/release && ls | tr '\n' ' ')
# would WHAT: says what publish does, or would do in a dry run.
would() {
	if [ "$mode" = publish ]; then echo "$*"; else echo "dry run, would $*"; fi
}

case "$tag" in
preview)
	current=$(curl -fsSL https://github.com/alimardon123/edel/releases/download/preview/release.toml 2>/dev/null |
		sed -n 's/^version = "\(.*\)"$/\1/p' || true)
	if [ -n "$current" ] && [ "$(printf '%s\n%s\n' "$current" "$version" | sort -V | tail -n 1)" != "$version" ]; then
		echo "the preview holds $current, newer than $version; leaving it"
		exit 0
	fi
	would "replace the preview pre-release's assets (now ${current:-none}) with $version: $assets"
	if [ "$mode" = publish ]; then
		gh release view preview >/dev/null 2>&1 ||
			gh release create preview --prerelease --title "Edel OS preview" --notes-file docs/TRY-IT.md
		gh release edit preview --prerelease --notes-file docs/TRY-IT.md
		gh release upload preview out/release/* --clobber
	fi
	;;
v*)
	grep -q "releases/download/$tag/" out/channel/release.toml || {
		echo "FAIL: out/channel/release.toml does not name the images under $tag"
		exit 1
	}
	would "make the draft release $tag ($version) with: $assets"
	would "leave the stable channel alone until the draft is published (release.yml)"
	if [ "$mode" = publish ]; then
		gh release create "$tag" --draft --title "Edel OS $version" --notes-file docs/TRY-IT.md out/release/*
	fi
	;;
*)
	echo "FAIL: TAG must be preview or v followed by the version, not $tag"
	exit 2
	;;
esac
