#!/usr/bin/env bash
# R1495 — tests for the githook libraries.
#
# The hooks are the most load-bearing code in the repo that nothing verified:
# they decide what becomes shared, they had grown to three libraries, and the
# measurement that opened this round found zero tests over any of them. A gate
# nobody tests is the thing this round is about, so the gate added here is
# tested here, and the sibling that has been rejecting commit messages all
# along gets the cases that actually bit.
#
# Deliberately plain bash: the things under test are bash, the harness has to
# run inside `pre-push` in well under a second, and a test framework would be a
# dependency the hooks themselves do not have.
#
# Run directly (`tools/test_hooks.sh`) or let `pre-push` run it.

set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../.githooks/lib/ci-status.sh
source "$repo_root/.githooks/lib/ci-status.sh"
# shellcheck source=../.githooks/lib/commit-msg-lint.sh
source "$repo_root/.githooks/lib/commit-msg-lint.sh"

pass=0
fail=0

ok() {
    local desc="$1" got="$2" want="$3"
    if [[ "$got" == "$want" ]]; then
        pass=$((pass + 1))
    else
        fail=$((fail + 1))
        printf 'FAIL: %s\n  want: %q\n  got:  %q\n' "$desc" "$want" "$got" >&2
    fi
}

# `gh run list` emits tab-separated columns:
#   STATUS, CONCLUSION, TITLE, WORKFLOW, BRANCH, EVENT, ID, ELAPSED, AGE
row() {
    printf '%s\t%s\tsome commit\tCI\tmain\tpush\t%s\t10m\t1h' "$1" "$2" "$3"
}

# ---------------------------------------------------------------------------
# ci_verdict_from_listing — the parse, which is the whole risk surface
# ---------------------------------------------------------------------------

ok "a successful run is green" \
   "$(ci_verdict_from_listing "$(row completed success 111)" main)" \
   "green 111"

ok "a failed run is red" \
   "$(ci_verdict_from_listing "$(row completed failure 222)" main)" \
   "red 222"

# The case this round was written in the middle of: two runs in flight on top
# of a green one. An in-progress run has judged nothing, so the verdict the
# base inherits is still the last one that finished.
in_flight_over_green="$(printf '%s\n%s\n%s' \
    "$(row in_progress '' 333)" "$(row in_progress '' 334)" \
    "$(row completed success 335)")"
ok "in-progress runs are skipped for the last COMPLETED verdict" \
   "$(ci_verdict_from_listing "$in_flight_over_green" main)" \
   "green 335"

# And the same shape over a red base — the reason the skip cannot simply
# answer "green when unsure".
in_flight_over_red="$(printf '%s\n%s' \
    "$(row in_progress '' 444)" "$(row completed failure 445)")"
ok "an in-progress run does not hide the red underneath it" \
   "$(ci_verdict_from_listing "$in_flight_over_red" main)" \
   "red 445"

# Newest-first ordering: an older green must not outvote a newer red.
red_then_green="$(printf '%s\n%s' \
    "$(row completed failure 555)" "$(row completed success 556)")"
ok "the most recent completed run decides, not the best one" \
   "$(ci_verdict_from_listing "$red_then_green" main)" \
   "red 555"

ok "no runs at all is unknown, not red" \
   "$(ci_verdict_from_listing "" main)" \
   "unknown"

ok "only in-progress runs is unknown, not red" \
   "$(ci_verdict_from_listing "$(row in_progress '' 666)" main)" \
   "unknown"

# The branch filter lives in the parse because `gh run list --branch` does not
# exist in gh 2.4.0 — see the lib. So it needs its own coverage: another
# branch's red must not stop a push, and must not be mistaken for this one.
other_branch="$(printf '%s\t%s\tsome commit\tCI\t%s\tpush\t%s\t10m\t1h' \
    completed failure feature-x 888)"
ok "another branch's red is not this branch's verdict" \
   "$(ci_verdict_from_listing "$other_branch" main)" \
   "unknown"

mixed="$(printf '%s\n%s' "$other_branch" "$(row completed success 889)")"
ok "the scan walks past other branches to this one" \
   "$(ci_verdict_from_listing "$mixed" main)" \
   "green 889"

# GitHub has conclusions other than success/failure. None of them is a
# verdict this project may build on top of unexamined.
for conclusion in cancelled timed_out startup_failure action_required; do
    ok "'$conclusion' is not a green light" \
       "$(ci_verdict_from_listing "$(row completed "$conclusion" 777)" main)" \
       "red 777"
done

# ---------------------------------------------------------------------------
# check_last_ci_run — the decision, with `gh` stubbed
# ---------------------------------------------------------------------------

# A `gh` that answers only the invocation the real one accepts.
#
# The first draft of this stub took any arguments at all, and that is exactly
# why it certified a lib that called `gh run list --branch main`, a flag gh
# 2.4.0 does not have. The real `gh` answers that with usage text and no rows,
# which the caller reads as "no runs yet" — so the gate fail-opened on every
# push and the tests said it was fine. A stub more permissive than the thing
# it stands in for does not test the caller; it tests the stub. This one
# rejects any flag it was not taught, so a lib that grows a new one fails here
# instead of in production.
stub_gh() {
    local listing="$1"
    _stub_dir="$(mktemp -d)"
    {
        echo '#!/usr/bin/env bash'
        # SC2016 is the point: these are the STUB's parameters, written into
        # the file verbatim, not this shell's to expand.
        # shellcheck disable=SC2016
        cat <<'STUB_HEAD'
[[ "$1" == "run" && "$2" == "list" ]] || exit 1
shift 2
while (( $# )); do
    case "$1" in
        --limit) shift 2 ;;
        *) echo "unknown flag: $1" >&2; exit 0 ;;
    esac
done
STUB_HEAD
        printf 'cat <<%s\n%s\n%s\n' "EOF_STUB" "$listing" "EOF_STUB"
    } >"$_stub_dir/gh"
    chmod +x "$_stub_dir/gh"
    PATH="$_stub_dir:$PATH"
}

with_stub() {
    local listing="$1"
    shift
    ( stub_gh "$listing"; "$@" >/dev/null 2>&1; echo $? )
}

ok "a green base publishes" \
   "$(with_stub "$(row completed success 111)" check_last_ci_run main test)" \
   "0"

ok "a red base refuses" \
   "$(with_stub "$(row completed failure 222)" check_last_ci_run main test)" \
   "1"

ok "an unknown base publishes (nothing to inherit)" \
   "$(with_stub "" check_last_ci_run main test)" \
   "0"

# The override exists so the FIX for a red base can be published. Without it a
# stop-the-line rule stops the line permanently.
ok "the override publishes onto a red base" \
   "$(PINION_PUSH_ON_RED=1 with_stub "$(row completed failure 222)" \
        check_last_ci_run main test)" \
   "0"

# ...and only for the exact value, so a stray `PINION_PUSH_ON_RED=0` in a
# shell profile cannot quietly disarm the gate.
ok "the override is not armed by any other value" \
   "$(PINION_PUSH_ON_RED=0 with_stub "$(row completed failure 222)" \
        check_last_ci_run main test)" \
   "1"

# Fail-open on infrastructure absence: no `gh`, no verdict, but publishing is
# not what is unsafe.
# SC2123 is the point: emptying the search path is how "there is no gh on
# this machine" is reproduced, and it is scoped to the subshell.
# shellcheck disable=SC2123
ok "no gh on PATH publishes" \
   "$( (PATH=/nonexistent; check_last_ci_run main test >/dev/null 2>&1; echo $?) )" \
   "0"

# ---------------------------------------------------------------------------
# commit-msg-lint — the sibling that has been enforcing rules untested
# ---------------------------------------------------------------------------

lint() {
    local f
    f="$(mktemp)"
    printf '%s' "$1" >"$f"
    lint_commit_message "$f" >/dev/null 2>&1
    local rc=$?
    rm -f "$f"
    echo "$rc"
}

ok "a conforming message passes" \
   "$(lint 'feat(widgets): R1 a short subject

- one bullet
- another bullet')" \
   "0"

# Both of the rules that rejected a commit during this session's rounds.
ok "a subject over 72 bytes is rejected" \
   "$(lint 'feat(widgets): R1493 a section says the size it has, not only the one given

- a bullet')" \
   "1"

ok "an indented continuation line is rejected" \
   "$(lint 'feat(widgets): R1 a short subject

- a bullet that wraps
  onto a second line')" \
   "1"

printf '[hooks] %d passed, %d failed\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]]
