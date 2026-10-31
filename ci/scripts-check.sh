#!/bin/sh
# Every shell script in the checkout parses, and every script a feature
# copies onto the root is executable (the architecture review of
# 2026-10-31). Paths that run only at shutdown, on resume, on a timer or at
# logout are otherwise first read on a machine. A script is a tracked file
# named *.sh, or one whose first line is a shell's #! line; each is read
# with dash -n, as busybox ash and dash take the same POSIX language.
#
# Usage: sh ci/scripts-check.sh [--self-test]
set -eu

# check DIR: prints one FAIL line for each problem under DIR (a git
# checkout) and ends 1 when there was one.
check() {
	(
		cd "$1"
		err=$(mktemp)
		trap 'rm -f "$err"' EXIT
		failed=0
		git ls-files >"$err.list"
		while IFS= read -r f; do
			[ -f "$f" ] || continue
			first=$(head -n 1 "$f" 2>/dev/null | cut -c 1-40 | tr -d '\0') || first=
			case "$f:$first" in
			*.sh:* | *:'#!/bin/sh'* | *:'#!/sbin/openrc-run'* | *:'#!/bin/ash'* | *:'#!/usr/bin/env sh'*) ;;
			*) continue ;;
			esac
			if ! dash -n "$f" 2>"$err"; then
				echo "FAIL: $f does not parse: $(cat "$err")"
				failed=1
			fi
			case "$f:$first" in
			features/*:'#!'*)
				if [ ! -x "$f" ]; then
					echo "FAIL: $f starts with #! but is not executable; the root gets it as it is in git (git update-index --chmod=+x $f)"
					failed=1
				fi
				;;
			esac
		done <"$err.list"
		rm -f "$err.list"
		exit "$failed"
	)
}

if [ "${1:-}" = --self-test ]; then
	copy=$(mktemp -d)
	trap 'rm -rf "$copy"' EXIT
	git -C "$copy" init -q
	mkdir -p "$copy/features/demo/usr/libexec"
	printf '#!/bin/sh\nif true; then\n\techo ok\n' >"$copy/features/demo/usr/libexec/broken"
	chmod +x "$copy/features/demo/usr/libexec/broken"
	printf '#!/bin/sh\necho ok\n' >"$copy/features/demo/usr/libexec/plain"
	printf 'echo ok\n' >"$copy/fine.sh"
	git -C "$copy" add -A
	git -C "$copy" update-index --chmod=-x features/demo/usr/libexec/plain
	out=$(check "$copy") && {
		echo "FAIL: the self-test's planted mistakes passed"
		exit 1
	}
	for want in "broken does not parse" "plain starts with #! but is not executable"; do
		case "$out" in
		*"$want"*) ;;
		*)
			echo "FAIL: the self-test missed: $want"
			echo "$out"
			exit 1
			;;
		esac
	done
	case "$out" in
	*fine.sh*)
		echo "FAIL: the self-test flagged a good script: $out"
		exit 1
		;;
	esac
	echo "PASS: scripts-check caught a script that does not parse and one that is not executable"
	exit 0
fi

check .
echo "PASS: every shell script parses and every feature's script is executable"
