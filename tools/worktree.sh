#!/usr/bin/env bash
# tools/worktree.sh — create, list and tear down *exploration* worktrees.
#
# ## What this is for
#
# A round's work splits into two halves with different parallelism. The front
# half — measuring, prototyping, building, testing, running demos — is
# embarrassingly parallel. The back half — the atomic store, the Phase B row,
# the memory index, the push — is serial by construction: the ledger is a single
# 14 MB JSON whose merge is forbidden (see `.gitattributes`), and there is one
# `main`.
#
# So this creates worktrees for the FRONT half only. They are created
# **detached**, never on a branch, and nothing here commits. That keeps the
# ratified single-linear-`main` history intact while letting several lines of
# enquiry run at once, which is worth the most exactly when the unknowns are
# things only a real build can answer.
#
# ## Why a script rather than `git worktree add`
#
# Three pieces of this machine do not follow a bare `git worktree add`, and all
# three fail QUIETLY:
#
#   1. `target/` is a SYMLINK onto the compressed fixed-size build-cache volume
#      (R1489). A fresh worktree has no symlink, so cargo materialises a real
#      `target/` on the root filesystem — uncompressed, unswept, and outside the
#      volume whose size *is* the quota. That is the 2026-07-29 full-disk
#      incident's exact precondition, re-created.
#   2. Submodules are not populated by `git worktree add`. `vendor/sce` and
#      `vendor/mnemosyne` come up empty, and `vendor/mnemosyne` is what the
#      hooks resolve their gate tool from.
#   3. The headless demo display is a SHARED resource. The standing convention
#      pins one display number, so two worktrees sweeping at once draw into each
#      other's screen and both readings are garbage. Each worktree gets its own.
#   4. `git worktree remove` REFUSES a worktree containing submodules, and this
#      repo always has two -- so the obvious teardown does not work at all, and
#      the obvious fix for that (`submodule deinit`) would damage the main tree
#      through the shared `.git/config`. See `cmd_remove`.
#
# Items 1-3 were written from the review that preceded this script. Item 4, the
# `--recursive` over-reach in `cmd_add`, and a submodule check that reported
# "verified" on a fatal git error were all found by RUNNING it -- the selftest
# had passed on every one of them.
#
# The build-cache directory is placed so the machine-wide sweep timer finds it:
# `buildcache-sweep` discovers projects with `find $HOME -maxdepth 3 -type l
# -name target`, and `$HOME/pinion-wt/<name>/target` is exactly depth 3.
# Teardown removes the cache directory too, because a cache with no symlink
# pointing at it is reported as an orphan.
#
# ## What a worktree may NOT do
#
# Printed on every `add` so the session standing in one has read them, and
# since R1759 the first two are ENFORCED — by `land`, which refuses to carry
# them back:
#
#   * no commit, no push (the round lands from the main tree)
#   * no mutation of `docs/.atomic/` (the ledger is main-tree-only)
#   * no `docs/phase-b-rounds.tsv` row
#
# ## The way back (R1759)
#
# `add` / `list` / `remove` were the whole surface until R1759, which is to say
# the front half of a round had a tool and the HANDOFF BETWEEN THE HALVES had
# none. R1757 landed by hand and the cost is what this section exists to
# remove: the patch was exported, `git apply` was tried, then a `cp` loop, and
# the three safety questions that make any of it legitimate — do the two trees
# agree on HEAD, does my file set collide with what the main tree already has
# uncommitted, did the worktree touch a main-tree-only file — were answered by
# hand with `comm -12` and eyes. Two of those three are the ones that damage
# someone ELSE's work, and a check made by hand is a check that will one day
# not be made. `land` asks all three and REFUSES; it never merges.
#
# It also cannot express a deletion, a rename or a submodule pin move, so it
# refuses those by name rather than guessing — the `classify_submodule_status`
# precedent, where the wrong answer was worse than no answer.
#
# ## Round numbers are CLAIMED, not derived (R1759)
#
# `git log` cannot see a round that has not committed yet, so two sessions
# starting the same afternoon both derive the same next number. Measured
# 2026-08-21: R1757 and R1758 were both begun as "R1757", and the collision
# surfaced only because one session happened to read the other's memory file —
# after which 85 sites had to be renumbered. `add` claims the next free number
# in this script's own state directory and `list` shows every claim, so the
# answer is visible before anything is committed.
#
# ## Usage
#
#   tools/worktree.sh add <name>       # create; claims a round, prints the DISPLAY
#   tools/worktree.sh list             # every worktree plus its cache, display, round
#   tools/worktree.sh land <name>      # carry the work back to the main tree
#   tools/worktree.sh remove <name>    # tear down, including the cache directory
#   tools/worktree.sh --selftest       # pure-logic checks, no git, no writes

set -euo pipefail

WT_HOME="${PINION_WT_HOME:-$HOME/pinion-wt}"
CACHE_ROOT="${BUILDCACHE_ROOT:-$HOME/.buildcache}"
STATE_DIR="$WT_HOME/.state"
# Display numbers this script may hand out. Deliberately excludes the standing
# sweep display and anything a desktop session is likely to hold.
DISPLAY_LOW="${PINION_WT_DISPLAY_LOW:-90}"
DISPLAY_HIGH="${PINION_WT_DISPLAY_HIGH:-96}"

say()  { printf 'worktree: %s\n' "$*"; }
warn() { printf 'worktree: %s\n' "$*" >&2; }
die()  { warn "$*"; exit 1; }

# ---------------------------------------------------------------- pure helpers
# Kept free of git and of the filesystem so `--selftest` can exercise them.

# A name becomes a directory, a build-cache directory and a git worktree id, so
# it is restricted to what all three accept without quoting.
valid_name() {
    local name="$1"
    [[ ${#name} -ge 1 && ${#name} -le 32 ]] || return 1
    [[ $name =~ ^[a-z0-9][a-z0-9-]*$ ]] || return 1
    # `.state` is this script's own bookkeeping directory under $WT_HOME.
    [[ $name != "state" ]] || return 1
    return 0
}

cache_dir_for() { printf '%s/pinion-%s\n' "$CACHE_ROOT" "$1"; }
wt_dir_for()    { printf '%s/%s\n' "$WT_HOME" "$1"; }
state_file_for() { printf '%s/%s.env\n' "$STATE_DIR" "$1"; }

# Lowest display number in range that is not in the caller-supplied taken list.
# The list is passed in (rather than probed here) so the selftest can drive it.
pick_display() {
    local taken=" $1 "
    local n
    for (( n = DISPLAY_LOW; n <= DISPLAY_HIGH; n++ )); do
        [[ $taken == *" $n "* ]] && continue
        printf '%s\n' "$n"
        return 0
    done
    return 1
}

# Displays with a RUNNING X server, as a space-separated list. A number with a
# socket is taken whether or not the server behind it belongs to us.
x_socket_displays() {
    local out=() sock
    for sock in /tmp/.X11-unix/X*; do
        [[ -e $sock ]] || continue
        out+=("${sock##*/X}")
    done
    printf '%s\n' "${out[*]:-}"
}

# Displays this script has already handed out, read back from its own state
# files.
#
# THIS IS HALF THE ANSWER AND ITS ABSENCE WAS A BUG. Until R1745 `taken_displays`
# was the socket scan alone, so a display allocated to a worktree whose Xvfb had
# not been started yet still looked free: create two worktrees back to back and
# BOTH were handed :90, which is the exact collision this allocation exists to
# prevent. One worktree at a time could never surface it -- and the selftest
# could not either, because it fed `pick_display` a taken list directly and so
# never exercised how that list is BUILT.
#
# A state file whose worktree directory is gone holds nothing, so a hand-deleted
# worktree releases its display instead of reserving it forever.
allocated_displays() {
    local f name key value out=()
    for f in "$STATE_DIR"/*.env; do
        [[ -e $f ]] || continue
        name="$(basename "$f" .env)"
        [[ -d "$(wt_dir_for "$name")" ]] || continue
        while IFS='=' read -r key value; do
            [[ $key == "PINION_WT_DISPLAY" ]] && out+=("$value")
        done < "$f"
    done
    printf '%s\n' "${out[*]:-}"
}

# Union of two space-separated number lists, de-duplicated. Pure, so the
# selftest can drive it.
merge_displays() {
    local n seen=" " out=()
    # Word splitting on both arguments is the point here.
    # shellcheck disable=SC2086
    for n in $1 $2; do
        [[ $seen == *" $n "* ]] && continue
        seen+="$n "
        out+=("$n")
    done
    printf '%s\n' "${out[*]:-}"
}

# Every display that must not be handed out: the ones a server holds AND the
# ones this script has already promised.
taken_displays() {
    merge_displays "$(x_socket_displays)" "$(allocated_displays)"
}

# Classify one `git submodule status` run into a verdict token. Takes the exit
# status and the output as arguments -- rather than running git itself -- so the
# four outcomes can be driven from `--selftest` without a repository.
#
#   failed      the command did not succeed; its output means nothing
#   empty       it succeeded but listed no submodules at all
#   unpopulated at least one submodule has no working tree ('-' prefix)
#   ok          every listed submodule is populated
classify_submodule_status() {
    local rc="$1" output="$2"
    if (( rc != 0 )); then
        printf 'failed\n'
    elif [[ -z ${output//[[:space:]]/} ]]; then
        printf 'empty\n'
    elif grep -q '^-' <<<"$output"; then
        printf 'unpopulated\n'
    else
        printf 'ok\n'
    fi
}

# R1759 — paths a worktree may not carry back, because the main tree owns them.
#
# `docs/.atomic/` is one 14 MB JSON whose merge `.gitattributes` REFUSES, so two
# writers cannot both append and resolving the conflict would be the hand edit
# the Mnemosyne contract forbids. `docs/phase-b-rounds.tsv` is `merge=union` and
# would therefore merge silently — which is worse here, not better: the row
# would arrive without the ledger entry it is supposed to accompany.
LEDGER_PATHS=("docs/.atomic/" "docs/phase-b-rounds.tsv")

# The submodules. A pin move is a gitlink, not file content, so copying it back
# would carry nothing; the triple discipline (gitlink + two manifest revs + the
# lock) belongs to the main tree.
SUBMODULE_PATHS=("vendor/sce" "vendor/mnemosyne")

# R1759 — classify one `git status --porcelain` record into a land verdict.
#
# Takes the two status characters and the path as arguments rather than reading
# git, so `--selftest` drives every arm. The verdicts:
#
#   copy              a modification or an addition; `land` copies the file
#   refuse-delete     a removal — a copy cannot express one
#   refuse-rename     a rename — two paths, and the old one must disappear
#   refuse-ledger     main-tree-only (see LEDGER_PATHS)
#   refuse-submodule  a gitlink move (see SUBMODULE_PATHS)
#
# Refusing is deliberate where guessing was available. A delete could be
# `rm`-ed and a rename could be replayed, but both are destructive actions
# derived from a two-character parse, and this file's own history says what
# that is worth: the first `classify_submodule_status` guessed and reported
# "verified" on a fatal error.
classify_land_status() {
    local xy="$1" path="$2" ledger sub
    # A rename is R in either column; the record carries two paths.
    [[ $xy == R* || $xy == *R ]] && { printf 'refuse-rename\n'; return; }
    # A delete in either column. `??` must not reach this test -- it does not.
    [[ $xy == D* || $xy == *D ]] && { printf 'refuse-delete\n'; return; }
    for sub in "${SUBMODULE_PATHS[@]}"; do
        [[ $path == "$sub" || $path == "$sub/"* ]] && { printf 'refuse-submodule\n'; return; }
    done
    for ledger in "${LEDGER_PATHS[@]}"; do
        # A trailing slash means "this directory"; otherwise an exact path.
        if [[ $ledger == */ ]]; then
            [[ $path == "$ledger"* ]] && { printf 'refuse-ledger\n'; return; }
        else
            [[ $path == "$ledger" ]] && { printf 'refuse-ledger\n'; return; }
        fi
    done
    printf 'copy\n'
}

# R1759 — the paths present in BOTH newline-separated lists, one per line.
#
# This is the check that protects the other session's work: if a file I edited
# in the worktree is also uncommitted in the main tree, copying mine over it
# destroys theirs with no diff to review and no reflog to recover from. R1757
# ran the equivalent `comm -12` by hand and got an empty answer; the next round
# might not, and might not look.
#
# Sorting is internal so callers may pass the lists in any order.
land_overlap() {
    comm -12 <(printf '%s\n' "$1" | sort -u) <(printf '%s\n' "$2" | sort -u) | grep -v '^$' || true
}

# R1759 — the highest round number appearing in a block of commit subjects.
#
# Pure, so `--selftest` drives it without git. Subjects here look like
# `feat(rpc): R1757 a burst of keys arrives together`, and the ledger also
# holds a historical `Round <n>` form, so both are read. `0` when the text
# names none, which keeps the caller's arithmetic total.
newest_round_in() {
    grep -oE '\b(R|Round )[0-9]+' <<<"${1:-}" \
        | grep -oE '[0-9]+' \
        | sort -n \
        | tail -1 \
        || true
}

# R1759 — the round `add` should claim: one past the highest number that is
# either COMMITTED or already CLAIMED by a live worktree.
#
# The second half is the whole point. `git log` cannot see a round that has not
# committed yet, which is how two sessions came to begin the same afternoon as
# "R1757".
next_round() {
    local git_newest="${1:-0}" claimed="${2:-}" highest n
    highest="${git_newest:-0}"
    for n in $claimed; do
        (( n > highest )) && highest="$n"
    done
    printf '%s\n' "$(( highest + 1 ))"
}

# Rounds this script has already handed out, read back from its own state
# files. Same "the directory is gone, so the claim is gone" rule the display
# allocator uses -- a hand-deleted worktree must not reserve a number forever.
claimed_rounds() {
    local f name key value out=()
    for f in "$STATE_DIR"/*.env; do
        [[ -e $f ]] || continue
        name="$(basename "$f" .env)"
        [[ -d "$(wt_dir_for "$name")" ]] || continue
        while IFS='=' read -r key value; do
            [[ $key == "PINION_WT_ROUND" ]] && out+=("$value")
        done < "$f"
    done
    printf '%s\n' "${out[*]:-}"
}

# ------------------------------------------------------------------ guardrails

require_main_worktree() {
    local git_dir common_dir
    git_dir="$(git rev-parse --absolute-git-dir)"
    common_dir="$(git rev-parse --path-format=absolute --git-common-dir)"
    [[ $git_dir == "$common_dir" ]] \
        || die "run this from the MAIN worktree, not from a linked one ($git_dir)"
}

require_cache_volume() {
    # The point of the symlink is the fixed-size compressed volume. If it is not
    # mounted, the link would aim at a plain directory on the root filesystem and
    # every guarantee in the header evaporates -- silently, which is why this is
    # checked rather than assumed.
    mountpoint -q "$CACHE_ROOT" \
        || die "$CACHE_ROOT is not a mountpoint; refusing to link a target/ into it"
}

# ---------------------------------------------------------------------- add

cmd_add() {
    local name="${1:-}"
    [[ -n $name ]] || die "usage: tools/worktree.sh add <name>"
    valid_name "$name" \
        || die "invalid name '$name' (need ^[a-z0-9][a-z0-9-]*$, 1-32 chars)"

    require_main_worktree
    require_cache_volume

    local repo_root wt cache state display round
    repo_root="$(git rev-parse --show-toplevel)"
    wt="$(wt_dir_for "$name")"
    cache="$(cache_dir_for "$name")"
    state="$(state_file_for "$name")"

    [[ -e $wt ]] && die "$wt already exists"

    display="$(pick_display "$(taken_displays)")" \
        || die "no free display in :$DISPLAY_LOW-:$DISPLAY_HIGH"

    # R1759 — claim a round number before anything is created, so a second
    # session running `list` sees it immediately. 200 subjects is far more than
    # the span any two live worktrees cover, and reading more costs nothing but
    # says nothing either.
    round="$(next_round \
        "$(newest_round_in "$(git -C "$repo_root" log --format=%s -200)")" \
        "$(claimed_rounds)")"

    mkdir -p "$WT_HOME" "$STATE_DIR"

    # Detached on purpose: no branch is created, so the single-linear-main
    # history is untouched and nothing here can be pushed by accident.
    say "creating detached worktree at $wt"
    git -C "$repo_root" worktree add --detach "$wt" HEAD >/dev/null

    say "linking target/ -> $cache"
    mkdir -p "$cache"
    ln -s "$cache" "$wt/target"

    # NOT `--recursive`. Measured on the main tree: `vendor/sce`'s four nested
    # submodules are deliberately left un-populated there
    # (`git -C vendor/sce submodule status` prefixes all four with '-'), and the
    # `third_party/` directories that do exist are vendored source inside the
    # sce repository rather than submodules. A recursive init therefore clones
    # four repositories over the network that the main tree does not have and
    # the build does not want. A worktree must MIRROR the main tree, not
    # maximise it.
    say "populating submodules (vendor/sce, vendor/mnemosyne)"
    git -C "$wt" submodule update --init >/dev/null

    printf 'PINION_WT_NAME=%s\nPINION_WT_DISPLAY=%s\nPINION_WT_CACHE=%s\nPINION_WT_ROUND=%s\n' \
        "$name" "$display" "$cache" "$round" > "$state"

    verify_worktree "$name" "$wt" "$cache"

    cat <<EOF

worktree '$name' ready, holding round R$round.

  cd $wt
  export DISPLAY=:$display        # this worktree's own headless display
                                  # (Xvfb :$display must be started separately)

R$round IS CLAIMED, not derived. \`git log\` cannot see a round that has not
committed, so two sessions starting the same afternoon both compute the same
next number -- measured 2026-08-21, when R1757 and R1758 were both begun as
"R1757" and 85 sites had to be renumbered. \`tools/worktree.sh list\` shows every
claim, including this one, before anything is committed.

BUILDS: \`bx\` resolves its repo root from the CURRENT DIRECTORY
(\`git rev-parse --show-toplevel\`), and it has no override, so from a shell
pinned to the main tree it sends the MAIN tree to a build host and then runs
your --manifest-path against a directory that is not there (measured: exit 101
in 17s). Until \`bx\` grows a --root, paste this and use \`wtb\` instead:

  wtb() { BX_LOCAL_REASON="testing linked worktree $name; bx resolves its root \\
from cwd and has no override" \\
    "\$HOME/.claude/remote-build/bin/bx" --local -- \\
    "\$@" --manifest-path $wt/Cargo.toml; }

  wtb cargo test -p pinion-core        # -> runs against $name

⚠ That forces LOCAL execution and the build fleet stays idle, which on a busy
host is the round's largest avoidable cost. It is a stopgap; the real fix is a
\`--root\` flag in bx, which lives in its own repository.

This worktree is for EXPLORATION. It must not:
  * commit or push -- the round lands from $repo_root
  * mutate docs/.atomic/ -- the ledger is main-tree-only, and its merge is
    refused by .gitattributes precisely so this cannot be done by accident
  * add a docs/phase-b-rounds.tsv row
  (\`land\` refuses to carry the last two back, so this is enforced and not
   only asked.)

⚠ This tree's build cache is EMPTY. A failure here that the main tree does not
show may be about the cache -- or about the FLAGS you happen to be passing.
Isolate one variable before naming a cause: R1757 registered a rustc ICE as
"cold cache" when the discriminator was actually \`--document-private-items\`,
and the wrong name nearly entered a frozen ledger.

Carry it back with: tools/worktree.sh land $name
Tear down with:     tools/worktree.sh remove $name
EOF
}

# Every claim the header makes about a fresh worktree is asserted here, because
# all three failure modes it exists to prevent are silent ones.
verify_worktree() {
    local name="$1" wt="$2" cache="$3" hooks head_ref
    local failures=0

    hooks="$(git -C "$wt" rev-parse --path-format=absolute --git-path hooks)"
    if [[ $hooks == "$wt/.githooks" ]]; then
        say "verified: hooks resolve to this worktree ($hooks)"
    else
        warn "FAIL: hooks resolve to '$hooks', expected '$wt/.githooks'"
        failures=$((failures + 1))
    fi

    if [[ -L "$wt/target" && "$(readlink -f "$wt/target")" == "$(readlink -f "$cache")" ]]; then
        say "verified: target/ is a symlink onto the build-cache volume"
    else
        warn "FAIL: $wt/target is not a symlink onto $cache"
        failures=$((failures + 1))
    fi

    # The first draft of this check was `submodule status | grep -q '^-'`, and
    # it printed "verified" on a run where git had exited FATAL -- a failed
    # pipeline produces no '-' lines, so the absence of evidence read as
    # evidence of absence. It also never said how many submodules it had looked
    # at, so a status that listed nothing would have passed too. Both are the
    # same defect: a check must distinguish its outcomes, and it must say what
    # its population was.
    local sm_out sm_rc=0 sm_verdict sm_count
    sm_out="$(git -C "$wt" submodule status 2>&1)" || sm_rc=$?
    sm_verdict="$(classify_submodule_status "$sm_rc" "$sm_out")"
    sm_count="$(grep -c . <<<"$sm_out" || true)"
    case "$sm_verdict" in
        ok)
            say "verified: $sm_count submodule(s) populated"
            ;;
        unpopulated)
            warn "FAIL: at least one submodule is still un-populated"
            grep '^-' <<<"$sm_out" >&2 || true
            failures=$((failures + 1))
            ;;
        empty)
            warn "FAIL: submodule status listed nothing -- this repo has submodules"
            failures=$((failures + 1))
            ;;
        *)
            warn "FAIL: submodule status exited $sm_rc: $sm_out"
            failures=$((failures + 1))
            ;;
    esac

    head_ref="$(git -C "$wt" symbolic-ref -q HEAD || true)"
    if [[ -z $head_ref ]]; then
        say "verified: HEAD is detached (no branch created)"
    else
        warn "FAIL: HEAD is on branch '$head_ref'; expected detached"
        failures=$((failures + 1))
    fi

    (( failures == 0 )) || die "$failures check(s) failed for worktree '$name'"
}

# --------------------------------------------------------------------- list

cmd_list() {
    require_main_worktree
    git worktree list
    [[ -d $STATE_DIR ]] || return 0
    local f name
    printf '\n'
    for f in "$STATE_DIR"/*.env; do
        [[ -e $f ]] || continue
        name="$(basename "$f" .env)"
        # shellcheck disable=SC1090  # generated by this script, fixed shape
        ( set -a; . "$f"; printf 'worktree: %-16s round R%-6s display :%-4s cache %s\n' \
            "$PINION_WT_NAME" "${PINION_WT_ROUND:-?}" \
            "$PINION_WT_DISPLAY" "$PINION_WT_CACHE" )
    done
}

# --------------------------------------------------------------------- land

# R1759 — carry a worktree's work back to the main tree.
#
# Copies files. It does not merge, does not commit, and does not stage: what it
# prints at the end is the exact `git add` line for what it moved, so the round
# still closes by hand in the main tree where the ledger lives.
#
# The three refusals are the reason this exists. Two of them protect work that
# is not yours.
cmd_land() {
    local name="${1:-}"
    [[ -n $name ]] || die "usage: tools/worktree.sh land <name>"
    valid_name "$name" || die "invalid name '$name'"
    require_main_worktree

    local wt repo_root
    wt="$(wt_dir_for "$name")"
    repo_root="$(git rev-parse --show-toplevel)"
    [[ -d $wt ]] || die "no worktree at $wt"

    # (1) The two trees must agree on HEAD. Copying a file built against a
    # different base is how a silent semantic conflict is made: the bytes apply
    # cleanly and the meaning does not. R1757 checked this by hand.
    local wt_head main_head
    wt_head="$(git -C "$wt" rev-parse HEAD)"
    main_head="$(git -C "$repo_root" rev-parse HEAD)"
    if [[ $wt_head != "$main_head" ]]; then
        warn "worktree is at ${wt_head:0:8}, main tree at ${main_head:0:8}"
        die "rebase the worktree onto the main tree's HEAD first, then re-run and RE-RUN ITS GATES -- a landing that skips them is a landing onto an untested base"
    fi

    # (2) Classify every change. `-z` because git quotes odd paths in the plain
    # porcelain and a quoted path would be copied to the wrong name.
    local -a copy=() refused=()
    local xy path verdict record
    while IFS= read -r -d '' record; do
        [[ -n $record ]] || continue
        xy="${record:0:2}"
        path="${record:3}"
        verdict="$(classify_land_status "$xy" "$path")"
        case "$verdict" in
            copy) copy+=("$path") ;;
            *)    refused+=("$verdict $path") ;;
        esac
    done < <(git -C "$wt" status --porcelain=v1 -z)

    if (( ${#refused[@]} > 0 )); then
        # Not backticks: inside a double-quoted bash string they COMMAND
        # SUBSTITUTE, so the first draft of this line ran `land` as a program
        # and printed "command not found" above the refusal it was announcing.
        warn "this worktree changed things 'land' cannot carry:"
        printf '  %s\n' "${refused[@]}" >&2
        die "${#refused[@]} refusal(s) -- a ledger/submodule path belongs to the main tree, and a delete or rename must be replayed by hand"
    fi
    (( ${#copy[@]} > 0 )) || die "worktree '$name' has no changes to land"

    # (3) THE CHECK THAT PROTECTS SOMEONE ELSE. A file uncommitted in BOTH trees
    # would be overwritten with no diff to review and nothing to recover from.
    local main_changed wt_changed overlap
    main_changed="$(git -C "$repo_root" status --porcelain=v1 | cut -c4-)"
    wt_changed="$(printf '%s\n' "${copy[@]}")"
    overlap="$(land_overlap "$wt_changed" "$main_changed")"
    if [[ -n $overlap ]]; then
        warn "these paths are uncommitted in BOTH trees:"
        printf '  %s\n' "$overlap" >&2
        die "refusing -- landing would destroy the main tree's copy. Commit or stash there first."
    fi

    local f
    for f in "${copy[@]}"; do
        mkdir -p "$repo_root/$(dirname "$f")"
        cp "$wt/$f" "$repo_root/$f"
    done

    # (4) Verify the bytes arrived, rather than trusting that `cp` said nothing.
    local mismatched=0
    for f in "${copy[@]}"; do
        cmp -s "$wt/$f" "$repo_root/$f" || { warn "FAIL: $f did not copy identically"; mismatched=$((mismatched + 1)); }
    done
    (( mismatched == 0 )) || die "$mismatched file(s) did not land"

    say "landed ${#copy[@]} file(s) from '$name' onto ${main_head:0:8}"
    local round=""
    # shellcheck disable=SC1090  # generated by this script, fixed shape
    [[ -e "$(state_file_for "$name")" ]] && round="$( . "$(state_file_for "$name")"; printf '%s' "${PINION_WT_ROUND:-}" )"

    cat <<EOF

Stage exactly what landed (never \`git add -A\` -- this tree may hold work that
is not yours):

  git add ${copy[*]}

Then close the round HERE, in the main tree, because the worktree cannot:
  * mnemosyne-cli append-changelog-entry --entry-id R${round:-<NNNN>} ...
  * one row appended to docs/phase-b-rounds.tsv
  * commit (the hooks compile the WHOLE working tree, so anything else
    uncommitted here must compile too)

⚠ Re-run the round's gates in THIS tree before committing. The worktree's
verdicts were taken against its own build cache and its own copy of every file
you did not touch.
EOF
}

# ------------------------------------------------------------------- remove

cmd_remove() {
    local name="${1:-}"
    [[ -n $name ]] || die "usage: tools/worktree.sh remove <name>"
    valid_name "$name" || die "invalid name '$name'"
    require_main_worktree

    local wt cache state
    wt="$(wt_dir_for "$name")"
    cache="$(cache_dir_for "$name")"
    state="$(state_file_for "$name")"

    # The cache path is computed, and this deletes it. Re-derive and check it
    # rather than trusting the composition above.
    [[ $cache == "$CACHE_ROOT/pinion-$name" ]] || die "refusing: cache path '$cache' is not the expected shape"
    [[ $cache != "$CACHE_ROOT" && $cache != "/" ]] || die "refusing: cache path '$cache' is a root"

    # `git worktree remove` CANNOT be used here. Measured: it refuses outright
    # with "cannot move or remove a working tree containing submodules", and
    # this repo always has two. The tempting fix -- `submodule deinit` first --
    # is worse than the problem: deinit rewrites `submodule.<name>.url` in
    # `.git/config`, which worktrees SHARE (there is no
    # `extensions.worktreeConfig` here), so tearing down an exploration
    # worktree would strip the main tree's submodule configuration.
    #
    # So the removal goes the way git documents for a worktree whose directory
    # is gone: delete the directory, then `prune`. Config is never touched.
    [[ $wt == "$WT_HOME/$name" ]] || die "refusing: worktree path '$wt' is not the expected shape"
    [[ $wt != "$WT_HOME" && $wt != "$HOME" && $wt != "/" ]] || die "refusing: '$wt' is a root"
    if [[ -e $wt ]]; then
        # The symlink goes first so nothing in the tree still reaches into the
        # shared cache volume while it is being deleted.
        [[ -L "$wt/target" ]] && rm "$wt/target"
        say "removing worktree directory $wt"
        rm -rf -- "$wt"
    else
        warn "no worktree at $wt (removing its leftovers anyway)"
    fi

    if [[ -d $cache ]]; then
        say "removing build cache $cache"
        rm -rf -- "$cache"
    fi
    [[ -e $state ]] && rm "$state"

    git worktree prune
    say "removed '$name'"
}

# ----------------------------------------------------------------- selftest

cmd_selftest() {
    local failures=0
    check() {
        local label="$1" expect="$2" got="$3"
        if [[ $expect == "$got" ]]; then
            return 0
        fi
        warn "selftest FAIL: $label -- expected '$expect', got '$got'"
        failures=$((failures + 1))
    }

    # names that must be accepted
    local ok
    for ok in a android jni-seam ndk2 a-b-c 0; do
        valid_name "$ok" || { warn "selftest FAIL: '$ok' should be valid"; failures=$((failures + 1)); }
    done
    # names that must be refused -- each for a different reason, so a single
    # over-broad pattern cannot pass them all
    local bad
    for bad in "" "-lead" "Upper" "has space" "has/slash" "trailing_" "state" \
               "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; do
        if valid_name "$bad"; then
            warn "selftest FAIL: '$bad' should be invalid"
            failures=$((failures + 1))
        fi
    done

    # display allocation: lowest free in range, and exhaustion is an error
    local saved_low="$DISPLAY_LOW" saved_high="$DISPLAY_HIGH"
    DISPLAY_LOW=90; DISPLAY_HIGH=92
    check "empty taken list -> low bound" "90" "$(pick_display "")"
    check "skips taken" "91" "$(pick_display "90")"
    check "skips a run" "92" "$(pick_display "90 91")"
    check "ignores out-of-range" "90" "$(pick_display "1 97 99")"
    # A substring match would read '9' as taking '90'; it must not.
    check "matches whole numbers only" "90" "$(pick_display "9")"
    if pick_display "90 91 92" >/dev/null 2>&1; then
        warn "selftest FAIL: exhausted range should fail"
        failures=$((failures + 1))
    fi
    DISPLAY_LOW="$saved_low"; DISPLAY_HIGH="$saved_high"

    # submodule verdicts -- the four outcomes must be distinguishable. The first
    # draft of the caller could not tell 'failed' from 'ok', and said "verified"
    # on a fatal git error, so each of these is a regression test for a bug that
    # actually shipped in this file.
    check "rc!=0 is a failure whatever the output says" \
        "failed" "$(classify_submodule_status 128 " 183a17a5 vendor/mnemosyne")"
    check "rc!=0 with empty output is still a failure" \
        "failed" "$(classify_submodule_status 1 "")"
    check "success with no rows is not a pass" \
        "empty" "$(classify_submodule_status 0 "")"
    check "whitespace-only output is not a pass" \
        "empty" "$(classify_submodule_status 0 "   ")"
    check "leading dash means un-populated" \
        "unpopulated" "$(classify_submodule_status 0 "-6e52d0a0 third_party/pugixml")"
    check "un-populated wins over a populated sibling" \
        "unpopulated" "$(classify_submodule_status 0 " 183a17a5 vendor/mnemosyne
-6e52d0a0 third_party/pugixml")"
    check "all populated is ok" \
        "ok" "$(classify_submodule_status 0 " 183a17a5 vendor/mnemosyne
 e0fdd46b vendor/sce")"
    # A dash that is not in column 1 is a path character, not a status flag.
    check "dash inside a path is not a status flag" \
        "ok" "$(classify_submodule_status 0 " 183a17a5 vendor/some-dashed-name")"

    # the union that feeds pick_display -- the composition the ORIGINAL selftest
    # skipped, which is why the two-worktree collision survived it
    check "union of two lists" "90 92 91" "$(merge_displays "90 92" "91")"
    check "union de-duplicates" "90 91" "$(merge_displays "90 91" "90")"
    check "union of empty and a list" "97" "$(merge_displays "" "97")"
    check "union of a list and empty" "97" "$(merge_displays "97" "")"
    check "union of two empties" "" "$(merge_displays "" "")"

    # ★ THE REGRESSION TEST FOR THE BUG ITSELF: an allocation with no X server
    # behind it yet must still take its display. Before R1745 the left operand
    # was the whole answer and this returned 90.
    DISPLAY_LOW=90; DISPLAY_HIGH=92
    check "an allocated display with no socket is still taken" \
        "91" "$(pick_display "$(merge_displays "" "90")")"
    check "sockets and allocations are both honoured" \
        "92" "$(pick_display "$(merge_displays "91" "90")")"
    DISPLAY_LOW="$saved_low"; DISPLAY_HIGH="$saved_high"

    # path composition
    check "cache dir" "$CACHE_ROOT/pinion-x" "$(cache_dir_for x)"
    check "worktree dir" "$WT_HOME/x" "$(wt_dir_for x)"
    check "state file" "$WT_HOME/.state/x.env" "$(state_file_for x)"

    # ---- R1759: land verdicts. Every arm, because a wrong one either drops a
    # file silently or destroys a path the main tree owns.
    check "a modification is copied" \
        "copy" "$(classify_land_status " M" "crates/pinion-core/src/input.rs")"
    check "a staged addition is copied" \
        "copy" "$(classify_land_status "A " "examples/hello-x/src/judge.rs")"
    check "an untracked file is copied" \
        "copy" "$(classify_land_status "??" "tools/demos/r1759_x.py")"
    check "a tool script is copied" \
        "copy" "$(classify_land_status " M" "tools/rpc_verify.py")"
    # A copy cannot express a removal, and guessing would delete in the main tree.
    check "a worktree delete is refused" \
        "refuse-delete" "$(classify_land_status " D" "crates/pinion-core/src/old.rs")"
    check "a staged delete is refused" \
        "refuse-delete" "$(classify_land_status "D " "crates/pinion-core/src/old.rs")"
    check "a rename is refused" \
        "refuse-rename" "$(classify_land_status "R " "crates/a.rs")"
    # ★ `??` must not be read as a delete by a careless character test -- it has
    # no D, but a `[[ $xy == *D* ]]`-shaped check on the PATH would misfire.
    check "an untracked path containing D is still a copy" \
        "copy" "$(classify_land_status "??" "crates/D/mod.rs")"
    # The ledger paths, which are the main tree's alone.
    check "the atomic store is refused" \
        "refuse-ledger" "$(classify_land_status " M" "docs/.atomic/workspace.atomic.json")"
    check "the phase-b row file is refused" \
        "refuse-ledger" "$(classify_land_status " M" "docs/phase-b-rounds.tsv")"
    # ...and a NEIGHBOUR of a ledger path is not a ledger path. A prefix test
    # without the directory slash would refuse this one too.
    check "a docs file that is not the ledger is copied" \
        "copy" "$(classify_land_status " M" "docs/phase-b-axis-history.md")"
    check "a file merely starting with the tsv name is copied" \
        "copy" "$(classify_land_status " M" "docs/phase-b-rounds.tsv.bak")"
    # ★ FOUND BY A COUNTERFACTUAL THAT PASSED. The two ledger entries take
    # DIFFERENT arms -- one is a directory, one is an exact path -- and the
    # assertion above only covers the exact-path arm. Dropping the slash from
    # the directory arm left the whole suite green, so the sibling of the
    # DIRECTORY entry needed its own case. Over-refusal is the quiet direction:
    # `land` would refuse a legitimate file and the reason would look official.
    check "a sibling of the ledger DIRECTORY is copied" \
        "copy" "$(classify_land_status " M" "docs/.atomic-notes.md")"
    check "a file inside the ledger directory is still refused" \
        "refuse-ledger" "$(classify_land_status "??" "docs/.atomic/sidecar.json")"
    # Submodules are gitlinks; copying carries nothing.
    check "a submodule pin move is refused" \
        "refuse-submodule" "$(classify_land_status " M" "vendor/sce")"
    check "a path inside a submodule is refused" \
        "refuse-submodule" "$(classify_land_status " M" "vendor/mnemosyne/src/lib.rs")"
    # ...but a path that merely shares the prefix is not inside it.
    check "vendor-sibling is not a submodule" \
        "copy" "$(classify_land_status " M" "vendor/sce-notes.md")"

    # ---- R1759: the overlap check -- the one that protects another session.
    check "no overlap is empty" "" "$(land_overlap "a.rs
b.rs" "c.rs")"
    check "one shared path is reported" "b.rs" "$(land_overlap "a.rs
b.rs" "b.rs
c.rs")"
    check "order does not matter" "a.rs" "$(land_overlap "b.rs
a.rs" "a.rs")"
    check "an empty main tree overlaps nothing" "" "$(land_overlap "a.rs" "")"
    check "an empty worktree overlaps nothing" "" "$(land_overlap "" "a.rs")"

    # ---- R1759: round claiming. The bug this exists for is the SECOND check:
    # a number nobody has committed yet is still taken.
    check "newest from R-form subjects" "1758" \
        "$(newest_round_in "feat(rpc): R1757 a burst
feat(core): R1758 a verdict")"
    check "the highest wins, not the first line" "1758" \
        "$(newest_round_in "feat(core): R1758 a verdict
chore: R1756 the perf axis")"
    check "the historical Round form is read too" "1519" \
        "$(newest_round_in "docs: Round 1519 the re-tally")"
    check "no round in the text is zero-ish" "" "$(newest_round_in "chore: tidy")"
    check "next round is one past git" "1759" "$(next_round 1758 "")"
    # ★ THE REGRESSION TEST FOR THE COLLISION ITSELF: git says 1756, but a live
    # worktree already holds 1757, so the answer is 1758 and not 1757. Before
    # R1759 this returned 1757 and two sessions built the same round.
    check "a claimed round is taken even though git cannot see it" \
        "1758" "$(next_round 1756 "1757")"
    check "the highest claim wins" "1760" "$(next_round 1756 "1757 1759")"
    check "a claim below git's newest does not lower the answer" \
        "1759" "$(next_round 1758 "1700")"
    check "no git history and no claims still yields a number" "1" "$(next_round "" "")"

    if (( failures == 0 )); then
        say "selftest OK"
        return 0
    fi
    die "$failures selftest failure(s)"
}

# --------------------------------------------------------------------- main

case "${1:-}" in
    add)        shift; cmd_add "${1:-}" ;;
    list)       cmd_list ;;
    land)       shift; cmd_land "${1:-}" ;;
    remove)     shift; cmd_remove "${1:-}" ;;
    --selftest) cmd_selftest ;;
    ""|-h|--help)
        sed -n '/^# ## Usage/,/^$/p' "$0" | sed 's/^# \?//'
        ;;
    *) die "unknown command '${1}' (try --help)" ;;
esac
