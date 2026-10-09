#!/bin/sh
# The desktop on another distribution (roadmap M5.15a, ADR-002's separable
# desktop decision): runs edel-compositor nested under Xvfb on this
# machine's own libraries, with none of Edel OS's folders, lets it start
# edel-shell-ui, maps a test client, and checks two pixels of the screen:
# the panel in its token colour and the test client in its own. "Rust
# checks" runs it on GitHub's Ubuntu runner, outside the Alpine build.
#
#   cargo build --locked -p edel-compositor -p edel-shell-ui -p edel-testclient
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
mkdir -m 700 "$work/run" "$work/home"

# show: what each part said, for a failure.
show() {
	for log in compositor client; do
		echo "--- $log" >&2
		cat "$work/$log.log" >&2 2>/dev/null || true
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

export DISPLAY="$display" XDG_RUNTIME_DIR="$work/run" HOME="$work/home"
export PATH="$bin:$PATH" LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY XDG_DATA_DIRS XDG_CONFIG_HOME XDG_DATA_HOME
edel-compositor >"$work/compositor.log" 2>&1 &
pids="$pids $!"
wait_for "$work/compositor.log" 'listening on wayland-' 30
socket=$(sed -n 's/^edel-compositor: listening on //p' "$work/compositor.log" | head -n 1)
wait_for "$work/compositor.log" 'edel-shell-ui: panel edel-panel along the bottom' 30

WAYLAND_DISPLAY=$socket edel-testclient --size 300x200 --colour "$colour" --title elsewhere >"$work/client.log" 2>&1 &
pids="$pids $!"
wait_for "$work/compositor.log" 'mapped window elsewhere at' 30
# The panel redraws its window list for the new window; let it land.
sleep 2

pixel() {
	convert "$work/screen.png" -format "%[hex:p{$1,$2}]" info: | cut -c1-6
}
# The window's middle, from the place the compositor gave it.
place=$(sed -n 's/^edel-compositor: mapped window elsewhere at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\)$/\1 \2 \3 \4/p' "$work/compositor.log" | head -n 1)
set -- $place
want=$(echo "$colour" | tr a-f A-F)
# A window is placed before its first picture reaches the screen, and a
# software renderer on a busy runner can take a while: look again, on a
# new screenshot, for up to 10 s before calling it a failure.
tries=0
while :; do
	import -display "$display" -window root "$work/screen.png"
	window=$(pixel $(($1 + $3 / 2)) $(($2 + $4 / 2)))
	# The panel lies along the bottom; its middle is empty in Classic.
	bar=$(pixel 640 796)
	[ "$bar" = "$panel" ] && [ "$window" = "$want" ] && break
	tries=$((tries + 1))
	if [ "$tries" -ge 10 ]; then
		echo "FAIL: the panel is #$bar (its token is #$panel) and the test client #$window (it draws #$colour), after $tries screenshots 1 s apart" >&2
		show
		exit 1
	fi
	sleep 1
done
echo "PASS: on this machine's libraries, with no $share_dir, $data_dir or $run_dir, the compositor started shell-ui, whose panel is #$panel as its token says, and mapped a test client in #$colour"
