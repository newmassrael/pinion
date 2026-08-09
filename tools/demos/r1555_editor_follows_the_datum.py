#!/usr/bin/env python3
"""R1555 §5.27 §5.12 §5.40 — the cell-editor FACTORY, over the wire.

R1532 gave the virtualized grid a per-column paint delegate and R1544 gave it
the editing seam. Both are the *override* half of the toolkit's editing decomposition
(`setItemDelegateForColumn`). The other half — item editor factory, a registry
from the datum's TYPE to an editor, which styled item delegate consults when no
delegate overrides it — did not exist, so one inline text field was the built-in
editor for all six `CellKind`s. For two of the six that editor cannot work at
all: `Bool` and `Choice` refuse every keystroke and parse to nothing, so the seam
opened a field that could not be typed into and whose commit could never produce
a value.

This drives `hello-cell-editors`, one column per kind, and proves each piece from
the outside:

  (A) the factory is ENUMERABLE — `scene/cell_editors` publishes kind -> form
      plus the behaviour that follows. The toolkit cannot be asked at all:
      `createEditor` *instantiates a widget*, and `creatorMap` is private, so
      there is no "which types do you handle".
  (B) the form follows the DATUM, not the column position — each column opens
      the editor its kind names, and the read-only column opens none.
  (C) each form paints its own affordances: a field, a stepper's two arrows, a
      checkbox reading On/Off, a selector's option label, a swatch's hex.
  (D) the stepper's arrows are ADDRESSABLE and step over the wire — the only
      editor sub-part with its own address, because up and down are two targets
      inside one cell with nothing else to tell them apart.
  (E) every commit outcome is NAMED: `malformed` (the model was never asked),
      `refused` (it was, and said no), `committed`. The toolkit's `commitData` discards
      `setData`'s verdict and its validators make the malformed case
      unreachable — so both failures are the same non-event there.
  (F) a toggle edit is REVERTIBLE. The toolkit's `editorEvent` calls `setModelData` on
      the click, so a mis-click on a toolkit check column is already committed.
  (G) a selector keeps its DOMAIN across a commit — the options are part of the
      value, which is why an edit role carries a datum and why the toolkit's type-keyed
      factory cannot populate a combo box for an enumerated cell.
  (H) the open editor reaches assistive technology with its form's role. The toolkit
      reaches a role by accident of construction, so a toolkit bool cell announces
      as a COMBO BOX and a toolkit colour cell announces nothing at all.
  (I) a float survives an untouched open-and-commit. The toolkit's default factory hands
      a double spin box left at `decimals() == 2`, which silently rounds.

ZERO-FLAKE: bounded `wait_snap` / `wait_until` polling, never a fixed sleep.
>=30 assertions.

Run from the workspace root:
    cargo build -p hello-cell-editors --release
    python3 tools/demos/r1555_editor_follows_the_datum.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
    wait_snap,
    wait_until,
)

WIN = (900, 420)
TABLE_TAG = "cells"
STATUS_TAG = "cells_status"
EDIT_FIELD_TAG = "cells_editor"
MIDDOT = "·"

# Column roles, mirroring the binding's COL_KINDS.
ASSET_COL = 0  # not editable
NAME_COL = 1  # Text   -> field
COUNT_COL = 2  # Int    -> stepper
RATIO_COL = 3  # Float  -> stepper
ACTIVE_COL = 4  # Bool   -> toggle
TIER_COL = 5  # Choice -> selector
TINT_COL = 6  # Color  -> swatch

COUNT_MAX = 999
TIERS = ("Draft", "Review", "Final")
ROW = 1


def cell_tag(row: int, col: int) -> str:
    return f"{TABLE_TAG}#{row}_{col}"


def step_tag(row: int, col: int, up: bool) -> str:
    return f"{TABLE_TAG}#{'su' if up else 'sd'}{row}_{col}"


def cell_texts(snap: Any, row: int, col: int) -> list[str]:
    node = find_by_tag(snap, cell_tag(row, col))
    assert node is not None, f"cell {row}_{col} is painted"
    return texts_of(node)


def has_field(snap: Any, row: int, col: int) -> bool:
    """Whether the inline text field is inside this cell's subtree."""
    node = find_by_tag(snap, cell_tag(row, col))
    return node is not None and find_by_tag(node, EDIT_FIELD_TAG) is not None


def status(snap: Any) -> str:
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "status line painted"
    return " ".join(texts_of(node))


def clause(snap: Any, name: str) -> str:
    """The `<name> ...` clause of the middot-separated status line."""
    for part in status(snap).split(MIDDOT):
        part = part.strip()
        if part.startswith(f"{name} "):
            return part[len(name) + 1 :].strip()
    raise AssertionError(f"no {name!r} clause in {status(snap)!r}")


def editing(snap: Any) -> str:
    return clause(snap, "editing")


def form_of(snap: Any) -> Optional[str]:
    """The form token out of the `editing <row>_<col> <form> "<value>"` clause."""
    parts = editing(snap).split(" ", 2)
    return None if len(parts) < 2 else parts[1]


def in_flight(snap: Any) -> Optional[str]:
    """The quoted in-flight value out of the editing clause."""
    text = editing(snap)
    first = text.find('"')
    last = text.rfind('"')
    return None if first < 0 or last <= first else text[first + 1 : last]


def open_editor_on(tf: Any, col: int, desc: str) -> Any:
    """Double-click a cell (Qt `DoubleClicked`) and wait for its latch."""
    tf.double_click(path=cell_tag(ROW, col))
    return wait_snap(
        tf,
        lambda s: editing(s).startswith(f"{ROW}_{col} "),
        desc=desc,
        viewport=WIN,
    )


def close_editor(tf: Any) -> Any:
    tf.key(path=TABLE_TAG, name="Escape")
    return wait_snap(
        tf,
        lambda s: editing(s) == "none",
        desc="the editor closes",
        viewport=WIN,
    )


def body() -> None:
    with RpcSubprocess("hello-cell-editors", boot_grace=1.5) as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, cell_tag(0, TINT_COL)) is not None,
            desc="the grid paints every column at boot",
            viewport=WIN,
        )
        assert_eq(editing(snap), "none", "no editor is open at boot")
        assert_eq(clause(snap, "commit"), "none", "and nothing has committed")

        # ── (A) the factory is enumerable ────────────────────────────
        census = tf.request("scene/cell_editors").result
        rows = census["editors"]
        assert_eq(len(rows), 6, "one row per CellKind — the answer is a census")
        by_kind = {r["kind"]: r for r in rows}
        assert_eq(
            sorted(by_kind),
            ["bool", "choice", "color", "float", "int", "text"],
            "every kind is named",
        )
        assert_eq(by_kind["text"]["form"], "field")
        assert_eq(by_kind["int"]["form"], "stepper")
        assert_eq(by_kind["float"]["form"], "stepper")
        assert_eq(by_kind["bool"]["form"], "toggle")
        assert_eq(by_kind["choice"]["form"], "selector")
        assert_eq(
            by_kind["color"]["form"],
            "swatch",
            "Qt's default factory has NO QColor creator, so a colour cell in a "
            "plain QTableView is not editable at all",
        )
        # The behaviour that follows, which the toolkit keeps inside each
        # delegate's qobject_cast: where the in-flight value lives, and what
        # may be typed.
        assert by_kind["bool"]["buffer_is_text"] is False
        assert by_kind["choice"]["buffer_is_text"] is False
        assert by_kind["color"]["buffer_is_text"] is True, "its hex half"
        assert by_kind["color"]["inline_text"] is False, (
            "but the cell's own box is not a field — a printable key at the "
            "cell must not start a hex edit"
        )
        assert by_kind["bool"]["accepts_keystrokes"] is False
        assert by_kind["choice"]["accepts_keystrokes"] is False
        assert by_kind["int"]["accepts_keystrokes"] is True
        # And the census is self-consistent: a kind that takes no typing cannot
        # have a text buffer to type into.
        for kind, row in by_kind.items():
            if not row["accepts_keystrokes"]:
                assert row["inline_text"] is False, (
                    f"{kind} takes no typing, so its cell is not a text field"
                )

        # ── (B) the form follows the datum ───────────────────────────
        expected_forms = {
            NAME_COL: "field",
            COUNT_COL: "stepper",
            RATIO_COL: "stepper",
            ACTIVE_COL: "toggle",
            TIER_COL: "selector",
            TINT_COL: "swatch",
        }
        for col, form in expected_forms.items():
            snap = open_editor_on(tf, col, f"column {col} opens an editor")
            assert_eq(form_of(snap), form, f"column {col} opens the {form} form")
            assert_eq(
                by_kind[
                    {
                        NAME_COL: "text",
                        COUNT_COL: "int",
                        RATIO_COL: "float",
                        ACTIVE_COL: "bool",
                        TIER_COL: "choice",
                        TINT_COL: "color",
                    }[col]
                ]["form"],
                form,
                f"and it is the form the published census names for that kind",
            )
            snap = close_editor(tf)

        # The read-only column produces no edit role, so no trigger opens one.
        tf.double_click(path=cell_tag(ROW, ASSET_COL))
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(
            editing(snap), "none", "the identity column opens no editor at all"
        )

        # ── (C) each form paints its own affordances ─────────────────
        snap = open_editor_on(tf, NAME_COL, "the text column opens a field")
        assert has_field(snap, ROW, NAME_COL), "a field form hosts the text field"
        assert not has_field(snap, ROW, COUNT_COL), "its neighbour keeps display"
        snap = close_editor(tf)

        snap = open_editor_on(tf, ACTIVE_COL, "the bool column opens a toggle")
        assert not has_field(snap, ROW, ACTIVE_COL), (
            "a toggle holds its value in the latch, so it paints NO text field "
            "— which is exactly what the pre-R1555 built-in editor got wrong"
        )
        painted = cell_texts(snap, ROW, ACTIVE_COL)
        assert any(t in ("On", "Off") for t in painted), (
            f"the checkbox states itself: {painted}"
        )
        snap = close_editor(tf)

        snap = open_editor_on(tf, TIER_COL, "the choice column opens a selector")
        assert not has_field(snap, ROW, TIER_COL), "a selector paints no field"
        painted = cell_texts(snap, ROW, TIER_COL)
        assert any(t in TIERS for t in painted), (
            f"the selected option is painted from the datum's own domain: {painted}"
        )
        snap = close_editor(tf)

        snap = open_editor_on(tf, TINT_COL, "the colour column opens a swatch")
        assert has_field(snap, ROW, TINT_COL), "a swatch's hex half IS a field"
        assert (in_flight(snap) or "").startswith("#"), (
            f"seeded with the hex form: {in_flight(snap)!r}"
        )
        snap = close_editor(tf)

        # ── (D) the stepper's arrows are addressable and step ────────
        snap = open_editor_on(tf, COUNT_COL, "the int column opens a stepper")
        assert has_field(snap, ROW, COUNT_COL), "a stepper hosts the field"
        # Each arrow is asserted to paint the glyph its ADDRESS claims. Checking
        # only that both tags exist would round-trip through the grammar under
        # test: swapping the two prefixes in `GridSendKey::encode` leaves both
        # tags present and both decodable, and the demo passed (measured).
        for up, glyph in ((True, "\u25b2"), (False, "\u25bc")):
            node = find_by_tag(snap, step_tag(ROW, COUNT_COL, up))
            assert node is not None, (
                f"the {'up' if up else 'down'} arrow is painted AND addressable"
            )
            assert_eq(
                texts_of(node),
                [glyph],
                f"and the {'up' if up else 'down'} ADDRESS draws the "
                f"{'up' if up else 'down'} triangle",
            )
        before = int(in_flight(snap) or "0")
        tf.click(path=step_tag(ROW, COUNT_COL, True))
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) == str(before + 1),
            desc="a click on the up arrow steps the buffer",
            viewport=WIN,
        )
        tf.click(path=step_tag(ROW, COUNT_COL, False))
        tf.click(path=step_tag(ROW, COUNT_COL, False))
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) == str(before - 1),
            desc="and the down arrow steps back past the seed",
            viewport=WIN,
        )
        assert_eq(
            form_of(snap), "stepper", "still the same open editor, not a reopen"
        )
        # An arrow is a hit target of its own, so pressing it moved focus to
        # the grid until R1555 handed it back. The toolkit cannot have this bug
        # — its arrows are sub-controls of one focus widget — and without the
        # hand-back the keystroke in section (E) below silently goes nowhere.
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == EDIT_FIELD_TAG,
            desc="stepping leaves focus on the field it just wrote",
        )

        # ── (E) every commit outcome is named ───────────────────────
        # The model's DISPLAY text is read with the editor CLOSED: while one is
        # open the cell paints the editor, so reading it there would compare the
        # buffer with itself. (It did, on this round's first demo run.)
        snap = close_editor(tf)
        model_before = cell_texts(snap, ROW, COUNT_COL)
        snap = open_editor_on(tf, COUNT_COL, "reopen the int cell")
        assert "\u25b2" in "".join(cell_texts(snap, ROW, COUNT_COL)), (
            "and a reopened stepper paints its arrows again"
        )
        # `malformed`: the buffer is not a value of the cell's kind, so the model is
        # NEVER asked. The toolkit's validator makes this unreachable, at the
        # price of committing a value the user did not type.
        for _ in range(6):
            tf.key(path=TABLE_TAG, name="Backspace")
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) == "<malformed>",
            desc="an empty int buffer holds no value of its kind",
            viewport=WIN,
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "malformed",
            desc="the commit names the failure",
            viewport=WIN,
        )
        assert_eq(
            form_of(snap), "stepper", "and the editor is still open on the cell"
        )

        # `refused`: well-formed, and the model says no.
        tf.text(str(COUNT_MAX + 1), path=TABLE_TAG)
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) == str(COUNT_MAX + 1),
            desc="an out-of-range value is typed",
            viewport=WIN,
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "refused",
            desc="the model refused it, and the wire says which failure it was",
            viewport=WIN,
        )
        assert_eq(
            in_flight(snap),
            str(COUNT_MAX + 1),
            "holding exactly what the user typed, so they can correct it",
        )
        # Abandon it and read the model with the editor closed: neither the
        # malformed buffer nor the refused value reached it.
        snap = close_editor(tf)
        assert_eq(
            cell_texts(snap, ROW, COUNT_COL),
            model_before,
            "neither failure touched the model",
        )

        # `committed`.
        snap = open_editor_on(tf, COUNT_COL, "reopen to commit a valid value")
        for _ in range(6):
            tf.key(path=TABLE_TAG, name="Backspace")
        tf.text("42", path=TABLE_TAG)
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) == "42",
            desc="a valid value replaces it",
            viewport=WIN,
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "committed",
            desc="the in-range value commits",
            viewport=WIN,
        )
        assert_eq(editing(snap), "none", "and the editor closed")
        assert "42" in cell_texts(snap, ROW, COUNT_COL), (
            f"the committed datum is what the grid paints: "
            f"{cell_texts(snap, ROW, COUNT_COL)}"
        )

        # ── (F) a toggle edit is revertible ─────────────────────────
        model_before = cell_texts(snap, ROW, ACTIVE_COL)
        snap = open_editor_on(tf, ACTIVE_COL, "reopen the bool cell")
        seeded = in_flight(snap)
        tf.key(path=TABLE_TAG, name="Space")
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) != seeded,
            desc="Space flips the in-flight bool",
            viewport=WIN,
        )
        flipped = in_flight(snap)
        assert flipped in ("On", "Off") and flipped != seeded
        # And the PAINTED checkbox shows the flip. The status readout above is a
        # different derivation of the same latch, so asserting only that leaves
        # the painter unchecked — measured: making the painter read the seed
        # instead of the in-flight value passed every other assertion here.
        assert flipped in cell_texts(snap, ROW, ACTIVE_COL), (
            f"the editor paints the in-flight value, not the seed: "
            f"{cell_texts(snap, ROW, ACTIVE_COL)}"
        )
        assert seeded not in cell_texts(snap, ROW, ACTIVE_COL), (
            "and the seed is no longer on screen"
        )
        snap = close_editor(tf)
        assert_eq(
            cell_texts(snap, ROW, ACTIVE_COL),
            model_before,
            "Escape reverted it — Qt's editorEvent had already written it",
        )
        # And committing it does reach the model.
        snap = open_editor_on(tf, ACTIVE_COL, "reopen the bool cell again")
        tf.key(path=TABLE_TAG, name="Space")
        wait_snap(
            tf,
            lambda s: in_flight(s) == flipped,
            desc="flip it again",
            viewport=WIN,
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "committed",
            desc="the toggle commits through the same arc a field does",
            viewport=WIN,
        )
        assert_eq(editing(snap), "none")
        assert cell_texts(snap, ROW, ACTIVE_COL) != model_before, (
            "and the flip reached the model"
        )

        # ── (G) a selector keeps its domain across a commit ─────────
        snap = open_editor_on(tf, TIER_COL, "reopen the choice cell")
        seeded = in_flight(snap)
        assert seeded in TIERS, f"seeded with an option label: {seeded!r}"
        tf.key(path=TABLE_TAG, name="ArrowDown")
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) != seeded,
            desc="ArrowDown moves the in-flight option",
            viewport=WIN,
        )
        moved = in_flight(snap)
        assert moved in TIERS, (
            f"still an option OF THAT DOMAIN — a Qt combo built from a "
            f"type-keyed factory has no options to move through: {moved!r}"
        )
        assert moved in cell_texts(snap, ROW, TIER_COL), (
            f"and the selector PAINTS the moved option, not the seed: "
            f"{cell_texts(snap, ROW, TIER_COL)}"
        )
        assert_eq(
            TIERS.index(moved), TIERS.index(seeded) + 1, "one option down"
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "committed",
            desc="the selection commits",
            viewport=WIN,
        )
        assert moved in cell_texts(snap, ROW, TIER_COL), (
            f"the display role reads the new selection: "
            f"{cell_texts(snap, ROW, TIER_COL)}"
        )
        # Reopening proves the DOMAIN survived the write, not just the index.
        # The move is UPWARD because the commit above landed on the last option,
        # where a downward move correctly clamps — a probe has to be chosen
        # against the state it runs in, not against the property it is testing.
        snap = open_editor_on(tf, TIER_COL, "reopen it once more")
        assert_eq(in_flight(snap), moved, "the committed option is the seed")
        assert_eq(
            TIERS.index(moved), len(TIERS) - 1, "premise: it is the last option"
        )
        tf.key(path=TABLE_TAG, name="ArrowDown")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(
            in_flight(snap),
            moved,
            "a move past the end is REFUSED, not silently cleared — "
            "QComboBox::setCurrentIndex accepts an out-of-range index by "
            "clearing the selection",
        )
        tf.key(path=TABLE_TAG, name="ArrowUp")
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) != moved,
            desc="but it can still move inside the domain, so the option list "
            "came back with the committed value",
            viewport=WIN,
        )
        assert in_flight(snap) in TIERS
        snap = close_editor(tf)

        # ── (H) the open editor reaches AT with its form's role ─────
        for col, role in (
            (NAME_COL, "textbox"),
            (COUNT_COL, "spinbutton"),
            (ACTIVE_COL, "checkbox"),
            (TIER_COL, "combobox"),
            (TINT_COL, "textbox"),
        ):
            open_editor_on(tf, col, f"open column {col} for its AT role")
            access = tf.request("scene/access").result
            node = access_node_by_tag(access, f"{cell_tag(ROW, col)}#editor")
            assert node is not None, f"column {col} announces its open editor"
            assert_eq(
                node.get("role"),
                role,
                f"column {col} announces as {role} — Qt would announce a bool "
                f"cell as a combobox and a colour cell as nothing",
            )
            host = access_node_by_tag(access, cell_tag(ROW, col))
            assert host is not None and host.get("role") == "gridcell", (
                "the host stays a gridcell, so AT table navigation is intact"
            )
            close_editor(tf)
        access = tf.request("scene/access").result
        assert access_node_by_tag(access, f"{cell_tag(ROW, NAME_COL)}#editor") is None, (
            "with nothing open, no editor node exists"
        )

        # ── (I) a float survives an untouched open-and-commit ───────
        snap = open_editor_on(tf, RATIO_COL, "open the float cell")
        seeded = in_flight(snap)
        assert seeded is not None and "." in seeded, (
            f"the float seed carries its fraction: {seeded!r}"
        )
        tf.key(path=TABLE_TAG, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: clause(s, "commit") == "committed",
            desc="an untouched editor commits",
            viewport=WIN,
        )
        snap = open_editor_on(tf, RATIO_COL, "reopen it")
        assert_eq(
            in_flight(snap),
            seeded,
            "the datum is unchanged — Qt's default QDoubleSpinBox editor sits "
            "at decimals()==2, so this round trip loses precision there",
        )
        # The stepper's arrows work on a float too, and its step is 1.
        tf.click(path=step_tag(ROW, RATIO_COL, True))
        snap = wait_snap(
            tf,
            lambda s: in_flight(s) != seeded,
            desc="a float steps",
            viewport=WIN,
        )
        assert_eq(
            float(in_flight(snap) or "0"),
            float(seeded) + 1.0,
            "one step is 1, which is Qt's default single step",
        )
        close_editor(tf)

        # The binding wires no editor painter and no column delegate, so every
        # editor above came from the factory. The selection axis is untouched
        # throughout (the toolkit: current != selected).
        wait_until(
            lambda: tf.query("/external/item_count") == 60,
            desc="the coordinator still holds the whole dataset",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1555 §5.27 the editor follows the datum", body))
