#!/usr/bin/env python3
"""R1923 §5.12 §2 #7 — **a card says what it is, and which of its two sources
said so.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability two reference-census rows name and neither covered —
a node asked for its tooltip text, and a node type asked to describe a given
node — through the node lab as it is mounted in the shell.

# ★★★★★ The property the reference cannot express

Its node tooltip hook hands back a bare string, and its own default returns the
class's, so a caller there is given one value and cannot tell *a person wrote
this about this node* from *this is what nodes of this sort are*. Those are
different facts: the first is editable and belongs to this node, the second is
not and belongs to every node of the kind. An editor that cannot separate them
cannot offer "clear the note", and cannot say whether there is a note to clear.

Here every sentence arrives with its SOURCE, and this walk holds that over the
wire — including the transition in both directions, which is where a
flattened answer would be indistinguishable from a correct one.

# What this walk holds

  (A) every card is described, and each says which source spoke. On this
      screen that is `kind` to begin with, because the lab's roles all carry
      the one line the palette shows.
  (B) ★ that line is the PALETTE'S line, not a second sentence written for
      nodes — checked against what the palette itself publishes, so a screen
      that grew a second description would fail here rather than look right.
  (C) ★★★★★ a note written over the wire takes precedence AND the source flips
      to `authored`. Precedence alone is not the property: a flattened answer
      would show the same sentence and be wrong about who said it.
  (D) ★★★★★ clearing the note returns BOTH halves — the kind's line comes back
      and the source flips to `kind`. That is the transition the reference
      cannot report, and asserting only (C) would leave it unmeasured.
  (E) an empty note is REFUSED rather than silently clearing, because clearing
      is a thing a caller means on purpose and `none` is how they say it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1923_a_card_says_who_described_it.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def notes(app: RpcSubprocess, surface: str) -> dict:
    return {row["node"]: row for row in js(app.query(f"{surface}/notes"))["nodes"]}


def cards(app: RpcSubprocess) -> list[str]:
    return sorted(
        tag.removeprefix("lab.node.")
        for tag in abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
        if tag.startswith("lab.node.") and tag.count(".") == 2
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)
        drawn = cards(app)
        ok(f"the canvas draws cards to describe — {len(drawn)}", len(drawn) >= 2)
        subject = drawn[0]

        banner("A — every card is described, and says which source spoke")
        rows = notes(app, surface)
        ok(
            f"A: ★ every drawn card has a row — {sorted(set(drawn) - set(rows))} missing",
            set(drawn) <= set(rows),
        )
        for name, row in sorted(rows.items()):
            ok(
                f"A: {name} carries a sentence — {row['sentence']!r}",
                bool(row["sentence"]),
            )
            ok(
                f"A: and names its source — {row['source']!r}",
                row["source"] in {"authored", "kind"},
            )
        ok(
            "A: ★ with nothing written on them, every card speaks from its KIND",
            all(row["source"] == "kind" for row in rows.values()),
        )

        banner("B — the kind's line is the PALETTE'S line, not a second sentence")
        # ★★★★★ Checked against what the palette itself publishes. A screen that
        # grew a separate description for nodes would still satisfy (A) — every
        # card described, every source named — and be wrong in the way this
        # crate spends its design avoiding: two statements about one role, free
        # to drift.
        spec = js(app.query(f"{surface}/spec"))
        gists = {entry["name"]: entry.get("gist") for entry in spec["roles"]}
        ok(
            f"B: the screen's own specification publishes its roles' lines — "
            f"{len(gists)}",
            len(gists) >= 4,
        )
        said = {row["sentence"] for row in rows.values()}
        ok(
            f"B: ★★★★★ and every card's sentence IS one of them — {sorted(said)} "
            f"against {sorted(v for v in gists.values() if v)}",
            said <= {v for v in gists.values() if v},
        )

        banner("C — ★★★★★ a note takes precedence AND flips the source")
        was = rows[subject]["sentence"]
        mine = "the one I keep having to restart"
        ok(
            f"C: the verb answers the source the card now speaks from — "
            f"{app.invoke(f'{surface}/note', f'{subject},{mine}')}",
            app.invoke(f"{surface}/note", f"{subject},{mine}") == "authored",
        )
        app.tick_ms(16)
        now = notes(app, surface)[subject]
        ok(f"C: the sentence is mine — {now['sentence']!r}", now["sentence"] == mine)
        ok(
            "C: ★★★★★ and the source says a PERSON wrote it — the half a bare "
            f"string cannot carry — {now['source']!r}",
            now["source"] == "authored",
        )
        ok(
            "C: ★ and only that card changed",
            all(
                notes(app, surface)[other]["source"] == "kind"
                for other in drawn
                if other != subject
            ),
        )

        banner("D — ★★★★★ clearing returns BOTH halves")
        ok(
            "D: the verb answers `kind` when the note goes",
            app.invoke(f"{surface}/note", f"{subject},none") == "kind",
        )
        app.tick_ms(16)
        back = notes(app, surface)[subject]
        ok(
            f"D: the kind's line is back — {back['sentence']!r} was {was!r}",
            back["sentence"] == was,
        )
        ok(
            "D: ★★★★★ and the source flipped back — asserting only the sentence "
            "would leave this transition unmeasured, and it is exactly the one "
            "a flattened answer gets wrong",
            back["source"] == "kind",
        )

        banner("E — an empty note is refused, not silently a clear")
        try:
            app.invoke(f"{surface}/note", f"{subject},")
            refused = False
            told = ""
        except Exception as why:  # noqa: BLE001 - the refusal is the subject
            refused = True
            told = str(why)
        ok(f"E: ★ it is refused — {told[:90]}", refused)
        ok(
            "E: and the card is untouched",
            notes(app, surface)[subject]["source"] == "kind",
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1923_a_card_says_who_described_it", body))
