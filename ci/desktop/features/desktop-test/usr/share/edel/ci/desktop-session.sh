#!/bin/sh
# Test only (roadmap M4.1): runs in CI's sway session as user ci. Writes
# $XDG_RUNTIME_DIR/desktop-test, which the test service prints: the
# seconds from the kernel's start to sway running this script, the median
# and 99th percentile of the time between presented frames while
# weston-presentation-shm draws a frame every time sway asks, for 10 s,
# and sway's window tree once foot is the only window, the one the console
# case looks at and types into.
set -u
out=$XDG_RUNTIME_DIR/desktop-test
ready=$(cut -d' ' -f1 /proc/uptime)
timeout -s INT 10 weston-presentation-shm >"$out.frames" 2>&1
# Lines hold "p2p N us": microseconds since the previous presented frame.
frames=$(awk '{ for (i = 1; i < NF; i++) if ($i == "p2p" && $(i + 1) > 0) print $(i + 1) }' "$out.frames" | sort -n)
count=$(echo "$frames" | grep -c . || true)
# percentile P: the P-th percentile of the frame times, in milliseconds.
percentile() {
	echo "$frames" | awk -v n="$count" -v p="$1" 'NR == int(n * p / 100 + 0.999) { printf "%.1f", $1 / 1000 }'
}
p50=$(percentile 50)
p99=$(percentile 99)
foot &
i=0
while ! swaymsg -t get_tree | grep -Eq '"app_id": ?"foot"' && [ "$i" -lt 60 ]; do
	sleep 1
	i=$((i + 1))
done
# One frame for foot's first paint.
sleep 1
{
	echo "ready_seconds $ready"
	echo "frames $count"
	echo "frame_p50_ms ${p50:-none}"
	echo "frame_p99_ms ${p99:-none}"
	swaymsg -p -t get_tree | sed 's/^/tree /'
} >"$out.tmp"
mv "$out.tmp" "$out"
