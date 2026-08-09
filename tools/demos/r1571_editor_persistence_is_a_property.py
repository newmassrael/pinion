#!/usr/bin/env python3
"""R1571 §5.27 §5.12 §5.40 — N editors open at once, over the wire.

the toolkit's `openPersistentEditor(index)` keeps any number of
editors open simultaneously. R1544 and R1555 both closed naming its absence as
the DCC axis's remaining item, and R1555 also wrote down the prescription: widen
`use_text_edit_state`'s `&'static str` key so a per-cell buffer can be cached at
a runtime id. That prescription is wrong, and the round found out by building
it — `Owner::cache` has no removal of any kind, so a per-cell buffer would be
retained for every cell ever edited, for the life of the window. An editor's
buffer has to die with the editor, so the buffers live in the editor SET.

What the set needs then follows from a fact about this framework rather than
about the toolkit: there is exactly ONE keyboard focus, where the toolkit has one focusable
widget per editor. So only the focused editor holds the shared inline field;
every other open editor's text is PARKED in the latch and swapped back when
focus returns.

This drives `hello-cell-editors` and proves each piece from the outside:

  (A) the set is ENUMERABLE. `persistent` is a private
      `set<widget *>`, and the only public question is
      `isPersistentEditorOpen(index)` — one index at a time — so a toolkit view
      cannot be asked what it has open: you must already know in order to ask.
  (B) persistence is a property OF THE EDITOR, and an editor is not what a
      trigger on another cell replaces.
  (C) FOCUS IS DATA. In the toolkit this is `focusWidget()` reverse-mapped
      through a private hash — not answerable through the view's public API.
  (D) each editor's in-flight value is readable WITHOUT focusing it, and each
      one parks and restores its own text.
  (E) <kbd>Escape</kbd> REVERTS a persistent editor and keeps it open.
      `closeEditor` returns early for a persistent editor, so
      Escape there does nothing at all and the original is unrecoverable.
  (F) `dirty` is per editor. abstract item view keeps no record of what
      `setEditorData` seeded, so there is nothing there to compare against.
  (G) the COST IS WINDOWED: an editor on a row outside the painted window
      contributes no scene node and keeps its value. The toolkit's
      `updateEditorGeometries()` walks every persistent editor on every scroll.
  (H) every open editor reaches assistive technology with its form's role — not
      only the focused one.
  (I) commit-every-open-editor lands the good ones and leaves a model refusal
      dirty and open; a persistent editor SURVIVES its commit and is reseeded.
  (J) a malformed buffer carries a null value, named rather than absent.
  (K) `openPersistentEditor` on a cell the model will not edit is REFUSED.
      the toolkit's `createEditor` never consults `flags() & ItemIsEditable`: it
      opens a live editor on a read-only cell and drops every write in silence.
  (L) a transient editor beside them is still replaced by the next one, and a
      transient editor is PROMOTED in place, keeping what was typed into it.

ZERO-FLAKE: bounded `wait_snap` / `wait_until` polling, never a fixed sleep.
>=30 assertions.

Run from the workspace root:
    cargo build -p hello-cell-editors --release
    python3 tools/demos/r1571_editor_persistence_is_a_property.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    chord_click,
    find_by_tag,
    run_demo,
    texts_of,
    wait_snap,
    wait_until,
)

WIN = (900, 420)
TABLE_TAG = "cells"
STATUS_TAG = "cells_status"
EDIT_KEY = "cells_edit"
EDIT_FIELD_TAG = "cells_editor"
SCROLL_KEY = "cells_scroll"
MIDDOT = "·"

ASSET_COL = 0  # not editable
NAME_COL = 1  # Text   -> field
COUNT_COL = 2  # Int    -> stepper
ACTIVE_COL = 4  # Bool   -> toggle

COUNT_MAX = 999
ROW_H = 38
FAR_ROW = 50


def cell_tag(row: int, col: int) -> str:
    return f"{TABLE_TAG}#{row}_{col}"


def status(snap: Any) -> str:
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "status line painted"
    return " ".join(texts_of(node))


def clause(snap: Any, name: str) -> str:
    for part in status(snap).split(MIDDOT):
        part = part.strip()
        if part.startswith(f"{name} "):
            return part[len(name) + 1 :].strip()
    raise AssertionError(f"no {name!r} clause in {status(snap)!r}")


def editors(tf: Any) -> Any:
    """`scene/grid_editors` — the whole open set, in one call."""
    return tf.request("scene/grid_editors", {"tag": EDIT_KEY}).result


def by_cell(reply: Any) -> dict[tuple[int, int], Any]:
    return {(e["row"], e["col"]): e for e in reply["editors"]}


def entry(tf: Any, row: int, col: int) -> Any:
    found = by_cell(editors(tf)).get((row, col))
    assert found is not None, f"an editor is open on {row}_{col}"
    return found


def open_persistent(tf: Any, row: int, col: int, desc: str) -> Any:
    """The binding's persistent-editor gesture: a modified activation."""
    chord_click(tf, cell_tag(row, col), ctrl=True)
    return wait_until(lambda: (row, col) in by_cell(editors(tf)), desc=desc)


def focus_editor(tf: Any, row: int, col: int, desc: str) -> None:
    """A plain click on a cell that already has an editor focuses it."""
    tf.click(path=cell_tag(row, col))
    wait_until(
        lambda: editors(tf)["focused"] == {"row": row, "col": col},
        desc=desc,
    )


def type_into_field(tf: Any, text: str) -> None:
    """Replace the focused editor's buffer through the shared inline field."""
    tf.request("scene/set_text", {"tag": EDIT_FIELD_TAG, "text": text})


def body() -> None:
    with RpcSubprocess("hello-cell-editors", boot_grace=1.5) as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, cell_tag(0, NAME_COL)) is not None,
            desc="the grid paints at boot",
            viewport=WIN,
        )

        # ── (A) the set is enumerable, and empty is a real answer ─────
        boot = editors(tf)
        assert_eq(boot["tag"], EDIT_KEY, "the request's tag is echoed")
        assert_eq(boot["field_tag"], EDIT_FIELD_TAG, "and the shared buffer named")
        assert_eq(boot["count"], 0, "no editor is open at boot")
        assert_eq(boot["editors"], [], "and the set is empty, not absent")
        assert_eq(boot["focused"], None, "nothing holds the keyboard")
        assert_eq(clause(snap, "editing"), "none")
        # A grid that does not exist is a different answer from one with no
        # editors — the toolkit's `isPersistentEditorOpen` answers `false` to both.
        try:
            tf.request("scene/grid_editors", {"tag": "no_such_grid"})
            raise AssertionError("an unbound tag must be an error, not an empty set")
        except RpcError as err:
            assert_eq(err.data, "NotBound", "and it names why")

        # ── (B) two persistent editors, open at once ─────────────────
        open_persistent(tf, 1, NAME_COL, "a persistent editor opens on 1_1")
        one = editors(tf)
        assert_eq(one["count"], 1)
        assert_eq(one["editors"][0]["persistence"], "persistent")
        assert_eq(one["editors"][0]["form"], "field")
        assert_eq(one["focused"], {"row": 1, "col": NAME_COL}, "and takes the keyboard")
        type_into_field(tf, "alpha")
        wait_until(
            lambda: entry(tf, 1, NAME_COL)["value"] == "alpha",
            desc="the focused editor's buffer reaches the wire",
        )

        open_persistent(tf, 3, COUNT_COL, "a SECOND persistent editor opens on 3_2")
        two = editors(tf)
        assert_eq(two["count"], 2, "both are open — Qt cannot be asked this at all")
        assert_eq(
            [(e["row"], e["col"]) for e in two["editors"]],
            [(1, NAME_COL), (3, COUNT_COL)],
            "canonical cell order, so two equal sets serialize identically",
        )
        assert_eq(len([e for e in two["editors"] if e["focused"]]), 1, "one keyboard")

        # ── (C)+(D) focus is data, and each editor parks its own text ─
        assert_eq(two["focused"], {"row": 3, "col": COUNT_COL}, "the newest has it")
        parked = by_cell(two)[(1, NAME_COL)]
        assert_eq(parked["focused"], False)
        assert_eq(
            parked["value"],
            "alpha",
            "an unfocused editor's in-flight value is readable WITHOUT "
            "focusing it — the shared field holds somebody else's text",
        )
        assert_eq(parked["seed"], "asset_001", "and what it opened with")
        assert_eq(parked["dirty"], True)
        stepper = by_cell(two)[(3, COUNT_COL)]
        assert_eq(stepper["form"], "stepper", "the form follows the datum")
        assert_eq(stepper["dirty"], False, "untouched")

        type_into_field(tf, "77")
        wait_until(
            lambda: entry(tf, 3, COUNT_COL)["value"] == "77",
            desc="the focused stepper takes the typing",
        )
        assert_eq(
            entry(tf, 1, NAME_COL)["value"],
            "alpha",
            "and the parked editor's text is untouched by it",
        )

        focus_editor(tf, 1, NAME_COL, "focus goes back to the first editor")
        back = editors(tf)
        assert_eq(back["count"], 2, "focusing closes nothing")
        assert_eq(by_cell(back)[(1, NAME_COL)]["value"], "alpha", "restored")
        assert_eq(by_cell(back)[(3, COUNT_COL)]["value"], "77", "and the other parked")
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, EDIT_FIELD_TAG) is not None,
            desc="the live field is inside the focused editor's cell",
            viewport=WIN,
        )
        focused_cell = find_by_tag(snap, cell_tag(1, NAME_COL))
        assert focused_cell is not None
        assert find_by_tag(focused_cell, EDIT_FIELD_TAG) is not None, (
            "the shared field is painted in the FOCUSED editor's cell"
        )
        other_cell = find_by_tag(snap, cell_tag(3, COUNT_COL))
        assert other_cell is not None
        assert find_by_tag(other_cell, EDIT_FIELD_TAG) is None, (
            "and not in the unfocused one — a second live field there would be "
            "this cell's tag showing another cell's buffer"
        )
        assert "77" in " ".join(texts_of(other_cell)), (
            "which still paints its own parked text, as an unfocused QLineEdit "
            "shows its own with no caret"
        )

        # ── (E) Escape reverts a persistent editor, and keeps it open ─
        tf.key(path=EDIT_FIELD_TAG, name="Escape")
        reverted = wait_until(
            lambda: entry(tf, 1, NAME_COL)["value"] == "asset_001",
            desc="Escape reverts the persistent editor to its seed",
        )
        del reverted
        after_escape = editors(tf)
        assert_eq(
            after_escape["count"],
            2,
            "and it is still open — Qt's closeEditor returns early for a "
            "persistent editor, so Escape there does nothing at all",
        )
        assert_eq(by_cell(after_escape)[(1, NAME_COL)]["dirty"], False, "clean again")

        # ── (F)+(J) dirty and malformed, per editor ──────────────────
        type_into_field(tf, "12a")
        malformed = wait_until(
            lambda: entry(tf, 1, NAME_COL)["value"] is not None
            or entry(tf, 1, NAME_COL)["malformed"],
            desc="the buffer reaches the wire",
        )
        del malformed
        assert_eq(
            entry(tf, 1, NAME_COL)["value"],
            "12a",
            "a Text cell takes any string",
        )
        focus_editor(tf, 3, COUNT_COL, "focus the Int editor to malform it")
        type_into_field(tf, "12a")
        wait_until(
            lambda: entry(tf, 3, COUNT_COL)["malformed"] is True,
            desc="a half-typed number is named malformed",
        )
        bad = entry(tf, 3, COUNT_COL)
        assert_eq(bad["value"], None, "and carries NO value, rather than a blank")
        assert_eq(bad["dirty"], True, "a malformed buffer is unsaved work")

        # ── (G) the cost is windowed ─────────────────────────────────
        tf.request(
            "scene/set_scroll_offset",
            {"tag": SCROLL_KEY, "x": 0, "y": FAR_ROW * ROW_H},
        )
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, cell_tag(FAR_ROW, NAME_COL)) is not None,
            desc=f"row {FAR_ROW} scrolls into the window",
            viewport=WIN,
        )
        assert find_by_tag(snap, cell_tag(1, NAME_COL)) is None, (
            "and row 1 is windowed out"
        )
        open_persistent(tf, FAR_ROW, NAME_COL, "a third editor, far down the model")
        type_into_field(tf, "far")
        wait_until(
            lambda: entry(tf, FAR_ROW, NAME_COL)["value"] == "far",
            desc="which takes its own text",
        )
        assert_eq(editors(tf)["count"], 3)
        tf.request("scene/set_scroll_offset", {"tag": SCROLL_KEY, "x": 0, "y": 0})
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, cell_tag(0, NAME_COL)) is not None,
            desc="back to the top of the model",
            viewport=WIN,
        )
        assert find_by_tag(snap, cell_tag(FAR_ROW, NAME_COL)) is None, (
            "the far editor's cell is not painted at all"
        )
        assert_eq(
            entry(tf, FAR_ROW, NAME_COL)["value"],
            "far",
            "yet its in-flight value survives being scrolled out — Qt keeps a "
            "live QWidget for it and repositions it on every scroll",
        )
        assert_eq(editors(tf)["count"], 3, "and it is still in the set")

        # ── (H) every open editor reaches assistive technology ────────
        for row, col, role in ((1, NAME_COL, "textbox"), (3, COUNT_COL, "spinbutton")):
            access = tf.request("scene/access").result
            node = access_node_by_tag(access, f"{cell_tag(row, col)}#editor")
            assert node is not None, f"editor {row}_{col} is announced"
            assert_eq(
                node["role"],
                role,
                "with the role its FORM has, focused or not — an AT told about "
                "only the focused one would describe a grid that is not there",
            )

        # ── (K) a read-only cell refuses an editor, by name ───────────
        chord_click(tf, cell_tag(2, ASSET_COL), ctrl=True)
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "not_editable",
            desc="the identity column refuses a persistent editor and says so",
            viewport=WIN,
        )
        assert_eq(
            editors(tf)["count"],
            3,
            "and nothing opened — Qt's createEditor never consults "
            "flags() & ItemIsEditable, so it opens a live editor there and "
            "drops every write the user makes into it in silence",
        )

        # ── (L) a transient editor beside them, and a promotion ───────
        tf.double_click(path=cell_tag(5, NAME_COL))
        wait_until(
            lambda: (5, NAME_COL) in by_cell(editors(tf)),
            desc="a double-click opens a TRANSIENT editor",
        )
        assert_eq(entry(tf, 5, NAME_COL)["persistence"], "transient")
        assert_eq(editors(tf)["count"], 4)
        tf.double_click(path=cell_tag(6, NAME_COL))
        replaced = wait_until(
            lambda: (6, NAME_COL) in by_cell(editors(tf)),
            desc="a second trigger opens another transient editor",
        )
        del replaced
        after = editors(tf)
        assert_eq(
            (5, NAME_COL) in by_cell(after),
            False,
            "which REPLACED the first — one transient editor at a time",
        )
        assert_eq(
            len([e for e in after["editors"] if e["persistence"] == "persistent"]),
            3,
            "and replaced none of the persistent ones",
        )
        type_into_field(tf, "half typed")
        wait_until(
            lambda: entry(tf, 6, NAME_COL)["value"] == "half typed",
            desc="typing into the transient editor",
        )
        # Ctrl+E rather than Ctrl+click, because the FOCUSED editor's live
        # field covers its cell — a click at the cell's centre lands on the
        # field, as it does on a toolkit editor widget, so the keyboard is the
        # way in.
        tf.modifiers(ctrl=True)
        tf.key(path=EDIT_FIELD_TAG, name="e")
        tf.modifiers()
        promoted = wait_until(
            lambda: entry(tf, 6, NAME_COL)["persistence"] == "persistent",
            desc="Ctrl+E PROMOTES it in place",
        )
        del promoted
        assert_eq(
            entry(tf, 6, NAME_COL)["value"],
            "half typed",
            "keeping what was typed — Qt reaches the same end by inserting the "
            "existing widget into its private set, and reports nothing",
        )
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "promoted",
            desc="and the outcome is NAMED, where openPersistentEditor is void",
            viewport=WIN,
        )

        # ── (I) commit every open editor at once ─────────────────────
        focus_editor(tf, 3, COUNT_COL, "focus the Int editor")
        type_into_field(tf, str(COUNT_MAX + 1))
        wait_until(
            lambda: entry(tf, 3, COUNT_COL)["value"] == str(COUNT_MAX + 1),
            desc="a value the MODEL will refuse",
        )
        tf.modifiers(ctrl=True)
        tf.key(path=TABLE_TAG, name="s")
        tf.modifiers()
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit").startswith("committed "),
            desc="Ctrl+S commits every open editor",
            viewport=WIN,
        )
        landed = clause(snap, "commit")
        assert_eq(
            landed,
            "committed 3/4",
            "three landed and the out-of-range one did not — the walk Qt "
            "cannot make, because its editor hash is private",
        )
        survivors = editors(tf)
        assert_eq(
            survivors["count"],
            4,
            "every persistent editor SURVIVED its commit, as Qt's do",
        )
        assert_eq(
            by_cell(survivors)[(1, NAME_COL)]["dirty"],
            False,
            "and a committed one is reseeded, so it is no longer dirty — Qt "
            "keeps no seed at all, so a second commit has nothing to compare",
        )
        refused = by_cell(survivors)[(3, COUNT_COL)]
        assert_eq(refused["dirty"], True, "the refused one is still dirty")
        assert_eq(
            refused["value"],
            str(COUNT_MAX + 1),
            "still holding the typed value, which is the only state the user "
            "can correct it from",
        )
        assert_eq(
            find_by_tag(snap, cell_tag(FAR_ROW, NAME_COL)),
            None,
            "and the far editor committed without ever being painted",
        )


if __name__ == "__main__":
    run_demo("r1571 an editor's persistence is a property of the editor", body)
