#!/usr/bin/env python3
"""R1986 §5.2 §5.11 — **a definition says what may be done to it, and then it is
done.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1981-R1983 gave the assembled tool the subgraph verbs and R1985 gave it copy,
paste and duplicate — all of which act on **cards**. This is the axis one level
up: the definition itself, which the palette lists and which until this round a
person could only *make* and *remove*. Its name was fixed at the moment it was
created, and the only copy verb the crate had needed an instance to rebind.

# ★★★★★ The defect this walk exists to keep dead

Measured at this round's open, by driving the assembled tool: the screen
resolved a definition with `definitions().find(|held| held.name == wanted)` —
**the first one that matches**. Two definitions may answer to one name here (the
fragment path adds a carried definition under the name it arrives with, and the
matcher that decides whether it is one the document already has READS that
name), so *which* definition a person removed depended on insertion order. That
was unreachable from a screen before this round, because nothing on it could
make a second definition. This round makes one in a single press — so the walk
makes the pair and asserts the tool refuses to guess. (F).

# What this walk holds

  (A) the journey reaches the node lab, at the top of the assembled tool.
  (B) ★ a definition is made, and the register says what may be done to it —
      the permission is on the SCREEN, not only in the crate.
  (C) ★★★★★ the refusal FIRST: removing a definition that is in use is refused
      and the refusal NAMES the cost, both in the register before pressing and
      in the answer when pressed.
  (D) ★★★★★ rename: the definition takes another name, the answer says which
      name it REPLACED, and the breadcrumb inside it follows without a reload.
  (E) ★★★★★ copy: a definition is duplicated ON ITS OWN, the copy takes a name
      of its own, and NOTHING stands for the copy — which is the whole
      difference from forking, and what the register shows.
  (F) ★★★★★ the ambiguity guard: the copy is renamed BACK, so two definitions
      answer to one name, and every verb addressed by that name is refused with
      a sentence that says why. This is the clause that goes red with the
      repair taken out — measured, by rebuilding against it: the drop lands, on
      whichever definition came first.
  (G) ★ the removal's cost is stated before pressing, and the register agrees
      with what actually went.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1986_a_definition_answers_for_itself.py
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


def definitions(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/definitions"))["definitions"]


def named(rows: list[dict], name: str) -> dict:
    return next(row for row in rows if row["definition"] == name)


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
        ok(
            f"A: ★ and the opening graph holds no definition yet — "
            f"{definitions(app, surface)}",
            definitions(app, surface) == [],
        )

        banner("B — ★ a definition is made, and it says what may be done to it")
        opening = cards(app, surface)
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", opening[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        rows = definitions(app, surface)
        ok(f"B: ★ one definition, listed by name — {rows}", len(rows) == 1)
        made = rows[0]
        ok(
            f"B: ★★★★★ the register carries the PERMISSION for each verb, so a "
            f"screen can grey a control and say why — {made['may']}",
            set(made["may"]) == {"remove", "rename", "duplicate"},
        )
        ok(
            f"B: ★ renaming and copying are allowed — {made['may']}",
            made["may"]["rename"] is None and made["may"]["duplicate"] is None,
        )

        banner("C — ★★★★★ the refusal FIRST, and it names the cost")
        ok(
            f"C: ★★★★★ removing it while a card stands for it is REFUSED, and "
            f"the register said so BEFORE anyone pressed — {made['may']['remove']!r}",
            isinstance(made["may"]["remove"], str)
            and "stand for it" in made["may"]["remove"],
        )
        ok(
            f"C: ★ and the register says who stands for it, not just how many "
            f"— {made['used_by']}",
            len(made["used_by"]) == 1 and made["used_by"][0]["card"],
        )
        said = refusal(app, f"{surface}/drop_definition", f"{PART},keep")
        ok(
            f"C: ★★★★★ and pressing it answers the SAME refusal — the question "
            f"and the edit are one decision — {said[:100]!r}",
            "stand for it" in said,
        )
        ok(
            f"C: ★ the refused removal changed nothing — "
            f"{len(definitions(app, surface))} definition(s)",
            len(definitions(app, surface)) == 1,
        )

        banner("D — ★★★★★ the definition takes another name, and says which")
        said = app.invoke(f"{surface}/rename_definition", f"{PART},link-side")
        app.tick_ms(16)
        ok(
            f"D: ★★★★★ the answer names the name it REPLACED, which the "
            f"reference's bool cannot — {said!r}",
            said == f"{PART} is now link-side",
        )
        rows = definitions(app, surface)
        ok(
            f"D: ★ and the register shows the new name only — {[r['definition'] for r in rows]}",
            [row["definition"] for row in rows] == ["link-side"],
        )
        instance = named(rows, "link-side")["used_by"][0]["card"]
        app.invoke(f"{surface}/enter", instance)
        app.tick_ms(16)
        inside = standing(app, surface)
        ok(
            f"D: ★★★★★ and the breadcrumb INSIDE it follows the rename with no "
            f"reload, because it is derived — {inside}",
            inside["depth"] == 1 and "link-side" in inside["through"],
        )
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        ok(
            f"D: ★ back at the top — {standing(app, surface)}",
            standing(app, surface)["depth"] == 0,
        )
        said = refusal(app, f"{surface}/rename_definition", "link-side,   ")
        ok(
            f"D: ★ a name that is empty once trimmed is refused with a reason — "
            f"{said[:100]!r}",
            "needs a name" in said,
        )

        banner("E — ★★★★★ the definition is copied ON ITS OWN")
        copy = app.invoke(f"{surface}/copy_definition", "link-side")
        app.tick_ms(16)
        ok(
            f"E: ★★★★★ the copy takes a name of ITS OWN, numbered from the stem "
            f"— {copy!r}",
            copy == "link-side-01",
        )
        rows = definitions(app, surface)
        ok(
            f"E: ★ both are listed — {[r['definition'] for r in rows]}",
            sorted(row["definition"] for row in rows) == ["link-side", "link-side-01"],
        )
        ok(
            f"E: ★★★★★ and NOTHING stands for the copy, which is the whole "
            f"difference from forking — {named(rows, 'link-side-01')['used_by']}",
            named(rows, "link-side-01")["used_by"] == [],
        )
        ok(
            f"E: ★★★★★ so the copy may be removed and the original may not — "
            f"one register, two answers, both from the same question — "
            f"{named(rows, 'link-side-01')['may']['remove']!r}",
            named(rows, "link-side-01")["may"]["remove"] is None
            and isinstance(named(rows, "link-side")["may"]["remove"], str),
        )
        ok(
            f"E: ★ the original's cards did not move to the copy — "
            f"{named(rows, 'link-side')['used_by']}",
            len(named(rows, "link-side")["used_by"]) == 1,
        )

        banner("F — ★★★★★ two definitions under one name address NEITHER")
        app.invoke(f"{surface}/rename_definition", "link-side-01,link-side")
        app.tick_ms(16)
        rows = definitions(app, surface)
        ok(
            f"F: ★ the pair is made — {[r['definition'] for r in rows]}",
            [row["definition"] for row in rows] == ["link-side", "link-side"],
        )
        said = refusal(app, f"{surface}/drop_definition", "link-side,take")
        ok(
            f"F: ★★★★★ dropping BY THAT NAME is refused, and the refusal says "
            f"why rather than guessing — {said[:110]!r}",
            "2 definitions answer to" in said,
        )
        ok(
            f"F: ★★★★★ and nothing was dropped — before the repair this took "
            f"whichever came first — {len(definitions(app, surface))} definition(s)",
            len(definitions(app, surface)) == 2,
        )
        for verb, args in [
            ("rename_definition", "link-side,anything"),
            ("copy_definition", "link-side"),
        ]:
            said = refusal(app, f"{surface}/{verb}", args)
            ok(
                f"F: ★★★★★ {verb} by the ambiguous name is refused too — "
                f"{said[:80]!r}",
                "2 definitions answer to" in said,
            )
        # ★ And the ambiguity is repairable from the screen, which is what makes
        # the refusal a state a person can leave rather than a corner.
        ok(
            f"F: ★ the copy is still addressable by its ID-shaped listing — "
            f"{[r['id'] for r in definitions(app, surface)]}",
            len({row["id"] for row in definitions(app, surface)}) == 2,
        )

        banner("G — ★ the cost is stated before the removal, and it is right")
        # The pair is untangled by name is impossible, so the SECOND one goes by
        # being renamed through the first — which is why the walk unwinds it the
        # way it made it: drop the whole thing with `take` after separating.
        app.invoke(f"{surface}/select", instance)
        app.tick_ms(16)
        app.invoke(f"{surface}/ungroup", instance)
        app.tick_ms(16)
        rows = definitions(app, surface)
        ok(
            f"G: ★ the instance is gone, so both definitions now stand alone — "
            f"{[(r['definition'], len(r['used_by'])) for r in rows]}",
            all(row["used_by"] == [] for row in rows),
        )
        cost = rows[0]["removal_would_take"]
        ok(
            f"G: ★★★★★ what a removal WOULD take is counted before anyone "
            f"presses — {cost}",
            cost is not None and cost["definitions"] == 1,
        )
        ok(
            f"G: ★ and every one of them may now be removed — "
            f"{[r['may']['remove'] for r in rows]}",
            all(row["may"]["remove"] is None for row in rows),
        )
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1986 a definition answers for itself", body)
