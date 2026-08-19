#!/usr/bin/env python3
"""R1733 §5.21 §5.51 §5.49 §2 #2 §2 #7 — **a palette hands the board a
footprint, and the board says where it would land.**

# What this exists for

The behaviour reference populates its dashboard by DRAGGING a row out of the
widget palette and dropping it on the canvas: the row is draggable, the canvas
takes `dragover` / `dragleave` / `drop`, and while a footprint is over it the
canvas raises a cell overlay, marks the destination cell and covers itself with
a dashed frame reading "drop to add widget". This build could only ever ADD from
a palette row — a click that appended at the bottom — which was the last
first-pass reproduction gap its GUI element census had open.

The reference's row carries BOTH: a drag start and a click that adds. So the
drag is an ADDITION here, never a replacement — the reference has zero keyboard
bindings, and moving its pointer-only gesture over instead of the click would
take the palette away from a reader who cannot drag. Section C is that
assertion, and it is the one worth the round.

★ And what it holds is ONE thing. The reference keeps a held card id and a held
palette kind as two nullable fields; measured in its script, the held-card field
is read by two guards and assigned a non-null value nowhere at all, because the
reorder gesture moved onto another field and the guards were left behind. Two
nullable fields can be set at once and that state has no meaning; `Carried` has
two arms and the compiler is what checks them.

# The floor, measured rather than remembered

Two probes built against 6.11.1 and run offscreen:

* across its grid container, its layout base, its widget class and its item
  view, **no member answers "where would a `w`-wide item land at this cell"**.
  The one name that matches at all is a bool on the item view saying whether to
  draw an indicator. So a preview of where a drop will go is the application's
  to invent, and the application's to keep in step with the commit.
* two items asked for the SAME cell of a grid container both get geometry and
  **overlap**; the add call returns `void`, so there is nothing to refuse with,
  and the position query answers only the first.
* a drag-move event there carries a **pixel**. Nothing on the event, the widget
  or the layout turns it into the cell a release would use.
* ★ the axis where the floor is ABOVE us, stated rather than hidden: a target
  there accepts a payload from a source it has never heard of, negotiated by
  format. What it cannot do is answer *which* kinds, per part, without running
  a drag: `acceptDrops` is one bool, a row's drop flag is one bool, the kinds
  are declared once for the whole model outside the meta-object, and the
  refusal is a bare bool with no reason.

# What it asserts

* **A** — the specification's three surfaces are painted, while a footprint is
  actually being carried, and the difference from the reference is exactly what
  `docs/analyzer-board-spec.json` records.
* **B** — ★★★★★ the cell the preview drew is the cell the release placed, and
  the wire can read both while the button is still down.
* **C** — ★★★★★ the click still adds at the bottom. The gesture was added, not
  substituted.
* **D** — a carry let go off the board places nothing where the cursor was.
* **E** — ★ an agent reaches the same cell: `add` takes one, refuses half of
  one, and clamps what it is given by the board's own rule.
* **F** — driven by the machine's own pointer, so the green is a real gesture's
  rather than a harness's.

Run from the workspace root (a real pointer needs a display):
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1733_a_palette_hands_the_board_a_footprint.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import board_spec, surfaces  # noqa: E402
from rpc_verify import (  # noqa: E402
    RealPointer,
    RealPointerUnavailable,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"

CHECKS: list[str] = []
REAL_POINTER_RUNS = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def layout(app: RpcSubprocess) -> dict:
    """The board's arrangement, as the wire publishes it."""
    return json.loads(q(app, "layout"))


def tiles(app: RpcSubprocess) -> dict:
    return {t["id"]: t for t in layout(app)["tiles"]}


def rows(app: RpcSubprocess) -> int:
    return max((t["row"] + t["h"] for t in layout(app)["tiles"]), default=0)


def centre(rect) -> tuple[float, float]:
    return (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)


def pointer(app: RpcSubprocess):
    global REAL_POINTER_RUNS
    try:
        driver = RealPointer(app)
    except RealPointerUnavailable as exc:
        print(f"[real-pointer] UNAVAILABLE — section F is not driven: {exc}")
        return None
    REAL_POINTER_RUNS += 1
    return driver


def a_palette_kind(app: RpcSubprocess) -> str:
    """A kind the palette places, taken from what the screen publishes.

    From the screen's own catalogue rather than named here: a kind written into
    a demo is one the catalogue can rename out from under, and a reserved row
    refuses to be picked up at all — by design, and by the same sentence its
    click refuses with.
    """
    placeable = [
        row["kind"] for row in q(app, "spec")["catalogue"] if row.get("tier") == "placeable"
    ]
    ok("the palette publishes the kinds it places", len(placeable) >= 3)
    return placeable[0]


# ── A: the surfaces the specification fixes ────────────────────────────────


def section_a(app: RpcSubprocess, spec: dict, kind: str) -> None:
    banner("A — what a carry puts on screen is what the specification declares")
    named = surfaces(spec)
    ok("the specification fixes three surfaces", len(named) == 3)
    ok(
        "and every one declares an ordered roster of named parts",
        all(
            [p["ordinal"] for p in spec[s]["canon"]] == list(range(1, len(spec[s]["canon"]) + 1))
            for s in named
        ),
    )

    rects = abs_rects_of(app.snapshot(source="paint"))
    for part in spec["palette_row"]["canon"]:
        tag = f"shell.palette.part.{part['key']}.{kind}"
        ok(f"A: the palette row paints its {part['key']}", tag in rects)
    lefts = [
        rects[f"shell.palette.part.{p['key']}.{kind}"][0] for p in spec["palette_row"]["canon"]
    ]
    ok("A: ★ and they run left to right in the specified order", lefts == sorted(lefts))

    ok(
        "A: nothing is carried before anything is picked up",
        q(app, "carrying") == "" and q(app, "drag") == "",
    )
    for part in spec["carry"]["canon"]:
        ok(
            f"A: and no {part['key']} is painted while nothing is carried",
            f"shell.carry.{part['key']}" not in rects,
        )

    # Pick the row up and hold it over the middle of the board.
    row = rects[f"shell.palette.{kind}"]
    board = rects["shell.canvas"] if "shell.canvas" in rects else None
    target = centre(board) if board else (600.0, 500.0)
    app.drag(from_at=centre(row), to_at=target, phase="begin")
    app.tick(16)

    held = abs_rects_of(app.snapshot(source="paint"))
    for part in spec["carry"]["canon"]:
        ok(
            f"A: ★ carrying a footprint paints the {part['key']} — {part['title']}",
            f"shell.carry.{part['key']}" in held,
        )
    for part in spec["slot"]["canon"]:
        ok(
            f"A: and the mark carries its {part['key']}",
            f"shell.carry.slot.{part['key']}" in held,
        )
    owed = [entry["key"] for entry in spec["carry"]["owed"]]
    ok(
        "A: ★★ the parts this build has and the reference does not are recorded "
        f"rather than deleted — {owed}",
        all(f"shell.carry.{key}" in held for key in owed),
    )
    app.drag(from_at=target, to_at=target, phase="end")
    app.tick(16)


# ── B: the preview is the placement ────────────────────────────────────────


def section_b(app: RpcSubprocess, kind: str) -> None:
    banner("B — ★★★★★ the cell the preview drew is the cell the release placed")
    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    before = set(tiles(app))
    bottom = rows(app)

    # Aim well to the RIGHT of centre, so a placement that ignored the pointer
    # and appended at column zero would be caught. The reference's own defect
    # class here (R1668) was a preview and a commit clamping differently.
    canvas = rects["shell.canvas"]
    aim = (canvas[0] + canvas[2] * 0.72, canvas[1] + canvas[3] * 0.30)
    app.drag(from_at=centre(row), to_at=aim, phase="begin")
    app.tick(16)

    carrying = q(app, "carrying")
    ok(
        f"B: ★ the wire says what is being carried and that it is not placed yet "
        f"— {carrying!r}",
        carrying.startswith("fresh:") and carrying.split(":", 1)[1].startswith(kind),
    )
    preview = q(app, "drag")
    ok(f"B: and where a release would put it — {preview!r}", preview.count(",") == 2)
    ident, col, rownum = preview.split(",")
    ok("B: the landing names the card the drop would create", ident.startswith(kind))
    ok("B: ★ and it is not column zero, so the pointer is what chose it", col != "0")

    held = abs_rects_of(app.snapshot(source="paint"))
    ok("B: the mark is painted at that moment", "shell.carry.slot" in held)
    ok(
        "B: ★ and the mark SAYS which cell, rather than leaving it to be counted",
        "shell.carry.slot.cell" in held,
    )

    app.drag(from_at=aim, to_at=aim, phase="end")
    app.tick(16)

    after = tiles(app)
    fresh = sorted(set(after) - before)
    assert_eq(len(fresh), 1, "B: a release over the board places exactly one card")
    placed = after[fresh[0]]
    assert_eq(fresh[0], ident, "B: and it is the card the landing named")
    assert_eq(
        (str(placed["col"]), str(placed["row"])),
        (col, rownum),
        "B: ★★★★★ the preview promised a cell and the release took THAT cell. "
        "The floor has no member that answers this question at all, so there "
        "the two are two computations",
    )
    ok("B: the board grew by a row or the card sits inside what was there", rows(app) >= bottom)
    assert_eq(q(app, "carrying"), "", "B: and nothing is left in hand")
    assert_eq(q(app, "drag"), "", "B: with no landing left over")


# ── C: the action survives the gesture ─────────────────────────────────────


def section_c(app: RpcSubprocess, kind: str) -> None:
    banner("C — ★★★★★ the click still adds at the bottom")
    bottom = rows(app)
    before = set(tiles(app))
    app.click(path=f"shell.palette.{kind}")
    app.tick(16)
    after = tiles(app)
    fresh = sorted(set(after) - before)
    assert_eq(len(fresh), 1, "C: a click on a palette row still adds a card")
    placed = after[fresh[0]]
    assert_eq(
        (placed["col"], placed["row"]),
        (0, bottom),
        "C: ★★★★★ at the BOTTOM of the board, which is where it went before this "
        "round. The reference is pointer-only; reproducing its drag instead of "
        "this would be a regression wearing a reproduction's clothes",
    )
    assert_eq(q(app, "carrying"), "", "C: and the press that armed a carry left none")
    ok(
        "C: ★ the palette's own line now tells a reader both ways are there — "
        "the reference's own wording, which was an instruction to do something "
        "this build could not do until this round",
        "Drag" in q(app, "spec")["palette"]["hint"],
    )


# ── D: let go off the board ────────────────────────────────────────────────


def section_d(app: RpcSubprocess, kind: str) -> None:
    banner("D — a carry let go off the board places nothing where the cursor was")
    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    canvas = rects["shell.canvas"]
    bottom = rows(app)
    before = set(tiles(app))

    aim = (canvas[0] + canvas[2] * 0.6, canvas[1] + canvas[3] * 0.5)
    app.drag(from_at=centre(row), to_at=aim, phase="begin")
    app.tick(16)
    over = q(app, "drag")
    ok(f"D: over the board there is a cell a release would use — {over!r}", over != "")

    # Carry it back out over the palette and let go there.
    app.drag(from_at=aim, to_at=centre(row), phase="move")
    app.tick(16)
    assert_eq(
        q(app, "drag"),
        "",
        "D: ★★ off the board there is no cell, which is a different answer from "
        "cell zero — the floor's board drag listens on the whole document, so a "
        "release over its palette commits",
    )
    ok("D: and something is still in hand", q(app, "carrying") != "")

    app.drag(from_at=centre(row), to_at=centre(row), phase="end")
    app.tick(16)
    after = tiles(app)
    fresh = sorted(set(after) - before)
    assert_eq(len(fresh), 1, "D: the release over the row acted as the row's click")
    assert_eq(
        (after[fresh[0]]["col"], after[fresh[0]]["row"]),
        (0, bottom),
        "D: ★ at the bottom, not at the cell the carry had been over",
    )


# ── E: the wire reaches the same cell ──────────────────────────────────────


def section_e(app: RpcSubprocess, kind: str) -> None:
    banner("E — ★ an agent reaches the cell a person's drag reaches")
    before = set(tiles(app))
    target_row = rows(app) + 1
    app.invoke(f"{EXT}/add", f"{kind},3,{target_row}")
    app.tick(16)
    after = tiles(app)
    fresh = sorted(set(after) - before)
    assert_eq(len(fresh), 1, "E: the wire places one card")
    assert_eq(
        (after[fresh[0]]["col"], after[fresh[0]]["row"]),
        (3, target_row),
        "E: at the cell it named — §2 #2, the headless path is the primary one "
        "and not a subset of what a hand can do",
    )

    refused = 0
    for call in (f"{kind},3", f"{kind},a,b", f"{kind},1,2,3"):
        try:
            app.invoke(f"{EXT}/add", call)
        except Exception:  # noqa: BLE001 - the refusal is the assertion
            refused += 1
    assert_eq(refused, 3, "E: ★ half a cell, a cell that is not a number and a third coordinate are all refused by name")

    # Past the right edge: a gesture, not an error, clamped by the board's rule.
    before = set(tiles(app))
    edge_row = rows(app) + 1
    app.invoke(f"{EXT}/add", f"{kind},11,{edge_row}")
    app.tick(16)
    after = tiles(app)
    fresh = sorted(set(after) - before)
    assert_eq(len(fresh), 1, "E: a column past the right edge still places")
    tile = after[fresh[0]]
    ok(
        "E: ★ and stops ON the board — the same clamp the pointer gets, because "
        "it is the same function",
        tile["col"] + tile["w"] <= layout(app)["columns"],
    )


# ── F: the machine's own pointer ───────────────────────────────────────────


def section_f(app: RpcSubprocess, kind: str) -> None:
    banner("F — the gesture, performed by the machine's own pointer")
    driver = pointer(app)
    if driver is None:
        return
    with driver as hand:
        rects = abs_rects_of(app.snapshot(source="paint"))
        row = rects[f"shell.palette.{kind}"]
        canvas = rects["shell.canvas"]
        before = set(tiles(app))

        hand.move(centre(row))
        hand.press()
        aim = (canvas[0] + canvas[2] * 0.45, canvas[1] + canvas[3] * 0.35)
        hand.move(aim)
        app.tick(16)
        held = q(app, "drag")
        ok(f"F: ★ a real press and a real move put a footprint in hand — {held!r}", held != "")
        painted = abs_rects_of(app.snapshot(source="paint"))
        ok("F: and the board marks where it would land", "shell.carry.slot" in painted)
        ok("F: and invites the release in words", "shell.carry.banner" in painted)

        hand.release()
        app.tick(16)
        after = tiles(app)
        fresh = sorted(set(after) - before)
        assert_eq(len(fresh), 1, "F: a real release places one card")
        ident, col, rownum = held.split(",")
        assert_eq(
            (fresh[0], str(after[fresh[0]]["col"]), str(after[fresh[0]]["row"])),
            (ident, col, rownum),
            "F: ★★★★★ and a REAL gesture lands where its preview said — the "
            "harness and the hand agree, which is the only way this green means "
            "what it says",
        )


def body() -> None:
    spec = board_spec()
    with RpcSubprocess(SHELL, boot_grace=1.5, visible_window=True) as app:
        kind = a_palette_kind(app)
        section_a(app, spec, kind)
        section_b(app, kind)
        section_c(app, kind)
        section_d(app, kind)
        section_e(app, kind)
        section_f(app, kind)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {REAL_POINTER_RUNS} real-pointer session(s) contributed to "
        f"this run; {len(CHECKS)} named check(s) plus the assert_eq comparisons above."
    )
    if REAL_POINTER_RUNS == 0:
        print(
            "[coverage] ⚠ section F's gesture did NOT run on this host. The run "
            "is shorter than it looks and this line is the only evidence."
        )


if __name__ == "__main__":
    run_demo("R1733 a palette hands the board a footprint", body)
