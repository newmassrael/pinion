#!/usr/bin/env bash
# .githooks/lib/ident-gate.sh — which addresses may author or commit here.
#
# WHY THIS EXISTS, and it is not hypothetical. Commits reached a PUBLIC remote
# of this owner's authored with an address other than the one every other
# commit carries. A history rewrite removed them from the branch and did NOT
# un-publish them; the repository had to be deleted and recreated, which threw
# away its CI history. This gate exists so the next occurrence costs a refused
# commit instead.
#
# ★★★★★ R1828 — THE PARAGRAPH ABOVE WAS RE-MEASURED, AND IT IS NOW WRITTEN IN
# TWO HALVES, because its first draft argued from an incident it named in
# numbers that nothing here can reproduce. A gate justified by an uncitable
# event is the defect this repository has paid for repeatedly, so what follows
# separates what a command answers from what no command can.
#
# WHAT REPRODUCES, with the command that answers it:
#
#   gh repo view newmassrael/watching-zenoh --json createdAt,isPrivate
#     -> created 2026-08-25T07:02:05Z, public
#   git -C /home/coin/watching-zenoh log --all --reverse --format=%aI | head -1
#     -> 2026-04-24
#   gh run list -R newmassrael/watching-zenoh --json databaseId -q length
#     -> 2, against 3380 local commits
#
# A repository whose earliest commit is four months older than the repository
# itself was deleted and recreated; two CI runs behind three thousand commits
# is the history that cost. Run the same `createdAt` probe across this owner's
# other repositories and every one of them lands within hours of its own first
# commit -- so this is a signature, not the normal spread.
#
#   for r in <the owner's repos>; do
#       git -C /home/coin/$r log --all --format=%ae; done | sort | uniq -c
#     -> newmassrael@gmail.com everywhere, plus that account's own GitHub
#        noreply forms, a github-actions bot, and one harness artifact.
#        NO foreign address survives anywhere -- which is what a completed
#        rewrite looks like, and is why the offending commits cannot be shown.
#
# WHAT DOES NOT REPRODUCE, and cannot, by construction. The first draft said
# EIGHT commits, served by SHA FOR DAYS, the other account listed as a
# contributor throughout, and 1146 runs of CI lost. Those are first-hand
# observations from the session that lived through it. They are not re-derivable
# from here and never will be: deleting the repository is what destroyed both
# the commits and the run history, so the incident consumed its own evidence.
# They are kept as testimony, labelled as testimony, and no count from them is
# repeated anywhere this file can be read as measurement.
#
# ⚠ AND THE GATE DOES NOT REST ON THEM. Whether it was eight commits or one,
# the reproducible half already carries the argument: a public repository of
# this owner's was destroyed and rebuilt to unpublish an identity, this month.
#
# WHY A CONFIG WOULD NOT HAVE CAUGHT IT. `git config user.email` was already
# correct there, in the clone AND in ~/.gitconfig. The commits came from a
# different environment. A config is a DEFAULT and it is per-clone; a rule
# that has to be present on the machine that gets it wrong has to travel with
# the tree, which is what a tracked hook does.
#
# ★ R1828 — this one REPRODUCES, and it is the strongest half of the argument:
#
#   git config --global user.email                          -> the allowed one
#   git -C /home/coin/watching-zenoh config --local user.email -> the allowed one
#
# Both were already right in the repository the commits escaped from. A config
# that is correct and a history that is wrong is precisely the pair that shows
# grading the config answers a different question -- which is the rule the next
# paragraph states, now standing on a measurement rather than on a memory.
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
        echo "  Why: commits reached a PUBLIC repo of this owner's under a"
        echo "  different address. Rewriting history did NOT un-publish them;"
        echo "  the repository had to be deleted and recreated, and its CI"
        echo "  history went with it. Checkable from here:"
        echo "    gh repo view newmassrael/watching-zenoh --json createdAt"
        echo "  answers a creation date months NEWER than that repo's own"
        echo "  first commit -- the signature of exactly that repair."
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
