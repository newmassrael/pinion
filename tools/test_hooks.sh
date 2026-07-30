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

ok "no vendored cache at all is silent" \
   "$( { report_vendored_cache "$mn_tmp/no-such-repo" test >/dev/null; } 2>&1 | wc -c )" \
   "0"

printf '[hooks] %d passed, %d failed\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]]
