#!/usr/bin/env python3
"""R1983 §5.2 §5.11 — **a folded part can be taken apart again, and cards move
across its boundary in both directions.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1981 gave the assembled tool make / enter / exit and R1982 gave it a door a
person can press. This is the other half of a subgraph's life: pull it apart,
move a card out of it, move a card into it.

# ★★★★★ The debt this finishes

`debt-the-assembled-tools-subgraph-surface-is-half-built` listed FOUR items.
R1982 paid the sharpest — the one about a person who could not get out. These
are the other three, and they are what twelve `have` rows of the reference
census say the FRAMEWORK can do while no screen could reach them:
`group_ungroup`, `group_separate`, `group_insert`.

# ⚠ What the first job of this round measured, and what it corrected

The round began by driving `debt-four-per-card-tables-are-keyed-by-a-number-…`,
whose premise R1981 registered UNMEASURED. Driven (in the crate tests, where the
tables are readable):

* the id collision is REAL — a card made inside a subgraph is minted with a
  number a root card already holds;
* the `forms` TABLE really is overwritten under that number;
* and the root card's SHOWN configuration is nevertheless unchanged, because
  `shown_form` re-derives from the node.

⇒ the debt's premise holds and its predicted CONSEQUENCE does not, on the shown
half. The corruption is latent in the stored half. That is the eighth round
running in which re-measuring a debt corrected part of its diagnosis.

# What this walk holds

  (A) the journey reaches the node lab, at the top.
  (B) ★ two cards are folded into a part — the setup, and R1981's capability.
  (C) ★★★★★ a card is moved OUT of the part without entering it, and it is on
      the canvas afterwards.
  (D) ★★★★★ a card is moved INTO the part, and it leaves the canvas.
  (E) ★★★★★ the part is UNFOLDED and its cards come back to this tree.
  (F) ★ and the part is gone — a card that no longer exists is not on the frame.
  (G) ⚠ the refusals are real: separating with nothing to separate from is
      refused, and the refusal says which.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1983_a_folded_part_is_taken_apart_again.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
PART = "capture-side"

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


def cards(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def standing(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/standing"))


def refusal(app: RpcSubprocess, path: str, args: str) -> str:
    """Drive a verb that should be refused, and answer what it said."""
    try:
        app.invoke(path, args)
    except Exception as why:  # noqa: BLE001 — the refusal is the assertion
        return str(why)
    return ""


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "A: the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)
        ok(f"A: ★ at the top — {standing(app, surface)}", standing(app, surface)["depth"] == 0)

        banner("G — ⚠ the refusal FIRST, while there is nothing to separate from")
        said = refusal(app, f"{surface}/separate", cards(app, surface)[0])
        ok(
            f"G: ★★★★★ separating at the top is refused, and the refusal says "
            f"WHY rather than only that it refused — {said[:110]}",
            "top" in said.lower(),
        )

        banner("B — ★ two cards are folded into a part")
        opening = cards(app, surface)
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", opening[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        folded = cards(app, surface)
        ok(
            f"B: ★ the part is here and the two cards are not — {folded}",
            PART in folded and opening[0] not in folded and opening[1] not in folded,
        )

        banner("D — ★★★★★ a card is moved INTO the part, from out here")
        movable = next(name for name in folded if name != PART)
        said = app.invoke(f"{surface}/insert", f"{movable},{PART}")
        app.tick_ms(16)
        after_insert = cards(app, surface)
        ok(
            f"D: ★★★★★ it has left this tree — {said!r}, {movable} in "
            f"{after_insert}",
            movable not in after_insert and PART in after_insert,
        )

        banner("C — ★★★★★ a card is moved OUT of the part, from inside it")
        app.invoke(f"{surface}/enter", PART)
        app.tick_ms(16)
        inside = cards(app, surface)
        ok(f"C: ★ the card that went in is in here — {inside}", movable in inside)
        said = app.invoke(f"{surface}/separate", movable)
        app.tick_ms(16)
        ok(
            f"C: ★★★★★ and separating it takes it out — {said!r}, now "
            f"{cards(app, surface)}",
            movable not in cards(app, surface),
        )
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        ok(
            f"C: ★★★★★ it is on the canvas out here again — {cards(app, surface)}",
            movable in cards(app, surface),
        )

        banner("E — ★★★★★ the part is unfolded and its cards come back")
        before_unfold = cards(app, surface)
        said = app.invoke(f"{surface}/ungroup", PART)
        app.tick_ms(16)
        unfolded = cards(app, surface)
        ok(
            f"E: ★★★★★ the cards that were inside are out here — {said!r}, "
            f"{len(before_unfold)} then {len(unfolded)}",
            len(unfolded) > len(before_unfold),
        )
        ok(
            f"E: ★ and both of the cards folded at (B) are back — {unfolded}",
            opening[0] in unfolded and opening[1] in unfolded,
        )

        banner("F — ★ the part itself is gone")
        ok(
            f"F: ★★★★★ a card that no longer exists is not on the frame — "
            f"{PART} in {unfolded}",
            PART not in unfolded,
        )
        ok(
            f"F: ★ and the tool is at the top, with nowhere above — "
            f"{standing(app, surface)}",
            standing(app, surface)["depth"] == 0,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1983 a folded part is taken apart again", body)
