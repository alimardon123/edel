#!/bin/sh
# desktop-test.sh CASE... (roadmap M4.1 to M4.5): boots the CI desktop image
# (ci/desktop/vm.toml) once, with a virtual GPU, keyboard and tablet, a
# QMP socket for ci/qmp.py and a second serial port for commands to its
# test service, waits for that service's results on the serial console,
# then runs each CASE against the running VM:
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
#   titlebar    every window with a title bar in the token colours and its
#               title on it; a click on two's close button closes it, and
#               dragging one's bar moves it
#   tiling      edel system set shell.tiling=true, sent to the VM, tiles
#               foot and one side by side with their bars; Super+T puts
#               them back where they floated, overlapping
#   pointer     the cursor where the pointer is (drawn into the frame in
#               CI, EDEL_SOFTWARE_CURSOR=1), and wayland-info lists the
#               tablet, cursor shape, fractional scale and viewporter
#               protocols
#   outputs     both screens of virtio-vga,max_outputs=2 lit (the second
#               forced on by the image's kernel command line, at the seed
#               system file's mode 1024x768), side by side in the state
#               file, and the second turned off by
#               edel system set outputs.Virtual-2.enabled=false
#   scale       edel system set outputs.Virtual-1.scale=2 halves the
#               logical screen and doubles the title bar's height in
#               screen pixels, after the other cases
#
# Screenshots and the serial log are kept in out/desktop-test/. The VM
# gets 2 GiB and 4 CPUs; the VM and server tests keep 512 MiB and 2, so
# their budgets stay honest.
set -eu
. ci/vm.sh

dir=out/desktop-test
log=out/desktop-test.log
export QMP="$dir/qmp.sock"
commands="$dir/commands.sock"
rm -f "$QMP" "$commands" "$dir"/*.png

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

# token KEY: the colour KEY has in design/tokens.toml, as rrggbb.
token() {
	sed -n "s/^$1 = \"#\\([0-9a-f]\\{6\\}\\)\".*/\\1/p" design/tokens.toml
}

# guest COMMAND: sends one line to the test service, which runs the
# commands it knows as root and prints "ran COMMAND: STATUS".
guest() {
	python3 -c 'import socket, sys, time
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n")
# QEMU drops what it has not passed to the serial port when the socket
# closes, so the line gets a second to go through.
time.sleep(1)
s.close()' "$commands" "$1"
}

# wait_for PATTERN: waits up to 20 s for a serial line matching PATTERN.
wait_for() {
	i=0
	while ! tr -d '\r' <"$log" | grep -qE "$1"; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || return 1
		sleep 0.2
	done
}

# The background colour the compositor clears to, from the design tokens.
background=$(token background)
# foot's own background. Window frames open centred and cascade (M4.3),
# each a 28 px title bar above the window and a 1 px border round the
# rest (M4.4): foot, asked for 400x300, draws 396x288, whole character
# cells, and opens first at 442,269; 460,530 is inside it but outside
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
	# Up and left of the click, where the cursor does not reach.
	shot typed $((foot_x - 8)) $((foot_y - 8)) "$background" >/dev/null ||
		fail "typing exit did not close foot; $((foot_x - 8)),$((foot_y - 8)) is not the background #$background"
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
	# On a 1280x800 screen, centred frames share the centre 640,400, so
	# one's frame, opened after foot's, sits 32 px down and right of
	# centred, and two's 64 px; each window is 28 px below its frame's top
	# and 1 px inside its left edge.
	python3 - "$dir/state.toml" <<-'EOF' || fail "the state file does not show the test clients where the floating policy puts them"
		import sys, tomllib
		state = tomllib.load(open(sys.argv[1], "rb"))
		windows = {w["title"]: w for w in state["windows"]}
		place = lambda t: (windows[t]["x"], windows[t]["y"], windows[t]["width"], windows[t]["height"])
		assert state["format"] == 1 and state["policy"] == "floating", state
		assert place("one") == (522, 345, 300, 200), place("one")
		assert place("two") == (604, 402, 200, 150), place("two")
		assert windows["two"]["focused"], "the newest window has the keyboard"
		assert [w["title"] for w in state["windows"]][-2:] == ["one", "two"], "two is on top"
	EOF
	shot floating 530 353 cc3333 >/dev/null || fail "test client one is not red at 530,353"
	shot floating 704 477 3366cc >/dev/null || fail "test client two is not blue at its centre, 704,477"
	echo "PASS: the floating policy placed foot and two test clients centred and cascading, the state file lists them, and each shows its colour"
}

case_titlebar() {
	bar=$(token title_bar)
	focused=$(token title_bar_focused)
	sed -n 's/.*DESKTOP-TEST: state //p' "$log" | tr -d '\r' >"$dir/state.toml"
	# Each bar's left end, 5 px in from its window's left edge and 24 px
	# above its top, from the state file, since foot sizes itself to whole
	# character cells: two's lighter, as it has the keyboard.
	bars=$(
		python3 - "$dir/state.toml" <<-'EOF'
			import sys, tomllib
			windows = tomllib.load(open(sys.argv[1], "rb"))["windows"]
			assert len(windows) == 3 and all(w["title_bar"] for w in windows), windows
			for w in windows:
			    print(w["title"], w["x"] + 5, w["y"] - 24)
		EOF
	) || fail "the state file does not give foot and the test clients title bars"
	while read -r title x y; do
		want=$bar
		[ "$title" = two ] && want=$focused
		shot titlebar "$x" "$y" "$want" >/dev/null || fail "$title's title bar is not #$want at $x,$y"
	done <<-EOF
		$bars
	EOF
	text=$(python3 ci/qmp.py uniform "$dir/titlebar.png" 610 376 135 24)
	[ "$text" = varied ] || fail "two's title bar shows no title: it is $text"
	# Close two with its close button, the bar's rightmost 28 px.
	python3 ci/qmp.py click 791 388
	wait_for 'edel-compositor: unmapped window two' ||
		fail "clicking two's close button at 791,388 did not close it"
	wait_for 'DESKTOP-TEST: windows 2 ' || fail "the state file still lists two"
	# Drag one by its bar 200 px right and up; foot stays uncovered at
	# $foot_x,$foot_y for the console case.
	python3 ci/qmp.py drag 560 331 760 131
	wait_for 'DESKTOP-TEST: windows 2 .*one@72[123],14[456],300x200' ||
		fail "dragging one's title bar from 560,331 to 760,131 did not move it to about 722,145: $(value windows)"
	echo "PASS: foot and the test clients have title bars in the token colours with their titles, two's close button closed it and one's bar moved it"
}

case_tiling() {
	# Through the system file, as Settings and people will: the compositor
	# follows it and re-lays the windows out at once. foot is the master on
	# the left, one the stack on the right, 8 px apart and from the edges.
	guest 'tiling on'
	wait_for 'DESKTOP-TEST: ran tiling on: 0' ||
		fail "edel system set shell.tiling=true did not run in the VM"
	wait_for 'edel-compositor: windows now tiling' ||
		fail "the compositor did not follow shell.tiling = true"
	wait_for 'DESKTOP-TEST: windows 2 foot@9,36,[0-9]+x[0-9]+ one@645,36,626x755' ||
		fail "foot and one are not tiled side by side: $(value windows)"
	tiled=$(value windows)
	python3 - "$tiled" <<-'EOF' || fail "the tiled windows overlap: $tiled"
		import re, sys
		frames = [(int(x) - 1, int(y) - 28, int(w) + 2, int(h) + 29)
		          for x, y, w, h in re.findall(r"@(\d+),(\d+),(\d+)x(\d+)", sys.argv[1])]
		(ax, ay, aw, ah), (bx, by, bw, bh) = frames
		assert ax + aw <= bx or bx + bw <= ax or ay + ah <= by or by + bh <= ay, frames
	EOF
	# Each tile keeps its title bar: their left ends, as in titlebar.
	bar=$(token title_bar)
	focused=$(token title_bar_focused)
	colour=$(shot tiling 14 12 "!$(token background)") ||
		fail "foot's tile shows no title bar at 14,12"
	[ "$colour" = "$bar" ] || [ "$colour" = "$focused" ] ||
		fail "foot's tile's bar is #$colour at 14,12, not a title bar colour"
	colour=$(shot tiling 650 12 "!$(token background)") ||
		fail "one's tile shows no title bar at 650,12"
	[ "$colour" = "$bar" ] || [ "$colour" = "$focused" ] ||
		fail "one's tile's bar is #$colour at 650,12, not a title bar colour"
	# Super+T: this workspace floats again, each window where it was.
	python3 ci/qmp.py key meta_l-t
	wait_for 'edel-compositor: windows now floating' || fail "Super+T did not switch back to floating"
	wait_for 'DESKTOP-TEST: windows 2 foot@442,269,396x288 one@722,145,300x200' ||
		fail "back in floating, foot and one are not where they floated: $(value windows)"
	echo "PASS: edel system set shell.tiling=true tiled foot and one side by side with their title bars ($tiled), and Super+T floated them back where they were"
}

case_pointer() {
	globals=$(value globals)
	for protocol in zwp_tablet_manager_v2 wp_cursor_shape_manager_v1 wp_fractional_scale_manager_v1 wp_viewporter zxdg_decoration_manager_v1; do
		case " $globals " in
		*" $protocol "*) ;;
		*) fail "the compositor does not offer $protocol; wayland-info listed: $globals" ;;
		esac
	done
	# The pointer's tip at 100,700, on the background: the arrow's black
	# body below and right of it, nothing left of it.
	python3 ci/qmp.py move 100 700
	shot pointer 102 709 000000 >/dev/null || fail "no cursor at the pointer: 102,709 is not black"
	shot pointer 97 700 "$background" >/dev/null || fail "97,700, left of the cursor, is not the background"
	echo "PASS: the cursor is where the pointer is, and the compositor offers tablets, cursor shapes, fractional scale and viewporter"
}

case_outputs() {
	n=$(tr -d '\r' <"$log" | grep -c 'DESKTOP-TEST: screen output Virtual-[12] [0-9]*x[0-9]* ready' || true)
	[ "$n" = 2 ] || fail "$n screens showed a first frame, not 2: $(grep 'DESKTOP-TEST: screen' "$log" | tr -d '\r')"
	sed -n 's/.*DESKTOP-TEST: state //p' "$log" | tr -d '\r' >"$dir/state.toml"
	python3 - "$dir/state.toml" <<-'EOF' || fail "the state file does not show the two screens side by side"
		import sys, tomllib
		outputs = {o["name"]: o for o in tomllib.load(open(sys.argv[1], "rb"))["outputs"]}
		place = lambda n: (outputs[n]["x"], outputs[n]["y"], outputs[n]["width"], outputs[n]["height"])
		assert place("Virtual-1") == (0, 0, 1280, 800), place("Virtual-1")
		assert place("Virtual-2") == (1280, 0, 1024, 768), place("Virtual-2")
	EOF
	guest 'screen 2 off'
	wait_for 'DESKTOP-TEST: ran screen 2 off: 0' ||
		fail "edel system set outputs.Virtual-2.enabled=false did not run in the VM"
	wait_for 'edel-compositor: output Virtual-2 off' ||
		fail "the compositor did not turn Virtual-2 off"
	echo "PASS: two screens lit side by side, Virtual-1 at 0,0 and Virtual-2 at 1280,0 in the system file's mode 1024x768, and outputs.Virtual-2.enabled = false turned the second off"
}

case_scale() {
	guest 'scale 2'
	wait_for 'DESKTOP-TEST: ran scale 2: 0' ||
		fail "edel system set outputs.Virtual-1.scale=2 did not run in the VM"
	wait_for 'edel-compositor: output Virtual-1 scale 2' ||
		fail "the compositor did not follow outputs.Virtual-1.scale = 2"
	# The screen is 640x400 logical pixels now, so one's frame, last at
	# 721,117 and 302x229, is pulled in to 338,117.
	wait_for 'DESKTOP-TEST: windows 1 one@339,145,300x200' ||
		fail "one is not where the smaller screen puts it: $(value windows)"
	# Its 28 px bar is 56 screen pixels from y 234: row 284 is still the
	# bar, row 294 the window.
	focused=$(token title_bar_focused)
	shot scale 688 284 "$focused" >/dev/null ||
		fail "at scale 2, 688,284 is not one's bar, #$focused: the bar is not 56 pixels high"
	shot scale 688 294 cc3333 >/dev/null || fail "at scale 2, 688,294 is not one's red"
	echo "PASS: outputs.Virtual-1.scale = 2 applied at once: a 640x400 screen and a title bar 56 pixels high"
}

[ "$#" -gt 0 ] || set -- floating titlebar tiling console pointer outputs compositor scale
for c in "$@"; do
	case "$c" in
	console | compositor | floating | outputs | pointer | scale | tiling | titlebar) ;;
	*)
		echo "unknown case $c; the cases are console, compositor, floating, outputs, pointer, scale, tiling and titlebar"
		exit 1
		;;
	esac
done

keep_vm=1 run_vm "$log" 'DESKTOP-TEST: (done|FAIL)' "${DESKTOP_TEST_TIMEOUT:-300}" -no-reboot -snapshot \
	-m 2048 -smp 4 -vga none -device virtio-vga,max_outputs=2 \
	-device virtio-keyboard-pci -device virtio-tablet-pci \
	-qmp unix:"$QMP",server=on,wait=off \
	-serial unix:"$commands",server=on,wait=off \
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
