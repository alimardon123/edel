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
#               CI's session forces, logged, and the same from edel
#               status), the top left corner in the background colour of
#               design/tokens.toml, and its numbers (boot to ready, memory,
#               its RSS, frame p99, idle frames) within ci/budgets.toml,
#               written to the step summary
#   floating    the two test clients where the floating policy puts them,
#               in the state file and in their colours on screen
#   titlebar    every window with a title bar in the token colours and its
#               title on it; a click on two's close button closes it, and
#               dragging one's bar moves it
#   tiling      edel settings set layout.tiling=true, sent to the VM, tiles
#               foot and one side by side with their bars; Super+T puts
#               them back where they floated, overlapping
#   pointer     the cursor where the pointer is (drawn into the frame in
#               CI, EDEL_SOFTWARE_CURSOR=1), and wayland-info lists the
#               tablet, cursor shape, fractional scale and viewporter
#               protocols
#   outputs     both screens of virtio-vga,max_outputs=2 lit (the second
#               forced on by the image's kernel command line, at the seed
#               settings file's mode 1024x768), side by side in the state
#               file, and the second turned off by
#               edel settings set displays.Virtual-2.enabled=false; a
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
#   animations  appearance.animations = "reduced" logs fades only and its
#               removal logs the tier's own animations again; ten windows
#               opening one after another and closing at the tier llvmpipe
#               picks keep the frame budget (M5.11b)
#   shortcuts   edel settings set shortcuts.close_window=Super+W, sent to the VM,
#               moves close_window: Super+Q then leaves a window open and Super+W
#               closes it; Ctrl+Alt+T opens a terminal; removing the key
#               brings Super+Q back (M5.13a)
#   workspaces  Super+Shift+2 sends a new window to workspace 2, which
#               the state file says while workspace 1 stays shown;
#               Super+2 shows it tiled, as layout.tiling from the tiling
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
#   quick       the status area's pill in the panel (a click on it) opens quick
#               settings above it, the panel's colour at 1000,745 where the
#               background was; the output's volume, set to 20 percent, is
#               the slider's accent at a tenth of its track, a drag on the
#               track from three tenths to half sets the sink to about 0.50 as
#               wpctl status shows, a click on the Dark style tile writes
#               appearance.mode to ci's settings file and the compositor takes
#               the dark colours; Escape and a second click on the pill close
#               the card (M5.9a)
#   notify      a notification sent on ci's session bus as an app sends it
#               (gdbus call, org.freedesktop.Notifications.Notify) shows a
#               banner at the panel's corner in the panel's colour; a click
#               on the clock opens the notification centre with it listed;
#               with notifications.do_not_disturb set, a second one is
#               listed but shows no banner; unset, a third shows one again
#               and it goes by itself (M5.9b)
#   switcher    with Alt held, Tab shows the window switcher with the
#               window used before chosen; letting go switches (M5.3c)
#   styles      layout.tiling_style = "split" leaves workspace 4 floating;
#               Super+T tiles four windows there, each halving the one
#               before; Super+Shift+Left swaps the last two, and stack
#               lays them out as master and stack (M5.16a); the layout
#               button's right-click menu chooses split (M5.16b)
#   scroll      layout.tiling_style = "scroll" lays four windows on workspace
#               4 out as columns, the newest whole and the first off screen;
#               Super+Left and the window list scroll to them (M5.16c)
#   sandbox     a client in a security context sees none of the shell's
#               four protocols, which a plain client sees (M5.22)
#   everyday    the five everyday protocols are offered, a locked pointer
#               gives its window relative motion, and a window keeping the
#               screen on is counted in the state file (M5.23)
#   language    region.language = "xx" restarts shell-ui in a made-up
#               language, whose launcher draws its one long word where
#               English leaves the search line empty; unset, English
#               (M5.24a)
#   display     Settings opened on its Displays page (edel-settings --page
#               displays) draws the chosen scale in the accent; a click on
#               Scale 200% for Virtual-1 makes the compositor log
#               `output Virtual-1 scale 2` and writes the line to ci's
#               settings file, a click on its Reset logs scale 1 and takes
#               the line out (M5.7a)
#   sound       the compositor started PipeWire, WirePlumber and the
#               PulseAudio server from the preset's session list (M5.7b);
#               the VM's sound card (-device intel-hda -device hda-duplex)
#               shows as a sink in wpctl status, and Settings opened on
#               its Sound page (edel-settings --page sound), the volume
#               slider holding the keyboard, takes Home and six presses of
#               Right and wpctl status shows the sink at 0.30; kept as
#               sound.png
#   network     NetworkManager and BlueZ (M5.8a): nmcli -t general status
#               reports connected (the VM's user-mode network, by
#               NetworkManager's own DHCP), ci, in group seat, may change the
#               network and nobody may not (the network feature's D-Bus
#               policy stands in for polkit), bluetoothctl list exits 0 with
#               no adapter in the VM; Settings opened on its Network and
#               Bluetooth pages (edel-settings --page network, --page
#               bluetooth) draws each page and says on its stderr what its
#               headline is ("Connected", "This computer has no Bluetooth");
#               kept as network.png and bluetooth.png
#   power       UPower, and the Power and Users pages (M5.8b): upower -e
#               exits 0 as ci with upowerd running (QEMU has no battery, so
#               it lists the display device and perhaps a power cable);
#               Settings opened on its Power and Users pages
#               (edel-settings --page power, --page users) draws each page
#               and says on its stderr what its headline is ("Plugged in",
#               "ci"); then, tiled alone on a screen made 366 logical px
#               wide by scale 3.5, Compact, each page again: the window is no
#               wider than the screen, the sidebar is folded away and the
#               page's margins on both sides are the window token's colour;
#               kept as power.png, users.png, power-compact.png and
#               users-compact.png
#   updates     updates.channel is refused unless it is a channel name,
#               read back, and an edel update --check with no release
#               takes that channel's list; Settings opened on its Updates
#               page (edel-settings --page updates) runs edel status and
#               says so on its stderr, and its card at the top is the
#               card token's colour; kept as updates.png (M5.8c); the
#               About page (edel-settings --page about) likewise, kept
#               as about.png (M5.8d)
#   tray        a StatusNotifierItem from edel-testclient --sni (a
#               #33aa66 pixmap) registers with shell-ui's watcher and its
#               icon lies in the panel's tray; AT-SPI names it, a click
#               and a right click reach it, and the icon goes with its app
#               (M5.2e)
#   scale       edel settings set displays.Virtual-1.scale=2 halves the
#               logical screen and doubles the title bar's height in
#               screen pixels, after the other cases
#   respawn     kill -9 edel-compositor ends the session, and greetd's
#               greeter, our compositor, logs a new ready line within 5 s,
#               picks the Lite tier itself under llvmpipe, and owns the
#               health file; typing ci and a password into
#               its agreety starts ci's compositor again; last, as it
#               ends the first session
#   live        alone (CI runs it in the VM test lane, as it boots another
#               image): the desktop image as released, out/edel-desktop-
#               x86_64.img, started as a removable USB stick (QEMU's
#               usb-storage, removable=on) on a plain screen (-vga std),
#               with no seed and no test service; edel boot live says the
#               stick logs live in, the slot is confirmed, and the panel
#               lies along the bottom in the token colour, so the desktop
#               showed with nobody logging in (M3.6); and the checks the
#               laptop stick had in boot-test: /data, the hardware
#               watchdog, the report on the EFI system partition, ssh off
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

# token KEY [TABLE]: the colour KEY has in design/tokens.toml's
# [colour.light] table, the release's default scheme (M5.12a), or in
# TABLE, such as colour for the dark one (M5.5c), as rrggbb.
token() {
	awk -v key="$1" -v table="[${2:-colour.light}]" '
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
	*) fail "the compositor wrote no ready line to its health file: \"$line\"" ;;
	esac
	# llvmpipe draws on the CPU, so the compositor picks the Lite tier
	# itself (M5.11); edel status reads the tier now from the state
	# file, and the animations follow it.
	case "$(value tier)" in
	"edel-compositor: tier=lite (renderer llvmpipe"*) ;;
	*) fail "the compositor did not pick tier=lite under llvmpipe: \"$(value tier)\"" ;;
	esac
	now=$(value tier_now)
	[ "tier=$(value status_tier)" = "$now" ] ||
		fail "edel status said effects \"$(value status_tier)\", but the compositor logged $now last"
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
	check 'Memory the desktop holds, session idle' "$(value memory_held_mib)" desktop_held_mib MiB
	echo "memory in use (MemTotal less MemAvailable, not budgeted): $(value memory_in_use_mib) MiB"
	check 'Compositor RSS' "$(value compositor_rss_mib)" compositor_rss_mib MiB
	check 'Time between frames, p99' "$(value frame_p99_ms)" frame_p99_ms ms
	check 'Frames drawn while idle' "$(value idle_frames)" idle_frames ''
	echo "memory in use, by part (MiB): $(value memory_parts)"
	echo "memory in use, the most per process (Pss MiB): $(value memory_top)"
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
	# The session's log (M5.28a), ci's ~/.local/state/edel/session.log:
	# the new session warns that the one before ended without closing,
	# edel status points to that log, and edel report holds both logs'
	# last lines, the new one's ready line among them.
	wait_for 'DESKTOP-TEST: log_report ' || fail "no word from the test service about the session's log"
	[ "$(value log_warned)" = 1 ] || fail "the new session's log does not say the session before ended without closing"
	case "$(value log_status)" in
	"logs: ci's last desktop session ended without closing; its log is /home/ci/.local/state/edel/session.old.log"*) ;;
	*) fail "edel status did not point to the killed session's log: \"$(value log_status)\"" ;;
	esac
	read -r logs ready <<-EOF
		$(value log_report)
	EOF
	[ "$logs" = 2 ] && [ "$ready" -ge 1 ] ||
		fail "edel report holds $logs session logs, not 2, with $ready ready lines, not at least 1"
	echo "PASS: kill -9 edel-compositor ended the session, greetd's greeter, our compositor, logged \"$(value greeter | sed 's/^edel-compositor: //')\" $seconds s later and showed agreety in foot, and logging in there started ci's compositor again, whose log said the session before ended without closing; edel status pointed to that log, and edel report held both logs' last lines"
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
	# switches whatever the settings file says: the tiling case leaves
	# layout.tiling at true with the windows floating.
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
	# The clock, at the panel's right end, is drawn: not one colour. It
	# is two lines (M5.29), the time over a short date, right-aligned to
	# 8 px from the panel's end: the time's letters lie in rows 768 to
	# 779 and the date's, a step smaller, in rows 784 to 792, so each
	# region is not one colour and the rows between them are.
	clock=$(python3 ci/qmp.py uniform "$dir/panel.png" 1214 765 62 30)
	[ "$clock" = varied ] || fail "the clock's region at the panel's right end is $clock"
	time_lines=$(python3 ci/qmp.py uniform "$dir/panel.png" 1236 768 36 12)
	[ "$time_lines" = varied ] || fail "the clock's time, at 1236,768 36x12, is $time_lines"
	date_lines=$(python3 ci/qmp.py uniform "$dir/panel.png" 1226 784 46 9)
	[ "$date_lines" = varied ] || fail "the clock's date, below the time at 1226,784 46x9, is $date_lines"
	between=$(python3 ci/qmp.py uniform "$dir/panel.png" 1226 781 46 2)
	[ "$between" = "uniform $panel" ] || fail "the rows between the clock's time and date, at 1226,781 46x2, are $between, not the panel's #$panel"
	cp "$dir/panel.png" "$dir/panel-polished.png"
	# The layout toggle (M5.3a): a click switches the shown workspace's
	# policy over edel-shell-v1, as Super+T, and fills the button, 30 px
	# wide after 2 px of room, with the accent; another switches it back.
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
	echo "$tree" | grep -qE '^3 label: [0-9]{2}:[0-9]{2}, [A-Z][a-z]{2} [0-9]{1,2} [A-Z][a-z]{2}$' ||
		fail "AT-SPI holds no clock among the panel's widgets: $(echo "$tree" | tr '\n' ';')"
	# Its memory, as the service read it once the desktop was idle, with
	# no screen reader: what it holds of its own, without the pages it
	# maps from files, whose count swings from run to run.
	own=$(value shell_ui_own_mib)
	peak=$(value shell_ui_own_peak_mib)
	rss=$(value shell_ui_rss_mib)
	limit=$(budget shell_ui_own_mib)
	awk -v m="$own" -v b="$limit" 'BEGIN { exit !(m != "" && m <= b) }' ||
		fail "edel-shell-ui keeps ${own:-?} MiB of its own once settled (${peak:-?} MiB at most over 8 s, reads $(value shell_ui_own_reads), $(value shell_ui_threads) threads, $rss MiB resident), over its budget of $limit MiB; its largest mappings $(value shell_ui_anon_top), anonymous ranges $(value shell_ui_anon_ranges), threads and stack pointers $(value shell_ui_thread_stacks)"
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
	# appearance.animations (M5.11b): reduced keeps the fades and drops the
	# growing and sliding, which Lite has none of; removing the key logs
	# the tier's own animations again.
	full=$(tr -d '\r' <"$log" | grep -c 'motion full: open' || true)
	guest 'motion reduced'
	wait_for 'edel-compositor: animations at tier [a-z]+, motion reduced: open [1-9][0-9]* ms, close [1-9][0-9]* ms, slide 0 ms' ||
		fail "appearance.animations = \"reduced\" did not leave fades only: $(tr -d '\r' <"$log" | grep -a 'animations at' | tail -n 1)"
	reduced=$(tr -d '\r' <"$log" | grep -a 'motion reduced: open' | tail -n 1 | sed 's/.*: open/open/')
	guest 'motion default'
	i=0
	while [ "$(tr -d '\r' <"$log" | grep -c 'motion full: open' || true)" -le "$full" ]; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "removing appearance.animations did not bring the full animations back"
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
	# Measured as two pairs, off then full, and the smaller of the two
	# differences counts: a moment the runner was busy lifts one run of a
	# pair (4.1 ms with motion off and 1.8 ms added on 2026-10-08, against
	# 0.7 ms added on the runs before and after), while animations that
	# really cost more lift both pairs alike.
	added=
	for pair in 1 2; do
		off=$(count 'edel-compositor: animations at tier [a-z]+, motion off: open 0 ms')
		guest 'motion off'
		wait_more 'edel-compositor: animations at tier [a-z]+, motion off: open 0 ms' "$off" ||
			fail "appearance.animations = \"off\" did not stop the animations"
		windows=$(value windows)
		runs=$(count 'DESKTOP-TEST: open_ten_tier ')
		guest 'open ten'
		wait_more 'DESKTOP-TEST: open_ten_tier ' "$runs" 90 || fail "no word from the test service on the ten windows"
		pair_off=$(value open_ten_p99_ms)
		tier_off=$(value open_ten_tier)
		i=0
		while [ "$(value windows)" != "$windows" ]; do
			i=$((i + 1))
			[ "$i" -lt 100 ] || fail "the ten windows did not all close: $(value windows)"
			sleep 0.2
		done
		full=$(count 'motion full: open')
		guest 'motion default'
		wait_more 'motion full: open' "$full" || fail "removing appearance.animations again did not bring the animations back"
		runs=$(count 'DESKTOP-TEST: open_ten_tier ')
		guest 'open ten'
		wait_more 'DESKTOP-TEST: open_ten_tier ' "$runs" 90 || fail "no word from the test service on the ten windows"
		pair_full=$(value open_ten_p99_ms)
		pair_frames=$(value open_ten_frames)
		tier=$(value open_ten_tier)
		pair_added=$(awk -v m="$pair_full" -v o="$pair_off" 'BEGIN { if (m != "" && o != "") printf "%.1f", m - o }')
		echo "ten windows, pair $pair: frames p99 ${pair_off:-?} ms with motion off (tier $tier_off), ${pair_full:-?} ms with it full (tier $tier), ${pair_added:-?} ms added"
		if [ -z "$added" ] || awk -v a="$pair_added" -v b="$added" 'BEGIN { exit !(a != "" && a < b) }'; then
			added=$pair_added p99=$pair_full p99_off=$pair_off frames=$pair_frames
		fi
		i=0
		while [ "$(value windows)" != "$windows" ]; do
			i=$((i + 1))
			[ "$i" -lt 100 ] || fail "the ten windows did not all close: $(value windows)"
			sleep 0.2
		done
	done
	limit=$(budget animation_p99_ms)
	echo "ten windows: the smaller pair added ${added:-?} ms (${p99_off:-?} ms off, ${p99:-?} ms full), budget $limit ms"
	[ "${frames:-0}" -ge 100 ] || fail "weston-presentation-shm timed only ${frames:-no} frames while the ten windows opened"
	awk -v a="$added" -v b="$limit" 'BEGIN { exit !(a != "" && a <= b) }' ||
		fail "with ten windows opening and closing at tier $tier, the animations added ${added:-?} ms to the frames at p99 in the better of two pairs (${p99_off:-?} ms with motion off, ${p99:-?} ms with it full), over the budget of $limit ms"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Ten windows opening and closing with animations (M5.11b): frames p99 $p99 ms, $p99_off ms with motion off, $added ms added (budget $limit ms), $frames frames, tier $tier." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: appearance.animations = reduced left fades only ($reduced) and its removal brought the tier's own back; ten windows opened and closed at tier $tier with frames p99 $p99 ms over $frames frames, $added ms more than without animations ($p99_off ms), within $limit ms"
}

case_shortcuts() {
	# [shortcuts] (M5.13a), live from the settings file: a window of its own,
	# which takes the keyboard as it opens.
	guest 'shortcut window'
	wait_more 'edel-compositor: mapped window keys' 0 || fail "the test client keys did not open: $(value windows)"
	guest 'close on super-w'
	wait_for 'edel-compositor: shortcut close_window is Super\+W' ||
		fail "the compositor did not follow shortcuts.close_window = \"Super+W\""
	# Super+Q now reaches the window, which ignores it; Super+W closes it.
	python3 ci/qmp.py key meta_l-q
	sleep 2
	[ "$(count 'edel-compositor: unmapped window keys')" = 0 ] ||
		fail "Super+Q still closed keys after close_window moved to Super+W"
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
	wait_for 'edel-compositor: shortcut close_window is Super\+Q' ||
		fail "removing shortcuts.close_window did not bring Super+Q back"
	echo "PASS: shortcuts.close_window = \"Super+W\" moved close off Super+Q at once, Super+W closed keys, Ctrl+Alt+T opened foot, and removing the key brought Super+Q back"
}

case_settings() {
	# Settings (M5.6a): the app opens as a window with our title bar,
	# asked for through KDE's server decoration protocol, its sidebar of
	# pages in the tokens' card colour beside the page in their window
	# colour; the chosen preset's card, Classic's, has a border in the
	# accent; a click on Mac-like's card writes layout.preset to ci's own
	# settings file, and the desktop switches to Mac-like at once; a click
	# on Classic, the default, takes the key out again. Kept as
	# settings.png and settings-mac.png.
	opened=$(count 'edel-compositor: mapped window Settings')
	guest 'settings window'
	wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open a window: $(value windows)"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
	set -- $place
	x=$1 y=$2 w=$3 h=$4
	panel=$(token panel)
	page=$(token window)
	side=$(token card)
	sleep 2
	shot settings $((x + 20)) $((y + h - 40)) "$side" >/dev/null ||
		fail "Settings' sidebar at $((x + 20)),$((y + h - 40)) is not the card token's #$side"
	shot settings $((x + w - 40)) $((y + h - 40)) "$page" >/dev/null ||
		fail "Settings' page at $((x + w - 40)),$((y + h - 40)) is not the window token's #$page"
	# Our title bar above it: the compositor's focused bar colour.
	shot settings $((x + 8)) $((y - 14)) "$(token title_bar_focused)" >/dev/null ||
		fail "Settings has no title bar of ours at $((x + 8)),$((y - 14))"
	# The page's cards (crates/settings): a 204 px sidebar, the page's
	# text and cards at most 704 px wide with 32 px margins, centred in
	# what is left, three cards to a line 12 px apart.
	area=$((w - 204))
	clamp=$((area < 704 ? area : 704))
	left=$((x + 204 + (area - clamp) / 2 + 32))
	inner=$((clamp - 64))
	cell=$(((inner - 24) / 3))
	top=none
	for dx in 3 4 5 2 6; do
		top=$(python3 ci/qmp.py find "$dir/settings.png" $((left + dx)) "$y" $((y + h)) "$(token accent)")
		[ "$top" != none ] && break
	done
	[ "$top" != none ] || fail "no chosen card's accent border at $left in Settings"
	classic=$((top + 40))
	mac=$((left + cell + 12 + cell / 2))
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now mac-like')
	python3 ci/qmp.py click "$mac" "$classic"
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now mac-like' "$restarts" ||
		fail "a click on Mac-like at $mac,$classic did not switch the desktop to Mac-like"
	guest 'settings file'
	wait_for 'DESKTOP-TEST: settings_file ' || fail "the service did not read ci's settings file"
	value settings_file | grep -q '\[layout\];preset = "mac-like"' ||
		fail "ci's settings file is not what edel settings set layout.preset=mac-like writes: $(value settings_file)"
	shot settings-mac 640 12 "$panel" >/dev/null || fail "Mac-like's bar is not along the top"
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now classic')
	python3 ci/qmp.py click $((left + cell / 2)) "$classic"
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now classic' "$restarts" ||
		fail "a click on Classic did not bring Classic back"
	# Reset (M5.6b), at the end of the Preset heading, left of Copy as
	# command, about 32 px above where the accent border was found:
	# Mac-like again, then Reset takes the key out of ci's file, and the
	# desktop goes back to Classic.
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now mac-like')
	python3 ci/qmp.py click "$mac" "$classic"
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now mac-like' "$restarts" ||
		fail "a second click on Mac-like did not switch the desktop to Mac-like"
	sleep 1
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the preset is now classic')
	python3 ci/qmp.py click $((left + inner - 50)) $((top - 32))
	wait_more 'edel-compositor: restarting edel-shell-ui: the preset is now classic' "$restarts" ||
		fail "a click on Reset at $((left + inner - 50)),$((top - 32)) did not bring Classic back"
	guest 'settings file'
	sleep 1
	value settings_file | grep -q 'preset' &&
		fail "Reset left layout.preset in ci's file: $(value settings_file)"
	# Its memory while open, against its budget.
	guest 'settings memory'
	wait_for 'DESKTOP-TEST: settings_own_mib ' || fail "the service did not read Settings' memory"
	settings_own=$(value settings_own_mib)
	settings_limit=$(budget settings_own_mib)
	awk -v m="$settings_own" -v b="$settings_limit" 'BEGIN { exit !(m != "" && (b == "" || m <= b)) }' ||
		fail "Settings keeps ${settings_own:-?} MiB of its own while open ($(value settings_rss_mib) MiB resident), over its budget of $settings_limit MiB"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Settings (M5.6a): $settings_own MiB of its own while open (budget ${settings_limit:-none yet}), $(value settings_rss_mib) MiB resident." >>"$GITHUB_STEP_SUMMARY"
	fi
	# Narrow (M5.6b): tiled alone on a screen 512 px wide (scale 2.5),
	# Settings folds its sidebar away and shows the pages' list across
	# the window, in the sidebar's colour where the page was.
	tiled=$(count 'edel-compositor: windows now tiling')
	guest 'tiling on'
	wait_more 'edel-compositor: windows now tiling' "$tiled" || fail "layout.tiling = true did not tile Settings"
	scaled=$(count 'edel-compositor: output Virtual-1 scale 2.5')
	guest 'scale 2.5'
	wait_more 'edel-compositor: output Virtual-1 scale 2.5' "$scaled" || fail "displays.Virtual-1.scale = 2.5 was not followed"
	sleep 3
	narrow=$(value windows | grep -o 'Settings@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | head -n 1)
	[ -n "$narrow" ] || fail "Settings is not in the windows line: $(value windows)"
	set -- $(echo "$narrow" | sed 's/Settings@//; s/[,x]/ /g')
	# A point low in the part of the window on screen, below the pages'
	# list: Settings keeps at least 360 by 300, so it may reach past the
	# screen's 512 by 320.
	right=$(($1 + $3)) bottom=$(($2 + $4))
	[ "$right" -le 512 ] || right=512
	[ "$bottom" -le 320 ] || bottom=320
	nx=$((($1 + (right - $1) / 2) * 5 / 2)) ny=$((($2 + (bottom - $2) * 3 / 4) * 5 / 2))
	shot settings-narrow "$nx" "$ny" "$side" >/dev/null ||
		fail "Settings at $narrow on the 512 px screen did not fold its sidebar: $nx,$ny is not the sidebar's #$side"
	scaled=$(count 'edel-compositor: output Virtual-1 scale 1$')
	guest 'scale default'
	wait_more 'edel-compositor: output Virtual-1 scale 1$' "$scaled" || fail "unsetting the scale did not bring scale 1 back"
	floated=$(count 'edel-compositor: windows now floating')
	guest 'tiling off'
	wait_more 'edel-compositor: windows now floating' "$floated" || fail "layout.tiling = false did not float the windows again"
	closed=$(count 'edel-compositor: unmapped window Settings')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	echo "PASS: Settings opened at $x,$y ${w}x$h under our title bar with its sidebar and page in the token colours, Classic's card chosen, Mac-like's card wrote layout.preset and moved the bar to the top at once, Classic and Reset took the key out again, it held $settings_own MiB of its own ($(value settings_rss_mib) MiB resident), and on a 512 px screen it folded its sidebar away"
}

# sink_volume: the volume wpctl status shows on the sink in use, such as
# 0.30, asked of the test service, which runs it as ci.
sink_volume() {
	asked=$(count 'DESKTOP-TEST: sound_sinks ')
	guest 'sound status'
	wait_more 'DESKTOP-TEST: sound_sinks ' "$asked" 10 || return 1
	value sound_sinks | tr ';' '\n' | grep '^\*' | sed -n 's/.*vol: \([0-9.]*\).*/\1/p' | head -n 1
}

case_sound() {
	# Sound (M5.7b): the compositor started the sound system from the
	# preset's session list, WirePlumber found the VM's HDA card and made
	# its sink the one in use; Settings, opened on its Sound page, gives
	# the keyboard to the volume slider, and Home then six presses of
	# Right (a step is 5 percent) set the sink to 0.30, which wpctl status
	# shows. The volume is the sound system's own state, so nothing is
	# read from a settings file. Kept as sound.png.
	sink_volume >/dev/null || :
	for program in pipewire wireplumber; do
		case " $(value sound_started) " in
		*" $program "*) ;;
		*) fail "the compositor did not start $program from the session's list; it started: $(value sound_started); it said: $(value sound_said)" ;;
		esac
	done
	[ "$(value sound_pulse)" = yes ] ||
		fail "PipeWire serves no PulseAudio socket for ci: $(value sound_pulse)"
	i=0
	while :; do
		now=$(sink_volume)
		[ -n "$now" ] && break
		i=$((i + 1))
		[ "$i" -lt 20 ] || fail "wpctl status lists no sink in use after 60 s; the sound card did not come up: $(value sound_status)"
		sleep 3
	done
	opened=$(count 'edel-compositor: mapped window Settings')
	guest 'sound window'
	wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open on its Sound page: $(value windows)"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
	set -- $place
	x=$1 y=$2 w=$3 h=$4
	sleep 3
	shot sound $((x + w - 40)) $((y + h - 40)) "$(token window)" >/dev/null ||
		fail "the Sound page at $((x + w - 40)),$((y + h - 40)) is not the window token's #$(token window)"
	python3 ci/qmp.py key home
	sleep 1
	python3 ci/qmp.py key right right right right right right
	i=0
	while [ "$(sink_volume)" != 0.30 ]; do
		i=$((i + 1))
		[ "$i" -lt 10 ] || fail "after Home and six presses of Right on the Sound page, wpctl status shows the sink at $(sink_volume), not 0.30: $(value sound_status)"
		sleep 1
	done
	shot sound $((x + w - 40)) $((y + h - 40)) "$(token window)" >/dev/null || fail "Settings left the Sound page"
	guest 'sound memory'
	wait_for 'DESKTOP-TEST: sound_rss_kib ' || fail "the service did not read the sound system's memory"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Sound (M5.7b): resident memory of the sound system, KiB: $(value sound_rss_kib)." >>"$GITHUB_STEP_SUMMARY"
	fi
	closed=$(count 'edel-compositor: unmapped window Settings')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	echo "PASS: the compositor started pipewire and wireplumber, PipeWire served the PulseAudio socket itself, Settings opened on its Sound page with the slider focused, and Home with six presses of Right set the sink to 0.30 as wpctl status shows (sound system resident, KiB: $(value sound_rss_kib))"
}

case_network() {
	# Network and Bluetooth (M5.8a). NetworkManager, started as a service,
	# gives the VM's virtio network an address through its own DHCP client
	# (busybox's networking is not on the desktop), so nmcli, run as ci,
	# reports `connected`. With no polkit, NetworkManager authorizes what
	# the D-Bus policy lets reach it: `nmcli radio wifi` changes
	# something and works for ci, in group seat, and is refused for
	# nobody. QEMU has no Bluetooth adapter: bluetoothd runs and
	# bluetoothctl list exits 0 with nothing listed. Settings opened on
	# each page draws it (the window token's colour in its corner, kept as
	# network.png and bluetooth.png) and says what its headline is on its
	# stderr, which the service reads back.
	i=0
	while :; do
		asked=$(count 'DESKTOP-TEST: network_status ')
		guest 'network status'
		wait_more 'DESKTOP-TEST: network_status ' "$asked" 15 || fail "the service did not run nmcli"
		case "$(value network_status)" in
		connected:*) break ;;
		esac
		i=$((i + 1))
		[ "$i" -lt 20 ] || fail "nmcli -t general status does not report connected after 60 s: \"$(value network_status)\"; devices: $(value network_devices); daemons: $(value network_daemons)"
		sleep 3
	done
	asked=$(count 'DESKTOP-TEST: network_auth ')
	guest 'network auth'
	wait_more 'DESKTOP-TEST: network_auth ' "$asked" 30 || fail "the service did not try nmcli radio wifi"
	case "$(value network_auth)" in
	"ci=0 nobody="[1-9]*) ;;
	*) fail "ci should change the network and nobody should be refused: $(value network_auth)" ;;
	esac
	i=0
	while :; do
		asked=$(count 'DESKTOP-TEST: bluetooth_list ')
		guest 'bluetooth status'
		wait_more 'DESKTOP-TEST: bluetooth_list ' "$asked" 30 || fail "the service did not run bluetoothctl"
		case "$(value bluetooth_list)" in
		"0: "*) break ;;
		esac
		i=$((i + 1))
		[ "$i" -lt 6 ] || fail "bluetoothctl list does not exit 0: \"$(value bluetooth_list)\"; $(value bluetooth_daemon)"
		sleep 5
	done
	for page in network bluetooth; do
		opened=$(count 'edel-compositor: mapped window Settings')
		guest "$page window"
		wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open on its $page page: $(value windows)"
		place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
		set -- $place
		x=$1 y=$2 w=$3 h=$4
		sleep 3
		shot "$page" $((x + w - 40)) $((y + h - 40)) "$(token window)" >/dev/null ||
			fail "the $page page at $((x + w - 40)),$((y + h - 40)) is not the window token's #$(token window)"
		i=0
		while :; do
			asked=$(count "DESKTOP-TEST: ${page}_log ")
			guest "$page log"
			wait_more "DESKTOP-TEST: ${page}_log " "$asked" || fail "the service did not read Settings' log"
			value "${page}_log" | grep -q "edel-settings: $page page shows" && break
			i=$((i + 1))
			[ "$i" -lt 10 ] || fail "the $page page did not say what it shows within 10 tries: $(value "${page}_log")"
			sleep 2
		done
		shows=$(value "${page}_log" | tr ';' '\n' | grep "edel-settings: $page page shows" | head -n 1)
		case "$page:$shows" in
		'network:edel-settings: network page shows "Connected"') ;;
		'bluetooth:edel-settings: bluetooth page shows "This computer has no Bluetooth"') ;;
		*) fail "the $page page shows something else than expected: $shows" ;;
		esac
		closed=$(count 'edel-compositor: unmapped window Settings')
		python3 ci/qmp.py key meta_l-q
		wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	done
	guest 'network memory'
	wait_for 'DESKTOP-TEST: network_rss_kib ' || fail "the service did not read the daemons' memory"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Network and Bluetooth (M5.8a): resident (VmRSS) and proportional (Pss) memory of the daemons, KiB: $(value network_rss_kib)." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: nmcli -t general status reports connected ($(value network_status)), ci may change the network and nobody may not, bluetoothctl list exits 0 with no adapter in the VM, and Settings drew its Network and Bluetooth pages (daemons, KiB: $(value network_rss_kib))"
}

case_power() {
	# Power and Users (M5.8b). upowerd, started as a service on the system
	# bus, answers `upower -e` for ci with exit 0 (the VM has no battery,
	# so it lists the display device and perhaps a power cable). Settings
	# opened on each page draws it (the window token's colour in its
	# corner, kept as power.png and users.png) and says what its headline
	# is on its stderr, which the service reads back: a machine with no
	# battery is "Plugged in", and the Users page leads with ci. Then the
	# Compact rule (crates/settings/CLAUDE.md): maximized on an empty
	# workspace of a screen made 366 logical px wide by scale 3.5, each
	# page is drawn again
	# (power-compact.png, users-compact.png); GTK sizes a window to its
	# content's minimum, so a page too wide would leave the window wider
	# than the screen, and the page's margins on both sides must be the
	# window token's colour, which they are only with the sidebar folded
	# away and nothing cut off.
	i=0
	while :; do
		asked=$(count 'DESKTOP-TEST: power_list ')
		guest 'power status'
		wait_more 'DESKTOP-TEST: power_list ' "$asked" 15 || fail "the service did not run upower"
		case "$(value power_list)" in
		"0: /org/freedesktop/UPower/devices/"*) break ;;
		esac
		i=$((i + 1))
		[ "$i" -lt 10 ] || fail "upower -e does not exit 0 with a device within 30 s: \"$(value power_list)\"; $(value power_daemon)"
		sleep 3
	done
	for want in 'power:Plugged in' 'users:ci'; do
		page=${want%%:*}
		expect=${want#*:}
		opened=$(count 'edel-compositor: mapped window Settings')
		guest "$page window"
		wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open on its $page page: $(value windows)"
		place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
		set -- $place
		x=$1 y=$2 w=$3 h=$4
		sleep 3
		shot "$page" $((x + w - 40)) $((y + h - 40)) "$(token window)" >/dev/null ||
			fail "the $page page at $((x + w - 40)),$((y + h - 40)) is not the window token's #$(token window)"
		i=0
		while :; do
			asked=$(count "DESKTOP-TEST: ${page}_log ")
			guest "$page log"
			wait_more "DESKTOP-TEST: ${page}_log " "$asked" || fail "the service did not read Settings' log"
			value "${page}_log" | grep -q "edel-settings: $page page shows" && break
			i=$((i + 1))
			[ "$i" -lt 10 ] || fail "the $page page did not say what it shows within 10 tries: $(value "${page}_log")"
			sleep 2
		done
		shows=$(value "${page}_log" | tr ';' '\n' | grep "edel-settings: $page page shows" | head -n 1)
		[ "$shows" = "edel-settings: $page page shows \"$expect\"" ] ||
			fail "the $page page shows something else than \"$expect\": $shows"
		closed=$(count 'edel-compositor: unmapped window Settings')
		python3 ci/qmp.py key meta_l-q
		wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	done
	guest 'power memory'
	wait_for 'DESKTOP-TEST: power_rss_kib ' || fail "the service did not read upowerd's memory"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Power (M5.8b): resident (VmRSS) and proportional (Pss) memory of upowerd, KiB: $(value power_rss_kib)." >>"$GITHUB_STEP_SUMMARY"
	fi
	# Compact: 366 logical px wide, Settings maximized on an empty
	# workspace, as a phone shows an app (tiled beside another window it
	# would get a tile narrower than its own minimum).
	python3 ci/qmp.py key meta_l-4
	i=0
	while [ "$(value windows | cut -d" " -f1)" != 0 ]; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "Super+4 did not show an empty workspace: $(value windows)"
		sleep 0.2
	done
	scaled=$(count 'edel-compositor: output Virtual-1 scale 3.5')
	guest 'scale 3.5'
	wait_more 'edel-compositor: output Virtual-1 scale 3.5' "$scaled" || fail "displays.Virtual-1.scale = 3.5 was not followed"
	for page in power users; do
		opened=$(count 'edel-compositor: mapped window Settings')
		guest "$page window"
		wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open on its $page page at 366 px: $(value windows)"
		sleep 2
		python3 ci/qmp.py key meta_l-m
		sleep 3
		compact=$(value windows | grep -o 'Settings@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | head -n 1)
		[ -n "$compact" ] || fail "Settings is not in the windows line: $(value windows)"
		set -- $(echo "$compact" | sed 's/Settings@//; s/[,x]/ /g')
		x=$1 y=$2 w=$3 h=$4
		# The screen is 366 logical px wide at scale 3.5; a page that
		# does not fit makes GTK size the window to the page's minimum.
		[ "$((x + w))" -le 366 ] || fail "the $page page does not fit at Compact width: Settings reaches $((x + w)) px on a 366 px screen ($compact)"
		# Margins left and right of the page, and the page between them: the
		# window token. Physical pixels are logical ones times 3.5.
		mid=$(((y + h / 2) * 7 / 2))
		shot "$page-compact" $(((x + 8) * 7 / 2)) "$mid" "$(token window)" >/dev/null ||
			fail "the $page page at Compact width has no page margin at its left edge (the sidebar did not fold away?)"
		shot "$page-compact" $(((x + w - 8) * 7 / 2)) "$mid" "$(token window)" >/dev/null ||
			fail "the $page page at Compact width is cut off at its right edge"
		closed=$(count 'edel-compositor: unmapped window Settings')
		python3 ci/qmp.py key meta_l-q
		wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	done
	scaled=$(count 'edel-compositor: output Virtual-1 scale 1$')
	guest 'scale default'
	wait_more 'edel-compositor: output Virtual-1 scale 1$' "$scaled" || fail "unsetting the scale did not bring scale 1 back"
	python3 ci/qmp.py key meta_l-1
	echo "PASS: upower -e exits 0 with upowerd running ($(value power_list)), Settings drew its Power page (\"Plugged in\") and its Users page (\"ci\"), and at Compact width, 366 px, maximized, both fit with the sidebar folded away (upowerd, KiB: $(value power_rss_kib))"
}

case_display() {
	# Displays (M5.7a): Settings opened on its Displays page lists each
	# screen from the compositor's state file; the first screen's Scale
	# row, 100% chosen in the accent, is where its rows start (a card of
	# four rows under the arrangement picture, in crates/settings). A
	# click on 200% writes displays.Virtual-1.scale to ci's settings file
	# and the compositor logs the new scale; the screen is then 640x400
	# logical pixels, the window keeps its size and is pulled to the top
	# left, and the Reset the row shows once the value is the person's own
	# takes the line out again. Kept as display.png and display-scale2.png.
	opened=$(count 'edel-compositor: mapped window Settings')
	guest 'display window'
	wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open a window on its Displays page: $(value windows)"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
	set -- $place
	x=$1 y=$2 w=$3
	# The page's card as in the settings case: at most 704 px wide with
	# 32 px margins, centred beside the 204 px sidebar.
	area=$((w - 204))
	clamp=$((area < 704 ? area : 704))
	left=$((x + 204 + (area - clamp) / 2 + 32))
	right=$((left + clamp - 64))
	# The first screen's Scale row, 314 px down the page; its 200% button
	# 45 px in from the card's right edge, the chosen 100% button's left
	# end 285 px in, and Reset 354 px in.
	row=$((y + 314))
	sleep 2
	shot display $((right - 285)) "$row" "$(token accent)" >/dev/null ||
		fail "the Displays page does not show Virtual-1's 100% scale chosen: $((right - 285)),$row is not the accent #$(token accent)"
	scaled=$(count 'edel-compositor: output Virtual-1 scale 2$')
	python3 ci/qmp.py click $((right - 45)) "$row"
	wait_more 'edel-compositor: output Virtual-1 scale 2$' "$scaled" ||
		fail "a click on Scale 200% at $((right - 45)),$row did not give Virtual-1 scale 2"
	filed=$(count 'DESKTOP-TEST: settings_file ')
	guest 'settings file'
	wait_more 'DESKTOP-TEST: settings_file ' "$filed" || fail "the service did not read ci's settings file"
	value settings_file | grep -q '\[displays.Virtual-1\];scale = 2' ||
		fail "ci's settings file is not what edel settings set displays.Virtual-1.scale=2 writes: $(value settings_file)"
	# Reset, where the window now lies on the 640x400 logical screen.
	sleep 2
	now=$(value windows | grep -o 'Settings@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | head -n 1)
	[ -n "$now" ] || fail "Settings is not in the windows line: $(value windows)"
	set -- $(echo "$now" | sed 's/Settings@//; s/[,x]/ /g')
	rx=$((($1 + right - 354 - x) * 2)) ry=$((($2 + 314) * 2))
	python3 ci/qmp.py screendump "$dir/display-scale2.png"
	scaled=$(count 'edel-compositor: output Virtual-1 scale 1$')
	python3 ci/qmp.py click "$rx" "$ry"
	wait_more 'edel-compositor: output Virtual-1 scale 1$' "$scaled" ||
		fail "a click on Reset at $rx,$ry (Settings at $now) did not bring scale 1 back"
	filed=$(count 'DESKTOP-TEST: settings_file ')
	guest 'settings file'
	wait_more 'DESKTOP-TEST: settings_file ' "$filed" || fail "the service did not read ci's settings file"
	value settings_file | grep -q 'scale' &&
		fail "Reset left displays.Virtual-1.scale in ci's file: $(value settings_file)"
	closed=$(count 'edel-compositor: unmapped window Settings')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	echo "PASS: Settings opened on its Displays page showed Virtual-1's 100% scale chosen, a click on 200% logged output Virtual-1 scale 2 and wrote displays.Virtual-1.scale to ci's file, and Reset logged scale 1 and took the line out"
}

case_updates() {
	# Updates (M5.8c): updates.channel, the channel `edel update` follows,
	# is refused when it is not a channel name; set through the command
	# (as root, the machine's file, as every service command) it reads
	# back, and `edel update --check` with no release named says which
	# channel's list it takes, before it reaches the network, so it works
	# with or without one. Settings opened on its Updates page runs `edel
	# status` and says so on its stderr, which the service reads back;
	# the page is the window token's colour in its corner, kept as
	# updates.png; the card at its top (M5.8d), the headline and the big
	# buttons, is the card token's colour. The About page opened the same
	# way is kept as about.png, its window and its card at the top
	# checked alike. Reset brings the image's own channel back (CI's images
	# are built for their own, not preview).
	guest 'channel bad'
	wait_for 'DESKTOP-TEST: channel_bad ' || fail "the service did not report the refused channel"
	value channel_bad | grep -q 'is not a channel name' ||
		fail "edel settings set updates.channel=Beta was not refused with its message: $(value channel_bad)"
	guest 'channel set'
	asked=$(count 'DESKTOP-TEST: channel_value ')
	guest 'channel get'
	wait_more 'DESKTOP-TEST: channel_value ' "$asked" || fail "the service did not read updates.channel"
	value channel_value | grep -q '"preview"' ||
		fail "edel settings get updates.channel does not read back preview: $(value channel_value)"
	asked=$(count 'DESKTOP-TEST: channel_update ')
	guest 'channel update'
	wait_more 'DESKTOP-TEST: channel_update ' "$asked" 40 || fail "edel update --check did not finish in 30 s"
	value channel_update | grep -q 'no release named; taking https://[^ ]*/preview/release.toml' ||
		fail "edel update --check with no release does not take the preview channel's list: $(value channel_update)"
	opened=$(count 'edel-compositor: mapped window Settings')
	guest 'updates window'
	wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open a window on its Updates page: $(value windows)"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
	set -- $place
	x=$1 y=$2 w=$3 h=$4
	sleep 3
	shot updates $((x + w - 40)) $((y + h - 40)) "$(token window)" >/dev/null ||
		fail "the Updates page at $((x + w - 40)),$((y + h - 40)) is not the window token's #$(token window)"
	# The card at the top (crates/settings, M5.8d): the page's cards are
	# at most 704 px wide with 32 px margins, centred beside the 204 px
	# sidebar; 80 px down is inside the card, above its words.
	area=$((w - 204))
	clamp=$((area < 704 ? area : 704))
	card_x=$((x + 204 + (area - clamp) / 2 + 32 + clamp - 64 - 40))
	shot updates $card_x $((y + 80)) "$(token card)" >/dev/null ||
		fail "the Updates page has no card at the top at $card_x,$((y + 80)) in the card token's #$(token card)"
	i=0
	while :; do
		asked=$(count 'DESKTOP-TEST: updates_log ')
		guest 'updates log'
		wait_more 'DESKTOP-TEST: updates_log ' "$asked" || fail "the service did not read Settings' log"
		value updates_log | grep -q 'edel-settings: updates page read edel status, exit' && break
		i=$((i + 1))
		[ "$i" -lt 10 ] || fail "the Updates page did not run edel status within 10 tries: $(value updates_log)"
		sleep 2
	done
	status_line=$(value updates_log | tr ';' '\n' | grep 'edel-settings: updates page read edel status' | head -n 1)
	closed=$(count 'edel-compositor: unmapped window Settings')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings"
	# The About page (M5.8d): the window and its card at the top.
	opened=$(count 'edel-compositor: mapped window Settings')
	guest 'about window'
	wait_more 'edel-compositor: mapped window Settings' "$opened" 60 || fail "Settings did not open a window on its About page: $(value windows)"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*mapped window Settings at \([0-9]*\),\([0-9]*\) \([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p' | tail -n 1)
	set -- $place
	x=$1 y=$2 w=$3 h=$4
	sleep 3
	shot about $((x + w - 40)) $((y + h - 40)) "$(token window)" >/dev/null ||
		fail "the About page at $((x + w - 40)),$((y + h - 40)) is not the window token's #$(token window)"
	area=$((w - 204))
	clamp=$((area < 704 ? area : 704))
	card_x=$((x + 204 + (area - clamp) / 2 + 32 + clamp - 64 - 40))
	shot about $card_x $((y + 80)) "$(token card)" >/dev/null ||
		fail "the About page has no card at the top at $card_x,$((y + 80)) in the card token's #$(token card)"
	closed=$(count 'edel-compositor: unmapped window Settings')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window Settings' "$closed" || fail "Super+Q did not close Settings after the About page"
	guest 'channel reset'
	asked=$(count 'DESKTOP-TEST: channel_value ')
	guest 'channel get'
	wait_more 'DESKTOP-TEST: channel_value ' "$asked" || fail "the service did not read updates.channel after the reset"
	value channel_value | grep -q 'not set' ||
		fail "edel settings reset updates.channel left it set: $(value channel_value)"
	asked=$(count 'DESKTOP-TEST: channel_update ')
	guest 'channel update'
	wait_more 'DESKTOP-TEST: channel_update ' "$asked" 40 || fail "edel update --check did not finish in 30 s after the reset"
	value channel_update | grep -q 'no release named; taking https://[^ ]*/release.toml' ||
		fail "edel update --check with no release names no channel's list after the reset: $(value channel_update)"
	value channel_update | grep -q '/preview/' &&
		fail "edel update --check still takes the preview channel after the reset: $(value channel_update)"
	echo "PASS: updates.channel refused Beta, read back preview and made edel update --check take the preview channel's list, Settings opened on its Updates page with its card at the top and said it ran edel status ($status_line), the About page opened with its card too, and Reset gave the image's own channel back"
}

case_keyboard() {
	# Keyboard layouts (M5.21): region.keyboard = "de,us" loads German
	# then US at once; in foot the key QEMU calls y then types z, as on a
	# German keyboard, so typing "touch y" makes a file named z; Super+Space
	# goes to the next layout; a layout xkeyboard-config lacks is refused
	# with a message that names its list; unset, the layout is US again.
	guest 'keyboard bad'
	wait_for 'DESKTOP-TEST: keyboard_bad ' || fail "the service did not report the refused layout"
	value keyboard_bad | grep -q 'is not a keyboard layout this machine knows' ||
		fail "edel settings set region.keyboard=xx was not refused with its message: $(value keyboard_bad)"
	guest 'keyboard de'
	wait_for 'edel-compositor: keyboard layouts de, us' || fail "the compositor did not load the layouts de and us"
	# The layout indicator (M5.9f): with two layouts the panel shows the one
	# in use, DE, and a click on it goes to the next, English. shell-ui says
	# where it lies (`panel places`); the click is at its middle. Super+Space
	# then goes back to German, as the rest of the case expects.
	wait_for 'edel-shell-ui: panel places .*keyboard [0-9]+\+[1-9][0-9]*' 5 ||
		fail "the panel shows no keyboard indicator with two layouts: $(tr -d '\r' <"$log" | grep -a 'panel places' | tail -n 1)"
	kbd=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 | sed -n 's/.*keyboard \([0-9]*\)+\([0-9]*\).*/\1 \2/p')
	set -- $kbd
	at=$(($1 + $2 / 2))
	switched=$(count 'edel-compositor: keyboard layout now English')
	python3 ci/qmp.py click "$at" 780
	wait_more 'edel-compositor: keyboard layout now English' "$switched" ||
		fail "a click on the keyboard indicator at $at,780 did not go to English"
	switched=$(count 'edel-compositor: keyboard layout now German')
	python3 ci/qmp.py key meta_l-spc
	wait_more 'edel-compositor: keyboard layout now German' "$switched" ||
		fail "Super+Space did not go back to German after the click"
	opened=$(count 'edel-compositor: mapped window foot')
	python3 ci/qmp.py key ctrl-alt-t
	wait_more 'edel-compositor: mapped window foot' "$opened" || fail "Ctrl+Alt+T opened no terminal: $(value windows)"
	sleep 2
	python3 ci/qmp.py type 'cd'
	python3 ci/qmp.py key ret
	python3 ci/qmp.py type 'touch y'
	python3 ci/qmp.py key ret
	sleep 1
	guest 'key file'
	wait_for 'DESKTOP-TEST: keyfile ' || fail "the service did not look for the file"
	[ "$(value keyfile)" = z ] || fail "with the German layout, typing y in foot made $(value keyfile), not z"
	switched=$(count 'edel-compositor: keyboard layout now English')
	python3 ci/qmp.py key meta_l-spc
	wait_more 'edel-compositor: keyboard layout now English' "$switched" ||
		fail "Super+Space did not go to the next layout, English (US)"
	closed=$(count 'edel-compositor: unmapped window foot')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window foot' "$closed" || fail "Super+Q did not close foot"
	guest 'keyboard default'
	wait_for "edel-compositor: keyboard layout us, xkb's default" || fail "unsetting region.keyboard did not bring the US layout back"
	echo "PASS: region.keyboard = \"de,us\" loaded at once, the panel showed the keyboard indicator and a click on it went to English (US) and Super+Space back to German, y typed z in foot, Super+Space went to English (US), xx was refused with its message, and unsetting it brought US back"
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
	# Super+2 shows it tiled, as layout.tiling = true from the tiling case
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

# search_line NAME WANT: takes screenshots into $dir/NAME.png, one a
# second for up to 10 s, until the open launcher's search line, from
# 160 to 340 across its middle 20 rows, is WANT, uniform (empty there)
# or varied (words reach it); prints the last answer and fails if never.
search_line() {
	i=0
	while :; do
		python3 ci/qmp.py screendump "$dir/$1.png"
		seen=$(python3 ci/qmp.py uniform "$dir/$1.png" 160 418 180 20)
		case "$seen" in "$2"*) break ;; esac
		i=$((i + 1))
		[ "$i" -lt 10 ] || break
		sleep 1
	done
	echo "$seen"
	case "$seen" in "$2"*) true ;; *) false ;; esac
}

case_language() {
	# Translations (M5.24a): region.language = "xx", a made-up language
	# the test feature ships a catalogue for, restarts shell-ui, which
	# reads its words in it; the launcher's "Type to search", 14 letters
	# from 28,428, becomes a line long enough to reach past 160, where
	# English leaves the search line empty. Unset, English is back. Kept
	# as language-xx.png and language-en.png.
	for language in en xx en; do
		if [ "$language" = xx ]; then
			restarts=$(count 'edel-compositor: restarting edel-shell-ui: the language is now xx')
			guest 'language xx'
			wait_more 'edel-compositor: restarting edel-shell-ui: the language is now xx' "$restarts" ||
				fail "setting region.language=xx did not restart shell-ui"
			wait_for 'edel-shell-ui: words in xx, 1 translated' || fail "shell-ui did not read xx's one word"
			want=varied
		elif [ "$(count 'edel-shell-ui: words in xx')" -gt 0 ]; then
			restarts=$(count 'edel-compositor: restarting edel-shell-ui: the language is now English')
			guest 'language default'
			wait_more 'edel-compositor: restarting edel-shell-ui: the language is now English' "$restarts" ||
				fail "unsetting region.language did not restart shell-ui"
			want=uniform
		else
			want=uniform
		fi
		sleep 2
		shown=$(count 'edel-shell-ui: launcher shown')
		python3 ci/qmp.py key meta_l
		wait_more 'edel-shell-ui: launcher shown' "$shown" || fail "Super did not open the launcher in $language"
		seen=$(search_line "language-$language" "$want") ||
			fail "in $language the launcher's search line from 160 to 340 is $seen, not $want"
		hidden=$(count 'edel-shell-ui: launcher hidden')
		python3 ci/qmp.py key esc
		wait_more 'edel-shell-ui: launcher hidden' "$hidden" || fail "Escape did not close the launcher in $language"
	done
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Translations (M5.24a): language-xx.png and language-en.png are in the edel-desktop-test artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: region.language = \"xx\" restarted shell-ui, which read xx's one word, and the launcher drew it past 160, where English leaves the search line empty; unset, English came back"
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

case_quick() {
	# Quick settings (M5.9a). The status area is a pill in the panel,
	# shell-ui says where (`panel places`); a click on it opens the card
	# above it, 8 px from the screen's side and the panel, in the panel's
	# colour: 1000,745 lies in its bottom padding and is the background
	# before. The VM has a network but no Wi-Fi adapter and no Bluetooth, so
	# its card holds the Do not disturb and Dark style tiles, the volume and
	# Settings; shell-ui
	# logs where each lies (`quick places`), in logical pixels from the
	# card's corner, which the layers line (the card and its shadow's room)
	# places on screen. Kept as quick.png.
	background=$(token background)
	panel=$(token panel)
	fill=$(token window)
	pill() {
		tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 |
			sed -n 's/.*status \([0-9]*\)+\([0-9]*\).*/\1 \2/p'
	}
	# The sound card comes up a little after the desktop.
	i=0
	while [ -z "$(sink_volume)" ]; do
		i=$((i + 1))
		[ "$i" -lt 20 ] || fail "wpctl status lists no sink in use after 60 s: $(value sound_status)"
		sleep 3
	done
	guest 'volume 20'
	i=0
	while [ "$(sink_volume)" != 0.20 ]; do
		i=$((i + 1))
		[ "$i" -lt 10 ] || fail "the sink is at $(sink_volume), not 0.20, after wpctl set-volume"
		sleep 1
	done
	set -- $(pill)
	[ -n "${1:-}" ] || fail "shell-ui did not say where its status area lies: $(tr -d '\r' <"$log" | grep -a 'panel places' | tail -n 1)"
	pill_x=$(($1 + $2 / 2))
	shot quick 1000 745 "$background" >/dev/null || fail "1000,745 is not the background before quick settings open"
	# A click on the pill opens the card.
	shown=$(count 'edel-shell-ui: quick settings shown')
	python3 ci/qmp.py click "$pill_x" 780
	wait_more 'edel-shell-ui: quick settings shown' "$shown" || fail "a click on the status area at $pill_x,780 did not open quick settings"
	shot quick 1000 745 "$panel" >/dev/null || fail "the card is not drawn at 1000,745 in the panel's colour"
	i=0
	until value layers | grep -q 'edel-quick@'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "the state file lists no quick settings surface: $(value layers)"
		sleep 0.2
	done
	set -- $(value layers | grep -o 'edel-quick@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | sed 's/edel-quick@//; s/[,x]/ /g')
	sx=$1 sy=$2 sw=$3 sh=$4
	line=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: quick places' | tail -n 1)
	set -- $(echo "$line" | sed -n 's/.*quick places card \([0-9]*\)x\([0-9]*\).*/\1 \2/p')
	[ -n "${1:-}" ] || fail "shell-ui did not say where the card's parts lie: $line"
	cx=$((sx + (sw - $1) / 2)) cy=$((sy + (sh - $2) / 2))
	[ "$1" = 352 ] || fail "the card is $1 px wide, not 352"
	set -- $(echo "$line" | sed -n 's/.*track \([0-9]*\)+\([0-9]*\)+\([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p')
	[ -n "${1:-}" ] || fail "the card has no volume track: $line"
	tx=$(($1 + cx)) ty=$(($2 + cy + $4 / 2)) tw=$3
	# 20 percent: the fill (the tokens' window colour, white on the light
	# scheme) three twentieths of the way along the track, clear of the
	# speaker at its left end, and the track's own colour near its end.
	shot quick $((tx + tw * 3 / 20)) "$ty" "$fill" >/dev/null || fail "the slider at $((tx + tw * 3 / 20)),$ty is not filled at 20 percent"
	shot quick $((tx + tw - 14)) "$ty" "!$fill" >/dev/null || fail "the slider at $((tx + tw - 14)),$ty is still filled near its end"
	# A drag from three tenths of the track to half of it.
	python3 ci/qmp.py drag $((tx + tw * 3 / 10)) "$ty" $((tx + tw / 2)) "$ty"
	i=0
	until awk -v v="$(sink_volume)" 'BEGIN { exit !(v >= 0.45 && v <= 0.55) }'; do
		i=$((i + 1))
		[ "$i" -lt 15 ] || fail "after dragging the slider to half, wpctl status shows the sink at $(sink_volume), not about 0.50"
		sleep 1
	done
	volume=$(sink_volume)
	shot quick $((tx + tw * 2 / 5)) "$ty" "$fill" >/dev/null || fail "after the drag the slider at $((tx + tw * 2 / 5)),$ty is not filled"
	# A second click on the pill closes the card, and it opens again.
	hidden=$(count 'edel-shell-ui: quick settings hidden')
	python3 ci/qmp.py click "$pill_x" 780
	wait_more 'edel-shell-ui: quick settings hidden' "$hidden" || fail "a second click on the status area did not close quick settings"
	shot quick 1000 745 "$background" >/dev/null || fail "1000,745 is not the background after the card closed"
	shown=$(count 'edel-shell-ui: quick settings shown')
	python3 ci/qmp.py click "$pill_x" 780
	wait_more 'edel-shell-ui: quick settings shown' "$shown" || fail "the status area did not open quick settings again"
	# The Dark style tile: a click on its left part writes appearance.mode
	# to ci's file and the compositor takes the dark colours.
	line=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: quick places' | tail -n 1)
	set -- $(echo "$line" | sed -n 's/.*dark_style \([0-9]*\)+\([0-9]*\)+\([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p')
	[ -n "${1:-}" ] || fail "the card has no Dark style tile: $line"
	dx=$(($1 + cx + 24)) dy=$(($2 + cy + $4 / 2))
	said=$(count 'edel-compositor: colour scheme dark')
	python3 ci/qmp.py click "$dx" "$dy"
	wait_more 'edel-compositor: colour scheme dark' "$said" || fail "a click on Dark style at $dx,$dy did not turn the colour scheme dark"
	guest 'settings file'
	i=0
	until value settings_file | grep -q 'mode = "dark"'; do
		i=$((i + 1))
		[ "$i" -lt 20 ] || fail "ci's settings file does not hold appearance.mode = \"dark\": $(value settings_file)"
		guest 'settings file'
		sleep 1
	done
	# Back to light, as the person's file without the key; shell-ui starts
	# again in each scheme, which closes the card.
	said=$(count 'edel-compositor: colour scheme light')
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the colour scheme is now light')
	guest 'scheme mine default'
	wait_more 'edel-compositor: colour scheme light' "$said" || fail "taking appearance.mode out of ci's file did not bring the light scheme back"
	wait_more 'edel-compositor: restarting edel-shell-ui: the colour scheme is now light' "$restarts" || fail "shell-ui did not start again for the light scheme"
	shot quick 640 790 "$panel" >/dev/null || fail "the panel at 640,790 is not the light scheme's #$panel"
	sleep 3
	# Escape closes the card.
	set -- $(pill)
	pill_x=$(($1 + $2 / 2))
	shown=$(count 'edel-shell-ui: quick settings shown')
	python3 ci/qmp.py click "$pill_x" 780
	wait_more 'edel-shell-ui: quick settings shown' "$shown" || fail "the status area did not open quick settings after shell-ui started again"
	shot quick 1000 745 "$panel" >/dev/null || fail "the card is not drawn at 1000,745 in the panel's colour"
	hidden=$(count 'edel-shell-ui: quick settings hidden')
	python3 ci/qmp.py key esc
	wait_more 'edel-shell-ui: quick settings hidden' "$hidden" || fail "Escape did not close quick settings"
	shot quick 1000 745 "$background" >/dev/null || fail "1000,745 is not the background after Escape"
	echo "PASS: a click on the status area opened quick settings in the panel's colour, a drag on the volume slider took the sink from 0.20 to $volume as wpctl status shows, a second click on the pill closed the card, a click on Dark style wrote appearance.mode to ci's file and the compositor took the dark colours, and Escape closed the card"
}

case_notify() {
	# Notifications (M5.9b). An app sends a notification as ci on the
	# session bus (gdbus call, the one tool every image has for it), and
	# shell-ui shows a banner at the panel's corner, 8 px from the screen's
	# side and the panel, in the panel's colour: 1200,745 lies in its
	# padding, clear of its words and its cross, and is the background
	# before. A click on the clock opens the notification centre over it,
	# which lists the notification; Escape closes it. With
	# notifications.do_not_disturb set by `edel settings set`, a second
	# notification is listed in the centre, which the log says, and shows no
	# banner; with the key taken out again a third shows one, which goes by
	# itself after its few seconds. The layers lines (the card and its
	# shadow's room) are the regions. Kept as notify.png.
	background=$(token background)
	panel=$(token panel)
	clock() {
		tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 |
			sed -n 's/.*clock \([0-9]*\)+\([0-9]*\).*/\1 \2/p'
	}
	set -- $(clock)
	[ -n "${1:-}" ] || fail "shell-ui did not say where its clock lies: $(tr -d '\r' <"$log" | grep -a 'panel places' | tail -n 1)"
	clock_x=$(($1 + $2 / 2))
	shot notify 1200 745 "$background" >/dev/null || fail "1200,745 is not the background before a notification"
	# The first notification: a banner in the panel's colour.
	shown=$(count 'edel-shell-ui: banner shown')
	guest 'notify one'
	wait_more 'edel-shell-ui: banner shown' "$shown" || fail "a notification sent on the session bus showed no banner: $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui' | tail -n 5)"
	tr -d '\r' <"$log" | grep -aEq 'edel-shell-ui: notification [0-9]+ from ci-test: Hello from CI' ||
		fail "shell-ui did not log the notification it was sent"
	shot notify 1200 745 "$panel" >/dev/null || fail "the banner is not drawn at 1200,745 in the panel's colour"
	i=0
	until value layers | grep -q 'edel-notification@'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "the state file lists no notification surface: $(value layers)"
		sleep 0.2
	done
	set -- $(value layers | grep -o 'edel-notification@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | sed 's/edel-notification@//; s/[,x]/ /g')
	line=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: banner places' | tail -n 1)
	set -- $1 $2 $3 $4 $(echo "$line" | sed -n 's/.*banner places card \([0-9]*\)x\([0-9]*\).*/\1 \2/p')
	[ -n "${5:-}" ] || fail "shell-ui did not say how big the banner is: $line"
	[ "$5" = 360 ] || fail "the banner is $5 px wide, not 360"
	# Its surface is the card and the room for its shadow round it.
	[ "$3" -ge "$5" ] && [ "$4" -ge "$6" ] || fail "the banner's region ${3}x$4 is smaller than its card ${5}x$6: $(value layers)"
	python3 ci/qmp.py screendump "$dir/notify.png"
	# A click on the clock opens the centre over it: the banner gives way.
	shown=$(count 'edel-shell-ui: notification centre shown')
	python3 ci/qmp.py click "$clock_x" 780
	wait_more 'edel-shell-ui: notification centre shown' "$shown" || fail "a click on the clock at $clock_x,780 did not open the notification centre"
	i=0
	until value layers | grep -q 'edel-centre@'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "the state file lists no notification centre: $(value layers)"
		sleep 0.2
	done
	set -- $(value layers | grep -o 'edel-centre@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | sed 's/edel-centre@//; s/[,x]/ /g')
	cw=$3 ch=$4
	shot notify 1200 745 "$panel" >/dev/null || fail "the centre is not drawn at 1200,745 in the panel's colour"
	line=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: centre places' | tail -n 1)
	echo "$line" | grep -q 'centre places card 360x[0-9]*, .*notification0 ' ||
		fail "the centre does not list the notification: $line"
	echo "$line" | grep -q 'clear ' && fail "Clear all shows with one notification in the centre: $line"
	echo "$line" | grep -q 'notification1 ' && fail "the centre lists two notifications after one was sent: $line"
	# Its surface is the card and, on Full and Balanced, the room for its
	# shadow round it; CI runs on Lite, where there is none (M5.5e).
	[ "$cw" -ge 360 ] && [ "$ch" -gt 300 ] || fail "the centre's region is only ${cw}x$ch: $(value layers)"
	python3 ci/qmp.py screendump "$dir/notify-centre.png"
	# Do not disturb: the next notification is listed and shows no banner.
	shown=$(count 'edel-shell-ui: banner shown')
	listed=$(count 'edel-shell-ui: notification [0-9]+ is listed, with no banner while do not disturb is on')
	guest 'dnd on'
	sleep 1
	guest 'notify two'
	wait_more 'edel-shell-ui: notification [0-9]+ is listed, with no banner while do not disturb is on' "$listed" ||
		fail "with do not disturb set, the second notification was not logged as listed without a banner: $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui' | tail -n 5)"
	sleep 2
	[ "$(count 'edel-shell-ui: banner shown')" = "$shown" ] || fail "a banner showed while do not disturb was set"
	value layers | grep -q 'edel-notification@' && fail "a notification surface is listed while do not disturb is set: $(value layers)"
	i=0
	until tr -d '\r' <"$log" | grep -a 'edel-shell-ui: centre places' | tail -n 1 | grep -q 'notification1 '; do
		i=$((i + 1))
		[ "$i" -lt 25 ] || fail "the open centre does not list the second notification: $(tr -d '\r' <"$log" | grep -a 'centre places' | tail -n 1)"
		sleep 0.2
	done
	tr -d '\r' <"$log" | grep -a 'edel-shell-ui: centre places' | tail -n 1 | grep -q 'clear ' ||
		fail "Clear all does not show with two notifications in the centre: $(tr -d '\r' <"$log" | grep -a 'centre places' | tail -n 1)"
	# Escape closes the centre.
	hidden=$(count 'edel-shell-ui: notification centre hidden')
	python3 ci/qmp.py key esc
	wait_more 'edel-shell-ui: notification centre hidden' "$hidden" || fail "Escape did not close the notification centre"
	shot notify 1200 745 "$background" >/dev/null || fail "1200,745 is not the background after the centre closed"
	# The key taken out, banners are back; this one goes by itself.
	guest 'dnd default'
	sleep 1
	shown=$(count 'edel-shell-ui: banner shown')
	hidden=$(count 'edel-shell-ui: banner hidden')
	guest 'notify three'
	wait_more 'edel-shell-ui: banner shown' "$shown" || fail "with do not disturb taken out again, the third notification showed no banner"
	wait_more 'edel-shell-ui: banner hidden' "$hidden" 15 || fail "the banner did not go away by itself after its few seconds"
	shot notify 1200 745 "$background" >/dev/null || fail "1200,745 is not the background after the banner went"
	echo "PASS: a notification sent on the session bus showed a 360 px banner in the panel's colour, a click on the clock opened the notification centre with it listed, with do not disturb set the second one was listed without a banner, and with the key taken out the third showed a banner that went by itself"
}

case_osd() {
	# The volume keys (M5.9c): the compositor keeps the keyboard's volume
	# up key from apps and tells shell-ui, which turns the sink in use up
	# by five percent and shows the pop-up, the slider and its button
	# above the status area, gone 1.5 s after the last key. Kept as
	# osd.png.
	before=$(sink_volume)
	[ -n "$before" ] || fail "wpctl status lists no sink in use: $(value sound_status)"
	want=$(awk -v v="$before" 'BEGIN { w = v + 0.05; if (w > 1) w = 1; printf "%.2f", w }')
	pressed=$(count 'edel-compositor: media key volume_up')
	shown=$(count 'edel-shell-ui: osd shown')
	hidden=$(count 'edel-shell-ui: osd hidden')
	python3 ci/qmp.py key volumeup
	wait_more 'edel-compositor: media key volume_up' "$pressed" || fail "the volume up key did not reach the compositor as a media key"
	wait_more 'edel-shell-ui: osd shown' "$shown" || fail "the volume up key showed no pop-up: $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui' | tail -n 5)"
	i=0
	until value layers | grep -q 'edel-osd@'; do
		i=$((i + 1))
		[ "$i" -lt 25 ] || fail "the state file lists no pop-up surface: $(value layers)"
		sleep 0.2
	done
	python3 ci/qmp.py screendump "$dir/osd.png"
	i=0
	until [ "$(sink_volume)" = "$want" ]; do
		i=$((i + 1))
		[ "$i" -lt 10 ] || fail "after the volume up key the sink is at $(sink_volume), not $want (it was $before)"
		sleep 1
	done
	wait_more 'edel-shell-ui: osd hidden' "$hidden" 10 || fail "the pop-up did not go by itself"
	echo "PASS: the volume up key took the sink from $before to $want as wpctl status shows, and shell-ui showed its pop-up above the status area and took it away by itself"
}

case_player() {
	# Now playing (M5.9d): a test MPRIS player plays "Night Drive" by
	# Lumen on ci's session bus; quick settings, opened from the status
	# area, reads it and shows its card, and a click on the card's play or
	# pause button reaches the player, which prints it (`mpris log`).
	# Kept as player.png.
	guest mpris
	set -- $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 |
		sed -n 's/.*status \([0-9]*\)+\([0-9]*\).*/\1 \2/p')
	[ -n "${1:-}" ] || fail "shell-ui did not say where its status area lies"
	pill_x=$(($1 + $2 / 2))
	sleep 2
	shown=$(count 'edel-shell-ui: quick settings shown')
	heard=$(count 'edel-shell-ui: player: Night Drive by Lumen')
	python3 ci/qmp.py click "$pill_x" 780
	wait_more 'edel-shell-ui: quick settings shown' "$shown" || fail "a click on the status area did not open quick settings"
	wait_more 'edel-shell-ui: player: Night Drive by Lumen' "$heard" ||
		fail "quick settings did not read the test player: $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: player' | tail -n 3)"
	i=0
	until tr -d '\r' <"$log" | grep -a 'edel-shell-ui: quick places' | tail -n 1 | grep -q ' play '; do
		i=$((i + 1))
		[ "$i" -lt 25 ] || fail "quick settings shows no player card: $(tr -d '\r' <"$log" | grep -a 'quick places' | tail -n 1)"
		sleep 0.2
	done
	set -- $(value layers | grep -o 'edel-quick@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | sed 's/edel-quick@//; s/[,x]/ /g')
	sx=$1 sy=$2 sw=$3 sh=$4
	line=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: quick places' | tail -n 1)
	set -- $(echo "$line" | sed -n 's/.*quick places card \([0-9]*\)x\([0-9]*\).*/\1 \2/p')
	cx=$((sx + (sw - $1) / 2)) cy=$((sy + (sh - $2) / 2))
	set -- $(echo "$line" | sed -n 's/.* play \([0-9]*\)+\([0-9]*\)+\([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p')
	[ -n "${1:-}" ] || fail "the player card has no play button: $line"
	python3 ci/qmp.py screendump "$dir/player.png"
	python3 ci/qmp.py click $((cx + $1 + $3 / 2)) $((cy + $2 + $4 / 2))
	i=0
	until guest 'mpris log' && value mpris | grep -q 'play pause'; do
		i=$((i + 1))
		[ "$i" -lt 10 ] || fail "a click on the card's play button at $((cx + $1 + $3 / 2)),$((cy + $2 + $4 / 2)) did not reach the player: $(value mpris)"
		sleep 1
	done
	hidden=$(count 'edel-shell-ui: quick settings hidden')
	python3 ci/qmp.py key esc
	wait_more 'edel-shell-ui: quick settings hidden' "$hidden" || fail "Escape did not close quick settings"
	guest 'mpris off'
	echo "PASS: quick settings read the test player over MPRIS and showed Night Drive by Lumen on its card, and a click on its play button reached the player (mpris $(value mpris))"
}

case_fallback() {
	# The fallback notice (M5.9e): a record that the last update went back
	# (`fallback record`, written as root in the data partition, as
	# edel's update code writes it) is shown by shell-ui when it starts,
	# as a banner and in its list, and its stamp keeps the record's date.
	# After shell-ui starts again it says the notice was already seen and
	# shows no second banner. Kept as fallback.png.
	guest 'fallback record'
	shown=$(count 'edel-shell-ui: fallback notice shown')
	banners=$(count 'edel-shell-ui: banner shown')
	started=$(count 'edel-compositor: started edel-shell-ui')
	guest 'kill panel'
	wait_more 'edel-compositor: started edel-shell-ui' "$started" ||
		fail "the compositor did not start edel-shell-ui again after kill -9"
	wait_more 'edel-shell-ui: fallback notice shown' "$shown" ||
		fail "shell-ui did not show the fallback notice from the record: $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui' | tail -n 5)"
	wait_more 'edel-shell-ui: banner shown' "$banners" ||
		fail "shell-ui showed no banner for the fallback notice"
	python3 ci/qmp.py screendump "$dir/fallback.png"
	already=$(count 'edel-shell-ui: fallback notice already seen')
	banners=$(count 'edel-shell-ui: banner shown')
	started=$(count 'edel-compositor: started edel-shell-ui')
	guest 'kill panel'
	wait_more 'edel-compositor: started edel-shell-ui' "$started" ||
		fail "the compositor did not start edel-shell-ui again after the second kill -9"
	wait_more 'edel-shell-ui: fallback notice already seen' "$already" ||
		fail "shell-ui did not say the fallback notice was already seen: $(tr -d '\r' <"$log" | grep -a 'edel-shell-ui' | tail -n 5)"
	sleep 2
	[ "$(count 'edel-shell-ui: banner shown')" = "$banners" ] || fail "a second banner showed for the same fallback record"
	guest 'fallback clear'
	echo "PASS: shell-ui showed the fallback notice as a banner when it started, and after it started again it said the notice was already seen and showed no second banner"
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
	# Presets (M5.4a): edel settings set layout.preset=hive restarts
	# shell-ui with Hive's bar along the top and tiles the windows, and
	# unsetting it brings Classic back, the panel along the bottom and the
	# windows floating, live; each is kept as preset-NAME.png with the
	# screenshots, for Alimardon to look at.
	panel=$(token panel)
	top=$(count 'edel-shell-ui: panel edel-panel along the top')
	tiled=$(count 'edel-compositor: windows now tiling')
	guest 'preset hive'
	wait_more 'edel-shell-ui: panel edel-panel along the top' "$top" ||
		fail "edel settings set layout.preset=hive did not bring shell-ui's bar to the top"
	tr -d '\r' <"$log" | grep -aq 'edel-compositor: restarting edel-shell-ui: the preset is now hive' ||
		fail "the compositor did not restart shell-ui for the Hive preset"
	wait_more 'edel-compositor: windows now tiling' "$tiled" || fail "the Hive preset did not tile the windows"
	shot preset-hive 640 12 "$panel" >/dev/null || fail "Hive's bar is not at 640,12 in the panel's colour"
	bottom=$(count 'edel-shell-ui: panel edel-panel along the bottom')
	floated=$(count 'edel-compositor: windows now floating')
	guest 'preset default'
	wait_more 'edel-shell-ui: panel edel-panel along the bottom' "$bottom" ||
		fail "unsetting layout.preset did not bring Classic's panel back along the bottom"
	wait_more 'edel-compositor: windows now floating' "$floated" || fail "back on Classic, the windows did not float"
	shot preset-classic 640 790 "$panel" >/dev/null || fail "Classic's panel is not at 640,790 in the panel's colour"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Presets (M5.4a): preset-hive.png and preset-classic.png are in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: edel settings set layout.preset=hive restarted shell-ui with Hive's bar along the top and tiled the windows, and unsetting it brought back Classic's panel along the bottom and floating windows"
}

case_taskbar() {
	# Windows-like (M5.4c): edel settings set layout.preset=windows-like
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
		fail "edel settings set layout.preset=windows-like did not restart shell-ui"
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
		fail "unsetting layout.preset did not bring Classic back"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Windows-like (M5.4c): preset-windows-like.png is in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: Windows-like's taskbar lay along the bottom with the menu button at its left edge, the search field and the apps centred at $x, $w px wide; foot's cell started foot, minimized it and brought it back, and unsetting the preset brought Classic back"
}

case_dock() {
	# Mac-like (M5.4d): edel settings set layout.preset=mac-like restarts
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
		fail "edel settings set layout.preset=mac-like did not restart shell-ui"
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
		fail "unsetting layout.preset did not bring Classic back"
	wait_more 'edel-compositor: window buttons on the right' "$right" || fail "back on Classic, the window buttons did not go back to the right"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Mac-like (M5.4d): preset-mac-like.png is in the edel-images artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: Mac-like put a bar along the top, a dock $((w + 16)) px wide centred at $x,732 above an 8 px gap, and the window buttons on the left; foot's cell in the dock started foot, and unsetting the preset brought Classic back"
}

case_panels() {
	# The panels as a setting (M5.4e): layout.panels with one panel along
	# the bottom holding only the clock restarts shell-ui with it in place
	# of Classic's, so 20,776, inside the menu button's first square, is
	# the panel's colour; unsetting it brings Classic's panel back. Kept
	# as panels-clock.png.
	panel=$(token panel)
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	guest 'panels clock'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "edel settings set layout.panels did not restart shell-ui"
	wait_for 'edel-shell-ui: panel places clock [0-9]+\+[0-9]+$' || fail "shell-ui's panel does not hold the clock alone"
	shot panels-clock 20 776 "$panel" >/dev/null || fail "20,776 is not the panel's colour: the menu button is still there"
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	classic=$(count 'edel-shell-ui: panel places menu 0\+')
	guest 'panels default'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "edel settings reset layout.panels did not restart shell-ui"
	wait_more 'edel-shell-ui: panel places menu 0\+' "$classic" || fail "unsetting layout.panels did not bring Classic's menu button back"
	echo "PASS: layout.panels with only the clock replaced Classic's panel at once, and unsetting it brought Classic's panel back"
}

case_dockhide() {
	# A dock that hides while covered (M5.4f): layout.panels with one dock
	# along the bottom, hide = "covered", holding the apps. Uncovered, it
	# shows; a 1200x740 window over it hides it, so its left end shows
	# the window's colour; the pointer at the screen's bottom edge brings
	# it back, and away from it the dock hides again. Kept as
	# dock-shown.png, dock-hidden.png and dock-back.png.
	panel=$(token panel)
	restarts=$(count 'edel-compositor: restarting edel-shell-ui: the panels changed')
	guest 'dock hiding'
	wait_more 'edel-compositor: restarting edel-shell-ui: the panels changed' "$restarts" ||
		fail "setting a hiding dock in layout.panels did not restart shell-ui"
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
		fail "unsetting layout.panels did not restart shell-ui"
	echo "PASS: a dock with hide = \"covered\" showed while uncovered, hid under big, came back with the pointer at the bottom edge and hid again when it left"
}

case_fullscreen() {
	# Fullscreen (M5.20): a test client that asks for fullscreen before
	# it is shown covers the whole 1280x800 screen, panel too, in its
	# colour with no title bar, and the windows line ends its entry in !;
	# Super+F takes it out, back to its own size with the panel, and in
	# again; Super+Q closes it. Then Super+F does the same for foot.
	# Kept as fullscreen.png and fullscreen-left.png.
	panel=$(token panel)
	opened=$(count 'edel-compositor: window full fullscreen')
	guest 'full window'
	wait_more 'edel-compositor: window full fullscreen' "$opened" || fail "the window full did not go fullscreen"
	wait_for 'DESKTOP-TEST: windows .* full@0,0,1280x800!' || fail "the windows line does not show full at 0,0 1280x800, fullscreen: $(value windows)"
	shot fullscreen 640 790 33aa66 >/dev/null || fail "640,790, where the panel is, is not full's colour"
	shot fullscreen 4 4 33aa66 >/dev/null || fail "the top left corner is not full's colour"
	left=$(count 'edel-compositor: window full not fullscreen')
	python3 ci/qmp.py key meta_l-f
	wait_more 'edel-compositor: window full not fullscreen' "$left" || fail "Super+F did not take full out of fullscreen"
	wait_for 'DESKTOP-TEST: windows .* full@[0-9]+,[0-9]+,300x200( |$)' || fail "full did not go back to 300x200: $(value windows)"
	shot fullscreen-left 640 790 "$panel" >/dev/null || fail "the panel did not come back at 640,790"
	again=$(count 'edel-compositor: window full fullscreen')
	python3 ci/qmp.py key meta_l-f
	wait_more 'edel-compositor: window full fullscreen' "$again" || fail "Super+F did not make full fullscreen again"
	closed=$(count 'edel-compositor: unmapped window full')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window full' "$closed" || fail "Super+Q did not close full"
	shot fullscreen-left 640 790 "$panel" >/dev/null || fail "the panel did not come back after full closed"
	opened=$(count 'edel-compositor: mapped window foot')
	python3 ci/qmp.py key ctrl-alt-t
	wait_more 'edel-compositor: mapped window foot' "$opened" || fail "Ctrl+Alt+T did not open foot"
	went=$(count 'edel-compositor: window foot fullscreen')
	python3 ci/qmp.py key meta_l-f
	wait_more 'edel-compositor: window foot fullscreen' "$went" || fail "Super+F did not make foot fullscreen"
	wait_for 'DESKTOP-TEST: windows .* foot@0,0,1280x800!' || fail "foot does not cover the screen: $(value windows)"
	shot fullscreen 640 790 "!$panel" >/dev/null || fail "the panel still shows over fullscreen foot"
	left=$(count 'edel-compositor: window foot not fullscreen')
	python3 ci/qmp.py key meta_l-f
	wait_more 'edel-compositor: window foot not fullscreen' "$left" || fail "Super+F did not take foot out of fullscreen"
	shot fullscreen-left 640 790 "$panel" >/dev/null || fail "the panel did not come back after foot left fullscreen"
	closed=$(count 'edel-compositor: unmapped window foot')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window foot' "$closed" || fail "Super+Q did not close foot"
	echo "PASS: full asked for fullscreen and covered the screen, panel too; Super+F took it out and back; foot went fullscreen and back with Super+F"
}

case_portal() {
	# The settings portal (M5.5a): an app asking xdg-desktop-portal for
	# the colour scheme and the accent hears shell-ui's answer, from the
	# tokens: prefer light, 2, the release's default (M5.12a), and the
	# light accent as three numbers from 0 to 1, each cut to two places.
	guest 'portal read'
	wait_for 'DESKTOP-TEST: portal accent-color ' 60 || fail "the service did not hear back from the portal"
	scheme=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: portal color-scheme //p' | tail -n 1)
	accent=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: portal accent-color //p' | tail -n 1)
	case "$scheme" in
	"(<uint32 2>,)"*) ;;
	*) fail "the portal's colour scheme is not prefer light: $scheme" ;;
	esac
	hex=$(token accent)
	want=$(echo "$hex" | awk '{
		for (i = 0; i < 3; i++) {
			v = 0
			for (j = 1; j <= 2; j++) v = v * 16 + index("0123456789abcdef", substr($0, 2 * i + j, 1)) - 1
			printf "%s0\\.%02d[0-9]*", (i ? ", " : ""), int(v / 255 * 100)
		}
	}')
	echo "$accent" | grep -qE "^\\(<\\($want\\)>,\\)" ||
		fail "the portal's accent is not the tokens' #$hex: $accent"
	echo "PASS: an app asking xdg-desktop-portal heard shell-ui's answer: colour scheme $scheme and accent $accent"
}

case_scheme() {
	# Light and dark (M5.5c): edel settings set appearance.mode=dark
	# gives the compositor the tokens' [colour], drawing the title bars
	# and the background again, and restarts shell-ui, whose panel and
	# portal follow; the portal tells apps already open with
	# SettingChanged, which xdg-desktop-portal passes on. Unset, it is
	# light again, the release's default (M5.12a). Kept as
	# scheme-dark.png and scheme-light.png.
	guest 'portal watch'
	opened=$(count 'edel-compositor: mapped window keys')
	guest 'shortcut window'
	wait_more 'edel-compositor: mapped window keys' "$opened" || fail "the test client keys did not open"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*edel-compositor: mapped window keys at \([0-9]*\),\([0-9]*\) .*/\1 \2/p' | tail -n 1)
	read -r x y <<-EOF
		$place
	EOF
	for scheme in dark light; do
		if [ "$scheme" = dark ]; then
			table=colour
			command='scheme dark'
		else
			table=colour.light
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
		echo "Light and dark (M5.5c): scheme-dark.png and scheme-light.png are in the edel-desktop-test artifact." >>"$GITHUB_STEP_SUMMARY"
	fi
	echo "PASS: appearance.mode=dark turned the panel, keys' title bar and the background to the dark tokens, the portal said prefer dark (1) and xdg-desktop-portal passed SettingChanged on to apps; unset, all of it went back to light (2), the release's default"
}

case_buttons() {
	# Window buttons on either side (M5.4b): with layout.window_buttons =
	# "left", close is the bar's leftmost 28 px square, then minimize and
	# maximize, and a click there closes the window; unset, they go back
	# to the preset's right. Kept as buttons-left.png.
	focused=$(token title_bar_focused)
	guest 'buttons left'
	wait_for 'edel-compositor: window buttons on the left' ||
		fail "the compositor did not follow layout.window_buttons = \"left\""
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
		fail "unsetting layout.window_buttons did not bring the buttons back to the right"
	# Each button as a setting (M5.18a): layout.minimize_button = false
	# takes minimize off every bar, so its square, the third from the
	# right, is the bar's colour, and unset, it comes back.
	guest 'no minimize'
	wait_for 'edel-compositor: title bar buttons close, maximize$' ||
		fail "the compositor did not follow layout.minimize_button = false"
	opened=$(count 'edel-compositor: mapped window keys')
	guest 'shortcut window'
	wait_more 'edel-compositor: mapped window keys' "$opened" || fail "the test client keys did not open again"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*edel-compositor: mapped window keys at \([0-9]*\),\([0-9]*\) \([0-9]*\)x.*/\1 \2 \3/p' | tail -n 1)
	read -r x y w <<-EOF
		$place
	EOF
	# Maximize's outline first: waiting for its ink also waits out the
	# window's opening animation, which may show the bar's colour anywhere.
	ink=$(token title_text)
	shot buttons-no-minimize $((x + w - 46)) $((y - 14)) "$ink" >/dev/null ||
		fail "maximize's outline at $((x + w - 46)),$((y - 14)) is not the title's #$ink: maximize went too"
	shot buttons-no-minimize $((x + w - 70)) $((y - 14)) "$focused" >/dev/null ||
		fail "with layout.minimize_button = false, minimize's square at $((x + w - 70)),$((y - 14)) is not the bar's #$focused"
	# Super+M maximizes it (M5.18a): its entry in the windows line spans
	# the screen's width; again, and it has its size back.
	python3 ci/qmp.py key meta_l-m
	i=0
	until value windows | grep -qE 'keys@[0-9]+,[0-9]+,12[0-9][0-9]x'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "Super+M did not maximize keys: $(value windows)"
		sleep 0.2
	done
	python3 ci/qmp.py key meta_l-m
	i=0
	while value windows | grep -qE 'keys@[0-9]+,[0-9]+,12[0-9][0-9]x'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "Super+M again did not give keys its size back: $(value windows)"
		sleep 0.2
	done
	closed=$(count 'edel-compositor: unmapped window keys')
	python3 ci/qmp.py key meta_l-q
	wait_more 'edel-compositor: unmapped window keys' "$closed" || fail "Super+Q did not close keys"
	guest 'minimize default'
	wait_for 'edel-compositor: title bar buttons close, minimize, maximize' ||
		fail "unsetting layout.minimize_button did not bring minimize back"
	echo "PASS: layout.window_buttons = \"left\" put close at the bar's left end, a click there closed keys at $x,$y, unsetting it brought the buttons back to the right, layout.minimize_button = false took minimize off the bar until it was unset, and Super+M maximized keys and gave it its size back"
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

# accent_line: waits up to 10 s for the accent line of the
# focused window's button, which lies at row 791, 2 px high and 14 px
# wide, in the middle of its button, inside the window list, whose place
# it reads afresh at each try from shell-ui's last places line, and
# prints the line's middle column and the list's x and width.
accent_line() {
	i=0
	while :; do
		place=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 | sed -n 's/.*windows \([0-9]*\)+\([0-9]*\).*/\1 \2/p')
		if [ -n "$place" ]; then
			python3 ci/qmp.py screendump "$dir/windows.png"
			last=$(python3 ci/qmp.py findlast "$dir/windows.png" 791 "${place% *}" "$((${place% *} + ${place#* }))" "$accent")
			if [ "$last" != none ]; then
				echo "$((last - 6)) $place"
				return 0
			fi
		fi
		i=$((i + 1))
		[ "$i" -lt 50 ] || return 1
		sleep 0.2
	done
}

case_windows() {
	# The panel's window list (M5.2h, M5.29): a button for each window on
	# the screen, as wide as its icon and title, at most 200 px, together
	# at most 45% of the panel (576 px), 2 px at each end and between
	# them. Launching away adds one at the end, lit, with the accent line
	# 2 px high and 14 px wide in the middle of its button, 4 px above the
	# foot of a 30 px button in the middle of the 40 px panel that starts
	# at 760: row 791. The button's width is not computed here, as it
	# follows the title's width in the font; it is read from where the
	# line lies, the line being the list's last accent at that row.
	panel=$(token panel)
	accent=$(token accent)
	opened=$(count 'edel-compositor: mapped window away')
	# Counted before: earlier cases left such windows lines too.
	listed=$(count 'DESKTOP-TEST: windows [0-9]+ .*away@[0-9]+,[0-9]+,200x150$')
	guest 'away window'
	wait_more 'edel-compositor: mapped window away' "$opened" || fail "the test client away did not open: $(value windows)"
	wait_more 'DESKTOP-TEST: windows [0-9]+ .*away@[0-9]+,[0-9]+,200x150$' "$listed" || fail "away is not on top in the state file: $(value windows)"
	n=$(value windows | cut -d' ' -f1)
	line=$(accent_line) || fail "the window list holds no accent line at row 791 for away, the last of $n windows"
	read -r cx x w <<-EOF
		$line
	EOF
	# The list is within its share of the panel, and away's button, whose
	# line lies in its middle, hugs its short title: the list ends 2 px
	# after the button, so the button is twice the distance from the
	# line to there, between 56 and 100 px for a four letter title.
	[ "$w" -le 576 ] || fail "the window list is $w px wide, over 45% of the panel"
	half=$((x + w - 2 - cx))
	[ $((2 * half)) -ge 56 ] && [ $((2 * half)) -le 100 ] ||
		fail "away's button is about $((2 * half)) px wide, not 56 to 100 for its icon and four letters; the list is $x+$w, the line at $cx"
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
	# A click on its button brings it back, focused; 22 px left of the
	# line, which the cursor would cover.
	python3 ci/qmp.py click $((cx - 22)) 780
	wait_for 'edel-compositor: restored window away' || fail "a click on away's button at $((cx - 22)),780 did not bring it back"
	shot windows "$cx" 791 "$accent" >/dev/null || fail "away's button has no accent line after it came back"
	shot windows 640 393 7744aa >/dev/null || fail "away is not drawn at 640,393 after it came back"
	# Closed, its button goes: the list is narrower than it was.
	guest 'away off'
	place=$(list_until "-lt $w") ||
		fail "the window list is still $w px wide after away closed: $place"
	echo "PASS: away's opening added a button of $((2 * half)) px to the panel's window list with the accent line, its title bar's minimize button hid it and left the button unmarked, a click on the button brought it back, and closing it took the button away"
}

case_completion() {
	# Tab completion (M5.26): bash, ci's login shell where the completion
	# feature is, completes edel's commands and a key one part at a time.
	before=$(count 'DESKTOP-TEST: complete ')
	guest complete
	wait_more 'DESKTOP-TEST: complete ' $((before + 2)) || fail "no completion answers: $(value complete)"
	[ "$(value login_shell)" = /bin/bash ] || fail "ci's login shell is \"$(value login_shell)\", not bash"
	answers=$(tr -d '\r' <"$log" | sed -n 's/.*DESKTOP-TEST: complete //p' | tail -n 3 | tr '\n' '|')
	[ "$answers" = "loader yes|settings|layout.|" ] ||
		fail "bash completed \"$answers\", not loader yes, settings and layout."
	echo "PASS: ci's login shell is bash, bash-completion loaded edel's completion, edel se completed to settings and edel settings set lay to layout."
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
	# Through the settings file, as Settings and people will: the compositor
	# follows it and re-lays the windows out at once. foot is the master on
	# the left, one the stack on the right, 8 px apart and from the edges.
	guest 'tiling on'
	wait_for 'DESKTOP-TEST: ran tiling on: 0' ||
		fail "edel settings set layout.tiling=true did not run in the VM"
	wait_for 'edel-compositor: windows now tiling' ||
		fail "the compositor did not follow layout.tiling = true"
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
	echo "PASS: edel settings set layout.tiling=true tiled foot and one side by side with their title bars ($tiled), and Super+T floated them back where they were"
}

case_styles() {
	# Tiling styles (M5.16a): layout.tiling_style = "split" changes how
	# tiling lays windows out and leaves a floating workspace floating. On
	# workspace 4, empty, four windows open floating; Super+T tiles them
	# as split: s1 on the left half, s2 top right, s3 and s4 sharing the
	# bottom right quarter side by side. Super+Shift+Left swaps s4, the
	# focused one, with s3; stack then lays the same windows out as master
	# and stack, live.
	python3 ci/qmp.py key meta_l-4
	wait_for 'edel-compositor: workspace 4$' || fail "Super+4 did not show workspace 4"
	guest 'style split'
	wait_for 'edel-compositor: tiling style split' ||
		fail "the compositor did not follow layout.tiling_style = \"split\""
	guest 'four windows'
	i=0
	until value windows | grep -qE ' s4@[0-9]+,'; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "four test windows did not open: $(value windows)"
		sleep 0.2
	done
	floated=$(value windows)
	python3 ci/qmp.py key meta_l-t
	wait_for 'edel-compositor: windows now tiling' || fail "Super+T did not tile workspace 4"
	# places WINDOWS: each of s1 to s4 as name x y w h, from the windows line.
	places() {
		echo "$1" | grep -oE 's[1-4]@[0-9]+,[0-9]+,[0-9]+x[0-9]+' | tr '@,x' '   '
	}
	split_ok() {
		places "$(value windows)" | awk '
			{ x[$1] = $2; y[$1] = $3; w[$1] = $4; h[$1] = $5 }
			END {
				ok = x["s1"] < x["s2"] && h["s1"] > 2 * h["s2"] &&
				     y["s3"] > y["s2"] && y["s3"] == y["s4"] && x["s3"] == x["s2"] &&
				     x["s4"] > x["s3"] && w["s3"] < w["s2"]
				exit !ok
			}'
	}
	i=0
	until split_ok; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "with split, s1 to s4 are not halved in turn: $(value windows)"
		sleep 0.2
	done
	split=$(value windows)
	echo "$floated" | grep -qE 's1@[0-9]+,[0-9]+,200x150' ||
		fail "the style tiled a floating workspace before Super+T: $floated"
	closed=$(count 'edel-compositor: swapped s4 and s3')
	python3 ci/qmp.py key meta_l-shift-left
	wait_more 'edel-compositor: swapped s4 and s3' "$closed" ||
		fail "Super+Shift+Left did not swap s4 with s3, the window to its left: $(value windows)"
	guest 'style stack'
	wait_for 'edel-compositor: tiling style stack' ||
		fail "the compositor did not follow layout.tiling_style = \"stack\""
	stack_ok() {
		places "$(value windows)" | awk '
			{ x[$1] = $2; y[$1] = $3 }
			END { exit !(x["s1"] < x["s2"] && x["s2"] == x["s3"] && x["s3"] == x["s4"] && y["s2"] < y["s4"] && y["s4"] < y["s3"]) }'
	}
	i=0
	until stack_ok; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "with stack, s2 to s4 are not stacked on the right, s4 above s3: $(value windows)"
		sleep 0.2
	done
	stacked=$(value windows)
	guest 'four off'
	guest 'style default'
	python3 ci/qmp.py key meta_l-t
	python3 ci/qmp.py key meta_l-1
	# The layout button's menu (M5.16b): a right click opens it above the
	# button in the menus' colour with stack in use; a click on its second
	# row, Split, writes layout.tiling_style to ci's file and the
	# compositor follows, while workspace 1 still floats.
	panel=$(token panel)
	toggle=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 | sed -n 's/.*layout \([0-9]*\)+.*/\1/p')
	[ -n "$toggle" ] || fail "shell-ui did not say where its layout button lies"
	shown=$(count 'edel-shell-ui: styles menu shown, stack in use')
	python3 ci/qmp.py rightclick $((toggle + 18)) 780
	wait_more 'edel-shell-ui: styles menu shown, stack in use' "$shown" ||
		fail "a right click on the layout button at $((toggle + 18)),780 did not open the styles menu"
	i=0
	until value layers | grep -q 'edel-styles@'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "the state file lists no styles menu: $(value layers)"
		sleep 0.2
	done
	set -- $(value layers | grep -o 'edel-styles@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | sed 's/edel-styles@//; s/[,x]/ /g')
	# A row of 36 px for each style, stack, split and scroll, with 8 px
	# round them, and the shadow's room, if the tier draws one, round the
	# card.
	room=$((($4 - 16 - 3 * 36) / 2))
	mx=$(($1 + $3 / 2)) my=$(($2 + room + 8 + 36 + 18))
	shot styles $((mx + 40)) $(($2 + room + 4)) "$panel" >/dev/null ||
		fail "the styles menu at $1,$2 ${3}x$4 is not in the menus' #$panel"
	tiled=$(count 'edel-compositor: windows now tiling')
	chosen=$(count 'edel-compositor: tiling style split')
	python3 ci/qmp.py click "$mx" "$my"
	wait_more 'edel-compositor: tiling style split' "$chosen" ||
		fail "a click on the menu's Split row at $mx,$my did not choose split"
	guest 'settings file'
	sleep 1
	value settings_file | grep -q 'tiling_style = "split"' ||
		fail "ci's settings file does not hold the chosen style: $(value settings_file)"
	[ "$(count 'edel-compositor: windows now tiling')" = "$tiled" ] ||
		fail "choosing a style tiled the floating workspace"
	styled=$(count 'edel-compositor: tiling style stack')
	guest 'style mine default'
	wait_more 'edel-compositor: tiling style stack' "$styled" || fail "taking layout.tiling_style out of ci's file did not bring stack back"
	echo "PASS: the layout button's right-click menu showed stack in use, its Split row wrote layout.tiling_style = \"split\" and the compositor followed with workspace 1 still floating"
	echo "PASS: layout.tiling_style = \"split\" left workspace 4 floating ($floated), Super+T tiled four windows halved in turn ($split), Super+Shift+Left swapped s4 and s3, and stack laid them out as master and stack at once ($stacked)"
}

case_scroll() {
	# The scroll style (M5.16c): on workspace 4, empty, tiled with
	# layout.tiling_style = "scroll", four windows open as columns half
	# the screen wide; the newest is whole and the first off screen to
	# the left. Super+Left three times brings the first whole; a click on
	# the last in the panel's window list scrolls back to it.
	python3 ci/qmp.py key meta_l-4
	styled=$(count 'edel-compositor: tiling style scroll')
	guest 'style scroll'
	wait_more 'edel-compositor: tiling style scroll' "$styled" ||
		fail "the compositor did not follow layout.tiling_style = \"scroll\""
	tiled=$(count 'edel-compositor: windows now tiling')
	python3 ci/qmp.py key meta_l-t
	wait_more 'edel-compositor: windows now tiling' "$tiled" || fail "Super+T did not tile workspace 4"
	guest 'four windows'
	# column NAME: the window's x and width from the windows line.
	column() {
		value windows | grep -oE " $1@-?[0-9]+,[0-9]+,[0-9]+x" | sed -E 's/.*@(-?[0-9]+),[0-9]+,([0-9]+)x/\1 \2/'
	}
	# whole NAME: its column lies on the 1280 px screen.
	whole() {
		set -- $(column "$1")
		[ -n "$1" ] && [ "$1" -ge 0 ] && [ $(($1 + $2)) -le 1280 ]
	}
	# gone NAME: its column lies off the screen's left edge.
	gone() {
		set -- $(column "$1")
		[ -n "$1" ] && [ $(($1 + $2)) -le 0 ]
	}
	i=0
	until whole s4 && gone s1; do
		i=$((i + 1))
		[ "$i" -lt 100 ] || fail "with scroll, s4 is not whole with s1 off screen: $(value windows)"
		sleep 0.2
	done
	opened=$(value windows)
	python3 ci/qmp.py key meta_l-left meta_l-left meta_l-left
	i=0
	until whole s1; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "Super+Left three times did not bring s1 whole: $(value windows)"
		sleep 0.2
	done
	# The window list's fourth button, s4, the last: the list's place from
	# the panel's log; a click 14 px from the list's end is in it, as the
	# button is wider than that.
	asked=$(count 'DESKTOP-TEST: places ')
	guest 'panel places'
	wait_more 'DESKTOP-TEST: places ' "$asked" || fail "the service did not say where the panel's widgets lie"
	set -- $(value places | sed -n 's/.*windows \([0-9]*\)+\([0-9]*\).*/\1 \2/p')
	[ -n "$1" ] || fail "shell-ui did not say where its window list lies"
	python3 ci/qmp.py click $(($1 + $2 - 14)) 780
	i=0
	until whole s4; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "a click on s4 in the window list did not scroll back to it: $(value windows)"
		sleep 0.2
	done
	back=$(value windows)
	guest 'four off'
	guest 'style default'
	python3 ci/qmp.py key meta_l-t
	python3 ci/qmp.py key meta_l-1
	echo "PASS: with scroll, four windows opened as columns with s4 whole and s1 off screen ($opened), Super+Left three times brought s1 whole, and a click on s4 in the window list scrolled back to it ($back)"
}

case_everyday() {
	# The protocols everyday apps expect (M5.23): wayland-info's list, as
	# a plain client is offered it, holds all five; a window that locks
	# the pointer hears the mouse's motion while the pointer stays put;
	# one that keeps the screen on is counted in the state file, and no
	# longer once it closes.
	asked=$(count 'DESKTOP-TEST: sandbox ')
	guest globals
	wait_more 'DESKTOP-TEST: sandbox ' "$asked" || fail "the service did not list the globals"
	plain=" $(value plain) "
	for g in zwp_pointer_constraints_v1 zwp_relative_pointer_manager_v1 zwp_primary_selection_device_manager_v1 zwp_idle_inhibit_manager_v1 xdg_activation_v1; do
		case "$plain" in *" $g "*) ;; *) fail "the compositor does not offer $g:$plain" ;; esac
	done
	opened=$(count 'edel-compositor: mapped window lock')
	guest 'lock window'
	wait_more 'edel-compositor: mapped window lock' "$opened" || fail "the window that locks the pointer did not open"
	place=$(tr -d '\r' <"$log" | sed -n 's/.*edel-compositor: mapped window lock at \([0-9]*\),\([0-9]*\) .*/\1 \2/p' | tail -n 1)
	set -- $place
	python3 ci/qmp.py move $(($1 + 200)) $(($2 + 150))
	wait_for 'DESKTOP-TEST: lock locked' || fail "the pointer over the window did not lock"
	python3 ci/qmp.py nudge 40 20
	wait_for 'DESKTOP-TEST: lock relative ' || fail "the window that locked the pointer heard no relative motion"
	opened=$(count 'edel-compositor: mapped window idle')
	guest 'idle window'
	wait_more 'edel-compositor: mapped window idle' "$opened" || fail "the window that keeps the screen on did not open"
	i=0
	until [ "$(value idle_inhibitors)" = 1 ]; do
		i=$((i + 1))
		[ "$i" -lt 25 ] || fail "the state file does not count the window that keeps the screen on: $(value idle_inhibitors)"
		guest idle
		sleep 0.4
	done
	guest 'everyday off'
	i=0
	until [ "$(value idle_inhibitors)" = 0 ]; do
		i=$((i + 1))
		[ "$i" -lt 25 ] || fail "the state file still counts a closed window as keeping the screen on: $(value idle_inhibitors)"
		guest idle
		sleep 0.4
	done
	echo "PASS: the compositor offers pointer constraints, relative pointer, primary selection, idle inhibit and xdg-activation; a locked pointer gave its window the mouse's motion, and a window keeping the screen on was counted until it closed"
}

case_sandbox() {
	# Only the shell may use the shell's protocols (M5.22): a plain test
	# client is offered the layer shell, the window list, the workspaces
	# and edel-shell-v1; one that connects through a socket it made with
	# wp_security_context_v1, as Flatpak does for its apps, is offered
	# none of the four, and still the core it draws with.
	asked=$(count 'DESKTOP-TEST: sandbox ')
	guest globals
	wait_more 'DESKTOP-TEST: sandbox ' "$asked" || fail "the service did not list the globals"
	plain=" $(value plain) "
	sandboxed=" $(value sandbox) "
	for g in zwlr_layer_shell_v1 zwlr_foreign_toplevel_manager_v1 ext_workspace_manager_v1 edel_shell_v1; do
		case "$plain" in *" $g "*) ;; *) fail "a plain client is not offered $g:$plain" ;; esac
		case "$sandboxed" in *" $g "*) fail "a sandboxed client is offered $g:$sandboxed" ;; esac
	done
	case "$sandboxed" in *" sandboxed globals "*" wl_compositor "*) ;; *) fail "the sandboxed client saw no wl_compositor:$sandboxed" ;; esac
	echo "PASS: a client in a security context is offered none of the layer shell, the window list, the workspaces and edel-shell-v1, which a plain client is"
}

case_dmabuf() {
	# Apps draw on the GPU (M5.19): the compositor offers
	# zwp_linux_dmabuf_v1, version 4 with feedback, in the formats its
	# renderer imports, at least one.
	case " $(value globals) " in
	*" zwp_linux_dmabuf_v1 "*) ;;
	*) fail "the compositor does not offer zwp_linux_dmabuf_v1; wayland-info listed: $(value globals)" ;;
	esac
	line=$(value dmabuf | grep -o 'apps may hand over GPU buffers in [0-9]* formats')
	[ -n "$line" ] || fail "the compositor offers no GPU buffer formats: $(value dmabuf)"
	echo "PASS: the compositor offers zwp_linux_dmabuf_v1: $line"
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
		fail "edel settings set displays.Virtual-2.enabled=false did not run in the VM"
	wait_for 'edel-compositor: output Virtual-2 off' ||
		fail "the compositor did not turn Virtual-2 off"
	echo "PASS: two screens lit side by side, Virtual-1 at 0,0 and Virtual-2 at 1280,0 in the settings file's mode 1024x768, a window opened centred on Virtual-2 with the pointer there, and displays.Virtual-2.enabled = false turned the second off"
}

case_tray() {
	# The tray (M5.2e, M5.9g): shell-ui serves org.kde.StatusNotifierWatcher on
	# the session's bus. `sni` starts the test item (edel-testclient --sni:
	# titled "edel test", its icon a 22x22 pixmap of #33aa66, its Id
	# edel-testclient), which registers by its object path. A new app waits
	# behind the tray's arrow: shell-ui logs `tray: 1 items`, then `tray: 1
	# behind the arrow`, and the panel's places line gives the tray 34 px
	# (two 2 px margins and the arrow's 30 px cell). A click on the arrow opens
	# the grid above the panel, where the icon lies at the middle of its cell
	# (kept as traygrid.png); dragging it onto the panel keeps it there and
	# writes layout.tray_in_panel into ci's settings file, and the icon lies
	# at the arrow's place in a 34 px tray with no arrow. The icon is at the
	# middle of its cell in the panel, and a screen reader reads it as a
	# button named by the item's title, a click asks the item to Activate and
	# a right click for its ContextMenu; when the item's app ends, the icon
	# goes. Pointing at the arrow shows its tooltip, Super+B opens the grid
	# with the keyboard and Escape closes it (M5.9h). The key is taken out
	# again at the end (`tray reset`).
	panel=$(token panel)
	python3 ci/qmp.py move 640 300
	items=$(count 'edel-shell-ui: tray: 1 items')
	guest 'sni'
	wait_more 'edel-shell-ui: tray: 1 items' "$items" 30 ||
		fail "shell-ui did not log tray: 1 items after the test item registered"
	wait_for 'edel-shell-ui: tray: 1 behind the arrow' ||
		fail "shell-ui did not log tray: 1 behind the arrow: the new app should wait behind the arrow"
	wait_for 'edel-shell-ui: panel places .*tray [0-9]+\+34' ||
		fail "shell-ui's places line does not give the tray 34 px for the arrow"
	place=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: panel places' | tail -n 1 | sed -n 's/.*tray \([0-9]*\)+\([0-9]*\).*/\1 \2/p')
	read -r x w <<-EOF
		$place
	EOF
	[ "$w" = 34 ] || fail "the tray is $w px wide in the last places line, not 34"
	# Pointing at the arrow says how many wait behind it (M5.9h): after a
	# rest of 600 ms a tooltip shows the same words. Super+B opens the grid
	# with the keyboard, which takes the tooltip away, and Escape closes it.
	tip=$(count 'edel-shell-ui: tooltip shown, Hidden icons \(1\)')
	python3 ci/qmp.py move $((x + 17)) 780
	wait_more 'edel-shell-ui: tooltip shown, Hidden icons \(1\)' "$tip" ||
		fail "pointing at the arrow did not show the tooltip Hidden icons (1)"
	opened=$(count 'edel-shell-ui: tray grid opened from the keyboard')
	hidden=$(count 'edel-shell-ui: tooltip hidden')
	python3 ci/qmp.py key meta_l-b
	wait_more 'edel-shell-ui: tray grid opened from the keyboard' "$opened" ||
		fail "Super+B did not open the tray's grid from the keyboard"
	wait_more 'edel-shell-ui: tooltip hidden' "$hidden" ||
		fail "the tooltip stayed up when the tray's grid opened"
	closed=$(count 'edel-shell-ui: tray grid hidden')
	python3 ci/qmp.py key esc
	wait_more 'edel-shell-ui: tray grid hidden' "$closed" ||
		fail "Escape did not close the tray's grid"
	# The arrow, 17 px along the tray in the panel's row, opens the grid.
	shown=$(count 'edel-shell-ui: tray grid shown, 1 icons')
	python3 ci/qmp.py click $((x + 17)) 780
	wait_more 'edel-shell-ui: tray grid shown, 1 icons' "$shown" ||
		fail "a click on the arrow at $((x + 17)),780 did not open the tray's grid"
	# The grid's places, in logical pixels from its card's corner, which
	# the state file's layers line (the card and its shadow's room) places
	# on screen, as quick settings' are found.
	i=0
	until value layers | grep -q 'edel-tray@'; do
		i=$((i + 1))
		[ "$i" -lt 50 ] || fail "the state file lists no tray grid surface: $(value layers)"
		sleep 0.2
	done
	set -- $(value layers | grep -o 'edel-tray@[0-9]*,[0-9]*,[0-9]*x[0-9]*' | sed 's/edel-tray@//; s/[,x]/ /g')
	sx=$1 sy=$2 sw=$3 sh=$4
	grid=$(tr -d '\r' <"$log" | grep -a 'edel-shell-ui: tray grid places' | tail -n 1)
	read -r cw ch <<-EOF
		$(echo "$grid" | sed -n 's/.*card \([0-9]*\)x\([0-9]*\),.*/\1 \2/p')
	EOF
	read -r ix iy iw ih <<-EOF
		$(echo "$grid" | sed -n 's/.*icon edel-testclient \([0-9]*\)+\([0-9]*\)+\([0-9]*\)x\([0-9]*\).*/\1 \2 \3 \4/p')
	EOF
	[ -n "$ch" ] && [ -n "$ih" ] || fail "the tray grid's places line is not what CI reads: $grid"
	mx=$((sx + (sw - cw) / 2 + ix + iw / 2))
	my=$((sy + (sh - ch) / 2 + iy + ih / 2))
	shot traygrid $mx $my 33aa66 >/dev/null ||
		fail "the test item's icon is not #33aa66 at its middle in the grid, $mx,$my"
	# Dragged from the grid onto the panel's arrow place: kept in the panel,
	# the grid closes (no icon is left behind the arrow) and the key is written.
	closed=$(count 'edel-shell-ui: tray grid hidden')
	python3 ci/qmp.py drag $mx $my $((x + 17)) 780
	wait_for 'edel-shell-ui: tray: edel-testclient kept in the panel' ||
		fail "dragging the icon from $mx,$my onto the panel at $((x + 17)),780 did not keep it there"
	wait_more 'edel-shell-ui: tray grid hidden' "$closed" || fail "the grid did not close once its icon was kept"
	filed=$(count 'DESKTOP-TEST: settings_file ')
	guest 'settings file'
	wait_more 'DESKTOP-TEST: settings_file ' "$filed" || fail "the service did not read ci's settings file"
	value settings_file | grep -q 'tray_in_panel = \["edel-testclient"\]' ||
		fail "ci's settings file does not hold layout.tray_in_panel = [\"edel-testclient\"]: $(value settings_file)"
	# The kept icon takes the arrow's cell: the tray is still 34 px wide, at x.
	shot tray $((x + 17)) 780 33aa66 >/dev/null ||
		fail "the kept icon is not #33aa66 at its middle, $((x + 17)),780"
	shot tray $((x + 3)) 780 "$panel" >/dev/null ||
		fail "the margin beside the icon, $((x + 3)),780, is not the panel's #$panel"
	# A screen reader (M5.1d): the icon is the fourth level of AT-SPI's
	# tree, a button named by the item's title.
	asked=$(count 'DESKTOP-TEST: a11y_done')
	guest 'a11y tree'
	wait_more 'DESKTOP-TEST: a11y_done' "$asked" 30 || fail "the screen reader's walk of AT-SPI did not finish"
	tr -d '\r' <"$log" | grep -a 'DESKTOP-TEST: a11y ' | sed 's/.*DESKTOP-TEST: a11y //' | grep -qx '4 button: edel test' ||
		fail "AT-SPI holds no button named edel test under the panel's tray"
	# A left click asks the item to Activate, a right click for its menu;
	# the item prints what it was asked, at the click's place along the
	# panel.
	python3 ci/qmp.py click $((x + 17)) 780
	python3 ci/qmp.py rightclick $((x + 17)) 780
	i=0
	while :; do
		guest 'sni log'
		wait_for 'DESKTOP-TEST: sni .*activate [0-9]+ 0;context menu [0-9]+ 0;' 3 && break
		i=$((i + 1))
		[ "$i" -lt 5 ] || fail "the test item was not asked to Activate and then for its ContextMenu: $(value sni)"
	done
	python3 ci/qmp.py move 640 300
	items=$(count 'edel-shell-ui: tray: 0 items')
	guest 'sni off'
	wait_more 'edel-shell-ui: tray: 0 items' "$items" 30 ||
		fail "shell-ui did not log tray: 0 items after the test item's app ended"
	# The panel closes up: the layout toggle lies where the tray did.
	shot tray $((x + 17)) 780 '!33aa66' >/dev/null ||
		fail "the icon is still at $((x + 17)),780 after its app ended"
	# The key taken out again, as ci's file had none before the test.
	ran='DESKTOP-TEST: ran tray reset: 0'
	guest 'tray reset'
	wait_for "$ran" || fail "edel settings reset layout.tray_in_panel did not run as ci in the VM"
	filed=$(count 'DESKTOP-TEST: settings_file ')
	guest 'settings file'
	wait_more 'DESKTOP-TEST: settings_file ' "$filed" || fail "the service did not read ci's settings file"
	value settings_file | grep -q 'tray_in_panel' &&
		fail "ci's settings file still holds layout.tray_in_panel after tray reset: $(value settings_file)"
	echo "PASS: the test item first waited behind the arrow, pointing at the arrow said Hidden icons (1), Super+B opened the grid and Escape closed it, a click on the arrow opened the grid with its icon at $mx,$my, a drag from there onto the panel kept it in the panel and wrote layout.tray_in_panel = [\"edel-testclient\"], then the icon lay at $((x + 17)),780 in a 34 px tray and AT-SPI named it edel test, a click and a right click reached its Activate and ContextMenu, the icon went when its app ended, and tray reset took the key out again"
}

case_scale() {
	guest 'scale 2'
	wait_for 'DESKTOP-TEST: ran scale 2: 0' ||
		fail "edel settings set displays.Virtual-1.scale=2 did not run in the VM"
	wait_for 'edel-compositor: output Virtual-1 scale 2' ||
		fail "the compositor did not follow displays.Virtual-1.scale = 2"
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
	echo "PASS: displays.Virtual-1.scale = 2 applied at once: a 640x400 screen and a title bar 56 pixels high"
}

[ "$#" -gt 0 ] || set -- completion dmabuf floating titlebar tiling console pointer outputs compositor panel xwayland layers animations shortcuts workspaces windows launcher quick switcher presets buttons styles scroll sandbox taskbar dock panels dockhide fullscreen keyboard settings display sound network power updates portal tray scheme scale respawn
# Every case is a case_NAME function, so this list is the functions
# themselves and cannot miss one (the sandbox case was once left out).
cases=$(sed -n 's/^case_\([a-z]*\)() {$/\1/p' "$0" | sort | tr '\n' ' ')
for c in "$@"; do
	case "$c" in
	rollback) [ "$#" = 1 ] || { echo "rollback runs alone: it restarts the VM"; exit 1; } ;;
	live) [ "$#" = 1 ] || { echo "live runs alone: it boots the released image"; exit 1; } ;;
	*)
		case " $cases" in
		*" $c "*) ;;
		*)
			echo "unknown case $c; the cases are $cases"
			exit 1
			;;
		esac
		;;
	esac
done

# live (M3.6): the released desktop image as a USB stick, on a screen
# without 3D, which Mesa draws on with llvmpipe by itself; edel-boot-ok's
# "Started in" line comes once the session's compositor showed a frame.
if [ "$*" = live ]; then
	dir=out/desktop-live
	log=out/desktop-live.log
	QMP="$dir/qmp.sock"
	mkdir -p "$dir"
	rm -f "$QMP" "$dir"/*.png
	# A sparse copy rather than -snapshot, so a failure can show what the
	# stick wrote on its data partition (M5.28a).
	stick="$dir/stick.img"
	cp --sparse=always out/edel-desktop-x86_64.img "$stick"
	# stick_logs: the session's, greetd's and the system's logs from the
	# stick's data partition (partition 4), which /home and /var live on.
	stick_logs() {
		stop_vm
		part=$(sfdisk -d "$stick" | sed -n 's/^[^ ]*4 : start= *\([0-9]*\), size= *\([0-9]*\),.*/\1 \2/p')
		dd if="$stick" of="$dir/data.img" bs=512 skip="${part% *}" count="${part#* }" status=none
		# The VM was stopped with /data mounted, so the copy's journal is
		# not written back; debugfs cannot read it until e2fsck replays it.
		e2fsck -fy "$dir/data.img" >/dev/null 2>&1 || true
		. ci/names.sh
		people=$(debugfs -R "ls -p /home" "$dir/data.img" 2>&1 | awk -F/ '$6 != "." && $6 != ".." && $6 != "" { print "/home/" $6 }')
		for f in $(for p in $people; do echo "$p/$home_session_log"; done) /var/log/greetd.log "$system_log"; do
			echo "== $f on the stick"
			debugfs -R "cat $f" "$dir/data.img" 2>&1 | tail -n 80
		done
	}
	keep_vm=1 run_vm "$log" 'Started in [0-9.]+ s' "${DESKTOP_TEST_TIMEOUT:-300}" -no-reboot \
		-m 2048 -smp 4 -vga std \
		-qmp unix:"$QMP",server=on,wait=off \
		-device qemu-xhci,id=xhci \
		-drive if=none,id=stick,format=raw,file="$stick" \
		-device usb-storage,bus=xhci.0,drive=stick,removable=on,bootindex=0
	if [ "$found" = 0 ]; then
		cat "$log"
		stick_logs
		fail "the desktop image started from a USB stick did not confirm its slot"
	fi
	grep -a 'edel boot live: ' "$log" | tr -d '\r'
	tr -d '\r' <"$log" | grep -qa 'edel boot live: started from a removable disk, sd[a-z]*, so live logs in by itself' ||
		fail "edel boot live did not take the USB stick for one"
	python3 ci/qmp.py screendump "$dir/live.png"
	size=$(python3 ci/qmp.py size "$dir/live.png")
	w=${size% *} h=${size#* }
	panel=$(token panel)
	# The session starts after the slot is confirmed, and a runner busy
	# with the other lane draws late: up to a minute for the panel.
	i=0
	until shot live $((w / 2)) $((h - 10)) "$panel" >/dev/null; do
		i=$((i + 1))
		if [ "$i" -ge 6 ]; then
			tr -d '\r' <"$log" | tail -n 100
			stick_logs
			fail "$((w / 2)),$((h - 10)) on the ${w}x$h screen is not the panel's #$panel: nobody logged in by themselves"
		fi
	done
	# The whole screen drawn, not only what changed: the framebuffer
	# driver the stick may run on shows only the areas a frame names.
	background=$(token background)
	shot live $((w / 2)) $((h / 2)) "$background" >/dev/null ||
		fail "$((w / 2)),$((h / 2)) is not the background's #$background: the screen was drawn only in part"
	# What boot-test checked on the laptop stick until the desktop took
	# its place (2026-10-06): linux-lts from USB, /data, the version, the
	# guard on the hardware watchdog, the report on the EFI system
	# partition (printed after the write), and ssh shipped off.
	for line in 'Welcome to Edel OS' 'edel update: slot A confirmed' 'edel-data: mounted /data' \
		"Edel OS ${EDEL_VERSION:-0.1}, channel " 'edel guard: using the hardware watchdog' \
		'edel report: wrote /EFI/edel/report.toml on the EFI system partition'; do
		i=0
		while ! tr -d '\r' <"$log" | grep -qaF "$line" && [ "$i" -lt 60 ]; do
			i=$((i + 1))
			sleep 1
		done
		tr -d '\r' <"$log" | grep -qaF "$line" || fail "the stick's serial log has no \"$line\""
	done
	! tr -d '\r' <"$log" | grep -qa 'Starting sshd' || fail "the desktop stick started sshd, but desktop.toml ships ssh off"
	stop_vm
	rm -f "$stick" "$dir/data.img"
	echo "PASS: the desktop image started from a USB stick logged live in by itself (its panel lies along the bottom of the ${w}x$h screen), mounted /data, guarded the boot with the hardware watchdog, wrote its report and left sshd off"
	exit 0
fi

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
	-audiodev none,id=sound0 -device intel-hda -device hda-duplex,audiodev=sound0 \
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
