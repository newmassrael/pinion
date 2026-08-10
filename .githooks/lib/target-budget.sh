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
#      Cost is one `du -sbL`, measured at 0.12s on a 69 GiB tree.
#
#   R1489: this is no longer the HARD bound. Every project's target/ now lives
#   in a fixed-size compressed btrfs image, so build caches cannot fill the
#   root filesystem at all — overflow is impossible rather than detected late,
#   which is what the 2026-07-29 incident needed (the disk hit 100% and another
#   session ran into it before any push could report the size). What survives
#   here is the per-repo trend line and an eager reclaim; the ceiling is the
#   volume, and a daily user timer sweeps every project rather than only this
#   one. Note the budget below is measured on APPARENT bytes, while the volume
#   stores them compressed ~3.9x (measured), so 100 GiB here is ~26 GiB of disk.
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

# R1641 — THE BUDGET ABOVE IS PER PROJECT AND THE VOLUME IS SHARED, so keeping
# it is not the same as having room. Measured 2026-08-10, mid-round, when a
# build died:
#
#   error: failed to write .../.fingerprint/pinion-rpc-.../invoked.timestamp
#   /dev/loop0  160G  158G  0  100%  /home/coin/.buildcache
#
# pinion was at exactly 100 GiB apparent — ON budget, so this gate had nothing
# to say — while six sibling projects held another ~160 GiB apparent between
# them and the volume was full. R1489 established that a fixed-size image makes
# filling the ROOT filesystem impossible; what it did not make impossible is
# the build failing, and this is the part that was missing.
#
# Two things follow, and this file supplies both:
#
#   1. THE VOLUME'S NUMBER, printed every push beside the project's own, for
#      the reason (1) above gives for the project's: the failure mode is a
#      resource nobody is watching. One `df`, no tree walk.
#   2. PRESSURE TIGHTENS THE BUDGET. Below the floor, the effective budget
#      drops to the tight one, so the sweep fires while the project is still
#      nominally within its steady-state allowance. That is the case that
#      actually happened: nothing was wrong per project, and the build stopped.
#
# The tight budget is measured, not picked: `cargo sweep --maxsize 45GB`
# reclaimed 73.94 GiB here and took the volume from 0 to 47 GiB free, with the
# workspace rebuilding incrementally afterwards.
#
# The floor is deliberately larger than one workspace rebuild: a full clean
# workspace test build materialises ~40 GiB apparent (R1489's measurement),
# which is ~10 GiB of volume at the compression measured today.
#
# NOT swept from here: the sibling projects. They are other repositories, and
# a push hook in this one reaching into them is the cross-repo line this
# project holds elsewhere. The daily user timer sweeps every project; what
# this can honestly do is say that the pressure is not ours to fix alone.
TARGET_BUDGET_TIGHT_GB_DEFAULT=45
VOLUME_FREE_FLOOR_GB_DEFAULT=15

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

    # `-L` follows the link: R1489 moved every project's target/ onto a
    # compressed btrfs volume and left a symlink behind, and without `-L` this
    # measured the LINK — 29 bytes, so the gate reported "0 GiB" and would
    # never have fired again. A bound that cannot fire is worse than none,
    # because the number it prints looks like reassurance.
    local bytes
    if ! bytes="$(du -sbL "$target" 2>/dev/null | cut -f1)" || [[ -z "$bytes" ]]; then
        echo "$label: could not measure $target — build cache left unbounded" >&2
        return 0
    fi

    local gib=$(( bytes / 1073741824 ))

    # R1641 — the volume this project's target/ actually lives on, and whether
    # the per-project budget is still the binding constraint. Read BEFORE the
    # budget is applied, because under pressure it changes which budget applies.
    local free_gib
    free_gib="$(volume_free_gib "$target")"
    if [[ -n "$free_gib" ]]; then
        local floor_gb="${PINION_VOLUME_FREE_FLOOR_GB:-$VOLUME_FREE_FLOOR_GB_DEFAULT}"
        echo "$label: build-cache volume has ${free_gib} GiB free (floor ${floor_gb} GiB, shared with every project)" >&2
        if (( free_gib < floor_gb )); then
            local tight_gb="${PINION_TARGET_BUDGET_TIGHT_GB:-$TARGET_BUDGET_TIGHT_GB_DEFAULT}"
            if (( tight_gb < budget_gb )); then
                echo "$label: volume under the floor — tightening this project's budget ${budget_gb} -> ${tight_gb} GiB" >&2
                budget_gb="$tight_gb"
            fi
        fi
    else
        # Announced, not swallowed: the whole point of this block is that an
        # unwatched shared resource is what failed, and silence here would
        # restore exactly that condition.
        echo "$label: could not read the build-cache volume's free space — only the per-project bound applies" >&2
    fi

    echo "$label: target/ is ${gib} GiB (budget ${budget_gb} GiB)" >&2

    # R1508 — the vendored gate tool builds into its OWN target/, inside a
    # submodule, which this budget does not measure and `cargo sweep` over this
    # workspace does not reach. R1507 created it and recorded the gap; this
    # reports it, because the whole reason this file prints a number on every
    # push is that an unreported cache is one nobody notices growing. Reported
    # rather than swept: it is another repository's tree, and one release build
    # of one binary is not the accretion this budget exists for.
    report_vendored_cache "$repo_root" "$label"
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
    if bytes="$(du -sbL "$target" 2>/dev/null | cut -f1)" && [[ -n "$bytes" ]]; then
        echo "$label: target/ now $(( bytes / 1073741824 )) GiB" >&2
    fi

    # R1641 — and whether it was ENOUGH. A sweep that empties this project and
    # leaves the volume full is the case worth naming out loud, because the
    # remedy is then somewhere this hook must not go.
    free_gib="$(volume_free_gib "$target")"
    if [[ -n "$free_gib" ]]; then
        local floor_gb="${PINION_VOLUME_FREE_FLOOR_GB:-$VOLUME_FREE_FLOOR_GB_DEFAULT}"
        echo "$label: volume now ${free_gib} GiB free" >&2
        if (( free_gib < floor_gb )); then
            echo "$label: STILL under the floor — the pressure is not this project's alone." >&2
            echo "$label:   du -shL /home/coin/.buildcache/* 2>/dev/null | sort -h" >&2
            echo "$label:   (the daily buildcache-sweep timer covers the others; this hook does not)" >&2
        fi
    fi
    return 0
}

# R1641 — GiB free on the filesystem holding `path`, or empty when it cannot be
# read.
#
# `-P` for POSIX single-line output: the default wraps a long device name onto
# its own line, which would make the field offsets below read the wrong column.
# `--output=avail` would be tidier and is coreutils-only, so this stays on the
# portable form the rest of these hooks use.
volume_free_gib() {
    local path="${1:?volume_free_gib needs a path}"
    local avail_kib
    avail_kib="$(df -Pk "$path" 2>/dev/null | awk 'NR==2 {print $4}')" || return 0
    [[ "$avail_kib" =~ ^[0-9]+$ ]] || return 0
    echo $(( avail_kib / 1048576 ))
}

# R1508 — report the vendored gate tool's build cache, and (R1509) bound it.
#
# Deliberately separate from `enforce_target_budget`: that one bounds what THIS
# workspace accretes across hundreds of rounds and reclaims OLDEST artifacts,
# because most of the tree is still wanted. This one holds a single binary of a
# single pinned revision, so there is nothing to keep selectively — when it is
# over budget the whole cache goes, and the resolver rebuilds it in about a
# minute. R1508 only reported it, and recorded as a limit that nothing would
# ever shrink it; a cache that is measured and never reclaimed is a number
# watched going one way.
#
# The budget is generous (2 GiB against a measured 326 MiB) because the cost of
# firing is a minute of somebody's next commit. It exists for accretion across
# pin bumps, not for the steady state.
VENDORED_CACHE_BUDGET_GB_DEFAULT=2

report_vendored_cache() {
    local repo_root="${1:?report_vendored_cache needs the repo root}"
    local label="${2:-hook}"
    local vendored="$repo_root/vendor/mnemosyne/target"

    [[ -d "$vendored" ]] || return 0

    local bytes
    bytes="$(du -sbL "$vendored" 2>/dev/null | cut -f1)" || return 0
    [[ -n "$bytes" ]] || return 0

    local budget_gb="${PINION_VENDORED_CACHE_BUDGET_GB:-$VENDORED_CACHE_BUDGET_GB_DEFAULT}"
    if [[ ! "$budget_gb" =~ ^[1-9][0-9]*$ ]]; then
        # Rejected rather than silently defaulted, for the reason the sibling
        # budget states: a typo'd bound that quietly reverts is how a bound
        # stops being a bound.
        echo "$label: PINION_VENDORED_CACHE_BUDGET_GB must be a positive integer of GiB (got '$budget_gb')" >&2
        return 0
    fi

    local gib=$(( bytes / 1073741824 ))
    local mib=$(( bytes / 1048576 ))
    if (( gib > 0 )); then
        echo "$label: vendor/mnemosyne/target/ is ${gib} GiB (budget ${budget_gb} GiB)" >&2
    else
        echo "$label: vendor/mnemosyne/target/ is ${mib} MiB (budget ${budget_gb} GiB)" >&2
    fi
    (( gib <= budget_gb )) && return 0

    echo "$label: reclaiming it — one pinned revision has nothing worth keeping" >&2
    echo "$label: the next hook run rebuilds the gate tool (~1 min)" >&2
    if ! ( cd "$repo_root" && cargo clean --manifest-path vendor/mnemosyne/Cargo.toml >&2 ); then
        echo "$label: cargo clean failed; vendored cache left at ${gib} GiB" >&2
    fi
}
