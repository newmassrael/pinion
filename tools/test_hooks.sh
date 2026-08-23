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
# (R1500) SCRIPTDIR, not a CWD-relative path: the `source=../...` R1495 wrote
# resolved against the working directory, so `shellcheck -x` from the repo root
# — where every other gate in this project runs — could not find either library
# and reported SC1091 for both. The libraries went unchecked and the round that
# added them recorded the run as clean.
# shellcheck source=SCRIPTDIR/../.githooks/lib/ci-status.sh
source "$repo_root/.githooks/lib/ci-status.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/commit-msg-lint.sh
source "$repo_root/.githooks/lib/commit-msg-lint.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/mnemosyne-tool.sh
source "$repo_root/.githooks/lib/mnemosyne-tool.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/target-budget.sh
source "$repo_root/.githooks/lib/target-budget.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/phase-b-tally.sh
source "$repo_root/.githooks/lib/phase-b-tally.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/consumer-tests.sh
source "$repo_root/.githooks/lib/consumer-tests.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/worktree-guard.sh
source "$repo_root/.githooks/lib/worktree-guard.sh"
# shellcheck source=SCRIPTDIR/../.githooks/lib/ssh-keepalive.sh
source "$repo_root/.githooks/lib/ssh-keepalive.sh"

pass=0
fail=0

# Every temp tree this suite creates lives under ONE root, removed by ONE exit
# trap (R1522). Two things forced this shape, both measured rather than guessed:
#
#   * `trap ... EXIT` REPLACES the handler instead of adding to it, so a trap
#     per section silently orphans the earlier sections' trees;
#   * a registry of paths does not work either, because `stub_gh` is called
#     inside `$(...)` — the append lands in a subshell's copy of the array and
#     the parent's trap never learns the path. Counting /tmp across a run is
#     what showed this: the registry version still leaked exactly one directory
#     per `stub_gh` call, as it had since R1495.
#
# Containment survives subshells; bookkeeping does not.
_tmp_root="$(mktemp -d)"
trap 'rm -rf "$_tmp_root"' EXIT

mktemp_tracked() { mktemp -d -p "$_tmp_root"; }

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
    _stub_dir="$(mktemp_tracked)"
    {
        echo '#!/usr/bin/env bash'
        # SC2016 is the point: these are the STUB's parameters, written into
        # the file verbatim, not this shell's to expand.
        # shellcheck disable=SC2016
        cat <<'STUB_HEAD'
if [[ "$1" == "api" ]]; then
    # R1579 — the run-count probe. Answers from PINION_STUB_RUN_COUNT so a
    # case states the fact it is asserting about; `-` means "gh failed".
    shift
    [[ "$1" == repos/:owner/:repo/actions/runs\?head_sha=* ]] || exit 1
    shift
    [[ "$1" == "--jq" && "$2" == ".total_count" ]] || exit 1
    [[ "${PINION_STUB_RUN_COUNT:-0}" == "-" ]] && exit 1
    echo "${PINION_STUB_RUN_COUNT:-0}"
    exit 0
fi
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

# (R1500) `$1` is the `PINION_PUSH_ON_RED` posture, and it is MANDATORY: `-`
# unsets the override, anything else is its literal value. Every case therefore
# states the environment it is asserting about, and none can inherit one.
#
# R1495 wrote these tests taking the variable from whatever the caller had, and
# set it only on the two cases that wanted it armed. That was invisible until
# the override was used for the purpose it was built for — `PINION_PUSH_ON_RED=1
# git push`, publishing the fix for a red base. `pre-push` runs this file before
# trusting its own libraries, inherits the exported value, and "a red base
# refuses" gets 0 instead of 1: the escape hatch disarmed the test that guards
# the rule it escapes, and the push was refused for the wrong reason. A gate
# whose documented escape hatch cannot be used has no escape hatch.
#
# A test that reads ambient state is testing the environment, not the code —
# the R1476 "state the premise by construction" rule, applied to a shell fixture.
with_stub() {
    local override="$1" listing="$2"
    shift 2
    # SC2030/SC2031 are the POINT: the export is scoped to this subshell, which
    # is how one case's posture is kept out of the next one's.
    # shellcheck disable=SC2030
    (
        if [[ "$override" == "-" ]]; then
            unset PINION_PUSH_ON_RED
        else
            export PINION_PUSH_ON_RED="$override"
        fi
        stub_gh "$listing"
        "$@" >/dev/null 2>&1
        echo $?
    )
}

ok "a green base publishes" \
   "$(with_stub - "$(row completed success 111)" check_last_ci_run main test)" \
   "0"

ok "a red base refuses" \
   "$(with_stub - "$(row completed failure 222)" check_last_ci_run main test)" \
   "1"

ok "an unknown base publishes (nothing to inherit)" \
   "$(with_stub - "" check_last_ci_run main test)" \
   "0"

# The override exists so the FIX for a red base can be published. Without it a
# stop-the-line rule stops the line permanently.
ok "the override publishes onto a red base" \
   "$(with_stub 1 "$(row completed failure 222)" check_last_ci_run main test)" \
   "0"

# ...and only for the exact value, so a stray `PINION_PUSH_ON_RED=0` in a
# shell profile cannot quietly disarm the gate.
ok "the override is not armed by any other value" \
   "$(with_stub 0 "$(row completed failure 222)" check_last_ci_run main test)" \
   "1"

# (R1500) The override changes the DECISION, not the verdict: a red base under
# an armed override still says so, loudly, naming the run. Asserted on the
# message because the exit code cannot tell "published because green" from
# "published because overridden" — and that indistinguishability is what let
# R1495's leak turn a red base into a silent green one.
with_stub_stderr() {
    local override="$1" listing="$2"
    shift 2
    # shellcheck disable=SC2031
    (
        export PINION_PUSH_ON_RED="$override"
        stub_gh "$listing"
        # Stderr only: the verdict is what is being read, and the listing the
        # stub prints on stdout is not.
        { "$@" >/dev/null; } 2>&1
    )
}

ok "an armed override still reports the base as red" \
   "$(with_stub_stderr 1 "$(row completed failure 222)" check_last_ci_run main test \
        | grep -c 'FAILED (run 222)')" \
   "1"

ok "and says which rule let it through" \
   "$(with_stub_stderr 1 "$(row completed failure 222)" check_last_ci_run main test \
        | grep -c 'PINION_PUSH_ON_RED=1')" \
   "1"

# Fail-open on infrastructure absence: no `gh`, no verdict, but publishing is
# not what is unsafe.
# SC2123 is the point: emptying the search path is how "there is no gh on
# this machine" is reproduced, and it is scoped to the subshell.
# shellcheck disable=SC2123
ok "no gh on PATH publishes" \
   "$( (unset PINION_PUSH_ON_RED; PATH=/nonexistent; \
        check_last_ci_run main test >/dev/null 2>&1; echo $?) )" \
   "0"

# ---------------------------------------------------------------------------
# R1579 — check_base_ci_coverage: a run's ABSENCE is not a run's success
#
# The defect this closes was observed, not imagined: on 2026-08-06 one push to
# this repo waited half an hour for its run and another had none forty-five
# minutes on, while `check_last_ci_run` read an earlier commit's green and
# passed. Every case below states the run count it is asserting about, so none
# can inherit another's.
# ---------------------------------------------------------------------------

# `$1` is the stubbed run count (`-` = gh itself fails). Stderr only: what is
# under test is what the developer is TOLD.
with_run_count() {
    local count="$1"
    shift
    # SC2030/SC2031 are the POINT, as they are for `with_stub` above: scoping
    # the export to this subshell is how one case's stubbed count is kept out
    # of the next one's.
    # shellcheck disable=SC2030,SC2031
    (
        export PINION_STUB_RUN_COUNT="$count"
        stub_gh ""
        { "$@" >/dev/null; } 2>&1
    )
}

ok "a base with a run of its own says so" \
   "$(with_run_count 2 check_base_ci_coverage abcdef1234 main test 9999 \
        | grep -c 'has 2 CI run(s) of its own')" \
   "1"

ok "a base with no run of its own is called out" \
   "$(with_run_count 0 check_base_ci_coverage abcdef1234 main test 9999 \
        | grep -c 'NO CI run of its own')" \
   "1"

# The whole point: the developer is told the verdict they just read is about a
# different commit. Without this line the two facts stay conflated.
ok "and it says the verdict above is about an earlier commit" \
   "$(with_run_count 0 check_base_ci_coverage abcdef1234 main test 9999 \
        | grep -c 'EARLIER commit')" \
   "1"

ok "and hands over the command that fixes it" \
   "$(with_run_count 0 check_base_ci_coverage abcdef1234 main test 9999 \
        | grep -c 'gh workflow run ci.yml --ref main')" \
   "1"

# A push seconds after another legitimately has no run yet. Crying wolf there
# would train the reader to ignore the notice that matters.
ok "a base younger than the grace is not called out" \
   "$(with_run_count 0 check_base_ci_coverage abcdef1234 main test 5 \
        | grep -c 'NO CI run of its own')" \
   "0"

ok "a base at the grace boundary IS called out" \
   "$(with_run_count 0 check_base_ci_coverage abcdef1234 main test \
        "$CI_SCHEDULING_GRACE_SECONDS" | grep -c 'NO CI run of its own')" \
   "1"

# An unparseable age must not silently buy the grace.
ok "an unknown age does not buy the grace" \
   "$(with_run_count 0 check_base_ci_coverage abcdef1234 main test "" \
        | grep -c 'NO CI run of its own')" \
   "1"

# Fail-open, and say so — the same posture the rest of this file takes.
ok "a gh that fails reports that it could not ask" \
   "$(with_run_count - check_base_ci_coverage abcdef1234 main test 9999 \
        | grep -c 'could not ask')" \
   "1"

ok "and never refuses the push" \
   "$( (
        # shellcheck disable=SC2030,SC2031
        export PINION_STUB_RUN_COUNT=0; stub_gh ""; \
        check_base_ci_coverage abcdef1234 main test 9999 >/dev/null 2>&1; echo $?) )" \
   "0"

ok "a first push has no base to check" \
   "$(with_run_count 0 check_base_ci_coverage 0000000000 main test 9999 \
        | grep -c 'no base on main yet')" \
   "1"

# A count that is not a count must not be read as one — a gh answering with
# usage text or an error object would otherwise pass as "has runs". The first
# draft of this case fed it `0` and asserted `0`, which is a test of the stub.
ok "a numeric answer is the count" \
   "$( (
        # shellcheck disable=SC2030,SC2031
        export PINION_STUB_RUN_COUNT=3; stub_gh ""; \
        ci_run_count_for_sha abcdef1234) )" \
   "3"

ok "a non-numeric answer is unknown, not a count" \
   "$( (
        # shellcheck disable=SC2030,SC2031
        export PINION_STUB_RUN_COUNT='Usage: gh api'; stub_gh ""; \
        ci_run_count_for_sha abcdef1234) )" \
   "unknown"

ok "and an unknown count is reported as unaskable rather than as zero" \
   "$(with_run_count 'Usage: gh api' check_base_ci_coverage abcdef1234 main \
        test 9999 | grep -c 'could not ask')" \
   "1"

# ---------------------------------------------------------------------------
# R1582 — consumer_test_gate: the tests of what a change can BREAK
#
# The decision, with `cargo` and the radius tool stubbed. What is under test is
# WHICH branch it takes and what it tells the developer, not cargo's verdict.
# ---------------------------------------------------------------------------

# A fake repo root whose `tools/blast_radius.py` answers a fixed package list
# and whose `cargo` answers a fixed exit code. `$1` is the newline-separated
# radius (`-` = the tool itself fails), `$2` the cargo exit code.
stub_radius_repo() {
    local radius="$1" cargo_rc="$2" dir
    dir="$(mktemp_tracked)"
    mkdir -p "$dir/tools" "$dir/bin"
    {
        echo '#!/usr/bin/env bash'
        if [[ "$radius" == "-" ]]; then
            echo 'exit 1'
        else
            printf 'cat <<%s\n%s\n%s\n' "EOF_R" "$radius" "EOF_R"
        fi
    } >"$dir/tools/blast_radius.py"
    chmod +x "$dir/tools/blast_radius.py"
    {
        echo '#!/usr/bin/env bash'
        echo '[[ "$1" == "test" ]] || exit 1'
        echo "exit $cargo_rc"
    } >"$dir/bin/cargo"
    chmod +x "$dir/bin/cargo"
    printf '%s' "$dir"
}

# `$1` radius, `$2` cargo exit code, `$3` cap, `$4` skip-flag posture (`-` unset).
# Echoes `<exit code>|<stderr>`; every case therefore states the environment it
# is asserting about, the R1500 rule. Split with `rc_of` / `msg_of` rather than
# `cut`, which is line-oriented and would hand back the whole of every stderr
# line after the first.
with_radius() {
    local radius="$1" cargo_rc="$2" cap="$3" skip="$4" dir out rc
    dir="$(stub_radius_repo "$radius" "$cargo_rc")"
    # `python3 <script>` must reach the stub, so the subshell gets a python3
    # that execs the script it is handed. SCOPED to the subshell: an earlier
    # draft exported it at file scope and hijacked python3 for the whole suite,
    # turning the tally's own cases red — the same "a stub wider than what it
    # stands in for tests the stub" trap `stub_gh` was written to avoid, one
    # level out.
    {
        echo '#!/usr/bin/env bash'
        echo 'script="$1"; shift; exec "$script" "$@"'
    } >"$dir/bin/python3"
    chmod +x "$dir/bin/python3"
    # shellcheck disable=SC2030,SC2031
    (
        if [[ "$skip" == "-" ]]; then
            unset PINION_SKIP_CONSUMER_TESTS
        else
            export PINION_SKIP_CONSUMER_TESTS="$skip"
        fi
        export CONSUMER_TEST_CAP="$cap"
        PATH="$dir/bin:$PATH"
        # The lib runs `python3 tools/blast_radius.py`; the stub is executable
        # and starts with a shebang, so point python3 at a passthrough.
        out="$(consumer_test_gate test "$dir" staged 2>&1)"
        rc=$?
        printf '%s|%s' "$rc" "$out"
    )
}

rc_of() { local both="$1"; printf '%s' "${both%%|*}"; }
msg_of() { local both="$1"; printf '%s' "${both#*|}"; }

ok "an empty radius runs nothing" \
   "$(msg_of "$(with_radius "" 0 12 -)" | grep -c "no package's behaviour changed")" \
   "1"

ok "a small radius is tested here" \
   "$(msg_of "$(with_radius $'leaf\nmid' 0 12 -)" | grep -c '2 package(s) can be affected — testing them here')" \
   "1"

ok "and passes when the consumers pass" \
   "$(rc_of "$(with_radius $'leaf\nmid' 0 12 -)")" \
   "0"

# The whole point: a consumer's FAILING tests stop the commit.
ok "a failing consumer refuses" \
   "$(rc_of "$(with_radius $'leaf\nmid' 1 12 -)")" \
   "1"

ok "and says it was a consumer, not the edited crate" \
   "$(msg_of "$(with_radius $'leaf\nmid' 1 12 -)" | grep -c 'DEPENDS on this change')" \
   "1"

ok "and hands over the command to reproduce it" \
   "$(msg_of "$(with_radius $'leaf\nmid' 1 12 -)" | grep -c 'cargo test -p leaf -p mid')" \
   "1"

# Over the cap the radius is REPORTED, never silently dropped — the failure
# mode this whole file exists to prevent is a check that stops happening.
ok "a radius over the cap is reported and left to CI" \
   "$(msg_of "$(with_radius $'a\nb\nc' 0 2 -)" | grep -c 'over the local cap')" \
   "1"

ok "and names the command anyway" \
   "$(msg_of "$(with_radius $'a\nb\nc' 0 2 -)" | grep -c 'cargo test -p a -p b -p c')" \
   "1"

ok "an over-cap radius does not refuse" \
   "$(rc_of "$(with_radius $'a\nb\nc' 1 2 -)")" \
   "0"

# Exactly at the cap is INSIDE it: the bound is "at or below", and an
# off-by-one here silently drops the largest radius the gate can afford.
ok "a radius exactly at the cap is still tested" \
   "$(rc_of "$(with_radius $'a\nb' 1 2 -)")" \
   "1"

# Fail-open on infrastructure absence, loudly — lib/ci-status.sh's posture.
ok "a radius that cannot be computed continues" \
   "$(rc_of "$(with_radius "-" 0 12 -)")" \
   "0"

ok "and says it could not compute it" \
   "$(msg_of "$(with_radius "-" 0 12 -)" | grep -c 'could not compute')" \
   "1"

# The escape hatch, and only for the exact value.
ok "the skip flag skips" \
   "$(rc_of "$(with_radius $'leaf\nmid' 1 12 1)")" \
   "0"

ok "and says so rather than going quiet" \
   "$(msg_of "$(with_radius $'leaf\nmid' 1 12 1)" | grep -c 'PINION_SKIP_CONSUMER_TESTS=1')" \
   "1"

ok "the skip flag is not armed by any other value" \
   "$(rc_of "$(with_radius $'leaf\nmid' 1 12 0)")" \
   "1"

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

# ---------------------------------------------------------------------------
# R1507 mnemosyne-tool.sh — resolving the pinned gate tool
#
# What is worth testing here is the part that can be WRONG about a fact rather
# than merely absent: which revision the resolver believes it resolved. A
# resolver that reads a directory name and reports it as a revision would pass
# every test that only checks "something was found", which is the shape R1495
# got wrong with its `gh` stub. So the fake CLIs below LIE about their
# revision, and the resolver has to notice.
# ---------------------------------------------------------------------------

mn_tmp="$(mktemp_tracked)"

# A stand-in `mnemosyne-cli` that reports `$1` as its build revision, in the
# real binary's `--version` format. It answers ONLY `--version`, because that is
# all the resolver may rely on; a stub that answered everything would let a
# resolver that shelled out for something else pass anyway.
make_fake_cli() {
    local path="$1" rev="$2"
    mkdir -p "$(dirname "$path")"
    cat >"$path" <<FAKE
#!/usr/bin/env bash
case "\${1:-}" in
    --version) echo "mnemosyne-cli 0.1.0 ($rev)" ;;
    query) ;;   # the R1509 delegation probe; silent = did not hand off
    *) echo "fake cli: unexpected argv: \$*" >&2; exit 64 ;;
esac
FAKE
    chmod +x "$path"
}

# --- the pin reader ---
declared_pin() {
    local toml="$mn_tmp/probe.toml"
    printf '%s\n' "$1" >"$toml"
    mnemosyne_declared_pin "$toml" 2>/dev/null || echo "<none>"
}

ok "the pin is read from the [tool] table" \
   "$(declared_pin '[tool]
pin = "be4c164"')" \
   "be4c164"

ok "a trailing comment is not part of the pin" \
   "$(declared_pin '[tool]
pin = "abc1234"  # the revision this workspace attributes gate results to')" \
   "abc1234"

# The reader must be table-scoped. A `pin` under some other table is a
# different setting, and treating it as the tool pin would silently gate this
# workspace with a revision nobody declared for it.
ok "a pin in another table is not the tool pin" \
   "$(declared_pin '[schema]
pin = "deadbee"

[tool]
pin = "be4c164"')" \
   "be4c164"

ok "a pin under only another table is no pin at all" \
   "$(declared_pin '[schema]
pin = "deadbee"')" \
   "<none>"

ok "no [tool] table is no pin" \
   "$(declared_pin '[schema]
scan_exclusions = ["vendor/"]')" \
   "<none>"

ok "an unreadable file is no pin" \
   "$(mnemosyne_declared_pin "$mn_tmp/does-not-exist.toml" 2>/dev/null || echo '<none>')" \
   "<none>"

# --- the revision check, which is what makes this a guard ---
make_fake_cli "$mn_tmp/right/bin/mnemosyne-cli" "be4c1647331468b"
make_fake_cli "$mn_tmp/wrong/bin/mnemosyne-cli" "d02c12fdeadbeef"

matches() {
    mnemosyne_revision_matches "$1" "$2" && echo yes || echo no
}

ok "a build whose revision extends the pin matches" \
   "$(matches "$mn_tmp/right/bin/mnemosyne-cli" be4c164)" "yes"

# The whole point: the DIRECTORY says be4c164, the BINARY says otherwise.
# A resolver that trusted the path would accept this.
mkdir -p "$mn_tmp/liar/be4c164/bin"
cp "$mn_tmp/wrong/bin/mnemosyne-cli" "$mn_tmp/liar/be4c164/bin/mnemosyne-cli"
ok "a build in a correctly-named directory that reports another revision is rejected" \
   "$(matches "$mn_tmp/liar/be4c164/bin/mnemosyne-cli" be4c164)" "no"

ok "a pin that is not a prefix is rejected" \
   "$(matches "$mn_tmp/right/bin/mnemosyne-cli" d02c12f)" "no"

ok "a missing binary is rejected" \
   "$(matches "$mn_tmp/nope/bin/mnemosyne-cli" be4c164)" "no"

printf '#!/usr/bin/env bash\nexit 3\n' >"$mn_tmp/broken"
chmod +x "$mn_tmp/broken"
ok "a binary that cannot report its version is rejected" \
   "$(matches "$mn_tmp/broken" be4c164)" "no"

printf 'not executable\n' >"$mn_tmp/plain"
ok "a non-executable path is rejected" \
   "$(matches "$mn_tmp/plain" be4c164)" "no"

# --- the revision must be a revision, not a description of one (R1508) ---
#
# Mnemosyne's build stamp appends `-dirty` when the tracked tree differs from
# HEAD and reports `unknown` when git cannot say. R1507 matched the pin as a
# bare prefix, so `be4c1647-dirty` — a build made from MODIFIED sources —
# passed as the pin. Measured before the fix: accepted.
hexpin() {
    mnemosyne_is_pinned_revision "$1" "$2" && echo yes || echo no
}

ok "an exact hex revision extending the pin is the pin" \
   "$(hexpin be4c1647331468b be4c164)" "yes"
ok "a -dirty build is not the revision it names" \
   "$(hexpin be4c1647-dirty be4c164)" "no"
ok "a -dirty build is rejected even at the pin's exact length" \
   "$(hexpin be4c164-dirty be4c164)" "no"
ok "unknown is not a revision" \
   "$(hexpin unknown be4c164)" "no"
ok "hex comparison is case-sensitive, as git's is" \
   "$(hexpin BE4C1647 be4c164)" "no"
ok "a different revision is not the pin" \
   "$(hexpin d02c12fabc be4c164)" "no"

# --- both probes must agree (R1508) ---
#
# The probe suppresses delegation so `--version` describes the binary rather
# than a delegate. That is necessary, and it is also blind by construction to a
# build that hands off to a DIFFERENT one when left alone — the assumption
# R1507 leaned on with nothing checking it. So the resolver asks twice.

# Reports the pin either way: an honest build that does not delegate.
cat >"$mn_tmp/honest" <<'FAKE'
#!/usr/bin/env bash
case "${1:-}" in
    --version) echo "mnemosyne-cli 0.1.0 (be4c1647)" ;;
    query) ;;   # silent: answers for itself
    *) echo "unexpected argv: $*" >&2; exit 64 ;;
esac
FAKE
chmod +x "$mn_tmp/honest"

# Reports the pin, then hands the actual work to another build — and says so,
# which is what the real tool does. R1508's version of this fake made
# `--version` change with suppression instead; R1509 measured that the tool
# answers `--version` before the pin logic and never does that, so the fake was
# modelling a behaviour that does not exist and the check over it could not fail.
cat >"$mn_tmp/delegating" <<'FAKE'
#!/usr/bin/env bash
case "${1:-}" in
    --version) echo "mnemosyne-cli 0.1.0 (be4c1647)" ;;
    query) echo "note: switching to the pinned build at /elsewhere/bin/mnemosyne-cli" >&2 ;;
    *) echo "unexpected argv: $*" >&2; exit 64 ;;
esac
FAKE
chmod +x "$mn_tmp/delegating"

ok "a build that answers for itself is accepted" \
   "$(matches "$mn_tmp/honest" be4c164)" "yes"

ok "a build that announces handing the work off is rejected" \
   "$(matches "$mn_tmp/delegating" be4c164 2>/dev/null)" "no"

ok "and the refusal quotes what the tool said" \
   "$( { mnemosyne_revision_matches "$mn_tmp/delegating" be4c164 >/dev/null; } 2>&1 \
        | grep -c 'switching to the pinned build' )" \
   "1"

# The probe's precondition, observed directly rather than inferred: the fake
# records what it was given, so the assertion is about the call and not about a
# revision string that could agree by luck.
cat >"$mn_tmp/recorder" <<FAKE
#!/usr/bin/env bash
if [ "\${1:-}" = "--version" ]; then
    echo "\${MNEMOSYNE_PIN_SKIP:-unset}" >>"$mn_tmp/skips"
    echo "mnemosyne-cli 0.1.0 (be4c1647)"
fi
FAKE
chmod +x "$mn_tmp/recorder"
: >"$mn_tmp/skips"
mnemosyne_revision_matches "$mn_tmp/recorder" be4c164 >/dev/null 2>&1
ok "the version probe suppresses delegation, and asks exactly once" \
   "$(tr '\n' ' ' <"$mn_tmp/skips")" \
   "1 "

ok "the run path does not suppress the pin" \
   "$(MN_CLI="$mn_tmp/telltale_run" bash -c '
        printf "#!/usr/bin/env bash\necho PIN_SKIP=\${MNEMOSYNE_PIN_SKIP:-unset}\n" >"'"$mn_tmp"'/telltale_run"
        chmod +x "'"$mn_tmp"'/telltale_run"
        MNEMOSYNE_PIN_SKIP="" true' ; MN_CLI="$mn_tmp/telltale_run" mnemosyne_cli validate-workspace)" \
   "PIN_SKIP=unset"

# --- the dual pin, checked at rest (R1508) ---
#
# R1507 checked the submodule's revision only inside the vendored branch, so a
# workspace whose installed pin resolved never looked at it: mnemosyne.toml and
# the gitlink could disagree indefinitely. The gitlink is written directly here
# (`update-index --cacheinfo`), which is all a submodule is from the index's
# point of view, so the check can be exercised without a real one.
mn_repo="$mn_tmp/repo"
mkdir -p "$mn_repo"
git -C "$mn_repo" init -q
printf '[tool]\npin = "be4c164"\n' >"$mn_repo/mnemosyne.toml"

link_gitlink() {
    git -C "$mn_repo" update-index --add --cacheinfo "160000,$1,vendor/mnemosyne"
}

check_pin() {
    ( cd "$mn_repo" && mnemosyne_check_vendored_pin "$mn_repo" "$1" >/dev/null 2>&1 \
        && echo ok || echo refused )
}

check_pin_msg() {
    ( cd "$mn_repo" && { mnemosyne_check_vendored_pin "$mn_repo" "$1" >/dev/null; } 2>&1 )
}

# A tree that declares no such submodule is not broken — the resolver still
# works there, so this must not refuse.
ok "no gitlink is not a drifted gitlink" \
   "$(check_pin be4c164)" "ok"

link_gitlink "d02c12fa11111111111111111111111111111111"
ok "a gitlink that is not the pin is refused" \
   "$(check_pin be4c164)" "refused"

ok "and the refusal says both revisions and how to move them together" \
   "$(check_pin_msg be4c164 | grep -cE 'pins .be4c164.|records vendor/mnemosyne at d02c12fa|git add vendor/mnemosyne')" \
   "3"

# The auto-init is NON-DESTRUCTIVE, and that is the assertion.
#
# `git submodule update` force-checks-out the gitlink, so running it on an
# existing directory would discard a developer's local work there. It therefore
# runs only when the submodule has never been checked out. Asserted on the
# announcement rather than on the outcome: a `submodule update` in this fake
# repo would fail anyway, so an outcome-only test would pass whether or not the
# guard existed — the shape R1507's CF-4 was caught by.
mn_live="$mn_tmp/live"
mkdir -p "$mn_live/vendor/mnemosyne"
git -C "$mn_live" init -q
printf '[package]\nname = "x"\nversion = "0.0.0"\n' >"$mn_live/vendor/mnemosyne/Cargo.toml"
git -C "$mn_live/vendor/mnemosyne" init -q
git -C "$mn_live/vendor/mnemosyne" add -A
git -C "$mn_live/vendor/mnemosyne" -c user.email=t@t -c user.name=t commit -q -m "at the pin"
live_sha="$(git -C "$mn_live/vendor/mnemosyne" rev-parse HEAD)"
git -C "$mn_live" update-index --add --cacheinfo "160000,$live_sha,vendor/mnemosyne"

ok "a checked-out submodule at the pin is accepted without touching it" \
   "$( { mnemosyne_check_vendored_pin "$mn_live" "${live_sha:0:7}" >/dev/null; } 2>&1 \
        | grep -c 'initialising' )" \
   "0"

ok "and it is accepted" \
   "$(mnemosyne_check_vendored_pin "$mn_live" "${live_sha:0:7}" >/dev/null 2>&1 \
        && echo ok || echo refused)" \
   "ok"

# --- ★ and it is accepted INSIDE A HOOK, which is the only place it runs ---
#
# R1665. `git commit` exports `GIT_DIR` and `GIT_INDEX_FILE` to its hooks, and
# those variables OUTRANK `-C`: every `git -C vendor/mnemosyne ...` in the
# resolver silently read the PARENT repository instead, so the submodule checks
# answered about the wrong repo and the resolver reported that no pinned build
# could be produced — with the submodule present, at the pin, clean, and its
# binary built and correct.
#
# It went 150 rounds unmet because the vendored branch had never been taken
# inside a hook: an installed build at `$MN_ROOT/<pin>` existed for every pin
# this tree had used, and the resolver prefers it. R1665 moved the pin to a
# revision with no installed build and the fallback ran for the first time.
#
# The two assertions above pass with and without the fix, because a plain shell
# has no such environment. Only this one discriminates, which is exactly why it
# is a separate case rather than an extra condition on those.
# R1760 — NOT backticks. Inside this double-quoted label they COMMAND
# SUBSTITUTE, so `tools/test_hooks.sh` was running `git commit` on every
# invocation — including inside `pre-push`, which runs this suite. It exited
# non-zero and printed to a swallowed stream, so nothing ever showed it. Found
# by shellcheck (SC2006) while checking this round's own edits, an hour after
# the identical defect was found by RUNNING the new `worktree.sh land`.
ok "the submodule checks survive the environment 'git commit' gives a hook" \
   "$(GIT_DIR="$mn_live/.git" GIT_INDEX_FILE="$mn_live/.git/index" \
        mnemosyne_check_vendored_pin "$mn_live" "${live_sha:0:7}" >/dev/null 2>&1 \
        && echo ok || echo refused)" \
   "ok"

ok "and so does the worktree-clean check, for the same reason" \
   "$(GIT_DIR="$mn_live/.git" GIT_INDEX_FILE="$mn_live/.git/index" \
        mnemosyne_vendored_worktree_clean "$mn_live" >/dev/null 2>&1 \
        && echo clean || echo refused)" \
   "clean"

# --- a build from a dirty worktree is not the pinned revision (R1508) ---
#
# Upstream's build stamp derives `-dirty` from git metadata and its own docs say
# an unstaged edit moves neither HEAD nor the index, so it cannot always tell.
# R1507 made a locally built binary trusted AS a pin, which is the case those
# docs leave out of scope. The worktree check is the input they say is needed.
mn_sub="$mn_tmp/sub"
mkdir -p "$mn_sub/vendor/mnemosyne"
printf '[package]\nname = "x"\nversion = "0.0.0"\n' >"$mn_sub/vendor/mnemosyne/Cargo.toml"
git -C "$mn_sub/vendor/mnemosyne" init -q
git -C "$mn_sub/vendor/mnemosyne" add -A
git -C "$mn_sub/vendor/mnemosyne" -c user.email=t@t -c user.name=t commit -q -m "clean"

build_vendored_verdict() {
    mnemosyne_build_vendored "$mn_sub" "$1" >/dev/null 2>&1 && echo built || echo refused
}

printf 'local edit\n' >>"$mn_sub/vendor/mnemosyne/Cargo.toml"
# Asserted on the REASON. An outcome-only check passes whether or not the guard
# exists, because this fixture's build fails regardless — the third time that
# shape has surfaced (R1507 CF-4, R1508 CF-3 and CF-4), so it is called out
# here rather than rediscovered.
ok "a dirty vendored worktree is refused for BEING dirty, before any build" \
   "$( { mnemosyne_build_vendored "$mn_sub" deadbee >/dev/null; } 2>&1 \
        | grep -c 'uncommitted changes' )" \
   "1"

ok "and the refusal explains that the stamp cannot always tell" \
   "$({ mnemosyne_build_vendored "$mn_sub" deadbee >/dev/null; } 2>&1 \
        | grep -cE 'uncommitted changes|stamp cannot always tell')" \
   "2"

git -C "$mn_sub/vendor/mnemosyne" checkout -- .
# Clean now, so it gets past the worktree gate and fails on the build instead —
# which is a different refusal, and the point is that it reached it.
ok "a clean worktree gets past the worktree gate" \
   "$({ mnemosyne_build_vendored "$mn_sub" deadbee >/dev/null; } 2>&1 \
        | grep -cE 'uncommitted changes')" \
   "0"

# --- resolution, in a throwaway repo so no machine state leaks in ---
#
# The dual-pin tests above left a drifted gitlink in this repo, and the
# resolver now refuses on that BEFORE choosing a source — which is the point of
# them. Removed here so what follows exercises source selection rather than
# re-testing the drift refusal.
git -C "$mn_repo" update-index --force-remove vendor/mnemosyne

# `mnemosyne_resolve` asks git for the root, so run it from inside the repo.
resolve_in_repo() {
    ( cd "$mn_repo" && MN_ROOT="$1" mnemosyne_resolve >/dev/null 2>&1 \
        && echo "$MN_CLI_SOURCE" || echo "<refused>" )
}

mkdir -p "$mn_tmp/root-ok/be4c164/bin"
cp "$mn_tmp/right/bin/mnemosyne-cli" "$mn_tmp/root-ok/be4c164/bin/mnemosyne-cli"
ok "an installed pin that verifies is used" \
   "$(resolve_in_repo "$mn_tmp/root-ok")" \
   "installed pin be4c164"

# No installed pin and no vendor/mnemosyne in this throwaway repo. The
# resolver must REFUSE rather than reach for PATH — where a real
# `mnemosyne-cli` may well be sitting on this machine. That fall-back is the
# defect R1502 / R1503 measured, so its absence is the assertion.
ok "with no pin and no submodule the resolver refuses rather than using PATH" \
   "$(resolve_in_repo "$mn_tmp/root-empty")" \
   "<refused>"

# And it refuses for the right reason, in a message a reader can act on.
ok "the refusal names both sources" \
   "$( ( cd "$mn_repo" && { MN_ROOT="$mn_tmp/root-empty" mnemosyne_resolve >/dev/null; } 2>&1 ) \
        | grep -cE 'cargo install --git|git submodule update --init' )" \
   "2"

# The WIRING, not just the check. The dual-pin assertions above call
# `mnemosyne_check_vendored_pin` directly, so removing its call from
# `mnemosyne_resolve` left every one of them green — the resolver could stop
# looking at the gitlink entirely and nothing would say so. This asserts the
# call, with a VALID installed pin present so the drift is the only reason to
# refuse: exactly the situation R1507 left unchecked.
git -C "$mn_repo" update-index --add \
    --cacheinfo "160000,d02c12fa11111111111111111111111111111111,vendor/mnemosyne"
ok "a drifted gitlink refuses even when the installed pin is good" \
   "$(resolve_in_repo "$mn_tmp/root-ok")" \
   "<refused>"
git -C "$mn_repo" update-index --force-remove vendor/mnemosyne
# ---------------------------------------------------------------------------
# R1509 — which source wins, and the vendored cache bound
# ---------------------------------------------------------------------------

# A repo that has BOTH an already-built vendored binary and a good installed
# pin. The vendored one must win: its provenance is checkable (the worktree is
# right there) while an installed build made from an unstaged edit of the pinned
# revision cannot be told apart from a clean one. Nothing asserted this
# ordering when it was introduced, so a revert to installed-first would have
# been silent.
mn_both="$mn_tmp/both"
mkdir -p "$mn_both/vendor/mnemosyne/target/release"
git -C "$mn_both" init -q
printf '[package]\nname = "x"\nversion = "0.0.0"\n' >"$mn_both/vendor/mnemosyne/Cargo.toml"
git -C "$mn_both/vendor/mnemosyne" init -q
git -C "$mn_both/vendor/mnemosyne" add -A
git -C "$mn_both/vendor/mnemosyne" -c user.email=t@t -c user.name=t commit -q -m "clean"
both_sha="$(git -C "$mn_both/vendor/mnemosyne" rev-parse HEAD)"
both_pin="${both_sha:0:7}"
printf '[tool]\npin = "%s"\n' "$both_pin" >"$mn_both/mnemosyne.toml"
git -C "$mn_both" update-index --add --cacheinfo "160000,$both_sha,vendor/mnemosyne"
make_fake_cli "$mn_both/vendor/mnemosyne/target/release/mnemosyne-cli" "$both_sha"
mkdir -p "$mn_tmp/root-both/$both_pin/bin"
make_fake_cli "$mn_tmp/root-both/$both_pin/bin/mnemosyne-cli" "$both_sha"

resolve_both() {
    ( cd "$mn_both" && MN_ROOT="$mn_tmp/root-both" mnemosyne_resolve >/dev/null 2>&1 \
        && echo "$MN_CLI_SOURCE" || echo "<refused>" )
}

ok "an already-built vendored binary beats an installed pin" \
   "$(resolve_both)" \
   "vendor/mnemosyne @ $both_pin"

# …and it is not used when its worktree is dirty, even though the binary is
# right there. The same condition that gates the BUILD has to gate the reuse,
# or the second caller quietly trusts what the first refused to make.
printf 'local edit\n' >>"$mn_both/vendor/mnemosyne/Cargo.toml"
ok "a dirty worktree disqualifies the vendored binary, falling back to the pin" \
   "$(resolve_both)" \
   "installed pin $both_pin"
git -C "$mn_both/vendor/mnemosyne" checkout -- .

ok "and with the worktree restored the vendored binary is preferred again" \
   "$(resolve_both)" \
   "vendor/mnemosyne @ $both_pin"

# --- the vendored cache bound (R1509) ---
#
# R1508 reported this cache and recorded that nothing would ever shrink it. The
# apparent size is what `du -sbL` measures, so a sparse file exercises the
# over-budget path without costing disk.
mn_cache="$mn_tmp/cache"
mkdir -p "$mn_cache/vendor/mnemosyne/target"

ok "a cache under budget is reported and kept" \
   "$( { report_vendored_cache "$mn_cache" test >/dev/null; } 2>&1 \
        | grep -c 'vendor/mnemosyne/target/ is 0 MiB (budget 2 GiB)' )" \
   "1"

truncate -s 3G "$mn_cache/vendor/mnemosyne/target/sparse" 2>/dev/null || true
if [[ -f "$mn_cache/vendor/mnemosyne/target/sparse" ]]; then
    ok "a cache over budget is reclaimed, and says the next run rebuilds" \
       "$( { report_vendored_cache "$mn_cache" test >/dev/null; } 2>&1 \
            | grep -cE 'is 3 GiB \(budget 2 GiB\)|reclaiming it|rebuilds the gate tool' )" \
       "3"
fi

ok "a non-numeric vendored budget is rejected, not silently defaulted" \
   "$( { PINION_VENDORED_CACHE_BUDGET_GB=2G report_vendored_cache "$mn_cache" test >/dev/null; } 2>&1 \
        | grep -c 'must be a positive integer of GiB' )" \
   "1"

# --- the shared volume (R1641) ---
#
# The per-project budget was ON target and the build died anyway, because the
# volume every project's target/ lives on was full. These assert the two halves
# of what that cost: the volume's number is always said, and pressure on it
# tightens a budget the project is otherwise within.
vol_tmp="$(mktemp_tracked)"
mkdir -p "$vol_tmp/target"

ok "the volume's free space is read as a number" \
   "$( [[ "$(volume_free_gib "$vol_tmp")" =~ ^[0-9]+$ ]] && echo yes )" \
   "yes"

ok "an unreadable path yields nothing rather than a bogus zero" \
   "$(volume_free_gib "$vol_tmp/does-not-exist")" \
   ""

# A floor of 0 can never be crossed, so this is the "no pressure" arm: the
# volume is still REPORTED, and the declared budget is the one applied.
ok "with room, the volume is reported and the project's own budget stands" \
   "$( { PINION_VOLUME_FREE_FLOOR_GB=1 PINION_TARGET_BUDGET_GB=100 \
         enforce_target_budget "$vol_tmp" test; } 2>&1 \
        | grep -cE 'volume has [0-9]+ GiB free|target/ is 0 GiB \(budget 100 GiB\)' )" \
   "2"

# A floor above any real free space forces the pressure arm. The budget line
# must then show the TIGHT number, which is the whole behaviour: a project
# inside its steady-state allowance still gets swept when the shared resource
# is short.
ok "under pressure the budget tightens, and says so" \
   "$( { PINION_VOLUME_FREE_FLOOR_GB=999999 PINION_TARGET_BUDGET_GB=100 \
         PINION_TARGET_BUDGET_TIGHT_GB=45 enforce_target_budget "$vol_tmp" test; } 2>&1 \
        | grep -cE 'tightening this project.s budget 100 -> 45 GiB|target/ is 0 GiB \(budget 45 GiB\)' )" \
   "2"

# The tight budget is a FLOOR on tightening, not a replacement: a project
# already declaring less than it must not be loosened by the pressure arm.
ok "pressure never loosens an already-smaller budget" \
   "$( { PINION_VOLUME_FREE_FLOOR_GB=999999 PINION_TARGET_BUDGET_GB=10 \
         PINION_TARGET_BUDGET_TIGHT_GB=45 enforce_target_budget "$vol_tmp" test; } 2>&1 \
        | grep -c 'target/ is 0 GiB (budget 10 GiB)' )" \
   "1"

ok "no vendored cache at all is silent" \
   "$( { report_vendored_cache "$mn_tmp/no-such-repo" test >/dev/null; } 2>&1 | wc -c )" \
   "0"

# ---------------------------------------------------------------------------
# lib/phase-b-tally.sh (R1522)
#
# The reporter this replaces could not fail, and that was the problem: it piped
# through `2>/dev/null … || true`, so a broken tally printed nothing and nothing
# is what a tree with no drift also prints. Every case below is therefore about
# what gets SAID, and the fakes model a tool that is broken in each of the ways
# the real one can break.

tally_tmp="$(mktemp_tracked)"

# A tally tool that works: distinct answers for --selftest and the report.
mkdir -p "$tally_tmp/good/tools"
cat >"$tally_tmp/good/tools/phase_b_tally.py" <<'PY'
import sys
if "--selftest" in sys.argv:
    print("selftest: PASS (0 failure(s))")
else:
    print("Phase B tally - 1 examples, 1 demos")
    print("weighted (all axes)   100   42%")
PY

ok "a working tally speaks its selftest verdict, not only its numbers" \
   "$(report_phase_b_tally "$tally_tmp/good" | grep -c 'tally selftest: PASS')" \
   "1"

ok "a working tally still prints the weighted figure" \
   "$(report_phase_b_tally "$tally_tmp/good" | grep -c 'pre-push: weighted (all axes)')" \
   "1"

# A tally whose own logic is broken. The numbers must be WITHHELD: a tool that
# failed its own check has not earned having its figures quoted.
mkdir -p "$tally_tmp/badlogic/tools"
cat >"$tally_tmp/badlogic/tools/phase_b_tally.py" <<'PY'
import sys
if "--selftest" in sys.argv:
    print("SELFTEST FAIL: the perf axis counts no hot-path optimisation")
    print("selftest: FAIL (1 failure(s))")
    sys.exit(1)
print("weighted (all axes)   100   42%")
PY

ok "a failing selftest is reported verbatim" \
   "$(report_phase_b_tally "$tally_tmp/badlogic" | grep -c 'SELFTEST FAIL: the perf axis')" \
   "1"

ok "a failing selftest withholds the numbers it could not vouch for" \
   "$(report_phase_b_tally "$tally_tmp/badlogic" | grep -c 'pre-push: weighted')" \
   "0"

ok "a failing selftest says why the numbers are missing" \
   "$(report_phase_b_tally "$tally_tmp/badlogic" | grep -c 'numbers withheld')" \
   "1"

# A tally that crashes, printing nothing anywhere. This is the case the R1519
# call could not distinguish from a healthy tree, and the only one that matters:
# the check would have stopped happening with no trace.
mkdir -p "$tally_tmp/mute/tools"
cat >"$tally_tmp/mute/tools/phase_b_tally.py" <<'PY'
import sys
sys.exit(3)
PY

ok "a mute broken tally is loud, not silent" \
   "$(report_phase_b_tally "$tally_tmp/mute" | grep -c 'the selftest printed nothing at all')" \
   "1"

ok "a mute broken tally reports its exit status" \
   "$(report_phase_b_tally "$tally_tmp/mute" | grep -c 'selftest FAILED (exit 3)')" \
   "1"

# A tally that passes its selftest but whose report no longer emits a summary
# line — a renamed heading, a refactor that drops the totals. The grep miss must
# surface the raw output instead of eating it.
mkdir -p "$tally_tmp/nosummary/tools"
cat >"$tally_tmp/nosummary/tools/phase_b_tally.py" <<'PY'
import sys
if "--selftest" in sys.argv:
    print("selftest: PASS (0 failure(s))")
else:
    print("total across axes: 42%")
PY

ok "a report with no recognised summary line prints its raw output" \
   "$(report_phase_b_tally "$tally_tmp/nosummary" \
        | grep -cE 'produced no summary line|total across axes')" \
   "2"

# R1526 — a finding this hook did not know how to speak. The round ledger's
# whole defence against a forgotten declaration is that the omission is SAID at
# every push; if the grep does not carry the line, forgetting is silent again
# and the declaration is back to being prose someone has to remember.
mkdir -p "$tally_tmp/undeclared/tools"
cat >"$tally_tmp/undeclared/tools/phase_b_tally.py" <<'PY'
import sys
if "--selftest" in sys.argv:
    print("selftest: PASS (0 failure(s))")
else:
    print("weighted (all axes)   100   42%")
    print("UNDECLARED - 1 round(s) have no row in phase-b-rounds.tsv")
    print("DECLARED AHEAD - 1 ledger row(s) name a round with no commit yet")
PY

ok "a round that landed without declaring an axis is spoken at the push" \
   "$(report_phase_b_tally "$tally_tmp/undeclared" | grep -c 'pre-push: UNDECLARED')" \
   "1"

ok "a ledger row naming a round that does not exist is spoken too" \
   "$(report_phase_b_tally "$tally_tmp/undeclared" | grep -c 'pre-push: DECLARED AHEAD')" \
   "1"

# The one correct silence: no tool in the tree at all.
ok "an absent tally tool is silent" \
   "$(report_phase_b_tally "$tally_tmp/no-such-repo" | wc -c)" \
   "0"

# And the real tool in this repo must pass its own check, every push.
ok "this repo's tally passes its own selftest" \
   "$(report_phase_b_tally "$repo_root" | grep -c 'tally selftest: PASS')" \
   "1"

# ---------------------------------------------------------------------------
# R1611 — the reference-name ratchet
# ---------------------------------------------------------------------------
#
# The gate exists because a directive with no measurement grew to 7,999
# occurrences unseen. What has to be true of it is that it can FAIL: a ratchet
# that cannot refuse is a report. Both refusal shapes are exercised against a
# throwaway tree so no state of this repo's own leaks into the answer.

ok "the ratchet's classifier passes its own tests" \
   "$(python3 "$repo_root/tools/reference_names.py" --selftest | grep -c 'selftest: .* OK')" \
   "1"

ok "the migrator's classifier passes its own tests" \
   "$(python3 "$repo_root/tools/reference_names_migrate.py" --selftest \
      | grep -c 'selftest: .* OK')" \
   "1"

ok "this repo is at or under its own budget" \
   "$(python3 "$repo_root/tools/reference_names.py" --check >/dev/null 2>&1; echo $?)" \
   "0"

ratchet_tmp="$(mktemp -d)"
trap 'rm -rf "$ratchet_tmp"' EXIT
git init -q "$ratchet_tmp/repo"
mkdir -p "$ratchet_tmp/repo/tools" "$ratchet_tmp/repo/docs" \
         "$ratchet_tmp/repo/docs/.atomic" "$ratchet_tmp/repo/vendor" \
         "$ratchet_tmp/repo/crates/pinion-text-unicode/ucd"
cp "$repo_root/tools/reference_names.py" "$ratchet_tmp/repo/tools/"
printf 'clean prose\n' > "$ratchet_tmp/repo/a.rs"
one_name="Q""t"; two_name="Blen""der"; three_name="Un""real"
printf '// %s does one thing\n' "$one_name" > "$ratchet_tmp/repo/b.rs"
git -C "$ratchet_tmp/repo" add -A >/dev/null
python3 "$ratchet_tmp/repo/tools/reference_names.py" --write-budget >/dev/null

ok "a budgeted tree passes" \
   "$(python3 "$ratchet_tmp/repo/tools/reference_names.py" --check >/dev/null 2>&1; \
      echo $?)" \
   "0"

printf '// %s does one thing\n// and %s another\n' \
       "$one_name" "$two_name" > "$ratchet_tmp/repo/b.rs"
ok "a budgeted file that GAINS a name is refused" \
   "$(python3 "$ratchet_tmp/repo/tools/reference_names.py" --check >/dev/null 2>&1; \
      echo $?)" \
   "1"

one_name="Q""t"; two_name="Blen""der"; three_name="Un""real"
printf '// %s does one thing\n' "$one_name" > "$ratchet_tmp/repo/b.rs"
printf '// %s appears here\n' "$three_name" > "$ratchet_tmp/repo/a.rs"
git -C "$ratchet_tmp/repo" add -A >/dev/null
ok "a clean file that gains its FIRST name is refused" \
   "$(python3 "$ratchet_tmp/repo/tools/reference_names.py" --check >/dev/null 2>&1; \
      echo $?)" \
   "1"

printf 'clean prose\n' > "$ratchet_tmp/repo/a.rs"
printf '// nothing here now\n' > "$ratchet_tmp/repo/b.rs"
ok "clearing a name is allowed to lower the count" \
   "$(python3 "$ratchet_tmp/repo/tools/reference_names.py" --check >/dev/null 2>&1; \
      echo $?)" \
   "0"

# ---------------------------------------------------------------------------
# worktree_verdict — the guard that keeps a round closing from the main tree
# ---------------------------------------------------------------------------
#
# The paths below are the MEASURED forms (2026-08-20): in the main tree
# `--absolute-git-dir` and `--git-common-dir` are the same path, and in a linked
# worktree the first is `<common>/worktrees/<name>`. The classifier is pure so
# every verdict is reachable here without building a worktree -- which matters,
# because the one thing a check like this must never do is pass by accident on
# the tree it happens to be run from.

wg_main="/home/coin/pinion/.git"
wg_linked="/home/coin/pinion/.git/worktrees/gate"

ok "identical git dirs are the main tree" \
   "$(worktree_verdict "$wg_main" "$wg_main" "")" "main"
ok "a linked worktree is refused" \
   "$(worktree_verdict "$wg_linked" "$wg_main" "")" "linked"
ok "the override releases a linked worktree" \
   "$(worktree_verdict "$wg_linked" "$wg_main" "1")" "override"

# The override must not turn the MAIN tree into an override case -- the verdict
# a hook prints is read by a person, and "allowing commit from ..." in the main
# tree would train them to ignore it.
ok "the override does not relabel the main tree" \
   "$(worktree_verdict "$wg_main" "$wg_main" "1")" "main"

# A trailing slash is a form nobody produces today. That is exactly why it is
# here: the check would pass for years and then refuse every commit on the day
# git's normalisation changed.
ok "a trailing slash on one side still reads as main" \
   "$(worktree_verdict "$wg_main/" "$wg_main" "")" "main"
ok "a trailing slash on both sides still reads as main" \
   "$(worktree_verdict "$wg_main/" "$wg_main/" "")" "main"

# Prefix-only relationships must NOT read as main. A worktree's git dir always
# starts with the common dir, so a comparison written with `==` against a glob,
# or with a prefix test, would call every worktree the main tree -- which is the
# failure that makes the gate absent rather than wrong.
ok "a path that merely starts with the common dir is linked" \
   "$(worktree_verdict "${wg_main}x" "$wg_main" "")" "linked"
ok "a deeper worktree path is still linked" \
   "$(worktree_verdict "$wg_main/worktrees/a/b" "$wg_main" "")" "linked"

# An empty override string is what an unset variable expands to; it must not
# read as "set".
ok "an unset override is not an override" \
   "$(worktree_verdict "$wg_linked" "$wg_main" "")" "linked"

# ---------------------------------------------------------------------------
# R1760 — the round-number duplicate gate. `git log` cannot see a round that
# has not committed, so two sessions derive the same number; measured
# 2026-08-21, R1757 and R1758 were both begun as "R1757".
# ---------------------------------------------------------------------------

ok "the round token is read off a subject" \
   "$(round_token_of 'feat(rpc): R1757 a burst of keys arrives together')" "R1757"
ok "a continuation keeps its suffix" \
   "$(round_token_of 'fix(runtime): R1753.1 a count in a comment was never true')" "R1753.1"
ok "a subject with no round declares none" \
   "$(round_token_of 'chore: tidy the imports')" ""
# The token is the FIRST one: a subject mentioning another round in its prose
# still declares its own.
ok "the declared round is the first token" \
   "$(round_token_of 'fix(core): R1760 repay what R1757 got wrong')" "R1760"

HIST='feat(rpc): R1757 a burst of keys arrives together
feat(core): R1758 a verdict says what it was read from
fix(runtime): R1753.1 a count in a comment was never true'

taken() { if round_token_taken "$1" "$HIST"; then echo taken; else echo free; fi; }

ok "a duplicate round is refused" "$(taken R1757)" "taken"
ok "a fresh round is allowed" "$(taken R1760)" "free"
# ★ THE CASE THAT MAKES THE GATE USABLE: 106 commits in this history are `.N`
# follow-ups to a round that is already committed. Folding them onto the parent
# would refuse every one.
ok "a continuation of a committed round is allowed" "$(taken R1757.1)" "free"
ok "an already-used continuation is refused" "$(taken R1753.1)" "taken"
# ...and the parent of a committed continuation is NOT thereby taken, because
# `R1753` itself has not been used as a subject token here.
ok "a continuation does not reserve its parent" "$(taken R1753)" "free"
# A prefix must not match: R175 is not R1757.
ok "a shorter number is not a prefix match" "$(taken R175)" "free"
ok "a longer number is not a match either" "$(taken R17570)" "free"
ok "an empty token is never taken" "$(taken '')" "free"

# ---------------------------------------------------------------------------
# R1782 — the ssh keepalive, which is a gate now because clippy came back
# ---------------------------------------------------------------------------
#
# The verdict is a pure function precisely so it can be checked without a
# remote, a network or a push. What it has to get right is not "is the option
# there" but "is it ON", and those differ.
ka() { keepalive_verdict "$1" "$2"; }

REAL_KA='ssh -o ServerAliveInterval=20 -o ServerAliveCountMax=60 -o TCPKeepAlive=yes'

ok "an ssh remote with a real keepalive is armed" \
    "$(ka 'git@github.com:o/r.git' "$REAL_KA")" "armed"
ok "the ssh:// spelling is an ssh remote too" \
    "$(ka 'ssh://git@github.com/o/r.git' "$REAL_KA")" "armed"
ok "an ssh remote with no ssh command at all is missing" \
    "$(ka 'git@github.com:o/r.git' '')" "missing"
ok "a bare ssh command is missing" \
    "$(ka 'git@github.com:o/r.git' 'ssh')" "missing"
# ★★★★★ THE CASE THAT MAKES THIS MORE THAN A GREP: `ServerAliveInterval=0` is
# ssh's own default and means "never send one". A check that matched the option
# NAME would call the default armed — which is the failure it exists to catch.
ok "ServerAliveInterval=0 is ssh's default and means never" \
    "$(ka 'git@github.com:o/r.git' 'ssh -o ServerAliveInterval=0')" "missing"
ok "and the space spelling ssh also accepts is read" \
    "$(ka 'git@github.com:o/r.git' 'ssh -o "ServerAliveInterval 30"')" "armed"
# An https remote opens no ssh connection, so there is nothing to hold open and
# refusing one would be a gate that fires where its reason does not reach.
ok "an https remote is not an ssh remote" \
    "$(ka 'https://github.com/o/r.git' '')" "not-ssh"
ok "nor is a local path" "$(ka '/srv/mirror.git' '')" "not-ssh"
# And the hook must actually consult it.
ok "the push gate consults the keepalive" \
    "$(grep -c 'keepalive_verdict' "$repo_root/.githooks/pre-push")" "1"

# ---------------------------------------------------------------------------
# R1782 — the lint gates live in two files, and "I checked it as a move"
# ---------------------------------------------------------------------------
#
# R1779 moved clippy and rustdoc out of the hooks into CI and justified it with
# a principle: "checked as a MOVE before the removal — a gate deleted here and
# absent there would have been a gate deleted." That check was an ACT, done
# once, by hand. Nothing re-performs it, so an edit to either file can delete a
# gate while both files still look deliberate.
#
# R1782 then split the pair back apart on measurement, which leaves the two
# lint gates in DIFFERENT places — clippy in both `pre-push` and CI, rustdoc in
# CI alone. That arrangement is exactly the kind a reader cannot verify by
# looking at one file, so it gets a check instead of a sentence.
#
# Reads the two files; runs nothing.
hook_clippy="$(sed -n 's/^if ! \(car[g]o clippy [^;]*\); then$/\1/p' \
    "$repo_root/.githooks/pre-push" | head -1)"
ci_clippy="$(sed -n 's/^ *run: \(car[g]o clippy .*\)$/\1/p' \
    "$repo_root/.github/workflows/ci.yml" | head -1)"
ci_doc="$(sed -n 's/^ *run: \(car[g]o doc .*\)$/\1/p' \
    "$repo_root/.github/workflows/ci.yml" | head -1)"

# ★ The presence assertions come FIRST and are not optional: with both sides
# missing, an equality test passes vacuously — which is the failure this whole
# section exists to prevent.
ok "the push gate runs clippy at all" "${hook_clippy:+present}" "present"
ok "CI runs clippy at all" "${ci_clippy:+present}" "present"
ok "and both run the SAME clippy, flag for flag" "$hook_clippy" "$ci_clippy"
# Rustdoc has exactly one home. Before R1214 it had none, and ~1000 broken
# links accreted from ~R683 unseen because the gate ran clippy and never
# rustdoc. One home is enough; zero is how that happened.
ok "rustdoc still runs in CI, its only home" "${ci_doc:+present}" "present"

# ── R1791: the impact-ref guard is itself guarded ───────────────────────────
#
# ★ `tools/impact_refs.py` exists because a prescription nobody executes is not
# a repair — R1758 wrote the fix down and the class recurred 33 rounds later.
# A guard that silently stops working is the same failure one level up, so its
# selftest runs here, where the hook libraries' own tests already run.
impact_self="$(python3 "$repo_root/tools/impact_refs.py" --selftest 2>&1 || true)"
ok "the impact-ref guard passes its own selftest" \
   "$(grep -c 'selftest: 11 of 11 passed' <<<"$impact_self")" \
   "1"
# And it still refuses the token that created it, which is the one case a
# regression here would be silent about.
impact_prose="$(python3 - "$repo_root" <<'PY' 2>&1 || true
import sys
sys.path.insert(0, sys.argv[1] + "/tools")
from impact_refs import offenders
print("refused" if offenders(["2 #1"], {"2"}) else "accepted")
PY
)"
ok "and it refuses the prose form of an invariant" "$impact_prose" "refused"

printf '[hooks] %d passed, %d failed\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]]
