# common.sh, sourced by the spike's scripts inside their containers (M10.9, M10.10).

# ms: milliseconds since the machine started, from /proc/uptime (10 ms steps)
ms() { awk '{printf "%d", $1 * 1000}' /proc/uptime; }

# start_weston: a headless Wayland compositor on wayland-1, drawing with the machine's own mesa
start_weston() {
	apk add -q weston weston-backend-headless weston-shell-desktop mesa-dri-gallium font-dejavu >/dev/null
	export XDG_RUNTIME_DIR=/tmp/xdg WAYLAND_DISPLAY=wayland-1
	mkdir -p -m 700 $XDG_RUNTIME_DIR
	weston --backend=headless --socket=wayland-1 --width=1280 --height=800 >/tmp/weston.log 2>&1 &
	i=0; while [ $i -lt 100 ] && [ ! -S $XDG_RUNTIME_DIR/wayland-1 ]; do sleep 0.1; i=$((i + 1)); done
}

# first_frame FIELDS BINARY COMMAND...: starts COMMAND (an app that logs its Wayland requests,
# WAYLAND_DEBUG=1), waits up to 60 s for its first buffer on a surface, waits 3 s more, then prints
# a RESULT line: FIELDS, whether it drew, the milliseconds to the first frame and the memory of the
# process running BINARY (its proportional set size), and stops it.
first_frame() {
	fields=$1 bin=$2; shift 2
	rm -f /tmp/wl.log; t0=$(ms)
	"$@" 2>/tmp/wl.log >/dev/null &
	i=0; while [ $i -lt 6000 ] && ! grep -q 'attach(wl_buffer' /tmp/wl.log 2>/dev/null; do sleep 0.01; i=$((i + 1)); done
	t1=$(ms); sleep 3
	pid=; for q in $(pgrep -f "$bin"); do case $(readlink /proc/$q/exe) in *"$bin") pid=$q ;; esac; done
	pss=$(awk '/^Pss:/{print $2}' /proc/${pid:-0}/smaps_rollup 2>/dev/null)
	drew=no; grep -q 'attach(wl_buffer' /tmp/wl.log && drew=yes
	echo "RESULT $fields drew=$drew first_frame_ms=$((t1 - t0)) pss_kib=${pss:-0}"
	[ $drew = yes ] || grep -v '^\[' /tmp/wl.log | head -8
	pkill -f "$bin"; sleep 1
}
