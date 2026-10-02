#!/bin/sh
# Test only (roadmap M2.4), run by the edel-install-test service. On the
# live image, with a blank second disk /dev/vdb: once the running slot is
# confirmed, `edel install` without --yes must show its plan, exit 3 and
# leave the disk as it was; with --yes it installs. The installed system
# runs this too, from its copy of the slot, and only reports its ssh host
# key, which must differ from the live image's.

say() {
	echo "INSTALL-TEST: $*" >/dev/console
}

say "host key $(ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub | cut -d' ' -f2)"
if [ ! -b /dev/vdb ]; then
	say "installed system up"
	exit 0
fi
tries=0
until edel update status | grep -q '^A: ok=1'; do
	tries=$((tries + 1))
	if [ "$tries" -ge 120 ]; then
		say "FAIL: slot A was never confirmed"
		exit 1
	fi
	sleep 1
done

file=/usr/share/edel/ci/install.toml
before=$(sha256sum /dev/vdb | cut -d' ' -f1)
edel install /dev/vdb --system "$file" </dev/null
code=$?
after=$(sha256sum /dev/vdb | cut -d' ' -f1)
if [ "$before" = "$after" ]; then
	say "exit code $code without --yes, disk unchanged"
else
	say "exit code $code without --yes, disk CHANGED"
fi
if edel install /dev/vdb --system "$file" --yes </dev/null; then
	sync
	say "installed"
else
	say "FAIL: edel install --yes failed"
fi
