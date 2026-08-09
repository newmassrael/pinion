#!/usr/bin/env python3
"""R1540 §5.36 §5.41 §2 #7 — a text run's underline states its FORM and colour.

R1399 gave the terminal cell the full ECMA-48 SGR 4:x underline axis — single /
double / curly / dotted / dashed — and its own doc said why the distinction is
load-bearing: *"an editor's LSP diagnostics rely on the distinction (a red curly
error vs a blue dotted spellcheck) being renderable, not flattened to one
rule."* A GUI text run kept a single `bool`.

So the same tree could draw an undercurl in a terminal and not on screen. Worse,
the painter that knew how was in the same file: `paint_underline` has drawn five
forms since R1399 for the cell grid, while `paint_decorations` — the glyph-run
path — stroked one flat rule for everything, because `TextDecoration` had no
form to give it.

R1540 moved `UnderlineStyle` to the general text-style home and gave
`TextDecoration` both axes it was missing: the FORM, and the underline's own
COLOUR (the toolkit `setUnderlineColor`, SGR 58). `hello-richtext` now carries the two
canonical diagnostic marks:

  * `"quick"` — purple bold text, under a **blue dotted** rule (a spelling hint)
  * `"brown"` — saddle-brown italic text, under a **red undercurl** (an error)

The colours are deliberately unlike the text they sit under, because that is the
whole contract: a red squiggle under otherwise normally-coloured code is one
run, not a recolouring of the code.

This demo asserts:

  (A) The wire carries the form as a TOKEN, not a bool, in the same lowercase
      vocabulary the terminal cell's `attrs.underline` has spoken since R1399.
      One enum must not have two spellings.

  (B) The two marks are DIFFERENT forms in one paragraph — the property a
      flattened painter cannot have.

  (C) The underline colour is an axis of its own: each mark's colour differs
      from the `fg_color` of the very run it decorates, and from the other
      mark's.

  (D) An undecorated run answers `"none"` with a `null` colour — the key is
      always ANSWERED rather than omitted, so a client never has to guess
      whether absence means "no underline" or "not reported".

  (E) The marks survive a view re-run. Toggling rebuilds every styled run from
      scratch, so this separates "the marks are part of the model" from
      "something stamped them once at boot" — and re-checks that every form on
      the wire is one the vocabulary declares.

ZERO-FLAKE: every assertion reads published scene data. No pixels, no
wall-clock, and no value that depends on the host — the mark colours are
literals in the binding, chosen so they cannot collide with a theme.

Run from the workspace root:
    cargo build -p hello-richtext --release
    python3 tools/demos/r1540_underline_states_its_form.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-richtext"
PARA_TAG = "rich_para"
VIEWPORT = (480, 240)

# The binding's own SSOT (examples/hello-richtext/src/main.rs).
QUICK = (4, 9)
BROWN = (10, 15)
FOX = (16, 19)
EMPH_PURPLE = 0x7C3AED
SADDLE_BROWN = 0x8B4513
DIAG_ERROR_RED = 0xE11D1D
DIAG_HINT_BLUE = 0x1D5FE1

# The complete vocabulary — a census, so a form added without a wire decision
# is a finding rather than a silent omission.
FORMS = ["none", "single", "double", "curly", "dotted", "dashed"]


def rgb(value: Any) -> int:
    """A wire colour as a 24-bit RGB int, whatever shape it arrives in."""
    if isinstance(value, int):
        return value & 0xFFFFFF
    if isinstance(value, str):
        return int(value.lstrip("#"), 16) & 0xFFFFFF
    if isinstance(value, dict):
        for key in ("rgb", "hex", "value"):
            if key in value:
                return rgb(value[key])
        if {"r", "g", "b"} <= set(value):
            return (int(value["r"]) << 16) | (int(value["g"]) << 8) | int(value["b"])
    raise AssertionError(f"unrecognised wire colour {value!r}")


def find_para(snap: Any) -> dict[str, Any]:
    node = find_by_tag(snap, PARA_TAG)
    assert node is not None, f"tag {PARA_TAG!r} present in the snapshot"
    return node


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        para = find_para(snap)
        runs = para["runs"]
        assert_eq(len(runs), 3, "the paragraph carries its three styled runs")
        by_range = {(r["start"], r["end"]): r for r in runs}
        assert_eq(
            sorted(by_range),
            [QUICK, BROWN, FOX],
            "and they cover the byte ranges the binding declares",
        )

        quick = by_range[QUICK]["style"]
        brown = by_range[BROWN]["style"]
        fox = by_range[FOX]["style"]

        # ── (A) the form is a token, in ONE vocabulary ──────────────────────
        for name, style in (("quick", quick), ("brown", brown), ("fox", fox)):
            dec = style["decoration"]
            assert_eq(
                sorted(dec),
                ["strikethrough", "underline", "underline_color"],
                f"A: {name}'s decoration answers all three keys",
            )
            assert isinstance(dec["underline"], str), (
                f"A: {name}'s underline is a FORM token, not a bool: "
                f"{dec['underline']!r}. R1399 gave the terminal cell this same "
                f"vocabulary; a GUI run using a different one would be one enum "
                f"with two spellings"
            )
            assert dec["underline"] in FORMS, (
                f"A: {name} answers {dec['underline']!r}, outside {FORMS}"
            )

        # ── (B) two different forms in one paragraph ────────────────────────
        assert_eq(
            quick["decoration"]["underline"],
            "dotted",
            "B: the spelling hint is a dotted rule",
        )
        assert_eq(
            brown["decoration"]["underline"],
            "curly",
            "B: the error is an undercurl — the form an editor draws under a "
            "diagnostic, and the one a bool could not express",
        )
        assert (
            quick["decoration"]["underline"] != brown["decoration"]["underline"]
        ), (
            "B: the two marks must be DIFFERENT forms. A painter that flattens "
            "every style to one rule still satisfies every assertion that only "
            "checks a mark is present"
        )

        # Neither mark is `single`. A bool could already express a plain rule,
        # so a demo that only showed one would pass just as well against the
        # painter this round replaced. `dotted` and `curly` are exactly the
        # forms that were unrepresentable.
        for name, style in (("quick", quick), ("brown", brown)):
            form = style["decoration"]["underline"]
            assert form not in ("none", "single"), (
                f"B: {name} must carry a form a bool could NOT express, got "
                f"{form!r} — otherwise this demo cannot tell R1540 from R1539"
            )

        # The strikethrough axis is untouched: it has one form in both SGR (9)
        # and the toolkit, so it stayed a bool and must not have been dragged
        # along.
        for name, style in (("quick", quick), ("brown", brown), ("fox", fox)):
            assert style["decoration"]["strikethrough"] is False, (
                f"B: {name}'s strikethrough must still be a bool, and false"
            )

        # ── (C) the colour is an axis of its own ────────────────────────────
        assert_eq(
            rgb(brown["decoration"]["underline_color"]),
            DIAG_ERROR_RED,
            "C: the error mark is red",
        )
        assert_eq(
            rgb(quick["decoration"]["underline_color"]),
            DIAG_HINT_BLUE,
            "C: the hint mark is blue",
        )
        assert_eq(rgb(brown["fg_color"]), SADDLE_BROWN, "C: while the prose is brown")
        assert_eq(rgb(quick["fg_color"]), EMPH_PURPLE, "C: and the emphasis purple")
        for name, style in (("quick", quick), ("brown", brown)):
            assert rgb(style["decoration"]["underline_color"]) != rgb(
                style["fg_color"]
            ), (
                f"C: {name}'s mark must not be the colour of the text it "
                f"decorates — an underline that can only take the foreground "
                f"is not a separate axis, and a diagnostic under normally "
                f"coloured code is exactly what needs one"
            )
        assert rgb(quick["decoration"]["underline_color"]) != rgb(
            brown["decoration"]["underline_color"]
        ), "C: and the two marks differ from each other"

        # ── (D) an undecorated run answers, rather than omits ───────────────
        assert_eq(
            fox["decoration"]["underline"],
            "none",
            "D: the undecorated run says so explicitly",
        )
        assert_eq(
            fox["decoration"]["underline_color"],
            None,
            "D: with a null colour — `null` is the answer 'it tracks the text "
            "colour' (Qt's default), and it is ANSWERED rather than omitted so "
            "a client never guesses whether a missing key means no underline "
            "or no report",
        )

        # ── (E) the vocabulary is closed, and the marks survive a re-run ────
        # The toggle re-runs the view and rebuilds EVERY run from scratch, so
        # this distinguishes "the marks are part of the styled-run model" from
        # "something stamped them once at boot".
        tf.click(path="main_toggle")
        assert_eq(
            tf.query("/external/value"), True, "E: the toggle actually flipped"
        )
        again = find_para(tf.snapshot(source="paint", viewport=VIEWPORT))
        again_by_range = {(r["start"], r["end"]): r for r in again["runs"]}
        assert_eq(
            again_by_range[BROWN]["style"]["decoration"]["underline"],
            "curly",
            "E: the mark survives a view re-run",
        )
        assert_eq(
            rgb(again_by_range[BROWN]["style"]["decoration"]["underline_color"]),
            DIAG_ERROR_RED,
            "E: and so does its colour",
        )
        seen = {r["style"]["decoration"]["underline"] for r in again["runs"]}
        assert seen <= set(FORMS), f"E: an undeclared form on the wire: {seen}"
        assert_eq(
            len(seen),
            3,
            "E: three runs, three distinct forms — none / dotted / curly",
        )
        assert_eq(
            again_by_range[QUICK]["style"]["decoration"]["underline"],
            "dotted",
            "E: and the hint's form too",
        )
        # The pre-existing behaviour the marks were added ALONGSIDE: the fox
        # run still flips colour on toggle. A round that broke it while the
        # underline assertions passed would be a regression this demo owns.
        assert rgb(again_by_range[FOX]["style"]["fg_color"]) != rgb(
            fox["fg_color"]
        ), "E: the toggle still restyles the fox run (R713 behaviour intact)"
        assert_eq(
            again_by_range[FOX]["style"]["decoration"]["underline"],
            "none",
            "E: while the run with no mark still has none",
        )

        # ── (F) the base style answers too, not only the runs ───────────────
        # `decoration` is a field of every `TextStyle`, so the paragraph's own
        # base style carries the axis whether or not a run overrides it. A
        # binding reading the base and a binding reading a run must not get
        # two different shapes.
        base = again["style"]["decoration"]
        assert_eq(
            sorted(base),
            ["strikethrough", "underline", "underline_color"],
            "F: the base style answers the same three keys as a run",
        )
        assert_eq(base["underline"], "none", "F: with no mark of its own")
        assert_eq(base["underline_color"], None, "F: and a null colour")


if __name__ == "__main__":
    sys.exit(run_demo("R1540 a text run's underline states its form", body))
