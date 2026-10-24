#!/bin/sh
# The desktop on another distribution (roadmap M5.15a, ADR-002's separable
# desktop decision): runs edel-compositor nested under Xvfb on this
# machine's own libraries, with none of Edel OS's folders, lets it start
# edel-shell-ui, maps a test client, and checks the screen's pixels: the
# panel in its token colour and the test client in its own. It does so
# once at each effects tier, forced with EDEL_EFFECTS, and finds the
# window's lower left corner rounded on Full and Balanced and square on
# Lite (M5.14's corners, built in M8.21 part 21c), which the VM tests,
# held at Lite by their software renderer, cannot show. "Rust checks"
# runs it on GitHub's Ubuntu runner, outside the Alpine build.
#
#   cargo build --locked -p edel-compositor -p edel-shell-ui -p edel-testclient --features edel-compositor/winit
#   sh ci/elsewhere-test.sh
#
# Needs Xvfb, ImageMagick's import and convert, and Mesa's software
# renderer (xvfb imagemagick libegl1 libgl1-mesa-dri libxkbcommon-x11-0 on
# Ubuntu).
set -eu

. ci/names.sh
bin=${EDEL_BIN:-$PWD/target/debug}
colour=336699

for place in "$share_dir" "$data_dir" "$run_dir"; do
	if [ -e "$place" ]; then
		echo "FAIL: $place exists here, so this run would not show the desktop working without Edel OS's folders; run it on a machine without them" >&2
		exit 1
	fi
done

# The panel's colour in the release's default scheme, light, from its owner.
panel=$(awk '/^\[/ { light = ($0 == "[colour.light]") } light && /^panel *=/ { gsub(/[" #]/, "", $3); print toupper($3); exit }' design/tokens.toml)
[ -n "$panel" ] || {
	echo "FAIL: no panel colour in design/tokens.toml's [colour.light]" >&2
	exit 1
}

work=$(mktemp -d)
pids=
finish() {
	for pid in $pids; do kill "$pid" 2>/dev/null || true; done
	rm -rf "$work"
}
trap finish EXIT

# show: what each part said, for a failure.
show() {
	for part in compositor client; do
		echo "--- $part at $tier" >&2
		cat "$work/$tier-$part.log" >&2 2>/dev/null || true
	done
}

# wait_for FILE TEXT SECONDS: until FILE has a line with TEXT.
wait_for() {
	n=0
	until grep -q "$2" "$1" 2>/dev/null; do
		n=$((n + 1))
		if [ "$n" -gt $(($3 * 5)) ]; then
			echo "FAIL: no \"$2\" within $3 s" >&2
			show
			exit 1
		fi
		sleep 0.2
	done
}

display=:73
Xvfb "$display" -screen 0 1280x800x24 -nolisten tcp >"$work/xvfb.log" 2>&1 &
pids="$pids $!"
sleep 1

export DISPLAY="$display"
export PATH="$bin:$PATH" LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY XDG_DATA_DIRS XDG_CONFIG_HOME XDG_DATA_HOME
# pixel X Y: the colour at X,Y of the last screenshot, as RRGGBB.
pixel() {
	convert "$work/$tier.png" -format "%[hex:p{$1,$2}]" info: | cut -c1-6
}

# session: one session at $tier, its panel, window and window's corner
# checked, then ended.
session() {
	mkdir -m 700 "$work/$tier-run" "$work/$tier-home"
	log=$work/$tier-compositor.log
	EDEL_EFFECTS=$tier XDG_RUNTIME_DIR="$work/$tier-run" HOME="$work/$tier-home" edel-compositor >"$log" 2>&1 &
	compositor=$!
	pids="$pids $compositor"
	wait_for "$log" 'listening on wayland-' 30
	wait_for "$log" "tier=$tier (EDEL_EFFECTS)" 5
	socket=$(sed -n 's/^edel-compositor: listening on //p' "$log" | head -n 1)
	wait_for "$log" 'edel-shell-ui: panel edel-panel along the bottom' 30

	XDG_RUNTIME_DIR="$work/$tier-run" WAYLAND_DISPLAY=$socket edel-testclient --size 300x200 --colour "$colour" --title elsewhere >"$work/$tier-client.log" 2>&1 &
	client=$!
	pids="$pids $client"
	wait_for "$log" 'mapped window elsewhere at' 30
	# The panel redraws its window list for the new window; let it land.
	sleep 2

	# A point a quarter in from the window's lower left, from the place the
	# compositor gave it: the pointer rests near the screen's middle, and a
	# window placed before the panel took its room has its middle under the
	# pointer's white arrow.
	place=$(sed -n 's/^edel-compositor: mapped window elsewhere at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\)$/\1 \2 \3 \4/p' "$log" | head -n 1)
	set -- $place
	case $tier in
	lite) expected=square ;;
	*) expected=rounded ;;
	esac
	# A window is placed before its first picture reaches the screen, and a
	# software renderer on a busy runner can take a while: look again, on a
	# new screenshot, for up to 10 s before calling it a failure.
	tries=0
	while :; do
		import -display "$display" -window root "$work/$tier.png"
		window=$(pixel $(($1 + $3 / 4)) $(($2 + $4 * 3 / 4)))
		# The panel lies along the bottom; its middle is empty in Classic.
		bar=$(pixel 640 796)
		# The window's last pixel at its lower left, which a rounded
		# corner leaves out, and one well inside that corner's curve.
		corner=$(pixel "$1" $(($2 + $4 - 1)))
		inside=$(pixel $(($1 + 12)) $(($2 + $4 - 13)))
		shape=rounded
		[ "$corner" = "$want" ] && shape=square
		[ "$bar" = "$panel" ] && [ "$window" = "$want" ] && [ "$inside" = "$want" ] && [ "$shape" = "$expected" ] && break
		tries=$((tries + 1))
		if [ "$tries" -ge 10 ]; then
			echo "FAIL: at $tier the panel is #$bar (its token is #$panel), the test client #$window and #$inside 12 px inside its lower left corner (it draws #$colour), and that corner's last pixel #$corner, so $shape where it should be $expected, after $tries screenshots 1 s apart" >&2
			show
			exit 1
		fi
		sleep 1
	done
	kill "$client" "$compositor" 2>/dev/null || true
	wait "$compositor" 2>/dev/null || true
	echo "at $tier: the panel #$panel, the test client #$colour, its lower left corner $shape (#$corner)"
}

want=$(echo "$colour" | tr a-f A-F)
for tier in lite balanced full; do
	session
done
echo "PASS: on this machine's libraries, with no $share_dir, $data_dir or $run_dir, the compositor started shell-ui, whose panel is #$panel as its token says, and mapped a test client in #$colour, at each effects tier, the window's corner rounded on Full and Balanced and square on Lite"
