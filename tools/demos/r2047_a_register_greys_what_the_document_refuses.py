#!/usr/bin/env python3
"""R2047 §5.39 §5.40 — **the definitions register greys what the document
refuses, and says why before anybody presses.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1986 built the deciding half of this and published it: `may_definition` is the
document's ONE permission surface, three verbs come out of it, and the register
on the wire carries the refusal's own SENTENCE rather than a bool. R1986's own
closing audit then measured that **no painted control read any of it** — the
single call site on that screen was the wire's closure — so a person learned a
verb was refused by pressing it and being refused.

This round paints the register and holds it here.

# ★★★★★ What is being asserted, and why it is not a colour

The refused control is **declared** unavailable and not merely drawn dimmer.
The declaration is what fades the ink, announces the reason to a screen reader
and publishes it on `scene/disabled`; a dimmer colour chosen in the painter
does the first of those three and leaves nothing a walk can read. So every
clause below reads the DECLARATION — the census and the accessibility tree —
and the last one presses, because a greying that a press disagreed with would
be a lie drawn in the right colour.

The reference this is measured against carries a bool: its consumer can grey a
menu item and cannot say why. Here the greying and the sentence are one answer
out of one decision.

# What this walk holds

  (A) the journey reaches the node lab, at the top of the assembled tool.
  (B) ★ the register is EMPTY at rest, and says so rather than showing a
      heading over nothing.
  (C) ★★★★★ a definition is made, and the removal a card stands in the way of
      is inert ON SCREEN, with the document's own sentence as the reason and a
      recourse that says there is something to do about it.
  (D) ★★★★★ the counterfactual, IN THE SAME DOCUMENT: the definition is copied
      and nothing stands for the copy, so the copy's removal is not inert and
      neither is either copy verb. The greying is the document's answer and not
      a habit of the register.
  (E) ★★★★★ the announcement carries the REASON, which is the half a bool
      cannot hold.
  (F) ★★★★★ the press agrees with the paint: pressing the inert removal takes
      nothing away and says the same sentence, and pressing an allowed verb
      acts.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2047_a_register_greys_what_the_document_refuses.py
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
PART = "capture-side"

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


def cards(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def definitions(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/definitions"))["definitions"]


def named(rows: list[dict], name: str) -> dict:
    return next(row for row in rows if row["definition"] == name)


def inert(app: RpcSubprocess) -> dict[str, dict]:
    """Every region the RUNNING screen declares inert, by tag.

    Read off `scene/disabled` rather than out of the paint, because the reason
    is the thing being asserted and a rectangle does not carry one.
    """
    resp = app.request("scene/disabled", {})
    assert resp is not None and resp.result is not None, "scene/disabled answers"
    return {row["tag"]: row for row in resp.result["disabled"]}


def announced(app: RpcSubprocess, tag: str) -> dict | None:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access answers"
    return next((n for n in resp.result["nodes"] if n.get("tag") == tag), None)


def verb_tag(verb: str, definition: str) -> str:
    return f"lab.palette.verb.{verb}.{definition}"


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

        banner("B — ★ the register is empty at rest, and SAYS so")
        ok(
            f"B: ★ the opening graph holds no definition — {definitions(app, surface)}",
            definitions(app, surface) == [],
        )
        head = announced(app, "lab.palette.parts")
        ok(
            f"B: ★★★★★ and the register announces the count rather than "
            f"leaving a heading over silence — {head}",
            head is not None and head.get("value", {}).get("text") == "0 held",
        )

        banner("C — ★★★★★ a removal a card stands in the way of is INERT on screen")
        opening = cards(app, surface)
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", opening[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        rows = definitions(app, surface)
        held = named(rows, PART)
        ok(
            f"C: ★ the wire has said this all along: removal refused, with the "
            f"reason — {held['may']}",
            held["may"]["remove"] is not None,
        )
        census = inert(app)
        control = census.get(verb_tag("remove", PART))
        ok(
            f"C: ★★★★★ and NOW a control on the frame is inert for it, which is "
            f"the half nothing painted before this round — "
            f"{sorted(t for t in census if t.startswith('lab.palette.verb.'))}",
            control is not None,
        )
        ok(
            f"C: ★★★★★ carrying the DOCUMENT's own sentence and not one the "
            f"screen wrote — {control}",
            control["detail"] == held["may"]["remove"],
        )
        ok(
            f"C: ★ classed as a condition of this session, so the recourse says "
            f"there is something to do about it — {control['reason']} / "
            f"{control['recourse']}",
            control["reason"] == "precondition" and control["recourse"] == "satisfy",
        )

        banner("D — ★★★★★ the counterfactual, in the SAME document")
        copy = app.invoke(f"{surface}/copy_definition", PART)
        app.tick_ms(16)
        copy = copy.strip().strip('"')
        rows = definitions(app, surface)
        spare = named(rows, copy)
        ok(
            f"D: ★ nothing stands for the copy, which is what makes the pair "
            f"possible in one document — {spare['used_by']}",
            spare["used_by"] == [],
        )
        census = inert(app)
        open_verbs = [
            verb_tag("remove", copy),
            verb_tag("copy", copy),
            verb_tag("copy", PART),
        ]
        ok(
            f"D: ★★★★★ every verb the document ALLOWS is live — a register that "
            f"greyed all three would pass (C) and mean nothing — "
            f"{[t for t in open_verbs if t in census]}",
            all(tag not in census for tag in open_verbs),
        )
        ok(
            f"D: ★ while the one it refuses is still inert — "
            f"{verb_tag('remove', PART) in census}",
            verb_tag("remove", PART) in census,
        )

        banner("E — ★★★★★ the announcement carries the reason")
        node = announced(app, verb_tag("remove", PART))
        ok(f"E: ★ the control is announced at all — {node}", node is not None)
        ok(
            f"E: ★ and announced disabled — {node['state']}",
            node["state"]["disabled"] is True,
        )
        ok(
            f"E: ★★★★★ with the reason IN THE NAME, which is what a bool cannot "
            f"hold — {node['name']}",
            held["may"]["remove"] in node["name"],
        )
        allowed = announced(app, verb_tag("remove", copy))
        ok(
            f"E: ★★★★★ and the allowed removal states its COST instead, counted "
            f"before anybody presses — {allowed}",
            allowed is not None
            and "would go" in (allowed.get("value", {}).get("text") or ""),
        )

        banner("F — ★★★★★ the press agrees with the paint")
        # ★★★★★ The register sits at the BOTTOM of a pane that scrolls, and the
        # first draft of this walk pressed without scrolling: `scene/click
        # {path}` clicks a node's rect centre, and the centre of a row below the
        # fold is a point in the canvas. Nothing happened, the toast still held
        # the previous verb's answer, and the clause read as a passing press.
        # ⇒ a walk that drives a control must first put the control where a
        # person's cursor could be, which is what R1662's model already says
        # about this pane.
        app.scroll("lab.palette.body", to=(0, 4_000))
        app.tick_ms(16)
        before = [row["definition"] for row in definitions(app, surface)]
        app.click(path=verb_tag("remove", PART))
        app.tick_ms(16)
        after = [row["definition"] for row in definitions(app, surface)]
        ok(
            f"F: ★★★★★ nothing went, whatever the control looked like — "
            f"{after} was {before}",
            after == before,
        )
        said = js(app.query(f"{surface}/said"))
        ok(
            f"F: ★★★★★ and the press says the SAME sentence the greying does, "
            f"so the two halves cannot drift — {said}",
            said is not None and said["clause"] == held["may"]["remove"],
        )
        ok(
            f"F: ★ framed as a refusal — {said['tone']}",
            said["tone"] == "refused",
        )
        app.click(path=verb_tag("copy", copy))
        app.tick_ms(16)
        grown = [row["definition"] for row in definitions(app, surface)]
        ok(
            f"F: ★★★★★ while a verb the document allows ACTS from the same "
            f"register — {grown} was {after}",
            len(grown) == len(after) + 1,
        )
        print(f"\n{len(CHECKS)} check(s) held")


if __name__ == "__main__":
    run_demo("r2047 a register greys what the document refuses", body)
