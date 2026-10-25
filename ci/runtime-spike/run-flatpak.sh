#!/bin/sh
# run-flatpak.sh flathub | local BRANCH (M10.10), in an alpine container standing for the machine:
# installs GNOME Calculator from Flathub, or from /out/repo-BRANCH (runtime.sh), as a user, then
# measures what it took on disk and its first frame and memory three times. Prints RESULT lines.
set -u
. /scripts/common.sh
v=$(cut -d. -f1,2 /etc/alpine-release)
apk add -q flatpak >/dev/null
start_weston
export HOME=/tmp/home; mkdir -p $HOME
if [ "$1" = flathub ]; then
	from=flathub
	flatpak --user remote-add flathub https://dl.flathub.org/repo/flathub.flatpakrepo
else
	from=alpine-$2
	flatpak --user remote-add --no-gpg-verify $from file:///out/repo-$2
fi
rx() { awk -F'[: ]+' 'NR > 2 && $2 != "lo" {s += $3} END {print s + 0}' /proc/net/dev; }
r0=$(rx)
flatpak --user install -y --noninteractive $from org.gnome.Calculator >/tmp/install.log 2>&1 || { tail -5 /tmp/install.log; exit 1; }
download=$((($(rx) - r0) / 1024))
[ "$1" = local ] && download=$(du -sk /out/repo-$2/objects | cut -f1)
disk=$(du -sk $HOME/.local/share/flatpak | cut -f1)
flatpak --user list --columns=application,branch,size | sed 's/^/INSTALLED /'
for run in 1 2 3; do
	first_frame "base=$v from=$from run=$run download_kib=$download disk_kib=$disk" gnome-calculator \
		env WAYLAND_DEBUG=1 flatpak run org.gnome.Calculator
done
