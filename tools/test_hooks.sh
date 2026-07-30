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

mn_tmp="$(mktemp -d)"
trap 'rm -rf "$mn_tmp"' EXIT

# A stand-in `mnemosyne-cli` that reports `$1` as its build revision, in the
# real binary's `--version` format. It answers ONLY `--version`, because that is
# all the resolver may rely on; a stub that answered everything would let a
# resolver that shelled out for something else pass anyway.
make_fake_cli() {
    local path="$1" rev="$2"
    mkdir -p "$(dirname "$path")"
    cat >"$path" <<FAKE
#!/usr/bin/env bash
[ "\${1:-}" = "--version" ] || { echo "fake cli: unexpected argv: \$*" >&2; exit 64; }
echo "mnemosyne-cli 0.1.0 ($rev)"
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

# --- which invocations may suppress the pin ---
#
# The probe must, or a binary that hands off reports its delegate's revision
# and the check above proves nothing. The run path must not, or the tool
# announces on every commit that the pin is unenforced — which is false, and
# false in the direction that teaches a reader to ignore the gate's own log.
# This fake reports a MATCHING revision only when it was allowed to skip, so
# the probe's precondition is what is being asserted, not merely its result.
cat >"$mn_tmp/telltale" <<'FAKE'
#!/usr/bin/env bash
if [ "${1:-}" = "--version" ]; then
    if [ "${MNEMOSYNE_PIN_SKIP:-}" = "1" ]; then
        echo "mnemosyne-cli 0.1.0 (be4c1647)"
    else
        echo "mnemosyne-cli 0.1.0 (deadbeef)"
    fi
    exit 0
fi
echo "PIN_SKIP=${MNEMOSYNE_PIN_SKIP:-unset}"
FAKE
chmod +x "$mn_tmp/telltale"

ok "the revision probe suppresses delegation" \
   "$(matches "$mn_tmp/telltale" be4c164)" "yes"

ok "the run path does not suppress the pin" \
   "$(MN_CLI="$mn_tmp/telltale" mnemosyne_cli validate-workspace)" \
   "PIN_SKIP=unset"

# --- resolution, in a throwaway repo so no machine state leaks in ---
mn_repo="$mn_tmp/repo"
mkdir -p "$mn_repo"
git -C "$mn_repo" init -q
printf '[tool]\npin = "be4c164"\n' >"$mn_repo/mnemosyne.toml"

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
   "$( ( cd "$mn_repo" && MN_ROOT="$mn_tmp/root-empty" mnemosyne_resolve 2>&1 >/dev/null ) \
        | grep -cE 'cargo install --git|git submodule update --init' )" \
   "2"

# A submodule checked out somewhere other than the pin must not be built and
# called the pin: that reintroduces the ambiguity this file removes.
git -C "$mn_repo" -c user.email=t@t -c user.name=t commit -q --allow-empty -m "base"
mkdir -p "$mn_repo/vendor/mnemosyne"
printf '[package]\nname = "x"\nversion = "0.0.0"\n' >"$mn_repo/vendor/mnemosyne/Cargo.toml"
git -C "$mn_repo/vendor/mnemosyne" init -q
git -C "$mn_repo/vendor/mnemosyne" -c user.email=t@t -c user.name=t \
    commit -q --allow-empty -m "not the pin"
# Asserted on the REASON, not just the outcome. The first draft checked only
# that resolution refused, and a counterfactual that deleted the revision check
# entirely still passed it — the fake submodule's build fails anyway, so the
# test was measuring a broken Cargo.toml rather than the guard. The refusal has
# to name the mismatch, which it can only do before attempting a build.
ok "a submodule at another revision is refused BY REVISION, before any build" \
   "$( ( cd "$mn_repo" && MN_ROOT="$mn_tmp/root-empty" mnemosyne_resolve 2>&1 >/dev/null ) \
        | grep -cE 'vendor/mnemosyne is at [0-9a-f]{8}, not the' )" \
   "1"

ok "and it still refuses" \
   "$(resolve_in_repo "$mn_tmp/root-empty")" \
   "<refused>"

printf '[hooks] %d passed, %d failed\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]]
