#!/usr/bin/env python3
"""R1852 §5.2 §5.51 §2 #7 — **a capture builds a topology nobody told it.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `capture.t2.14` —
*building a topology from observed messages, with no management API* — as an
**app** verdict whose covering sentence has always been a claim about where a
parse LANDS:

    the parse is domain; where it LANDS is now `Document::observe` (R1645)

That verdict named **no assembly**, which is what R1807's `UNASSEMBLED` ratchet
records: a claim about a composition nobody had composed. This is the
composition, driven on the wire, on the capture section the analysis-tool shell
mounts — closing a census row on a demo that never touches the reference screen
closes a line without the screen gaining anything (the R1722 lesson).

# ★★★★★ Two measurements, and both reversed a premise

**(1) The row's own sentence pointed at the wrong half of the framework.**
`Document::observe` records traffic between sockets **that already exist**: it
answers `NoSuchNode` for a report about a node the document does not have. That
is the right shape for R1645's two layers, where a DRAWING comes first and
observations are compared with it — and it is the wrong shape for a capture,
which has no drawing and whose endpoints are whatever its hops mention. So the
substrate the row rested on could record an observation and could not BUILD a
topology out of observations. `pinion_node_graph::SightedTopology` is that
missing half.

**(2) Every ingredient was already in the capture and nothing asked.** Each of
its rows has carried `hop` — *who sent it and who received it* — since the
screen was written, and `hop` was read to be PAINTED and to pair a reply with
its request, and for nothing else. Measured at R1852, this capture's hops show
**eight** endpoints across **nine** directions and **seven** conversations.
`spec::SESSION`, the session the always-visible context strip says its values
were negotiated for, is **one** of those seven — so eleven of the sixteen rows
below it were being read against premises that are not theirs. That is R1845's
*a fact with no premise*, seen from the premise's side.

# ★★★★★ Where this stands against the floor, and the claim the probe REFUTED

Compiled and run against the reference toolkit at 6.11.1. The round's first
superiority claim was that its tabular model cannot tell *there is nothing here*
from *nothing is known here*. **That is false**: a cell holding an empty value
answers a valid one, a cell nothing was ever set on answers an invalid one, and
one call distinguishes them. The probe deleted the claim before it reached any
gate — the second premise this round measured and reversed.

What the same probe measured that DOES stand:

  * **That model asserts its own completeness.** Asked whether more rows may
    exist, it answered *no* — for a table whose content was a sample. There is
    nowhere in it to say *this is what I saw, not what there is*, so a consumer
    reading a topology out of one is told it holds the whole graph.
  * **The MEANING of its distinction is the consumer's.** *Invalid* there means
    *this index is not the model's*; whether that stands for *unknown* is an
    interpretation each reader makes. `Sighted` names its three answers and
    publishes the words, so a client reads a verdict instead of inferring one.
  * **A state that cannot occur is not expressible here.** An endpoint with no
    edges is representable in any graph model and is a logical impossibility in
    a sightings-only one, because an endpoint is present BECAUSE traffic was
    seen. `Vantage` has three arms and no fourth.

# What is shown

  (A) the capture publishes a topology it was never told — endpoints, directions
      and conversations, all derived from its own hops.
  (B) a direction has THREE answers on the wire, and the middle one is the point:
      *not seen* between two known endpoints is not *no such link*.
  (C) no endpoint is isolated, and that is a property of the vocabulary rather
      than of this capture.
  (D) the standing is `partial` and says why — a topology assembled from traffic
      is never claimed to be whole.
  (E) THE FINDING: the always-visible context strip states a premise negotiated
      for one of those conversations, and now says how far it reaches — and its
      ink says whether it reaches the row a reader is on.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

SCREEN = "hello-packet-view"
EXT = "/external"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def walk_tags(node: Any) -> list[str]:
    """Every tag in a scene tree, wherever it is nested."""
    out: list[str] = []
    if isinstance(node, dict):
        tag = node.get("tag")
        if isinstance(tag, str):
            out.append(tag)
        for value in node.values():
            out.extend(walk_tags(value))
    elif isinstance(node, list):
        for value in node:
            out.extend(walk_tags(value))
    return out


def reading(node: dict[str, Any]) -> str:
    """What a reader is told, normalised — `value` on the wire is typed."""
    value = node.get("value")
    if isinstance(value, dict):
        for key in ("text", "Text", "value"):
            if isinstance(value.get(key), str):
                return value[key]
        return str(value)
    return value if isinstance(value, str) else ""


def announced(app: RpcSubprocess, tag: str) -> dict[str, Any]:
    for node in app.request("scene/access").result["nodes"]:
        if (node.get("tag") or "") == tag:
            return node
    raise AssertionError(f"{tag} is not announced")


def body() -> None:
    with RpcSubprocess(SCREEN, boot_grace=1.5) as d:
        # ── (A) a topology the capture was never told ────────────────────────
        banner("A — the capture publishes a topology built from its own hops")
        topo: Any = d.query(f"{EXT}/topology")
        rows = topo["rows"]
        print(
            f"    {len(topo['endpoints'])} endpoint(s), {len(topo['edges'])} direction(s), "
            f"{len(topo['conversations'])} conversation(s), {topo['sightings']} sighting(s)"
        )
        assert_eq(topo["sightings"], rows, "every row of the capture is one hop")
        ok("more than one endpoint, or the topology says nothing", len(topo["endpoints"]) > 2)
        ok("fewer directions than sightings — hops repeat", len(topo["edges"]) < topo["sightings"])
        ok(
            "and fewer conversations than directions — somebody answered",
            len(topo["conversations"]) < len(topo["edges"]),
        )
        # Every endpoint carries its own vantage and its two degrees, derived.
        for end in topo["endpoints"]:
            ok(f"{end['name']} states a vantage", end["vantage"] is not None)
            ok(f"{end['name']} states both degrees", end["sends_to"] is not None and end["hears_from"] is not None)
        # The hub is the endpoint the most others were seen sending to — derived
        # here from the published degrees rather than named.
        hub = max(topo["endpoints"], key=lambda e: e["hears_from"])
        print(f"    the hub is {hub['name']}: hears from {hub['hears_from']}, answers {hub['sends_to']}")
        ok("the hub was seen both ways", hub["vantage"] == "both")

        # ── (B) three answers, not two ───────────────────────────────────────
        banner("B — a direction has three answers, and the middle one is the point")
        vocab: Any = d.query(f"{EXT}/topology_vocabulary")
        print(f"    sighted vocabulary {vocab['sighted']}, vantage vocabulary {vocab['vantage']}")
        assert_eq(sorted(vocab["sighted"]), ["not_seen", "seen", "unknown"], "three words")
        probe = topo["probe"]
        print(f"    probe: {probe}")
        assert_eq(probe["seen"], "seen", "the negotiated session was seen")
        # ★★★★★ THE CLAIM. Every hop in this capture runs toward the hub, so the
        # reverse of the negotiated session was not seen — which is NOT a claim
        # that no such link exists, and the wire says so with a different word
        # from the one it uses for an endpoint it never heard of.
        assert_eq(probe["reverse"], "not_seen", "the reverse was not seen")
        assert_eq(probe["stranger"], "unknown", "an endpoint no hop mentioned")
        ok(
            "*not seen* and *never heard of* are DIFFERENT answers on the wire",
            probe["reverse"] != probe["stranger"],
        )

        # ── (C) no endpoint is isolated, by construction ─────────────────────
        banner("C — an isolated endpoint is not expressible")
        assert_eq(sorted(vocab["vantage"]), ["both", "receiving", "sending"], "three vantages")
        ok("and *isolated* is not one of them", "isolated" not in vocab["vantage"])
        for end in topo["endpoints"]:
            # Every endpoint is here BECAUSE traffic was seen, so at least one
            # degree is non-zero — the fact the missing fourth arm encodes.
            ok(
                f"{end['name']} was seen at least one way",
                end["sends_to"] > 0 or end["hears_from"] > 0,
            )
            ok(f"{end['name']} names at least one peer", len(end["peers"]) > 0)

        # ── (D) never claimed to be whole ────────────────────────────────────
        banner("D — the standing is partial, and says why")
        print(f"    standing {topo['standing']!r}: {topo['why_partial']}")
        assert_eq(topo["standing"], "partial", "a drawing made of traffic is never whole")
        ok("and the reason is stated rather than implied", "discovery on" in topo["why_partial"])
        ok(
            "with the count of directions nothing drawn accounts for",
            str(len(topo["edges"])) in topo["why_partial"],
        )

        # ── (E) THE FINDING: the premise, and how far it reaches ─────────────
        banner("E — the always-visible premise now says how much of the table it is about")
        print(
            f"    negotiated for {topo['negotiated_session']['a']} <-> "
            f"{topo['negotiated_session']['b']}, one of {topo['negotiated_is_one_of']} "
            f"conversation(s), covering {topo['rows_in_session']} of {rows} rows"
        )
        ok(
            "★ the negotiated session is ONE of several the capture holds",
            topo["negotiated_is_one_of"] > 1,
        )
        ok(
            "★ so the premise covers some rows and not all of them",
            0 < topo["rows_in_session"] < rows,
        )
        # And the strip says it, on screen and to a reader who cannot see it.
        painted = walk_tags(d.snapshot(source="paint"))
        ok("the strip is on screen", "pv.context.session" in painted)
        said = reading(announced(d, "pv.context.session"))
        print(f"    the strip announces: {said!r}")
        ok("the announcement states the reach", f"{topo['rows_in_session']} of {rows}" in said)
        ok(
            "and whether it covers the row a reader is on",
            "including this one" in said or "not negotiated for it" in said,
        )
        # Move to a row outside the negotiated session and the announcement
        # changes — the same predicate the ink follows.
        outside = next(
            n
            for n, row in enumerate(d.query(f"{EXT}/spec")["rows"])
            if row["hop"] not in (
                f"{topo['negotiated_session']['a']} -> {topo['negotiated_session']['b']}",
                f"{topo['negotiated_session']['b']} -> {topo['negotiated_session']['a']}",
            )
        )
        d.invoke(f"{EXT}/select_message", outside)
        moved = reading(announced(d, "pv.context.session"))
        print(f"    on row {outside} it announces: {moved!r}")
        ok("the announcement changed", moved != said)
        ok(
            "★ and it names the hop, which the fixed strip has no room for",
            "the selected row is" in moved,
        )
        ok("while still stating the reach", f"{topo['rows_in_session']} of {rows}" in moved)

    print(f"\n{len(CHECKS)} named check(s) passed")


run_demo("r1852 a capture builds a topology it was never told", body)
