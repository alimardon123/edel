# Shared by the CI scripts that check budgets (sourced, not run; M8.15a). It
# reads ci/budgets.toml for every script that checks a budget, and checks the
# walks a newcomer takes (docs/REVIEW-experience.md) against their
# walk_NAME_seconds and walk_NAME_steps budgets. Sourced functions share the
# caller's variables, so every variable a function sets starts with walk_ or
# budget_.

# budget KEY: the number KEY has in ci/budgets.toml.
budget() {
	awk -F= -v key="$1" '{ k = $1; gsub(/ /, "", k) } k == key { v = $2; sub(/#.*/, "", v); gsub(/ /, "", v); print v }' ci/budgets.toml
}

# walk_boot_seconds LOG: the seconds a boot in LOG took from the kernel
# running /init to the `Started in` line, Edel OS's own part of the start.
# The kernel's part before it is Alpine's, the same for every change, and
# it is what a slow runner stretches most (a fresh lane's kernel reached
# /init a quarter later than usual on 2026-10-24 and its walk failed), so
# a walk times only what a change can make slower. Prints nothing when
# either line is missing, which walk then fails.
walk_boot_seconds() {
	tr -d '\r' <"$1" | awk '
		init == "" && /\] Run \/init as init process/ { s = $0; sub(/\] Run \/init.*/, "", s); sub(/.*\[ */, "", s); init = s }
		started == "" && /Started in [0-9.]+ s/ { s = $0; sub(/.*Started in /, "", s); sub(/ s.*/, "", s); started = s }
		END { if (init != "" && started != "") printf "%.2f\n", started - init }'
}

# Every key press, typed word or click a person makes on a walk goes through
# press, so the steps are counted where they are made.
walk_steps=0

# press ARGS...: one thing a person does on a walk (a key, a typed word, a
# click), counted as a step, then sent to the VM's console by ci/qmp.py.
press() {
	walk_steps=$((walk_steps + 1))
	python3 ci/qmp.py "$@"
}

# walk NAME MEASURE=VALUE...: checks one walk. MEASURE is seconds or steps;
# the measures are checked in the order given, and each is compared with
# walk_NAME_MEASURE (NAME's dashes become underscores). Returns 1 when a
# value is missing or not a number, has no budget, or is over its budget.
walk() {
	walk_name=$1
	shift
	walk_id=$(printf '%s' "$walk_name" | tr '-' '_')
	walk_said=''
	walk_over=0
	for walk_pair in "$@"; do
		walk_unit=${walk_pair%%=*}
		walk_value=${walk_pair#*=}
		case $walk_unit in
		seconds | steps) ;;
		*)
			echo "FAIL: walk $walk_name has no measure $walk_unit; use seconds or steps"
			return 1
			;;
		esac
		case $walk_value in
		'' | . | *[!0-9.]* | *.*.*)
			echo "FAIL: walk $walk_name measured $walk_unit $walk_value, not a time or count"
			return 1
			;;
		esac
		walk_limit=$(budget "walk_${walk_id}_${walk_unit}")
		if [ -z "$walk_limit" ]; then
			echo "FAIL: walk $walk_name has no budget walk_${walk_id}_${walk_unit} in ci/budgets.toml; add one from this measurement, $walk_value"
			return 1
		fi
		if ! awk -v m="$walk_value" -v b="$walk_limit" 'BEGIN { exit !(m <= b) }'; then
			walk_over=1
		fi
		walk_said="$walk_said${walk_said:+, }$walk_value $walk_unit (budget $walk_limit)"
	done
	if [ "$walk_over" = 1 ]; then
		walk_verdict='**over**'
	else
		walk_verdict=ok
	fi
	echo "walk $walk_name: $walk_said, $walk_verdict"
	if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
		echo "Walk \`$walk_name\` (M8.15a): $walk_said, $walk_verdict." >>"$GITHUB_STEP_SUMMARY"
	fi
	[ "$walk_over" = 0 ]
}
