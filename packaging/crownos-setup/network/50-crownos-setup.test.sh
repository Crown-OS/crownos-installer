#!/bin/sh
# Runs the NetworkManager hook with a fake systemctl and checks when it
# starts the runner. Usage: sh network/50-crownos-setup.test.sh
set -eu

hook="$(cd "$(dirname "$0")" && pwd)/50-crownos-setup"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

cat >"$scratch/systemctl" <<'FAKE'
#!/bin/sh
if [ "$1" = is-active ]; then
	echo "$FAKE_UNIT_STATE"
else
	echo "$*" >>"$FAKE_SYSTEMCTL_LOG"
fi
FAKE
chmod +x "$scratch/systemctl"

failures=0
expect() {
	description="$1" expected="$2" unit_state="$3" action="$4" connectivity="$5"
	log="$scratch/log"
	: >"$log"
	PATH="$scratch:$PATH" FAKE_SYSTEMCTL_LOG="$log" FAKE_UNIT_STATE="$unit_state" \
		CONNECTIVITY_STATE="$connectivity" sh "$hook" wlan0 "$action"
	if [ -s "$log" ]; then started=yes; else started=no; fi
	if [ "$started" = "$expected" ]; then
		echo "ok: $description"
	else
		echo "FAILED: $description (started: $started)"
		failures=$((failures + 1))
	fi
}

expect "up starts an inactive runner" yes inactive up ""
expect "full connectivity starts an inactive runner" yes inactive connectivity-change FULL
expect "limited connectivity does nothing" no inactive connectivity-change LIMITED
expect "a lost connection does nothing" no inactive connectivity-change NONE
expect "down does nothing" no inactive down ""
expect "a runner in its restart back-off keeps waiting" no activating up ""
expect "a running runner gets a no-op start" yes active up ""

[ "$failures" -eq 0 ]
