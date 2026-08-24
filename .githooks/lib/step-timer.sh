# R1799 — what each pre-push step COSTS, printed by the hook itself.
#
# ## Why the hook has to do this and a wrapper cannot
#
# `debt-the-push-hook-has-never-been-profiled` records three failed attempts to
# measure this from outside, and the third explains the other two: piping the
# hook's output through `awk` gives every line the same timestamp, because the
# buffering is not in `awk` — it is in each TOOL the hook runs, which buffers
# when its stdout is a pipe rather than a terminal. Per-step attribution from a
# pipe is therefore impossible in principle. The only instrument that works is
# the hook stamping its own steps, which is this.
#
# ## What it is careful not to claim
#
# A per-step number is NOT "what removing this step would save". That debt says
# so explicitly and names the measurement: `wire_only_deps.py` costs 0.27s warm
# and 119s cold, 440x, because the steps warm each other's build. Removing one
# hands its share to the next. These numbers say where the time GOES, which is
# the question nobody could answer; what to move is a separate judgement that
# needs the CI side measured too.
#
# ## And one sample is not a measurement here
#
# Three runs of the same warm tree measured 366, 368 and 251 seconds — a spread
# of 115s, larger than the biggest single step. Every step therefore prints its
# own cost, so several runs can be collected and compared rather than one run
# being read as the answer.

# Milliseconds since the epoch. `date +%s%N` is GNU; the fallback keeps the
# hook working where it is not, at second resolution, rather than printing
# nonsense.
_pinion_now_ms() {
    local ns
    ns=$(date +%s%N 2>/dev/null) || ns=""
    case "$ns" in
        '' | *[!0-9]*) echo $(( $(date +%s) * 1000 )) ;;
        *) echo $(( ns / 1000000 )) ;;
    esac
}

_PINION_STEP_NAME=""
_PINION_STEP_AT=0
_PINION_RUN_AT=$(_pinion_now_ms)
_PINION_STEP_TOTAL=0
_PINION_STEP_COUNT=0
_PINION_SUMMARY_DONE=0
_PINION_AT_EXIT=()

# Close the step that is open, if any, and print what it cost.
_pinion_close_step() {
    [ -z "$_PINION_STEP_NAME" ] && return 0
    local now cost
    now=$(_pinion_now_ms)
    cost=$(( now - _PINION_STEP_AT ))
    _PINION_STEP_TOTAL=$(( _PINION_STEP_TOTAL + cost ))
    _PINION_STEP_COUNT=$(( _PINION_STEP_COUNT + 1 ))
    printf 'pre-push:   [%4d.%03ds] %s\n' \
        "$(( cost / 1000 ))" "$(( cost % 1000 ))" "$_PINION_STEP_NAME" >&2
    _PINION_STEP_NAME=""
}

# Announce a step and start its clock.
#
# ★ This REPLACES the bare `echo "pre-push: <name> ..." >&2` the hook wrote at
# each step rather than sitting beside it. A timer added alongside the
# announcements would be a second list of the steps, and two lists of one thing
# is what this session has spent three rounds repairing.
step() {
    _pinion_close_step
    _PINION_STEP_NAME="$1"
    _PINION_STEP_AT=$(_pinion_now_ms)
    echo "pre-push: $1 ..." >&2
}

# Called once, from the EXIT trap below. Prints the total, how much of it the
# named steps accounted for, and — the number this instrument exists for — how
# much they did not.
#
# Idempotent: the trap is the only caller today, but a hook that also called it
# explicitly must not get two summaries, and two summaries would be read as two
# runs.
step_summary() {
    [ "$_PINION_SUMMARY_DONE" = 1 ] && return 0
    _PINION_SUMMARY_DONE=1
    _pinion_close_step
    # A process that SOURCED this library without timing anything must not
    # print a hook summary — `tools/test_hooks.sh` sources it to test it, and a
    # line reading `pre-push: 0 step(s)` in a test run would be a hook report
    # that no hook made. Checked after closing, so a hook that dies inside its
    # very first step still reports that step.
    [ "$_PINION_STEP_COUNT" -eq 0 ] && return 0
    local total unattributed
    total=$(( $(_pinion_now_ms) - _PINION_RUN_AT ))
    unattributed=$(( total - _PINION_STEP_TOTAL ))
    printf 'pre-push: %d step(s), %d.%03ds total, %d.%03ds named, %d.%03ds unattributed\n' \
        "$_PINION_STEP_COUNT" \
        "$(( total / 1000 ))" "$(( total % 1000 ))" \
        "$(( _PINION_STEP_TOTAL / 1000 ))" "$(( _PINION_STEP_TOTAL % 1000 ))" \
        "$(( unattributed / 1000 ))" "$(( unattributed % 1000 ))" >&2
}

# --- Which runs does this instrument report on? --------------------------
#
# ★★★★★ Its first draft answered "the ones that SUCCEED", which is the wrong
# half. A step prints its cost when the NEXT step opens, so any early `exit 1`
# — and pre-push has 34 of them — dropped the open step's cost AND the summary.
# The reader who most needs to know which gate they just waited six minutes for
# is the reader whose push was then refused. So the summary has to run on every
# exit path, which means a trap.
#
# ★★ And the trap has to COMPOSE, for a reason this repository has already
# measured and written down: `tools/test_hooks.sh` records at R1522 that
# `trap ... EXIT` REPLACES the installed handler instead of adding to it. bash
# has one EXIT slot. pre-push installs a temp-file cleanup trap ~300 lines
# after it sources this file, so a bare `trap step_summary EXIT` here would
# have been silently replaced — and the instrument would have gone on reporting
# on successful pushes only while I believed it covered all of them. A gate
# that cannot fail and an instrument that cannot see are the same defect.
#
# So the timer owns the slot from the moment it is sourced (covering every one
# of those 34 exits, including the ones that fire before the hook would have
# installed anything) and callers register cleanups THROUGH it.
step_at_exit() {
    _PINION_AT_EXIT+=("$1")
}

_pinion_at_exit() {
    local rc=$? cmd
    # Cleanups first: the summary is the last thing a reader should see.
    for cmd in ${_PINION_AT_EXIT[@]+"${_PINION_AT_EXIT[@]}"}; do
        eval "$cmd" || true
    done
    step_summary
    return "$rc"
}

trap _pinion_at_exit EXIT
