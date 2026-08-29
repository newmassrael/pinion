#!/usr/bin/env python3
"""R1905 §5.16 §5.21 §2 #7 — **a detached card's rectangle is in a SPACE, and
changing home crosses between two of them.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This one repays the geometry half of the remainder R1891
wrote down when it closed `debt-one-card-is-claimed-by-a-panel-and-a-window`:

> a panel in the canvas home does not know its window's geometry — changing
> home starts it at a new place. The transfer between the two coordinate
> systems is what that round did NOT decide.

# ★★★★★ The measurement that opened this round

Driven against this same assembled tool before any of it was built:

    tear off      -> floats [{x:120, y:40, w:520, h:380, home:"window"}]
                     windows torn-packet#0 position [120, 40]   <- DISPLAY space
    detach_home   -> floats [{x:120, y:40, w:520, h:380, home:"canvas"}]
                                                                <- HOST space
    windows main  -> position: None

The identical four numbers, read against two different origins, with nothing in
the value saying which — and `position: None` for the host, which is the
truthful report of a window the manager placed and is also why nothing in the
tree *could* convert. => *A rectangle without its space is not a place, and a
framework that cannot say where its own window is cannot give it one.*

# What this walk holds

  (A) the assembled tool publishes, for every detached card, which SPACE its
      rectangle is measured in — a fact `home` alone did not give a client,
      because the mapping from home to space lived only in this framework.
  (B) nothing has crossed yet, which is published as null rather than as an
      arrival; and the host window still declares no position, which is what
      made the origin unpublishable before this round.
  (C) sending a card to the canvas CONVERTS its position through the host's
      own origin, and says so — the crossing is not adrift.
  (D) sending it back is the inverse, so a reader who changes their mind does
      not pay a display origin every trip.
  (E) and the card that crossed is still reachable: the screen's own hit test
      answers for its header, at a point inside the window.

# What this walk deliberately does NOT hold

The arithmetic of the conversion. Offscreen there is no window manager to place
the host away from the display's corner, so the offset is zero here and
converting and relabelling are the same numbers — the shape R1901.2 named,
where both sides of a check move together and the check goes blind. That
assertion lives in the in-process gate, which stamps a non-zero origin through
the framework's own sink and asserts the subtraction:
`r1905_changing_home_crosses_the_two_coordinate_spaces`.

What a walk holds and no unit test can is that the RUNNING tool consulted the
transfer at all, published the space and the arrival, and left the panel
reachable.

# Superior to the floor

The floor toolkit's detached panel is always a top-level window, so it has no
second space to cross into and nothing to publish about one. What this tree has
is a named space per detached panel, a conversion between the two, and a word on
the wire saying whether the conversion happened — so an agent driving this tool
headlessly can tell "moved there" from "relabelled there", which is the
distinction a person reports as a panel jumping.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1905_a_card_changing_home_crosses_two_spaces.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import abs_rects_of, run_demo, RpcSubprocess  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, name: str):
    """One published slot, decoded."""
    value = app.query(f"{EXT}/{name}")
    return json.loads(value) if isinstance(value, str) else value


def settle(app: RpcSubprocess) -> None:
    for _ in range(6):
        app.tick_ms(16)


def float_of(app: RpcSubprocess, card: str) -> dict:
    """The one detached card this walk is about."""
    floats = q(app, "floats")
    found = [f for f in floats if f["id"] == card]
    assert found, f"{card!r} must be among the floats: {floats}"
    return found[0]


def window_extent(app: RpcSubprocess) -> tuple[int, int]:
    """The size the scene was painted at, from the snapshot's own root rect."""
    snap = app.snapshot(source="paint")
    rect = snap.get("rect", {}) if isinstance(snap, dict) else {}
    return (rect.get("w"), rect.get("h"))


def windows(app: RpcSubprocess) -> list[dict]:
    resp = app.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result["windows"]


def section_a(app: RpcSubprocess) -> dict:
    banner("A — a detached card publishes WHICH SPACE its rectangle is in")
    ok(
        "A: before any crossing the arrival is null — an unasked question is "
        "not an answer, which is why the slot is nullable and not four-armed",
        q(app, "arrival") is None,
    )
    said = app.invoke(f"{EXT}/act", f"{CARD},tear_off")
    ok(f"A: the board card tears off — {said!r}", said is not None)
    settle(app)
    torn = float_of(app, CARD)
    ok(
        f"A: ** it names its home AND its space — {torn['home']!r} / "
        f"{torn.get('space')!r}",
        torn.get("space") is not None,
    )
    ok(
        "A: ***** a window-homed card's rectangle is in the DISPLAY's space — "
        "a fact a client previously held only by knowing this framework's own "
        "mapping from home to space",
        torn["home"] == "window" and torn["space"] == "display",
    )
    ids = [w["id"] for w in windows(app)]
    ok(
        f"A: * and a real window carries it, so the space is not a claim — {ids}",
        any(i.endswith(CARD) for i in ids),
    )
    return torn


def section_b(app: RpcSubprocess) -> None:
    banner("B — the host still declares no position, which is the whole seam")
    main = [w for w in windows(app) if w["id"] == "main"]
    ok("B: the host window is there to have an origin", len(main) == 1)
    ok(
        f"B: ***** and it declares no position — {main[0]['position']!r} — which "
        "is what made the origin unpublishable before this round: a window the "
        "manager placed has no DECLARED place to read, and nothing else "
        "published where it was",
        main[0]["position"] is None,
    )


def section_c(app: RpcSubprocess, before: dict) -> dict:
    banner("C — sending it to the canvas CONVERTS the rectangle, and says so")
    said = app.invoke(f"{EXT}/detach_home", f"{CARD},canvas")
    ok(f"C: the canvas is a home this host admits — {said!r}", said is not None)
    settle(app)
    after = float_of(app, CARD)
    ok(
        f"C: ** the space it publishes followed the home — "
        f"{after['home']!r} / {after['space']!r}",
        after["home"] == "canvas" and after["space"] == "host",
    )
    arrival = q(app, "arrival")
    ok(f"C: ** and the crossing published how it went — {arrival}", arrival is not None)
    ok(
        "C: ***** the host could place itself, so the crossing was CONVERTED "
        f"rather than left unconverted — knows_offset="
        f"{arrival['knows_offset']}, how={arrival['how']!r}. `adrift` here "
        "would mean the seam this round built is not reaching the screen",
        arrival["knows_offset"] is True and arrival["how"] != "adrift",
    )
    # ★★★★★ The arithmetic, against the screen's OWN painted canvas rectangle
    # rather than against chrome constants written down here. Offscreen there is
    # no window manager to place this window away from the corner, so the
    # window's own origin is zero — but the CANVAS's is not, and that is what a
    # float's stored pair is measured from. So the conversion is visible in the
    # running tool after all.
    canvas = abs_rects_of(app.snapshot(source="paint"))["shell.canvas"]
    ok(
        f"C: ***** the x moved by exactly the canvas's own origin — "
        f"{before['x']} -> {after['x']}, canvas at x={canvas[0]}. Unchanged "
        "here is the defect this round repaid, not a pass",
        after["x"] == before["x"] - canvas[0],
    )
    ok(
        f"C: ***** and the y could not keep its place, so it says PULLED-IN "
        f"rather than reporting the place asked for — {before['y']} -> "
        f"{after['y']}, canvas at y={canvas[1]}; a window above the canvas has "
        "no y inside it, and silently keeping 40 is what this replaces",
        arrival["how"] == "pulled-in" and after["y"] == 0,
    )
    ids = [w["id"] for w in windows(app)]
    ok(
        f"C: ** and the window is gone, so the two pictures stay disjoint — {ids}",
        not any(i.endswith(CARD) for i in ids),
    )
    return after


def section_d(app: RpcSubprocess, before: dict) -> None:
    banner("D — a round trip that keeps its place is the identity")
    # ⚠ Started from the CANVAS side, and that is the point rather than a
    # convenience. This walk's first draft made the round trip window -> canvas
    # -> window and asserted the identity; it FAILED, measured, at
    # `(68, 0) -> (120, 98)` — because the outward leg had been pulled in, and a
    # crossing that moved the panel cannot be undone by crossing back. The
    # honest property is that a crossing which KEPT its place is invertible, and
    # a place inside the canvas is one that can.
    out = app.invoke(f"{EXT}/detach_home", f"{CARD},window")
    ok(f"D: a window is a home this host admits — {out!r}", out is not None)
    settle(app)
    away = float_of(app, CARD)
    first = q(app, "arrival")
    ok(
        f"D: ** leaving the canvas keeps its place — ({before['x']}, "
        f"{before['y']}) -> ({away['x']}, {away['y']}), how={first['how']!r}; "
        "the display's side has no bound of ours to be pulled into",
        first["how"] == "kept",
    )
    ok(
        f"D: * and the space came back with it — {away['space']!r}",
        away["space"] == "display",
    )
    app.invoke(f"{EXT}/detach_home", f"{CARD},canvas")
    settle(app)
    back = float_of(app, CARD)
    second = q(app, "arrival")
    ok(
        f"D: ***** a kept crossing is invertible — ({before['x']}, "
        f"{before['y']}) -> ({away['x']}, {away['y']}) -> ({back['x']}, "
        f"{back['y']}); a reader who changes their mind must not pay a canvas "
        "origin every trip",
        (back["x"], back["y"]) == (before["x"], before["y"]),
    )
    ok(
        f"D: * and both legs reported themselves — {first['how']!r} / "
        f"{second['how']!r}",
        second["how"] == "kept",
    )


def section_e(app: RpcSubprocess) -> None:
    banner("E — the card that crossed is still reachable by hand")
    panel = float_of(app, CARD)
    ok(
        f"E: it is on the canvas, which is where a hand of ours can reach it — "
        f"{panel['home']!r}",
        panel["home"] == "canvas",
    )
    # ★ The PAINTED rectangle, not arithmetic on the stored one. A float's own
    # pair is in the canvas's frame and the pointer speaks the window's, and a
    # walk that converted between them here would be a second spelling of the
    # chrome — the `debt-paint-and-gesture-read-two-facts` shape. Asking the
    # snapshot asks where the panel actually IS.
    drawn = abs_rects_of(app.snapshot(source="paint"))
    tag = f"float.{CARD}"
    ok(f"E: the crossed panel is painted — {tag}", tag in drawn)
    x, y, w, h = drawn[tag]
    win = window_extent(app)
    ok(
        f"E: ** it is painted whole inside the window — {drawn[tag]} in {win}; "
        f"stored at ({panel['x']}, {panel['y']}) in the canvas's own frame",
        x >= 0 and y >= 0 and x + w <= win[0] and y + h <= win[1],
    )
    px, py = x + w // 2, y + 8
    app.hover(at=(float(px), float(py)))
    settle(app)
    under = app.query(f"{EXT}/hit")
    ok(
        f"E: ***** and the panel that crossed answers a hover there — {under!r}; "
        "a panel a hand cannot reach is a panel that was not moved anywhere "
        "useful, and R1903 measured that a reach check satisfied by a point "
        "outside the surface is not a check",
        isinstance(under, str) and "float" in under,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        torn = section_a(app)
        section_b(app)
        crossed = section_c(app, torn)
        section_d(app, crossed)
        section_e(app)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1905 a card changing home crosses two spaces", body)
