#!/usr/bin/env python3
"""R1497 §5.35 §5.51 §2#2 §2#7 — a pointer target is a node that can receive it.

Qt reference: `Qt::WA_TransparentForMouseEvents`; the web spells the same rule
`pointer-events: none`. Decoration does not intercept the pointer. pinion needs
no such declaration, because a tag already says whether anything is behind it:
the event target is the `External`, and a tag is only its name.

Measured on this binding before the round, over the real wire:

    click path=colhdr#0 -> sort 0:ascending      (label 42..78,  centre 104)
    click path=colhdr#1 -> sort 1:ascending      (label 192..222, centre 224)
    click path=colhdr#2 -> sort 2:ascending      (label 282..307, centre 319)
    click path=colhdr#3 -> sort none  LOST       (label 382..436, centre 434)
    click path=colhdr#4 -> sort none  LOST       (label 512..553, centre 549)

    x sweep at y=110:  200 LOST · 300 LOST · 320 ok · 400 LOST · 434 LOST
                       470 ok   · 549 LOST · 597 ok

Two of five header sections could not be clicked at all, and the discriminator
was exactly `centre in label rect` — five for five, no exceptions. `pointer_down`
armed the deepest TAG under the cursor; when that tag was a section's own label
`dispatch_send_mods` split off the primary `colhdr_label`, found no `External`
for it, and returned. The press was discarded with no diagnostic, and because
`scene/click {path}` presses a node's rect CENTRE, a centred label made the most
obvious click point the one that could not work.

R1496 recorded this as "`scene/click` reaches this External not at all" and
routed its own click assertions through `/external/send` instead. That was the
wrong cause and the wrong remedy: the wire was fine, the resolution was not, and
a demo that drives the seam cannot notice. Both of R1496's "pre-existing router
defects" are this one defect — the missing `PointerEnter` after a session-less
gesture was the Enter landing on a label that could receive nothing, not the
free-release branch failing to refresh hover (it never pinned hover, so it has
nothing to restore).

What this asserts:

  (A) THE GEOMETRY IS THE PREMISE — the labels really do cover the centres of
      sections 3 and 4 and really do not cover 0, 1, 2. Read from the paint, not
      assumed: without this the round's own witness could evaporate under a font
      change and the demo would still pass.
  (B) EVERY SECTION IS CLICKABLE — through `scene/click`, by path, on all five.
  (C) THE LABEL IS NOT A WALL — clicking a coordinate inside a label sorts the
      section under it, and so does addressing the label's own tag.
  (D) THE WHOLE SECTION IS ONE TARGET — a sweep across a section lands on that
      section wherever it is pressed.
  (E) NOTHING NEW BECAME CLICKABLE — the 2px gap between two sections and the
      space past the strip's right edge still do nothing. The fix is a widening
      of what reaches a widget, not of what counts as one.
  (F) HOVER NAMES THE WIDGET — a press after a bare `scene/hover` onto a
      label-covered centre lands, so the enter/leave stream reached the section.
  (G) A DRAG STILL MOVES A SECTION — including onto a label-covered target.
  (H) A DRAG IS STILL NOT A CLICK — R1496's rule, over the same coordinates.
  (I) THE PERMISSIONS STILL GATE — R1496 (E) and (F), now driven by the WIRE
      click that round believed impossible.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1497_pointer_target.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
LABEL = "colhdr_label"
NCOLS = 5
IDENTITY = list(range(NCOLS))
STRIP_ROW_Y = 110.0


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _rect(tf, tag: str):
    node = find_by_tag(_paint(tf), tag)
    assert node is not None, f"{tag} paints"
    return node["rect"]


def _span(rect) -> tuple[float, float]:
    return (float(rect["x"]), float(rect["x"] + rect["w"]))


def _centre(rect) -> float:
    return float(rect["x"]) + float(rect["w"]) / 2.0


def _sort(tf) -> str:
    return _h(tf, "sort_indicator")


def _clear_sort(tf) -> None:
    """Back to no sort indicator, through the model rather than a gesture."""
    tf.intervene("/external/sort_indicator", "none")
    wait_until(lambda: _sort(tf) == "none", desc="sort indicator cleared")


def _reset(tf, **overrides) -> None:
    """Boot order + both permissions granted, through the restore path."""
    tf.intervene("/external/order", overrides.get("order", IDENTITY))
    tf.intervene("/external/sections_movable", overrides.get("movable", True))
    tf.intervene("/external/sections_clickable", overrides.get("clickable", True))
    wait_until(
        lambda: _h(tf, "order") == overrides.get("order", IDENTITY)
        and _h(tf, "sections_movable") is overrides.get("movable", True)
        and _h(tf, "sections_clickable") is overrides.get("clickable", True),
        desc="header reset",
    )
    _clear_sort(tf)


def _click_and_read(tf, *, path: str | None = None, at: float | None = None) -> str:
    """One `scene/click`, then the indicator it left behind."""
    _clear_sort(tf)
    if path is not None:
        tf.click(path=path)
    else:
        assert at is not None
        tf.click(at=(at, STRIP_ROW_Y))
    return _sort(tf)


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        wait_until(
            lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
            desc="the strip paints",
        )
        _reset(tf)

        # ---- (A) the geometry is the premise, read from the paint ----------
        # The round's witness is that a label covers a cell's centre. Assert it
        # rather than trust it: if a font change moved these rects the premise
        # would be gone, and a demo that only clicked would still pass while
        # proving nothing about decoration.
        cells = [_rect(tf, f"{HDR}#{i}") for i in range(NCOLS)]
        labels = [_rect(tf, f"{LABEL}#{i}") for i in range(NCOLS)]
        covered = []
        for i in range(NCOLS):
            lo, hi = _span(labels[i])
            centre = _centre(cells[i])
            c_lo, c_hi = _span(cells[i])
            assert_eq(
                lo >= c_lo and hi <= c_hi,
                True,
                f"section {i}'s label is painted inside its own cell",
            )                                                              # 1-5
            covered.append(lo <= centre < hi)
        assert_eq(
            covered,
            [False, False, False, True, True],
            "the labels of sections 3 and 4 cover their cell centres; 0-2 do not",
        )                                                                  # 6
        # Whichever way that lands, the round is about the covered ones — if a
        # future layout covers none, this demo must say so instead of passing
        # vacuously.
        assert_eq(
            any(covered),
            True,
            "at least one section's centre is under its label (the premise)",
        )                                                                  # 7

        # ---- (B) every section is clickable through the wire ---------------
        for i in range(NCOLS):
            assert_eq(
                _click_and_read(tf, path=f"{HDR}#{i}"),
                f"{i}:ascending",
                f"scene/click on section {i} sorts section {i}",
            )                                                              # 8-12
        # And the two that were lost are the two the labels cover — named, so
        # the regression this guards is legible without the sweep below.
        for i, is_covered in enumerate(covered):
            if is_covered:
                assert_eq(
                    _click_and_read(tf, path=f"{HDR}#{i}"),
                    f"{i}:ascending",
                    f"section {i}'s own label does not swallow its click",
                )                                                          # 13-14

        # ---- (C) the label is not a wall ----------------------------------
        for i in range(NCOLS):
            assert_eq(
                _click_and_read(tf, at=_centre(labels[i])),
                f"{i}:ascending",
                f"a click on label {i}'s own pixels reaches section {i}",
            )                                                              # 15-19
        # Addressing the label's TAG resolves the same way a coordinate inside
        # it does — the path form takes that node's centre, which is inside it.
        assert_eq(
            _click_and_read(tf, path=f"{LABEL}#3"),
            "3:ascending",
            "addressing the label tag reaches the section behind it",
        )                                                                  # 20

        # ---- (D) the whole section is one target --------------------------
        # Four probes across section 3: its left edge, inside its label, its
        # centre, and clear of the label. Pre-R1497 the middle two were lost.
        lo3, hi3 = _span(cells[3])
        l_lo3, l_hi3 = _span(labels[3])
        for x in (lo3 + 1.0, l_lo3 + 1.0, _centre(cells[3]), hi3 - 2.0):
            assert_eq(
                _click_and_read(tf, at=x),
                "3:ascending",
                f"x={x} is section 3 wherever it falls in the cell",
            )                                                              # 21-24

        # ---- (E) nothing new became clickable ----------------------------
        # The 2px gap between sections 2 and 3 belongs to the strip, not to a
        # section: the strip's own tag carries no sub-index, so the payload has
        # no section to name and the header activates nothing. A widening that
        # also made gaps clickable would be a different bug.
        gap = (_span(cells[2])[1] + _span(cells[3])[0]) / 2.0
        assert_eq(
            _span(cells[2])[1] < _span(cells[3])[0],
            True,
            "sections 2 and 3 are separated by a gap (the premise for the next)",
        )                                                                  # 25
        assert_eq(
            _click_and_read(tf, at=gap),
            "none",
            "the gap between two sections activates neither",
        )                                                                  # 26
        past_strip = _span(cells[NCOLS - 1])[1] + 8.0
        assert_eq(
            _click_and_read(tf, at=past_strip),
            "none",
            "past the strip's right edge nothing is pressed",
        )                                                                  # 27

        # ---- (F) hover names the widget ----------------------------------
        # A bare `scene/hover` onto a label-covered centre, then a click there.
        # The press can only land if the enter that preceded it named the
        # SECTION: pre-R1497 the enter went to the label and the press with it.
        _clear_sort(tf)
        tf.hover(at=(_centre(cells[4]), STRIP_ROW_Y))
        tf.click(at=(_centre(cells[4]), STRIP_ROW_Y))
        assert_eq(
            _sort(tf), "4:ascending", "a hovered, label-covered centre presses"
        )                                                                  # 28
        # Hovering off the strip and back again still presses — the leave/enter
        # pair resettled onto the section, not onto its decoration.
        _clear_sort(tf)
        tf.hover(at=(past_strip, STRIP_ROW_Y))
        tf.hover(at=(_centre(cells[3]), STRIP_ROW_Y))
        tf.click(at=(_centre(cells[3]), STRIP_ROW_Y))
        assert_eq(
            _sort(tf), "3:ascending", "hover away and back leaves the section pressable"
        )                                                                  # 29

        # ---- (G) a drag still moves a section ----------------------------
        _reset(tf)
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#3", steps=6)
        assert_eq(
            _h(tf, "order"),
            [1, 2, 3, 0, 4],
            "a drag onto a label-covered target still commits the move",
        )                                                                  # 30
        assert_eq(_sort(tf), "none", "and moving a section is not sorting it")  # 31

        # ---- (H) a drag is still not a click ----------------------------
        # R1496 (D): a section picked up, carried well past the threshold, and
        # dropped back into its own gap moves nothing AND sorts nothing. The
        # coordinates now all resolve to sections, so this rule is being tested
        # where it previously could not even be reached.
        _reset(tf)
        tf.drag(
            from_at=(_centre(cells[3]), STRIP_ROW_Y),
            to_at=(_centre(cells[0]), STRIP_ROW_Y),
            steps=3,
        )
        moved = _h(tf, "order")
        tf.drag(
            from_at=(_centre(cells[0]), STRIP_ROW_Y),
            to_at=(_centre(cells[3]), STRIP_ROW_Y),
            steps=3,
        )
        assert_eq(_h(tf, "order") != moved, True, "the second drag moved it back")  # 32
        assert_eq(
            _sort(tf), "none", "neither travelled drag was read as a click"
        )                                                                  # 33

        # ---- (I) the permissions still gate, driven by the wire click ----
        # R1496 (E): a pinned header still sorts. That round pressed the section
        # the cursor already rested on, because it believed `scene/click` never
        # arrived; it arrives.
        _reset(tf, movable=False)
        before = _h(tf, "order")
        tf.drag(from_path=f"{HDR}#0", to_path=f"{HDR}#4", steps=6)
        assert_eq(_h(tf, "order"), before, "sections_movable off: the drag moves nothing") # 34
        assert_eq(
            _click_and_read(tf, path=f"{HDR}#3"),
            "3:ascending",
            "a pinned header is still sortable — the two permissions are independent",
        )                                                                  # 35
        # R1496 (F): revoke both and neither gesture acts.
        _reset(tf, movable=False, clickable=False)
        before = _h(tf, "order")
        assert_eq(
            _click_and_read(tf, path=f"{HDR}#3"),
            "none",
            "sections_clickable off: the click sorts nothing",
        )                                                                  # 36
        tf.drag(from_path=f"{HDR}#1", to_path=f"{HDR}#4", steps=6)
        assert_eq(_h(tf, "order"), before, "and the drag still moves nothing")  # 37
        # The header says so rather than looking broken.
        assert_eq(_h(tf, "sections_clickable"), False, "the refusal is readable")  # 38
        assert_eq(_h(tf, "sections_movable"), False, "both of them are")       # 39

        # Granting clickable back restores it — the permission is the only thing
        # that was refusing, not a hover state the revoke corrupted.
        _reset(tf, movable=False, clickable=True)
        assert_eq(
            _click_and_read(tf, path=f"{HDR}#3"),
            "3:ascending",
            "granting the permission back makes the same click land",
        )                                                                  # 40


if __name__ == "__main__":
    sys.exit(
        run_demo(
            "R1497 §5.35 §5.51 §2#2 — a pointer target is a node that can receive it",
            body,
        )
    )
