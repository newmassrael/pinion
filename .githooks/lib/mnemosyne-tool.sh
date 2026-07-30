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
#   * `vendor/mnemosyne/target/` is the submodule's own directory, and is not
#     swept by `target-budget.sh` (which measures this workspace's `target/`).
#   * a `MN_ROOT` pin that matches the revision is trusted to BE that
#     revision's build. Verifying more would mean rebuilding it.

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

# Does `$1` exist, run, and report a revision the pin `$2` is a prefix of?
#
# The check is what makes this a guard. `~/.local/mn/<rev>/bin` is a path
# someone typed; the binary inside it is the fact. R1478's lesson — a check
# that cannot fail is not a check — applied to the tool that runs the checks.
mnemosyne_revision_matches() {
    local bin="$1" pin="$2" out rev
    [ -x "$bin" ] || return 1
    out="$(MNEMOSYNE_PIN_SKIP=1 "$bin" --version 2>/dev/null)" || return 1
    # `mnemosyne-cli 0.1.0 (be4c1647)` -> `be4c1647`
    rev="${out##*\(}"
    rev="${rev%%\)*}"
    [ -n "$rev" ] || return 1
    case "$rev" in
        "$pin"*) return 0 ;;
        *) return 1 ;;
    esac
}

# Build (or reuse) the vendored CLI at the pinned revision.
#
# Refuses when the submodule is absent or checked out somewhere other than the
# pin: building a different revision and calling it the pin would reintroduce
# exactly the ambiguity this file exists to remove.
mnemosyne_build_vendored() {
    local repo_root="$1" pin="$2" sub="$1/vendor/mnemosyne" head bin
    [ -f "$sub/Cargo.toml" ] || return 1

    head="$(git -C "$sub" rev-parse HEAD 2>/dev/null)" || return 1
    case "$head" in
        "$pin"*) ;;
        *)
            echo "mnemosyne-tool: vendor/mnemosyne is at ${head:0:8}, not the" >&2
            echo "  declared pin $pin. Run:" >&2
            echo "    git submodule update --init vendor/mnemosyne" >&2
            return 1
            ;;
    esac

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
