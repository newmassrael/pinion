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
# The branch is filtered HERE rather than by `gh run list --branch`, because
# that flag does not exist in gh 2.4.0 (the version on this machine, and the
# one Ubuntu ships). Passing it makes `gh` print usage and exit 0 with no
# rows, which the caller cannot distinguish from "no runs yet" — so the gate
# would have fail-opened on every push, forever, on the machine it was written
# on. The first draft did exactly that; the unit tests did not catch it,
# because the `gh` stub they use accepted any arguments and so was more
# permissive than the real thing.
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
ci_run_count_for_sha() {
    local sha="$1" out
    command -v gh >/dev/null 2>&1 || { printf 'unknown\n'; return 0; }
    if ! out="$(gh api "repos/:owner/:repo/actions/runs?head_sha=$sha&per_page=1" \
                    --jq '.total_count' 2>/dev/null)"; then
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
        echo "$label: base $short has $count CI run(s) of its own" >&2
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
# Returns 0 when publishing may proceed, 1 when it must not.
check_last_ci_run() {
    local branch="$1" label="$2"

    if ! command -v gh >/dev/null 2>&1; then
        echo "$label: gh not on PATH — last CI verdict unknown, continuing" >&2
        return 0
    fi

    local listing
    # No `--branch`: see ci_verdict_from_listing. The limit is generous
    # because the branch filter happens after the fetch, so other branches'
    # runs consume rows.
    if ! listing="$(gh run list --limit 30 2>/dev/null)"; then
        echo "$label: gh run list failed (no network or no auth) — last CI" \
             "verdict unknown, continuing" >&2
        return 0
    fi

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
                return 0
            fi
            echo "$label: last completed CI run on $branch FAILED (run $id)" >&2
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
