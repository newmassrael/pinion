#!/usr/bin/env bash
# R1582 — run the tests of the packages a change can BREAK, not the ones it edits.
#
# ## The defect
#
# The standing local rule is "test the crates this round touched" (2026-07-21;
# the workspace suite and the demo sweep are CI's). "Touched" means the crates
# whose *behaviour* changed — consumers included — and reading it as "the files
# I edited" is how R1497 broke `pinion-shell` while testing `pinion-runtime`,
# and how R1511 then committed `examples/hello-dialog` asserting a focus ring
# it had just retired. `clippy --all-targets` COMPILES a test and does not run
# it, so every gate was green and the assertion was false.
#
# R1499 wrote the lesson down and R1511 re-broke it. A lesson recorded twice
# and re-broken twice is a computation nobody had written, and
# `tools/blast_radius.py` is that computation: the packages that own the change
# plus every workspace package that depends on them, transitively, from
# `cargo metadata` rather than from a list somebody maintains.
#
# It found a live instance the moment it existed: R1578 edited `pinion-graph`
# and tested three of the five packages in its radius.
#
# ## Why a cap, and where it comes from
#
# Measured on this machine: `cargo metadata --no-deps` is 0.2s; a warm
# 8-package test run is 1.4s; the real case — a crate changed, so every
# consumer's test binary must relink — is 9.7s for a 5-package radius. Linking
# is the cost and it scales with the radius, so a core crate's ~230 consumers
# is a quarter of an hour and belongs to CI, while a leaf crate's handful is
# seconds and belongs here.
#
# The cap is therefore stated in packages and justified in time: at ~2s of link
# per consumer, 12 is under half a minute — smaller than the clippy run this
# hook already pays for. Above it the radius is REPORTED with its command, and
# the gate says so rather than going quiet.
#
# ## Why it does not simply always report
#
# Because that is what the SEED already did, in prose, and it was re-broken
# twice. [[r1470-paint-test-opened-the-speakers]]: a prose warning is not a
# gate. The small radius is the case where a missed consumer is most likely
# (a leaf crate feeding one or two bindings) and cheapest to cover, so that is
# the case that is covered rather than announced.

#: Radius at or below which the consumers are tested here rather than in CI.
#: See the header for the measurement that chose it.
CONSUMER_TEST_CAP=${CONSUMER_TEST_CAP:-12}

# Test everything a change can break, when that is affordable.
#
# `mode` is `staged` or `range`; `rev_range` is required for the latter.
# Returns 0 when publishing may proceed, 1 when a consumer's tests failed.
consumer_test_gate() {
    local label="$1" repo_root="$2" mode="$3" rev_range="${4:-}"

    if [[ "${PINION_SKIP_CONSUMER_TESTS:-}" == "1" ]]; then
        echo "$label: consumer tests skipped, PINION_SKIP_CONSUMER_TESTS=1" >&2
        return 0
    fi

    local args=(python3 "$repo_root/tools/blast_radius.py" --mode "$mode")
    [[ -n "$rev_range" ]] && args+=(--range "$rev_range")

    local names
    if ! names="$("${args[@]}" 2>/dev/null)"; then
        # An absent python or a cargo that cannot read the manifests is
        # infrastructure absence, not evidence of breakage — the posture
        # `lib/ci-status.sh` takes, and for the same reason.
        echo "$label: could not compute the blast radius — continuing" >&2
        return 0
    fi

    local -a packages=()
    local name
    while read -r name; do
        [[ -n "$name" ]] && packages+=("$name")
    done <<<"$names"

    local count=${#packages[@]}
    if (( count == 0 )); then
        echo "$label: no package's behaviour changed — no consumer tests" >&2
        return 0
    fi

    local -a flags=()
    for name in "${packages[@]}"; do
        flags+=(-p "$name")
    done

    if (( count > CONSUMER_TEST_CAP )); then
        echo "$label: $count package(s) can be affected — over the local cap" \
             "of $CONSUMER_TEST_CAP, so CI covers them" >&2
        echo "$label:   cargo test ${flags[*]}" >&2
        return 0
    fi

    echo "$label: $count package(s) can be affected — testing them here" >&2
    if ! (cd "$repo_root" && cargo test "${flags[@]}" >&2); then
        echo "$label: a package that DEPENDS on this change fails its tests" >&2
        echo "$label:   cargo test ${flags[*]}" >&2
        echo "$label: to bypass once: PINION_SKIP_CONSUMER_TESTS=1" >&2
        return 1
    fi
    return 0
}
