#!/bin/sh
# fair.sh flathub | local BRANCH (M10.10, the fairer rerun asked for on 2026-10-25): installs GNOME Calculator, Text Editor and
# Clocks from Flathub, or from /out/repo-BRANCH (APPS="gnome-calculator gnome-text-editor gnome-clocks"
# sh runtime.sh), prints what each installed item takes, times the calculator's first frame ten times,
# then opens all three at once and adds up their memory, which shows what they share.
set -u
. /scripts/common.sh
v=$(cut -d. -f1,2 /etc/alpine-release)
apk add -q flatpak >/dev/null
start_weston
export HOME=/tmp/home; mkdir -p $HOME
ids="org.gnome.Calculator org.gnome.TextEditor org.gnome.clocks"
if [ "$1" = flathub ]; then
	from=flathub; flatpak --user remote-add flathub https://dl.flathub.org/repo/flathub.flatpakrepo
else
	from=alpine-$2; flatpak --user remote-add --no-gpg-verify $from file:///out/repo-$2
fi
rx() { awk -F'[: ]+' 'NR > 2 && $2 != "lo" {s += $3} END {print s + 0}' /proc/net/dev; }
r0=$(rx)
flatpak --user install -y --noninteractive $from $ids >/tmp/install.log 2>&1 || { tail -5 /tmp/install.log; exit 1; }
download=$((($(rx) - r0) / 1024)); [ "$1" = local ] && download=$(du -sk /out/repo-$2/objects | cut -f1)
disk=$(du -sk $HOME/.local/share/flatpak | cut -f1)
flatpak --user list --columns=application,branch,size | sed "s/^/ITEM from=$from /"
for run in 1 2 3 4 5 6 7 8 9 10; do
	first_frame "base=$v from=$from app=calc run=$run download_kib=$download disk_kib=$disk" gnome-calculator env WAYLAND_DEBUG=1 flatpak run org.gnome.Calculator
done
for id in $ids; do flatpak run $id >/dev/null 2>&1 & done
sleep 8
total=0; n=0
for q in $(pgrep -f 'gnome-calculator|gnome-text-editor|gnome-clocks'); do
	case $(readlink /proc/$q/exe) in */gnome-calculator | */gnome-text-editor | */gnome-clocks)
		p=$(awk '/^Pss:/{print $2}' /proc/$q/smaps_rollup); total=$((total + p)); n=$((n + 1)) ;;
	esac
done
echo "TOGETHER base=$v from=$from apps=$n pss_kib=$total"
pkill -f 'gnome-calculator|gnome-text-editor|gnome-clocks'
