#!/usr/bin/env python3
"""R1903 §5.21 §5.49 §2 #2 — **the palette can be put away, and brought back.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the campaign
`debt-the-arrangeable-unit-is-a-panel-and-should-be-an-area`'s order step 3, and
it closes a **first-pass reproduction gap** — something the behaviour canon has
and this build did not — which the standing order rule ranks above any
second-pass borrowing.

# The gap, measured before the round

`grep -c 'palette_open\\|toggle_palette\\|palette_closed'` over the assembled
shell answered **0**, and its own source carried the gap as a comment: *"The
palette is always open in this shell, so the button is what SAYS where widgets
come from rather than opening a second chooser."* True when written, and the
thing to fix.

The canon carries `togglePalette`, `openPalette` and a `paletteClosed` flag, and
its markup shows what they do: the drawer is a flex sibling that is `width:300px`
open and, closed, is replaced by a `width:44px` strip whose own element carries
the toggle. So the panel does not vanish — it becomes a band, the canvas takes
the difference, and **the whole band is the way back**.

⚠ This round's own predecessor found the same prescription pointing at the wrong
panel: R1902 tried the campaign's "hidden by default" on the node lab, turned
seventeen gates red, and found on opening the canon that its palette initialises
OPEN and that `togglePalette` belongs to the DASHBOARD shell. This walk is that
correction carried out.

# What is superior to the floor here, and it is the same axis R1902 built

The placement is a **value a policy judges**, not a boolean. `EdgePolicy`
declares that this panel folds and does not move or resize; `admit_opening` was
asked whether the opening arrangement is one that declaration allows — the
judgement R1902 built after measuring that every CHANGE to a placement met a
policy and the OPENING state met nothing at all. A `bool` would have been the
cheap spelling and has nothing to be judged.

# What this walk holds

  (A) at rest the palette is open, at the width the specification gives it, and
      the wire says both where it IS and where it OPENS.
  (B) the control the canon draws is on screen and reachable, and pressing it
      puts the panel away — leaving a strip, not a hole.
  (C) the canvas grows by EXACTLY what the panel gave up, and the two still
      meet: one number read twice rather than two derivations that agree today.
  (D) a folded palette announces its way back and NOT its thirteen rows.
  (E) the strip brings it back, and the add button opens it as the canon's
      `openPalette` does — one verb under both.
  (F) the wire drives it too, and a word the vocabulary does not know is refused
      by name.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1903_the_palette_can_be_put_away.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
FOLD = "shell.palette.head.fold"
STRIP = "shell.palette.strip"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rects(app: RpcSubprocess) -> dict:
    return abs_rects_of(app.snapshot(source="paint"))


def centre(rect) -> tuple[float, float]:
    return (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)


def access(app: RpcSubprocess) -> dict:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result


def palette(app: RpcSubprocess) -> dict:
    """What the screen publishes about its palette's placement."""
    said = app.query(f"{EXT}/spec")
    said = json.loads(said) if isinstance(said, str) else said
    # `palette_placement`, not `palette`: the document already publishes a
    # `palette` — the catalogue roster — and this is a different fact.
    return said["palette_placement"]


def settle(app: RpcSubprocess) -> None:
    for _ in range(4):
        app.tick_ms(16)


def section_a(app: RpcSubprocess) -> dict:
    banner("A — at rest it is open, and the wire says where it IS and where it OPENS")
    said = palette(app)
    ok(f"A: ★★ the palette publishes its placement — {said!r}", "at" in said and "opens" in said)
    ok(
        "A: ★ it is open, which is the canon's own initial state",
        said["at"]["folded"] is False and said["opens"]["folded"] is False,
    )
    ok(
        "A: ★★★★★ `at` and `opens` are TWO facts — a client reading only one "
        "cannot tell a panel put away by a person from one that came that way",
        said["at"] == said["opens"],
    )
    ok(
        f"A: ★ and the policy is published, so a client is told what the verb "
        f"accepts rather than finding out by being refused — foldable="
        f"{said['foldable']}, strip {said['strip_w']}px",
        said["foldable"] is True and said["strip_w"] > 0,
    )
    shot = rects(app)
    panel = shot["shell.palette"]
    ok(
        f"A: ★ it is painted at the width the specification gives it — {panel}",
        panel[2] == said["opens"]["extent"],
    )
    return {"panel": panel, "canvas": shot["shell.canvas"], "said": said}


def section_b(app: RpcSubprocess, rest: dict) -> None:
    banner("B — the canon's control is on screen, and it puts the panel away")
    shot = rects(app)
    ok(f"B: ★★ the fold control is painted — {shot.get(FOLD)}", FOLD in shot)
    ok(
        "B: ★ inside the panel's own header band, which is where the canon "
        f"renders it — control {shot[FOLD]}, panel {rest['panel']}",
        shot[FOLD][0] >= rest["panel"][0]
        and shot[FOLD][0] + shot[FOLD][2] <= rest["panel"][0] + rest["panel"][2],
    )
    ok(
        "B: ★ and a reader who never sees the drawing is told what it does",
        (access_node_by_tag(access(app), FOLD) or {}).get("name") is not None,
    )

    app.click(centre(shot[FOLD]))
    settle(app)

    said = palette(app)
    ok(f"B: ★★★★★ the palette is put away — {said['at']!r}", said["at"]["folded"] is True)
    ok(
        "B: ★★ and it still says it OPENS showing, so the two fields now "
        "disagree — which is the whole reason there are two",
        said["opens"]["folded"] is False and said["at"] != said["opens"],
    )
    after = rects(app)
    ok(
        f"B: ★★★★★ what is left is a STRIP, not a hole — {after.get(STRIP)}",
        STRIP in after and after[STRIP][2] == said["strip_w"],
    )
    ok(
        "B: ★ and the catalogue is gone from the paint entirely, so the strip "
        "is the only way back rather than merely the visible one",
        "shell.palette" not in after,
    )


def section_c(app: RpcSubprocess, rest: dict) -> None:
    banner("C — the canvas grows by exactly what the panel gave up")
    shot = rects(app)
    canvas, strip = shot["shell.canvas"], shot[STRIP]
    said = palette(app)
    given_up = said["opens"]["extent"] - said["strip_w"]
    ok(
        f"C: ★★★★★ the canvas grew by exactly the difference — {canvas[2]} - "
        f"{rest['canvas'][2]} = {canvas[2] - rest['canvas'][2]}, panel gave up "
        f"{given_up}",
        canvas[2] - rest["canvas"][2] == given_up,
    )
    ok(
        f"C: ★★ and the two still meet, with nothing between and nothing "
        f"overlapping — canvas ends {canvas[0] + canvas[2]}, strip starts "
        f"{strip[0]}",
        canvas[0] + canvas[2] == strip[0],
    )
    ok(
        "C: ★ which was true before the fold too, so this is a property of the "
        "one derivation rather than a state that happens to line up",
        rest["canvas"][0] + rest["canvas"][2] == rest["panel"][0],
    )


def section_d(app: RpcSubprocess) -> None:
    banner("D — a folded palette announces its way back and not its rows")
    tree = access(app)
    ok(
        "D: ★★★★★ the strip is announced, with what pressing it does",
        (access_node_by_tag(tree, STRIP) or {}).get("value") is not None,
    )
    ok(
        "D: ★★ and the catalogue is NOT — a reader told about thirteen rows "
        "that are not on screen is a reader sent looking for them",
        access_node_by_tag(tree, "shell.palette") is None,
    )


def section_e(app: RpcSubprocess, rest: dict) -> None:
    banner("E — the strip brings it back, and so does the add button")
    app.click(centre(rects(app)[STRIP]))
    settle(app)
    said = palette(app)
    ok(f"E: ★★★★★ the strip opened it again — {said['at']!r}", said["at"]["folded"] is False)
    ok(
        f"E: ★★ back to exactly where it opened — {said['at']} vs {said['opens']}",
        said["at"] == said["opens"],
    )
    ok(
        f"E: ★ and the canvas is back to its opening width — "
        f"{rects(app)['shell.canvas'][2]}",
        rects(app)["shell.canvas"] == rest["canvas"],
    )

    # The canon's `openPalette`: the add button opens the drawer if it is shut.
    app.invoke(f"{EXT}/palette", "fold")
    settle(app)
    ok("E: (folded again, to aim the add button at a shut palette)",
       palette(app)["at"]["folded"] is True)
    app.click(centre(rects(app)["shell.subbar.add"]))
    settle(app)
    ok(
        "E: ★★★★★ the add button OPENS it, which is the canon's `openPalette` "
        "— a reader asking to add a widget wants the palette there",
        palette(app)["at"]["folded"] is False,
    )


def section_f(app: RpcSubprocess) -> None:
    banner("F — the wire drives it, and refuses a word it does not know")
    ok(f"F: ★★ `palette fold` — {app.invoke(f'{EXT}/palette', 'fold')!r}",
       palette(app)["at"]["folded"] is True)
    ok(f"F: ★★ `palette toggle` — {app.invoke(f'{EXT}/palette', 'toggle')!r}",
       palette(app)["at"]["folded"] is False)

    refused = None
    try:
        app.invoke(f"{EXT}/palette", "vanish")
    except Exception as why:  # noqa: BLE001 — the refusal is the assertion
        refused = str(why)
    ok(
        f"F: ★★★★★ a word the vocabulary does not know is refused BY NAME, not "
        f"ignored — {refused!r}",
        refused is not None and "vanish" in refused,
    )
    ok(
        "F: ★ and the refusal changed nothing",
        palette(app)["at"]["folded"] is False,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        rest = section_a(app)
        section_b(app, rest)
        section_c(app, rest)
        section_d(app)
        section_e(app, rest)
        section_f(app)
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1903 the palette can be put away", body)
