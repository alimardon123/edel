#!/bin/sh
# desktop-test.sh CASE... (roadmap M4.1, M4.2b, M4.3): boots the CI desktop image
# (ci/desktop/vm.toml) once, with a virtual GPU, keyboard and tablet and
# a QMP socket for ci/qmp.py, waits for its test service's results on the
# serial console, then runs each CASE against the running VM:
#
#   console     the login prompt on serial, /run/user/UID made with mode
#               0700 by pam_rundir for the autologin, foot's window on a
#               screenshot that is not one colour, and typing `exit` after
#               a click into foot closes it
#   compositor  the compositor's ready line, the top left corner in the
#               background colour of design/tokens.toml, and its numbers
#               (boot to ready, memory, its RSS, frame p99, idle frames)
#               within ci/budgets.toml, written to the step summary
#   floating    the two test clients where the floating policy puts them,
#               in the state file and in their colours on screen
#
# Screenshots and the serial log are kept in out/desktop-test/. The VM
# gets 2 GiB and 4 CPUs; the VM and server tests keep 512 MiB and 2, so
# their budgets stay honest.
set -eu
. ci/vm.sh

dir=out/desktop-test
log=out/desktop-test.log
export QMP="$dir/qmp.sock"
rm -f "$QMP" "$dir"/*.png

say() {
	echo "DESKTOP-TEST: $*"
}

fail() {
	echo "FAIL: $*"
	stop_vm
	exit 1
}

# value KEY: the value the test service printed for KEY; a line may
# start with the login prompt, which getty prints without a newline.
value() {
	sed -n "s/.*DESKTOP-TEST: $1 //p" "$log" | tr -d '\r' | tail -n 1
}

# shot NAME X Y WANT: takes screenshots into $dir/NAME.png, one a second
# for up to 10 s, until the pixel at X, Y is #WANT, or with WANT !RRGGBB
# until it is anything else; prints the last colour and fails if it never
# was. Slow runners draw late.
shot() {
	i=0
	while :; do
		python3 ci/qmp.py screendump "$dir/$1.png"
		colour=$(python3 ci/qmp.py pixel "$dir/$1.png" "$2" "$3")
		case "$4" in
		!*) [ "$colour" != "${4#!}" ] && break ;;
		*) [ "$colour" = "$4" ] && break ;;
		esac
		i=$((i + 1))
		[ "$i" -lt 10 ] || break
		sleep 1
	done
	echo "$colour"
	case "$4" in
	!*) [ "$colour" != "${4#!}" ] ;;
	*) [ "$colour" = "$4" ] ;;
	esac
}

# budget KEY: the number KEY has in ci/budgets.toml.
budget() {
	awk -F= -v key="$1" '{ k = $1; gsub(/ /, "", k) } k == key { v = $2; sub(/#.*/, "", v); gsub(/ /, "", v); print v }' ci/budgets.toml
}

# The background colour the compositor clears to, from the design tokens.
background=$(sed -n 's/^background = "#\([0-9a-f]\{6\}\)".*/\1/p' design/tokens.toml)
# foot's own background. Windows open centred and cascade (M4.3): foot,
# 400x300, opens first at 440,250, and 460,530 is inside it but outside
# the test clients opened after it.
foot=242424
foot_x=460 foot_y=530

case_console() {
	grep -q 'edel login:' "$log" || fail "no login prompt on the serial console"
	rundir=$(value rundir)
	[ "$rundir" = "700 ci" ] || fail "/run/user/UID of ci is \"$rundir\", not mode 700 owned by ci"
	shot console "$foot_x" "$foot_y" "$foot" >/dev/null ||
		fail "foot's window is not on the screen: $foot_x,$foot_y is not #$foot"
	uniform=$(python3 ci/qmp.py uniform "$dir/console.png")
	[ "$uniform" = varied ] || fail "the screen is $uniform"
	# Click into foot and type into its shell: the keyboard reaches the
	# window through libinput and the seat, and the window closes.
	python3 ci/qmp.py click "$foot_x" "$foot_y"
	python3 ci/qmp.py type 'exit\n'
	shot typed "$foot_x" "$foot_y" "$background" >/dev/null ||
		fail "typing exit did not close foot; $foot_x,$foot_y is not the background #$background"
	echo "PASS: the desktop image booted to the login prompt, logged ci in to the compositor with /run/user/UID at 0700, showed foot and closed it when exit was typed (${waited}s)"
}

case_compositor() {
	line=$(value ready_line)
	case "$line" in
	"output "*" ready") ;;
	*) fail "the compositor wrote no ready line to /run/edel/session/ready: \"$line\"" ;;
	esac
	# Windows open centred (M4.3), so the background shows in a corner.
	shot compositor 20 20 "$background" >/dev/null ||
		fail "the top left corner of the screen is not the background #$background"
	over=0
	rows=''
	# check WHAT MEASURED KEY UNIT
	check() {
		limit=$(budget "$3")
		if [ -n "$2" ] && awk -v m="$2" -v b="$limit" 'BEGIN { exit !(m <= b) }'; then
			verdict=ok
		else
			verdict='**over**'
			over=1
		fi
		echo "budget $3: ${2:-none} $4, budget $limit $4, $verdict"
		rows="$rows| $1 | ${2:-none} $4 | $limit $4 | $verdict |
"
	}
	check 'Boot to the compositor ready' "$(value ready_seconds)" desktop_ready_seconds s
	check 'Memory in use, session idle' "$(value memory_in_use_mib)" desktop_memory_mib MiB
	check 'Compositor RSS' "$(value compositor_rss_mib)" compositor_rss_mib MiB
	check 'Time between frames, p99' "$(value frame_p99_ms)" frame_p99_ms ms
	check 'Frames drawn while idle' "$(value idle_frames)" idle_frames ''
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		cat >>"$GITHUB_STEP_SUMMARY" <<-EOF
			### Desktop budgets (our compositor under llvmpipe, M4.2b)

			| Measure | Measured | Budget | Verdict |
			|---|---|---|---|
			$rows
			Frames: $(value frames), median $(value frame_p50_ms) ms between them; $(value ready_line); $(value telemetry).

		EOF
	fi
	[ "$over" = 0 ] || fail "the compositor is over a budget in ci/budgets.toml"
	echo "PASS: $(value ready_line) in $(value ready_seconds) s, the centre is the background, every budget met; $(value telemetry)"
}

case_floating() {
	sed -n 's/.*DESKTOP-TEST: state //p' "$log" | tr -d '\r' >"$dir/state.toml"
	# On a 1280x800 screen, centred windows share the centre 640,400, so
	# one, opened after foot, sits 32 px down and right of centred, and
	# two 64 px.
	python3 - "$dir/state.toml" <<-'EOF' || fail "the state file does not show the test clients where the floating policy puts them"
		import sys, tomllib
		state = tomllib.load(open(sys.argv[1], "rb"))
		windows = {w["title"]: w for w in state["windows"]}
		place = lambda t: (windows[t]["x"], windows[t]["y"], windows[t]["width"], windows[t]["height"])
		assert state["format"] == 1 and state["policy"] == "floating", state
		assert place("one") == (522, 332, 300, 200), place("one")
		assert place("two") == (604, 389, 200, 150), place("two")
		assert windows["two"]["focused"], "the newest window has the keyboard"
		assert [w["title"] for w in state["windows"]][-2:] == ["one", "two"], "two is on top"
	EOF
	shot floating 530 340 cc3333 >/dev/null || fail "test client one is not red at 530,340"
	shot floating 704 464 3366cc >/dev/null || fail "test client two is not blue at its centre, 704,464"
	echo "PASS: the floating policy placed foot and two test clients centred and cascading, the state file lists them, and each shows its colour"
}

[ "$#" -gt 0 ] || set -- console compositor floating
for c in "$@"; do
	case "$c" in
	console | compositor | floating) ;;
	*)
		echo "unknown case $c; the cases are console, compositor and floating"
		exit 1
		;;
	esac
done

keep_vm=1 run_vm "$log" 'DESKTOP-TEST: (done|FAIL)' "${DESKTOP_TEST_TIMEOUT:-300}" -no-reboot -snapshot \
	-m 2048 -smp 4 -vga none -device virtio-vga \
	-device virtio-keyboard-pci -device virtio-tablet-pci \
	-qmp unix:"$QMP",server=on,wait=off \
	-drive if=none,id=disk0,format=raw,file="$dir/edel-desktop-x86_64.img" \
	-device virtio-blk-pci,drive=disk0,bootindex=0
grep 'DESKTOP-TEST' "$log" | tr -d '\r' || true
if [ "$found" = 0 ] || grep -q 'DESKTOP-TEST: FAIL' "$log"; then
	cat "$log"
	fail "the desktop test service gave no results; see the serial log above"
fi
# getty prints the prompt once the default runlevel is done, which may be
# after the results.
i=0
while ! grep -q 'edel login:' "$log" && [ "$i" -lt 60 ]; do
	sleep 1
	i=$((i + 1))
done
for c in "$@"; do
	"case_$c"
done
stop_vm
