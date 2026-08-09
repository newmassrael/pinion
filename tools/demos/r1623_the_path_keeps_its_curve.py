#!/usr/bin/env python3
"""R1623 §5.3 §2 #7 — a path keeps the curve it was authored as.

`Scene::Path` spoke four commands — moveto, lineto, cubic, close — and its own
doc recorded the rest as carry-forward. So SVG path data, which is how every
icon anyone ships is actually written, could not be imported: an application
converted it outside the framework and arrived with the arcs already gone.

R1623 adds the two commands that are geometry rather than shorthand (`QuadTo`,
`ArcTo`) and a `d` parser, and the design decision this demo exists to prove is
that they SURVIVE. The reference toolkit converts on the way in — its
quadratic builder computes the equivalent cubic and calls the cubic builder,
its arc builder appends Béziers — so a path there cannot be asked whether it
holds a circle. Under §2 #7 the
scene is what a client reads, so an answer that has already been expanded into
a rasterizer's Béziers is an answer to a different question.

What each check discriminates:

* **The arc is on the wire with every argument.** Not "a path is present": the
  radii, the rotation, BOTH flags and the endpoint. A serializer that dropped
  the flags would still emit an `ArcTo` and would still look right in a
  picture, because this desk's icon happens to sweep the way the default does.
* **The quadratic has ONE control point.** This is what separates "kept" from
  "elevated on the way in" — an elevated quadratic is a `CurveTo` with `c1`
  and `c2`, and it draws the identical curve.
* **The wire says no command is `Unknown`.** That arm existed until this round
  and called itself forward-compatible.
* **Malformed data is REFUSED by the byte.** The reference answers one bit for
  this, so an application shows an empty area and cannot say whether the data
  was wrong, the file was missing, or the ink matched the background. Toggling
  the subject must replace the path with a message naming the offset.
* **The icon is painted, not merely described.** Rects come from `source=paint`
  after layout, so a `d` string that parsed but reached no rasterizer fails.

Run from the workspace root:
    cargo build -p hello-svg-path --release
    python3 tools/demos/r1623_the_path_keeps_its_curve.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
)

EXAMPLE = "hello-svg-path"
WIN = (420, 300)
ICONS = ("icon_clock", "icon_wave", "icon_bookmark")


def commands_of(snap, tag: str) -> list[dict]:
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} is in the scene"
    cmds = node.get("commands")
    assert isinstance(cmds, list), f"{tag} publishes its command stream: {node}"
    return cmds


def kinds(cmds: list[dict]) -> list[str]:
    return [c.get("type") for c in cmds]


def run(tf: RpcSubprocess) -> None:
    snap = tf.snapshot(source="paint", viewport=WIN)
    rects = abs_rects_of(snap)

    # ── 1. every icon reached the paint pass with real extent ────────────
    for tag in ICONS + ("subject",):
        assert tag in rects, f"{tag} is painted, not just parsed: {sorted(rects)}"
        _x, _y, w, h = rects[tag]
        assert w > 0 and h > 0, f"{tag} has extent: {rects[tag]}"
    print(f"[demo] icons painted: {[t for t in ICONS] + ['subject']}")

    # ── 2. no command anywhere is Unknown ────────────────────────────────
    #      The arm this round removed. It applied to EVERY path node, so the
    #      census is over all of them rather than the one under test.
    for tag in ICONS + ("subject",):
        seen = kinds(commands_of(snap, tag))
        assert_eq(
            [k for k in seen if k == "Unknown"],
            [],
            f"{tag} publishes what each command IS",
        )
    print("[demo] no command collapsed to Unknown")

    # ── 3. the clock kept its arcs, and every argument is on the wire ────
    clock = commands_of(snap, "icon_clock")
    arcs = [c for c in clock if c.get("type") == "ArcTo"]
    assert_eq(len(arcs), 2, "a full circle is two arcs, and both survived")
    assert_eq(
        "CurveTo" in kinds(clock),
        False,
        "nothing in the clock was authored as a cubic, so nothing was expanded "
        f"into one on the way in: {kinds(clock)}",
    )
    for i, arc in enumerate(arcs):
        for field in ("rx", "ry", "x_rotation", "large_arc", "sweep", "end"):
            assert field in arc, f"arc {i} publishes {field}: {arc}"
        assert isinstance(arc["large_arc"], bool), f"a flag is a flag: {arc}"
        assert isinstance(arc["sweep"], bool), f"a flag is a flag: {arc}"
        assert arc["rx"] > 0 and arc["ry"] > 0, f"arc {i} radii: {arc}"
        assert {"x", "y"} <= set(arc["end"]), f"arc {i} endpoint: {arc}"
    # The two dial arcs differ in where they end and agree on the sweep —
    # which is what makes them one circle rather than two lens shapes.
    assert_eq(
        arcs[0]["sweep"],
        arcs[1]["sweep"],
        "both halves of the dial sweep the same way",
    )
    assert arcs[0]["end"] != arcs[1]["end"], f"and end at opposite points: {arcs}"
    assert_eq(arcs[0]["large_arc"], True, "each half is the long way round")
    print(f"[demo] clock arcs on the wire: {arcs[0]}")

    # ── 4. the wave kept its quadratics — ONE control point, not two ─────
    wave = commands_of(snap, "icon_wave")
    assert_eq(
        kinds(wave),
        ["MoveTo", "QuadTo", "QuadTo"],
        "the smooth `T` is a quadratic whose control point is determined",
    )
    for q in wave[1:]:
        assert_eq(sorted(q.keys()), ["c", "end", "type"], "a quadratic's arguments")
        assert "c1" not in q and "c2" not in q, f"not elevated to a cubic: {q}"
    print(f"[demo] wave commands: {kinds(wave)}")

    # ── 5. the bookmark's relative and shorthand spellings are resolved ──
    bm = commands_of(snap, "icon_bookmark")
    assert_eq(
        kinds(bm),
        ["MoveTo", "LineTo", "LineTo", "LineTo", "LineTo", "Close"],
        "h / v / the implicit repeat are linetos; none of them is a curve",
    )
    xs = [c["point"]["x"] for c in bm if "point" in c]
    ys = [c["point"]["y"] for c in bm if "point" in c]
    assert min(xs) >= -0.01 and min(ys) >= -0.01, f"fitted inside its box: {bm}"
    print(f"[demo] bookmark resolved to {kinds(bm)}")

    # ── 6. the subject round-trips, and the echo says so ─────────────────
    subject = commands_of(snap, "subject")
    assert_eq(kinds(subject), ["MoveTo", "LineTo", "ArcTo", "Close"], "the sector")
    body = texts_of(snap)
    echo = [t for t in body if t.startswith("subject parsed")]
    assert len(echo) == 1, f"the round trip is on screen: {body}"
    assert " A " in echo[0], f"an A went in and an A came back out: {echo[0]}"
    print(f"[demo] round trip: {echo[0]}")

    # ── 7. NEGATIVE CONTROL: malformed data is refused BY THE BYTE ───────
    #      Not "the icon disappears" — a toolkit that drew nothing would also
    #      pass that. The message must name the offset, the command and the
    #      cause, which is the whole difference from an empty area.
    tf.click(path="main_toggle")
    tf.tick(0.016)
    bad = tf.snapshot(source="paint", viewport=WIN)
    assert_eq(
        find_by_tag(bad, "subject"),
        None,
        "data that did not parse is not painted as a guess",
    )
    said = texts_of(bad)
    refusal = [t for t in said if "REFUSED" in t]
    assert len(refusal) == 1, f"the refusal is on screen: {said}"
    msg = refusal[0]
    for expected in ("byte 25", "'A'", "flag", "'3'"):
        assert expected in msg, f"the refusal names {expected}: {msg}"
    print(f"[demo] refusal: {msg}")

    # ── 8. the other icons are untouched by the subject's failure ────────
    #      A parser that failed the whole document would take them too.
    bad_rects = abs_rects_of(bad)
    for tag in ICONS:
        assert_eq(bad_rects.get(tag), rects[tag], f"{tag} is unaffected")

    # ── 9. and it comes back ─────────────────────────────────────────────
    tf.click(path="main_toggle")
    tf.tick(0.016)
    again = tf.snapshot(source="paint", viewport=WIN)
    assert find_by_tag(again, "subject") is not None, "valid data draws again"
    assert_eq(
        kinds(commands_of(again, "subject")),
        ["MoveTo", "LineTo", "ArcTo", "Close"],
        "with the same vocabulary it had before",
    )
    assert_eq(
        [t for t in texts_of(again) if "REFUSED" in t],
        [],
        "and the refusal is gone",
    )

    # ── 10. the picture is stable across frames ──────────────────────────
    for _ in range(3):
        tf.tick(0.016)
    settled = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    for tag in ICONS + ("subject",):
        assert_eq(settled[tag], rects[tag], f"{tag} is a derivation, not a drift")

    print("[demo] the path keeps its curve")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1623 §5.3 — a path keeps the curve it was authored as", body)
