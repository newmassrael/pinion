#!/usr/bin/env python3
"""R1630 §5.12 §2 #7 — a published vocabulary is the one a client can observe.

R1629 put four closed vocabularies on the wire — the derivation `kind`, its
`source`, the node `channel`, and the evidence `type` — and `rpc/schema`
publishes the accepted set for each. A published set is a PROMISE, and a
promise nothing checks is the shape this tree keeps finding stale: an enum
grows a variant, the hand-written list beside it does not, and every consumer
that "covers the vocabulary" silently covers one fewer case while the code
compiles and the tests pass.

This round makes the list checkable at BUILD time (`#[derive(VariantCensus)]`
asserts `ALL.len() == ARMS` for all eighteen of them). This demo checks the
other end — the wire — where a build gate cannot reach:

* **soundness** — every value a client actually observes is in the set it was
  told to expect. A value outside it is a word no client can branch on.
* **reachability** — every value in the published set is produced by some real
  surface in this run. A promise nothing can fulfil is as misleading as a
  missing one, and it is the direction a census cannot see: a stale list that
  is too LONG still has the right length until someone looks.

The two directions together are what "closed" means. Neither alone is enough:
soundness passes trivially against an over-long list, and reachability passes
trivially against an under-long one.

`deferred` is the one channel arm no chart or list surface here produces on its
own — it belongs to a `Scroll`, so this drives a virtualized list to reach it
rather than leaving the arm unexercised.

Run from the workspace root:
    cargo build -p hello-boxplot -p hello-candlestick -p hello-multi-select --release
    python3 tools/demos/r1630_a_vocabulary_counts_its_own_arms.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    call,
    run_demo,
)

BOX_WIN = (820, 500)
CANDLE_WIN = (520, 420)
LIST_WIN = (520, 420)
LIST_SCROLL_TAG = "vlist_scroll"


def published_sets(tf: RpcSubprocess) -> dict[str, list[str]]:
    """The accepted-value sets `rpc/schema` promises, keyed `Type.field`.

    Read off the wire rather than restated here: a demo that hard-coded the
    four vocabularies would be a fifth copy of the closed set, which is the
    defect this round exists to remove.
    """
    schema = call(tf, "rpc/schema", {})
    out: dict[str, list[str]] = {}
    for wire_type in schema["types"]:
        fields = wire_type.get("shape", {}).get("fields")
        if not fields:
            continue
        for field in fields:
            # The wire key is `values` — the field's own doc calls it the
            # accepted set, and reading it off the schema rather than naming
            # the four vocabularies here is the point of the whole check.
            accepted = field.get("values")
            if accepted:
                out[f"{wire_type['name']}.{field['name']}"] = list(accepted)
    return out


def derivations(
    tf: RpcSubprocess, tag: str, win: tuple, source: str | None = None
) -> dict:
    params: dict = {"tag": tag, "viewport": list(win)}
    if source is not None:
        params["from"] = source
    return call(tf, "scene/derivations", params)


#: Which payload key each evidence `type` must carry. The demo's copy of the
#: mapping is deliberate: it is the CLIENT's side of the contract, and a client
#: that read the key name out of the same place the server writes it would be
#: proving nothing.
PAYLOAD_KEY = {"name": "name", "real": "value", "count": "count", "flag": "flag"}


def observe(answer: dict, seen: dict[str, set[str]]) -> None:
    """Fold one answer into the observed-value sets, checking each entry.

    ★ The per-entry check is the round's own debt made visible. Before this
    round the serializer matched `Evidence` — a `#[non_exhaustive]` enum, one
    crate over — so its match ended in a wildcard and a shape added upstream
    would have arrived as a bare `{"type": "..."}` with the payload silently
    dropped. That failure has a signature a client can test for: an evidence
    object with ONE key. So every entry is checked, everywhere, rather than
    once in a section of its own.
    """
    seen.setdefault("DerivationsOutcome.channel", set()).add(answer["channel"])
    for entry in answer.get("derivations", []):
        seen.setdefault("DerivationWire.kind", set()).add(entry["kind"])
        seen.setdefault("DerivationWire.source", set()).add(entry["source"])
        evidence = entry["evidence"]
        shape = evidence["type"]
        seen.setdefault("EvidenceWire.type", set()).add(shape)
        key = PAYLOAD_KEY.get(shape)
        assert key is not None, f"unknown evidence shape {shape}: {entry}"
        assert key in evidence, (
            f"{entry['name']}: `{shape}` arrived with no payload — the "
            f"signature of a shape that fell through the serializer: {evidence}"
        )
        assert len(evidence) == 2, (
            f"{entry['name']}: an evidence carries its discriminator and its "
            f"own field, nothing else: {evidence}"
        )
        # ...and both axes of the 2x2 agree with the kind they came from.
        assert entry["source"] in ("data", "request"), entry
        assert isinstance(entry["picture_has_more"], bool), entry


def a_chart_reaches_every_kind_and_every_shape(
    tf: RpcSubprocess, seen: dict[str, set[str]]
) -> None:
    # Boxes first, then violins: the violin mark is what produces an estimate,
    # and an estimate is what produces `real`, `flag` and `invented`.
    observe(derivations(tf, "chart", BOX_WIN), seen)
    tf.click(path="violin")
    tf.tick(0.016)
    violin = derivations(tf, "chart", BOX_WIN)
    observe(violin, seen)
    assert violin["derivations"], "the violin mark states its estimate"

    # A painted node: the wrong KIND of node to ask, which is its own answer.
    observe(derivations(tf, "chart.box.0", BOX_WIN), seen)
    # The binding's own External: a §3 escape hatch, opaque to the framework.
    # Asked of the STATE scene, because that is where an External lives — the
    # paint tree carries what the view emitted, and a view-fn binding's view
    # emits a container. Naming the scene is the point of the parameter.
    escape = derivations(tf, "method", BOX_WIN, source="state")
    assert_eq(escape["kind"], "External", "the state scene holds the escape")
    observe(escape, seen)

    # A log axis adds the omissions, so `omitted` is reached from real data
    # rather than from a contrived case.
    tf.click(path="logscale")
    tf.tick(0.016)
    observe(derivations(tf, "chart", BOX_WIN), seen)
    print(f"[demo] boxplot reached: {sorted(seen.get('DerivationWire.kind', set()))}")


def the_projection_did_not_change_the_wire(tf: RpcSubprocess) -> None:
    """R1630 moved the serializer onto a closed projection of `Evidence`. That
    is a refactor, and a refactor that changes the wire is not one — the round
    before this learned that when one refusal's wording moved and a demo caught
    it. So the values themselves are read back and checked."""
    violin = derivations(tf, "chart", BOX_WIN)
    by_name: dict[str, dict] = {}
    for entry in violin["derivations"]:
        by_name.setdefault(entry["name"], entry)

    kernel = by_name["kernel"]
    assert_eq(kernel["evidence"]["type"], "name", "a kernel is a name")
    assert_eq(kernel["evidence"]["name"], "gaussian", "and the name survives")

    bandwidth = by_name["bandwidth"]
    assert_eq(bandwidth["evidence"]["type"], "real", "a bandwidth is a real")
    assert bandwidth["evidence"]["value"] > 0.0, f"and positive: {bandwidth}"
    assert_eq(bandwidth["unit"], "value", "in the value axis' units")

    samples = by_name["samples"]
    assert_eq(samples["evidence"]["type"], "count", "a sample tally is a count")
    assert samples["evidence"]["count"] > 0, f"and non-empty: {samples}"

    bounded = by_name["bounded"]
    assert_eq(bounded["evidence"]["type"], "flag", "bounded is yes or no")
    assert_eq(bounded["evidence"]["flag"], False, "and this estimate is not")

    # The narrowing still works, which the projection could have broken by
    # changing what a `kind` compares against.
    narrowed = call(
        tf,
        "scene/derivations",
        {"tag": "chart", "kind": "invented", "viewport": list(BOX_WIN)},
    )
    assert_eq(narrowed["filter"], "invented", "the answer echoes the narrowing")
    assert narrowed["derivations"], "and the violin invented something"
    for entry in narrowed["derivations"]:
        assert_eq(entry["kind"], "invented", f"only that kind: {entry}")
    print("[demo] the projection is invisible on the wire")


def the_method_and_its_refusal_are_still_published(tf: RpcSubprocess) -> None:
    methods = call(tf, "rpc/methods", {})
    names = [m["name"] for m in methods["methods"]]
    assert "scene/derivations" in names, "the method is discoverable"
    errors = call(tf, "rpc/errors", {})
    words = str(errors)
    assert "UnknownDerivationKind" in words, (
        "the filter's refusal is in the published vocabulary, so a client "
        "learns it without reading our source"
    )
    print("[demo] the method and its refusal are published")


def a_bar_reaches_the_discarded_kind(
    tf: RpcSubprocess, seen: dict[str, set[str]]
) -> None:
    tf.click(path="caps")
    tf.click(path="bar")
    tf.tick(0.016)
    bars = derivations(tf, "chart", CANDLE_WIN)
    observe(bars, seen)
    discarded = [d for d in bars["derivations"] if d["kind"] == "discarded"]
    assert discarded, "caps asked for under a bar mark are reported"
    assert_eq(discarded[0]["source"], "request", "the picture ignored a request")
    print("[demo] candlestick reached: discarded")


def a_scroll_reaches_the_deferred_channel(
    tf: RpcSubprocess, seen: dict[str, set[str]]
) -> None:
    answer = derivations(tf, LIST_SCROLL_TAG, LIST_WIN)
    observe(answer, seen)
    assert_eq(
        answer["channel"],
        "deferred",
        "a viewport decides WHERE a drawing appears, never how it was produced",
    )
    assert_eq(answer["published"], False, "so it states nothing of its own")
    print("[demo] virtualized list reached: deferred")


def body() -> None:
    seen: dict[str, set[str]] = {}
    with RpcSubprocess("hello-boxplot", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        promised = published_sets(tf)
        a_chart_reaches_every_kind_and_every_shape(tf, seen)
        the_projection_did_not_change_the_wire(tf)
        the_method_and_its_refusal_are_still_published(tf)
    with RpcSubprocess("hello-candlestick", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        a_bar_reaches_the_discarded_kind(tf, seen)
    with RpcSubprocess("hello-multi-select", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        a_scroll_reaches_the_deferred_channel(tf, seen)

    # ── the four vocabularies, both directions ──────────────────────────
    fields = [
        "DerivationWire.kind",
        "DerivationWire.source",
        "DerivationsOutcome.channel",
        "EvidenceWire.type",
    ]
    for field in fields:
        assert field in promised, f"{field} publishes no accepted set: {promised}"
        expected = set(promised[field])
        got = seen.get(field, set())
        assert got, f"{field} was never observed"
        # Soundness: nothing outside the promise.
        assert got <= expected, f"{field} produced {got - expected}, not promised"
        # Reachability: nothing in the promise that no surface produces.
        assert expected <= got, f"{field} promises {expected - got}, unreachable"
        assert_eq(sorted(got), sorted(expected), f"{field} is exactly its promise")
        print(f"[demo] {field}: {sorted(got)}")

    # ...and the sizes are the ones the closed types have, restated on the wire
    # so a client reads the cardinality without counting the enum's source.
    assert_eq(len(promised["DerivationWire.kind"]), 4, "the 2x2 has four cells")
    assert_eq(len(promised["DerivationWire.source"]), 2, "two sources")
    assert_eq(len(promised["DerivationsOutcome.channel"]), 4, "four channels")
    assert_eq(len(promised["EvidenceWire.type"]), 4, "four evidence shapes")

    # The `filter` parameter accepts the SAME set the answers carry — one
    # vocabulary, published twice, and a drift between them would let a client
    # ask for a kind that can never come back.
    assert_eq(
        sorted(promised["DerivationsOutcome.filter"]),
        sorted(promised["DerivationWire.kind"]),
        "what a client may ASK for is what an answer may CARRY",
    )

    # Every other published set stays sound too — this round censused eighteen
    # `ALL` lists and four of them reach the wire, but nothing here should have
    # emptied any of the others.
    for field, values in promised.items():
        assert values, f"{field} publishes an EMPTY accepted set"
        assert len(set(values)) == len(values), f"{field} repeats a value"
    print(f"[demo] {len(promised)} published vocabularies, none empty, none repeating")

    print("[demo] a vocabulary counts its own arms")


if __name__ == "__main__":
    run_demo("R1630 §5.12 — a vocabulary counts its own arms", body)
