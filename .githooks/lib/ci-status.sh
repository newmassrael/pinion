#!/usr/bin/env bash
# R1495 — refuse to publish onto a base the last CI run said was broken.
#
# ## Why this is a gate and not a note
#
# The project's ratified rule is stop-the-line: one red, one flake, and the
# next thing that happens is the fix ([[zero-flake-policy]], R882.2). Until
# this hook, that rule was enforced by a sentence in `docs/SEED_PROMPT.md`
# telling the next session to run `gh run list` before doing anything else —
# in a file that is `.gitignore`d, so a fresh clone does not have it, and by a
# reader who has to remember to obey it.
#
# R1470 is the case in point and it wrote the lesson down: a paint assertion
# turned `lint-and-test` red, `demo-sweep` and `gpu-tests` are `needs:`-gated
# behind it, and so for **99 consecutive pushes** the demo sweep did not run
# at all while every push reported "done". The round that found it recorded
# "a prose warning is not a gate" — and then left this particular defence as
# prose. This is that lesson applied to the place it was written about.
#
# ## What it does NOT do
#
# It does not run the tests. Running the full workspace suite or the demo
# sweep locally is against a standing instruction (2026-07-21: local gates
# cover the crates a round touched, the full sweep is CI's job, and running it
# locally fights the user's own session for the display). This reads CI's
# verdict; it never reproduces it. Cost is one `gh` call, no build.
#
# ## Fail-open, deliberately
#
# A machine without `gh`, without network, or without an authenticated token
# still has to be able to publish. Infrastructure absence is not evidence of
# breakage, so it reports and continues — the same choice
# `lib/target-budget.sh` makes about a missing `cargo sweep`. What it will not
# do is stay quiet: the notice is printed either way, because the failure mode
# this exists to prevent is a check that silently stopped happening.
#
# ## Overriding
#
# `PINION_PUSH_ON_RED=1` publishes anyway, loudly. Pushing the fix for a red
# base is the normal reason to do that, and a stop-the-line rule with no way
# to push the fix would stop the line permanently.

# Decide a verdict for `branch` from `gh run list` plain-text output.
#
# Echoes one of `green <id>` / `red <id>` / `unknown`, and returns 0 always —
# the caller decides what a verdict means.
#
# Plain text, not `--json`: this host has no `jq`, and a `gh ... --json | jq`
# pipeline there dies silently, producing zero rows that are indistinguishable
# from "no runs" (measured R1478). The columns are
# STATUS, CONCLUSION, TITLE, WORKFLOW, BRANCH, EVENT, ID, ELAPSED, AGE —
# tab-separated, newest first. A run still going has an EMPTY conclusion
# field, which is why the scan takes the first row whose status is
# `completed`: an in-progress run has not judged anything yet, and the last
# run that DID judge is the one whose verdict the base inherits.
#
# The branch is filtered HERE rather than by `gh run list --branch`. R1495
# wrote that the flag does not exist in the `gh` on this machine, and passing
# it makes `gh` print usage and exit 0 with no rows — indistinguishable from
# "no runs yet", so the gate would have fail-opened on every push forever. The
# first draft did exactly that, and the unit tests did not catch it because the
# `gh` stub accepted any argument and was more permissive than the real thing.
#
# ⚠ R1857 — **the version in that sentence had rotted.** Measured here:
# `gh version` answers 2.45.0 and `gh run list --help` DOES list
# `-b, --branch string`. So the flag exists now and the original reason is no
# longer the reason. The parse-side filter STAYS, and on a better one: it does
# not depend on a flag existing, so it cannot regress on an older `gh` in a
# fresh clone or a CI image, and the rows it drops are rows this function
# already has to walk. What is corrected is the CLAIM, because a comment
# asserting a version nobody re-measures is how the sibling defect below went
# three misdiagnoses deep.
ci_verdict_from_listing() {
    local listing="$1" want_branch="$2"
    local status conclusion rest id branch
    while IFS=$'\t' read -r status conclusion rest; do
        [[ "$status" == "completed" ]] || continue
        # In the remainder: 1=TITLE, 2=WORKFLOW, 3=BRANCH, 4=EVENT, 5=ID.
        branch="$(printf '%s' "$rest" | cut -f3)"
        [[ "$branch" == "$want_branch" ]] || continue
        id="$(printf '%s' "$rest" | cut -f5)"
        if [[ "$conclusion" == "success" ]]; then
            printf 'green %s\n' "$id"
        else
            printf 'red %s\n' "$id"
        fi
        return 0
    done <<<"$listing"
    printf 'unknown\n'
}

# How long after a push GitHub is still allowed not to have scheduled a run.
#
# Two pushes in quick succession legitimately leave the first one without a run
# for a few seconds. Past this, "no run" stops being "not yet".
CI_SCHEDULING_GRACE_SECONDS=180

# Echo how many workflow runs GitHub holds for `sha`, or `unknown`.
#
# `gh api` rather than `gh run list`: the plain-text listing has no SHA column
# (STATUS, CONCLUSION, TITLE, WORKFLOW, BRANCH, EVENT, ID, ELAPSED, AGE), so the
# question "does THIS commit have a run" cannot be asked of it at all. `--jq` is
# gh's own embedded filter, not the external `jq` this host lacks.
#
# Anything that is not a run of digits becomes `unknown`, so a gh that answers
# with usage text, an error object or nothing at all cannot be read as a count.
#
# R1869 — a non-empty second argument restricts the count to runs that have
# FINISHED. **A run's existence is not a run's verdict**, and this function
# could not tell the two apart: measured on the R1868 push, the base had one
# run of its own and that run was still going, so the gate said the base was
# covered while the verdict it printed one line above belonged to a commit four
# back. That is R1579's own distinction — absence versus success — one step
# further along, and the same sentence covers it: a run that has judged nothing
# is not evidence about this commit either way.
ci_run_count_for_sha() {
    local sha="$1" only_completed="${2:-}" query out
    command -v gh >/dev/null 2>&1 || { printf 'unknown\n'; return 0; }
    query="repos/:owner/:repo/actions/runs?head_sha=$sha&per_page=1"
    [[ -n "$only_completed" ]] && query="$query&status=completed"
    if ! out="$(gh api "$query" --jq '.total_count' 2>/dev/null)"; then
        printf 'unknown\n'
        return 0
    fi
    out="${out//[$'\t\r\n ']/}"
    if [[ "$out" =~ ^[0-9]+$ ]]; then
        printf '%s\n' "$out"
    else
        printf 'unknown\n'
    fi
}

# Echo the commit a run judged, or `unknown`.
#
# `gh run view --json headSha` rather than the plain-text listing, for the
# reason stated on `ci_run_count_for_sha`: that listing has no SHA column at
# all (STATUS, CONCLUSION, TITLE, WORKFLOW, BRANCH, EVENT, ID, ELAPSED, AGE),
# so the verdict parse upstream can name a run and cannot name a commit.
# `--jq` is gh's own embedded filter, not the external `jq` this host lacks.
#
# Anything that is not 40 hex characters becomes `unknown`, so a gh that
# answers with usage text, an error object or nothing cannot be read as a sha.
ci_head_sha_for_run() {
    local id="$1" out
    [[ "$id" =~ ^[0-9]+$ ]] || { printf 'unknown\n'; return 0; }
    command -v gh >/dev/null 2>&1 || { printf 'unknown\n'; return 0; }
    if ! out="$(gh run view "$id" --json headSha --jq '.headSha' 2>/dev/null)"; then
        printf 'unknown\n'
        return 0
    fi
    out="${out//[$'\t\r\n ']/}"
    if [[ "$out" =~ ^[0-9a-f]{40}$ ]]; then
        printf '%s\n' "$out"
    else
        printf 'unknown\n'
    fi
}

# Echo where the commit a run judged sits relative to the tip being published.
#
# ## The property this exists to state
#
# **A verdict belongs to the commit it judged, never to the commit in front of
# you.** `check_last_ci_run` reads the branch's last COMPLETED run, and the
# newest run is frequently not completed — so on a busy branch the verdict on
# offer is routinely several commits old, and the commits since it may already
# contain its repair. Saying only "the last completed run FAILED" leaves a
# reader unable to tell a red they must fix from a red they already fixed.
#
# ## The case that demanded it, measured rather than supposed
#
# R1869 entered on a handover that said "CI is red (two demo sweeps) — that is
# this round". Re-measured, the red was run 33075355005, and it judged
# `3ca5414b` (R1866). Its two failing demos — `r1694_a_locked_seat_is_heard`
# and `r1695_the_rail_takes_you_there` — both PASS on the tip, run locally at
# R1869; the commit that touched both of them is `3c7afb10` (R1867), one
# commit after the one the run judged. So the handover was describing a red
# four commits behind the tree, and it was able to because the gate that
# produced it named a RUN and no commit: that a red can be inherited from an
# ancestor lived only in a prose note in a memory file, which is the shape
# this project has repeatedly paid for. R1868 then armed `PINION_PUSH_ON_RED`
# against it with nothing in front of it that could have said so.
#
# ⚠ What is NOT claimed: that CI now agrees. The runs for R1867 and R1868 were
# both still in progress when this was written, so the evidence here is a local
# reproduction of the two failures, not a green run. That is exactly the
# distinction this function exists to keep visible.
#
# ## What it deliberately does not decide
#
# Whether the intervening commits actually repaired it. Only CI knows that.
# This answers WHERE, exactly, and hands the reader the range to look at; a
# gate that guessed "probably fixed" would be inventing the very verdict it is
# supposed to be reading.
#
# Echoes `same`, `behind <n>`, `unrelated`, or `unknown`, and returns 0 always.
# `unknown` covers every case the local repository cannot answer — an absent
# sha, an object this clone does not have (a fresh clone, a shallow one, a
# force-pushed branch), or a git that failed — because a position that cannot
# be computed must not be reported as a position.
#
# ★ `tip` has NO default, and the first draft's `${2:-HEAD}` is why this
# paragraph exists: the test written against it caught the default answering
# `behind 1` for a caller that had passed no tip at all. A position is a
# relation and needs BOTH of its ends; substituting "whatever is checked out"
# for the missing one contradicts the very property the caller in `pre-push`
# is built on — the question is about the ref being PUBLISHED, never about the
# working tree — and it does so silently, which is the direction that invents
# a fact rather than declining to state one.
ci_red_position() {
    local red="$1" tip="${2:-}" r b n
    [[ -n "$red" && "$red" != "unknown" && ! "$red" =~ ^0+$ ]] ||
        { printf 'unknown\n'; return 0; }
    [[ -n "$tip" && ! "$tip" =~ ^0+$ ]] || { printf 'unknown\n'; return 0; }
    r="$(git rev-parse --verify --quiet "${red}^{commit}" 2>/dev/null)" || r=""
    b="$(git rev-parse --verify --quiet "${tip}^{commit}" 2>/dev/null)" || b=""
    [[ -n "$r" && -n "$b" ]] || { printf 'unknown\n'; return 0; }
    if [[ "$r" == "$b" ]]; then
        printf 'same\n'
        return 0
    fi
    if git merge-base --is-ancestor "$r" "$b" 2>/dev/null; then
        n="$(git rev-list --count "$r..$b" 2>/dev/null)"
        [[ "$n" =~ ^[0-9]+$ ]] || { printf 'unknown\n'; return 0; }
        printf 'behind %s\n' "$n"
        return 0
    fi
    printf 'unrelated\n'
}

# How many commits of the repairing range to spell out before counting the rest.
#
# A bound on LINES and never on FACTS, the rule R1863 wrote for the short-box
# warning: the count is always stated, so a range too long to print cannot be
# mistaken for a range that was short.
CI_RED_RANGE_LINES=5

# Say, on stderr, WHICH JOBS of a red run failed.
#
# ★★★★★ R1953 — the other half of "a defect a reader cannot act on is a number
# rather than a report".
#
# `ci_report_red_position` (R1869) answers *which commit* the red judged, which
# tells a reader whether it is theirs. It does not say what broke, and the run
# id it prints is the only handle on that. Measured at R1953: run 33498241879
# went red at R1950.1 and **four consecutive rounds published over it** —
# R1951, R1951.1, R1951.2 and R1952 — each of them shown that id and none of
# them opening it. The red was fourteen demo failures in one job, every one of
# them on the section this project is judged on, and it took a round of its own
# to find out.
#
# One `gh` call, on the run the gate has already identified, so the SHAPE of
# the red is in front of whoever is about to override it. Fails open and
# silently on everything — no gh, no network, an unparseable answer — because
# an absent report must never turn into an absent push.
#
# Returns 0 always; this reports and never decides.
ci_report_red_jobs() {
    local id="$1" label="$2" out
    [[ "$id" =~ ^[0-9]+$ ]] || return 0
    command -v gh >/dev/null 2>&1 || return 0
    if ! out="$(gh run view "$id" --json jobs \
        --jq '.jobs[] | select(.conclusion == "failure") | .name' 2>/dev/null)"; then
        echo "$label:   which job(s) failed could not be read" >&2
        return 0
    fi
    local shown=0 total=0 line
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        total=$((total + 1))
        if (( shown < CI_RED_RANGE_LINES )); then
            echo "$label:   FAILED JOB: $line" >&2
            shown=$((shown + 1))
        fi
    done <<<"$out"
    if (( total == 0 )); then
        # A run whose conclusion is failure and whose jobs are all green is a
        # real state (a cancelled or infrastructure-level failure), and saying
        # nothing about it would read as "no jobs were checked".
        echo "$label:   no job of it reports a failure — the run failed as a whole" >&2
    elif (( total > shown )); then
        echo "$label:   and $((total - shown)) more failing job(s)" >&2
    fi
}

# Say, on stderr, which commit a red verdict actually judged and where it sits.
#
# Printed on BOTH sides of the override, because the reader who most needs it
# is the one publishing anyway: R1868 armed `PINION_PUSH_ON_RED=1` against a
# red that had already been repaired, and nothing it was shown could have told
# it so. Returns 0 always; this reports and never decides.
ci_report_red_position() {
    local id="$1" label="$2" tip="${3:-}"
    [[ -n "$tip" ]] || return 0

    local red position kind distance
    red="$(ci_head_sha_for_run "$id")"
    if [[ "$red" == "unknown" ]]; then
        echo "$label:   which commit it judged could not be determined" >&2
        return 0
    fi

    position="$(ci_red_position "$red" "$tip")"
    read -r kind distance <<<"$position"
    local short="${red:0:8}"

    case "$kind" in
        same)
            echo "$label:   it judged $short, which is exactly what is being" \
                 "published — nothing since it could have repaired it" >&2
            ;;
        behind)
            echo "$label:   it judged $short, and this push publishes" \
                 "$distance commit(s) past it" >&2
            echo "$label:   a verdict belongs to the commit it judged, so the" \
                 "commits since may already carry its repair:" >&2
            local shown=0 line
            while IFS= read -r line; do
                [[ -n "$line" ]] || continue
                if (( shown < CI_RED_RANGE_LINES )); then
                    echo "$label:     $line" >&2
                    shown=$((shown + 1))
                fi
            done < <(git log --oneline --no-decorate "$red..$tip" 2>/dev/null)
            if (( distance > shown )); then
                echo "$label:     ... and $((distance - shown)) more" >&2
            fi
            echo "$label:   verify against the run itself; do not assume" \
                 "either way" >&2
            ;;
        unrelated)
            echo "$label:   it judged $short, which is not an ancestor of" \
                 "what is being published" >&2
            ;;
        *)
            echo "$label:   where $short sits relative to this push could not" \
                 "be computed here" >&2
            ;;
    esac
    return 0
}

# Report whether the commit being pushed ONTO has any CI run of its own.
#
# ## Why this is a second question
#
# `check_last_ci_run` reads the branch's last *completed* run. That answers
# "was the last thing CI judged broken", and it silently assumes the last thing
# CI judged is the thing you are building on. Measured here on 2026-08-06, it
# was not:
#
#   * `ae377e0c`, pushed 21:38:23Z — `actions/runs?head_sha=` answered `0`, and
#     still answered `0` forty-five minutes later.
#   * `a8249cd7`, pushed 21:41:16Z — `0` at 21:45, then a `push`-event run
#     created at **22:11:17Z**, half an hour after the push that caused it.
#
# The workflow was active, its trigger matched, the repo is public so no
# spending limit applies, and the neighbouring pushes got runs within seconds.
#
# The first reading of this was "GitHub scheduled none". Re-measuring corrected
# it to "one was half an hour late and one had not arrived", which changes the
# prescription: a lag is not a failure, so the honest report is that the base
# has no run OF ITS OWN and the verdict above therefore describes an earlier
# commit. **A run's absence and a run's success are different facts**, and the
# gate could not tell them apart — the R1470 shape one level up: not a check
# that reports red, a check that stopped happening.
#
# ## Why it does not refuse
#
# Because the fact is real and the cause is not ours. Nothing here makes GitHub
# schedule a run, the lag above shows a pending run is a normal state, and this
# file's own rule is that "cannot verify" must not become "cannot publish".
# What was wrong was the SILENCE. So this speaks, and prints the command that
# resolves it now rather than in half an hour.
#
# Returns 0 always.
check_base_ci_coverage() {
    local sha="$1" branch="$2" label="$3" age_seconds="${4:-}"

    if [[ -z "$sha" || "$sha" =~ ^0+$ ]]; then
        echo "$label: no base on $branch yet — no CI coverage to check" >&2
        return 0
    fi

    local count
    count="$(ci_run_count_for_sha "$sha")"
    local short="${sha:0:8}"

    if [[ "$count" == "unknown" ]]; then
        echo "$label: could not ask whether $short has a CI run — continuing" >&2
        return 0
    fi
    if [[ "$count" -gt 0 ]]; then
        # R1869 — and whether any of them has actually JUDGED anything. A
        # pending run is the normal state seconds after a push, so "has a run"
        # was silently answering a question the reader was not asking.
        local judged
        judged="$(ci_run_count_for_sha "$sha" completed)"
        if [[ "$judged" == "unknown" ]]; then
            echo "$label: base $short has $count CI run(s) of its own" >&2
            echo "$label:   how many of them have finished could not be asked" >&2
        elif [[ "$judged" -gt 0 ]]; then
            echo "$label: base $short has $count CI run(s) of its own," \
                 "$judged completed" >&2
        else
            echo "$label: base $short has $count CI run(s) of its own, none" \
                 "COMPLETED" >&2
            echo "$label:   a run that has not finished has judged nothing," \
                 "so any verdict above is about an EARLIER commit" >&2
        fi
        return 0
    fi

    if [[ "$age_seconds" =~ ^[0-9]+$ ]] &&
       (( age_seconds < CI_SCHEDULING_GRACE_SECONDS )); then
        echo "$label: base $short has no CI run yet (${age_seconds}s old) —" \
             "probably not scheduled yet" >&2
        return 0
    fi

    echo "$label: base $short has NO CI run of its own" >&2
    echo "$label:   so any verdict above is about an EARLIER commit" >&2
    echo "$label:   a run's absence and a run's success are different facts" >&2
    echo "$label:   scheduling can lag — measured 30 min here on 2026-08-06" >&2
    echo "$label:   schedule one now: gh workflow run ci.yml --ref $branch" >&2
    return 0
}

# Gate the push on the last completed run for `branch`.
#
# `tip` is optional and is what this push would make the branch point at. When
# given, a red verdict additionally says WHICH COMMIT it judged and where that
# sits relative to `tip` (`ci_report_red_position`). It is optional rather than
# required because the position is a second, git-local question: a caller that
# cannot answer it must still get the verdict, and every case that stated only
# the verdict before this change still states exactly that.
#
# Returns 0 when publishing may proceed, 1 when it must not.
check_last_ci_run() {
    local branch="$1" label="$2" tip="${3:-}"

    if ! command -v gh >/dev/null 2>&1; then
        echo "$label: gh not on PATH — last CI verdict unknown, continuing" >&2
        return 0
    fi

    local listing said_file said
    # No `--branch`: see ci_verdict_from_listing. The limit is generous
    # because the branch filter happens after the fetch, so other branches'
    # runs consume rows.
    #
    # ★★★★★ R1857 — **stderr is KEPT, and the refusal repeats it.** This line
    # was `2>/dev/null` and the message beside it named two causes — "no
    # network or no auth" — that nothing had measured. Fail-open is the right
    # posture (infrastructure absence is not evidence of breakage), but a gate
    # that opens without saying WHY sends every later reader to guess, and this
    # one was misdiagnosed three times running: as a host with no `gh` account
    # (R1851), as a daemon that had moved `XDG_CONFIG_HOME` (R1856), and as a
    # hook that could not see where the credential lives (R1857.1). Measured
    # while R1857 was open, by running this exact command and KEEPING stderr:
    # `failed to get runs: HTTP 502: Server Error`. A server error is a third
    # cause the sentence could not express, and the one piece of evidence that
    # named it was being thrown away one character at a time.
    #
    # ⇒ **a fail-open must name what it could not do, not guess why.**
    said_file="$(mktemp)"
    if ! listing="$(gh run list --limit 30 2>"$said_file")"; then
        said="$(head -n 1 "$said_file" | tr -d '\r')"
        rm -f "$said_file"
        echo "$label: gh run list failed — last CI verdict unknown," \
             "continuing" >&2
        echo "$label:   gh said: ${said:-(nothing on stderr)}" >&2
        return 0
    fi
    rm -f "$said_file"

    local verdict id
    read -r verdict id <<<"$(ci_verdict_from_listing "$listing" "$branch")"

    case "$verdict" in
        green)
            echo "$label: last completed CI run on $branch: SUCCESS (run $id)" >&2
            return 0
            ;;
        unknown)
            echo "$label: no completed CI run on $branch yet — nothing to" \
                 "inherit, continuing" >&2
            return 0
            ;;
        red)
            if [[ "${PINION_PUSH_ON_RED:-}" == "1" ]]; then
                echo "$label: last completed CI run on $branch FAILED (run" \
                     "$id) — publishing anyway, PINION_PUSH_ON_RED=1" >&2
                ci_report_red_position "$id" "$label" "$tip"
                ci_report_red_jobs "$id" "$label"
                return 0
            fi
            echo "$label: last completed CI run on $branch FAILED (run $id)" >&2
            ci_report_red_position "$id" "$label" "$tip"
            ci_report_red_jobs "$id" "$label"
            echo "$label: stop-the-line — fix the red before publishing more" \
                 "on top of it" >&2
            echo "$label:   gh run view $id" >&2
            echo "$label: to publish the FIX for it: PINION_PUSH_ON_RED=1 git push" >&2
            return 1
            ;;
        *)
            echo "$label: unrecognised CI verdict '$verdict' — continuing" >&2
            return 0
            ;;
    esac
}
