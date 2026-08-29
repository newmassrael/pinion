#!/usr/bin/env python3
"""R1904 §5.36 §5.11 §2 #7 — **a byte sits in the middle of the cell that
lights it, and the wire can say so.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This one repays
`debt-text-is-not-centred-in-the-box-that-holds-it` at the place a person
reported it from a running window:

> the bytes on the right of the decode inspector are not properly centred
> inside their pink rectangles

Measured off the rendered page at 1440x900: a 22-wide cell, a 10-wide glyph
pair, margins of **3 and 9** where centred is 6 and 6.

# ★★★★★ Why this walk joins TWO surfaces, and why that is the whole point

Every rectangle on this screen was already right. The band is centred in the
cell — two and two in twenty-two — and the run's box IS the band. What nothing
published was where the GLYPHS were inside that band, and they were flush left,
because a run with no declared alignment starts where its box starts.

⇒ **a box centred in a box centred in a box, with the ink at the far left of
all three.** A client reading `scene/snapshot` alone cannot see it, however
carefully it looks; every number it has agrees.

So this walk reads `scene/snapshot` for WHAT IS DRAWN and `scene/text_blocks`
for WHERE THE SHAPER PUT IT, and joins them by containment — no new tag on the
run, because the cell already has one and a second rectangle for one object is
a defect this screen has paid for before. That join is the capability being
demonstrated: an agent driving this tool headlessly can now detect a class of
visual defect that rectangles are blind to, which is what §2 #7 (scene as data,
queryable as text) has to mean if it means anything.

# Superior to the floor

The floor toolkit's introspection answers an item's rectangle and the alignment
that was REQUESTED. Neither says where the glyphs landed, so "asked for centre"
and "is centred" are the same answer there and different answers here — and it
is the difference that a person reading a window reports.

# What this walk holds

  (A) the assembled tool paints the decode inspector's byte cells, and every
      one of their rectangles is exactly centred in its column — the chain a
      client can check today, which said nothing was wrong.
  (B) `scene/text_blocks` answers for the SAME cells, one paragraph each,
      each declaring `center`: the surface that can see what (A) cannot.
  (C) the paragraph had ROOM to move, asserted rather than hoped — without it
      every claim below would hold for a run that could not have moved.
  (D) the glyphs landed at half that room: centred inside the box the run was
      given.
  (E) and centred inside the CELL a person is looking at, which is the sentence
      that was false. Margins from the ink, not from any rectangle.
  (F) at a SECOND window size, because a sweep is only complete for the size it
      measured and this defect was reported from a window nobody's gate ran at.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1904_a_byte_sits_in_the_middle_of_its_cell.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    resize_and_settle,
    run_demo,
)

SHELL = "hello-analyzer-shell"

# The decode inspector's byte cells, as the screen tags them. A pattern rather
# than a list: the population is the specification's and a walk that wrote it
# down would be a second copy to drift.
CELL = re.compile(r"^card\.decode#\d+\.byte\.\d+$")

# An odd amount of room cannot be split evenly, so one pixel is the floor a rule
# can demand. The defect reported was six.
TOLERANCE = 1.0

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def cells_of(snap) -> dict[str, tuple[int, int, int, int]]:
    """Every byte cell the assembled tool painted, by tag."""
    return {tag: rect for tag, rect in abs_rects_of(snap).items() if CELL.match(tag)}


def blocks_of(app: RpcSubprocess) -> list[dict]:
    """Every paragraph the shaper laid out, with where each line landed."""
    resp = app.request("scene/text_blocks", {})
    assert resp is not None and resp.result is not None, "scene/text_blocks must answer"
    return resp.result["blocks"]


def inked(line: dict) -> float:
    """The width alignment itself uses: the advance less its trailing space."""
    return line["advance"] - line["trailing_whitespace"]


def paragraph_in(blocks: list[dict], rect: tuple[int, int, int, int]) -> list[dict]:
    """The paragraphs whose box sits inside `rect`.

    The join is CONTAINMENT rather than a shared tag, deliberately: the cell
    carries the tag and the run inside it does not, and giving the run one would
    make two addressable rectangles for one thing a person points at.
    """
    x, y, w, h = rect
    return [
        b
        for b in blocks
        if b["x"] >= x and b["y"] >= y and b["x"] + b["width"] <= x + w and b["y"] + b["height"] <= y + h
    ]


def measure(app: RpcSubprocess, size_label: str) -> list[tuple[str, float, float, float]]:
    """For each byte cell: its tag, and the ink's left margin, right margin and
    the room the run had. Everything below is stated in those three numbers."""
    snap = app.snapshot(source="paint")
    cells = cells_of(snap)
    blocks = blocks_of(app)
    out = []
    for tag, rect in sorted(cells.items()):
        held = paragraph_in(blocks, rect)
        assert len(held) == 1, f"{size_label}: {tag} holds {len(held)} paragraph(s), not one"
        block = held[0]
        line = block["lines"][0]
        ink_w = inked(line)
        ink_x = block["x"] + line["x"]
        left = ink_x - rect[0]
        right = (rect[0] + rect[2]) - (ink_x + ink_w)
        out.append((tag, left, right, block["width"] - ink_w))
    return out


def section_a(app: RpcSubprocess) -> dict:
    banner("A — the cells are painted, and every rectangle is already right")
    snap = app.snapshot(source="paint")
    cells = cells_of(snap)
    ok(f"A: ★★ the assembled tool paints the decode inspector's byte cells — {len(cells)}", cells)
    widths = {rect[2] for rect in cells.values()}
    ok(f"A: ★ each is one width, which the specification gives it — {widths}", len(widths) == 1)

    # The chain a client can inspect today. It agrees with itself at every link,
    # which is exactly why the reported defect survived every gate this screen
    # carries: nothing in it is about ink.
    blocks = blocks_of(app)
    boxes = [paragraph_in(blocks, rect)[0] for rect in sorted(cells.values())]
    off = [
        (b["x"] - rect[0]) - ((rect[0] + rect[2]) - (b["x"] + b["width"]))
        for b, rect in zip(boxes, sorted(cells.values()), strict=True)
    ]
    ok(
        f"A: ★★★★★ every run's BOX is centred in its cell — offsets {sorted(set(off))} — "
        "so a client reading rectangles alone is told this screen is correct",
        all(abs(d) <= TOLERANCE for d in off),
    )
    return {"cells": cells, "blocks": blocks}


def section_b(app: RpcSubprocess, rest: dict) -> None:
    banner("B — a second surface answers what the rectangles cannot")
    ok(f"B: ★★ `scene/text_blocks` answers for this tool — {len(rest['blocks'])} paragraph(s)", rest["blocks"])
    held = [paragraph_in(rest["blocks"], rect) for rect in rest["cells"].values()]
    ok(
        f"B: ★ one paragraph per cell — {sorted({len(h) for h in held})}",
        all(len(h) == 1 for h in held),
    )
    # `Center`, in the spelling `scene/snapshot` uses — this surface's own
    # documentation says the two agree, and a walk that invented a spelling
    # would be asserting against its own guess.
    aligns = {h[0]["align"] for h in held}
    ok(
        f"B: ★★★★★ each DECLARES centring — {aligns}. Until R1904 this surface "
        "could not have been asked: a run whose only declaration is an "
        "alignment became a paragraph at R1780, and these runs declared none",
        aligns == {"Center"},
    )
    ok(
        "B: ★ and each is one line, so a line's landing is the run's landing",
        all(len(h[0]["lines"]) == 1 for h in held),
    )


def section_c(app: RpcSubprocess) -> list:
    banner("C — the premise: the run had room to move")
    seen = measure(app, "at rest")
    room = sorted({round(spare) for _, _, _, spare in seen})
    ok(
        f"C: ★★★★★ every run's box is WIDER than its ink, by {room}px — without "
        "this every claim below would hold for a run that could not have moved, "
        "which is the vacuous pass an 84-round-old debt turned out to be",
        all(spare >= 2.0 for _, _, _, spare in seen),
    )
    return seen


def section_d(app: RpcSubprocess, seen: list) -> None:
    banner("D — the glyphs landed at half that room")
    blocks = blocks_of(app)
    cells = cells_of(app.snapshot(source="paint"))
    wrong = []
    for rect in cells.values():
        block = paragraph_in(blocks, rect)[0]
        line = block["lines"][0]
        want = (block["width"] - inked(line)) / 2.0
        if abs(line["x"] - want) > TOLERANCE:
            wrong.append((line["x"], want))
    ok(
        f"D: ★★★★★ each line sits half its slack in, inside the box the run was "
        f"given — {len(wrong)} of {len(cells)} elsewhere",
        not wrong,
    )
    ok(
        "D: ★ and by a POSITIVE amount, so this is centring rather than a "
        f"tolerance wide enough to swallow flush — {sorted({round(left) for _, left, _, _ in seen})}px in",
        all(left > 0 for _, left, _, _ in seen),
    )


def section_e(app: RpcSubprocess, seen: list) -> None:
    banner("E — and centred in the CELL a person is looking at")
    off = sorted({round(left - right) for _, left, right, _ in seen})
    ok(
        f"E: ★★★★★ the two margins agree — offsets {off}px, where what was "
        "reported was 3 against 9. Measured from the INK, which is the only "
        "thing that could have said so",
        all(abs(left - right) <= TOLERANCE for _, left, right, _ in seen),
    )
    ok(
        f"E: ★ across every cell, not on average — {len(seen)} of them",
        len(seen) >= 8,
    )


def window_of(snap) -> tuple[int, int]:
    rect = snap.get("rect", {}) if isinstance(snap, dict) else {}
    return (rect.get("w"), rect.get("h"))


def section_f(app: RpcSubprocess) -> None:
    banner("F — and at a window size no gate of ours runs at")
    # The report came from 1440x900, and so does every in-process gate here. A
    # sweep is only complete for the size it measured, so the claim is re-made
    # somewhere else entirely.
    #
    # ⚠ LARGER, and the change is ASSERTED. The first draft asked for 1180x760,
    # which is under this window's declared floor: the resize was refused, the
    # harness granted 1440x900, and the section re-measured the size it had just
    # measured while reading as though it had generalised. A section whose whole
    # value is "somewhere else" has to prove it went there.
    before = window_of(app.snapshot(source="paint"))
    after = window_of(resize_and_settle(app, (1760, 1020)))
    ok(
        f"F: ★★★★★ the window really is a different size — {before} -> {after}. "
        "Asked because a refused resize leaves this section measuring the size "
        "it was written to escape, and saying nothing about it",
        after != before and None not in after,
    )
    seen = measure(app, f"{after[0]}x{after[1]}")
    ok(f"F: ★★ the cells are still painted after a real resize — {len(seen)}", seen)
    ok(
        f"F: ★★★★★ and still centred — offsets "
        f"{sorted({round(left - right) for _, left, right, _ in seen})}px",
        all(abs(left - right) <= TOLERANCE for _, left, right, _ in seen),
    )
    ok(
        "F: ★ with the room re-measured there rather than carried from the "
        f"first size — {sorted({round(spare) for _, _, _, spare in seen})}px",
        all(spare >= 2.0 for _, _, _, spare in seen),
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        rest = section_a(app)
        section_b(app, rest)
        seen = section_c(app)
        section_d(app, seen)
        section_e(app, seen)
        section_f(app)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1904 a byte sits in the middle of its cell", body)
