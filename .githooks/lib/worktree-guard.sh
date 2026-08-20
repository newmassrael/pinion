#!/usr/bin/env bash
# .githooks/lib/worktree-guard.sh — refuse a commit or a push made from a
# LINKED worktree, so a round can only be closed from the main tree.
#
# ## Why this is a gate and not a sentence
#
# `tools/worktree.sh` creates detached exploration worktrees so several lines of
# enquiry can run at once. Its `add` prints, in as many words, that a worktree
# must not commit, push, or touch `docs/.atomic/`. That printing is not a gate,
# and this project has already paid for the difference: a ratified stop-the-line
# rule lived only in a `.gitignore`d prose file until R1495, and a red lane kept
# a `needs:`-gated job from running for 99 consecutive pushes while everyone
# believed the rule was in force.
#
# The split it defends is measured rather than stylistic. A round's front half
# parallelises; its back half cannot. `docs/.atomic/workspace.atomic.json` is a
# single 14 MB store whose merge `.gitattributes` now *refuses*, because
# resolving a conflict in it would itself be the hand edit CLAUDE.md forbids —
# so two rounds closing at once have no correct resolution, only "drop one side
# and re-apply through the CLI". `docs/phase-b-rounds.tsv` and the push are
# serial for the same reason.
#
# ## The discriminator, measured
#
# In the main tree `--absolute-git-dir` and `--git-common-dir` are the same
# path. In a linked worktree the first points inside the second:
#
#   main tree      absolute-git-dir /home/coin/pinion/.git
#                  git-common-dir   /home/coin/pinion/.git
#   linked         absolute-git-dir /home/coin/pinion/.git/worktrees/gate
#                  git-common-dir   /home/coin/pinion/.git
#
# That is the whole test. It reads no path convention, so it holds for a
# worktree created by hand in any location, not only the ones the script makes.
#
# ## The override
#
# `PINION_WRITE_FROM_WORKTREE=1` proceeds anyway, loudly. It exists because the
# universal bypass — `--no-verify` — already exists and disables *every* gate:
# a named override records WHICH rule was set aside, which is the difference
# between an auditable exception and a silent one. Like the stop-the-line
# override, it needs an explicit request in the moment; it is not standing
# permission.

# Pure classifier. Takes the two paths and the override value rather than
# calling git, so every verdict can be driven from tools/test_hooks.sh without
# building a worktree.
#
# Verdicts: main | override | linked
worktree_verdict() {
    # One trailing slash stripped from each: the two paths come from the same
    # git normalisation today, but comparing raw strings is the kind of check
    # that passes for years and then fails on a form nobody produced before.
    local git_dir="${1%/}" common_dir="${2%/}" override="${3:-}"
    if [[ "$git_dir" == "$common_dir" ]]; then
        printf 'main\n'
    elif [[ -n "$override" ]]; then
        printf 'override\n'
    else
        printf 'linked\n'
    fi
}

# Hook entry point. `$1` is the operation being guarded, used only in the
# message ("commit" / "push"). Returns non-zero when the operation must be
# refused; the caller decides how to exit.
require_main_worktree_for() {
    local operation="$1"
    local git_dir common_dir verdict
    git_dir="$(git rev-parse --absolute-git-dir)"
    common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
    verdict="$(worktree_verdict "$git_dir" "$common_dir" "${PINION_WRITE_FROM_WORKTREE:-}")"

    case "$verdict" in
        main)
            return 0
            ;;
        override)
            echo "worktree-guard: PINION_WRITE_FROM_WORKTREE set -- allowing $operation from $git_dir" >&2
            return 0
            ;;
        *)
            cat >&2 <<EOF
worktree-guard: refusing to $operation from a linked worktree.

  this worktree : $git_dir
  main tree     : ${common_dir%/.git}

Exploration worktrees are for measuring, prototyping, building, testing and
demos. A round is CLOSED from the main tree, because the ledger, the Phase B
row and the push are serial -- and the ledger's merge is refused outright, so a
parallel close has no correct resolution.

  * carry the work over and commit there, or
  * PINION_WRITE_FROM_WORKTREE=1 git $operation ...   (say why; not standing
    permission -- and unlike --no-verify it records which rule was set aside)
EOF
            return 1
            ;;
    esac
}
