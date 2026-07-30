#!/usr/bin/env bash
# .githooks/lib/mnemosyne-tool.sh — resolve the pinned Mnemosyne CLI.
#
# R1507. THE CALLER RESOLVES THE PIN, and it resolves it to a build whose
# revision it has CHECKED.
#
# Before this, both hooks ran whatever `mnemosyne-cli` sat on PATH and trusted
# that binary to re-exec the pinned build itself (Mnemosyne R832 delegation).
# That asks the untrusted thing to enforce the trust, and it has a floor:
#
#   (a) a PATH binary older than R832 does not know the `[tool]` key at all.
#       It does not delegate — it dies parsing `mnemosyne.toml`, BEFORE any
#       hand-off, and `MNEMOSYNE_PIN_SKIP` cannot help because that check runs
#       in front of the parser too. Measured twice in one week (R1502, R1503):
#       a concurrent checkout reinstalled PATH at R807 and every commit and
#       push in this repo was blocked. Mnemosyne R861/R863 closed the case
#       where an unknown key stops the hand-off, but a pre-R832 build has no
#       hand-off to stop.
#   (b) a fresh clone has no `$MN_ROOT/<pin>` at all, so there is nothing for
#       any binary to delegate TO.
#
# Vendoring closes both, which is what `vendor/mnemosyne` is for, and is the
# conclusion R1503 reached and Mnemosyne upstream reached independently.
# Resolution order:
#
#   1. `$MN_ROOT/<pin>/bin/mnemosyne-cli` — the installed pin. An
#      optimisation only: it saves the vendored build. Skipped if its
#      `--version` revision does not match the pin.
#   2. `vendor/mnemosyne` at the pinned revision, built once into its own
#      `target/`. This is the guarantee: it needs no machine state.
#   3. Nothing. A LOUD, actionable failure — never a silent fall back to an
#      unknown-vintage PATH binary, because a gate whose tool has unknown
#      provenance is not a gate. That is the whole defect this closes.
#
# The revision is CHECKED, not inferred from the path (a directory name is a
# label anyone can write). `mnemosyne-cli --version` prints
# `mnemosyne-cli <ver> (<rev>)`, and the pin must be a prefix of that rev.
#
# `MNEMOSYNE_PIN_SKIP` is set for the revision PROBE and nowhere else.
#
# The probe needs it: `--version` must report the revision of the binary being
# interrogated, and a binary allowed to hand off would report its delegate's
# instead — which would defeat the check entirely.
#
# The run path must NOT set it, measured 2026-07-31. Every resolved build
# already recognises itself as the pin and does not delegate (both branches
# verified, including the vendored build with `MN_ROOT` pointing at an empty
# directory — rc=0, no delegation, no warning). Setting it there bought nothing
# and made the tool print, on every commit, that the pin "is NOT enforced" and
# "results from this run are not attributable to that revision" — the exact
# opposite of what the resolver had just established. A gate that contradicts
# itself in its own log teaches the reader to ignore the log. Leaving it unset
# also keeps Mnemosyne's own enforcement in place behind this one, so the pin is
# checked twice rather than once.
#
# The vendored build uses THIS workspace's toolchain, not the submodule's.
# Measured 2026-07-31: rustup resolves `rust-toolchain.toml` from the working
# directory, and the hooks run from the repo root, so `cargo build
# --manifest-path vendor/mnemosyne/...` builds under pinion's pinned 1.88.0
# even though the submodule pins 1.94.1 — and Mnemosyne compiles clean there
# (its own `rust-version` is 1.88). No second toolchain is required. If a
# future Mnemosyne raises its real MSRV past pinion's pin, this is where that
# shows up: the vendored build fails and says so.
#
# Limits, stated rather than discovered:
#   * the first vendored build takes ~1-2 minutes (286 dependencies, measured
#     1m16s release). It is announced before it starts, and cached after.
#   * `vendor/mnemosyne/target/` is the submodule's own directory, so
#     `target-budget.sh` does not SWEEP it — another repository's tree is not
#     this budget's to reclaim. Since R1508 it is reported on every push
#     (measured 326 MiB), because an unreported cache is one nobody notices
#     growing, which is the whole reason that file prints a number.
#   * an installed `MN_ROOT` build is trusted to be what its revision says.
#     R1508 narrowed that considerably — a `-dirty` or `unknown` stamp is
#     rejected, and the build must report the same revision whether or not it is
#     allowed to delegate — but the stamp still comes from git metadata, so an
#     `MN_ROOT` build made from an unstaged edit of the pinned revision would
#     pass. For the VENDORED build we close that ourselves by requiring a clean
#     worktree; for an installed one we cannot, because the source it was built
#     from is no longer there to inspect. Verifying more would mean rebuilding
#     it, which is the cost that branch exists to avoid.

# Resolve the pinned CLI. Sets `MN_CLI` (path) and `MN_CLI_SOURCE` (a short
# human label for the log line). Returns non-zero, with a message on stderr,
# when no pinned build can be produced.
#
# `MN_CLI_SOURCE` is an out-param read by the sourcing hook's log line. That
# read is invisible to the linter across a `source`, so the narrow directive
# below is preferred to a blanket file-level disable.
# shellcheck disable=SC2034
mnemosyne_resolve() {
    local repo_root pin mn_root candidate
    repo_root="$(git rev-parse --show-toplevel)"

    pin="$(mnemosyne_declared_pin "$repo_root/mnemosyne.toml")" || {
        echo "mnemosyne-tool: mnemosyne.toml declares no [tool].pin — this" >&2
        echo "  workspace's gate results would not be attributable to a" >&2
        echo "  revision. Add one (see the [tool] comment in mnemosyne.toml)." >&2
        return 1
    }

    # R1508 — the dual pin, checked AT REST rather than only when a build is
    # needed. R1507 verified the submodule's revision inside the vendored
    # branch, so a workspace whose installed pin resolved never looked at it:
    # `mnemosyne.toml` and the gitlink could disagree indefinitely and the
    # discrepancy would surface only on the day that install disappeared. This
    # is the same dual discipline `vendor/sce` has, made mechanical.
    mnemosyne_check_vendored_pin "$repo_root" "$pin" || return 1

    mn_root="${MN_ROOT:-$HOME/.local/mn}"
    candidate="$mn_root/$pin/bin/mnemosyne-cli"
    if mnemosyne_revision_matches "$candidate" "$pin"; then
        MN_CLI="$candidate"
        MN_CLI_SOURCE="installed pin $pin"
        return 0
    fi

    if mnemosyne_build_vendored "$repo_root" "$pin"; then
        MN_CLI="$repo_root/vendor/mnemosyne/target/release/mnemosyne-cli"
        MN_CLI_SOURCE="vendor/mnemosyne @ $pin"
        return 0
    fi

    echo "mnemosyne-tool: cannot produce the pinned build \`$pin\`." >&2
    echo "  Neither of the two sources worked:" >&2
    echo "    1. $candidate" >&2
    echo "       install it with:" >&2
    echo "         cargo install --git https://github.com/newmassrael/mnemosyne \\" >&2
    echo "             --rev $pin --locked mnemosyne-cli --root $mn_root/$pin" >&2
    echo "    2. $repo_root/vendor/mnemosyne" >&2
    echo "       check it out with:" >&2
    echo "         git submodule update --init vendor/mnemosyne" >&2
    echo "  Refusing to fall back to whatever is on PATH: a gate whose tool" >&2
    echo "  has unknown provenance is not a gate (R1502 / R1503)." >&2
    return 1
}

# Echo the `[tool].pin` value declared by a mnemosyne.toml, or fail.
#
# Deliberately a narrow reader rather than a TOML parser: it wants one scalar
# from one known table, and a shell hook that grew a TOML parser would be a
# second place this repo can be wrong about its own config.
mnemosyne_declared_pin() {
    local toml="$1"
    [ -r "$toml" ] || return 1
    awk '
        /^[[:space:]]*\[/ { in_tool = ($0 ~ /^[[:space:]]*\[tool\][[:space:]]*$/) ; next }
        in_tool && /^[[:space:]]*pin[[:space:]]*=/ {
            line = $0
            sub(/^[^=]*=[[:space:]]*/, "", line)
            gsub(/["\x27]/, "", line)
            sub(/[[:space:]]*(#.*)?$/, "", line)
            if (length(line) > 0) { print line; found = 1; exit }
        }
        END { if (!found) exit 1 }
    ' "$toml"
}

# Does `$1` exist, run, and report exactly the pinned revision `$2`?
#
# The check is what makes this a guard. `~/.local/mn/<rev>/bin` is a path
# someone typed; the binary inside it is the fact. R1478's lesson — a check
# that cannot fail is not a check — applied to the tool that runs the checks.
#
# R1508 — the revision must be HEX and nothing else after the pin. R1507 wrote
# this as a bare prefix match, which accepted `be4c1647-dirty`: Mnemosyne's
# build stamp appends that suffix when the tracked tree differs from HEAD, so a
# build made from MODIFIED sources passed as the pin. Measured: `be4c1647`,
# `be4c1647-dirty` and `be4c164-dirty` were all accepted. A dirty build is not
# the revision it names, and `unknown` (git could not say) is not a revision at
# all.
#
# Also verified WITHOUT the suppression, and the two answers must agree.
# `MNEMOSYNE_PIN_SKIP` is needed for the probe to describe the binary rather
# than a delegate — but that means the probe alone cannot notice a build that
# hands off to a DIFFERENT one when left to itself, which is the assumption
# R1507 leaned on with nothing checking it. Asking twice costs one exec and
# turns that assumption into a measurement.
mnemosyne_revision_matches() {
    local bin="$1" pin="$2" bare delegated
    [ -x "$bin" ] || return 1

    bare="$(mnemosyne_reported_revision "$bin" 1)" || return 1
    mnemosyne_is_pinned_revision "$bare" "$pin" || return 1

    # Unsuppressed: whatever this binary does when nobody stops it must still
    # be the pinned revision.
    delegated="$(mnemosyne_reported_revision "$bin" 0)" || return 1
    [ "$delegated" = "$bare" ] || {
        echo "mnemosyne-tool: $bin reports \`$bare\` for itself but" >&2
        echo "  \`$delegated\` when allowed to delegate — the build that would" >&2
        echo "  actually answer this workspace's gates is not the one checked." >&2
        return 1
    }
    return 0
}

# The revision `$1` reports via `--version`. `$2` = 1 suppresses delegation.
mnemosyne_reported_revision() {
    local bin="$1" suppress="$2" out rev
    if [ "$suppress" = "1" ]; then
        out="$(MNEMOSYNE_PIN_SKIP=1 "$bin" --version 2>/dev/null)" || return 1
    else
        out="$("$bin" --version 2>/dev/null)" || return 1
    fi
    # `mnemosyne-cli 0.1.0 (be4c1647)` -> `be4c1647`
    rev="${out##*\(}"
    rev="${rev%%\)*}"
    [ -n "$rev" ] || return 1
    printf '%s' "$rev"
}

# Is `$1` a pure-hex revision that starts with the pin `$2`?
mnemosyne_is_pinned_revision() {
    local rev="$1" pin="$2"
    case "$rev" in
        *[!0-9a-f]*) return 1 ;;  # `-dirty`, `unknown`, anything non-hex
    esac
    case "$rev" in
        "$pin"*) return 0 ;;
        *) return 1 ;;
    esac
}

# R1508 — is `vendor/mnemosyne` present and AT the declared pin?
#
# Three separate facts, and each can be wrong on its own:
#
#   * the GITLINK this repo records must be the pin. A bump that moves
#     `mnemosyne.toml` without moving the submodule leaves the two disagreeing,
#     which is the drift R1507 left unchecked.
#   * the submodule must be CHECKED OUT. When it has never been (no
#     `Cargo.toml`), this initialises it — there is nothing to lose, and it
#     turns a first clone's hard block into a wait. It does NOT run when the
#     directory exists, because `submodule update` force-checks-out the gitlink
#     and would discard a developer's local work there.
#   * HEAD must be the pin. Refused rather than corrected, for the same reason.
#
# Returns 0 when this repo declares no such submodule, so the resolver still
# works in a tree that has not adopted one.
mnemosyne_check_vendored_pin() {
    local repo_root="$1" pin="$2" sub="$1/vendor/mnemosyne" gitlink head
    gitlink="$(git -C "$repo_root" ls-files -s -- vendor/mnemosyne 2>/dev/null \
        | awk '$1 == "160000" { print $2 }')"
    [ -n "$gitlink" ] || return 0

    case "$gitlink" in
        "$pin"*) ;;
        *)
            echo "mnemosyne-tool: mnemosyne.toml pins \`$pin\` but this repo" >&2
            echo "  records vendor/mnemosyne at ${gitlink:0:8}. The two pins are" >&2
            echo "  one decision and must move together:" >&2
            echo "    git -C vendor/mnemosyne fetch --all" >&2
            echo "    git -C vendor/mnemosyne checkout $pin" >&2
            echo "    git add vendor/mnemosyne" >&2
            return 1
            ;;
    esac

    if [ ! -f "$sub/Cargo.toml" ]; then
        echo "mnemosyne-tool: vendor/mnemosyne is not checked out; initialising" >&2
        echo "  it at $pin (once per clone)" >&2
        ( cd "$repo_root" && git submodule update --init vendor/mnemosyne >&2 ) || {
            echo "mnemosyne-tool: could not initialise vendor/mnemosyne. Run" >&2
            echo "    git submodule update --init vendor/mnemosyne" >&2
            return 1
        }
    fi

    head="$(git -C "$sub" rev-parse HEAD 2>/dev/null)" || {
        echo "mnemosyne-tool: vendor/mnemosyne is not a git checkout" >&2
        return 1
    }
    case "$head" in
        "$pin"*) return 0 ;;
        *)
            echo "mnemosyne-tool: vendor/mnemosyne is checked out at" >&2
            echo "  ${head:0:8}, not the declared pin $pin. Refusing to build a" >&2
            echo "  different revision and call it the pin. To restore it:" >&2
            echo "    git submodule update --checkout vendor/mnemosyne" >&2
            return 1
            ;;
    esac
}

# Build (or reuse) the vendored CLI at the pinned revision.
#
# `mnemosyne_check_vendored_pin` has already established that the submodule is
# present and at the pin, so this is only about producing a binary from it.
#
# R1508 — and about the WORKTREE. Mnemosyne's build stamp derives `-dirty` from
# git metadata, and its own docs say an edit that is never staged moves neither
# HEAD nor the index, so a binary built after one can fail to say `-dirty`:
# "if a locally built binary ever has to be trusted AS a pin, this needs an
# input that watches the worktree rather than git's metadata". R1507 made a
# locally built binary exactly that, walking into the case upstream had left out
# of scope. This is that input, and we are the ones who can supply it.
mnemosyne_build_vendored() {
    local repo_root="$1" pin="$2" sub="$1/vendor/mnemosyne" bin dirt
    [ -f "$sub/Cargo.toml" ] || return 1

    dirt="$(git -C "$sub" status --porcelain --untracked-files=no 2>/dev/null)" || return 1
    if [ -n "$dirt" ]; then
        echo "mnemosyne-tool: vendor/mnemosyne has uncommitted changes, so a" >&2
        echo "  build from it would not be revision \`$pin\` — and the build" >&2
        echo "  stamp cannot always tell (an unstaged edit moves neither HEAD" >&2
        echo "  nor the index). Restore it with:" >&2
        echo "    git -C vendor/mnemosyne checkout -- ." >&2
        return 1
    fi

    bin="$sub/target/release/mnemosyne-cli"
    if mnemosyne_revision_matches "$bin" "$pin"; then
        return 0
    fi

    echo "mnemosyne-tool: building the pinned gate tool from" >&2
    echo "  vendor/mnemosyne @ $pin (first run here; ~1-2 min, then cached)" >&2
    # From the repo root, so rustup uses THIS workspace's toolchain.
    ( cd "$repo_root" && cargo build --release --locked \
        --manifest-path vendor/mnemosyne/Cargo.toml -p mnemosyne-cli >&2 ) || return 1

    mnemosyne_revision_matches "$bin" "$pin"
}

# Run the resolved CLI. `mnemosyne_resolve` must have succeeded first.
#
# Deliberately does not set `MNEMOSYNE_PIN_SKIP` — see the file header.
mnemosyne_cli() {
    "$MN_CLI" "$@"
}
