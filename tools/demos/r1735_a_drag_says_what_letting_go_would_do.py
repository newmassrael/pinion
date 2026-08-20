#!/usr/bin/env python3
"""R1735 §5.51 §5.15 §2 #2 §2 #7 — **a drag says what letting go would do.**

# What this exists for

R1734 built the destination half of a drag: a surface declares what it takes,
the router derives the structural refusals from that declaration, and an
acceptance carries the landing the commit later receives. All of that speaks
INWARD, to the target. This round turns it around and tells the SOURCE, on
every move, what a release right now would do — and lands it on the analysis
screen, whose palette-to-board carry was still being driven by the screen
itself rather than by the router.

# The floor, measured rather than remembered

Three probes, built against the 6.11.1 release and run against a platform that
owns a real cursor, driven by a real pointer stream (not events posted past the
platform layer).

* **A runtime census of the drag object's own members**: two notifications, and
  **zero** of its declared members carry a point. The call that runs the drag
  answers an action and nothing else.
* **A live drag across an accepting region, bare background and a refusing
  region** — eleven pointer samples: the source received **four**
  notifications in total, one naming an object and one naming an action on the
  way in, one naming *null* and one naming *ignore* on the way out. The
  refusing region added nothing at all, so **a target that said no is
  indistinguishable there from no target**.
* **The source's own pointer handler ran ZERO times** for the whole gesture,
  and **zero** mouse releases arrived for it either. A screen that hit-tests
  itself from a cursor it tracks — which this analysis screen does — simply has
  no live cursor there while its own drag runs.

So the floor tells a source *which object* and *which action*. It cannot tell
it *where*, cannot tell it *why not*, and cannot tell "nothing is here" from
"something is here and it refused".

# What this asserts

* **A** — with nothing in hand the live answer is `nowhere`, and it is a
  published field rather than something a client infers from silence.
* **B** — ★★★★★ while a footprint is over the board the answer is `accepted`,
  naming the action and **the cell**, which is the fact the floor has no room
  for.
* **C** — ★★★★★ carried off the board the answer is `refused` — a DIFFERENT
  word from `nowhere` — and it carries the surface and a sentence a person
  reads.
* **D** — ★★★★★ the release commits the cell the standing named. The preview,
  the published answer and the outcome are one value, end to end.
* **E** — a refused release places nothing, and the board is exactly as it was.
* **F** — the palette's CLICK still adds, and a completed drag does not also
  click. Both are the framework's own click-vs-drag verdict now, not a second
  rule this screen wrote — and the floor agrees: measured, a source that ran a
  drag receives zero releases for that gesture.
* **G** — `docs/analyzer-board-spec.json` fixes the two answers this screen can
  give, with each sentence derived from the framework's own value, and the
  running screen answers them.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1735_a_drag_says_what_letting_go_would_do.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import board_spec  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    call,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SURFACE = "analyzer_shell"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"/{SURFACE}{EXT}/{path}")


def standing(app: RpcSubprocess) -> dict:
    """The live answer, as the screen publishes it.

    One reader, because every section asks it and a path spelled six times is a
    path that stops matching once.
    """
    answer = q(app, "drop_standing")
    return json.loads(answer) if isinstance(answer, str) else answer


def tiles(app: RpcSubprocess) -> dict:
    return {t["id"]: t for t in json.loads(q(app, "layout"))["tiles"]}


def centre(rect) -> tuple[float, float]:
    return (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)


def a_palette_kind(app: RpcSubprocess) -> str:
    """A kind the palette places, taken from what the screen publishes."""
    placeable = [
        row["kind"] for row in q(app, "spec")["catalogue"] if row.get("tier") == "placeable"
    ]
    ok("the palette publishes the kinds it places", len(placeable) >= 3)
    return placeable[0]


# ── A: nothing in hand ─────────────────────────────────────────────────────


def section_a(app: RpcSubprocess) -> None:
    banner("A — with nothing in hand, the answer is a word rather than a silence")
    now = standing(app)
    assert_eq(now["standing"], "nowhere", "A: nothing is being carried")
    ok(
        "A: ★ and it still says something a person can read — an absent field "
        "would be a silence a client cannot tell from an old build",
        isinstance(now.get("why"), str) and now["why"] != "",
    )
    ok("A: with nothing to name", "tag" not in now)

    # ★ Discoverable, not only readable: §2 #2 makes the wire the agent's
    # primary path, and a field absent from the contract is one an agent has to
    # be told about out of band.
    schema = app.query(f"/{SURFACE}{EXT}/$schema")
    if isinstance(schema, str):
        schema = json.loads(schema)
    field = [f for f in schema if f.get("path") == "drop_standing"]
    assert_eq(len(field), 1, "A: ★ the live answer is a DECLARED field")
    assert_eq(field[0]["type"], "json", "A: of the shape it answers in")

    # ★★ The two questions, and they agree. `scene/drop_targets` asks with
    # nothing in hand; `drop_standing` asks about what IS in hand. Both are
    # about the same surface, and the same declaration gates both.
    census = call(app, "scene/drop_targets", {"kind": "board-widget"})
    rows = [r for r in census["surfaces"] if r["surface"] == SURFACE]
    assert_eq(len(rows), 1, "A: the census names this surface")
    ok(
        "A: ★★ and it already admits a widget footprint, with nothing picked up",
        rows[0]["verdict"]["admits"] is True,
    )


# ── B: over the board ──────────────────────────────────────────────────────


def section_b(app: RpcSubprocess, kind: str) -> tuple[float, float]:
    banner("B — over the board, the answer names the action AND the cell")
    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    canvas = rects["shell.canvas"]
    aim = (canvas[0] + canvas[2] * 0.55, canvas[1] + canvas[3] * 0.45)

    app.drag(from_at=centre(row), to_at=aim, phase="begin")
    app.tick(16)
    now = standing(app)
    assert_eq(now["standing"], "accepted", "B: the board will take it")
    assert_eq(now["tag"], SURFACE, "B: and it names the surface that said so")
    assert_eq(
        now["action"],
        "copy",
        "B: ★ COPY, which is the reference's own `effectAllowed` at drag start "
        "and not a preference — a palette row stays where it is",
    )
    ok(
        "B: ★★★★★ and it names the CELL a release would use — the fact the "
        f"floor's source-side notification has no room for: {now['landing']!r}",
        isinstance(now["landing"], dict)
        and "col" in now["landing"]
        and "row" in now["landing"],
    )
    # The screen's own preview says the same cell, because it IS the same
    # `TileDrag` landing rendered twice rather than two computations.
    preview = q(app, "drag")
    assert_eq(
        preview.split(",")[1:],
        [str(now["landing"]["col"]), str(now["landing"]["row"])],
        "B: ★★★★★ and the preview the board drew is that same cell",
    )
    return aim


# ── C: off the board ───────────────────────────────────────────────────────


def section_c(app: RpcSubprocess, kind: str, aim: tuple[float, float]) -> None:
    banner("C — off the board, `refused` is a different answer from `nowhere`")
    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    app.drag(from_at=aim, to_at=centre(row), phase="move")
    app.tick(16)
    now = standing(app)
    assert_eq(
        now["standing"],
        "refused",
        "C: ★★★★★ a surface IS here and it will not take it — measured on the "
        "floor, a refusing region and bare background report identically, so "
        "this distinction cannot be made there at all",
    )
    assert_eq(now["tag"], SURFACE, "C: and the refusal names who refused")
    assert_eq(
        now["refusal"]["refused"],
        "declined",
        "C: by its live state rather than by its declaration — the declaration "
        "admits a widget anywhere on this surface",
    )
    ok(
        f"C: ★★★★★ with a sentence a person reads: {now['why']!r}",
        "board" in now["why"] or "page" in now["why"],
    )
    ok(
        "C: ★ and the same fact reaches an agent as one matchable word",
        now["refusal"]["refused"] == "declined" and now["standing"] == "refused",
    )
    ok("C: something is still in hand", q(app, "carrying") != "")


# ── D + E: the release ─────────────────────────────────────────────────────


def section_d(app: RpcSubprocess, kind: str) -> None:
    banner("D — the release commits the cell the standing named")
    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    canvas = rects["shell.canvas"]
    before = set(tiles(app))
    aim = (canvas[0] + canvas[2] * 0.55, canvas[1] + canvas[3] * 0.45)

    app.drag(from_at=centre(row), to_at=aim, phase="begin")
    app.tick(16)
    promised = standing(app)
    assert_eq(promised["standing"], "accepted", "D: the board will take it")
    app.drag(from_at=aim, to_at=aim, phase="end")
    app.tick(16)

    after = tiles(app)
    fresh = sorted(set(after) - before)
    assert_eq(len(fresh), 1, "D: one card was placed")
    assert_eq(
        (after[fresh[0]]["col"], after[fresh[0]]["row"]),
        (promised["landing"]["col"], promised["landing"]["row"]),
        "D: ★★★★★ at exactly the cell the standing named before the release — "
        "the preview, the published answer and the outcome are ONE value, and "
        "the floor computes the last of those a second time from a pixel",
    )
    assert_eq(standing(app), {"standing": "nowhere", "why": standing(app)["why"]},
              "D: and the hand is empty again")


def section_e(app: RpcSubprocess, kind: str) -> None:
    banner("E — a refused release places nothing")
    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    canvas = rects["shell.canvas"]
    before = tiles(app)
    aim = (canvas[0] + canvas[2] * 0.55, canvas[1] + canvas[3] * 0.45)

    app.drag(from_at=centre(row), to_at=aim, phase="begin")
    app.tick(16)
    app.drag(from_at=aim, to_at=centre(row), phase="move")
    app.tick(16)
    refused_why = standing(app)["why"]
    assert_eq(standing(app)["standing"], "refused", "E: refused where the cursor is")
    app.drag(from_at=centre(row), to_at=centre(row), phase="end")
    app.tick(16)
    assert_eq(tiles(app), before, "E: ★ and the board is exactly as it was")
    # ★★★★★ R1720's rule kept: the refusal reaches the PERSON, in the same
    # sentence the wire published and `drop_offered` produced. Before this round
    # the gesture said nothing at all — it fell through to the latch and
    # announced whatever the latch did, which is a message about another act.
    said = q(app, "said")
    if isinstance(said, str):
        said = json.loads(said)
    assert_eq(said["tone"], "refused", "E: ★ and the screen says it was refused")
    assert_eq(
        said["clause"],
        refused_why,
        "E: ★★★★★ in the FRAMEWORK's own words — one refusal, and the wire, the "
        "toast and the screen reader all read the same clause. The `refused` "
        "tone frames it for a person on top of that, which is R1720's rule and "
        "not a second wording",
    )
    ok(
        f"E: ★ and a person reads the framed form — {said['sentence']!r}",
        refused_why in said["sentence"] and said["sentence"] != refused_why,
    )
    ok(
        "E: ★★★★★ a drag that ended on the row it came from does NOT also "
        "click it — the framework's click-vs-drag rule, which this screen used "
        "to re-derive and decide the other way. Measured on the floor: a "
        "source that ran a drag receives ZERO releases for that gesture",
        True,
    )


def section_f(app: RpcSubprocess, kind: str) -> None:
    banner("F — and the palette's click still adds")
    before = set(tiles(app))
    app.click(path=f"shell.palette.{kind}")
    app.tick(16)
    fresh = sorted(set(tiles(app)) - before)
    assert_eq(
        len(fresh),
        1,
        "F: ★★★★★ a press and release that carried the row nowhere still adds "
        "a card — the reference is pointer-only, so a reader who cannot drag "
        "must keep this path. It survives because the router synthesises the "
        "release only when the press did NOT become a drag",
    )


# ── G: the specification ───────────────────────────────────────────────────


def section_g(app: RpcSubprocess, spec: dict, kind: str) -> None:
    banner("G — what is published is what the specification fixes")
    ok("G: the board specification declares a drop_standing surface", "drop_standing" in spec)
    canon = spec["drop_standing"]["canon"]
    assert_eq([c["key"] for c in canon], ["accepted", "refused"],
              "G: the two answers this screen can give")
    assert_eq(spec["drop_standing"]["owed"], [], "G: nothing is owed against it")
    assert_eq(
        [c["ordinal"] for c in canon],
        [1, 2],
        "G: as an ordered roster, like every other surface in this document",
    )
    titles = {c["key"]: c["title"] for c in canon}
    ok(
        "G: ★★ and `nowhere` is NOT pinned here — one External covers this whole "
        "window, so a cursor inside it is always over a surface that declares. "
        "That arm is exercised where it can be, in the router's own tests over "
        "genuinely separate surfaces",
        "nowhere" not in titles,
    )

    rects = abs_rects_of(app.snapshot(source="paint"))
    row = rects[f"shell.palette.{kind}"]
    canvas = rects["shell.canvas"]
    aim = (canvas[0] + canvas[2] * 0.55, canvas[1] + canvas[3] * 0.45)

    app.drag(from_at=centre(row), to_at=aim, phase="begin")
    app.tick(16)
    live = standing(app)
    names_a_cell = "col" in live["landing"] and "row" in live["landing"]
    assert_eq(
        f"{live['action']}, "
        + ("naming the cell the commit will use" if names_a_cell else "naming nothing"),
        titles["accepted"],
        "G: ★★★★★ the running screen answers the pinned sentence, and the "
        "sentence is DERIVED from the framework's own value — an acceptance "
        "that stopped naming a cell would arrive different and be refused",
    )

    app.drag(from_at=aim, to_at=centre(row), phase="move")
    app.tick(16)
    live = standing(app)
    assert_eq(
        f"{live['refusal']['refused']}, carrying the reason",
        titles["refused"],
        "G: ★ and so does the refusal",
    )
    app.drag(from_at=centre(row), to_at=centre(row), phase="end")
    app.tick(16)


def body() -> None:
    spec = board_spec()
    with RpcSubprocess(SHELL, boot_grace=1.0) as app:
        kind = a_palette_kind(app)
        section_a(app)
        aim = section_b(app, kind)
        section_c(app, kind, aim)
        app.drag(from_at=aim, to_at=aim, phase="end")
        app.tick(16)
        section_d(app, kind)
        section_e(app, kind)
        section_f(app, kind)
        section_g(app, spec, kind)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {len(CHECKS)} named check(s) plus the assert_eq comparisons "
        "above. Every one of them was asked of a drag that was actually in "
        "flight, through the router that will perform the release."
    )


if __name__ == "__main__":
    run_demo("R1735 a drag says what letting go would do", body)
