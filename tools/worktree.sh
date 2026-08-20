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
# Nothing here enforces these yet — they are printed on every `add` so the
# session standing in one has read them:
#
#   * no commit, no push (the round lands from the main tree)
#   * no mutation of `docs/.atomic/` (the ledger is main-tree-only)
#   * no `docs/phase-b-rounds.tsv` row
#
# ## Usage
#
#   tools/worktree.sh add <name>       # create; prints the DISPLAY to export
#   tools/worktree.sh list             # every worktree plus its cache and display
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

# Displays currently held on this machine, as a space-separated list. X sockets
# are the observable fact; a number with a socket is taken whether or not the
# server behind it belongs to us.
taken_displays() {
    local out=() sock
    for sock in /tmp/.X11-unix/X*; do
        [[ -e $sock ]] || continue
        out+=("${sock##*/X}")
    done
    printf '%s\n' "${out[*]:-}"
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

    local repo_root wt cache state display
    repo_root="$(git rev-parse --show-toplevel)"
    wt="$(wt_dir_for "$name")"
    cache="$(cache_dir_for "$name")"
    state="$(state_file_for "$name")"

    [[ -e $wt ]] && die "$wt already exists"

    display="$(pick_display "$(taken_displays)")" \
        || die "no free display in :$DISPLAY_LOW-:$DISPLAY_HIGH"

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

    printf 'PINION_WT_NAME=%s\nPINION_WT_DISPLAY=%s\nPINION_WT_CACHE=%s\n' \
        "$name" "$display" "$cache" > "$state"

    verify_worktree "$name" "$wt" "$cache"

    cat <<EOF

worktree '$name' ready.

  cd $wt
  export DISPLAY=:$display        # this worktree's own headless display
                                  # (Xvfb :$display must be started separately)

This worktree is for EXPLORATION. It must not:
  * commit or push -- the round lands from $repo_root
  * mutate docs/.atomic/ -- the ledger is main-tree-only, and its merge is
    refused by .gitattributes precisely so this cannot be done by accident
  * add a docs/phase-b-rounds.tsv row

Tear down with: tools/worktree.sh remove $name
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
        ( set -a; . "$f"; printf 'worktree: %-16s display :%-4s cache %s\n' \
            "$PINION_WT_NAME" "$PINION_WT_DISPLAY" "$PINION_WT_CACHE" )
    done
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

    # path composition
    check "cache dir" "$CACHE_ROOT/pinion-x" "$(cache_dir_for x)"
    check "worktree dir" "$WT_HOME/x" "$(wt_dir_for x)"
    check "state file" "$WT_HOME/.state/x.env" "$(state_file_for x)"

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
    remove)     shift; cmd_remove "${1:-}" ;;
    --selftest) cmd_selftest ;;
    ""|-h|--help)
        sed -n '/^# ## Usage/,/^$/p' "$0" | sed 's/^# \?//'
        ;;
    *) die "unknown command '${1}' (try --help)" ;;
esac
