#!/bin/sh
# Points the stable channel at a published release (roadmap M3.4): copies
# the release's channel.toml and its signature to channels/stable/ on the
# gh-pages branch, which GitHub Pages serves, so `edel update check
# https://alimardon123.github.io/edel/channels/stable/release.toml` finds
# it. Pages holds only manifests; the images stay with the release.
#
#   sh ci/channel.sh dry-run|publish TAG
#
# publish downloads channel.toml from the published release TAG and pushes
# to gh-pages (.github/workflows/release.yml runs it when Alimardon
# publishes a draft); dry-run takes out/channel/ from ci/build.sh and
# commits in a scratch worktree without pushing, so CI walks the whole
# tag path on every pull request.
set -eu
mode=${1:-} tag=${2:-}
case "$mode" in
dry-run | publish) ;;
*)
	echo "usage: sh ci/channel.sh dry-run|publish vVERSION" >&2
	exit 2
	;;
esac
case "$tag" in v*) ;; *)
	echo "FAIL: TAG must be v followed by the version, not $tag"
	exit 2
	;;
esac

from=$(mktemp -d)
if [ "$mode" = publish ]; then
	gh release download "$tag" -p channel.toml -p channel.toml.sig -D "$from"
else
	cp out/channel/release.toml "$from/channel.toml"
	cp out/channel/release.toml.sig "$from/channel.toml.sig"
fi
grep -q "releases/download/$tag/" "$from/channel.toml" || {
	echo "FAIL: channel.toml does not name the images under $tag"
	exit 1
}

pages=$(mktemp -d)
rmdir "$pages"
if git fetch -q origin gh-pages 2>/dev/null; then
	git worktree add -q --detach "$pages" FETCH_HEAD
else
	git worktree add -q --orphan -b "gh-pages-$$" "$pages"
fi
mkdir -p "$pages/channels/stable"
cp "$from/channel.toml" "$pages/channels/stable/release.toml"
cp "$from/channel.toml.sig" "$pages/channels/stable/release.toml.sig"
git -C "$pages" add channels
git -C "$pages" -c user.name="github-actions[bot]" \
	-c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
	commit -qm "Point the stable channel at $tag"
git -C "$pages" show --stat --format='%s' HEAD
if [ "$mode" = publish ]; then
	git -C "$pages" push -q origin HEAD:refs/heads/gh-pages
	echo "the stable channel now points at $tag"
else
	echo "dry run, would push this commit to gh-pages"
fi
git worktree remove --force "$pages"
git branch -D -q "gh-pages-$$" 2>/dev/null || true
rm -rf "$from"
