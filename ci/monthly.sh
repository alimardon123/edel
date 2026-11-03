#!/bin/sh
# The monthly security rebuild (roadmap M8.3, docs/SECURITY.md), run by the
# workflow "Monthly rebuild" on the first of each month.
#
#   sh ci/monthly.sh prepare BODY [BASE]  refresh the build container's
#                                  digest in features/base.toml and write
#                                  the pull request's body, with the package
#                                  diff, to BODY; BASE is main or a kept
#                                  release/YYYY branch (M8.5), checked out
#   sh ci/monthly.sh branch [BASE] [day]  the rebuild's branch,
#                                  monthly/YYYY-MM for main,
#                                  monthly/release-YYYY/YYYY-MM for
#                                  release/YYYY; with day, the security
#                                  watch's rebuild, -DD after the month
#   sh ci/monthly.sh tag BRANCH    the release a merged BRANCH tags,
#                                  vYYYY.MM.0, or vYYYY.MM.D for a day's
#   sh ci/monthly.sh fetch BASE DIR  the security watch (M8.43): the bills
#                                  of materials of BASE's newest published
#                                  release into DIR; exits 4 when BASE has
#                                  none to watch
#   sh ci/monthly.sh --self-test   check the names and the package diff
#
# Every image is built from the Alpine branch features/base.toml names, so
# a new build already takes every fix the branch carries; what the rebuild
# moves is the build container, pinned by its digest, and the month's
# release. prepare needs Docker; for the diff it reads the package lists
# of the newest published release (or the preview) with gh, and the
# branch's current versions from apk inside the new container.
set -eu

# branch [BASE] [day]: this month's (UTC) rebuild of BASE, monthly/YYYY-MM
# for main and monthly/release-YYYY/YYYY-MM for a kept release/YYYY; with
# day, the daily security watch's rebuild (M8.43), monthly/YYYY-MM-DD, so
# it never replaces the month's after that one merged.
branch() {
	when=$(date -u +%Y-%m)
	if [ "${2:-}" = day ]; then when=$(date -u +%Y-%m-%d); fi
	case "${1:-main}" in
	main) echo "monthly/$when" ;;
	release/*) echo "monthly/release-${1#release/}/$when" ;;
	*)
		echo "monthly.sh: $1 is neither main nor a kept release/YYYY branch" >&2
		return 1
		;;
	esac
}

# tag BRANCH: the release a merged monthly/YYYY-MM tags, vYYYY.MM.0, the
# month's first; a security watch's monthly/YYYY-MM-DD tags vYYYY.MM.D,
# later in the month than .0 and never the same twice.
tag() {
	case "$1" in
	monthly/[0-9][0-9][0-9][0-9]-[0-1][0-9])
		month=${1#monthly/}
		echo "v${month%-*}.${month#*-}.0"
		;;
	monthly/[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9])
		day=${1#monthly/}
		month=${day%-*}
		echo "v${month%-*}.${month#*-}.$(expr "${day##*-}" + 0)"
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

# download PATTERN DIR: the files matching PATTERN of main's newest
# published release, else of the preview, into DIR; prints the release's
# tag, or fails when neither has such files. gh is $GH, so the self-test
# can stand in for it.
download() {
	for release in "$(${GH:-gh} api "repos/$GITHUB_REPOSITORY/releases?per_page=10" --jq '[.[] | select(.draft == false and .prerelease == false)][0].tag_name // empty' 2>/dev/null)" preview; do
		[ -n "$release" ] || continue
		${GH:-gh} release download "$release" -R "$GITHUB_REPOSITORY" -p "$1" -D "$2" --clobber 2>/dev/null || continue
		ls "$2"/$1 >/dev/null 2>&1 || continue
		echo "$release"
		return 0
	done
	return 1
}

# fetch BASE DIR: the security watch's input (M8.43), the bills of
# materials of BASE's newest published release, images and add-ons, in
# DIR. A kept release/YYYY publishes no release of its own yet (M8.5), so
# it has nothing to watch; neither has main before its first release.
fetch() {
	base=$1 dir=$2
	case "$base" in
	main) ;;
	release/*)
		echo "monthly.sh: $base publishes no release yet, so the watch has nothing to check there"
		return 4
		;;
	*)
		echo "monthly.sh: $base is neither main nor a kept release/YYYY branch" >&2
		return 1
		;;
	esac
	mkdir -p "$dir"
	if release=$(download '*.cdx.json' "$dir"); then
		echo "monthly.sh: the watch checks $base's release $release"
	else
		echo "monthly.sh: $base has no published release with bills of materials yet, so the watch has nothing to check"
		return 4
	fi
}

prepare() {
	body=$1 base=${2:-main}
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
	from=$(download '*.packages' out/monthly) || from=''

	month=$(branch "$base")
	# A kept release's rebuild tags nothing yet: which channel its
	# machines follow, so the newest release never reaches them as an
	# older one, waits for Alimardon (M8.5).
	if [ "$base" = main ]; then
		merged="Merging it tags the release \`$(tag "$month")\`, whose CI run makes its draft release."
	else
		merged="Merging it rebuilds the kept release \`$base\` (M8.5); it tags no release until its channel is decided."
	fi
	{
		echo "Before: the images were last rebuilt with the build container \`$old\`."
		echo
		echo "After: the monthly security rebuild (roadmap M8.3, \`docs/SECURITY.md\`) builds every image again from Alpine's \`v$alpine\` branch as it is today, in its current container \`$digest\`, and this pull request's CI run tests them all. $merged"
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
	check "a kept release's rebuild branch" "$(branch release/2026)" "monthly/release-2026/$(date -u +%Y-%m)"
	check "a kept release's rebuild tags nothing yet" "$(tag "$(branch release/2026)" 2>/dev/null || echo none)" none
	check "a lane is not rebuilt" "$(branch lane/ci 2>/dev/null || echo none)" none
	check "the watch's rebuild of main" "$(branch main day)" "monthly/$(date -u +%Y-%m-%d)"
	check "the watch's rebuild of a kept release" "$(branch release/2026 day)" "monthly/release-2026/$(date -u +%Y-%m-%d)"
	check "the tag of the watch's monthly/2026-11-07" "$(tag monthly/2026-11-07)" v2026.11.7
	check "the tag of the watch's monthly/2026-11-21" "$(tag monthly/2026-11-21)" v2026.11.21
	# The watch's fetch, with gh stood in for: the newest release's bills of
	# materials, the preview's when none is published, none for a kept
	# release or before any release.
	tmp=$(mktemp -d)
	cat >"$tmp/gh" <<'GH'
#!/bin/sh
case "$1" in
api) [ -z "$STUB_RELEASE" ] || echo "$STUB_RELEASE" ;;
release)
	[ "$3" = "$STUB_HAS" ] || exit 1
	while [ "$1" != -D ]; do shift; done
	echo '{}' >"$2/edel-x86_64.cdx.json"
	;;
esac
GH
	chmod +x "$tmp/gh"
	got=$(GH=$tmp/gh STUB_RELEASE=v2026.12.0 STUB_HAS=v2026.12.0 GITHUB_REPOSITORY=o/r fetch main "$tmp/a")
	check "the watch reads the newest release" "$got" "monthly.sh: the watch checks main's release v2026.12.0"
	got=$(GH=$tmp/gh STUB_RELEASE='' STUB_HAS=preview GITHUB_REPOSITORY=o/r fetch main "$tmp/b")
	check "the watch reads the preview when nothing is published" "$got" "monthly.sh: the watch checks main's release preview"
	status=0
	GH=$tmp/gh STUB_RELEASE='' STUB_HAS=none GITHUB_REPOSITORY=o/r fetch main "$tmp/c" >/dev/null || status=$?
	check "no release, nothing to watch" "$status" 4
	status=0
	fetch release/2027 "$tmp/d" >/dev/null || status=$?
	check "a kept release has nothing to watch yet" "$status" 4
	rm -rf "$tmp"
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
prepare) prepare "${2:?usage: sh ci/monthly.sh prepare BODY [BASE]}" "${3:-main}" ;;
branch) branch "${2:-main}" "${3:-}" ;;
fetch) fetch "${2:?usage: sh ci/monthly.sh fetch BASE DIR}" "${3:?usage: sh ci/monthly.sh fetch BASE DIR}" ;;
tag) tag "${2:?usage: sh ci/monthly.sh tag BRANCH}" ;;
--self-test) self_test ;;
*)
	echo "usage: sh ci/monthly.sh prepare BODY [BASE] | branch [BASE] [day] | tag BRANCH | fetch BASE DIR | --self-test" >&2
	exit 2
	;;
esac
