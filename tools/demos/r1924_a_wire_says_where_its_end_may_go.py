#!/usr/bin/env python3
"""R1924 §5.12 §2 #3 — **the assembled tool says where a wire's end may be
re-aimed, and why it may not, BEFORE the hand lets go.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability three reference-census rows name — the engine's
*may relinking start at this pin*, *can this connection be relinked to that
pin*, and *relink it* — through the node lab as it is mounted in the shell.

# ★★★★★ What R1924 measured before it built anything

Only TWO of those three were ever missing. The commit has been
`Document::relink` since R1681 — it keeps the wire's id, its mute and its place
in the order — and four rounds carried the whole group as absent because the
row's title covered three members and nobody measured its clauses apart. What
was genuinely absent is the QUESTION: every refusal this crate had was reached
by attempting the edit, so a hand could only find out by dropping.

# What this walk holds

  (A) with a wire picked, the screen publishes one row per card, each verdict
      one of two words, and a refusal carrying a sentence.
  (B) at least one card TAKES it — without this a screen refusing everything
      would satisfy (A) and (C) together.
  (C) ★★★★★ at least one card REFUSES it, and the sentence says WHAT is wrong
      rather than only that it refused.
  (D) ★★★★★ picking the wire up LIGHTS the cards that would take it, and the
      lit set is exactly the rule's — the paint is compared against the
      published verdict card by card, on the border, which is the property a
      pin's marking actually lives in.
  (E) ★★★★★ the verdict is SAID while the wire is still in the hand: hovering a
      refusing card during the drag puts its reason on the screen with the wire
      still where it was.
  (F) and the drop agrees with the question: released over a card the screen
      said would take it, the end moves; released over one it refused, the
      document is where it was.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1924_a_wire_says_where_its_end_may_go.py
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


def rewire(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/rewire"))


def links(app: RpcSubprocess, surface: str) -> str:
    return str(app.query(f"{surface}/links"))


def pin_edges(app: RpcSubprocess) -> dict[str, object]:
    """Every accept pin's tag and the BORDER it is drawn with.

    ★ R1919's lesson, applied: what changes when a pin is lit is its edge, and
    a walk that compared rectangles would see a screen that had lit nothing as
    identical to one that had lit everything.
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    out: dict[str, object] = {}
    for tag in abs_rects_of(snap):
        if tag.startswith("lab.pin.") and tag.endswith(".accept"):
            node = find_by_tag(snap, tag)
            out[tag] = (node or {}).get("style", {}).get("border")
    return out


def centre(app: RpcSubprocess, tag: str) -> tuple[float, float]:
    x, y, w, h = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))[tag]
    return (x + w / 2, y + h / 2)


def said(app: RpcSubprocess, surface: str) -> str:
    value = js(app.query(f"{surface}/said"))
    if not value:
        return ""
    return str(value.get("clause", value))


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

        banner("A — a picked wire publishes a verdict for every card")
        resting = pin_edges(app)
        picked = rewire(app, surface)
        ok(
            f"the opening canvas already has a wire picked — {picked['picked']!r}",
            picked["picked"] is not None,
        )
        ok(
            "and nothing is being carried yet, so what follows separates "
            "asking from dragging",
            picked["carried"] is False,
        )
        rows = {row["card"]: row for row in picked["cards"]}
        ok(f"one row per card — {sorted(rows)}", len(rows) >= 3)
        for name, row in sorted(rows.items()):
            ok(
                f"A: {name} answers with a known word — {row['verdict']!r}",
                # ⚠ R1930 added a FOURTH word, `grows` — a card with no free
                # pin that would make one — and this assertion caught it on the
                # first run of that round. The vocabulary is closed on purpose:
                # a screen that invented a word would answer something no reader
                # here knows how to act on.
                row["verdict"] in {"standing", "takes", "grows", "refuses"},
            )
            if row["verdict"] == "refuses":
                ok(
                    f"A: and its refusal carries a sentence — {row['because']!r}",
                    bool(row["because"]),
                )
            else:
                ok(
                    "A: ★ while a yes carries none — a reason beside a yes is a "
                    "reason nobody can act on",
                    row["because"] is None,
                )
        standing = [n for n, row in rows.items() if row["verdict"] == "standing"]
        ok(
            f"A: ★★★★★ exactly one card is the one it is ON, and that is a THIRD "
            f"word rather than a yes — {standing}",
            len(standing) == 1,
        )

        banner("B — at least one card TAKES it")
        # R1930 — either way of landing counts as "would take the wire" here:
        # this section is about a destination existing at all, and whether the
        # pin is already there is that round's question, not this one's.
        takers = sorted(
            n for n, r in rows.items() if r["verdict"] in ("takes", "grows")
        )
        ok(
            f"B: ★ {takers} would take the wire — without this a screen that "
            "refused everything would pass (A) and (C) at once",
            takers != [],
        )

        banner("C — ★★★★★ at least one REFUSES, and says what is wrong")
        refusers = sorted(n for n, r in rows.items() if r["verdict"] == "refuses")
        ok(f"C: ★ {refusers} would refuse it", refusers != [])
        # ★★★★★ The check a mutation demanded. Agreement between the question
        # and the act is not enough on its own: dropping a gate from BOTH keeps
        # them agreeing, and the walk went green with the screen's own
        # already-dials rule deleted. So the population is pinned too — this
        # canvas refuses for a SCREEN-level reason the crate cannot know, and
        # that is precisely the reason the first draft's question missed.
        reasons = [row["because"] for row in rows.values() if row["because"]]
        ok(
            f"C: ★★★★★ and one refusal is a rule of THIS SCREEN, not of the "
            f"model — {reasons}",
            any("already dials every endpoint" in why for why in reasons),
        )
        ok(
            "C: ★★★★★ while another is the MODEL's, so the question is asking "
            "both sides rather than one",
            any("cannot feed itself" in why or "cycle" in why for why in reasons),
        )
        for name in refusers:
            because = rows[name]["because"]
            ok(
                f"C: ★ {name}'s reason names WHAT is wrong, not that it failed "
                f"— {because!r}",
                any(
                    word in because
                    for word in (
                        "cannot feed itself",
                        "cycle",
                        "carries",
                        "names port",
                        "no accept pin",
                        # ★★★★★ A SCREEN-level rule, and the reason it is in
                        # this list: the first draft of this round asked only
                        # the crate, so the canvas said "P-02 will take it"
                        # and the drop then refused with this sentence. The
                        # question runs every gate the act runs now.
                        "already dials every endpoint",
                    )
                ),
            )
            # ★★★★★ R1924's own finding, kept as a check because a person reads
            # this string mid-drag: the first run of this walk read
            # `SelfLink(NodeId(4))` — the crate's `Display` had been formatting
            # the refusal with `{:?}`. A sentence with no reader is a sentence
            # nobody checks, so this walk is that reader.
            for spelling in ("NodeId(", "Socket {", "SelfLink", "WouldCycle"):
                ok(
                    f"C: ★★★★★ and it is a sentence, not Rust syntax — no "
                    f"{spelling!r} in {name}'s reason",
                    spelling not in because,
                )

        banner("D — ★★★★★ picking it up LIGHTS exactly what would take it")
        pin_of = {}
        for tag in resting:
            pin_of[tag.removeprefix("lab.pin.").removesuffix(".accept")] = tag
        held = pin_of[standing[0]]
        app.pointer_button("left", "down", path=held)
        app.tick_ms(16)
        carried = rewire(app, surface)
        ok(
            f"D: the wire is in the hand — carried={carried['carried']}",
            carried["carried"] is True,
        )
        lit = pin_edges(app)
        marked, unmarked = [], []
        for row in carried["cards"]:
            tag = pin_of.get(row["card"])
            if tag is None or tag not in lit or tag not in resting:
                continue
            changed = lit[tag] != resting[tag]
            ok(
                f"D: ★ {row['card']} is drawn {'lit' if changed else 'as it was'} "
                f"and the rule says {row['verdict']}",
                changed == (row["verdict"] in ("takes", "grows")),
            )
            (marked if changed else unmarked).append(row["card"])
        ok(
            f"D: ★★★★★ and BOTH sides are populated — lit {marked}, not lit "
            f"{unmarked}: a canvas that lit every pin, or none, would satisfy "
            "the agreement above vacuously",
            marked != [] and unmarked != [],
        )
        for row in carried["cards"]:
            ok(
                f"D: the screen's own `lit` agrees with its verdict for "
                f"{row['card']}",
                row["lit"] == (row["verdict"] in ("takes", "grows")),
            )

        banner("E — ★★★★★ the reason is SAID before the hand lets go")
        was = links(app, surface)
        refuser = refusers[0]
        app.hover(at=centre(app, pin_of[refuser]))
        app.tick_ms(16)
        heard = said(app, surface)
        ok(
            f"E: ★ passing over {refuser} says why it will not take it — {heard!r}",
            rows[refuser]["because"] in heard,
        )
        ok(
            "E: ★★★★★ and the document has not moved: the wire is still where "
            "it was while the reason is on the screen",
            links(app, surface) == was,
        )
        taker = takers[0]
        app.hover(at=centre(app, pin_of[taker]))
        app.tick_ms(16)
        # R1930 — the sentence depends on WHICH way the wire lands, and both
        # sentences are a yes: a pin that is there takes it, and a card with
        # none grows one. Asserted against the card's own published verdict
        # rather than against one of the two spellings, so the check cannot
        # drift from the rule the screen is deriving from.
        heard_over_taker = said(app, surface)
        expected = (
            "will take it" if rows[taker]["verdict"] == "takes" else "will grow a pin"
        )
        ok(
            f"E: ★ and passing over {taker} says so — {heard_over_taker!r}, and "
            f"its verdict is {rows[taker]['verdict']!r}",
            expected in heard_over_taker,
        )

        banner("F — the drop agrees with the question")
        app.pointer_button("left", "up", at=centre(app, pin_of[refuser]))
        app.tick_ms(16)
        ok(
            f"F: ★ dropped on {refuser}, which the screen had refused, the "
            "document is where it was",
            links(app, surface) == was,
        )
        ok(
            "F: and the lit pins went out with the gesture",
            pin_edges(app) == resting,
        )

        app.pointer_button("left", "down", path=held)
        app.tick_ms(16)
        app.pointer_button("left", "up", at=centre(app, pin_of[taker]))
        app.tick_ms(16)
        ok(
            f"F: ★★★★★ dropped on {taker}, which the screen had accepted, the "
            f"wire really moved — {links(app, surface)!r}",
            links(app, surface) != was,
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1924_a_wire_says_where_its_end_may_go", body))
