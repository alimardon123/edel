#!/bin/sh
# Fails when a part of Edel OS reaches into another part, so each part
# stands alone (roadmap M8.13; ADR-010's seams, ADR-011's settings file):
#
#   the library      edel never depends on a binary: not edel-compositor,
#                    edel-shell-ui, edel-settings, edel-testclient or
#                    edel-testheader
#   the desktop      edel-compositor, edel-shell-ui and edel-settings never
#                    depend on one another or on the test clients
#   the test client  nothing depends on edel-testclient
#   the cli feature  the desktop parts take edel without its cli feature
#                    (arguments, hashing, signing and downloads)
#   the os feature   the compositor and shell-ui take edel without its os
#                    feature, the system's own work (the install plan, the
#                    feature file reader, firmware, services and the like),
#                    so the desktop links only the settings format and its
#                    models; Settings, the system's own app, takes it (M6.17)
#   the settings     the settings file is written only through
#   file             edel::settings (crates/edel/src/settings.rs); a file
#                    that names it and writes a file of its own must be in
#                    allowed() below, with the reason its write is not the
#                    settings file's
#   the base         the base's tools (rc-update, rc-service, adduser,
#                    addgroup, delgroup, cryptpw, chpasswd, mdev, zzz,
#                    reboot and poweroff by path) are named only in
#                    edel::base (crates/edel/src/base.rs), so the base
#                    stays replaceable (ADR-010, roadmap M6.17)
#
#   the programs     the compositor and shell-ui run only the programs
#                    allowed_program() lists, each with its reason and what
#                    happens where it is missing, so the desktop's run-time
#                    ties are written down like its build-time ones (M6.17);
#                    a program chosen at run time (a person's app, a
#                    preset's session list) is not a tie
#
#   sh ci/seams.sh               check this checkout ("Rust checks")
#   sh ci/seams.sh --self-test   plant one violation of each kind in a copy
#                                and check that each is caught
set -eu

failed=0

# fail TEXT: prints one FAIL line and marks the check as failed.
fail() {
	echo "FAIL: $*"
	failed=1
}

# cargo_tree ROOT LOCK ARGS...: cargo tree in ROOT, offline first. LOCK is
# --locked or empty. A cargo error (not a violation) gets one online try,
# since a registry cache may lack a crate; when that fails too, the last
# lines of cargo's error go to standard error and the status is 1. Only
# the package and feature lines are printed.
cargo_tree() {
	root=$1 lock=$2
	shift 2
	# shellcheck disable=SC2086
	if out=$(cd "$root" && cargo tree --offline $lock "$@" 2>&1); then
		printf '%s\n' "$out" | grep -E '^[a-z0-9_-]+ (v[0-9]|feature ")' || true
	# shellcheck disable=SC2086
	elif out=$(cd "$root" && cargo tree $lock "$@" 2>&1); then
		printf '%s\n' "$out" | grep -E '^[a-z0-9_-]+ (v[0-9]|feature ")' || true
	else
		printf '%s\n' "$out" | tail -n 5 >&2
		return 1
	fi
}

# check_library ROOT LOCK: edel depends on no binary.
check_library() {
	tree=$(cargo_tree "$1" "$2" -p edel -e normal,build --prefix none --format '{p}') || {
		fail "cargo could not read edel's dependencies; run 'cargo tree -p edel' to see why"
		return 0
	}
	for part in edel-compositor edel-shell-ui edel-settings edel-testclient edel-testheader; do
		if printf '%s\n' "$tree" | grep -q "^$part "; then
			fail "edel depends on $part: the library must not use a binary; move the code it needs into crates/edel and drop the dependency"
		fi
	done
}

# check_desktop ROOT LOCK: no desktop part depends on another part or on
# the test client.
check_desktop() {
	for part in edel-compositor edel-shell-ui edel-settings; do
		tree=$(cargo_tree "$1" "$2" -p "$part" -e normal,build --prefix none --format '{p}') || {
			fail "cargo could not read the dependencies of $part; run 'cargo tree -p $part' to see why"
			continue
		}
		for other in edel-compositor edel-shell-ui edel-settings edel-testclient edel-testheader; do
			if [ "$other" != "$part" ] && printf '%s\n' "$tree" | grep -q "^$other "; then
				fail "$part depends on $other: a desktop part stands alone; drop that dependency and share the code through edel"
			fi
		done
	done
}

# check_test_client ROOT LOCK: nothing but edel-testclient itself depends
# on edel-testclient (the inverse tree, with every edge kind).
check_test_client() {
	users=$(cargo_tree "$1" "$2" --workspace -i edel-testclient -e normal,build,dev --prefix none --format '{p}') || {
		fail "cargo could not list what depends on edel-testclient"
		return 0
	}
	names=$(printf '%s\n' "$users" | awk '$1 != "edel-testclient" { print $1 }')
	for user in $names; do
		fail "$user depends on edel-testclient, the test client that only CI's images ship; drop that dependency"
	done
}

# check_features ROOT LOCK: the desktop parts take edel without its cli
# feature, and the compositor and shell-ui without its os feature.
check_features() {
	for part in edel-compositor edel-shell-ui edel-settings; do
		feats=$(cargo_tree "$1" "$2" -p "$part" -e features --prefix none) || {
			fail "cargo could not read the features $part takes; run 'cargo tree -p $part -e features' to see why"
			continue
		}
		if printf '%s\n' "$feats" | grep -qxF 'edel feature "cli"'; then
			fail "$part takes edel's cli feature (arguments, hashing, signing, downloads); set default-features = false on its edel dependency"
		fi
		[ "$part" = edel-settings ] && continue
		if printf '%s\n' "$feats" | grep -qxF 'edel feature "os"'; then
			fail "$part takes edel's os feature (the system's own work); the desktop links only the settings format and its models, so move what it needs out of the os modules or ask through a file or a command"
		fi
	done
}

# code_of FILE: FILE's code without its comment lines and without its tests
# (from the first #[cfg(test)] on), as one-place.sh reads it.
code_of() {
	awk '/^#\[cfg\(test\)\]/ { exit } !/^[ \t]*\/\// { print }' "$1"
}

# allowed FILE: true for a file that names the settings file and writes a
# file of its own, when that write is not the settings file's. Each entry
# gives the reason.
allowed() {
	case "$1" in
	# Writes po/PART.pot, the translation template, when EDEL_WRITE_DOCS is
	# set (check_template); it reads the settings file only for the language.
	crates/edel/src/i18n.rs) return 0 ;;
	# Writes release lists, their signatures and keys; it reads the settings
	# file only for the update channel.
	crates/edel/src/release.rs) return 0 ;;
	# Save as writes the export the person chose, a copy of the settings
	# file's text in a file they pick, not the settings file itself.
	crates/settings/src/system.rs) return 0 ;;
	*) return 1 ;;
	esac
}

# known_gap FILE: true for a file that writes the settings file itself,
# outside edel::settings. It passes with a NOTE line until its writes move
# into edel::settings (roadmap M8.13's finding); remove its line then.
known_gap() {
	case "$1" in
	# Copies the settings file given to edel install onto the new data
	# partition (write, fs::copy).
	crates/edel/src/installer.rs) return 0 ;;
	# Writes the machine's and the person's settings file itself: settle_names
	# (a rename), keep_as_machine_file, seed and edit (set and reset).
	crates/edel/src/machine.rs) return 0 ;;
	*) return 1 ;;
	esac
}

# check_seam ROOT: only edel::settings writes the settings file.
check_seam() {
	files=$(cd "$1" && find crates -name '*.rs' -type f | sort)
	for file in $files; do
		[ "$file" = crates/edel/src/settings.rs ] && continue
		names=$(code_of "$1/$file" | grep -E 'machine_settings\(\)|person_settings\(\)|slot_settings\(\)|places::SETTINGS' || true)
		[ -n "$names" ] || continue
		writes=$(code_of "$1/$file" | grep -E 'fs::write|File::create|OpenOptions::new|fs::rename|fs::copy' || true)
		[ -n "$writes" ] || continue
		allowed "$file" && continue
		if known_gap "$file"; then
			echo "NOTE: $file writes the settings file itself, outside edel::settings; a known gap (M8.13), to move into edel::settings"
			continue
		fi
		fail "$file names the settings file and writes a file itself; the settings file is written only through edel::settings (ADR-011), so call its writer there, or add $file to allowed() in ci/seams.sh with the reason its write is another file"
	done
}

# check_base ROOT: only edel::base names the base's tools, as a word in
# code or in a message.
check_base() {
	files=$(cd "$1" && find crates -name '*.rs' -type f | sort)
	for file in $files; do
		[ "$file" = crates/edel/src/base.rs ] && continue
		named=$(code_of "$1/$file" | grep -oE '(^|[^A-Za-z0-9_-])(rc-update|rc-service|adduser|addgroup|delgroup|cryptpw|chpasswd|mdev|zzz|/sbin/reboot|/sbin/poweroff)([^A-Za-z0-9_-]|$)' | sed 's/^[^A-Za-z0-9/]*//; s/[^A-Za-z0-9-]*$//' | sort -u | tr '\n' ' ' || true)
		[ -n "$named" ] || continue
		fail "$file names the base's tool ${named% }; only edel::base (crates/edel/src/base.rs) names the base's tools (ADR-010), so call its function, or add one there"
	done
}

# allowed_program PART PROGRAM: the programs each desktop part runs by name,
# each with its reason and what happens where it is missing (M6.17).
allowed_program() {
	case "$1 $2" in
	# The compositor starts the shell; without it there is no panel, and the
	# compositor says so.
	"compositor edel-shell-ui") return 0 ;;
	# X11 apps' bridge, started when the first one connects; without it X11
	# apps do not open and Wayland ones do.
	"compositor xwayland-satellite") return 0 ;;
	# The input method region.input_method names; without it typing works
	# with the keyboard's own layouts.
	"compositor fcitx5") return 0 ;;
	# Edel OS's own tool: the update check and the power menu, both only
	# where it is on PATH; elsewhere the check is skipped and the power menu
	# asks logind over D-Bus.
	"shell-ui edel") return 0 ;;
	# Settings, opened on a page; without it the buttons say it could not
	# open.
	"shell-ui edel-settings") return 0 ;;
	# PipeWire's own tools, for the in-use sign and its Stop; without them
	# the sign stays away and shell-ui says so once.
	"shell-ui pw-dump" | "shell-ui pw-cli") return 0 ;;
	# Tells foot that the colour scheme changed (procps or busybox); without
	# it a terminal follows at its next start.
	"shell-ui pkill") return 0 ;;
	*) return 1 ;;
	esac
}

# check_programs ROOT: the compositor and shell-ui run only what
# allowed_program() lists, read from each Command::new: a "NAME", or a
# constant of the same file, else of the part, holding one.
check_programs() {
	for part in compositor shell-ui; do
		files=$(cd "$1" && find "crates/$part/src" -name '*.rs' -type f | sort)
		all=$(for file in $files; do code_of "$1/$file"; done)
		for file in $files; do
			code=$(code_of "$1/$file")
			args=$(printf '%s\n' "$code" | grep -o 'Command::new([^)]*)' | sed 's/^Command::new(//; s/)$//' | sort -u)
			for arg in $args; do
				case "$arg" in
				\"*\") program=${arg#\"} program=${program%\"} ;;
				*)
					name=${arg#&}
					name=${name##*::}
					case "$name" in
					[A-Z]*) ;;
					# A program chosen at run time, not a tie.
					*) continue ;;
					esac
					program=$(printf '%s\n' "$code" | sed -n "s/.*const $name: &str = \"\([^\"]*\)\";.*/\1/p" | head -n 1)
					[ -n "$program" ] || program=$(printf '%s\n' "$all" | sed -n "s/.*const $name: &str = \"\([^\"]*\)\";.*/\1/p" | head -n 1)
					if [ -z "$program" ]; then
						fail "$file runs Command::new($arg), whose program ci/seams.sh cannot read; name it in a const of the part, or as a string"
						continue
					fi
					;;
				esac
				allowed_program "$part" "$program" ||
					fail "$file runs $program, which ci/seams.sh does not list for $part; the desktop runs only what allowed_program() names, so add it there with the reason and what happens where it is missing"
			done
		done
	done
}

# check ROOT LOCK: every rule over the checkout at ROOT.
check() {
	root=$1 lock=$2
	failed=0
	check_library "$root" "$lock"
	check_desktop "$root" "$lock"
	check_test_client "$root" "$lock"
	check_features "$root" "$lock"
	check_seam "$root"
	check_base "$root"
	check_programs "$root"
	[ "$failed" = 0 ]
}

if [ "${1:-}" = --self-test ]; then
	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT
	# seed DIR: copies the tracked files of this checkout into DIR.
	seed() {
		git ls-files | while IFS= read -r f; do
			[ -f "$f" ] && mkdir -p "$1/$(dirname "$f")" && cp "$f" "$1/$f"
		done
	}
	# edit FILE PROGRAM: rewrites FILE with awk PROGRAM.
	edit() {
		awk "$2" "$1" >"$work/edit.tmp" && mv "$work/edit.tmp" "$1"
	}
	ok=1
	# caught COPY WANT: runs the check in COPY and expects the FAIL line
	# WANT. The copy's manifests changed, so its lock file follows: cargo
	# runs without --locked there. A cargo error is not a catch, because
	# only the FAIL line's text counts.
	caught() {
		if out=$(check "$1" ""); then
			rc=0
		else
			rc=1
		fi
		if [ "$rc" = 1 ] && printf '%s\n' "$out" | grep -qF "FAIL: $2"; then
			echo "caught: $2"
		else
			echo "FAIL: the self-test planted $2 and the check missed it"
			ok=0
		fi
	}

	copy="$work/dependency"
	seed "$copy"
	edit "$copy/crates/shell-ui/Cargo.toml" '{ print } /^\[dependencies\]$/ { print "edel-compositor = { path = \"../compositor\" }" }'
	caught "$copy" "edel-shell-ui depends on edel-compositor"

	copy="$work/cli"
	seed "$copy"
	edit "$copy/crates/compositor/Cargo.toml" '/^edel = / { sub(/default-features = false, /, "") } { print }'
	caught "$copy" "edel-compositor takes edel's cli feature"

	copy="$work/os"
	seed "$copy"
	edit "$copy/crates/shell-ui/Cargo.toml" '/^edel = / { sub(/features = \[/, "features = [\"os\", ") } { print }'
	caught "$copy" "edel-shell-ui takes edel's os feature"

	copy="$work/seam"
	seed "$copy"
	printf '%s\n' 'fn probe() {' '    let path = places::person_settings();' '    let _ = std::fs::write(path, "[layout]");' '}' >"$copy/crates/shell-ui/src/seam_probe.rs"
	caught "$copy" "crates/shell-ui/src/seam_probe.rs names the settings file"

	copy="$work/base"
	seed "$copy"
	printf '%s\n' 'fn probe() {' '    let _ = std::process::Command::new("rc-update");' '}' >"$copy/crates/edel/src/base_probe.rs"
	caught "$copy" "crates/edel/src/base_probe.rs names the base's tool rc-update"

	copy="$work/program"
	seed "$copy"
	printf '%s\n' 'fn probe() {' '    let _ = std::process::Command::new("edel-secret-tool");' '}' >"$copy/crates/shell-ui/src/program_probe.rs"
	caught "$copy" "crates/shell-ui/src/program_probe.rs runs edel-secret-tool"

	[ "$ok" = 1 ] && echo "PASS: the self-test caught a desktop part on another, the cli feature in the compositor, the os feature in shell-ui, a settings write in shell-ui, rc-update outside edel::base and an unlisted program in shell-ui"
	[ "$ok" = 1 ]
	exit
fi

if check . --locked; then
	echo "PASS: every part stands alone (the library uses no binary, the desktop parts stand apart from each other and the test client, the compositor and shell-ui link none of the system's own work and run only the programs listed, the settings file is written by edel::settings, bar the known gaps in the NOTE lines, and only edel::base names the base's tools)"
else
	exit 1
fi
