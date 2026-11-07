#!/bin/sh
# Makes a copy of the test apps' repository in which org.edel.TestA has a
# second commit on its branch (roadmap M7.20), so the default apps' test
# (ci/apps-test.sh) finds an update of TestA, undoes it and takes it
# again. The commit adds one file, files/share/edel-test-version, which
# says 2; everything else is the first commit's. ci/build.sh runs it inside
# the build container after ci/apps/make-repo.sh and the images that take
# TestA's first commit. Usage: sh ci/apps/next-commit.sh REPO NEW_REPO
set -eu

repo=$1
next=$2
work=out/apps-src
[ -d "$work/org.edel.TestA" ] || {
	echo "ci/apps/next-commit.sh: $work/org.edel.TestA is missing; run ci/apps/make-repo.sh first" >&2
	exit 1
}
rm -rf "$next" "$work/next"
cp -a "$repo" "$next"
cp -a "$work/org.edel.TestA" "$work/next"
echo 2 >"$work/next/files/share/edel-test-version"
flatpak build-export --arch=x86_64 "$next" "$work/next" stable
echo "ci/apps/next-commit.sh: $next holds $repo and a second commit of org.edel.TestA//stable"
