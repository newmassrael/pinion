#!/usr/bin/env python3
"""R1902 §5.32 §2 #7 — **where a pane OPENS is a declaration, and it is judged
by the same rules a gesture is.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the campaign
`debt-the-arrangeable-unit-is-a-panel-and-should-be-an-area`'s order step 3.

# The hole this closes, measured before the round

Every CHANGE to a panel's placement went through a policy: R1801 built
`EdgePolicy::admit` for the edge, R1887 `admit_fold` for the fold, R1889
`admit_extent` for the size, and each returns a refusal that names what was
asked and what was allowed.

The placement a screen **opened in** went through nothing at all. It was two
`const`s in the painter, beside a specification that described the same panel in
different words, and nothing compared them. So a pane could declare it does not
fold and open folded, or open on an edge its own `allowed` list excludes, and
the contradiction would stand for the life of the program — with no call to
blame and no moment to watch. That is a sharper form of the habit this axis was
built against: the floor toolkit lets an imperative call quietly beat a declared
constraint, and here the initial state never met its declaration at all.

# ★★★★★ What the entry re-measurement overturned

The campaign's own step 3 reads "the remaining half of region: edge flip, and
**hidden by default** (the reference editor's palette is)". `flip` landed at
R1887, so the item left was the default. The reference editor does flag its node
editor's tools region hidden.

**The behaviour canon does not.** Extracted and read this round, its palette
state initialises OPEN and it carries `togglePalette` / `openPalette` so a
reader puts the panel away and brings it back afterwards. Opening folded would
therefore have been a second-pass change that UN-REPRODUCES the canon — the
standing order rule's named error, skipping reproduction because our way looks
better. It was built, it turned seventeen existing gates red, and the
measurement is why it was reverted rather than the gates rewritten.

⇒ what step 3 is owed is not the value but the **judgement**: an opening
placement that no longer gets to contradict its own policy.

⚠ AND THE PRESCRIPTION NAMED THE WRONG PANEL TOO, which is the third time this
campaign has done that (R1891's `detachWidget`, R1898's `split`). The canon's
`togglePalette` belongs to its DASHBOARD shell, not to the lab — and the
assembled tool's dashboard palette cannot be put away at all. That is a
first-pass reproduction gap, ranked above any second-pass borrowing, and it is
named as the next slice rather than half-built here.

# What this walk holds

  (A) the assembled tool mounts the lab, and the lab publishes for every pane
      both where it IS and where it OPENS — two facts, not one bit read twice.
  (B) every declared opening is one that pane's own policy admits, asked of the
      screen rather than recomputed here.
  (C) at rest, every pane is where it opens: the screen a reader arrives at is
      the screen the specification describes.
  (D) a pane that opens showing and folds by hand ends up DIFFERENT from its
      opening, which is what makes `opens` a second fact rather than a copy.
  (E) and no pane opens folded, because the canon opens its palette open.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1902_a_pane_opens_where_its_policy_admits.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
)

SHELL = "hello-analyzer-shell"
#: The HOST's own root surface — see R1890 on why this is named for what it is.
EXT = "/external"
SEAT = "lab"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    """A published value, whether the surface handed back JSON or a string."""
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    """Where the screen mounted at `seat` answers, as the application says.

    Asked rather than composed (R1890): the roster publishes each mounted
    destination's address, so this never has to know the shape of it.
    """
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def panes(app: RpcSubprocess, surface: str) -> dict:
    """Each pane's row of the specification, in the screen's own words."""
    said = js(app.query(f"{surface}/spec"))
    return {pane["tag"]: pane for pane in said["panes"]}


def admits(pane: dict, placement: dict) -> tuple[bool, str]:
    """Whether this pane's published policy admits that placement.

    ⚠ This is the walk's SECOND hand and it is deliberately a re-derivation
    rather than a call: the Rust gate asks `EdgePolicy::admit_opening`, and if
    this asked the same function the two would agree by construction. Here the
    rules are read off what the screen PUBLISHES — the admitted edges, whether
    it folds, the resize bounds — so a policy that stops being published, or a
    published policy that stops matching the one enforced, shows up as a
    disagreement between two sources rather than as silence.

    The two things it does not check are the two the framework does not either,
    for the reasons stated there: a pane that admits no edge is pinned rather
    than nowhere, and a pane with no resize bounds has no range to fall outside.
    """
    edges = pane["edges"]
    if edges and placement["edge"] not in edges:
        return False, f"edge {placement['edge']} not in {edges}"
    if placement["folded"] and not pane["foldable"]:
        return False, "opens folded but does not fold"
    bounds = pane["resize"]
    if bounds is not None and not (
        bounds["min"] <= placement["extent"] <= bounds["max"]
    ):
        return False, f"extent {placement['extent']} outside {bounds}"
    return True, "admitted"


def section_a(app: RpcSubprocess) -> dict:
    banner("A — the assembled tool mounts the lab, and it publishes both facts")
    surface = surface_of(app, SEAT)
    ok(f"A: the roster gives the lab an address — {surface!r}", bool(surface))
    declared = panes(app, surface)
    ok(f"A: ★ it declares panes — {sorted(declared)}", len(declared) >= 4)
    for tag, pane in sorted(declared.items()):
        ok(
            f"A: ★★ `{tag}` publishes where it OPENS — {pane.get('opens')!r}",
            isinstance(pane.get("opens"), dict)
            and set(pane["opens"]) == {"edge", "extent", "folded"},
        )
    ok(
        "A: ★★★★★ and `opens` is a SECOND fact beside `at`, not the same bit "
        "twice: a client reading only `at` cannot tell a panel that arrived "
        "folded from one somebody folded",
        all("at" in pane for pane in declared.values()),
    )
    return declared


def section_b(declared: dict) -> None:
    banner("B — every declared opening is one that pane's own policy admits")
    for tag, pane in sorted(declared.items()):
        good, why = admits(pane, pane["opens"])
        ok(
            f"B: ★★★★★ `{tag}` opens at {pane['opens']} and its own policy "
            f"admits it — {why}",
            good,
        )
    ok(
        f"B: and the population is the whole specification — {len(declared)} pane(s)",
        len(declared) == 4,
    )


def section_c(app: RpcSubprocess, declared: dict) -> None:
    banner("C — at rest, every pane IS where it opens")
    for tag, pane in sorted(declared.items()):
        at = pane["at"]
        if at is None:
            # A pane with no placement of its own — the rail and the canvas.
            # Saying so is the point: `null` is a statement, not a gap.
            ok(
                f"C: `{tag}` carries no placement of its own, and says so",
                pane["at"] is None,
            )
            continue
        ok(
            f"C: ★★ `{tag}` is at {at} and opens at {pane['opens']} — a reader "
            f"arrives at the screen the specification describes",
            at == pane["opens"],
        )


def section_d(app: RpcSubprocess, declared: dict) -> None:
    banner("D — folding by hand makes `at` differ from `opens`")
    surface = surface_of(app, SEAT)
    # The palette: it opens showing, and its policy admits a fold.
    pane = declared["lab.palette"]
    ok(
        "D: the palette opens showing and declares that it folds, so this "
        "gesture is one its own declaration permits",
        pane["opens"]["folded"] is False and pane["foldable"] is True,
    )
    app.invoke(f"{surface}/place", "palette,fold")
    for _ in range(4):
        app.tick_ms(16)

    after = panes(app, surface)["lab.palette"]
    ok(
        f"D: ★★★★★ it is folded now and it still opens showing — at "
        f"{after['at']}, opens {after['opens']}",
        after["at"]["folded"] is True and after["opens"]["folded"] is False,
    )
    ok(
        "D: ★★ so the two fields say DIFFERENT things, which is the whole "
        "reason there are two",
        after["at"] != after["opens"],
    )
    ok(
        "D: ★ and the extent survived the fold, so unfolding restores the size "
        f"rather than a default — {after['at']['extent']}",
        after["at"]["extent"] == after["opens"]["extent"],
    )

    app.invoke(f"{surface}/place", "palette,unfold")
    for _ in range(4):
        app.tick_ms(16)
    back = panes(app, surface)["lab.palette"]
    ok(
        f"D: ★★ and it comes back to exactly where it opened — {back['at']}",
        back["at"] == back["opens"],
    )


def section_e(declared: dict) -> None:
    banner("E — no pane opens folded, because the canon opens its palette open")
    folded = [tag for tag, pane in declared.items() if pane["opens"]["folded"]]
    ok(
        "E: ★★★★★ nothing on this screen starts folded — the behaviour canon's "
        "palette state initialises OPEN, so a pane that started folded here "
        f"would un-reproduce it — {folded}",
        folded == [],
    )
    ok(
        "E: ★ and that is not because nothing CAN fold: the panes that declare "
        "a fold are the side panels, which is what makes the line above a "
        "choice rather than an absence",
        sum(1 for pane in declared.values() if pane["foldable"]) == 2,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # The lab is mounted when a reader goes there, so the journey is part of
        # the walk rather than a fixture: a claim about a screen nobody can
        # reach is a claim about a binary nobody runs.
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        declared = section_a(app)
        section_b(declared)
        section_c(app, declared)
        section_d(app, declared)
        section_e(declared)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1902 a pane opens where its policy admits", body)
