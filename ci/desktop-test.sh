#!/bin/sh
# desktop-test.sh CASE... (roadmap M4.1): boots the CI desktop image
# (ci/desktop/vm.toml) once, with a virtual GPU, keyboard and tablet and
# a QMP socket for ci/qmp.py, waits for its test service's results on the
# serial console, then runs each CASE against the running VM:
#
#   console    the login prompt on serial, /run/user/UID made with mode
#              0700 by pam_rundir for the autologin, a screenshot that is
#              not uniformly black and shows foot's title bar, and typing
#              `exit` closes foot
#   baseline   sway's numbers under llvmpipe, written to the step summary:
#              the budgets of M4.2a are set from them
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

case_console() {
	grep -q 'edel login:' "$log" || fail "no login prompt on the serial console"
	rundir=$(value rundir)
	[ "$rundir" = "700 ci" ] || fail "/run/user/UID of ci is \"$rundir\", not mode 700 owned by ci"
	python3 ci/qmp.py screendump "$dir/size.png"
	set -- $(python3 ci/qmp.py size "$dir/size.png")
	rm -f "$dir/size.png"
	x=$(($1 / 2)) y=2 middle=$(($2 / 2))
	# Foot's title bar, the focused colour sway's CI config sets.
	bar=$(shot console "$x" "$y" 3366cc) ||
		fail "foot's title bar is not on the screen: $x,$y is #$bar, not #3366cc"
	uniform=$(python3 ci/qmp.py uniform "$dir/console.png")
	[ "$uniform" = varied ] || fail "the screen is $uniform"
	# Click into foot and type into its shell: the keyboard reaches the
	# window, and the window closes.
	python3 ci/qmp.py click "$x" "$middle"
	python3 ci/qmp.py type 'exit\n'
	shot typed "$x" "$y" '!3366cc' >/dev/null ||
		fail "typing exit did not close foot; its title bar is still at $x,$y"
	echo "PASS: the desktop image booted to the login prompt, logged ci in to sway with /run/user/UID at 0700, showed foot's title bar on screen and closed it when exit was typed (${waited}s)"
}

case_baseline() {
	ready=$(value ready_seconds) rss=$(value sway_rss_mib) memory=$(value memory_in_use_mib)
	p50=$(value frame_p50_ms) p99=$(value frame_p99_ms) frames=$(value frames)
	[ -n "$ready" ] && [ -n "$rss" ] && [ "${frames:-0}" -gt 0 ] && [ "$p99" != none ] ||
		fail "the baseline is incomplete: ready $ready s, sway $rss MiB, $frames frames, p99 $p99 ms"
	echo "PASS: sway baseline under llvmpipe: ready ${ready} s after the kernel started, sway ${rss} MiB, ${memory} MiB in use, frames every ${p50} ms, p99 ${p99} ms, over ${frames} frames"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		cat >>"$GITHUB_STEP_SUMMARY" <<-EOF
			### Desktop baseline (sway under llvmpipe, M4.1)

			| Measure | Value |
			|---|---|
			| Boot to sway ready | ${ready} s |
			| sway RSS | ${rss} MiB |
			| Memory in use | ${memory} MiB |
			| Time between frames | median ${p50} ms, p99 ${p99} ms, over ${frames} frames |

		EOF
	fi
}

[ "$#" -gt 0 ] || set -- console baseline
for c in "$@"; do
	case "$c" in
	console | baseline) ;;
	*)
		echo "unknown case $c; the cases are console and baseline"
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
