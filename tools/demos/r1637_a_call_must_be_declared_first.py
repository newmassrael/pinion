#!/usr/bin/env python3
"""R1637 — a call must be declared first: the wire refuses what it did not publish.

R1631 added an `arrange` verb to this example and did not add it to the schema.
For a whole round it was callable and undiscoverable, and nothing noticed:
`cargo test`, clippy, the demo and the push gate all passed, because none of
them had two lists to compare. R1632 found it by hand while adding a different
verb, and wrote the gap down as "an External's declared action list does not
reach the wire at all".

Half of that was wrong, and this demo is where it is corrected: the list has
been on the wire since R825 as the reserved `$schema` introspect path. What was
missing was not publication — it was the **order of the two questions**. The
transport dispatched first and consulted the declaration only to explain a
refusal, so a name that was implemented and never published worked perfectly.

R1637 asks the declaration first, on BOTH channels. A name absent from
`$schema` is now absent from the wire — `scene/invoke` will not call it and
`scene/query` will not read it.

The read half had drifted far less (two paths across the workspace, against
123 on the action channel) and was wrong in the same way: one of the two was
`pinion_widget_paint::dock`'s `lifecycle`, whose own doc calls it "surfaced as
scene-as-data via `query("lifecycle")` (§2 #7)" while the contract omitted it.

That is the reference's floor, not an invention. The toolkit's meta-object is
the only route to an object's action channel and it is generated from the
declarations, so an implemented-but-undeclared method is unreachable there by
construction — measured on the toolkit at 6.4.2 with a class carrying one
`slots:` method and one plain one: `invokeMethod` ran the declared name and
answered `true`, refused the undeclared one with `false` and
`indexOfMethod() == -1`, and its 6.11.1 source still emits the same
`"No such method"` warning. pinion reaches the guarantee by gate rather than by
codegen, because its surfaces are hand-written.

What each block discriminates:

* **(A) the contract is READABLE over the wire.** If `$schema` did not answer
  here, every later assertion would be vacuous — a comparison against an empty
  list passes.
* **(B) published ⊆ accepted**, for all 45 declared actions, one at a time. This
  is the direction R1632 said could be checked "only inside the crate". It can
  be checked from out here, and a name that answers `DeclaredButUnhandled` is a
  surface that published something it does not implement.
* **(C) accepted ⊆ published.** An undeclared name is refused — with the
  CALLER's word, not the surface's.
* **(D) the two words are different facts.** `UnknownInvokePath` (you made that
  name up) and `DeclaredButUnhandled` (the surface published it and did not
  answer) shared one word before R1637. The reference cannot express the second
  at all, because a hand-written declaration beside a hand-written dispatch is
  not a shape a meta-object has.
* **(E) the channels stay apart, and neither answers the undeclared.** A read
  slot refuses a call and an action refuses a read, each naming what the other
  is — the R1566 pair, now decided BEFORE the surface is reached rather than
  after it declines; and an invented name is refused on the read channel too.
* **(F) the new word is discoverable.** `rpc/errors` publishes the closed
  vocabulary `-32602` may carry, and a refusal a client cannot look up is one it
  cannot handle.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1637_a_call_must_be_declared_first.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_rpc_error,
    call,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: The reserved introspect path that answers the declaration instead of a value
#: (R825). Not a `scene/*` METHOD — which is why R1632 looked for one, did not
#: find it, and concluded the declaration was unpublished.
SCHEMA = f"{EXT}/$schema"

#: R1631's verb — the one this whole round exists because of.
THE_VERB = "arrange"

#: A name no surface in this workspace declares.
INVENTED = "kern"


def checks(tf: RpcSubprocess) -> None:
    # ── (A) the contract is readable over the wire ──────────────────────
    fields = tf.query(SCHEMA)
    assert isinstance(fields, list) and fields, f"$schema must answer a list: {fields!r}"
    actions = [f["path"] for f in fields if f.get("channel") == "invoke"]
    slots = [f["path"] for f in fields if f.get("channel") != "invoke"]
    assert len(actions) > 20, f"this surface's action channel is wide: {len(actions)}"
    assert len(slots) > 10, f"and it reads plenty too: {len(slots)}"
    assert not set(actions) & set(slots), "a path is on exactly one channel"
    assert THE_VERB in actions, (
        f"R1631's verb is published; it was not, for a whole round: {actions}"
    )
    print(f"[demo] $schema publishes {len(actions)} actions and {len(slots)} slots")

    # ── (B) published ⊆ accepted, one declared action at a time ─────────
    # Args are deliberately `null`: what is under test is whether the name
    # REACHES the surface, and every other outcome — a type refusal, the
    # surface's own stated refusal, a success — proves it did. Only the two
    # framework words below mean it did not.
    for name in actions:
        try:
            tf.invoke(f"{EXT}/{name}", None)
        except Exception as why:  # noqa: BLE001 — any refusal but those two is fine
            text = str(why)
            assert "UnknownInvokePath" not in text, (
                f"{name!r} is published and the wire says it does not exist: {text}"
            )
            assert "DeclaredButUnhandled" not in text, (
                f"{name!r} is published and the surface does not implement it: {text}"
            )
    print(f"[demo] all {len(actions)} declared actions reach the surface")

    # ── (C) accepted ⊆ published ────────────────────────────────────────
    assert_rpc_error(
        lambda: tf.invoke(f"{EXT}/{INVENTED}", None), data="UnknownInvokePath"
    )
    assert INVENTED not in actions, "and it is absent from the declaration, as claimed"

    # ── (D) the two words are different facts ───────────────────────────
    # `DeclaredButUnhandled` cannot be provoked from out here on a healthy
    # surface — that is the point of it — so what is asserted is that the word
    # this one answers is the CALLER's, and that the other word exists as its
    # own published member of the vocabulary (F).
    assert_rpc_error(
        lambda: tf.invoke(f"{EXT}/{INVENTED}", "with args this time"),
        data="UnknownInvokePath",
    )

    # ── (E) the channels stay apart ─────────────────────────────────────
    a_slot = slots[0]
    assert_rpc_error(lambda: tf.invoke(f"{EXT}/{a_slot}", None), data="PathIsAReadSlot")
    assert_rpc_error(
        lambda: tf.query(f"{EXT}/{THE_VERB}"), data="PathIsAnAction"
    )
    # ...and the slot half still reads, so the refusal above is about the
    # CHANNEL and not about the path being broken.
    value = tf.query(f"{EXT}/{a_slot}")
    assert value is not None, f"{a_slot!r} is declared readable and reads"
    # ...and the read channel refuses an invented name for the same reason the
    # action channel does, rather than handing back whatever the surface would
    # have answered.
    assert_rpc_error(
        lambda: tf.query(f"{EXT}/{INVENTED}"), data="UnknownIntrospectPath"
    )
    # Every declared slot reads — the read channel's `published ⊆ accepted`.
    for name in slots:
        tf.query(f"{EXT}/{name}")
    print(
        f"[demo] {a_slot!r} refuses a call, {THE_VERB!r} refuses a read, "
        f"and all {len(slots)} declared slots answer"
    )

    # ── (F) the new word is discoverable ────────────────────────────────
    catalogue = call(tf, "rpc/errors")
    entry = next(e for e in catalogue["errors"] if e["code"] == -32602)
    vocabulary = entry["data_vocabulary"]
    assert not entry["data_is_prose"], "-32602 carries words, so they are matchable"
    for word in ("DeclaredButUnhandled", "UnknownInvokePath", "PathIsAReadSlot"):
        assert word in vocabulary, f"{word!r} must be published: {vocabulary}"
    assert vocabulary == sorted(vocabulary), "published as a searchable set"
    print(f"[demo] rpc/errors publishes {len(vocabulary)} words under -32602")

    # ── (G) and the declaration is the same one the SNAPSHOT omits ──────
    # R1632's measurement was right about this half: a snapshot shows the
    # current value of each scalar READ, so it cannot show an action. The two
    # surfaces answer different questions and neither substitutes for the other.
    snapshot = call(tf, "scene/snapshot", {"path": ""})
    shown = set(snapshot["introspect"])
    assert shown, f"the snapshot shows values: {snapshot!r}"
    assert not shown & set(actions), (
        "an action has no value, so a snapshot cannot be the discovery surface"
    )
    assert shown <= set(slots), (
        f"and everything it shows IS a declared read: {sorted(shown - set(slots))}"
    )
    print("[demo] the snapshot shows values; $schema shows the contract")


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        checks(tf)


if __name__ == "__main__":
    run_demo("R1637 — a call must be declared first", body)
