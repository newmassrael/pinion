#!/usr/bin/env python3
"""R1985 §5.2 §5.11 — **a copy asks the questions the original had to answer.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1981-R1983 gave the assembled tool the subgraph verbs. These are the three a
person reaches for far more often — copy, paste, duplicate — and the crate has
answered all three since R1578 while **no screen could reach any of them**.

# ★★★★★ The defect this walk exists to keep dead

Measured at this round's open, on the crate's own fixture, by driving it:

    holders   = [NodeId(2), NodeId(4)]     two cards answer to one name
    lookup    = None                       so the name addresses NEITHER
    relabel?  = Err(LabelTaken { .. })     may() refuses to CREATE that state
    validate  = 0                          and nothing reports it

`Document::duplicate` copied a label verbatim, so it built the exact state this
crate's own permission surface refuses. Two paths deriving one rule (R1977's
class), live rather than latent, and it is what R1984 repaired one *consequence*
of: a name two cards hold resolved to whichever came first.

On the screen it shows up here: a pasted card takes a name of its own, and
`select` — which resolves a card BY NAME — still finds every card on the frame.
Before the repair, pasting made both the original and the copy unaddressable.

# What this walk holds

  (A) the journey reaches the node lab, at the top of the assembled tool.
  (B) ⚠ the refusal FIRST: pasting with nothing copied is refused, and the
      refusal says what is missing rather than only that it refused.
  (C) ★ two cards are copied, and the clipboard SAYS what it holds.
  (D) ★★★★★ they are pasted and the copies are NOT called what they came from.
      This is the clause that goes red first with the repair taken out —
      measured, by rebuilding against it: the copies keep their originals'
      names, so there is nothing new on the frame to name at all.
  (E) ★★★★★ the consequence a person meets: EVERY card on the frame still
      addresses exactly one card. Driven through `select`, which resolves by
      name and answers `None` for a name two cards hold (R1984). Measured to
      have its own failing path — a batch that mints one name twice reaches
      here and not (D). Then the screen says what each copy was renamed to,
      which neither reference reports at all.
  (F) ★ duplicate puts a copy beside the original and numbers from the stem.
  (G) ★ the clipboard crosses a subgraph boundary — copied out here, pasted
      inside — which is what a clipboard is for and what a list of ids could
      not do.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1985_a_copy_takes_a_name_of_its_own.py
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


def stem(name: str) -> str:
    """The name with a trailing `-NN` taken off — the crate's own `stem_of`.

    Spelled here so the walk can CHOOSE a pair that collides. Two cards with
    different stems can never number to one name, so a walk that copies the
    opening graph's cards cannot reach the batch case at all — measured, by a
    counterfactual that broke the batch rule and was NOT caught.
    """
    head, _, tail = name.rpartition("-")
    return head if head and tail.isdigit() else name


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def cards(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def clipboard(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/clipboard"))


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

        banner("B — ⚠ the refusal FIRST, while nothing has been copied")
        held = clipboard(app, surface)
        ok(f"B: ★ the clipboard says it is empty — {held}", held["held"] is False)
        said = refusal(app, f"{surface}/paste", "")
        ok(
            f"B: ★★★★★ pasting is refused and the refusal says WHAT is missing "
            f"— {said[:110]}",
            "copied" in said.lower(),
        )

        banner("C — ★ two cards are copied")
        opening = cards(app, surface)
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", opening[1])
        app.tick_ms(16)
        said = app.invoke(f"{surface}/copy", "")
        app.tick_ms(16)
        held = clipboard(app, surface)
        ok(
            f"C: ★★★★★ the clipboard holds the two cards BY NAME — {said!r}, "
            f"{held}",
            held["held"] is True and sorted(held["cards"]) == sorted(opening[:2]),
        )
        ok(
            f"C: ★ and the document is untouched — a copy is not a cut — "
            f"{len(cards(app, surface))} card(s)",
            cards(app, surface) == opening,
        )

        banner("D — ★★★★★ they are pasted, and the screen says what was renamed")
        said = app.invoke(f"{surface}/paste", "")
        app.tick_ms(16)
        after = cards(app, surface)
        ok(
            f"D: ★ two more cards are on the canvas — {len(opening)} then "
            f"{len(after)}",
            len(after) == len(opening) + 2,
        )
        minted = [name for name in after if name not in opening]
        ok(
            f"D: ★★★★★ the copies are NOT called what they were copied from — "
            f"{minted}",
            len(minted) == 2 and all(name not in opening for name in minted),
        )

        banner("E — ★★★★★ every card on the frame still addresses ONE card")
        # ★ This runs BEFORE the reporting check on purpose. It is the property
        # the repair exists for, and it is the one that goes red first in the
        # pre-repair tree — measured, by rebuilding against it: the paste
        # lands, and `select` can then address neither the original nor the
        # copy, because `node_of` answers `None` for a name two cards hold
        # (R1984). A screen that publishes a name it will not take is the
        # defect R1981 found on the other axis.
        for name in after:
            picked = app.invoke(f"{surface}/select", name)
            app.tick_ms(16)
            ok(
                f"E: ★★★★★ {name!r} addresses exactly one card — {picked!r}",
                picked == name,
            )
        ok(
            f"E: ★★★★★ and the screen SAYS what each copy is called instead — "
            f"{said!r}",
            "renamed" in said and "is now" in said,
        )

        # ★★★★★ A card and its OWN copy, pasted together. Both number from one
        # stem, which is the case a batch has to decide against copies that are
        # not in the document yet — and the case this walk could not reach
        # until it went looking for it: with two differently-stemmed cards the
        # rule cannot be broken.
        mate = next(name for name in minted if stem(name) == stem(opening[0]))
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", mate)
        app.tick_ms(16)
        app.invoke(f"{surface}/copy", "")
        app.tick_ms(16)
        said = app.invoke(f"{surface}/paste", "")
        app.tick_ms(16)
        kin = cards(app, surface)
        ok(
            f"E: ★★★★★ a card and its own copy pasted TOGETHER get two names, "
            f"not one — {said!r}",
            len(kin) == len(after) + 2,
        )
        for name in kin:
            picked = app.invoke(f"{surface}/select", name)
            app.tick_ms(16)
            ok(
                f"E: ★★★★★ {name!r} still addresses exactly one card — "
                f"{picked!r}",
                picked == name,
            )
        after = kin

        banner("F — ★ duplicate puts a copy beside the original")
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        said = app.invoke(f"{surface}/duplicate", "")
        app.tick_ms(16)
        grown = cards(app, surface)
        ok(
            f"F: ★ one more card — {len(after)} then {len(grown)}: {said!r}",
            len(grown) == len(after) + 1,
        )
        ok(
            f"F: ★★★★★ and it numbers from the STEM rather than growing a tail "
            f"— {[n for n in grown if n not in after]}",
            all("-01-" not in name for name in grown),
        )

        banner("G — ★ the clipboard crosses a subgraph boundary")
        floor = cards(app, surface)
        app.invoke(f"{surface}/select", floor[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", floor[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/copy", "")
        app.tick_ms(16)
        app.invoke(f"{surface}/select", floor[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", floor[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        app.invoke(f"{surface}/enter", PART)
        app.tick_ms(16)
        ok(
            f"G: ★ the tool is inside the part — {standing(app, surface)}",
            standing(app, surface)["depth"] == 1,
        )
        ok(
            f"G: ★★★★★ and the clipboard survived the descent — "
            f"{clipboard(app, surface)}",
            clipboard(app, surface)["held"] is True,
        )
        inside_before = cards(app, surface)
        said = app.invoke(f"{surface}/paste", "")
        app.tick_ms(16)
        inside_after = cards(app, surface)
        ok(
            f"G: ★★★★★ the paste landed WHERE THE PERSON IS STANDING, not at "
            f"the root — {len(inside_before)} then {len(inside_after)}: {said!r}",
            len(inside_after) == len(inside_before) + 2,
        )
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        ok(
            f"G: ★ and out here is unchanged by it — {standing(app, surface)}",
            len(cards(app, surface)) == len(floor) - 1,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1985 a copy takes a name of its own", body)
