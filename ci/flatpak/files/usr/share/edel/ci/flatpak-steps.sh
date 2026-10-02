#!/bin/sh
# Test only (roadmap M1.9), run as user ci by the edel-flatpak-test
# service: adds Flathub with `flatpak --user`, installs the newest and then
# the oldest org.freedesktop.Platform runtime Flathub serves, and runs a
# shell from each. Every result is a FLATPAK-TEST line; install errors are
# printed, and the oldest runtime has a time limit, so it can only add a
# report line. Writes "ok" to /tmp/flatpak-newest when the newest ran.

say() {
	echo "FLATPAK-TEST: $*"
}

# try BRANCH SECONDS: install the runtime within SECONDS and run a shell
# from it; succeeds when the shell printed the line.
try() {
	start=$(date +%s)
	if ! err=$(timeout "$2" flatpak --user install -y --noninteractive flathub \
		"org.freedesktop.Platform//$1" 2>&1 >/dev/null); then
		say "runtime $1: install failed after $(($(date +%s) - start)) s"
		echo "$err" | tail -n 5 | sed 's/^/FLATPAK-TEST:   /'
		return 1
	fi
	# One line per runtime: the shell's line, or the tail of the error.
	if dbus-run-session -- flatpak run --command=sh "org.freedesktop.Platform//$1" \
		-c 'echo hello from glibc' 2>/tmp/flatpak-run.err | grep -qx 'hello from glibc'; then
		say "runtime $1: ran a shell, which printed hello from glibc ($(($(date +%s) - start)) s)"
	else
		say "runtime $1: installed but did not run"
		tail -n 5 /tmp/flatpak-run.err | sed 's/^/FLATPAK-TEST:   /'
		return 1
	fi
}

if unshare -U true 2>/dev/null; then
	say "unprivileged user namespaces: yes"
else
	say "unprivileged user namespaces: no"
fi
flatpak --user remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo || exit 1
branches=$(flatpak --user remote-ls --runtime --all --columns=application,branch flathub |
	awk '$1 == "org.freedesktop.Platform" && $2 ~ /^[0-9]+\.[0-9]+$/ { print $2 }' |
	sort -u -t. -k1,1n -k2,2n)
say "org.freedesktop.Platform branches on Flathub:" $branches
newest=$(echo "$branches" | tail -n 1)
oldest=$(echo "$branches" | head -n 1)
[ -n "$newest" ] && try "$newest" 900 && echo ok >/tmp/flatpak-newest
[ -n "$oldest" ] && [ "$oldest" != "$newest" ] && try "$oldest" 600
say "installed size $(du -sh ~/.local/share/flatpak | cut -f1)"
exit 0
