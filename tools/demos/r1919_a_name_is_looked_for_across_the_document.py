#!/usr/bin/env python3
"""R1919 §5.12 §2 #7 — **the assembled tool looks a name up across its
graph, and says where each hit is.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability six reference-census rows name and none of them
covered — the DCC's `find_node` and the engine's five per-editor finds — through
the node lab as it is mounted in the shell.

# ★★★★★ Why one mechanism closes six rows

The pin's own `covered_by` had already recorded it: `a search across the tree
AND the path to the hit` on the DCC row, `finding a node by name inside this
graph` on the engine's. R1632 and R1644 each found the same shape — one census
chunk being one mechanism spread over several editors — and this is the third.

# What is past both references

Both **perform** the descent to a hit and select it; neither **publishes** the
way in. `Document::find` returns it as `Found::at`, which is this crate's OWN
editing position rather than a list invented for the occasion, so a caller hands
it straight to an editor. On the wire that becomes `depth` — how far away the
hit is — and `through`, the trees the way in passes through under the names a
reader sees. So a caller here can ask *how far away is this* before going, which
is a question neither reference can be asked.

# What this walk holds

  (A) with nothing being looked for, nothing is found and no card is marked —
      the control, without which a screen that marks everything would pass.
  (B) a name that no node carries finds nothing.
  (C) a name a person authored is found, ANNOUNCED as such, and the card it
      names CHANGES ON THE FRAME — in the WIDTH of its edge, keeping the COLOUR
      that belongs to the selection axis, and without moving.
  (D) every card answers to its own name, and WHICH of `Matched`'s two reasons
      this screen can reach is MEASURED rather than asserted.
  (E) the WAY IN is published beside every hit, and each one is a single tree —
      which is this tool stating, in its own output, that it opens exactly one
      graph. A TRIPWIRE: see below.
  (F) the search is a DERIVATION: renaming a card through the wire changes what
      the same needle finds, with no second command. Neither reference's result
      list does that.
  (G) clearing it puts the frame back exactly as it was.
  (H) ★★★★★ *found* and *selected* are TWO states and the frame says which:
      both hits are wide, and the selected one is a different colour. The draft
      this walk first ran against gave a hit the selected card's own colour,
      collapsing two of the four states into one look — while the comment above
      it argued that this was exactly what must not happen. The walk could not
      see it, because it was comparing RECTANGLES and a border is not one.

⚠ TWO HALVES OF THE CAPABILITY ARE UNREACHABLE ON THIS SCREEN, and the walk
MEASURES both rather than skipping them. The lab's document is flat, so every
hit is at depth 0 and the *path* half never runs here; and every card in it is
NAMED (`slot.label = Some(node.id)`), so `Matched::Kind` never runs here
either. Both are exercised by `pinion-node-graph::dcc_find_node`. Asserting a
vocabulary the screen cannot produce is what R1885 recorded the cost of — so
this walk asks the screen what it CAN produce and states the answer.

★★★★★ AND THE FIRST OF THOSE TWO WAS NOT A PROPERTY OF SEARCH — IT WAS A GAP IN
THE ASSEMBLED TOOL, AND R1981 CLOSED IT. `pinion-node-graph` makes, enters,
leaves, ungroups and separates subgraphs; twelve reference-census rows say so
and every one of them was proven by a crate test. No screen in this workspace
constructed one until R1981 gave the node lab `group` / `enter` / `exit`.

⚠★★★★★ THIS WALK'S OWN TRIPWIRE DID NOT FIRE, AND THAT IS THE LESSON. (E) used
to assert `len(through) == 1` and say it would go RED on the day the tool could
descend. It did not: run against the tool that CAN descend, this walk still
passes, because it never folds anything — an assertion is about the graph the
walk itself builds, never about what the tool is capable of. A tripwire in a
fixture nobody deepens is a comment, not a gate.

So (E) now asserts what is true at ANY depth — that the two halves of a hit's
answer agree, `len(through) == depth + 1` — and the DEEP case is driven where it
belongs, in a walk that makes a subgraph:
`tools/demos/r1981_a_part_of_the_graph_becomes_a_graph.py` (E).

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1919_a_name_is_looked_for_across_the_document.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    find_by_tag,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

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


def found(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/found"))


def shared(a: str, b: str) -> str:
    """The longest run of characters BOTH names carry, matched as the screen
    matches — case-insensitive containment. Empty when they share nothing.

    Exists so (H) can hold two hits on the frame at once without this file
    knowing what any card is called.
    """
    best = ""
    lowered = b.lower()
    for start in range(len(a)):
        for stop in range(start + len(best) + 1, len(a) + 1):
            piece = a[start:stop]
            if piece.lower() in lowered:
                best = piece
    return best


def card_marks(app: RpcSubprocess) -> dict:
    """Every card box on the frame, by tag — **its place AND its edge**.

    ★★★★★ R1919 — the edge is half of this and the round's first draft left it
    out, which is how a highlight that changed nothing but the border read as a
    search that had not happened at all. `abs_rects_of` answers *where a pointer
    can reach this mark*; it cannot answer *what the mark looks like*, and a
    highlight is the second question. Both halves are read from ONE snapshot so
    they cannot end up describing two different frames.
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    marks = {}
    for tag, rect in abs_rects_of(snap).items():
        if not (tag.startswith("lab.node.") and tag.count(".") == 2):
            continue
        node = find_by_tag(snap, tag) or {}
        marks[tag] = {"rect": rect, "edge": node.get("style", {}).get("border")}
    return marks


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

        banner("A — nothing is being looked for")
        ok("A: the needle is empty", found(app, surface)["needle"] == "")
        ok("A: and nothing is found", not found(app, surface)["hits"])
        rest = card_marks(app)
        ok(f"A: the canvas draws cards to search — {len(rest)}", len(rest) >= 2)

        banner("B — a name nothing carries")
        ok(
            "B: an unheld name finds nothing",
            app.invoke(f"{surface}/find", "no node is called this") == 0,
        )
        ok(
            "B: and the frame is untouched",
            card_marks(app) == rest,
        )

        banner("C — a name a person authored")
        subject = sorted(rest)[0].removeprefix("lab.node.")
        count = app.invoke(f"{surface}/find", subject)
        app.tick_ms(16)
        ok(f"C: {subject} answers to its own name — {count}", count >= 1)
        hits = found(app, surface)["hits"]
        ok(
            f"C: and it is the one named — {[h['node'] for h in hits]}",
            any(h["node"] == subject for h in hits),
        )
        mine = next(h for h in hits if h["node"] == subject)
        ok(
            f"C: announced as a name a PERSON gave it — {mine['because']}",
            mine["because"] == "label",
        )
        marked = card_marks(app)
        mark = f"lab.node.{subject}"
        ok(
            "C: ★★★★★ and the card CHANGES ON THE FRAME — a search a reader "
            "cannot see is a search that did not happen",
            marked[mark] != rest[mark],
        )
        # ★★★★★ WHICH channel changed, because that is the round's design
        # decision and a comment is not a gate. The edge carries two axes at
        # once — WIDTH is the search, COLOUR is the selection — so a hit that
        # is not selected has to gain weight WITHOUT taking a selected card's
        # colour. The first draft gave it `accent_line`, the colour selection
        # already owns, and the two states were then the same edge on the
        # frame. Nothing saw it: this walk was comparing rectangles.
        was, now = rest[mark]["edge"], marked[mark]["edge"]
        ok(
            f"C: ★★★★★ the edge gains WEIGHT — {was['width']} then {now['width']}",
            now["width"] > was["width"],
        )
        ok(
            f"C: ★★★★★ and keeps its COLOUR, which is the SELECTION axis's — "
            f"{was['color']}",
            now["color"] == was["color"],
        )
        ok(
            "C: ★ and the card does not MOVE — a search re-reads a graph, it "
            "does not re-lay it out",
            marked[mark]["rect"] == rest[mark]["rect"],
        )
        # ⚠ Which cards SHOULD have changed is read off the wire rather than
        # assumed to be one: a card's name can be a substring of another's, and
        # a walk that assumed a single hit would fail on a graph it never saw.
        struck = {f"lab.node.{h['node']}" for h in hits}
        moved = {tag for tag in rest if marked[tag] != rest[tag]}
        ok(
            f"C: ★ and EXACTLY the hits change — {sorted(moved)} against "
            f"{sorted(struck)}",
            moved == struck & set(rest),
        )

        banner("D — every card answers to its own name, and WHICH reasons this screen can reach")
        # ⚠★★★★★ The reasons are MEASURED here rather than asserted. R1885
        # recorded what asserting an unreachable vocabulary costs: rules and
        # gate lines that exist for a state nothing can produce. So this asks
        # the screen which of `Matched`'s two arms it can actually produce, and
        # then says which — rather than demanding both and passing vacuously,
        # or demanding one and hiding that the other is unreachable.
        reasons = set()
        depths = set()
        for tag in sorted(rest):
            name = tag.removeprefix("lab.node.")
            answered = app.invoke(f"{surface}/find", name)
            ok(f"D: {name} answers to its own name — {answered}", answered >= 1)
            for hit in found(app, surface)["hits"]:
                reasons.add(hit["because"])
                depths.add(hit["depth"])
                ok(
                    f"D: {hit['node']} really carries {name!r}",
                    name.lower() in hit["shown"].lower(),
                )
        ok(f"D: the reasons this screen produces — {sorted(reasons)}", reasons)
        ok(
            "D: ★ every card on this screen is NAMED, so `kind` is unreachable "
            "HERE — that half is proven by pinion-node-graph::dcc_find_node, "
            "and measuring it beats asserting a vocabulary nothing can produce",
            reasons == {"label"},
        )
        ok(
            f"D: and this document is FLAT, so every hit is at depth 0 — "
            f"{sorted(depths)}. The PATH half is the crate proof's too",
            depths == {0},
        )

        banner("E — the way IN is published, and it says how far this tool can go")
        # ★★★★★ R1919 — `through` is the OTHER half of the answer and nothing
        # here was reading it, so it could have been wrong in silence. Reading
        # it turns D's `depth == 0` from a number into a STATEMENT ABOUT THIS
        # TOOL: every way in is one entry long, which is to say the assembled
        # tool opens exactly ONE tree and there is no second one to descend to.
        #
        # ⚠★★★★★ R1981 — this used to assert `len(way) == 1` as a TRIPWIRE for
        # the day the assembled tool could descend. That day came and this walk
        # STILL PASSED, because it never folds a subgraph: what a walk asserts
        # is a property of the graph the walk builds. The deep case is driven in
        # r1981_a_part_of_the_graph_becomes_a_graph.py (E); what belongs here is
        # the invariant that holds at every depth — the two halves of a hit's
        # answer agree, so neither can drift from the other unnoticed.
        app.invoke(f"{surface}/find", subject)
        app.tick_ms(16)
        hits = found(app, surface)["hits"]
        ways = [h["through"] for h in hits]
        ok(f"E: every hit publishes the way IN to it — {ways}", ways and all(ways))
        ok(
            "E: ★★★★★ and each way in is one longer than the hit's own depth, "
            "so `depth` and `through` are two renderings of one answer rather "
            f"than two answers — {[(h['depth'], h['through']) for h in hits]}",
            all(len(h["through"]) == h["depth"] + 1 for h in hits),
        )

        banner("F — the hits are a DERIVATION, not a stored list")
        app.invoke(f"{surface}/find", subject)
        app.tick_ms(16)
        before = len(found(app, surface)["hits"])
        renamed = f"{subject}-moved"
        app.invoke(f"{surface}/rename", f"{subject},{renamed}")
        app.tick_ms(16)
        after = found(app, surface)["hits"]
        ok(
            f"F: ★★★★★ renaming the node changes what the SAME needle finds, "
            f"with no second command — {before} then {len(after)}",
            all(h["node"] != subject for h in after),
        )
        ok(
            "F: and the new name answers",
            app.invoke(f"{surface}/find", renamed) >= 1,
        )
        app.invoke(f"{surface}/rename", f"{renamed},{subject}")
        app.tick_ms(16)

        banner("G — clearing it puts the frame back")
        ok("G: the empty needle finds nothing", app.invoke(f"{surface}/find", "") == 0)
        app.tick_ms(16)
        ok("G: nothing is published as found", not found(app, surface)["hits"])
        ok(
            "G: ★ and every card is exactly where it was before any search",
            card_marks(app) == rest,
        )

        banner("H — found and selected are TWO states, and the frame says which")
        # ★★★★★ R1919 — the assertion the round's comment had only DESCRIBED.
        # A card can be found-and-selected, found-and-not, selected-and-not-
        # found, or neither, and a reader has to tell them apart on the frame.
        # That works only if the two axes take different channels of the edge:
        # WIDTH is the search, COLOUR is the selection. The draft this walk
        # first ran against gave a hit the selected colour, collapsing two of
        # the four states into one look — with the comment above it arguing
        # that exactly this must not happen.
        # ⚠ The needle and the second card are DERIVED from the names this
        # screen actually draws rather than spelled here, so a walk that has to
        # hold two hits AT ONCE cannot be defeated by a graph whose cards were
        # renamed. A first draft used the common prefix of every name and
        # measured it empty on the very screen it was written for.
        names = sorted(tag.removeprefix("lab.node.") for tag in rest)
        pair = next(
            (
                (name, shared(subject, name))
                for name in names
                if name != subject and shared(subject, name)
            ),
            None,
        )
        ok(f"H: two cards share a word to search for — {pair}", pair is not None)
        other, stem = pair
        app.invoke(f"{surface}/select", subject)
        app.tick_ms(16)
        app.invoke(f"{surface}/find", stem)
        app.tick_ms(16)
        both = card_marks(app)
        hit_names = {h["node"] for h in found(app, surface)["hits"]}
        ok(
            f"H: it finds the selected one AND another — {sorted(hit_names)}",
            {subject, other} <= hit_names,
        )
        chosen_edge = both[mark]["edge"]
        plain_edge = both[f"lab.node.{other}"]["edge"]
        ok(
            f"H: ★ both are found, so both are WIDE — {chosen_edge['width']} "
            f"and {plain_edge['width']}",
            chosen_edge["width"] == plain_edge["width"] > rest[mark]["edge"]["width"],
        )
        ok(
            "H: ★★★★★ and the selected one is a DIFFERENT COLOUR, so *found "
            f"and selected* is not the same look as *found* — {chosen_edge['color']} "
            f"against {plain_edge['color']}",
            chosen_edge["color"] != plain_edge["color"],
        )
        ok(
            "H: ★ while the unselected hit keeps the colour it had before any "
            "search — the search axis does not touch the selection's channel",
            plain_edge["color"] == rest[f"lab.node.{other}"]["edge"]["color"],
        )
        app.invoke(f"{surface}/find", "")
        app.tick_ms(16)

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1919_a_name_is_looked_for_across_the_document", body))
