#!/bin/sh
# Roadmap M2.2 and M2.3: boots the system test image, whose slot carries a
# seed settings file (made by ci/build.sh) that names the machine ci-seeded
# and adds user ci with a fresh ssh key. Passes when the first boot reaches
# the login prompt under that hostname, ci logs in over ssh and exports the
# machine, and then, as root: diff is empty after apply, set changes exactly
# the hostname, and unset plus apply brings the image's hostname back, with
# an unknown key and a comment kept byte for byte.
set -eu
. ci/vm.sh

log=out/system-test.log
keep_vm=1
vm_net=",hostfwd=tcp:127.0.0.1:2222-:22"
run_vm "$log" 'ci-seeded login:' "${BOOT_TIMEOUT:-300}" -no-reboot -snapshot \
	-drive if=none,id=disk0,format=raw,file=out/system-test/edel-vm-x86_64.img \
	-device virtio-blk-pci,drive=disk0,bootindex=0
trap stop_vm EXIT
if [ "$found" != 1 ]; then
	cat "$log"
	echo "FAIL: the first boot did not reach the login prompt as ci-seeded"
	exit 1
fi

ssh_as() {
	who=$1
	shift
	ssh -i out/keys/ci-ssh -p 2222 -o BatchMode=yes -o ConnectTimeout=10 -o LogLevel=ERROR \
		-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "$who@127.0.0.1" "$@"
}
ssh_ci() { ssh_as ci "$@"; }
ssh_root() { ssh_as root "$@"; }
fail() {
	cat "$log"
	echo "FAIL: $*"
	exit 1
}
tries=0
until ssh_ci edel settings export >out/system-export.toml 2>out/ssh.log; do
	tries=$((tries + 1))
	if [ "$tries" -ge 10 ]; then
		cat "$log" out/ssh.log
		echo "FAIL: ci could not log in over ssh and run edel settings export"
		exit 1
	fi
	sleep 3
done
cat "$log" out/system-export.toml
if grep -qx 'hostname = "ci-seeded"' out/system-export.toml &&
	grep -qx '\[users.ci\]' out/system-export.toml &&
	grep -qx 'admin = true' out/system-export.toml; then
	echo "PASS: seeded on first boot as ci-seeded; ci logged in with its key and exported the machine (${waited}s to the login prompt)"
else
	fail "the export lacks the seeded hostname or the admin user ci"
fi

# M2.3, as root.
without_hostname() { printf '%s\n' "$1" | grep -v '^hostname = '; }
. ci/names.sh
# The machine's file, through the link in /etc/edel where admins look.
link=$(ssh_root readlink "/etc/edel/$settings_name") || fail "no /etc/edel/$settings_name"
[ "$link" = "$data_dir/$settings_name" ] || fail "/etc/edel/$settings_name points at $link"
grep -q '^# Edel OS settings' out/system-export.toml || fail "export does not begin with its header comment"
before=$(ssh_root cat "/etc/edel/$settings_name")
ssh_root edel settings apply || fail "edel settings apply failed"
ssh_root edel settings diff >out/system-diff.log || fail "diff is not empty after apply: $(cat out/system-diff.log)"
ssh_root edel settings set network.hostname=other || fail "edel settings set failed"
after=$(ssh_root cat "/etc/edel/$settings_name")
printf '%s\n' "$after" | grep -qx 'hostname = "other"' || fail "set did not write the hostname"
[ "$(without_hostname "$before")" = "$(without_hostname "$after")" ] || fail "set changed more than the hostname line"
if ssh_root edel settings diff >out/system-diff.log; then
	fail "diff after set exited 0"
fi
[ "$(grep -c '^change: ' out/system-diff.log)" = 1 ] &&
	grep -qx 'change: network.hostname: set to other' out/system-diff.log ||
	fail "diff after set is not exactly the hostname: $(cat out/system-diff.log)"
ssh_root edel settings reset network.hostname || fail "edel settings reset failed"
ssh_root edel settings apply || fail "edel settings apply after reset failed"
now=$(ssh_root hostname)
[ "$now" = edel ] || fail "the hostname did not go back to the image's: it is $now"
final=$(ssh_root cat "/etc/edel/$settings_name")
printf '%s\n' "$final" | grep -qxF 'future.key = 1' || fail "reset lost future.key"
printf '%s\n' "$final" | grep -A1 -xF '# The person who runs CI' | grep -qxF '[users.ci]' ||
	fail "reset lost the comment above [users.ci]"
echo "PASS: diff empty after apply; set changed only the hostname; reset brought back edel; future.key and the comment kept"

# M5.25a: several settings in one set, a page in the app's words, --toml
# for scripts, a typo answered with the nearest key, and import.
ssh_root edel settings set network.hostname=two system.developer_mode=true || fail "set with two assignments failed"
ssh_root edel settings get network >out/settings-get.log || fail "edel settings get network failed"
grep -q '^Network (network)' out/settings-get.log && grep -q 'network.hostname  *"two"  (this machine)' out/settings-get.log ||
	fail "get network does not show the Network page's row: $(cat out/settings-get.log)"
ssh_root edel settings get system --toml >out/settings-get.toml || fail "edel settings get system --toml failed"
python3 -c 'import sys, tomllib; assert tomllib.load(open(sys.argv[1], "rb"))["system"]["developer_mode"] is True' out/settings-get.toml ||
	fail "get --toml did not read back: $(cat out/settings-get.toml)"
if ssh_root edel settings set network.hostnme=x >out/settings-typo.log 2>&1; then
	fail "set took the unknown key network.hostnme"
fi
grep -q 'did you mean network.hostname?' out/settings-typo.log || fail "a typo got no suggestion: $(cat out/settings-typo.log)"
ssh_root edel settings reset network.hostname system.developer_mode || fail "reset with two keys failed"
ssh_root edel settings export >out/settings-copy.toml || fail "export as root failed"
# After the reset and apply above, the file names no hostname.
grep -q '^\[network\]' out/settings-copy.toml && fail "the export still names a hostname: $(cat out/settings-copy.toml)"
printf '\n[network]\nhostname = "imported"\n' >>out/settings-copy.toml
ssh_root 'cat >/tmp/copy.toml' <out/settings-copy.toml || fail "copying the exported file failed"
ssh_root edel settings import /tmp/copy.toml || fail "edel settings import failed"
[ "$(ssh_root hostname)" = imported ] || fail "import did not apply the file's hostname"
ssh_root edel settings diff >out/system-diff.log || fail "diff is not empty after import: $(cat out/system-diff.log)"
echo "PASS: /etc/edel/$settings_name links the machine's file, export begins with its header, set took two settings, get showed the Network page and read back as TOML, a typo got the nearest key, and import applied an exported file"

# M3.3b: edel report prints TOML a reader can parse, with this boot's line.
ssh_root edel report >out/report.toml || fail "edel report failed"
python3 -c '
import sys, tomllib
r = tomllib.load(open("out/report.toml", "rb"))
assert r["format"] == 1 and r["version"] and r["kernel"], "release or kernel missing"
assert r["started"].startswith("Started in "), "no boot line"
assert r["pci"] and all(d["driver"] for d in r["pci"]), "no PCI devices"
assert "Linux version" in r["dmesg"], "no kernel log"
# M4.0: the image carries the files of the features ci/vm/vm.toml lists.
assert r["features"] == ["ab-boot", "base", "kernel", "machine", "network", "seat", "ssh", "udev"], "features: %s" % r["features"]
assert r["feature_notes"] == [], "feature notes: %s" % r["feature_notes"]
print("report:", r["version"], r["kernel"], len(r["pci"]), "PCI devices,", r["memory_in_use_mib"], "MiB in use, features", " ".join(r["features"]))
' || fail "edel report printed no valid report: $(head -c 400 out/report.toml)"
echo "PASS: edel report printed the release, kernel, features, boot line, PCI devices and kernel log as TOML"

# M6.5: doas and the administrator's password. As root: ci gets a password,
# and visitor is added with no admin rights. Each check then runs as the user
# named, and a refusal must exit non-zero and say why.
# ssh joins its arguments into one command line for the remote shell, so
# each command that needs quoting goes as one quoted string.
ssh_root 'echo "ci:edel-ci-test" | chpasswd' >out/doas-setup.log 2>&1 ||
	fail "could not give ci a password: $(cat out/doas-setup.log)"
ssh_root 'adduser -D visitor' >out/doas-setup.log 2>&1 ||
	fail "could not add the user visitor: $(cat out/doas-setup.log)"
ssh_ci doas -n edel status >out/doas-status.log 2>&1 || fail "ci could not run doas -n edel status: $(cat out/doas-status.log)"
grep -q '^running: ' out/doas-status.log || fail "doas -n edel status printed no status: $(cat out/doas-status.log)"
echo "PASS: ci (in group admin) ran doas -n edel status with no password and got the status"
ssh_ci sudo -n edel status >out/sudo-status.log 2>&1 || fail "ci could not run sudo -n edel status: $(cat out/sudo-status.log)"
grep -q '^running: ' out/sudo-status.log || fail "sudo -n edel status printed no status: $(cat out/sudo-status.log)"
echo "PASS: sudo, doas's second name, ran edel status for ci with the same rule"
if ssh_root "su -s /bin/sh visitor -c 'doas -n edel status'" >out/doas-visitor.log 2>&1; then
	fail "visitor, who is not in group admin, ran doas -n edel status"
fi
grep -q 'not permitted' out/doas-visitor.log || fail "visitor's doas -n edel status failed, but not by doas's refusal: $(cat out/doas-visitor.log)"
echo "PASS: visitor, who is not in group admin, was refused by doas: $(head -c 200 out/doas-visitor.log)"
code=0
ssh_ci 'echo wrong | doas -n edel rollback --password-stdin' >out/doas-wrong.log 2>&1 || code=$?
[ "$code" = 1 ] || fail "a wrong password exited $code, not 1: $(cat out/doas-wrong.log)"
grep -q 'wrong password for ci' out/doas-wrong.log || fail "a wrong password was not said: $(cat out/doas-wrong.log)"
echo "PASS: rollback with a wrong password exited 1 and said wrong password for ci"
code=0
ssh_ci 'echo edel-ci-test | doas -n edel update --password-stdin /nonexistent/release.toml' >out/doas-update.log 2>&1 || code=$?
[ "$code" != 0 ] || fail "update with a missing release exited 0"
grep -qi 'release' out/doas-update.log && ! grep -qi 'password' out/doas-update.log ||
	fail "update with the right password did not get as far as the release: $(cat out/doas-update.log)"
echo "PASS: update with the right password got as far as reading the release, and failed there: $(head -c 200 out/doas-update.log)"
code=0
ssh_ci 'doas -n edel update --unsigned /nonexistent.img' >out/doas-unsigned.log 2>&1 || code=$?
[ "$code" != 0 ] && grep -q 'developer mode' out/doas-unsigned.log ||
	fail "update --unsigned without developer mode was not refused by name: $(cat out/doas-unsigned.log)"
echo "PASS: update --unsigned was refused because developer mode is off"

# M6.10: [services]. A feature that cannot be switched is refused by name,
# with the ones that can; then ssh switched off stops sshd and takes it
# out of the default runlevel, so the next connection is refused. The VM
# switches ssh on again by itself 15 s later (a detached sleep, so it
# survives the ssh session that started it, which sshd's stop may cut),
# and the test reaches it again: services.ssh=true starts sshd without a
# reboot.
code=0
ssh_root edel settings set services.udev=false >out/services-bad.log 2>&1 || code=$?
[ "$code" != 0 ] && grep -q 'udev is not a feature that can be switched' out/services-bad.log &&
	grep -q 'ssh' out/services-bad.log ||
	fail "services.udev=false was not refused with the features that can be switched: $(cat out/services-bad.log)"
ssh_root 'edel settings set services.ssh=false && edel settings apply >/tmp/services-off.log 2>&1; cat /tmp/services-off.log; nohup sh -c "sleep 15; edel settings set services.ssh=true && edel settings apply >/tmp/services-on.log 2>&1" >/dev/null 2>&1 &' >out/services-off.log 2>&1 || true
grep -q 'services: stop sshd and take it out of the default runlevel' out/services-off.log ||
	fail "apply did not say it stopped sshd: $(cat out/services-off.log)"
sleep 2
if ssh_ci true >out/services-ssh.log 2>&1; then
	fail "ssh still answered after services.ssh=false"
fi
# Up to 60 s, every 3 s, for the VM's own services.ssh=true (M6.10).
back=0
until ssh_ci true >out/services-back.log 2>&1; do
	back=$((back + 3))
	if [ "$back" -ge 60 ]; then
		fail "ssh did not come back within 60 s of services.ssh=true, without a reboot"
	fi
	sleep 3
done
ssh_root 'cat /tmp/services-on.log; rc-status default' >out/services-on.log 2>&1 || true
grep -E 'sshd.*started' out/services-on.log >/dev/null ||
	fail "services.ssh=true did not start sshd in the default runlevel: $(cat out/services-on.log)"
echo "PASS: services.udev=false was refused naming the features that can be switched, and services.ssh=false stopped sshd, which then refused the next connection; services.ssh=true brought it back without a reboot, sshd started in the default runlevel"
