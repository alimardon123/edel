#!/bin/sh
# A kept release changes no look (roadmap M8.5, ADR-008): on a release/*
# branch, a commit that changes design/ or presets/ must say so with a
# subject starting "look-fix:" (an accessibility or broken-shortcut fix,
# which still passes the contrast test with every other commit), so a
# security rebuild never changes what people see. Rust checks runs it on
# every pull request into a release/* branch and every push to one.
#
#   sh ci/look-fix.sh FROM TO     every commit in FROM..TO, first parents' diffs
#   sh ci/look-fix.sh --self-test  the check on a throwaway repository
set -eu

# look_paths: the paths only a look-fix: commit may change on a release
# branch.
look_paths="design presets"

# check FROM TO: fails naming each commit in FROM..TO that changes a look
# path without a look-fix: subject; a merge counts by its diff against its
# first parent, so a merge cannot carry one in unnoticed.
check() {
	bad=0
	for commit in $(git rev-list "$1..$2"); do
		# shellcheck disable=SC2086 # look_paths is a list of words
		changed=$(git diff --name-only "$commit^1" "$commit" -- $look_paths 2>/dev/null) || continue
		[ -n "$changed" ] || continue
		subject=$(git log -1 --format=%s "$commit")
		case "$subject" in
		look-fix:*) echo "look-fix: $(git rev-parse --short "$commit") \"$subject\" changes $(echo "$changed" | tr '\n' ' ')" ;;
		*)
			echo "FAIL: $(git rev-parse --short "$commit") \"$subject\" changes $(echo "$changed" | tr '\n' ' ')on a kept release; only a commit whose subject starts \"look-fix:\" (an accessibility or broken-shortcut fix) may change design/ or presets/ there, so a security rebuild never changes what people see (ROADMAP M8.5)"
			bad=1
			;;
		esac
	done
	[ "$bad" = 0 ] || return 1
	echo "PASS: no commit in $1..$2 changes the look without look-fix:"
}

self_test() {
	repo=$(mktemp -d)
	trap 'rm -rf "$repo"' EXIT
	(
		cd "$repo"
		git init -q
		git config user.name test
		git config user.email test@example.com
		mkdir design docs
		echo a >design/tokens.toml
		echo a >docs/x.md
		git add . && git commit -q -m "Start"
		git tag base
		echo b >docs/x.md && git commit -qam "Fix a security hole"
		git tag docs-only
		echo b >design/tokens.toml && git commit -qam "Change the accent colour"
		git tag plain-look
		git reset -q --hard docs-only
		echo c >design/tokens.toml && git commit -qam "look-fix: raise the contrast of the focus ring"
		git tag look-fix
	)
	fails=0
	expect() {
		if (cd "$repo" && check "$2" "$3" >/dev/null 2>&1); then got=pass; else got=fail; fi
		if [ "$got" = "$1" ]; then
			echo "PASS: $4"
		else
			echo "FAIL: $4: wanted $1, got $got"
			fails=$((fails + 1))
		fi
	}
	expect pass base docs-only "a change outside design/ and presets/ passes"
	expect fail base plain-look "a look change without look-fix: fails"
	expect pass base look-fix "the same kind of change as a look-fix: commit passes"
	[ "$fails" = 0 ] || exit 1
}

case "${1:-}" in
--self-test) self_test ;;
'' | -*)
	echo "usage: sh ci/look-fix.sh FROM TO | --self-test" >&2
	exit 2
	;;
*) check "$1" "${2:?usage: sh ci/look-fix.sh FROM TO}" ;;
esac
