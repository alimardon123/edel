#!/bin/sh
# Tags the container image for a registry, so developers can write
# `FROM ghcr.io/OWNER/edel:VERSION` (roadmap M8.9), and with push sends it.
#
#   sh ci/registry.sh dry-run|push
#
# Both read the release ci/build.sh made in out/release/: the container
# image's file (named from images/container.toml) and the version in
# release.toml. dry-run, in the images job, imports the file into Docker
# under the registry's name and checks that the tagged image is Edel OS of
# that version, and says what push would do. push, in the Release
# workflow once a v* release is published and its checklist ticked, does
# the same and pushes it. Pushing waits for Alimardon's yes: the workflow
# runs it only while the repository variable EDEL_REGISTRY is yes
# (docs/RELEASE.md). push needs GH_TOKEN allowed to write packages.
set -eu
mode=${1:-}
case "$mode" in
dry-run | push) ;;
*)
	echo "usage: sh ci/registry.sh dry-run|push" >&2
	exit 2
	;;
esac
name=$(sed -n 's/^name = "\(.*\)"$/\1/p' images/container.toml)
arch=$(sed -n 's/^arch = "\(.*\)"$/\1/p' images/container.toml)
file="out/release/$name-$arch.tar.gz"
[ -s "$file" ] || {
	echo "FAIL: $file is missing; ci/build.sh makes it"
	exit 1
}
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' out/release/release.toml 2>/dev/null)
[ -n "$version" ] || {
	echo "FAIL: out/release/release.toml names no version"
	exit 1
}
# The repository's owner, as the registry wants it, in lower case: from the
# workflow, or from this checkout's remote.
owner=${GITHUB_REPOSITORY_OWNER:-$(git remote get-url origin | sed -n 's|.*github.com[:/]\([^/]*\)/.*|\1|p')}
owner=$(echo "$owner" | tr 'A-Z' 'a-z')
[ -n "$owner" ] || {
	echo "FAIL: no repository owner: set GITHUB_REPOSITORY_OWNER"
	exit 1
}
image="ghcr.io/$owner/edel:$version"

# The file is a root file system, so it is imported, not loaded; a shell is
# what it runs when nothing else is asked.
docker import --change 'CMD ["/bin/sh"]' "$file" "$image" >/dev/null
got=$(docker run --rm "$image" sh -c '. /etc/os-release && echo "$NAME $VERSION_ID"')
[ "$got" = "Edel OS $version" ] || {
	echo "FAIL: $image says \"$got\", not Edel OS $version"
	exit 1
}
if [ "$mode" = dry-run ]; then
	echo "PASS: tagged $image from $file, which says Edel OS $version; dry run, would push it"
	exit 0
fi
echo "$GH_TOKEN" | docker login ghcr.io -u "${GITHUB_ACTOR:-$owner}" --password-stdin
docker push "$image"
echo "pushed $image"
