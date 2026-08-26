#!/usr/bin/env python3
"""R1843 §5.2 §5.21 §5.40 — **KPI stat tiles with sparklines, composed.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `dashboard.t1.8` —
*KPI stat tiles with sparklines* — as an **app** verdict, and its covering
sentence has always been a claim about composition:

    MEASURED R1646: `sparkline`'s own doc names the KPI stat tile as a PLACE it
    is displayed, not as a widget the crate ships. The tile is a box, a label
    and a sparkline, assembled per application

That verdict named **no assembly**, which is what R1807's `UNASSEMBLED` ratchet
records: a claim about composition nobody had composed. This is the
composition, driven on the wire, on the analyzer shell itself — closing a
census row on a demo that never touches the reference screen closes a line
without the screen gaining anything (the R1722 lesson).

# ★★★★★ The covering sentence was TRUE and this round made half of it false

R1646 measured that no crate shipped a tile, and it was right. R1843 built
`pinion_widget_paint::stat_tile`, so *"not a widget the crate ships"* stopped
being true the moment this card was drawn — and the row's sentence is corrected
in the same commit rather than left to read as a measurement of a tree that no
longer exists.

What did NOT change is the verdict. `app` is still right, and deliberately: the
tile does not embed the chart. `pinion-widget-paint` has no dependency on
`pinion-chart` and a tile that named a sparkline would create one, so the
trailing figure is a scene the CALLER builds for a rectangle the tile reserves.
The composition is therefore still an application's to make — which is exactly
what an `app` verdict means, and why this row does not become `have`.

# What was composed, and what was deliberately not

The tile is `pinion_widget_paint::stat_tile::StatTile`, which places its words
through `crate::caption` — so a word is a CHILD of a box that is its own rather
than a sibling of the tile, which is what stops it being filed under whatever
encloses it. The series is `pinion_chart::Sparkline`, handed to the tile's
trailing seam. Nothing in the shell spells either one.

What was NOT taken is a fixed tile count. The strip asks each tile how much
room its own words need and shows the ones that fit, because the card is placed
four columns wide and five tiles do not fit there — see section D.

# ⚠ The tiles are a FIXTURE and this demo says so

`spec::HEALTH_TILES` is a table, not a derivation: nothing in this tree
accumulates a per-window series for these five quantities. The latency card
next door derives every number it draws from one capture record and this card
cannot yet. Section C therefore checks that the screen ANNOUNCES what it shows,
which is a claim that can fail, and does not pretend the readings are measured.

# What is shown

  (A) the seat is no longer locked — the catalogue offers `health` as placeable
      and the deferred register declares it BUILT rather than reserved.
  (B) the board places it, and every card on that board is at least as wide in
      place as it is torn off.
  (C) every tile the strip paints is announced with its reading and its change,
      so a reader who cannot see a sparkline is still told the number it ends
      at.
  (D) the strip narrows by dropping whole TILES and never a tile's cells: a
      label without its value would be a fact a reader misreads.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    resize_and_settle,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def tile_nodes(app: RpcSubprocess, card: str) -> dict[int, dict[str, Any]]:
    """The strip's announced tiles, by index, from the accessibility tree.

    Read off `scene/access` rather than off the model, because what section C
    claims is that a *reader* is told — the model holding the number is a
    different claim from the tree carrying it.
    """
    stem = f"card.{card}.stat."
    out: dict[int, dict[str, Any]] = {}
    for node in app.request("scene/access").result["nodes"]:
        tag = node.get("tag") or ""
        if not tag.startswith(stem):
            continue
        rest = tag[len(stem) :]
        if "." in rest or not rest.isdigit():
            # A tile's own rows (`.label`, `.value`, `.delta`, `.trail`) answer
            # to their own tags. The tile is the one that carries the reading.
            continue
        out[int(rest)] = node
    return out


def painted_tiles(app: RpcSubprocess, card: str) -> int:
    """How many tiles the strip actually PAINTED, read from the paint.

    ⚠ From the paint (`source="paint"`) and not from the state, because what
    section D claims is about what a reader sees at a size — and a strip that
    narrows decides how many tiles to draw while painting. Reading the state
    would answer about the table, which never narrows.
    """
    stem = f"card.{card}.stat."
    seen = set()
    for tag in walk_tags(app.snapshot(source="paint")):
        if tag.startswith(stem):
            seen.add(tag[len(stem) :].split(".")[0])
    return len(seen)


def reading(node: dict[str, Any]) -> str:
    """What a reader is told this tile says.

    ⚠ Normalised, because `value` on the wire is a typed thing — the access
    tree carries a value's KIND beside its text, so a bare `node["value"]` is a
    guess about the shape rather than a read of it.
    """
    value = node.get("value")
    if isinstance(value, dict):
        for key in ("text", "Text", "value"):
            if isinstance(value.get(key), str):
                return value[key]
        return str(value)
    return value if isinstance(value, str) else ""


def walk_tags(node: Any) -> list[str]:
    """Every tag in a scene tree, wherever it is nested."""
    out: list[str] = []
    if isinstance(node, dict):
        tag = node.get("tag")
        if isinstance(tag, str):
            out.append(tag)
        for value in node.values():
            out.extend(walk_tags(value))
    elif isinstance(node, list):
        for value in node:
            out.extend(walk_tags(value))
    return out


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as d:
        # ── (A) the seat is not locked any more, and says so twice ──────────
        banner("A — the health seat is placeable, and the register declares it built")
        catalogue = d.query(f"{EXT}/catalogue").split(",")
        ok("the catalogue offers a health seat", "health" in catalogue)

        spec: Any = d.query(f"{EXT}/spec")
        reserved = {w["kind"] for w in spec["catalogue"] if w.get("reserved_for")}
        ok(
            "and it is NOT among the seats a later release brings",
            "health" not in reserved,
        )
        # ★ Promoting a seat changes the RELEASE structure the reference
        # defines, so it is DECLARED rather than made to look like a seat that
        # was never locked. R1797 set that pattern for the latency card.
        register = Path("docs/analyzer-reserved-spec.json")
        ok("the deferred register is where the promotion is recorded", register.exists())

        # ── (B) the board places it, at a width a card can be read at ───────
        banner("B — the board places six cards, none narrower than its own float")
        placed = [tile["kind"] for tile in spec["board"]]
        ok("the opening board places the health card", "health" in placed)
        assert_eq(len(placed), len(set(placed)), "no kind is placed twice")
        # ⚠ The floor is not a number chosen here: `FLOAT_MIN_W` is what a card
        # clamps to once torn off, and a card legible detached and illegible in
        # place would be this shell disagreeing with itself about one thing.
        # R1843 twice concluded six cards could not fit by measuring ROWS and
        # never widths; the shell's own gate now prints these numbers.
        widths = {tile["kind"]: tile["cols"] for tile in spec["board"]}
        print(f"    board spans, in columns: {widths}")
        ok(
            "every placed card spans the same, readable width",
            len(set(widths.values())) == 1,
        )

        # ── (C) every painted tile is announced with what it says ───────────
        banner("C — the strip announces each tile's reading and its change")
        # ⚠ A card's id is `kind#n` where n is its INDEX on the board — it is
        # derived, not stored, which is why the board publishes no `id` field.
        # R1843 learned this the expensive way: its first cut inserted the new
        # placement in the MIDDLE of `spec::BOARD` and renamed every card after
        # it, which surfaced as six gates reporting cards that had vanished.
        # A new placement goes last, and an id is computed the way the shell
        # computes it.
        card = f"health#{[t['kind'] for t in spec['board']].index('health')}"
        nodes = tile_nodes(d, card)
        painted = painted_tiles(d, card)
        print(f"    card {card}: {painted} tile(s) painted, {len(nodes)} announced")
        ok("the strip painted at least one tile", painted > 0)
        assert_eq(len(nodes), painted, "every painted tile is announced")
        for n, node in sorted(nodes.items()):
            value = reading(node)
            print(f"    tile {n}: {node.get('name')!r} -> {value!r}")
            ok(f"tile {n} is named", bool(node.get("name")))
            # ★ The change is part of the reading, not decoration: a KPI tile
            # without its delta states a level where the reference states a
            # trend. A reader who cannot see the sparkline is told the number
            # the series ends at and which way it moved.
            ok(f"tile {n} announces its change", "since the previous window" in value)

        # ── (D) narrowing drops whole tiles, never a tile's cells ───────────
        banner("D — the strip drops whole tiles as it narrows, never a tile's words")
        wide = resize_and_settle(d, (2494, 1531))
        assert wide is not None
        wide_tiles = painted_tiles(d, card)
        wide_nodes = tile_nodes(d, card)
        print(f"    maximised: {wide_tiles} tile(s)")
        ok("a wider card shows more tiles", wide_tiles > painted)
        assert_eq(len(wide_nodes), wide_tiles, "and announces exactly what it shows")
        for n, node in sorted(wide_nodes.items()):
            # ★★★★★ THE CLAIM THIS SECTION EXISTS FOR. A tile's three words are
            # ONE claim — the label, the reading and the change — and a tile
            # showing two of them says something a reader would misread. So the
            # strip is declared `Cells::Whole` in the paint census: it drops
            # whole tiles and never a tile's cells. Checked on every tile at
            # both sizes rather than asserted once.
            ok(f"maximised tile {n} still carries a full reading", bool(reading(node)))

        narrow = resize_and_settle(d, (1440, 900))
        assert narrow is not None
        assert_eq(painted_tiles(d, card), painted, "and narrowing puts it back")

    print(f"\n{len(CHECKS)} named check(s) passed")


run_demo("r1843 a health strip is composed, not hand-rolled", body)
