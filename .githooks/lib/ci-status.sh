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
