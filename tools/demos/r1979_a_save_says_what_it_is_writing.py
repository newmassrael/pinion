#!/usr/bin/env python3
"""R1979 §5.2 §5.21 — **the save says what it is writing, and a person can
climb back out of a broken document.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the node lab's file half — the save, the open, and the editing verbs
between them — along the state R1977 made reachable: a saved graph that parses
and does not satisfy its own invariants.

# ★★★★★ The debt's sharp end was measured and is FALSE

`debt-an-unsound-graph-can-be-edited-and-nothing-says-what-that-does` named
R1920's `may` hook as the danger: *if it refuses edits because the graph is
unsound, the only route to repairing the document is closed*. Measured at R1979
by reading `Document::may` end to end and then by DRIVING it: it asks whether
the tree exists, whether the node exists, whether the node is an interface end,
and what the label would clash with. It never asks `validate`. So editing an
unsound document is allowed, and (D) below drives the repair to prove it rather
than quoting the source.

# ★★★★★ And the measurement found the real defect one surface over

Driven at R1979, the same broken document answered:

    save   ->  saved · 6 cards · 111767 bytes
    open   ->  opened · 6 cards · 1 fault(s) the gate will name: link 2 in
               tree 0 names a socket that is not there

The READ side named the trouble and the WRITE side did not. A person who had
just broken their graph pressed Save and was told it went fine — which it did,
and which is not the whole truth about what is now on disk. R1979 gives the save
the same clause, from the SAME derivation (`persist::fault_clause`), so the two
halves of one round trip cannot drift apart.

⚠ The save is NOT refused. Refusing to keep a person's work because the work is
unfinished is how an editor loses it — R1977's rule for the read half (*the
graph is here*) applied to the write half.

# What this walk holds

  (A) a clean document saves and says nothing about faults, so (B) is caused.
  (B) ★★★★★ a broken document SAVES and the sentence names its faults.
  (C) ★★★★★ the save's clause and the reopen's clause are the SAME text — one
      derivation, both halves of the round trip.
  (D) ★★★★★ the debt's question: the editing verbs answer on an unsound
      document, the links register NAMES the end that is missing, and deleting
      those rows repairs it — the launch gate goes from shut to open.
  (E) ★ the refusals that remain are about the ACT, not about the document's
      health, so nothing is refused merely for standing on a broken graph.
  (F) ★ and the stored copy is left sound, because a walk that leaves a broken
      graph in the person's data directory is a walk that breaks the next one.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1979_a_save_says_what_it_is_writing.py
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
CLAUSE = "fault(s) the gate will name"

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


def clause_of(sentence: str) -> str | None:
    """The fault clause of a screen sentence, or None when it carries none."""
    for part in str(sentence).split(" · "):
        if CLAUSE in part:
            return " · ".join(str(sentence).split(" · ")[str(sentence).split(" · ").index(part):])
    return None


def break_a_link(whole: str) -> str:
    """A saved document whose links name a card that is not there.

    Built by REMOVING a card the opening graph's wires point at, so the break is
    the consequence of an ordinary-looking edit to a saved file rather than a
    hand-built fixture — the same construction R1977 and R1978 used, kept so the
    three walks are talking about one document.
    """
    envelope = json.loads(whole)
    tree = envelope["document"]["trees"][0]
    landed = {link["to"]["node"] for link in tree["links"]}
    victim = next(
        node
        for node in tree["nodes"]
        if node.get("id") in landed and isinstance(node.get("body"), dict)
    )
    tree["nodes"] = [n for n in tree["nodes"] if n.get("id") != victim["id"]]
    return json.dumps(envelope)


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
        whole = app.query(f"{surface}/archive")

        banner("A — a clean document saves and says nothing about faults")
        review = js(app.query(f"{surface}/review"))
        ok(
            f"A: ★ the canvas is clean to begin with, so everything after it is "
            f"caused — {review['fitness']!r}",
            review["fitness"] == "clean" and not review["findings"],
        )
        said = app.invoke(f"{surface}/save_graph", "")
        app.tick_ms(16)
        ok(
            f"A: ★★★★★ the save reports what it wrote and NOTHING about faults, "
            f"because there are none — {said!r}",
            "saved" in str(said)
            and "cards" in str(said)
            and "bytes" in str(said)
            and CLAUSE not in str(said),
        )

        banner("B — ★★★★★ a broken document SAVES and the save names its faults")
        opened = app.invoke(f"{surface}/open_graph", break_a_link(whole))
        app.tick_ms(16)
        ok(
            f"B: ★ the broken document is on the screen and the open named it "
            f"(R1977) — {opened!r}",
            CLAUSE in str(opened),
        )
        saved = app.invoke(f"{surface}/save_graph", "")
        app.tick_ms(16)
        ok(
            f"B: ★★★★★ and the SAVE names it too — until R1979 this said only "
            f"how many cards and how many bytes — {saved!r}",
            "saved" in str(saved) and CLAUSE in str(saved),
        )

        banner("C — ★★★★★ the two halves of the round trip say the same thing")
        reopened = app.invoke(f"{surface}/open_graph", "")
        app.tick_ms(16)
        ok(
            f"C: ★ the bytes that were written come back — {reopened!r}",
            "opened" in str(reopened) and CLAUSE in str(reopened),
        )
        # ★★★★★ Not merely "both mention faults": the SAME clause, character for
        # character. Two spellings of one fact is the shape this repository keeps
        # paying for, and one derivation is the only thing that rules it out.
        ok(
            f"C: ★★★★★ the save's clause and the open's clause are one text — "
            f"{clause_of(saved)!r}",
            clause_of(saved) is not None and clause_of(saved) == clause_of(reopened),
        )

        banner("D — ★★★★★ the editing verbs answer WHILE the document is unsound")
        # ★★★★★ ORDER IS THE ASSERTION HERE, and the first draft of this walk got
        # it wrong: it drove the permission hook AFTER the repair, where the
        # document is sound and `may` would allow the edit whatever it asked. A
        # mutation that made `Document::may` refuse every edit on an unsound
        # document — the debt's own sharp end — PASSED that draft. So the hook is
        # driven here, while the faults still stand.
        # ★ Driven BEFORE anything is deleted, so the pair is the one the opened
        # document actually holds. A domain rule refusing here is the control for
        # the check below: refusals still happen, and they are about the act.
        names = app.query(f"{surface}/nodes").split(",")
        why = refusal(app, f"{surface}/connect", f"{names[0]},{names[1]}")
        ok(
            f"D: ★ a DOMAIN rule still refuses on an unsound document, and says "
            f"which rule — so what refuses, refuses about the ACT — {why!r}",
            why is not None and "listen" in str(why),
        )
        links = js(app.query(f"{surface}/links"))
        touched = {
            str(row[end])
            for row in links
            for end in ("from", "to")
            if str(row["from"]).startswith("#") or str(row["to"]).startswith("#")
        }
        spare = next(
            name for name in app.query(f"{surface}/nodes").split(",") if name not in touched
        )
        # ⚠ Through `refusal` rather than a bare invoke, so a hook that started
        # refusing fails as an ASSERTION that says what it means, instead of as
        # a bare RPC error the reader has to interpret.
        why = refusal(app, f"{surface}/delete_node", spare)
        app.tick_ms(16)
        ok(
            f"D: ★★★★★ the permission hook lets a card go while the graph is "
            f"UNSOUND — nothing is refused for standing on a broken document, "
            f"which would close the only route to repairing it — {spare!r}: "
            f"refused with {why!r}",
            why is None and spare not in app.query(f"{surface}/nodes").split(","),
        )
        ok(
            "D: ★ and the faults are still standing, so that edit really did "
            "happen on an unsound document",
            js(app.query(f"{surface}/verdict"))["blocking"] > 0,
        )

        banner("E — ★★★★★ and the document can be REPAIRED from the screen")
        links = js(app.query(f"{surface}/links"))
        # ★ The screen already has a word for an end that is not there: every
        # other endpoint is a card NAME and a missing one is `#<id>`. That is
        # what makes the row addressable instead of merely wrong.
        dangling = [
            row
            for row in links
            if str(row["from"]).startswith("#") or str(row["to"]).startswith("#")
        ]
        ok(
            f"E: ★★★★★ the links register NAMES the missing end rather than "
            f"dropping the row — {[(r['id'], r['from'], r['to']) for r in dangling]}",
            len(dangling) > 0,
        )
        before = js(app.query(f"{surface}/verdict"))
        ok(
            f"E: ★ and the launch is shut while they stand — {before}",
            before["may_launch"] is False and before["blocking"] == len(dangling),
        )
        for row in dangling:
            said = app.invoke(f"{surface}/delete_link", str(row["id"]))
            app.tick_ms(16)
            ok(
                f"E: ★★★★★ deleting the dangling link ANSWERS rather than being "
                f"refused for the document's health — link {row['id']}: {said!r}",
                said is not None,
            )
        after = js(app.query(f"{surface}/verdict"))
        ok(
            f"E: ★★★★★ and the gate OPENS, so the broken document was a state a "
            f"person can leave by editing — {before} -> {after}",
            after["blocking"] == 0 and after["may_launch"] is True,
        )

        banner("F — ★ the stored copy is left sound")
        app.invoke(f"{surface}/open_graph", whole)
        app.tick_ms(16)
        said = app.invoke(f"{surface}/save_graph", "")
        app.tick_ms(16)
        ok(
            f"F: ★★★★★ the whole document is back and its save is quiet again — "
            f"{said!r}",
            "saved" in str(said) and CLAUSE not in str(said),
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1979 a save says what it is writing", body)
