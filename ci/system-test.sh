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
[ "$link" = "/data/edel/$settings_name" ] || fail "/etc/edel/$settings_name points at $link"
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
[ "$(ssh_root hostname)" = edel ] || fail "the hostname did not go back to the image's"
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
# M4.0: the image carries the files of the features vm.toml lists.
assert r["features"] == ["ab-boot", "base", "machine", "mdev", "ssh", "vm"], "features: %s" % r["features"]
assert r["feature_notes"] == [], "feature notes: %s" % r["feature_notes"]
print("report:", r["version"], r["kernel"], len(r["pci"]), "PCI devices,", r["memory_in_use_mib"], "MiB in use, features", " ".join(r["features"]))
' || fail "edel report printed no valid report: $(head -c 400 out/report.toml)"
echo "PASS: edel report printed the release, kernel, features, boot line, PCI devices and kernel log as TOML"
