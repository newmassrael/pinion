# shellcheck shell=bash
# lib/phase-b-tally.sh — speak the Phase B tally, and check the tool that speaks.
#
# R1519 made the progress figure a tool (`tools/phase_b_tally.py`) so its
# staleness would be mechanical rather than a slogan, and taught this hook to
# print it every push. R1522 found two holes in that arrangement, both of the
# shape the tool itself exists to catch:
#
#   1. THE TOOL'S OWN CHECK WAS NEVER RUN. `--selftest` existed from R1519 and
#      lived only in the module docstring's usage line, so whether it had ever
#      executed was a matter of who remembered to type it — the same "a prose
#      warning is not a gate" that `[[r1470-paint-test-opened-the-speakers]]`
#      records. It is pure python and finishes in milliseconds; there was never
#      a cost reason not to run it here.
#
#   2. THE REPORT COULD GO SILENT. The R1519 call was
#      `python3 tally.py 2>/dev/null | grep -E ... || true`, which discards
#      stderr, discards the exit code and discards a grep miss. A crashing or
#      renamed tool printed exactly nothing, and nothing-printed is
#      indistinguishable from a tree that stopped drifting — which is precisely
#      the failure mode (`~56%` held for 587 rounds) that the tool was built to
#      end. The check would have stopped happening silently.
#
# So: run the selftest first, and if it fails, say so and WITHHOLD the numbers.
# A tool that failed its own check has not earned the right to have its figures
# quoted. Never fail the push, though — for the same reason the cache budget
# does not: a self-estimate going stale, or its tooling breaking, is not a
# reason to refuse to publish code. The value is that it is spoken out loud
# every time.
#
# Reports on stdout with a `pre-push: ` prefix. Returns 0 always.

report_phase_b_tally() {
    local repo_root="${1:-}"
    local tool="$repo_root/tools/phase_b_tally.py"

    # The tally is optional tooling; a tree without it is not a defect. This is
    # the one silence that is correct, and it is asserted in tools/test_hooks.sh
    # so it cannot quietly widen to cover the broken cases below.
    [[ -f "$tool" ]] || return 0

    if ! command -v python3 >/dev/null 2>&1; then
        printf 'pre-push: phase-b tally skipped: no python3 on PATH\n'
        return 0
    fi

    local self_out self_rc
    self_out="$(python3 "$tool" --selftest 2>&1)" && self_rc=0 || self_rc=$?
    [[ -n "$self_out" ]] || self_out="(the selftest printed nothing at all)"
    printf '%s\n' "$self_out" | sed 's/^/pre-push: tally /'
    if [[ "$self_rc" -ne 0 ]]; then
        printf 'pre-push: phase-b tally selftest FAILED (exit %s):' "$self_rc"
        printf ' numbers withheld, the tool failed its own check\n'
        return 0
    fi

    # `report` exits 1 when an axis is STALE. That is a finding to print, not a
    # reason to refuse a push, so the status is deliberately ignored here — but
    # the OUTPUT is not, which is the difference from the R1519 call.
    local out lines
    out="$(python3 "$tool" 2>&1)" || true
    lines="$(printf '%s\n' "$out" | grep -E '^(weighted|STALE|UNCLASSIFIED|PROBE)' || true)"
    if [[ -n "$lines" ]]; then
        printf '%s\n' "$lines" | sed 's/^/pre-push: /'
        return 0
    fi

    # Silence is the one outcome this must not have.
    printf 'pre-push: phase-b tally produced no summary line; raw output:\n'
    [[ -n "$out" ]] || out="(no output on stdout or stderr)"
    printf '%s\n' "$out" | sed 's/^/pre-push: /'
}
