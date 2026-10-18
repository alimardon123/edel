#!/bin/sh
# Test only (roadmap M2.4), run by the edel-install-test service. On the
# live image, with a blank second disk /dev/vdb: once the running slot is
# confirmed, `edel install` without --yes must show its plan, exit 3 and
# leave the disk as it was; with --yes it installs. The installed system
# runs this too, from its copy of the slot, and only reports its ssh host
# key, which must differ from the live image's. With a third disk /dev/vdc
# and a second serial port, `edel install` with no arguments installs on it
# from the answers typed there (M6.6b).

say() {
	echo "INSTALL-TEST: $*" >/dev/console
}

say "host key $(ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub | cut -d' ' -f2)"
if [ ! -b /dev/vdb ]; then
	say "installed system up"
	exit 0
fi
tries=0
until edel status | grep -q '^A: ok=1'; do
	tries=$((tries + 1))
	if [ "$tries" -ge 120 ]; then
		say "FAIL: slot A was never confirmed"
		exit 1
	fi
	sleep 1
done

# A refused install writes nothing; any install writes the partition
# tables at both ends first, so the first 64 MiB and the last 1 MiB show
# a write without reading the whole 10 GiB disk.
ends() {
	sectors=$(cat /sys/block/vdb/size)
	{
		dd if=/dev/vdb bs=1M count=64 2>/dev/null
		dd if=/dev/vdb bs=512 skip=$((sectors - 2048)) count=2048 2>/dev/null
	} | sha256sum | cut -d' ' -f1
}
. /usr/share/edel/places.sh
# M6.6b: the settings file over https, from ci/https-serve.py on the
# runner; its throwaway authority comes over plain http first and is
# added to the machine's own, as a fleet adds its authority. The same
# file over plain http must be refused before anything is read.
tries=0
until wget -q -O /tmp/ca.pem http://10.0.2.2:8080/ca.pem; do
	tries=$((tries + 1))
	if [ "$tries" -ge 60 ]; then
		say "FAIL: the test authority never came over http"
		exit 1
	fi
	sleep 1
done
cat /tmp/ca.pem >>/etc/ssl/certs/ca-certificates.crt
say "authority trusted"
say "http refused: $(edel install /dev/vdb --settings http://10.0.2.2:8080/install.toml --plan 2>&1 </dev/null | tail -n 1 | sed 's/^edel install: //')"
file=https://10.0.2.2:8443/install.toml
before=$(ends)
edel install /dev/vdb --settings "$file" </dev/null
code=$?
after=$(ends)
if [ "$before" = "$after" ]; then
	say "exit code $code without --yes, disk unchanged"
else
	say "exit code $code without --yes, disk CHANGED"
fi
# The install walk (M8.15a) is timed here, in the guest, so the host's
# polling never counts; the guest's clock is the host's through QEMU's RTC.
began=$(date +%s)
if ! edel install /dev/vdb --settings "$file" --yes </dev/null; then
	say "FAIL: edel install --yes failed"
	exit 1
fi
say "walk install $(($(date +%s) - began)) 1"
sync

# M6.6b: with no arguments, edel install asks its questions on a
# terminal: the second serial port, made its controlling terminal, which
# ci/serial.py answers from the host. Then the new disk's data partition
# shows what the answers wrote.
if [ -b /dev/vdc ] && [ -c /dev/ttyS1 ]; then
	setsid -c edel install </dev/ttyS1 >/dev/ttyS1 2>&1
	say "tty exit $?"
	mkdir -p /run/tty-target
	if mount /dev/vdc4 /run/tty-target; then
		say "tty settings $(tr '\n' ';' <"/run/tty-target${data_dir#/data}/$settings_name")"
		say "tty passwords $(stat -c %a "/run/tty-target${first_passwords#/data}") $(cut -c1-7 "/run/tty-target${first_passwords#/data}")"
		umount /run/tty-target
	fi
fi
say "installed"
