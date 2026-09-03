#!/usr/bin/env python3
"""R1981 §5.2 §5.11 — **a person folds part of this graph into a graph of its
own, goes inside it, and comes back.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability twelve reference-census rows name — make, enter,
exit, ungroup, separate, insert — through the node lab **as it is mounted in
the shell**, which is the half those twelve rows never had.

# ★★★★★ The debt this closes, and why a census row was not enough

`debt-the-assembled-tool-cannot-open-a-subgraph` measured the gap and named it
precisely: every one of those `have` verdicts is proven by a CRATE test, no
screen in this workspace ever constructed a subgraph, and so the assembled tool
opened exactly one tree. The census was not wrong — it measures the framework —
but *the framework can* and *this app does* are different sentences, and only
one of them is what a person gets.

# ★★★★★ Two other walks assert the OLD fact, and both go red today

That debt left tripwires rather than prose, and this round is the day they
fire:

* `r1919_a_name_is_looked_for_across_the_document.py` (E) asserted that every
  search hit's way in is ONE tree long — the tool saying, in its own output,
  that it opens a single graph.
* `r1920_an_agent_asks_before_it_edits.py` (F) asserted that EVERY card on the
  frame is deletable, because the crate's only per-node refusal is *a tree's
  own interface end may not be deleted* and an interface end lives only inside
  a subgraph.

Both were strengthened by this round rather than deleted: (E) now asserts a way
in of depth two exists, and (F) now drives the refusal it could not reach.

# ⚠ What the debt did NOT know, measured here

`NodeId` is minted PER TREE (`Tree::next_node`), so the same number names a
different card in every tree. The screen kept four side tables keyed by it, and
a selection, a picked wire and a stacking order carried across a descent would
name whatever card in the new tree happened to have that number. Nothing about
that is visible — it is not a crash, it is the wrong card. `stand_in` drops all
of it on the way through, and (D) below is what performs the check.

# What this walk holds

  (A) the journey reaches the node lab, and the tool says where it is standing:
      one tree deep, with no way up.
  (B) ★ two cards are chosen — which the wire could not express before this
      round — and folded into a subgraph. The cards leave this tree and one
      card takes their place.
  (C) ★★★★★ going INSIDE it shows that tree's own cards, and the breadcrumb
      names the way in.
  (D) ★ nothing came through the door with us: what was selected out there is
      not selected in here.
  (E) ★★★★★ a name that lives INSIDE is found from the root with a way in TWO
      trees long — the assertion R1919 left as a tripwire.
  (F) ★★★★★ the interface end inside REFUSES to be deleted, quoting the model —
      the refusal R1920's walk asserted could not be reached from any screen.
  (G) ★ coming back out stands where we came from, with the cards that were
      there.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1981_a_part_of_the_graph_becomes_a_graph.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, find_by_tag, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
PART = "capture-side"
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


def standing(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/standing"))


def card_names(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def editable(app: RpcSubprocess, surface: str) -> dict:
    """What the screen says each card will and will not allow (R1920)."""
    return {row["node"]: row for row in js(app.query(f"{surface}/editable"))["nodes"]}


def selected(app: RpcSubprocess, surface: str) -> list[str]:
    """The cards the screen says are chosen right now."""
    return js(app.query(f"{surface}/selection"))


def crumb_text(app: RpcSubprocess) -> str:
    """What the breadcrumb chip actually PAINTS.

    Read off the frame rather than off the wire, because the claim is that a
    person standing in front of the screen can see where they are.

    ⚠ The first draft read `lab.appbar.graph` and got nothing: mounted in the
    shell, the lab's own app bar is never painted — the shell draws its own. A
    mark can be perfectly correct and in a place the assembled tool does not
    show, and only asking the frame says which.
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)

    def words(node, out):
        if isinstance(node, dict):
            tag = node.get("tag")
            if isinstance(tag, str) and tag.startswith("lab.crumb"):
                for kid in node.get("children") or []:
                    words(kid, out)
                text = node.get("content") or node.get("text")
                if text:
                    out.append(text)
                return
            for value in node.values():
                words(value, out)
        elif isinstance(node, list):
            for value in node:
                words(value, out)

    # ⚠ R1982 — EVERY crumb chip, not one. R1981 drew the whole path in a single
    # chip; R1982 split it into a chip per step so each one above could be
    # pressed, and this read went from "the joined sentence" to "the step a
    # person is standing on". What the check below is about is unchanged — that
    # the FRAME says the way in — so the reader was widened rather than the
    # assertion weakened.
    out: list[str] = []
    words(snap, out)
    return "  /  ".join(out)


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "A: the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — the tool says where it is standing")
        at = standing(app, surface)
        # ⚠ `depth` counts DESCENTS and is 0 at the top, while `through` has one
        # entry there — the model's own two answers, and asserting both is what
        # caught the screen's first draft reading them as the same number.
        ok(
            f"A: ★ it opens at the top, with nowhere above, and says so — {at}",
            at["depth"] == 0 and at["inside"] is False and len(at["through"]) == 1,
        )
        outside = card_names(app, surface)
        ok(f"A: ★ with the opening graph's cards — {len(outside)}", len(outside) > 2)
        ok(
            "A: ★ and the count it publishes is the count of cards it shows, so "
            f"the two halves of 'where am I' cannot disagree — {at['cards']}",
            at["cards"] == len(outside),
        )
        # ★ The control for (C): at the top the chip carries ONE step. Without
        # this, a chip that always printed every tree in the document would pass
        # (C) while saying nothing about where a person is.
        top_crumb = crumb_text(app)
        ok(
            f"A: ★ the canvas chip says the graph and nothing beyond it — "
            f"{top_crumb!r}",
            top_crumb == at["through"][0],
        )

        banner("B — ★ two cards are chosen and folded into a graph of their own")
        first, second = outside[0], outside[1]
        app.invoke(f"{surface}/select", first)
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", second)
        app.tick_ms(16)
        chose = selected(app, surface)
        ok(
            f"B: ★★★★★ the wire can name a set — before R1981 it could say only "
            f"'this one' — {chose}",
            sorted(chose) == sorted([first, second]),
        )
        said = app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        after = card_names(app, surface)
        ok(
            f"B: ★★★★★ the two cards have left this tree and the part is here in "
            f"their place — {said!r}, {len(outside)} -> {len(after)}",
            PART in after and first not in after and second not in after,
        )
        ok(
            "B: ★ and nothing is selected, because what WAS selected is not in "
            f"this tree any more — {selected(app, surface)}",
            not selected(app, surface),
        )

        banner("C — ★★★★★ going inside shows that graph's own cards")
        app.invoke(f"{surface}/enter", PART)
        app.tick_ms(16)
        at = standing(app, surface)
        ok(
            f"C: ★★★★★ the tool has descended once and says the way in — {at}",
            at["depth"] == 1 and at["inside"] is True and len(at["through"]) == 2,
        )
        ok(
            f"C: ★ the breadcrumb ends at the part a person folded — {at['through']}",
            at["through"][-1] == PART,
        )
        inside = card_names(app, surface)
        ok(
            f"C: ★★★★★ and the cards are the ones that went in, not the ones out "
            f"there — {inside}",
            first in inside and second in inside and PART not in inside,
        )
        # ★★★★★ The FRAME has to say it too. A tool that changes what every card
        # on it means and mentions the change only on the wire has moved a
        # person somewhere without telling them.
        shown = crumb_text(app)
        ok(
            f"C: ★★★★★ and the FRAME says the way in, on the canvas — {shown!r} "
            f"against {at['through']}",
            all(step in shown for step in at["through"]),
        )

        banner("D — ★ nothing came through the door with us")
        # ⚠ `NodeId` is minted per tree, so a selection carried across would not
        # be empty — it would name whatever card in here holds that number.
        ok(
            f"D: ★★★★★ nothing is selected in here — {selected(app, surface)}",
            not selected(app, surface),
        )

        banner("E — ★★★★★ a name inside is found from the root, two trees away")
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        app.invoke(f"{surface}/find", first)
        app.tick_ms(16)
        hits = js(app.query(f"{surface}/found"))["hits"]
        deep = [h for h in hits if len(h["through"]) >= 2]
        ok(
            f"E: ★★★★★ at least one hit is TWO trees in — this is the assertion "
            f"r1919 (E) left as a tripwire, now the other way up — {[h['through'] for h in hits]}",
            deep,
        )
        ok(
            f"E: ★ and its way in passes through the part — {deep[0]['through']}",
            deep[0]["through"][-1] == PART,
        )
        app.invoke(f"{surface}/find", "")
        app.tick_ms(16)

        banner("F — ★★★★★ the interface end inside refuses to be deleted")
        app.invoke(f"{surface}/enter", PART)
        app.tick_ms(16)
        rows = editable(app, surface)
        ends = sorted(name for name, row in rows.items() if row["delete"] == "refused")
        ok(
            f"F: ★★★★★ a card in here says it may NOT be deleted — r1920 (F) "
            f"asserted every card on this tool was deletable, because this one "
            f"could not be reached — refused: {ends}, of {sorted(rows)}",
            ends,
        )
        refused = None
        try:
            app.invoke(f"{surface}/delete_node", ends[0])
        except Exception as why:  # noqa: BLE001 — the refusal is the assertion
            refused = str(why)
        ok(
            f"F: ★★★★★ and the act is refused, in the MODEL's own words — {refused!r}",
            refused and "interface" in refused.lower(),
        )

        banner("G — ★ coming back out stands where we came from")
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        at = standing(app, surface)
        ok(
            f"G: ★ back at the top, with nowhere above — {at}",
            at["depth"] == 0 and at["inside"] is False,
        )
        ok(
            f"G: ★★★★★ and the cards are the ones that were out here, the part "
            f"among them — {card_names(app, surface)}",
            sorted(card_names(app, surface)) == sorted(after),
        )
        refused_at_top = None
        try:
            app.invoke(f"{surface}/exit", "")
        except Exception as why:  # noqa: BLE001 — the refusal is the assertion
            refused_at_top = str(why)
        ok(
            f"G: ★ and there is nowhere above the top, said rather than "
            f"silently ignored — {refused_at_top!r}",
            refused_at_top,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1981 a part of the graph becomes a graph", body)
