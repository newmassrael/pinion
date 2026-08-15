#!/usr/bin/env python3
"""R1693 — **screen B, announced**: the analyzer's capture viewer reproduced
against its own specification, over the wire.

Before this round `hello-packet-view` painted **186** addressable regions and
announced **three** accessibility nodes. Two of the three claimed a collection
role and held nothing: a `table` with no row, a `tree` with no item. Sixteen
messages of seven columns, twenty-one decoded fields over four layers,
seventy-two bytes, the query, the negotiated session context and the reassembly
lanes reached a reader as nothing at all — and every check in the example was
green, because a region with no accessibility node paints perfectly and answers
every question about its rectangle.

## The floor, built and run at 6.11.1 rather than read

* a 16x7 model-driven item view answers **137 nodes**, a table interface with
  `rowCount = 16` / `columnCount = 7`, and `cellAt(3, 4)` naming the cell and
  reporting its row, column and column header. That is the strong case, and it
  is why this screen's grid is built cell by cell rather than row by row.
* the same view with an **emptied model** answers `role = Table`,
  `rowCount = 0`, and no diagnostic. A tree with no items answers the same way.
  Nothing there separates a collection that is empty from one nobody filled.
* a **two-column tree** announces a row as **two sibling items** — the field and
  its value are peers, the value reports `expandable = 1`, leaf rows report
  `expanded = 1` while reporting they cannot expand, and the hierarchy is gone:
  every item is a direct child of the tree whatever its depth.
* a **custom-painted** 72-cell pane answers **one** node, empty-named, with no
  children. Everything painted rather than modelled is simply absent.

What this demo drives:

  (A) the SPECIFICATION off the wire — every population below is expanded from
      what the running application publishes, so a table that drifts fails here
      rather than being quietly re-asserted;
  (B) VOICE — every addressable region is classified, and the split between what
      speaks and what is deliberately quiet is the specification's;
  (C) STRUCTURE — every announced collection holds what its role promises, and
      every member is inside the collection its role requires;
  (D) the GRID — every cell is addressable and says which row and column it is
      in, under a header row of column headers;
  (E) the TREE — one item per field, carrying its depth AND its value, which is
      the pair the floor cannot express;
  (F) the BYTE GRID — nine rows of eight cells, each row headed by its offset;
  (G) DRIVEN — selecting a message moves the announced selection, folding a
      layer removes items, and the tree stays well formed through both;
  (H) the DISCRIMINATING read — the censuses track the live tree rather than
      reporting a constant, shown by the numbers moving when the screen does.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    run_demo,
    voice_rows,
)

EXAMPLE = "hello-packet-view"
VIEW = "packet_view"

CHECKS = []


def banner(what: str) -> None:
    print(f"[demo] -- {what}")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app, path):
    return app.query(f"/{VIEW}/external/{path}")


def nodes_by_tag(app):
    result = app.request("scene/access").result
    return {n["tag"]: n for n in result["nodes"]}


def painted_rects(app):
    """Every tag on the rendered scene, mapped to its rectangle.

    The value is `None` for a node that carries a tag and no box of its own — a
    `Scroll` viewport is the case here, and recording the tag anyway is what
    lets a caller ask "is this on the scene" separately from "where is it".
    """
    out: dict[str, dict | None] = {}

    def walk(node):
        if isinstance(node, dict):
            tag = node.get("tag")
            rect = node.get("rect") or node.get("bounds")
            if tag:
                out.setdefault(tag, rect)
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(app.snapshot(source="paint"))
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE) as app:
        # ── (A) the specification, off the wire ────────────────────────────
        banner("A — the screen publishes the specification it is built against")
        spec = q(app, "spec")
        ok("the screen publishes a specification", isinstance(spec, dict))
        rows, columns, fields = spec["rows"], spec["columns"], spec["fields"]
        voices, silences = spec["voices"], spec["silences"]
        ok("the specification declares what owes a voice", len(voices) > 0)
        ok("the specification declares what owes a silence", len(silences) > 0)
        print(
            f"[demo] {len(rows)} message(s) x {len(columns)} column(s), "
            f"{len(fields)} field(s); {len(voices)} region(s) owe a voice and "
            f"{len(silences)} owe a silence"
        )

        # ── (A2) the screen is LAID OUT the way the specification says ─────
        banner("A2 — the painted geometry is the specification's")
        rects = painted_rects(app)
        panes = spec["panes"]
        for pane in panes:
            ok(f"A2: {pane['tag']} is painted", rects.get(pane["tag"]) is not None)
            # ★ Its scrolling body is a `Scroll` node, which the paint snapshot
            # carries by tag and without a rectangle of its own — the viewport is
            # what it clips against, not a box it draws. So this asserts the tag
            # is THERE and leaves what it reaches to `r1662`, which drives it.
            ok(f"A2: {pane['body']} is on the scene", pane["body"] in rects)
            if pane["width"]:
                assert_eq(
                    rects[pane["tag"]]["w"],
                    pane["width"],
                    f"A2: {pane['tag']} is the width the specification gives it",
                )
        CHECKS.extend(["pane widths", "pane bodies present"])

        # The three panes are side by side on one line, and the flexible one
        # takes exactly what the other two leave.
        boxes = sorted((rects[p["tag"]] for p in panes), key=lambda r: r["x"])
        ok("A2: the three panes share a top edge", len({b["y"] for b in boxes}) == 1)
        ok("A2: and a height", len({b["h"] for b in boxes}) == 1)
        ok(
            "A2: ★ they tile left to right with no gap and no overlap",
            all(a["x"] + a["w"] == b["x"] for a, b in zip(boxes, boxes[1:])),
        )

        # Every column starts where the declared widths put it. The header rects
        # are in the list pane's own space, so the first one's x IS the pane's
        # padding — read rather than assumed, because a number assumed here would
        # be a second copy of a constant the painter owns.
        pad = rects["pv.list.head.0"]["x"]
        flex = rects["pv.list"]["w"] - sum(c["width"] for c in columns) - pad * 2
        ok("A2: the flexible column has room", flex > 0)
        offset = 0
        for n, column in enumerate(columns):
            assert_eq(
                rects[f"pv.list.head.{n}"]["x"],
                pad + offset,
                f"A2: column {column['title']!r} starts where the widths put it",
            )
            offset += column["width"] or flex
        assert_eq(
            offset + pad * 2,
            rects["pv.list"]["w"],
            "A2: ★ and the seven columns fill the pane exactly",
        )
        CHECKS.extend(["panes tile", "column offsets", "columns fill the pane"])
        print(
            f"[demo] panes {[b['w'] for b in boxes]}, "
            f"columns {[c['width'] or flex for c in columns]}"
        )

        # ── (B) voice: everything classified, and the split is the declared one
        banner("B — every addressable region is classified, the declared way")
        voice = app.voice()
        rowsv = voice_rows(voice)
        assert_eq(voice["counts"]["unvoiced"], 0, "B: nothing is unclassified")
        for arm in ("ghost", "dangling", "mumbled", "hollow"):
            assert_eq(voice["counts"][arm], 0, f"B: no {arm} region")
        CHECKS.extend(["unvoiced", "ghost", "dangling", "mumbled", "hollow"])

        for want in voices:
            row = rowsv.get(want["tag"])
            ok(f"B: {want['tag']} is painted", row is not None)
            assert_eq(row["voice"], "announced", f"B: {want['tag']} owes a voice")
        for want in silences:
            row = rowsv.get(want["tag"])
            ok(f"B: {want['tag']} is painted", row is not None)
            assert_eq(row["voice"], "silent", f"B: {want['tag']} owes a silence")
            assert_eq(row["reason"], want["kind"], f"B: {want['tag']}'s reason")
        CHECKS.extend(["declared voices", "declared silences", "declared reasons"])

        # ★ The two halves are the WHOLE screen. A floor would be satisfied by a
        # screen that grew a region nobody classified.
        assert_eq(
            len(voices) + len(silences),
            voice["total"],
            "B: ★ the specification classifies every painted region and no more",
        )
        CHECKS.append("the split is the whole screen")
        print(
            f"[demo] {voice['total']} region(s): "
            f"{voice['counts']['announced']} announced, "
            f"{voice['counts']['silent']} declared quiet"
        )

        # ── (C) structure: the tree is one a reader can walk ───────────────
        banner("C — every collection holds what its role promises")
        conform = app.conform()
        assert_eq(conform["counts"]["empty"], 0, "C: no collection holds nothing")
        assert_eq(conform["counts"]["stray"], 0, "C: no member is outside its collection")
        ok(
            "C: ★ and the census actually judged this screen's collections",
            conform["judged"] > 100,
        )
        CHECKS.extend(["conform empty", "conform stray"])
        print(f"[demo] {conform['judged']} node(s) carry a structural requirement")

        # ── (D) the grid ───────────────────────────────────────────────────
        banner("D — the message list is a grid a reader can traverse")
        tree = nodes_by_tag(app)
        grid = tree["pv.list"]
        assert_eq(grid["role"], "grid", "D: the list announces as a grid")
        # `aria-rowcount` is the TOTAL, header row included — WAI-ARIA's own
        # reading, and the one this tree's chart tables already use.
        assert_eq(grid["row_count"], len(rows) + 1, "D: it says how many rows it has")
        assert_eq(grid["column_count"], len(columns), "D: and how many columns")
        CHECKS.extend(["grid role", "grid rowcount", "grid colcount"])

        header = tree["pv.list.header"]
        assert_eq(header["role"], "row", "D: the headers sit in a row")
        assert_eq(len(header["children"]), len(columns), "D: one header per column")
        # ★ R1694 — the header row is row ONE, because `aria-rowcount` above
        # counts it. Until this was corrected the count said seventeen, the
        # rows were numbered one to sixteen, the header carried no index at all,
        # and the first message claimed the header's place.
        assert_eq(header["row_index"], 1, "D: the header row is row one")
        for n, column in enumerate(columns):
            head = tree[f"pv.list.head.{n}"]
            assert_eq(head["role"], "columnheader", f"D: head {n} is a column header")
            assert_eq(head["name"], column["title"], f"D: head {n} is named")
            assert_eq(head["column_index"], n + 1, f"D: head {n} says which column")
            assert_eq(head["row_index"], 1, f"D: head {n} is in the header row")
        CHECKS.extend(["header row", "column headers named", "column indices"])

        # Every cell of every message, addressable and placed.
        for n, message in enumerate(rows):
            row_node = tree[f"pv.list.row.{n}"]
            assert_eq(row_node["role"], "row", f"D: message {n} is a row")
            assert_eq(row_node["row_index"], n + 2, f"D: message {n} says which row")
            assert_eq(len(row_node["children"]), len(columns), f"D: message {n} cells")
            for c in range(len(columns)):
                cell = tree[f"pv.list.cell.{n}_{c}"]
                assert_eq(cell["role"], "gridcell", f"D: cell {n},{c}")
                assert_eq(cell["row_index"], n + 2, f"D: cell {n},{c} row")
                assert_eq(cell["column_index"], c + 1, f"D: cell {n},{c} column")
            # And the cells say what is painted in them.
            assert_eq(
                tree[f"pv.list.cell.{n}_0"]["name"],
                message["time"],
                f"D: message {n}'s timestamp cell",
            )
        assert_eq(
            tree[f"pv.list.row.{len(rows) - 1}"]["row_index"],
            tree["pv.list"]["row_count"],
            "D: ★ the last message is the row count, so no row is unreachable",
        )
        CHECKS.extend(["every row", "every cell", "cell coordinates", "cell contents"])
        print(f"[demo] {len(rows) * len(columns)} addressable cell(s)")

        # ★ The annotations are announced WITH the name cell, not beside it: a
        # reader told only "out of band" has nothing to attach it to.
        marked = next(n for n, r in enumerate(rows) if r["note"])
        ok(
            "D: ★ an annotated message announces its note with its name",
            rows[marked]["note"] in tree[f"pv.list.cell.{marked}_5"]["name"],
        )

        # ── (E) the tree ───────────────────────────────────────────────────
        banner("E — the decode is a tree of items that carry their own values")
        decode = tree["pv.tree"]
        assert_eq(decode["role"], "tree", "E: the decode announces as a tree")
        layer_ids = [layer["id"] for layer in spec["layers"]]
        for field in fields:
            item = tree[f"pv.tree.field.{field['path']}"]
            assert_eq(item["role"], "treeitem", f"E: {field['path']} is an item")
            assert_eq(item["name"], field["name"], f"E: {field['path']} is named")
            # ★ THE value, on the item — the floor makes it a sibling item.
            assert_eq(
                item["value"]["text"],
                field["value"],
                f"E: ★ {field['path']} carries its value",
            )
            depth = 1 if field["path"] in layer_ids else 2
            assert_eq(item["level"], depth, f"E: {field['path']} says its depth")
            # ★ And `expanded` is on the layers ONLY — the floor reports it on
            # leaves and on values alike.
            assert_eq(
                "expanded" in item,
                field["path"] in layer_ids,
                f"E: ★ {field['path']} claims to fold only if it folds",
            )
        CHECKS.extend(["tree items", "item names", "item values", "levels", "expanded"])
        print(f"[demo] {len(fields)} decode item(s), 4 layer(s) deep")

        # ── (F) the byte grid ──────────────────────────────────────────────
        banner("F — the bytes are a grid with a header per row")
        byte_len = spec["sources"][0]["len"]
        per_row = 8
        bytes_grid = tree["pv.bytes"]
        assert_eq(bytes_grid["role"], "grid", "F: the byte pane announces as a grid")
        assert_eq(
            bytes_grid["row_count"],
            (byte_len + per_row - 1) // per_row,
            "F: it says how many rows of bytes it has",
        )
        for r in range((byte_len + per_row - 1) // per_row):
            head = tree[f"pv.bytes.offset.{r}"]
            assert_eq(head["role"], "rowheader", f"F: row {r} is headed by its offset")
            assert_eq(head["name"], f"{r * per_row:04x}", f"F: row {r}'s offset")
        for b in range(byte_len):
            cell = tree[f"pv.bytes.cell.{b}"]
            assert_eq(cell["role"], "gridcell", f"F: byte {b}")
            assert_eq(cell["row_index"], b // per_row + 1, f"F: byte {b} row")
        CHECKS.extend(["byte grid", "row headers", "byte cells"])
        print(f"[demo] {byte_len} addressable byte(s) in {bytes_grid['row_count']} row(s)")

        # ── (G) driven ─────────────────────────────────────────────────────
        banner("G — the announcement follows the screen")
        opening = q(app, "selected_row")
        assert_eq(
            tree[f"pv.list.row.{opening}"]["selected"],
            True,
            "G: the open message is the announced selection",
        )
        # ★ The chord a real key press produces. This screen matched `Down`
        # until R1693, which no keyboard sends — so a `grid` that promises arrow
        # navigation had none, and every test that drove it used the screen's own
        # spelling and passed.
        app.key(path=VIEW, name="ArrowDown")
        app.tick(16)
        moved = q(app, "selected_row")
        ok("G: ★ a real arrow key moved the selection", moved != opening)
        tree = nodes_by_tag(app)
        assert_eq(
            tree[f"pv.list.row.{moved}"]["selected"],
            True,
            "G: and the announcement moved with it",
        )
        ok(
            "G: ★ the message left behind stopped claiming to be selected",
            tree[f"pv.list.row.{opening}"].get("selected") is not True,
        )
        assert_eq(app.conform()["counts"]["empty"], 0, "G: still well formed")
        CHECKS.extend(["selection announced", "selection follows", "sound after"])

        # Fold a layer: items disappear and the tree must stay a tree.
        before = len(q(app, "visible_fields"))
        app.invoke(f"/{VIEW}/external/toggle_layer", 0)
        app.tick(16)
        after = len(q(app, "visible_fields"))
        ok("G: folding a layer removed items", after < before)
        folded = app.conform()
        assert_eq(folded["counts"]["empty"], 0, "G: the folded tree still holds items")
        assert_eq(folded["counts"]["stray"], 0, "G: and nothing came loose")
        tree = nodes_by_tag(app)
        assert_eq(
            tree["pv.tree.field.l0"]["expanded"],
            False,
            "G: ★ the folded layer says it is folded",
        )
        CHECKS.extend(["folding removes items", "folded tree is sound", "expanded flips"])

        # ── (H) the discriminating read ────────────────────────────────────
        banner("H — the censuses read the live tree, not a constant")
        ok(
            "H: ★ folding the tree moved the structural denominator",
            folded["judged"] < conform["judged"],
        )
        ok(
            "H: ★ and the voice census's population moved with the paint",
            app.voice()["total"] != voice["total"],
        )
        # ★ The negative control that makes the two above mean something: what
        # moved is the TREE, and the screen is still classified in full.
        assert_eq(app.voice()["counts"]["unvoiced"], 0, "H: still nothing unclassified")
        CHECKS.extend(["judged tracks the tree", "total tracks the paint", "still total"])

        print(f"[demo] closing with message {moved} open, layer 0 folded")

    print(f"[demo] {len(CHECKS)} named assertion(s)")


if __name__ == "__main__":
    run_demo("R1693 a screen is a tree a reader can walk", body)
