#!/usr/bin/env bash
# .githooks/lib/ident-gate.sh — which addresses may author or commit here.
#
# WHY THIS EXISTS, and it is not hypothetical. In a sibling repository of this
# owner's, eight commits reached a PUBLIC remote authored with a work address
# instead of the one every other commit carries. A history rewrite removed
# them from the branch and that did NOT un-publish them: the host kept the
# unreachable objects, went on serving all eight by SHA for days, and went on
# listing the other account as a contributor the whole time. The only thing
# that actually removed them was deleting and recreating the repository, which
# cost 1146 runs of CI history. This gate exists so the next occurrence costs
# a refused commit instead.
#
# WHY A CONFIG WOULD NOT HAVE CAUGHT IT. `git config user.email` was already
# correct there, in the clone AND in ~/.gitconfig. The commits came from a
# different environment. A config is a DEFAULT and it is per-clone; a rule
# that has to be present on the machine that gets it wrong has to travel with
# the tree, which is what a tracked hook does.
#
# WHY IT GRADES `git var` AND NEVER `git config`. The identity a commit will
# carry is not what the config says: GIT_AUTHOR_EMAIL and GIT_COMMITTER_EMAIL
# in the environment override it, `commit --author` overrides it again, and
# `git -c user.email=` overrides it for one invocation. `git var
# GIT_AUTHOR_IDENT` is git answering with what it will actually stamp, after
# all of that. Reading the config grades a different question.
#
# WHY AN ALLOW-LIST. A deny-list passes every identity it has not been taught
# yet; an allow-list fails closed for the one nobody thought about. It also
# avoids writing the offending address into a tracked file of a public
# repository, which is the exposure the incident was about.
#
# TWO CALLERS, ONE LIST. `pre-commit` grades the identity the next commit
# would carry; `pre-push` grades every commit in the range being published.
# Neither subsumes the other: pre-commit runs only for `git commit`, so
# cherry-pick, rebase, merge and `--no-verify` reach the remote without it,
# and a commit made where the hooks are not installed is caught only at the
# push -- which is the shape the incident actually had.
#
# Tested in tools/test_hooks.sh, like every other library here.

# The identities this repository accepts. Add one DELIBERATELY, in its own
# commit: an edit here is a statement about who may write history that the
# remote publishes.
PINION_ALLOWED_IDENT_EMAILS=(
    "newmassrael@gmail.com"
)

# "Name <email> 1756100000 +0900" -> "email".
#
# Cut on the angle brackets, not on whitespace: a display name may contain
# spaces, and a field-counting parse returns the wrong token when it does --
# silently, which is the failure mode this file is about.
ident_email_of() {
    local ident="$1"
    ident="${ident#*<}"
    printf '%s' "${ident%%>*}"
}

ident_is_allowed() {
    local email="$1" allowed
    for allowed in "${PINION_ALLOWED_IDENT_EMAILS[@]}"; do
        [[ "$email" == "$allowed" ]] && return 0
    done
    return 1
}

# The shared refusal, so the two hooks cannot drift into explaining one rule
# two ways.
ident_refuse() {
    local hook="$1" what="$2" email="$3"
    {
        echo "${hook}: ${what} <${email}>,"
        echo "  which is not an identity this repository accepts."
        echo ""
        echo "  Measured: eight commits reached a PUBLIC repo of this owner's"
        echo "  under a different address. Rewriting history did NOT un-publish"
        echo "  them -- they stayed reachable by SHA and the repository had to"
        echo "  be deleted and recreated, costing 1146 runs of CI history."
        echo ""
        echo "  fix, in this clone:"
        echo "    git config user.email ${PINION_ALLOWED_IDENT_EMAILS[0]}"
        echo "    git config user.name  <your name>"
        echo "  and check the environment too -- these override the config:"
        echo "    env | grep -E '^GIT_(AUTHOR|COMMITTER)_EMAIL='"
        echo ""
        echo "  If a NEW identity is genuinely meant to write here, add it to"
        echo "  PINION_ALLOWED_IDENT_EMAILS in .githooks/lib/ident-gate.sh --"
        echo "  deliberately, in its own commit."
    } >&2
}

# pre-commit's arm: the identity the commit ABOUT TO BE MADE would carry.
ident_gate_pending() {
    local hook="$1" pair role verb ident email
    for pair in "AUTHOR authored" "COMMITTER committed"; do
        role="${pair%% *}"
        verb="${pair##* }"
        if ! ident="$(git var "GIT_${role}_IDENT")"; then
            echo "${hook}: \`git var GIT_${role}_IDENT\` failed; cannot" >&2
            echo "  determine the identity this commit would carry. A gate" >&2
            echo "  that cannot read its input must not report green." >&2
            return 1
        fi
        email="$(ident_email_of "$ident")"
        if [[ -z "$email" ]]; then
            echo "${hook}: could not read an email out of GIT_${role}_IDENT." >&2
            echo "  git said: ${ident}" >&2
            return 1
        fi
        if ! ident_is_allowed "$email"; then
            ident_refuse "$hook" "this commit would be ${verb} as" "$email"
            return 1
        fi
    done
    return 0
}

# pre-push's arm: every commit in the range being published.
#
# Takes the rev-range as `git log` ARGUMENTS rather than one string, because
# `pre-push` already computes that range for the commit-message lint and it is
# not always a `A..B`: a first push onto a new remote ref uses
# `<tip> --not --remotes`. Passing the same array is what keeps this gate from
# disagreeing with the message lint about what is being published -- the point
# R1779 makes one loop down for the touched-paths list.
#
# Reports EVERY offender rather than the first, because the fix is a rebase
# whose scope the author needs to know before starting it.
ident_gate_range() {
    local hook="$1" sha email bad=0 shown=0 log
    shift
    local range="$*"
    if ! log="$(git log --format='%H %ae%n%H %ce' "$@" --)"; then
        echo "${hook}: \`git log ${range}\` failed; cannot determine which" >&2
        echo "  identities this push would publish. A gate that cannot read" >&2
        echo "  its input must not report green." >&2
        return 1
    fi
    while IFS=' ' read -r sha email; do
        [[ -n "$sha" ]] || continue
        if ! ident_is_allowed "$email"; then
            if [[ "$bad" -eq 0 ]]; then
                ident_refuse "$hook" "this push would publish commits by" "$email"
                echo "" >&2
                echo "  offending commits in ${range}:" >&2
            fi
            bad=$((bad + 1))
            if [[ "$shown" -lt 20 ]]; then
                echo "    ${sha} <${email}>" >&2
                shown=$((shown + 1))
            fi
        fi
    done <<<"$log"
    if [[ "$bad" -gt 0 ]]; then
        [[ "$bad" -gt "$shown" ]] && echo "    ... and $((bad - shown)) more" >&2
        return 1
    fi
    return 0
}
