#!/bin/sh
# Fails when a known kind of fact is written outside its owner (ADR-010's
# one owner decision, roadmap M5.27), so a change to one is one edit:
#
#   Edel OS's places  /data/edel, /run/edel, /usr/share/edel and /data/etc,
#                     owned by edel::places (crates/edel/src/places.rs);
#                     scripts read ci/names.sh or the image's places.sh
#   colours           the design tokens (design/tokens.toml): the
#                     compositor and shell-ui draw nothing in a colour of
#                     their own
#   the Alpine branch the base feature (features/base.toml), which CI's
#                     build container follows too
#   the ESP's folder  /EFI/edel, owned by edel::places (ESP_DIR and
#                     on_esp, with the files GRUB_CFG, GRUBENV and
#                     LOADER_TOML), in Rust
#
# Comments, Markdown and Rust tests may name a fact; code may not.
#
#   sh ci/one-place.sh               check this checkout ("Rust checks")
#   sh ci/one-place.sh --self-test   plant one of each in a copy and
#                                    check that each is caught
set -eu

# code FILE: FILE without its comment lines and, for Rust, without its
# tests (everything from #[cfg(test)] on).
code() {
	case "$1" in
	*.rs) awk '/^#\[cfg\(test\)\]/ { exit } !/^[ \t]*\/\// { print FNR ": " $0 }' "$1" ;;
	*) awk '!/^[ \t]*#/ { print FNR ": " $0 }' "$1" ;;
	esac
}

# find_in ROOT KIND PATTERN OWNER FILE...: reports each line of FILE (under
# ROOT) that matches PATTERN outside OWNER.
found=0
find_in() {
	root=$1 kind=$2 pattern=$3 owner=$4
	shift 4
	for file in "$@"; do
		[ -f "$root/$file" ] || continue
		case "$file" in
		# foot reads only its config: a cargo test writes the shipped one from
		# the tokens, as the apps' gtk.css is written from them at run time.
		$owner | ci/one-place.sh | features/terminal/etc/skel/.config/foot/foot.ini | *.md | *.png | *.jpg | *.svg) continue ;;
		esac
		code "$root/$file" | grep -E "$pattern" | grep -v 'places\.sh' | while IFS= read -r line; do
			echo "FAIL: $file:$line: $kind belongs to its owner"
		done | grep . && found=1 || true
	done
}

# check ROOT: every rule over the files of ROOT.
check() {
	root=$1
	found=0
	files=$(cd "$root" && find crates ci features images presets .github -type f 2>/dev/null | grep -v '/target/\|crates/edel/tests/' | sort)
	rust=$(echo "$files" | grep '\.rs$' || true)
	scripts=$(echo "$files" | grep -v '\.rs$' || true)
	desktop=$(echo "$rust" | grep '^crates/\(compositor\|shell-ui\)/' || true)
	# shellcheck disable=SC2086
	{
		find_in "$root" "an Edel OS place" '/data/edel|/run/edel([^-]|$)|/usr/share/edel|/data/etc' 'crates/edel/src/places.rs' $rust
		find_in "$root" "an Edel OS place" '/data/edel|/run/edel([^-]|$)|/usr/share/edel|/data/etc' 'ci/names.sh' $scripts
		find_in "$root" "the ESP's folder" '"/?EFI/edel' 'crates/edel/src/places.rs' $rust
		find_in "$root" "a colour" '"#[0-9a-fA-F]{6}' 'design/tokens.toml' $desktop
		find_in "$root" "the Alpine branch" 'v3\.[0-9]+|alpine:3\.[0-9]+' 'features/base.toml' $rust $scripts
	} >"$root/.one-place.out" 2>&1 || true
	cat "$root/.one-place.out"
	if grep -q '^FAIL' "$root/.one-place.out"; then
		rm -f "$root/.one-place.out"
		return 1
	fi
	rm -f "$root/.one-place.out"
}

if [ "${1:-}" = --self-test ]; then
	copy=$(mktemp -d)
	trap 'rm -rf "$copy"' EXIT
	git ls-files | grep -v '\.\(png\|jpg\)$' | while IFS= read -r f; do
		[ -f "$f" ] && mkdir -p "$copy/$(dirname "$f")" && cp "$f" "$copy/$f"
	done
	# At the top of each file, where code is, not after its tests.
	plant() {
		{ printf '%s\n' "$2"; cat "$copy/$1"; } >"$copy/$1.new" && mv "$copy/$1.new" "$copy/$1"
	}
	plant crates/shell-ui/src/main.rs 'const DATA: &str = "/data/edel";'
	plant crates/compositor/src/frame.rs 'const RED: &str = "#ff0000";'
	plant crates/edel/src/update.rs 'const ENV: &str = "EFI/edel/grubenv";'
	printf '#!/bin/sh\ndocker run alpine:3.24 true\n' >"$copy/ci/new.sh"
	out=$(check "$copy" || true)
	ok=1
	for want in 'shell-ui/src/main.rs.*an Edel OS place' 'compositor/src/frame.rs.*a colour' 'ci/new.sh.*the Alpine branch' "edel/src/update.rs.*the ESP's folder"; do
		echo "$out" | grep -qE "$want" || {
			echo "FAIL: the self-test planted $want and the check missed it"
			ok=0
		}
	done
	[ "$ok" = 1 ] && echo "PASS: the check caught a place in shell-ui, a colour in the compositor, the ESP's folder in the updater and the Alpine branch in a new script"
	[ "$ok" = 1 ]
	exit
fi

if check .; then
	echo "PASS: every place, colour, the ESP's folder and the Alpine branch is written at its owner only"
else
	exit 1
fi
