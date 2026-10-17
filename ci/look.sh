#!/bin/sh
# A picture of the desktop as the release draws it, without a VM: builds
# the compositor, shell-ui and Settings inside the Alpine release the base
# feature names, with the packages and files of the desktop's UI features,
# runs them nested under Xvfb at the VM tests' 1280x800, and keeps a
# screenshot. The host's own GTK, libadwaita and fonts draw a different
# picture (an older libadwaita lays rows out otherwise, and without the
# shell feature's gtk.css Settings misses our look), so UI work shows
# pictures from here while coding and CI's desktop tests prove them.
#
#   sh ci/look.sh NAME [COMMAND...]
#   sh ci/look.sh location edel-settings --page location
#
# keeps out/look/NAME.png; with no COMMAND, the desktop alone. Needs a
# Docker daemon and the Alpine mirror; the first run builds for some
# minutes, later runs reuse target/look.
set -eu

[ $# -ge 1 ] || {
	echo "usage: sh ci/look.sh NAME [COMMAND...]" >&2
	exit 2
}

# The desktop's UI features: their packages and their files over the root.
features="fonts graphics compositor shell settings terminal wallpapers"

if [ -z "${EDEL_LOOK_INSIDE:-}" ]; then
	branch=$(sed -n 's/^branch = "v\(.*\)"$/\1/p' features/base.toml)
	packages=$(for f in $features; do
		sed -n 's/^packages = \[\(.*\)\]$/\1/p' "features/$f.toml"
	done | tr -d '",' | tr '\n' ' ')
	# One image per package list, so later runs start at once.
	tag=edel-look:$(echo "$branch $packages $(sed -n '/apk add/,/x11$/p' ci/look.sh)" | cksum | cut -d' ' -f1)
	proxy=
	for v in HTTPS_PROXY HTTP_PROXY https_proxy http_proxy NO_PROXY no_proxy; do
		eval "[ -n \"\${$v:-}\" ]" && proxy="$proxy -e $v"
	done
	ca=
	[ -e /root/.ccr/ca-bundle.crt ] && ca="-v /root/.ccr/ca-bundle.crt:/etc/ssl/certs/ca-certificates.crt:ro"
	if ! docker image inspect "$tag" >/dev/null 2>&1; then
		docker rm -f edel-look-setup >/dev/null 2>&1 || true
		# shellcheck disable=SC2086
		docker run --name edel-look-setup --network host $proxy $ca "alpine:$branch" \
			apk add --no-cache $packages cargo pkgconf eudev-dev libinput-dev libseat-dev libxkbcommon-dev mesa-dev \
			gtk4.0-dev libadwaita-dev xvfb imagemagick xdotool dbus dbus-x11 libx11 libxcursor libxrandr libxi libxkbcommon-x11
		docker commit edel-look-setup "$tag" >/dev/null
		docker rm edel-look-setup >/dev/null
	fi
	# shellcheck disable=SC2086
	exec docker run --rm --network host -e EDEL_LOOK_INSIDE=1 $proxy $ca -v "$PWD:/src" -w /src "$tag" sh ci/look.sh "$@"
fi

. ci/names.sh
name=$1
shift
export CARGO_HOME=/src/.cargo-home CARGO_TARGET_DIR=/src/target/look
cargo build --locked -p edel-compositor -p edel-shell-ui -p edel-settings -p edel-testclient --features edel-compositor/winit
for f in $features; do
	[ -d "features/$f" ] && cp -a "features/$f/." /
done
# Every feature file of the desktop image, as the image keeps them, since
# Settings shows a page only where its feature is.
mkdir -p "$share_dir/features"
for f in $(sed -n 's/^features = \[\(.*\)\]$/\1/p' images/desktop.toml | tr -d '",'); do
	cp "features/$f.toml" "$share_dir/features/"
done
fc-cache -f >/dev/null

work=$(mktemp -d)
mkdir -m 700 "$work/run" "$work/home"
cp -a /etc/skel/. "$work/home/"
export DISPLAY=:74 XDG_RUNTIME_DIR="$work/run" HOME="$work/home"
export PATH="$CARGO_TARGET_DIR/debug:$PATH" LIBGL_ALWAYS_SOFTWARE=1
Xvfb "$DISPLAY" -screen 0 1280x800x24 -nolisten tcp >"$work/xvfb.log" 2>&1 &
sleep 1
eval "$(dbus-launch --sh-syntax)"
edel-compositor >"$work/compositor.log" 2>&1 &

# wait_for TEXT SECONDS: until the compositor's log has a line with TEXT.
wait_for() {
	n=0
	until grep -q "$1" "$work/compositor.log"; do
		n=$((n + 1))
		if [ "$n" -gt $(($2 * 5)) ]; then
			echo "ci/look.sh: no \"$1\" from the compositor within $2 s; its log:" >&2
			cat "$work/xvfb.log" "$work/compositor.log" "$work/command.log" >&2 2>/dev/null || true
			exit 1
		fi
		sleep 0.2
	done
}
wait_for 'listening on wayland-' 30
WAYLAND_DISPLAY=$(sed -n 's/^edel-compositor: listening on //p' "$work/compositor.log" | head -n 1)
export WAYLAND_DISPLAY
wait_for 'edel-shell-ui: panel ' 30
if [ $# -gt 0 ]; then
	"$@" >"$work/command.log" 2>&1 &
	wait_for 'mapped window ' 60
	# Maximized, as CI's Settings tour photographs each page: a click on
	# the title bar's maximize button, from the place the compositor gave.
	if [ -z "${EDEL_LOOK_FLOAT:-}" ]; then
		set -- $(sed -n 's/^edel-compositor: mapped window .* at \([0-9]*\),\([0-9]*\) \([0-9]*\)x[0-9]*$/\1 \2 \3/p' "$work/compositor.log" | tail -n 1)
		[ $# -ge 3 ] || {
			echo "ci/look.sh: no place for the window in the compositor's log" >&2
			cat "$work/compositor.log" >&2
			exit 1
		}
		# The bar's middle row, from the title bar token; its maximize
		# button is the second from the right in the default layout.
		bar=$(awk '/^\[/ { size = ($0 == "[size]") } size && /^title_bar *=/ { print $3; exit }' design/tokens.toml)
		sleep 1
		xdotool mousemove $(($1 + $3 - 41)) $(($2 - bar / 2)) click 1 mousemove 1279 400
	fi
fi
# Let the window's first frames and the panel's redraw land.
sleep "${EDEL_LOOK_WAIT:-4}"
mkdir -p out/look
import -display "$DISPLAY" -window root "out/look/$name.png"
# Hand the files back to the checkout's owner, as ci/build.sh does.
chown -R "$(stat -c %u:%g /src)" target/look .cargo-home out/look
echo "ci/look.sh: kept out/look/$name.png"
