#!/usr/bin/env python3
"""R1977 §5.2 §5.21 — **a saved document that is structurally broken opens, and
the gate says so instead of letting it launch.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row names — the pre-launch
validation pass — along the path a broken document ACTUALLY arrives on.

# ★★★★★ The debt's diagnosis was WRONG, and driving it is what said so

R1976 registered this as *sixteen structural arms are driven through a
conversion, not a document*, and named the thing it had not measured: whether
`Archive::read` accepts such a document at all. Measured at R1977 by driving it
rather than by reading the code — the first draft of this walk assumed
acceptance, from having read `Archive::read`'s first sixty lines, and the run
answered:

    Action refused — 'the graph is not sound in 2 ways, starting with:
    link 0 in tree 0 names a socket that is not there'

So the archive DOES validate (`Opening::violations`) and the lab refused the
whole document on it. ⇒ those arms were UNREACHABLE, which is the outcome the
debt's own text says closes it.

# ★★★★★ And the refusal was the real defect, one layer further in

A person whose saved graph had gone structurally wrong could see ONE SENTENCE
and nothing else — on a screen that, since R1976, can say which card every one
of those faults is on. `Opening::take_despite_violations` exists for exactly
this and had **zero callers**, in this tree and in the crate's own tests: the
door was built and nobody opened it.

R1977 splits the two failures, which are not the same kind:

  * **unreadable** — not the envelope, another revision, a taxonomy this build
    lacks. There is no document. Still refused, and nothing changes.
  * **unsound** — it parsed and its own invariants do not hold. There IS a
    document, and looking at it is how a person repairs it. Opened, named in the
    sentence, and held shut by the launch gate.

★ The behaviour canon is the same shape and weaker: its import checks the
snapshot's SHAPE and says *loaded*, and its validation pass then reads FIELD
VALUES only — it has no structural axis, so it opens such a graph and never
mentions it. Here the graph opens AND every fault is named on a card.

# What this walk holds

  (A) the canvas opens clean, so everything after it is caused.
  (B) ★★★★★ a document whose link names a node that is gone OPENS, and the
      sentence says how many faults the gate will name.
  (C) ★★★★★ and the gate names them: the review carries `structure` findings,
      the panel carries their sentences, the fitness is `stopped`.
  (D) ★ the launch is refused while it stands, quoting the gate's own verdict.
  (E) ★★★★★ an UNREADABLE document is still refused and changes nothing — the
      split is a split, not a door left open.
  (F) ★ opening a whole document puts the screen back, so (B)-(D) is a state a
      person can leave rather than a trap.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1977_a_broken_document_is_opened_and_reported.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def refusal(app: RpcSubprocess, path: str, arg: str):
    """Answer the refusal's sentence, or None when the call went through."""
    try:
        app.invoke(path, arg)
        return None
    except Exception as why:  # noqa: BLE001 — the refusal IS the measurement
        return str(why)


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — the canvas opens clean, so what follows is caused")
        review = js(app.query(f"{surface}/review"))
        ok(
            f"A: ★★★★★ clean to begin with — {review['fitness']!r}, "
            f"{len(review['findings'])} finding(s)",
            review["fitness"] == "clean" and not review["findings"],
        )
        whole = app.query(f"{surface}/archive")
        ok(
            f"A: ★ and the screen hands out its own document, which is what a "
            f"person's saved file IS — {len(whole)} bytes",
            len(whole) > 0 and json.loads(whole)["revision"] is not None,
        )

        banner("B — ★★★★★ a broken document is ACCEPTED, not refused")
        # ★ The break is the simplest structural one there is and the one the
        # taxonomy's own header calls unreachable by this crate's edits: a link
        # whose far end names a node that is not in the document. Built by
        # REMOVING a node the opening graph's links point at, so the break is a
        # consequence of an ordinary-looking edit to a saved file rather than a
        # hand-built fixture.
        envelope = json.loads(whole)
        tree = envelope["document"]["trees"][0]
        landed = {link["to"]["node"] for link in tree["links"]}
        victim = next(
            node
            for node in tree["nodes"]
            if node.get("id") in landed and isinstance(node.get("body"), dict)
        )
        before_nodes = len(tree["nodes"])
        tree["nodes"] = [n for n in tree["nodes"] if n.get("id") != victim["id"]]
        ok(
            f"B: ★ the file now has one card fewer, and wires still name it — "
            f"{before_nodes} -> {len(tree['nodes'])}, node {victim['id']}",
            len(tree["nodes"]) == before_nodes - 1,
        )
        said = app.invoke(f"{surface}/open_graph", json.dumps(envelope))
        app.tick_ms(16)
        ok(
            f"B: ★★★★★ and it OPENS — until R1977 this refused the whole "
            f"document and a person saw one sentence and nothing else — {said!r}",
            "opened" in str(said),
        )
        # ★★★★★ And the sentence SAYS the graph is unsound. An `opened` with no
        # more to it would be the screen reporting a success the launch gate is
        # about to refuse — the shape this repository keeps meeting.
        ok(
            f"B: ★★★★★ and the sentence names the faults rather than reporting "
            f"a plain success — {said!r}",
            "fault(s) the gate will name" in str(said),
        )

        banner("C — ★★★★★ and the gate reports what was opened")
        review = js(app.query(f"{surface}/review"))
        structural = [row for row in review["findings"] if row["half"] == "structure"]
        ok(
            f"C: ★★★★★ the review carries a STRUCTURAL finding, which is the "
            f"half nothing on this screen could reach before R1976 — "
            f"{[row['sentence'] for row in structural]}",
            len(structural) > 0,
        )
        ok(
            f"C: ★★★★★ and the fitness says the document cannot run — "
            f"{review['fitness']!r}, may_run={review['may_run']}",
            review["fitness"] == "stopped" and review["may_run"] is False,
        )
        # ★ The panel a person reads, not only the register an agent reads.
        gate = js(app.query(f"{surface}/gate"))
        lines = [row["sentence"] for row in gate]
        ok(
            f"C: ★★★★★ and the PANEL carries it, blocking — "
            f"{[l for l in lines if 'not there' in l or 'socket' in l][:2]}",
            any(
                row["blocks"] and any(f["sentence"] in row["sentence"] for f in structural)
                for row in gate
            ),
        )
        # ★ The count and the list come from one walk, which is R1717's rule and
        # is what makes the number on the button trustworthy.
        verdict = js(app.query(f"{surface}/verdict"))
        ok(
            f"C: ★ the verdict agrees with the review rather than being a "
            f"second opinion — {verdict}",
            verdict["may_launch"] is False and verdict["blocking"] > 0,
        )

        banner("D — ★ the launch is refused while it stands")
        why = refusal(app, f"{surface}/run", True)
        ok(
            f"D: ★★★★★ starting the graph is refused — {why!r}",
            why is not None,
        )
        # ★ And the refusal QUOTES the gate rather than being a second rule, so
        # what a person reads on the button and what stops them are one verdict.
        ok(
            f"D: ★★★★★ and the refusal is the gate's own sentence — "
            f"{verdict['sentence']!r} inside {why!r}",
            verdict["sentence"] in str(why),
        )

        banner("E — ★★★★★ an UNREADABLE document is still refused")
        # ★★★★★ The split has to be a SPLIT. A change that let everything
        # through would satisfy (B) just as well, so what is asserted here is
        # the other side: a text with no document in it changes nothing.
        cards_now = app.query(f"{surface}/nodes")
        why = refusal(app, f"{surface}/open_graph", "definitely not a saved graph")
        ok(
            f"E: ★★★★★ a text that is not the envelope is REFUSED — {why!r}",
            why is not None,
        )
        ok(
            f"E: ★ and the screen is where it was, so a refusal is not a "
            f"half-open — {cards_now!r}",
            app.query(f"{surface}/nodes") == cards_now,
        )
        # ★ And the other unreadable arm, because one of three is a sample and
        # the split is over the KIND of failure rather than over one message.
        why = refusal(
            app,
            f"{surface}/open_graph",
            whole.replace('"revision": 1', '"revision": 77'),
        )
        ok(
            f"E: ★★★★★ so is a document written by another revision, and it "
            f"names the number it found — {why!r}",
            why is not None and "77" in str(why),
        )

        banner("F — ★ and the unsound state is one a person can leave")
        # Opening the WHOLE document again is the ordinary act; the point is
        # that the broken state is not a trap the screen cannot come out of.
        said = app.invoke(f"{surface}/open_graph", whole)
        app.tick_ms(16)
        after = js(app.query(f"{surface}/review"))
        ok(
            f"F: ★★★★★ the screen comes back — {said!r}, fitness "
            f"{after['fitness']!r}",
            "opened" in str(said) and after["fitness"] == "clean",
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1977 a broken document is opened and reported", body)
