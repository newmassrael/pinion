#!/usr/bin/env python3
"""R1566 §5.12 §5.15 §2 #7 — a refusal never says a DECLARED path is unknown.

Three verbs address one surface: `query` reads a slot, `intervene` writes one,
`invoke` calls an action. Each answered the same word when it did not recognise
a name — "no such path" — and each was frequently saying something the surface
itself contradicts, because a name it does not recognise on ONE channel may be
published on another.

The trait says so out loud. `InterveneError::UnknownPath`'s own doc reads "path
is not declared in the schema", and no `intervene` impl can honour that: it
knows only that its own `match` fell through. R1353 wrote the fix down as
`read_only_or_unknown`, whose doc says "**every** read-only External needs
exactly this rule" — and left it opt-in, so **2** surfaces route through it
against **97** that implement `intervene`.

R1566 makes it structural instead. The wire refusal is derived from the
surface's own `$schema`, at one place the compiler will not let a dispatch site
skip: the infallible `From<TraitInterveneError>` conversion is gone, replaced by
one that cannot be called without the declaration.

    query     on a declared ACTION  ->  PathIsAnAction   (was UnknownIntrospectPath)
    intervene on a declared SLOT    ->  ReadOnly         (was UnknownIntervenePath)
    intervene on a declared ACTION  ->  PathIsAnAction   (was UnknownIntervenePath)
    invoke    on a declared SLOT    ->  PathIsAReadSlot  (was UnknownInvokePath)

Past the toolkit 6.11 — the toolkit fuses every one of these into a value that carries no reason:
`setProperty()` answers a bare `bool`, `invokeMethod()`
answers a bare `bool`, and `property()` answers an **invalid
dynamic value** — the same value it answers for a name that is in no meta-object at
all. A toolkit caller who addressed a method as a property learns only that it did
not work, and has to go back to the meta-object and search it themselves. Here
the surface answers with what the name IS.

# The measurement this round rests on

Deriving the refusal made `SchemaChannel` load-bearing, and it had never been
read by anything. R1504 added it in 2026; `Read` was the silent default; nothing
branched on it. Probed at R1566 over nine bindings: **116 of 288** declared
scalar fields — 40% — were actions declared as readable slots. `hello-data-grid`
published `add_row`, `paste` and `reset_all` as string fields; `hello-topology`
published `untangle`, which the R1443 demo's own docstring calls "a **verb**".
Every one of them answered an agent's `query` with "no such path" about a name
that agent had just read out of `$schema`.

Nobody was lying: a declaration with no consumer is a declaration nothing keeps
true. This demo ships the consumer AND the gate —
`assert_declared_channels_are_true` walks a whole surface and asserts every
scalar answers on the channel it declares, which is what stops the 116 from
coming back one commit at a time.

ZERO-FLAKE: no sleeps and no timing assertions. Every check is a
request/response pair against deterministic state, and the whole-surface gate is
side-effect free by construction — it uses `query` only, because probing the
write directions would fire the very actions it is inspecting.

Run from the workspace root:
    cargo build -p hello-topology -p hello-data-grid --release
    python3 tools/demos/r1566_refusal_names_the_channel.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_declared_channels_are_true,
    assert_eq,
    assert_rpc_error,
    call,
    rpc_error_data,
    run_demo,
)

EXT = "/external"


def q(tf: RpcSubprocess, path: str) -> Any:
    return tf.query(f"{EXT}/{path}")


def schema_channels(tf: RpcSubprocess) -> dict[str, str]:
    """`{path: channel}` for every scalar the surface declares."""
    fields = tf.query(f"{EXT}/$schema")
    if isinstance(fields, dict):
        fields = fields.get("fields", fields)
    return {
        f["path"]: f.get("channel", "read")
        for f in fields
        if f.get("path") and "<" not in f["path"]
    }


def body() -> None:
    with RpcSubprocess("hello-topology", boot_grace=1.5) as tf:
        # ── (A) the surface declares both channels, and both are populated ──
        # Without this the rest is vacuous: a gate over a surface with no
        # actions cannot tell a channel-aware refusal from the old one.
        channels = schema_channels(tf)
        reads = [p for p, c in channels.items() if c == "read"]
        actions = [p for p, c in channels.items() if c != "read"]
        assert len(reads) >= 5 and len(actions) >= 5, (
            f"A: the fixture must declare BOTH kinds: {len(reads)} read / "
            f"{len(actions)} invoke"
        )
        assert "mode" in reads, f"A: `mode` is the read slot this demo drives: {reads}"
        assert "untangle" in actions, (
            "A: `untangle` is the action this demo drives — R1566 corrected its "
            f"declaration, which had said `read` since R1443: {actions}"
        )

        # ── (B) ★ query on a declared ACTION names the channel ──────────────
        # Before R1566 this said `UnknownIntrospectPath` — "no such path" about
        # a path the very same surface had just published.
        assert_rpc_error(lambda: q(tf, "untangle"), data="PathIsAnAction")
        assert_rpc_error(lambda: q(tf, "reset"), data="PathIsAnAction")
        # And a name that is genuinely on NO channel keeps the old word, which
        # is what makes the new one worth anything.
        assert_rpc_error(lambda: q(tf, "no_such_name"), data="UnknownIntrospectPath")

        # ── (C) ★ intervene on a declared ACTION names the channel ──────────
        assert_rpc_error(
            lambda: tf.intervene(f"{EXT}/untangle", "yes"), data="PathIsAnAction"
        )
        assert_rpc_error(
            lambda: tf.intervene(f"{EXT}/no_such_name", 1), data="UnknownIntervenePath"
        )

        # ── (D) ★ intervene on a declared SLOT says READ-ONLY ───────────────
        # `crossings` is a measurement: `query` answers it, and the write
        # channel used to deny it existed. The two channels now agree about
        # what the surface has.
        assert isinstance(q(tf, "crossings"), int), "D: crossings reads as a number"
        assert_rpc_error(lambda: tf.intervene(f"{EXT}/crossings", 0), data="ReadOnly")
        for path in ("order_changes", "depth", "bends", "straight_inner"):
            assert isinstance(q(tf, path), int), f"D: {path} reads"
            assert_rpc_error(
                lambda p=path: tf.intervene(f"{EXT}/{p}", 0), data="ReadOnly"
            )

        # ── (E) ★ invoke on a declared SLOT names the channel ───────────────
        assert_rpc_error(lambda: tf.invoke(f"{EXT}/crossings", None), data="PathIsAReadSlot")
        assert_rpc_error(lambda: tf.invoke(f"{EXT}/mode", None), data="PathIsAReadSlot")
        assert_rpc_error(
            lambda: tf.invoke(f"{EXT}/no_such_name", None), data="UnknownInvokePath"
        )

        # ── (F) the surface's own verdicts are UNTOUCHED ────────────────────
        # The derivation only answers where the impl had nothing to say. A
        # surface that judged the write still owns its answer, including the
        # R1565 sentence — otherwise this round would have traded one wrong
        # word for a differently wrong one.
        assert_eq(q(tf, "mode"), "stable", "F: the slot reads before the writes")
        tf.intervene(f"{EXT}/mode", "fresh")
        assert_eq(q(tf, "mode"), "fresh", "F: and a legal write still lands")
        tf.intervene(f"{EXT}/mode", "stable")
        said = rpc_error_data(
            lambda: tf.intervene(f"{EXT}/mode", "sideways"),
            code=-32006,
            label="F: an out-of-range write",
        )
        assert "is not a layout mode" in said, f"F: still the producer's own: {said!r}"
        assert_eq(q(tf, "mode"), "stable", "F: and it changed nothing")

        # ── (G) the refusal composes with R1487 provenance ──────────────────
        disclosed = rpc_error_data(
            lambda: tf.intervene(f"{EXT}/untangle", "yes", with_origin=True),
            expect=dict,
            label="G: a channel refusal with origin",
        )
        assert_eq(disclosed["reason"], "PathIsAnAction", "G: the word lands under reason")
        assert "origin" in disclosed, f"G: beside the surface that refused: {disclosed}"

        # ── (H) ★ the WHOLE surface is honest, not the paths this demo picked ─
        counted = assert_declared_channels_are_true(tf)
        assert counted == {"read": 12, "invoke": 12}, (
            f"H: the fixture's declared split, exactly: {counted}"
        )

    # ── (I) ★ and the gate holds for the surface that had the MOST wrong ────
    # `hello-data-grid` declared 22 verbs as readable string fields — the
    # largest single population found. Asserting it here is what keeps this
    # round's correction from being one binding's local tidy-up.
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as grid:
        counted = assert_declared_channels_are_true(grid)
        assert counted["invoke"] >= 22, (
            f"I: every one of the 22 corrected declarations is still a verb: {counted}"
        )
        assert_rpc_error(lambda: grid.query(f"{EXT}/add_row"), data="PathIsAnAction")
        assert_rpc_error(
            lambda: grid.intervene(f"{EXT}/add_row", 1), data="PathIsAnAction"
        )

        # ── (I2) ★ the sharp edge: a FAMILY is not re-judged ────────────────
        # `SchemaChannel::Read` means "readable", NOT "read-only" — every
        # writable slot is declared that way too. For a scalar that costs
        # nothing, because an impl that writes a name does not answer
        # "unknown path" for it. For a parametric family it costs everything:
        # the impl recognised the SHAPE and rejected the ARGUMENT, so calling
        # it read-only would be a fresh false statement about a family a
        # client may write all day.
        #
        # `value.<row>.<col>` is exactly that family — writable at a real
        # address, and absent at this one.
        grid.intervene(f"{EXT}/value.0.0", "27")
        assert_eq(grid.query(f"{EXT}/value.0.0"), "27", "I2: the family IS writable")
        assert_rpc_error(
            lambda: grid.intervene(f"{EXT}/value.9999.0", "x"),
            data="UnknownIntervenePath",
        )
        # The scalar peer, same surface, same round: decidable, so decided.
        assert_rpc_error(lambda: grid.intervene(f"{EXT}/col_count", 3), data="ReadOnly")

        # ── (J) ★ and the three new words are DISCOVERABLE ──────────────────
        # R1564 published the codes on the argument that shipping a contract a
        # client can only learn by reading pinion's source is no contract at
        # all. This round adds words to a vocabulary R1565 called closed and
        # never published, so the same argument lands on the vocabulary.
        catalogue = call(grid, "rpc/errors")
        invalid_params = next(e for e in catalogue["errors"] if e["code"] == -32602)
        vocabulary = invalid_params["data_vocabulary"]
        for word in ("PathIsAnAction", "PathIsAReadSlot", "ReadOnly"):
            assert word in vocabulary, f"J: {word!r} is published: {vocabulary}"
        # Every word a client can meet is in it — asserted against a refusal
        # this demo actually provoked, not against the list alone.
        met = rpc_error_data(
            lambda: grid.query(f"{EXT}/add_row"), label="J: a channel refusal"
        )
        assert met in vocabulary, f"J: the payload received is a published word: {met!r}"
        # A prose-carrying code publishes NO vocabulary, which is what stops
        # a client trying to match a sentence.
        for entry in catalogue["errors"]:
            if entry["data_is_prose"]:
                assert entry["data_vocabulary"] == [], (
                    f"J: code {entry['code']} carries prose and offers words to "
                    f"match: {entry['data_vocabulary']}"
                )
        # The corrected claim is on the wire, not only in this crate's rustdoc:
        # -32602's payload is a published word OR an echo of what the caller
        # sent, and R1566 is the round that found the second half.
        assert "echo of what the caller itself supplied" in invalid_params["meaning"], (
            f"J: the entry states both shapes: {invalid_params['meaning']!r}"
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1566_refusal_names_the_channel", body))
