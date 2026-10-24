#!/bin/sh
# The monthly security rebuild (roadmap M8.3, docs/SECURITY.md), run by the
# workflow "Monthly rebuild" on the first of each month.
#
#   sh ci/monthly.sh prepare BODY  refresh the build container's digest in
#                                  features/base.toml and write the pull
#                                  request's body, with the package diff, to
#                                  BODY
#   sh ci/monthly.sh branch        the rebuild's branch, monthly/YYYY-MM
#   sh ci/monthly.sh tag BRANCH    the release a merged BRANCH tags,
#                                  vYYYY.MM.0
#   sh ci/monthly.sh --self-test   check the names and the package diff
#
# Every image is built from the Alpine branch features/base.toml names, so
# a new build already takes every fix the branch carries; what the rebuild
# moves is the build container, pinned by its digest, and the month's
# release. prepare needs Docker; for the diff it reads the package lists
# of the newest published release (or the preview) with gh, and the
# branch's current versions from apk inside the new container.
set -eu

# branch: monthly/YYYY-MM for this month (UTC).
branch() {
	echo "monthly/$(date -u +%Y-%m)"
}

# tag BRANCH: the release a merged monthly/YYYY-MM tags, vYYYY.MM.0; the
# month's first, so a second fix release that month is .1.
tag() {
	case "$1" in
	monthly/[0-9][0-9][0-9][0-9]-[0-1][0-9])
		month=${1#monthly/}
		echo "v${month%-*}.${month#*-}.0"
		;;
	*)
		echo "monthly.sh: $1 is not a monthly rebuild's branch, monthly/YYYY-MM" >&2
		return 1
		;;
	esac
}

# package_diff OLD NEW: the packages of the list OLD (one name-version a
# line, as out/IMAGE.packages) whose version in NEW (the branch's
# name-version lines) differs, as - old and + new lines; a package gone
# from the branch shows only its - line. apk's versions end in -rN, so
# the name is all but the last two dash-separated fields.
package_diff() {
	awk '
		function name(p,   n, i, s) { n = split(p, f, "-"); s = f[1]; for (i = 2; i <= n - 2; i++) s = s "-" f[i]; return s }
		FNR == NR { if ($1 != "") now[name($1)] = $1; next }
		$1 != "" {
			n = name($1)
			if (!(n in now)) { print "- " $1 " (gone from the branch)" }
			else if (now[n] != $1) { print "- " $1; print "+ " now[n] }
		}
	' "$2" "$1"
}

prepare() {
	body=$1
	alpine=$(sed -n 's/^branch = "v\(.*\)"$/\1/p' features/base.toml)
	[ -n "$alpine" ] || {
		echo "monthly.sh: features/base.toml names no Alpine branch" >&2
		exit 1
	}
	# Google's copy first, as ci/pull-alpine.sh does: Docker Hub limits
	# anonymous requests per runner.
	digest=''
	for ref in "mirror.gcr.io/library/alpine:$alpine" "alpine:$alpine"; do
		digest=$(docker buildx imagetools inspect "$ref" 2>/dev/null | awk '$1 == "Digest:" { print $2; exit }') && [ -n "$digest" ] && break
	done
	[ -n "$digest" ] || {
		echo "monthly.sh: could not read the digest of alpine:$alpine" >&2
		exit 1
	}
	old=$(sed -n 's/^image_digest = "\(.*\)"$/\1/p' features/base.toml)
	sed -i "s/^image_digest = \".*\"$/image_digest = \"$digest\"/; s/Last read [0-9-]*\\./Last read $(date -u +%Y-%m-%d)./" features/base.toml

	# The branch's versions now, from apk in the new container.
	mkdir -p out/monthly
	docker run --rm "$ref@$digest" sh -c 'apk update -q >/dev/null && apk list --available 2>/dev/null' |
		awk '{ print $1 }' >out/monthly/branch.list
	# The last release's lists: the newest published release, else the preview.
	from=''
	for release in "$(gh api "repos/$GITHUB_REPOSITORY/releases?per_page=10" --jq '[.[] | select(.draft == false and .prerelease == false)][0].tag_name // empty' 2>/dev/null)" preview; do
		[ -n "$release" ] || continue
		gh release download "$release" -R "$GITHUB_REPOSITORY" -p '*.packages' -D out/monthly --clobber 2>/dev/null || continue
		from=$release
		break
	done

	month=$(branch)
	{
		echo "Before: the images were last rebuilt with the build container \`$old\`."
		echo
		echo "After: the monthly security rebuild (roadmap M8.3, \`docs/SECURITY.md\`) builds every image again from Alpine's \`v$alpine\` branch as it is today, in its current container \`$digest\`, and this pull request's CI run tests them all. Merging it tags the release \`$(tag "$month")\`, whose CI run makes its draft release."
		echo
		echo "How: \`ci/monthly.sh prepare\` read the digest of \`alpine:$alpine\` and wrote it into \`features/base.toml\`."
		echo
		echo "**Package changes:**"
		echo
		listed=0
		for list in out/monthly/*.packages; do
			[ -e "$list" ] || continue
			listed=1
			echo "$(basename "$list") against \`$from\`:"
			echo
			echo '```diff'
			package_diff "$list" out/monthly/branch.list | grep . || echo "(no change)"
			echo '```'
			echo
		done
		[ "$listed" = 1 ] || echo "No release is published yet, so there is nothing to compare with; the CI run's summary lists every image's packages."
	} >"$body"
	echo "monthly.sh: alpine:$alpine is $digest (was $old); the body is in $body"
}

self_test() {
	fails=0
	check() {
		if [ "$2" = "$3" ]; then
			echo "PASS: $1"
		else
			echo "FAIL: $1: wanted $(printf '%s' "$3" | tr '\n' '|'), got $(printf '%s' "$2" | tr '\n' '|')"
			fails=$((fails + 1))
		fi
	}
	check "the tag of monthly/2026-11" "$(tag monthly/2026-11)" v2026.11.0
	check "a branch not a monthly's has no tag" "$(tag lane/ci 2>/dev/null || echo none)" none
	case $(branch) in
	monthly/[0-9][0-9][0-9][0-9]-[0-1][0-9]) echo "PASS: this month's branch is $(branch)" ;;
	*)
		echo "FAIL: this month's branch is $(branch)"
		fails=$((fails + 1))
		;;
	esac
	tmp=$(mktemp -d)
	printf '%s\n' musl-1.2.5-r10 libgcc-14.2.0-r6 xz-libs-5.8.1-r0 gone-1.0-r0 >"$tmp/old"
	printf '%s\n' musl-1.2.5-r11 libgcc-14.2.0-r6 xz-libs-5.8.1-r1 xz-5.8.1-r1 >"$tmp/new"
	check "the package diff" "$(package_diff "$tmp/old" "$tmp/new")" "- musl-1.2.5-r10
+ musl-1.2.5-r11
- xz-libs-5.8.1-r0
+ xz-libs-5.8.1-r1
- gone-1.0-r0 (gone from the branch)"
	rm -rf "$tmp"
	[ "$fails" = 0 ]
}

case "${1:-}" in
prepare) prepare "${2:?usage: sh ci/monthly.sh prepare BODY}" ;;
branch) branch ;;
tag) tag "${2:?usage: sh ci/monthly.sh tag BRANCH}" ;;
--self-test) self_test ;;
*)
	echo "usage: sh ci/monthly.sh prepare BODY | branch | tag BRANCH | --self-test" >&2
	exit 2
	;;
esac
