# shellcheck shell=bash
# lib/target-budget.sh — bound the build cache the push gate itself creates.
#
# R1486. Measured 2026-07-29: `target/` had reached 198 GiB (191 GiB of it
# `debug/`) and the filesystem was 100% full with 8.1 GiB free. Nothing in
# this repo had ever said how large the cache was, so ~800 rounds of growth
# were invisible until a DIFFERENT session hit the full disk and had to ask
# whether it was safe to clean. `cargo sweep --time 1` reclaimed 165.37 GiB,
# and an incremental `cargo check -p pinion-rpc --all-targets` still finished
# in 2.3s afterwards — so essentially all of it was dead weight.
#
# Two things were missing, and this file supplies both:
#
#   1. THE NUMBER. The size is printed on every push, over budget or not.
#      That is the fact whose absence let the growth run unseen; a bound
#      that only speaks when it fires would still leave the trend invisible.
#      Cost is one `du -sb`, measured at 0.12s on a 69 GiB tree.
#
#   2. A BOUND, enforced rather than documented. `[[r1470-paint-test-opened-
#      the-speakers]]` — a prose warning is not a gate. Being over budget
#      reclaims oldest-first automatically.
#
# Why a SIZE budget and not `--time N` days: what ran out was space, not age.
# A week-based sweep reclaims nothing during a heavy week and deletes useful
# artifacts during a quiet one, because neither reading is about the resource
# that actually failed. `cargo sweep --maxsize` is an invariant on the thing
# that broke, and removing oldest-first naturally preserves exactly the
# artifacts an incremental rebuild wants.
#
# Why this runs AFTER the build gates and not before: clippy and rustdoc BUILD
# the workspace, so their outputs are the newest artifacts in the tree and an
# oldest-first sweep cannot touch them. Run first, it would delete artifacts
# the very next command rebuilds. The ordering is load-bearing, not cosmetic.
#
# Why the push hook owns this at all: this hook is what grows the cache. It
# runs an unconditional `cargo clippy --workspace --all-targets` plus a
# `cargo doc --workspace`, once per push, forever. Making the producer bound
# what it produces is cleanup-after-yourself, not a surprise deletion.
#
# Limits, stated rather than hidden:
#   * `cargo sweep` converts a size budget into a TIMESTAMP cutoff, so it can
#     overshoot. Measured on a 69 GiB tree with `--maxsize 40GB`: it would
#     remove 36.63 GiB, landing at ~32 GiB rather than 40. Safe for this
#     purpose (it only ever means a bit more rebuilding), but it is not a
#     precise trim.
#   * It is not a lock. A concurrently building session's artifacts are the
#     newest in the tree and oldest-first will not reach them, but nothing
#     here coordinates with another process.
#   * Neither a missing `cargo-sweep` nor a failing sweep fails the push. The
#     commit being published is not what is unsafe; blocking a push over disk
#     hygiene would be the wrong priority. Both paths report loudly, and (1)
#     still prints the number every time, so the condition cannot go unseen
#     the way the original 198 GiB did.

# Chosen against a measured steady state: 69 GiB immediately after the R1486
# sweep, with a full workspace debug build, release example binaries and docs
# all present. The slack absorbs many rounds without thrashing, while still
# catching the runaway that prompted this at less than half the size it
# reached. Override per-machine with `PINION_TARGET_BUDGET_GB`.
TARGET_BUDGET_GB_DEFAULT=100

# Drop artifacts built by a toolchain rustup no longer has.
#
# Unconditional, unlike the budget sweep below, because this removal is
# provably free rather than a trade: a toolchain that is not installed cannot
# build anything, so nothing here can ever be reused and deleting it costs no
# rebuild. Budget-gating a free removal would mean carrying dead weight
# whenever the tree happens to be under budget.
#
# Measured 2026-07-29: reclaims NOTHING in this repo, and the reason is worth
# stating so a later reader does not mistake it for a broken step.
# `rust-toolchain.toml` pins `channel = "1.88.0"` — an exact version, not
# `stable` — so every build in the project's history used one rustc, which is
# still installed. There has been no toolchain rotation, so there is no
# rotation debris; the 198 GiB this file exists for was entirely SAME-toolchain
# accretion. It earns its place at the moment that pin moves, which the
# toolchain file anticipates ("Bumping past 1.88 needs a similar trigger from
# a workspace dep"): the tree can then hold two toolchains' artifacts without
# exceeding the budget, so the size sweep would not fire and the dead half
# would be carried indefinitely. Cost measured at 0.33s.
sweep_uninstalled_toolchains() {
    local repo_root="$1"
    local label="$2"

    command -v cargo-sweep >/dev/null 2>&1 || return 0
    ( cd "$repo_root" && cargo sweep --installed 2>&1 ) | while read -r line; do
        # Only a non-empty reclaim is worth a line. The tool also echoes the
        # full installed-toolchain list, and — the case a dry run does NOT
        # show — reports `Cleaned nothing` rather than staying silent when
        # there was nothing to reclaim. That is this step's normal outcome in
        # this repo, so passing it through would print a meaningless line on
        # every push and dilute the size report, which is the line that
        # matters.
        case "$line" in
            *"Cleaned nothing"*) ;;
            *"Cleaned"*) echo "$label: reclaimed dead-toolchain artifacts — $line" >&2 ;;
        esac
    done
    return 0
}

# Report the workspace build-cache size and, when it exceeds the budget,
# reclaim oldest artifacts until it fits. Never fails the caller.
enforce_target_budget() {
    local repo_root="${1:?enforce_target_budget needs the repo root}"
    local target="$repo_root/target"
    local label="${2:-hook}"

    [[ -d "$target" ]] || return 0

    # Before measuring: dead-toolchain artifacts are not part of what the
    # workspace legitimately needs, so counting them toward the budget would
    # report a size the project is not actually using.
    sweep_uninstalled_toolchains "$repo_root" "$label"

    local budget_gb="${PINION_TARGET_BUDGET_GB:-$TARGET_BUDGET_GB_DEFAULT}"
    if [[ ! "$budget_gb" =~ ^[1-9][0-9]*$ ]]; then
        # Rejected rather than silently replaced by the default: a typo'd
        # budget that quietly reverts is how a bound stops being a bound.
        echo "$label: PINION_TARGET_BUDGET_GB must be a positive integer of GiB (got '$budget_gb')" >&2
        return 0
    fi

    local bytes
    if ! bytes="$(du -sb "$target" 2>/dev/null | cut -f1)" || [[ -z "$bytes" ]]; then
        echo "$label: could not measure $target — build cache left unbounded" >&2
        return 0
    fi

    local gib=$(( bytes / 1073741824 ))
    echo "$label: target/ is ${gib} GiB (budget ${budget_gb} GiB)" >&2
    (( gib <= budget_gb )) && return 0

    if ! command -v cargo-sweep >/dev/null 2>&1; then
        echo "$label: target/ is OVER budget and cargo-sweep is not installed." >&2
        echo "$label:   cargo install cargo-sweep" >&2
        echo "$label: then re-push, or reclaim now with:" >&2
        echo "$label:   cargo sweep --maxsize ${budget_gb}GB" >&2
        return 0
    fi

    echo "$label: over budget — reclaiming oldest artifacts ..." >&2
    if ! ( cd "$repo_root" && cargo sweep --maxsize "${budget_gb}GB" ); then
        echo "$label: cargo sweep failed — target/ left as it was" >&2
        return 0
    fi

    # The after-number, so the line above is a claim with evidence rather
    # than an announcement of intent.
    if bytes="$(du -sb "$target" 2>/dev/null | cut -f1)" && [[ -n "$bytes" ]]; then
        echo "$label: target/ now $(( bytes / 1073741824 )) GiB" >&2
    fi
    return 0
}
