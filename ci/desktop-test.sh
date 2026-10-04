#!/bin/sh
# desktop-test.sh CASE... (roadmap M4.1 to M4.5): boots the CI desktop image
# (ci/desktop/vm.toml) once, with a virtual GPU, keyboard, tablet and
# mouse, a QMP socket for ci/qmp.py and a second serial port for commands
# to its test service, waits for that service's results on the serial
# console, then runs each CASE against the running VM:
#
#   console     the login prompt on serial, /run/user/UID made with mode
#               0700 by pam_rundir for the autologin, foot's window on a
#               screenshot that is not one colour, and typing `exit` after
#               a click into foot closes it
#   compositor  the compositor's ready line, its effect tier (Full, which
#               CI's session forces, logged, and the same from edel shell
#               tier), the top left corner in the background colour of
#               design/tokens.toml, and its numbers (boot to ready, memory,
#               its RSS, frame p99, idle frames) within ci/budgets.toml,
#               written to the step summary
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
#               edel system set outputs.Virtual-2.enabled=false; a
#               window opens on the screen the mouse took the pointer to
#               (M5.2g)
#   xwayland    no X11 process at first; xclock, an X11 app, starts XWayland
#               through xwayland-satellite and opens with our title bar,
#               whose close button closes it
#   layers      a second layer-shell panel (the test client, --layer
#               bottom) lies above shell-ui's along the bottom in its
#               colour, keeps its height free of tiled windows, and goes
#               when closed (M5.1a)
#   panel       shell-ui's panel along the bottom in the token colour with
#               its clock drawn, within its memory budget, and back after
#               kill -9 (M5.1b); its layout toggle tiles the windows and
#               floats them again (M5.3a); a screen reader reads it over
#               AT-SPI (M5.1d)
#   animations  appearance.motion = "reduced" logs fades only and its
#               removal logs the tier's own animations again; ten windows
#               opening one after another and closing at the tier llvmpipe
#               picks keep the frame budget (M5.11b)
#   shortcuts   edel system set shortcuts.close=Super+W, sent to the VM,
#               moves close: Super+Q then leaves a window open and Super+W
#               closes it; Ctrl+Alt+T opens a terminal; removing the key
#               brings Super+Q back (M5.13a)
#   workspaces  Super+Shift+2 sends a new window to workspace 2, which
#               the state file says while workspace 1 stays shown;
#               Super+2 shows it tiled, as shell.tiling from the tiling
#               case says, Super+T floats that workspace alone, Super+1
#               brings workspace 1 back as it was, and the window closed
#               while hidden leaves the state file (M5.2a); then
#               edel-testclient --workspace 3, over ext-workspace-v1, sees
#               four workspaces with the first shown and shows the third,
#               and --workspace 1 brings the first back (M5.2b); over
#               wlr-foreign-toplevel-management away is on no screen while
#               hidden, and activating it shows workspace 2 (M5.2d); the
#               panel's switcher shows the first as the accent pill, and
#               a click on its 3 shows the third (M5.2c)
#   windows     a new window adds a button to the panel's window list,
#               lit; its title bar's minimize button hides it and a click
#               on its button brings it back (M5.2h)
#   launcher    Super opens the launcher; typing foot and Return starts
#               foot; Escape closes it (M5.3b)
#   switcher    with Alt held, Tab shows the window switcher with the
#               window used before chosen; letting go switches (M5.3c)
#   scale       edel system set outputs.Virtual-1.scale=2 halves the
#               logical screen and doubles the title bar's height in
#               screen pixels, after the other cases
#   respawn     kill -9 edel-compositor ends the session, and greetd's
#               greeter, our compositor, logs a new ready line within 5 s,
#               picks the Lite tier itself under llvmpipe, and owns the
#               health file; typing ci and a password into
#               its agreety starts ci's compositor again; last, as it
#               ends the first session
#   rollback    alone (CI runs it in the VM test lane, ci/vm-tests.sh, as
#               it restarts the VM): the test service installs the update
#               ci/build.sh signs, served here on port 8001, puts a
#               compositor that only fails into slot B and restarts; no
#               frame, so the guard lets the watchdog restart slot B, and
#               GRUB, out of tries, starts slot A, which says so (M4.8)
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
# for up to 10 s, until the pixel at X, Y is #WANT (or any of several
# colours written RRGGBB|RRGGBB), or with WANT !RRGGBB until it is
# anything else; prints the last colour and fails if it never was. Slow
# runners draw late, and windows animate (M5.11b).
shot() {
	i=0
	while :; do
		python3 ci/qmp.py screendump "$dir/$1.png"
		colour=$(python3 ci/qmp.py pixel "$dir/$1.png" "$2" "$3")
		matches "$colour" "$4" && break
		i=$((i + 1))
		[ "$i" -lt 10 ] || break
		sleep 1
	done
	echo "$colour"
	matches "$colour" "$4"
}

# matches COLOUR WANT: whether COLOUR is what shot's WANT asks for.
matches() {
	case "$2" in
	!*) [ "$1" != "${2#!}" ] ;;
	*) case "|$2|" in *"|$1|"*) true ;; *) false ;; esac ;;
	esac
}

# budget KEY: the number KEY has in ci/budgets.toml.
budget() {
	awk -F= -v key="$1" '{ k = $1; gsub(/ /, "", k) } k == key { v = $2; sub(/#.*/, "", v); gsub(/ /, "", v); print v }' ci/budgets.toml
}

# token KEY [TABLE]: the colour KEY has in design/tokens.toml's [colour]
# table, or in TABLE such as colour.light (M5.5c), as rrggbb.
token() {
	awk -v key="$1" -v table="[${2:-colour}]" '
		/^\[/ { here = ($1 == table); next }
		here && $1 == key && $3 ~ /^"#[0-9a-f]{6}"$/ { print substr($3, 3, 6); exit }
	' design/tokens.toml
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

# wait_for PATTERN [SECONDS]: waits up to SECONDS (20) for a serial line
# matching PATTERN.
wait_for() {
	i=0
	while ! tr -d '\r' <"$log" | grep -qE "$1"; do
		i=$((i + 1))
		[ "$i" -lt $((${2:-20} * 5)) ] || return 1
		sleep 0.2
	done
}

# The background colour the compositor clears to, from the design tokens.
background=$(token background)
# foot's own background. Window frames open centred in the area above
# shell-ui's 40 px panel (M5.1b), 1280x760, and cascade (M4.3), each a
# 28 px title bar above the window and a 1 px border round the rest
# (M4.4): foot, asked for 400x300, draws 396x288, whole character cells,
# and opens first at 442,249; 460,530 is inside it but outside the test
# clients opened after it.
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
	# llvmpipe draws on the CPU, so the compositor picks the Lite tier
	# itself (M5.11); edel shell tier reads the tier now from the state
	# file, and the animations follow it.
	case "$(value tier)" in
	"edel-compositor: tier=lite (renderer llvmpipe"*) ;;
	*) fail "the compositor did not pick tier=lite under llvmpipe: \"$(value tier)\"" ;;
	esac
	now=$(value tier_now)
	[ "tier=$(value shell_tier)" = "$now" ] ||
		fail "edel shell tier said \"$(value shell_tier)\", but the compositor logged $now last"
	case "$(value animations)" in
	"edel-compositor: animations at tier ${now#tier=}, motion full: "*) ;;
	*) fail "the animations are not the ${now#tier=} tier's: \"$(value animations)\"" ;;
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
	echo "PASS: $(value ready_line) in $(value ready_seconds) s at ${now#tier=}, the centre is the background, every budget met; $(value telemetry)"
}

case_floating() {
	sed -n 's/.*DESKTOP-TEST: state //p' "$log" | tr -d '\r' >"$dir/state.toml"
	# Above a 40 px panel on a 1280x800 screen, centred frames share the
	# centre 640,380, so
	# one's frame, opened after foot's, sits 32 px down and right of
	# centred, and two's 64 px; each window is 28 px below its frame's top
	# and 1 px inside its left edge.
	python3 - "$dir/state.toml" <<-'EOF' || fail "the state file does not show the test clients where the floating policy puts them"
		import sys, tomllib
		state = tomllib.load(open(sys.argv[1], "rb"))
		windows = {w["title"]: w for w in state["windows"]}
		place = lambda t: (windows[t]["x"], windows[t]["y"], windows[t]["width"], windows[t]["height"])
		assert state["format"] == 1 and state["policy"] == "floating", state
		assert place("one") == (522, 325, 300, 200), place("one")
		assert place("two") == (604, 382, 200, 150), place("two")
		assert windows["two"]["focused"], "the newest window has the keyboard"
		assert [w["title"] for w in state["windows"]][-2:] == ["one", "two"], "two is on top"
	EOF
	shot floating 530 333 cc3333 >/dev/null || fail "test client one is not red at 530,333"
	shot floating 704 457 3366cc >/dev/null || fail "test client two is not blue at its centre, 704,457"
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
	text=$(python3 ci/qmp.py uniform "$dir/titlebar.png" 610 356 135 24)
	[ "$text" = varied ] || fail "two's title bar shows no title: it is $text"
	# Close two with its close button, the bar's rightmost 28 px.
	python3 ci/qmp.py click 791 368
	wait_for 'edel-compositor: unmapped window two' ||
		fail "clicking two's close button at 791,368 did not close it"
	wait_for 'DESKTOP-TEST: windows 2 ' || fail "the state file still lists two"
	# Drag one by its bar 200 px right and 180 up, to where later cases
	# expect it; foot stays uncovered at $foot_x,$foot_y for the console
	# case.
	python3 ci/qmp.py drag 560 311 760 131
	wait_for 'DESKTOP-TEST: windows 2 .*one@72[123],14[456],300x200' ||
		fail "dragging one's title bar from 560,311 to 760,131 did not move it to about 722,145: $(value windows)"
	echo "PASS: foot and the test clients have title bars in the token colours with their titles, two's close button closed it and one's bar moved it"
}

case_xwayland() {
	# The compositor holds an X11 display, and nothing runs for it until an
	# X11 app connects (M4.7); xclock is one that draws no bar of its own,
	# so it gets ours, through xwayland-satellite.
	value x11 | grep -q 'X11 apps on :[0-9]' || fail "the compositor holds no X11 display: \"$(value x11)\""
	[ "$(value x11_processes)" = 0 ] ||
		fail "$(value x11_processes) X11 processes ran before any X11 app did"
	guest xclock
	wait_for 'DESKTOP-TEST: windows [0-9]+ .*xclock@' || fail "xclock did not open: $(value windows)"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: windows .*xclock@\([0-9]*\),\([0-9]*\),\([0-9]*\)x.*/\1 \2 \3/p' | tail -n 1)
	read -r x y w <<-EOF
		$place
	EOF
	# Its bar's left end, in the focused colour: it has the keyboard.
	focused=$(token title_bar_focused)
	shot xwayland $((x + 5)) $((y - 24)) "$focused" >/dev/null ||
		fail "xclock at $x,$y has no title bar in #$focused at $((x + 5)),$((y - 24))"
	wait_for 'DESKTOP-TEST: x11_rss_mib [0-9]' ||
		fail "the test service did not say what XWayland and satellite use"
	# Its close button, the bar's rightmost 28 px.
	python3 ci/qmp.py click $((x + w - 13)) $((y - 14))
	wait_for 'edel-compositor: unmapped window xclock' ||
		fail "clicking xclock's close button at $((x + w - 13)),$((y - 14)) did not close it"
	echo "PASS: no X11 process ran until xclock connected, then XWayland and xwayland-satellite ($(value x11_rss_mib) MiB) showed it at $x,$y with a title bar in the focused colour, and its close button closed it"
}

case_respawn() {
	# greetd owns the compositor's lifecycle (M4.7b): when the session's
	# compositor dies, the session ends and greetd starts its greeter, our
	# compositor running agreety in foot, which writes the health file.
	guest 'kill compositor'
	wait_for 'DESKTOP-TEST: (respawn_seconds|respawn:)' ||
		fail "no word from the test service after kill -9 edel-compositor"
	seconds=$(value respawn_seconds)
	[ -n "$seconds" ] || fail "greetd's greeter never logged a ready line: $(value respawn:)"
	awk -v s="$seconds" 'BEGIN { exit !(s <= 5) }' ||
		fail "the greeter's compositor showed its first frame $seconds s after the kill, over 5"
	owner=$(value ready_owner)
	[ "$owner" = greetd ] || fail "the health file is $owner's, not the greeter's"
	# The greeter's compositor picks its tier itself too: Lite, under
	# llvmpipe (M5.11).
	case "$(value greeter_tier)" in
	"edel-compositor: tier=lite (renderer llvmpipe"*) ;;
	*) fail "the greeter's compositor did not pick tier=lite under llvmpipe: \"$(value greeter_tier)\"" ;;
	esac
	# The login screen itself: foot, running agreety, the only window, with
	# the keyboard. Logging in as ci there ends the greeter's compositor,
	# and greetd starts ci's, which takes the health file back.
	wait_for 'DESKTOP-TEST: windows 1 foot@' ||
		fail "the greeter's compositor shows no foot window: $(value windows)"
	sleep 2
	python3 ci/qmp.py type 'ci\n'
	sleep 3
	python3 ci/qmp.py type 'edel-desktop-test\n'
	wait_for 'DESKTOP-TEST: login_ready ' || fail "no word from the test service after the login"
	case "$(value login_ready)" in
	"ci output "*" ready") ;;
	*) fail "logging in as ci through the greeter did not start ci's compositor: \"$(value login_ready)\"" ;;
	esac
	echo "PASS: kill -9 edel-compositor ended the session, greetd's greeter, our compositor, logged \"$(value greeter | sed 's/^edel-compositor: //')\" $seconds s later and showed agreety in foot, and logging in there started ci's compositor again"
}

case_layers() {
	# A second panel (M5.1a): the test client on the top layer along the
	# bottom, the screen's width and 40 px high, in #2f343f, stacked above
	# shell-ui's own panel (M5.1b). Only one window is open, where titlebar
	# left it.
	guest 'panel on'
	wait_for 'DESKTOP-TEST: layers 2 .*edel-testclient@0,720,1280x40' ||
		fail "the test panel is not above shell-ui's along the bottom in the state file: $(value layers)"
	shot layers 640 740 2f343f >/dev/null || fail "640,740 is not the test panel's #2f343f"
	# Windows tile, and would maximize, in the area above both. Super+T
	# switches whatever the system file says: the tiling case leaves
	# shell.tiling at true with the windows floating.
	python3 ci/qmp.py key meta_l-t
	wait_for 'DESKTOP-TEST: windows 1 one@9,36,1262x675' ||
		fail "one did not tile above the panels: $(value windows)"
	python3 ci/qmp.py key meta_l-t
	i=0
	while [ "$(value windows)" != '1 one@722,145,300x200' ]; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "one did not float back: $(value windows)"
		sleep 0.2
	done
	guest 'panel off'
	i=0
	while [ "$(value layers)" != '1 edel-panel@0,748,1280x52' ]; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "the test panel did not go: $(value layers)"
		sleep 0.2
	done
	echo "PASS: a second layer-shell panel lay above shell-ui's along the bottom in its colour, windows tiled above both (one@9,36,1262x675), and it went when closed"
}

# count PATTERN: how many serial lines match PATTERN so far.
count() {
	tr -d '\r' <"$log" | grep -cE "$1" || true
}

# wait_more PATTERN N [SECONDS]: waits up to SECONDS (20) for more than N
# lines to match.
wait_more() {
	i=0
	while [ "$(count "$1")" -le "$2" ]; do
		i=$((i + 1))
		[ "$i" -lt $((${3:-20} * 5)) ] || return 1
		sleep 0.2
	done
}

case_panel() {
	# shell-ui's panel (M5.1b), which the compositor started once the
	# desktop was on screen: a 40 px panel along the bottom, drawn with a
	# 12 px strip above it for the fillets, which takes no space.
	panel=$(token panel)
	value layers | grep -q 'edel-panel@0,748,1280x52' ||
		fail "shell-ui's panel is not along the bottom in the state file: $(value layers)"
	shot panel 640 790 "$panel" >/dev/null || fail "640,790 is not the panel's #$panel"
	shot panel 640 761 "$panel" >/dev/null || fail "640,761, the panel's top row, is not #$panel"
	# The clock, at the panel's right end, is drawn: not one colour.
	clock=$(python3 ci/qmp.py uniform "$dir/panel.png" 1214 765 62 30)
	[ "$clock" = varied ] || fail "the clock's region at the panel's right end is $clock"
	# The layout toggle (M5.3a): a click switches the shown workspace's
	# policy over edel-shell-v1, as Super+T, and fills the button, 30 px
	# wide after 3 px of room, with the accent; another switches it back.
	# The checked pixel is left of where the cursor lies after a click.
	toggle=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 | sed -n 's/.*layout \([0-9]*\)+.*/\1/p')
	[ -n "$toggle" ] || fail "shell-ui did not say where its layout toggle lies"
	shot panel $((toggle + 6)) 780 "$panel" >/dev/null || fail "the layout toggle at $((toggle + 6)),780 is filled while the windows float"
	tiled=$(count 'edel-compositor: windows now tiling')
	python3 ci/qmp.py click $((toggle + 18)) 780
	wait_more 'edel-compositor: windows now tiling' "$tiled" || fail "a click on the layout toggle at $((toggle + 18)),780 did not tile the windows"
	shot panel $((toggle + 6)) 780 "!$panel" >/dev/null || fail "the layout toggle is not filled while the windows tile"
	floated=$(count 'edel-compositor: windows now floating')
	python3 ci/qmp.py click $((toggle + 18)) 780
	wait_more 'edel-compositor: windows now floating' "$floated" || fail "a second click on the layout toggle did not float the windows again"
	shot panel $((toggle + 6)) 780 "$panel" >/dev/null || fail "the layout toggle is still filled after the windows float again"
	# A screen reader (M5.1d): with accessibility turned on, AT-SPI holds
	# shell-ui, its panel and each widget, by role and name.
	asked=$(count 'DESKTOP-TEST: a11y_done')
	guest 'a11y tree'
	wait_more 'DESKTOP-TEST: a11y_done' "$asked" 30 || fail "the screen reader's walk of AT-SPI did not finish"
	tree=$(tr -d '\r' <"$log" | grep -a 'DESKTOP-TEST: a11y ' | sed 's/.*DESKTOP-TEST: a11y //')
	for want in '1 application: edel-shell-ui' '2 frame: Panel' '3 button: Menu' '3 button: Layout: floating'; do
		echo "$tree" | grep -qx "$want" ||
			fail "AT-SPI does not hold \"$want\"; it holds: $(echo "$tree" | tr '\n' ';')"
	done
	echo "$tree" | grep -qE '^3 label: [0-9]{2}:[0-9]{2}$' ||
		fail "AT-SPI holds no clock among the panel's widgets: $(echo "$tree" | tr '\n' ';')"
	# Its memory, as the service read it once the desktop was idle, with
	# no screen reader: what it holds of its own, without the pages it
	# maps from files, whose count swings from run to run.
	own=$(value shell_ui_own_mib)
	rss=$(value shell_ui_rss_mib)
	limit=$(budget shell_ui_own_mib)
	awk -v m="$own" -v b="$limit" 'BEGIN { exit !(m != "" && m <= b) }' ||
		fail "edel-shell-ui holds ${own:-?} MiB of its own ($rss MiB resident), over its budget of $limit MiB"
	# Killed, it comes back: the compositor starts it again.
	started=$(count 'edel-compositor: started edel-shell-ui')
	guest 'kill panel'
	wait_more 'edel-compositor: started edel-shell-ui' "$started" ||
		fail "the compositor did not start edel-shell-ui again after kill -9"
	shot panel 640 790 "$panel" >/dev/null || fail "after kill -9, the panel did not come back at 640,790"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "shell-ui (M5.1b): $own MiB of its own (budget $limit MiB), $rss MiB resident with $(value shell_ui_file_mib) MiB mapped from files." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: shell-ui's panel lies along the bottom in #$panel with its clock drawn, its layout toggle tiled the windows and floated them again, a screen reader found it, its menu button, layout button and clock over AT-SPI, it holds $own MiB of its own (budget $limit), $(value shell_ui_shared_kib) kB of it buffers shared with the compositor, $rss MiB resident with $(value shell_ui_file_mib) MiB mapped from files, and it came back after kill -9"
}

case_animations() {
	# appearance.motion (M5.11b): reduced keeps the fades and drops the
	# growing and sliding, which Lite has none of; removing the key logs
	# the tier's own animations again.
	full=$(tr -d '\r' <"$log" | grep -c 'motion full: open' || true)
	guest 'motion reduced'
	wait_for 'edel-compositor: animations at tier [a-z]+, motion reduced: open [1-9][0-9]* ms, close [1-9][0-9]* ms, slide 0 ms' ||
		fail "appearance.motion = \"reduced\" did not leave fades only: $(tr -d '\r' <"$log" | grep -a 'animations at' | tail -n 1)"
	reduced=$(tr -d '\r' <"$log" | grep -a 'motion reduced: open' | tail -n 1 | sed 's/.*: open/open/')
	guest 'motion default'
	i=0
	while [ "$(tr -d '\r' <"$log" | grep -c 'motion full: open' || true)" -le "$full" ]; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "removing appearance.motion did not bring the full animations back"
		sleep 0.2
	done
	# Ten windows opening one after another, then closing at once, while
	# weston-presentation-shm times the frames as at the start: first with
	# motion off, what opening ten programs costs without any animation,
	# then with the animations of the tier llvmpipe picks, Lite. M5.11's
	# done-when asks for the frame budget there: what the animations add
	# to the frames at p99 over the same ten windows without them, as the
	# runner's own speed moves both by up to 2 ms; the Full tier, which no
	# machine without a GPU starts in, drops on its own when its frames
	# miss (11a's cargo tests). The case says which tier each run ended at.
	off_runs=$(count 'DESKTOP-TEST: open_ten_tier ')
	guest 'motion off'
	wait_for 'edel-compositor: animations at tier [a-z]+, motion off: open 0 ms' ||
		fail "appearance.motion = \"off\" did not stop the animations"
	windows=$(value windows)
	guest 'open ten'
	wait_more 'DESKTOP-TEST: open_ten_tier ' "$off_runs" 90 || fail "no word from the test service on the ten windows"
	p99_off=$(value open_ten_p99_ms)
	tier_off=$(value open_ten_tier)
	i=0
	while [ "$(value windows)" != "$windows" ]; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "the ten windows did not all close: $(value windows)"
		sleep 0.2
	done
	full=$(count 'motion full: open')
	guest 'motion default'
	wait_more 'motion full: open' "$full" || fail "removing appearance.motion again did not bring the animations back"
	guest 'open ten'
	wait_more 'DESKTOP-TEST: open_ten_tier ' "$((off_runs + 1))" 90 || fail "no word from the test service on the ten windows"
	p99=$(value open_ten_p99_ms)
	frames=$(value open_ten_frames)
	tier=$(value open_ten_tier)
	limit=$(budget animation_p99_ms)
	added=$(awk -v m="$p99" -v o="$p99_off" 'BEGIN { if (m != "" && o != "") printf "%.1f", m - o }')
	echo "ten windows: frames p99 ${p99_off:-?} ms with motion off (tier $tier_off), ${p99:-?} ms with it full (tier $tier), ${added:-?} ms added, budget $limit ms"
	[ "${frames:-0}" -ge 100 ] || fail "weston-presentation-shm timed only ${frames:-no} frames while the ten windows opened"
	awk -v a="$added" -v b="$limit" 'BEGIN { exit !(a != "" && a <= b) }' ||
		fail "with ten windows opening and closing at tier $tier, the animations added ${added:-?} ms to the frames at p99 (${p99_off:-?} ms with motion off, ${p99:-?} ms with it full), over the budget of $limit ms"
	i=0
	while [ "$(value windows)" != "$windows" ]; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "the ten windows did not all close: $(value windows)"
		sleep 0.2
	done
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Ten windows opening and closing with animations (M5.11b): frames p99 $p99 ms, $p99_off ms with motion off, $added ms added (budget $limit ms), $frames frames, tier $tier." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: appearance.motion = reduced left fades only ($reduced) and its removal brought the tier's own back; ten windows opened and closed at tier $tier with frames p99 $p99 ms over $frames frames, $added ms more than without animations ($p99_off ms), within $limit ms"
}

case_shortcuts() {
	# [shortcuts] (M5.13a), live from the system file: a window of its own,
	# which takes the keyboard as it opens.
	guest 'shortcut window'
	wait_more 'edel-compositor: mapped window keys' 0 || fail "the test client keys did not open: $(value windows)"
	guest 'close on super-w'
	wait_for 'edel-compositor: shortcut close is Super\+W' ||
		fail "the compositor did not follow shortcuts.close = \"Super+W\""
	# Super+Q now reaches the window, which ignores it; Super+W closes it.
	python3 ci/qmp.py key meta_l-q
	sleep 2
	[ "$(count 'edel-compositor: unmapped window keys')" = 0 ] ||
		fail "Super+Q still closed keys after close moved to Super+W"
	python3 ci/qmp.py key meta_l-w
	wait_more 'edel-compositor: unmapped window keys' 0 || fail "Super+W did not close keys: $(value windows)"
	# Ctrl+Alt+T opens a terminal, which Super+W closes in turn.
	opened=$(count 'edel-compositor: mapped window foot')
	python3 ci/qmp.py key ctrl-alt-t
	wait_more 'edel-compositor: mapped window foot' "$opened" || fail "Ctrl+Alt+T opened no terminal: $(value windows)"
	closed=$(count 'edel-compositor: unmapped window foot')
	sleep 1
	python3 ci/qmp.py key meta_l-w
	wait_more 'edel-compositor: unmapped window foot' "$closed" || fail "Super+W did not close the terminal"
	# Unset, close is Super+Q again.
	guest 'shortcuts default'
	wait_for 'edel-compositor: shortcut close is Super\+Q' ||
		fail "removing shortcuts.close did not bring Super+Q back"
	echo "PASS: shortcuts.close = \"Super+W\" moved close off Super+Q at once, Super+W closed keys, Ctrl+Alt+T opened foot, and removing the key brought Super+Q back"
}

case_workspaces() {
	# Classic's four workspaces (M5.2a). away opens on the first, over the
	# windows there, and takes the keyboard.
	first=$(value windows)
	opened=$(count 'edel-compositor: mapped window away')
	guest 'away window'
	wait_more 'edel-compositor: mapped window away' "$opened" || fail "the test client away did not open: $(value windows)"
	shot workspaces 640 393 7744aa >/dev/null || fail "away is not drawn at 640,393"
	# Super+Shift+2 sends it to the second, which the state file says;
	# the first stays shown.
	python3 ci/qmp.py key meta_l-shift-2
	wait_for 'edel-compositor: window away to workspace 2' || fail "Super+Shift+2 did not send away to workspace 2"
	wait_for 'DESKTOP-TEST: hidden on 1, 1 away@2$' || fail "the state file does not keep away on workspace 2: $(value hidden)"
	shot workspaces 640 393 '!7744aa' >/dev/null || fail "away is still drawn on workspace 1"
	# Super+2 shows it tiled, as shell.tiling = true from the tiling case
	# says for every workspace; Super+T floats this one alone, with away
	# centred where its floating policy put it.
	python3 ci/qmp.py key meta_l-2
	wait_for 'edel-compositor: workspace 2' || fail "Super+2 did not show workspace 2"
	wait_for 'DESKTOP-TEST: windows 1 away@9,36,1262x715' || fail "away does not fill workspace 2, tiled: $(value windows)"
	shot workspaces 640 393 7744aa >/dev/null || fail "away is not drawn on workspace 2"
	floated=$(count 'edel-compositor: windows now floating')
	python3 ci/qmp.py key meta_l-t
	wait_more 'edel-compositor: windows now floating' "$floated" || fail "Super+T did not float workspace 2"
	wait_for 'DESKTOP-TEST: windows 1 away@540,318,200x150' || fail "away is not centred on workspace 2, floating: $(value windows)"
	# Super+1 brings the first back as it was, floating.
	back=$(count "DESKTOP-TEST: windows $first\$")
	kept=$(count 'DESKTOP-TEST: hidden on 1, 1 away@2$')
	python3 ci/qmp.py key meta_l-1
	wait_for 'edel-compositor: workspace 1' || fail "Super+1 did not show workspace 1"
	wait_more "DESKTOP-TEST: windows $first\$" "$back" || fail "Super+1 did not bring back $first: $(value windows)"
	wait_more 'DESKTOP-TEST: hidden on 1, 1 away@2$' "$kept" || fail "away is not kept on workspace 2: $(value hidden)"
	shot workspaces 640 393 '!7744aa' >/dev/null || fail "away is drawn on workspace 1"
	# As a window list sees them (M5.2d): away is on no screen while its
	# workspace is hidden, and activating it shows its workspace and
	# focuses it.
	guest 'toplevels'
	wait_for 'DESKTOP-TEST: toplevels .*away-( |$)' ||
		fail "wlr-foreign-toplevel-management does not list away on no screen: $(value toplevels)"
	shows=$(count 'edel-compositor: workspace 2')
	guest 'toplevels away'
	wait_more 'edel-compositor: workspace 2' "$shows" || fail "activating away did not show workspace 2"
	wait_for 'DESKTOP-TEST: toplevels .*away\*' || fail "activating away did not focus it: $(value toplevels)"
	again=$(count "DESKTOP-TEST: windows $first\$")
	python3 ci/qmp.py key meta_l-1
	wait_more "DESKTOP-TEST: windows $first\$" "$again" || fail "Super+1 did not bring workspace 1 back after the activation"
	# away closes while hidden, and leaves the state file.
	none=$(count 'DESKTOP-TEST: hidden on 1, 0$')
	closed=$(count 'edel-compositor: unmapped window away')
	guest 'away off'
	wait_more 'edel-compositor: unmapped window away' "$closed" || fail "away, closed on workspace 2, was not unmapped"
	wait_more 'DESKTOP-TEST: hidden on 1, 0$' "$none" || fail "the state file still lists away: $(value hidden)"
	# ext-workspace-v1 (M5.2b): a client sees the four workspaces, the
	# first shown, and shows the third, as the panel's switcher will.
	guest 'activate 3'
	wait_for 'DESKTOP-TEST: ext workspaces 1 2 3\* 4$' ||
		fail "edel-testclient --workspace 3 did not see workspace 3 shown: $(tr -d '\r' <"$log" | grep -a 'DESKTOP-TEST: ext' | tail -n 3)"
	wait_for 'DESKTOP-TEST: ext workspaces 1\* 2 3 4$' || fail "the client did not first see workspace 1 shown"
	wait_for 'edel-compositor: workspace 3' || fail "the compositor did not show workspace 3"
	wait_for 'DESKTOP-TEST: hidden on 3, ' || fail "the state file does not say workspace 3 is shown: $(value hidden)"
	seen=$(count 'DESKTOP-TEST: ext workspaces 1\* 2 3 4$')
	back=$(count "DESKTOP-TEST: windows $first\$")
	guest 'activate 1'
	wait_more 'DESKTOP-TEST: ext workspaces 1\* 2 3 4$' "$seen" || fail "edel-testclient --workspace 1 did not bring workspace 1 back"
	wait_more "DESKTOP-TEST: windows $first\$" "$back" || fail "workspace 1 is not back as it was: $(value windows)"
	# The panel's switcher (M5.2c): three round buttons, the shown one the
	# accent pill, then the fourth peeking in, faded; a click on its 3
	# shows the third. Each button is 20 px, the pill 32, 5 apart, after
	# 6 px of room and the 14 px, and a gap, where the button before them
	# would peek in; the fourth's strip starts 112 px in.
	guest 'panel places'
	wait_for 'DESKTOP-TEST: places .*workspaces [0-9]+\+' || fail "shell-ui did not say where its widgets lie"
	x=$(value places | sed -n 's/.*workspaces \([0-9]*\)+.*/\1/p')
	accent=$(token accent)
	# Each pill is checked 4 px in from its left end, clear of its digit.
	shot switcher $((x + 29)) 780 "$accent" >/dev/null || fail "the switcher's 1, at $((x + 29)),780, is not the accent pill"
	shot switcher $((x + 115)) 780 "!$(token panel)" >/dev/null || fail "the fourth workspace does not peek in at $((x + 115)),780"
	shows=$(count 'edel-compositor: workspace 3')
	python3 ci/qmp.py click $((x + 97)) 780
	wait_more 'edel-compositor: workspace 3' "$shows" || fail "a click on the switcher's 3 did not show workspace 3"
	# The view follows: 2, then 3 as the pill, then 4.
	shot switcher $((x + 54)) 780 "$accent" >/dev/null || fail "after the click, the pill at $((x + 54)),780 is not the accent"
	back=$(count "DESKTOP-TEST: windows $first\$")
	python3 ci/qmp.py key meta_l-1
	wait_more "DESKTOP-TEST: windows $first\$" "$back" || fail "Super+1 did not bring workspace 1 back after the switcher: $(value windows)"
	echo "PASS: Super+Shift+2 sent away to workspace 2, Super+2 showed it tiled, Super+T floated that workspace alone, Super+1 brought back $first, and away, closed while hidden, left the state file; over ext-workspace-v1 a client saw four workspaces and showed the third, then the first; the panel's switcher showed 1 as the accent pill, and a click on its 3 showed the third"
}

case_launcher() {
	# The launcher (M5.3b): Super, tapped alone, opens it beside the
	# panel's start, 360x352 at 8,400, 8 px from the screen's side and
	# the panel, in the panel's colour; 188,748 lies in its bottom
	# margin, below its last row, and is the background before.
	background=$(token background)
	panel=$(token panel)
	shot launcher 188 748 "$background" >/dev/null || fail "188,748 is not the background before the launcher opens"
	shown=$(count 'edel-shell-ui: launcher shown')
	python3 ci/qmp.py key meta_l
	wait_more 'edel-shell-ui: launcher shown' "$shown" || fail "Super, tapped alone, did not open the launcher"
	shot launcher 188 748 "$panel" >/dev/null || fail "the launcher is not drawn at 188,748 in the panel's colour"
	apps=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: launcher shown' | tail -n 1 | sed 's/.*shown, //')
	# Typed, foot is the best match, and Return starts it.
	opened=$(count 'edel-compositor: mapped window foot')
	hidden=$(count 'edel-shell-ui: launcher hidden')
	python3 ci/qmp.py type foot
	python3 ci/qmp.py key ret
	wait_more 'edel-compositor: mapped window foot' "$opened" || fail "typing foot and Return did not open foot"
	wait_more 'edel-shell-ui: launcher hidden' "$hidden" || fail "the launcher did not close when foot started"
	tr -d '\r' <"$log" | grep -aq 'edel-shell-ui: launched Foot (foot)' || fail "the launcher did not start foot's own command"
	closed=$(count 'edel-compositor: unmapped window foot')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window foot' "$closed" || fail "Super+Q did not close foot, which should have had the keyboard"
	# Opened again, Escape closes it.
	shown=$(count 'edel-shell-ui: launcher shown')
	hidden=$(count 'edel-shell-ui: launcher hidden')
	python3 ci/qmp.py key meta_l
	wait_more 'edel-shell-ui: launcher shown' "$shown" || fail "Super did not open the launcher a second time"
	python3 ci/qmp.py key esc
	wait_more 'edel-shell-ui: launcher hidden' "$hidden" || fail "Escape did not close the launcher"
	shot launcher 188 748 "$background" >/dev/null || fail "188,748 is not the background after the launcher closed"
	guest 'shell rss'
	wait_for 'DESKTOP-TEST: shell_ui_rss_now_mib [0-9]' || fail "the service did not read shell-ui's memory"
	echo "PASS: Super opened the launcher in the panel's colour with $apps, typing foot and Return started foot and closed it, Super+Q closed foot, and Escape closed it again; shell-ui then held $(value shell_ui_own_now_mib) MiB of its own, $(value shell_ui_shared_now_kib) kB of it shared buffers, $(value shell_ui_rss_now_mib) MiB resident"
}

case_switcher() {
	# The window switcher (M5.3c): with away opened over one, Alt held
	# and Tab chooses one, the window used before away; shell-ui draws
	# the list in the middle of the screen in the panel's colour, 4 px
	# in from its left end clear of the rows; letting go of Alt switches
	# to one and hides the list.
	panel=$(token panel)
	opened=$(count 'edel-compositor: mapped window away')
	guest 'away window'
	wait_more 'edel-compositor: mapped window away' "$opened" || fail "the test client away did not open"
	shown=$(count 'edel-shell-ui: switcher shown')
	python3 ci/qmp.py down alt
	python3 ci/qmp.py key tab
	wait_more 'edel-shell-ui: switcher shown' "$shown" ||
		{ python3 ci/qmp.py up alt; fail "Alt+Tab did not show the switcher"; }
	wait_for 'DESKTOP-TEST: layers [0-9]+ .*edel-switcher@[0-9]+,[0-9]+,420x[0-9]+' ||
		{ python3 ci/qmp.py up alt; fail "the state file does not list the switcher: $(value layers)"; }
	read -r x y h <<-EOF
		$(value layers | sed -n 's/.*edel-switcher@\([0-9]*\),\([0-9]*\),420x\([0-9]*\).*/\1 \2 \3/p')
	EOF
	shot switcher $((x + 4)) $((y + h / 2)) "$panel" >/dev/null ||
		{ python3 ci/qmp.py up alt; fail "the switcher is not drawn at $((x + 4)),$((y + h / 2)) in the panel's colour"; }
	tr -d '\r' <"$log" | grep -a 'edel-compositor: switcher at' | tail -n 1 | grep -q 'at one$' ||
		{ python3 ci/qmp.py up alt; fail "Alt+Tab did not choose one, the window before away"; }
	switched=$(count 'edel-compositor: switched to window one')
	hidden=$(count 'edel-shell-ui: switcher hidden')
	python3 ci/qmp.py up alt
	wait_more 'edel-compositor: switched to window one' "$switched" || fail "letting go of Alt did not switch to one"
	wait_more 'edel-shell-ui: switcher hidden' "$hidden" || fail "the switcher did not hide"
	closed=$(count 'edel-compositor: unmapped window away')
	guest 'away off'
	wait_more 'edel-compositor: unmapped window away' "$closed" || fail "away did not close"
	echo "PASS: holding Alt, Tab showed the switcher at $x,$y in the panel's colour with one chosen, the window used before away, and letting go of Alt switched to one and hid it"
}

case_presets() {
	# Presets (M5.4a): edel system set shell.preset=hive restarts
	# shell-ui with Hive's bar along the top and tiles the windows, and
	# unsetting it brings Classic back, the panel along the bottom and the
	# windows floating, live; each is kept as preset-NAME.png with the
	# screenshots, for Alimardon to look at.
	panel=$(token panel)
	top=$(count 'edel-shell-ui: panel edel-panel along the top')
	tiled=$(count 'edel-compositor: windows now tiling')
	guest 'preset hive'
	wait_more 'edel-shell-ui: panel edel-panel along the top' "$top" ||
		fail "edel system set shell.preset=hive did not bring shell-ui's bar to the top"
	tr -d '\r' <"$log" | grep -aq 'edel-compositor: restarting edel-shell-ui: the preset is now hive' ||
		fail "the compositor did not restart shell-ui for the Hive preset"
	wait_more 'edel-compositor: windows now tiling' "$tiled" || fail "the Hive preset did not tile the windows"
	shot preset-hive 640 12 "$panel" >/dev/null || fail "Hive's bar is not at 640,12 in the panel's colour"
	bottom=$(count 'edel-shell-ui: panel edel-panel along the bottom')
	floated=$(count 'edel-compositor: windows now floating')
	guest 'preset default'
	wait_more 'edel-shell-ui: panel edel-panel along the bottom' "$bottom" ||
		fail "unsetting shell.preset did not bring Classic's panel back along the bottom"
	wait_more 'edel-compositor: windows now floating' "$floated" || fail "back on Classic, the windows did not float"
	shot preset-classic 640 790 "$panel" >/dev/null || fail "Classic's panel is not at 640,790 in the panel's colour"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Presets (M5.4a): preset-hive.png and preset-classic.png are in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: edel system set shell.preset=hive restarted shell-ui with Hive's bar along the top and tiled the windows, and unsetting it brought back Classic's panel along the bottom and floating windows"
}

case_taskbar() {
	# Windows-like (M5.4c): edel system set shell.preset=windows-like
	# restarts shell-ui with one taskbar along the bottom, the menu button
	# at its left edge, the search field beside it and the apps in its
	# centre. Of the apps it pins (files, browser, terminal, mail, music) the VM
	# has only foot, the terminal, so foot's cell comes first: a click
	# starts foot, a second minimizes it, a third brings it back. Kept as
	# preset-windows-like.png.
	panel=$(token panel)
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now windows-like')
	guest 'preset windows-like'
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now windows-like' "$restarts" ||
		fail "edel system set shell.preset=windows-like did not restart shell-ui"
	wait_for 'edel-shell-ui: panel places menu 0\+[0-9]+, search [0-9]+\+188, apps [0-9]+\+[0-9]+' ||
		fail "Windows-like's bar does not start with the menu button and the search field before the apps"
	# The open windows' apps join the pinned ones as shell-ui learns of
	# them; foot's cell stays first.
	sleep 1
	place=$(tr -d '\r' <"$log" | sed -n 's/.*edel-shell-ui: panel places menu 0+[0-9]*, search [0-9]*+188, apps \([0-9]*\)+\([0-9]*\).*/\1 \2/p' | tail -n 1)
	read -r x w <<-EOF
		$place
	EOF
	[ "$w" -ge 48 ] || fail "the apps widget is $w px wide, too narrow for foot's 40 px cell"
	# The apps sit in the bar's centre, the 1280 px screen's.
	[ $((x + w / 2 - 640)) -le 1 ] && [ $((640 - x - w / 2)) -le 1 ] ||
		fail "the apps, at $x and $w px wide, are not centred on the bar"
	shot preset-windows-like 1000 790 "$panel" >/dev/null || fail "Windows-like's bar is not at 1000,790 in the panel's colour"
	cell=$((x + 24))
	opened=$(count 'edel-compositor: mapped window foot')
	launched=$(count 'edel-shell-ui: launched Foot \(foot\)')
	python3 ci/qmp.py click "$cell" 774
	wait_more 'edel-compositor: mapped window foot' "$opened" || fail "a click on foot's cell at $cell,774 did not start foot"
	wait_more 'edel-shell-ui: launched Foot \(foot\)' "$launched" || fail "foot's cell did not start foot's own command"
	minimized=$(count 'edel-compositor: minimized window foot')
	python3 ci/qmp.py click "$cell" 774
	wait_more 'edel-compositor: minimized window foot' "$minimized" || fail "a second click on foot's cell did not minimize foot"
	restored=$(count 'edel-compositor: restored window foot')
	python3 ci/qmp.py click "$cell" 774
	wait_more 'edel-compositor: restored window foot' "$restored" || fail "a third click on foot's cell did not bring foot back"
	closed=$(count 'edel-compositor: unmapped window foot')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window foot' "$closed" || fail "Super+Q did not close foot, which should have had the keyboard"
	back=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now classic')
	guest 'preset default'
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now classic' "$back" ||
		fail "unsetting shell.preset did not bring Classic back"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Windows-like (M5.4c): preset-windows-like.png is in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: Windows-like's taskbar lay along the bottom with the menu button at its left edge, the search field and the apps centred at $x, $w px wide; foot's cell started foot, minimized it and brought it back, and unsetting the preset brought Classic back"
}

case_dock() {
	# Mac-like (M5.4d): edel system set shell.preset=mac-like restarts
	# shell-ui with a bar along the top and a dock along the bottom, a
	# card in the panel's colour as wide as its apps, centred, 60 px high
	# and 8 px above the screen's bottom (732 to 792), and moves the
	# window buttons to the left. Of the apps it pins the VM has foot, so
	# foot's cell comes first; a click there starts foot. Kept as
	# preset-mac-like.png.
	panel=$(token panel)
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now mac-like')
	docks=$(count 'edel-shell-ui: panel edel-dock along the bottom')
	left=$(count 'edel-compositor: window buttons on the left')
	guest 'preset mac-like'
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now mac-like' "$restarts" ||
		fail "edel system set shell.preset=mac-like did not restart shell-ui"
	wait_more 'edel-shell-ui: panel edel-dock along the bottom' "$docks" || fail "Mac-like has no dock along the bottom"
	wait_more 'edel-compositor: window buttons on the left' "$left" || fail "Mac-like did not move the window buttons to the left"
	wait_for 'edel-shell-ui: panel places apps 8\+[0-9]+$' || fail "the dock does not hold the apps 8 px from its start"
	# The open windows' apps join the pinned ones as shell-ui learns of
	# them, and the dock grows to hold them.
	sleep 1
	w=$(tr -d '\r' <"$log" | sed -n 's/.*edel-shell-ui: panel places apps 8+\([0-9]*\)$/\1/p' | tail -n 1)
	x=$((640 - (w + 16) / 2))
	shot preset-mac-like 640 12 "$panel" >/dev/null || fail "Mac-like's bar is not at 640,12 in the panel's colour"
	shot preset-mac-like $((x + 3)) 762 "$panel" >/dev/null ||
		fail "the dock, $((w + 16)) px wide, does not start at $((x + 3)),762 in the panel's colour"
	shot preset-mac-like $((x - 3)) 762 "$background" >/dev/null || fail "$((x - 3)),762, left of the dock, is not the background"
	shot preset-mac-like 640 797 "$background" >/dev/null || fail "640,797, under the dock, is not the background"
	# Foot's cell, 52 px, 4 px into the apps, which start 8 px in.
	cell=$((x + 8 + 4 + 26))
	opened=$(count 'edel-compositor: mapped window foot')
	python3 ci/qmp.py click "$cell" 762
	wait_more 'edel-compositor: mapped window foot' "$opened" || fail "a click on foot's cell in the dock at $cell,762 did not start foot"
	closed=$(count 'edel-compositor: unmapped window foot')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window foot' "$closed" || fail "Super+Q did not close foot"
	back=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now classic')
	right=$(count 'edel-compositor: window buttons on the right')
	guest 'preset default'
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now classic' "$back" ||
		fail "unsetting shell.preset did not bring Classic back"
	wait_more 'edel-compositor: window buttons on the right' "$right" || fail "back on Classic, the window buttons did not go back to the right"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Mac-like (M5.4d): preset-mac-like.png is in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: Mac-like put a bar along the top, a dock $((w + 16)) px wide centred at $x,732 above an 8 px gap, and the window buttons on the left; foot's cell in the dock started foot, and unsetting the preset brought Classic back"
}

case_panels() {
	# The panels as a setting (M5.4e): shell.panels with one panel along
	# the bottom holding only the clock restarts shell-ui with it in place
	# of Classic's, so 20,768, inside the menu button's first square, is
	# the panel's colour; unsetting it brings Classic's panel back. Kept
	# as panels-clock.png.
	panel=$(token panel)
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	guest 'panels clock'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "edel system set shell.panels did not restart shell-ui"
	wait_for 'edel-shell-ui: panel places clock [0-9]+\+[0-9]+$' || fail "shell-ui's panel does not hold the clock alone"
	shot panels-clock 20 768 "$panel" >/dev/null || fail "20,768 is not the panel's colour: the menu button is still there"
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	classic=$(count 'edel-shell-ui: panel places menu 0\+')
	guest 'panels default'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "edel system unset shell.panels did not restart shell-ui"
	wait_more 'edel-shell-ui: panel places menu 0\+' "$classic" || fail "unsetting shell.panels did not bring Classic's menu button back"
	echo "PASS: shell.panels with only the clock replaced Classic's panel at once, and unsetting it brought Classic's panel back"
}

case_dockhide() {
	# A dock that hides while covered (M5.4f): shell.panels with one dock
	# along the bottom, hide = "covered", holding the apps. Uncovered, it
	# shows; a 1200x740 window over it hides it, so its left end shows
	# the window's colour; the pointer at the screen's bottom edge brings
	# it back, and away from it the dock hides again. Kept as
	# dock-shown.png, dock-hidden.png and dock-back.png.
	panel=$(token panel)
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	guest 'dock hiding'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "setting a hiding dock in shell.panels did not restart shell-ui"
	wait_for 'edel-shell-ui: panel places apps 8\+[0-9]+$' || fail "the dock does not hold the apps"
	sleep 1
	w=$(tr -d '\r' <"$log" | sed -n 's/.*edel-shell-ui: panel places apps 8+\([0-9]*\)$/\1/p' | tail -n 1)
	x=$((640 - (w + 16) / 2 + 3))
	python3 ci/qmp.py move 640 300
	shot dock-shown "$x" 762 "$panel" >/dev/null || fail "the uncovered dock is not at $x,762 in the panel's colour"
	opened=$(count 'edel-compositor: mapped window big')
	guest 'big window'
	wait_more 'edel-compositor: mapped window big' "$opened" || fail "the window big did not open"
	shot dock-hidden "$x" 762 884488 >/dev/null || fail "with big over it, $x,762 is not big's colour: the dock did not hide"
	python3 ci/qmp.py move 640 799
	shot dock-back "$x" 762 "$panel" >/dev/null || fail "the pointer at the bottom edge did not bring the dock back"
	python3 ci/qmp.py move 640 300
	shot dock-hidden "$x" 762 884488 >/dev/null || fail "the pointer away from the dock did not let it hide again"
	closed=$(count 'edel-compositor: unmapped window big')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window big' "$closed" || fail "Super+Q did not close big"
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	guest 'panels default'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "unsetting shell.panels did not restart shell-ui"
	echo "PASS: a dock with hide = \"covered\" showed while uncovered, hid under big, came back with the pointer at the bottom edge and hid again when it left"
}

case_portal() {
	# The settings portal (M5.5a): an app asking xdg-desktop-portal for
	# the colour scheme and the accent hears shell-ui's answer, from the
	# tokens: prefer dark, 1, as the tokens are dark, and the accent
	# #5b8ef5 as three numbers from 0 to 1.
	guest 'portal read'
	wait_for 'DESKTOP-TEST: portal accent-color ' 60 || fail "the service did not hear back from the portal"
	scheme=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: portal color-scheme //p' | tail -n 1)
	accent=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: portal accent-color //p' | tail -n 1)
	case "$scheme" in
	"(<uint32 1>,)"*) ;;
	*) fail "the portal's colour scheme is not prefer dark: $scheme" ;;
	esac
	echo "$accent" | grep -qE '^\(<\(0\.35[0-9]*, 0\.55[0-9]*, 0\.96[0-9]*\)>,\)' ||
		fail "the portal's accent is not the tokens' #5b8ef5: $accent"
	echo "PASS: an app asking xdg-desktop-portal heard shell-ui's answer: colour scheme $scheme and accent $accent"
}

case_scheme() {
	# Light and dark (M5.5c): edel system set appearance.color_scheme=light
	# gives the compositor the tokens' [colour.light], drawing the title
	# bars and the background again, and restarts shell-ui, whose panel
	# and portal follow; the portal tells apps already open with
	# SettingChanged, which xdg-desktop-portal passes on. Unset, it is
	# dark again. Kept as scheme-light.png and scheme-dark.png.
	guest 'portal watch'
	opened=$(count 'edel-compositor: mapped window keys')
	guest 'shortcut window'
	wait_more 'edel-compositor: mapped window keys' "$opened" || fail "the test client keys did not open"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*edel-compositor: mapped window keys at \([0-9]*\),\([0-9]*\) .*/\1 \2/p' | tail -n 1)
	read -r x y <<-EOF
		$place
	EOF
	for scheme in light dark; do
		if [ "$scheme" = light ]; then
			table=colour.light
			command='scheme light'
		else
			table=colour
			command='scheme default'
		fi
		said=$(count "edel-compositor: colour scheme $scheme")
		restarts=$(count "edel-compositor: restarting edel-shell-ui: the colour scheme is now $scheme")
		guest "$command"
		wait_more "edel-compositor: colour scheme $scheme" "$said" ||
			fail "the compositor did not take the $scheme scheme"
		wait_more "edel-compositor: restarting edel-shell-ui: the colour scheme is now $scheme" "$restarts" ||
			fail "the compositor did not restart shell-ui for the $scheme scheme"
		shot "scheme-$scheme" 640 790 "$(token panel "$table")" >/dev/null ||
			fail "the panel at 640,790 is not the $scheme scheme's #$(token panel "$table")"
		shot "scheme-$scheme-bar" $((x + 5)) $((y - 24)) "$(token title_bar_focused "$table")" >/dev/null ||
			fail "keys' title bar at $((x + 5)),$((y - 24)) is not the $scheme scheme's #$(token title_bar_focused "$table")"
		shot "scheme-$scheme-background" 4 4 "$(token background "$table")" >/dev/null ||
			fail "the background at 4,4 is not the $scheme scheme's #$(token background "$table")"
		asked=$(count 'DESKTOP-TEST: portal accent-color ')
		guest 'portal read'
		wait_more 'DESKTOP-TEST: portal accent-color ' "$asked" 60 || fail "the service did not hear back from the portal"
		read=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: portal color-scheme //p' | tail -n 1)
		want=1
		[ "$scheme" = dark ] || want=2
		case "$read" in
		"(<uint32 $want>,)"*) ;;
		*) fail "in the $scheme scheme the portal's colour scheme is not $want: $read" ;;
		esac
	done
	guest 'portal signals'
	wait_for 'DESKTOP-TEST: portal signals ' || fail "the service did not print the portal's signals"
	signals=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: portal signals //p' | tail -n 1)
	for want in 2 1; do
		echo "$signals" | grep -q "member=SettingChanged string \"org.freedesktop.appearance\" string \"color-scheme\" variant uint32 $want" ||
			fail "xdg-desktop-portal did not tell apps the colour scheme changed to $want: $signals"
	done
	closed=$(count 'edel-compositor: unmapped window keys')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window keys' "$closed" || fail "Super+Q did not close keys"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Light and dark (M5.5c): scheme-light.png and scheme-dark.png are in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: appearance.color_scheme=light turned the panel, keys' title bar and the background to the light tokens, the portal said prefer light (2) and xdg-desktop-portal passed SettingChanged on to apps; unset, all of it went back to dark (1)"
}

case_buttons() {
	# Window buttons on either side (M5.4b): with shell.window_buttons =
	# "left", close is the bar's leftmost 28 px square, then minimize and
	# maximize, and a click there closes the window; unset, they go back
	# to the preset's right. Kept as buttons-left.png.
	focused=$(token title_bar_focused)
	guest 'buttons left'
	wait_for 'edel-compositor: window buttons on the left' ||
		fail "the compositor did not follow shell.window_buttons = \"left\""
	opened=$(count 'edel-compositor: mapped window keys')
	guest 'shortcut window'
	wait_more 'edel-compositor: mapped window keys' "$opened" || fail "the test client keys did not open"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*edel-compositor: mapped window keys at \([0-9]*\),\([0-9]*\) .*/\1 \2/p' | tail -n 1)
	read -r x y <<-EOF
		$place
	EOF
	shot buttons-left $((x + 5)) $((y - 24)) "$focused" >/dev/null ||
		fail "keys at $x,$y has no focused title bar at $((x + 5)),$((y - 24))"
	closed=$(count 'edel-compositor: unmapped window keys')
	python3 ci/qmp.py click $((x + 13)) $((y - 14))
	wait_more 'edel-compositor: unmapped window keys' "$closed" ||
		fail "a click on the bar's leftmost square at $((x + 13)),$((y - 14)) did not close keys"
	guest 'buttons default'
	wait_for 'edel-compositor: window buttons on the right' ||
		fail "unsetting shell.window_buttons did not bring the buttons back to the right"
	echo "PASS: shell.window_buttons = \"left\" put close at the bar's left end, a click there closed keys at $x,$y, and unsetting it brought the buttons back to the right"
}

# list_until TEST: waits up to 10 s for shell-ui's last places line to
# give its window list a width W for which [ W TEST ] holds, and prints
# the list's x and W.
list_until() {
	i=0
	while :; do
		place=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 | sed -n 's/.*windows \([0-9]*\)+\([0-9]*\).*/\1 \2/p')
		[ "${place#* }" $1 ] 2>/dev/null && break
		i=$((i + 1))
		[ "$i" -lt 50 ] || break
		sleep 0.2
	done
	echo "$place"
	[ "${place#* }" $1 ] 2>/dev/null
}

# list_width N: the window list's width with N buttons on CI's 1280 px
# panel, as shell-ui's windows widget lays them out: each button at most
# 180 px and together at most 45% of the panel (576 px), 4 px at each end
# and between them.
list_width() {
	[ "$1" -gt 0 ] || { echo 0; return; }
	b=$(((576 - 8 - ($1 - 1) * 4) / $1))
	[ "$b" -le 180 ] || b=180
	[ "$b" -ge 32 ] || b=32
	echo $((8 + $1 * b + ($1 - 1) * 4))
}

case_windows() {
	# The panel's window list (M5.2h): a button for each window on the
	# screen, sharing its width, 4 px at each end and between them.
	# Launching away adds one, lit with the accent line along its foot,
	# 2 px high and 4 px above the foot of a 30 px button in the middle of
	# the 40 px panel that starts at 760: row 791.
	panel=$(token panel)
	accent=$(token accent)
	opened=$(count 'edel-compositor: mapped window away')
	# Counted before: earlier cases left such windows lines too.
	listed=$(count 'DESKTOP-TEST: windows [0-9]+ .*away@[0-9]+,[0-9]+,200x150$')
	guest 'away window'
	wait_more 'edel-compositor: mapped window away' "$opened" || fail "the test client away did not open: $(value windows)"
	wait_more 'DESKTOP-TEST: windows [0-9]+ .*away@[0-9]+,[0-9]+,200x150$' "$listed" || fail "away is not on top in the state file: $(value windows)"
	# The width is waited for, not compared with an earlier one: the
	# panel may still be catching up with the case before.
	n=$(value windows | cut -d' ' -f1)
	place=$(list_until "-eq $(list_width "$n")") ||
		fail "the window list is not $(list_width "$n") px wide with $n windows, away among them: $place"
	read -r x w <<-EOF
		$place
	EOF
	button=$(((w - 8 - (n - 1) * 4) / n))
	cx=$((x + w - 4 - button / 2))
	shot windows "$cx" 791 "$accent" >/dev/null || fail "away's button, the last of $n at $cx, has no accent line at row 791"
	# Its title bar's minimize button, the third square from the right,
	# hides it; its button stays, with no mark.
	read -r ax ay aw <<-EOF
		$(value windows | sed -n 's/.*away@\([0-9]*\),\([0-9]*\),\([0-9]*\)x150$/\1 \2 \3/p')
	EOF
	python3 ci/qmp.py click $((ax + aw - 69)) $((ay - 14))
	wait_for 'edel-compositor: minimized window away' || fail "away's minimize button at $((ax + aw - 69)),$((ay - 14)) did not minimize it"
	wait_for 'DESKTOP-TEST: windows [0-9]+ .*away@[0-9]+,[0-9]+,200x150-$' || fail "the state file does not say away is minimized: $(value windows)"
	shot windows 640 393 '!7744aa' >/dev/null || fail "away is still drawn at 640,393"
	shot windows "$cx" 791 "$panel" >/dev/null || fail "away's button still has a mark at $cx,791"
	# A click on its button brings it back, focused; 40 px left of the
	# line, which the cursor would cover.
	python3 ci/qmp.py click $((cx - 40)) 780
	wait_for 'edel-compositor: restored window away' || fail "a click on away's button at $((cx - 40)),780 did not bring it back"
	shot windows "$cx" 791 "$accent" >/dev/null || fail "away's button has no accent line after it came back"
	shot windows 640 393 7744aa >/dev/null || fail "away is not drawn at 640,393 after it came back"
	# Closed, its button goes.
	guest 'away off'
	place=$(list_until "-eq $(list_width $((n - 1)))") ||
		fail "the window list is not $(list_width $((n - 1))) px wide for $((n - 1)) windows after away closed: $place"
	echo "PASS: away's opening added a button to the panel's window list with the accent line, its title bar's minimize button hid it and left the button unmarked, a click on the button brought it back, and closing it took the button away"
}

case_rollback() {
	guest 'break update'
	wait_for 'DESKTOP-TEST: rollback: (slot B has|FAIL)' "${DESKTOP_ROLLBACK_TIMEOUT:-240}" ||
		fail "the test service did not install the broken update: $(tr -d '\r' <"$log" | grep -a 'rollback' | tail -n 3)"
	value rollback: | grep -q '^FAIL' && fail "$(value rollback:)"
	# Two boots: slot B, broken, then slot A once the guard gave up on B.
	wait_for 'DESKTOP-TEST: rollback: (PASS|FAIL)' "${DESKTOP_ROLLBACK_TIMEOUT:-240}" ||
		fail "the VM did not come back on slot A: $(tr -d '\r' <"$log" | grep -a -E 'rollback|edel guard|running:' | tail -n 6)"
	value rollback: | grep -q '^PASS' || fail "$(value rollback:)"
	tr -d '\r' <"$log" | grep -q 'DESKTOP-TEST: rollback: slot B runs' ||
		fail "slot B never ran, so its fallback proves nothing"
	echo "PASS: an update whose compositor fails was installed into slot B; slot B never showed a frame, the guard let the watchdog restart it and the VM came back on slot A, with slot B switched off"
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
	wait_for 'DESKTOP-TEST: windows 2 foot@9,36,[0-9]+x[0-9]+ one@645,36,626x715' ||
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
	colour=$(shot tiling 14 12 "$bar|$focused") ||
		fail "foot's tile's bar is #$colour at 14,12, not a title bar colour"
	colour=$(shot tiling 650 12 "$bar|$focused") ||
		fail "one's tile's bar is #$colour at 650,12, not a title bar colour"
	# Super+T: this workspace floats again, each window where it was.
	python3 ci/qmp.py key meta_l-t
	wait_for 'edel-compositor: windows now floating' || fail "Super+T did not switch back to floating"
	wait_for 'DESKTOP-TEST: windows 2 foot@442,249,396x288 one@722,145,300x200' ||
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
	# A window opens on the screen the pointer is on (M5.2g): the mouse
	# takes the pointer over the shared edge onto Virtual-2, where away,
	# 200x150, opens centred in that screen's own area, which has no
	# panel: its frame, 202x179, at 1691,294.
	python3 ci/qmp.py move 1270 400
	python3 ci/qmp.py nudge 300 0
	opened=$(count 'edel-compositor: mapped window away')
	guest 'away window'
	wait_more 'edel-compositor: mapped window away' "$opened" || fail "the test client away did not open"
	wait_for 'DESKTOP-TEST: windows [0-9]+ .*away@1692,322,200x150$' ||
		fail "away did not open centred on Virtual-2, where the pointer is: $(value windows)"
	closed=$(count 'edel-compositor: unmapped window away')
	guest 'away off'
	wait_more 'edel-compositor: unmapped window away' "$closed" || fail "away did not close"
	# The pointer back on Virtual-1 before Virtual-2 goes.
	python3 ci/qmp.py move 640 400
	guest 'screen 2 off'
	wait_for 'DESKTOP-TEST: ran screen 2 off: 0' ||
		fail "edel system set outputs.Virtual-2.enabled=false did not run in the VM"
	wait_for 'edel-compositor: output Virtual-2 off' ||
		fail "the compositor did not turn Virtual-2 off"
	echo "PASS: two screens lit side by side, Virtual-1 at 0,0 and Virtual-2 at 1280,0 in the system file's mode 1024x768, a window opened centred on Virtual-2 with the pointer there, and outputs.Virtual-2.enabled = false turned the second off"
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

[ "$#" -gt 0 ] || set -- floating titlebar tiling console pointer outputs compositor panel xwayland layers animations shortcuts workspaces windows launcher switcher presets buttons taskbar dock panels dockhide portal scheme scale respawn
for c in "$@"; do
	case "$c" in
	animations | buttons | console | compositor | dock | dockhide | floating | launcher | layers | outputs | panel | panels | pointer | portal | presets | respawn | scale | scheme | shortcuts | switcher | taskbar | tiling | titlebar | windows | workspaces | xwayland) ;;
	rollback) [ "$#" = 1 ] || { echo "rollback runs alone: it restarts the VM"; exit 1; } ;;
	*)
		echo "unknown case $c; the cases are animations, buttons, console, compositor, dock, dockhide, floating, launcher, layers, outputs, panel, panels, pointer, portal, presets, respawn, rollback, scale, scheme, shortcuts, switcher, taskbar, tiling, titlebar, windows, workspaces and xwayland"
		exit 1
		;;
	esac
done

# Any restart ends the VM, so a crash is never missed, except in rollback,
# whose VM restarts on purpose and fetches the update from this host, the
# guest's 10.0.2.2, on a port of its own (ab-test.sh has 8000).
restart=-no-reboot
if [ "$*" = rollback ]; then
	restart=''
	python3 -m http.server 8001 --bind 127.0.0.1 --directory "$dir/update" >out/desktop-http.log 2>&1 &
	server=$!
	trap 'kill "$server" 2>/dev/null || true' EXIT
fi

keep_vm=1 run_vm "$log" 'DESKTOP-TEST: (done|FAIL)' "${DESKTOP_TEST_TIMEOUT:-300}" $restart -snapshot \
	-m 2048 -smp 4 -vga none -device virtio-vga,max_outputs=2 \
	-device virtio-keyboard-pci -device virtio-tablet-pci -device virtio-mouse-pci \
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
