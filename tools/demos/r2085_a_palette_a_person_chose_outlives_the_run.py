#!/usr/bin/env python3
"""R2085 §5.11 §5.15 — **a palette a person chose outlives the run, and the
screen says what the colour costs.**

# What this walk exists for, and why it cannot be a unit test

The node lab's kind colours were constants: the behaviour canon declares ten of
them across twenty-one kinds and this screen reproduced them hex for hex. The
owner's instruction added an axis over that reproduction — *make the group
colour choosable* — so a heading now carries a chip, one press colours every
kind under it, and the choice is written to a store of its own.

**Written to a store is exactly the half no test in one process can finish.**
The screen's own suite runs against an in-memory backend on purpose (its
`app_storage` says why: forty tests, two of them about saving, and the one that
forgot would write into the developer's real data directory), so the FILE path
has no in-process cover at all. Two launches against one isolated directory is
the only place *the palette came back* is a fact rather than a plan.

The assembled-tool half — the chip pressed through the host's own router, on the
shell, with a card of that heading repainted — is
`r2085_the_assembled_palette_colours_a_group_and_says_what_it_costs`. This walk
is the process half.

# What this walk holds

  (A) at boot nobody has chosen anything, and every kind is drawn in what the
      taxonomy declares — which is what keeps the opening screen the canon's.
  (B) the chip is painted beside its heading, and a POINT resolves to it: a
      control routed only by name is one a mouse cannot reach (R2047).
  (C) one press colours every kind under that heading, and the CHIP'S PIXELS are
      the colour the register says — the frame, not the model.
  (D) the sentence a person reads names the heading and how many kinds moved.
  (E) the file backend wrote it, under an isolated directory, one line per kind.
  (F) ★★★★★ a FRESH PROCESS opens with the chosen palette — nothing else in this
      tree can prove that — while the graph deliberately does not auto-load.
  (G) pressing round the ring comes back to the colours the kinds declare, and
      the store comes back with it.
  (H) a colour under the legibility floor is TAKEN and reported, and the marks
      register says which floor it misses. Measured: two of the ten colours the
      canon itself declares are under the boundary floor on this screen's own
      grounds, so a refusal would refuse the palette it is already painted in.

Run from the workspace root:
    cargo build --release -p hello-node-lab
    DISPLAY=:1 python3 tools/demos/r2085_a_palette_a_person_chose_outlives_the_run.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    isolated_storage_dir,
    press_painted_tag,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1400, 900)
# The key the chosen palette is written under — `persist::PALETTE_KEY`, which is
# also the file's name under the storage directory.
PALETTE_KEY = "node_lab.palette"
# The graph's own key, read here only to hold the two apart in (F).
GRAPH_KEY = "node_lab.graph"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def palette(app: RpcSubprocess) -> dict:
    """The register, split the two ways this walk reads it."""
    published = js(app.query(f"{EXT}/palette"))
    return {
        "groups": {row["group"]: row for row in published["groups"]},
        "roles": {row["role"]: row for row in published["roles"]},
    }


def chip_fill(app: RpcSubprocess, tag: str) -> dict | None:
    """The colour the chip is actually PAINTED in, off the frame.

    A container carries its colour as `style.fill` (R1921 measured that a run
    carries `fg_color` instead, which is how a walk reading one field for both
    finds a colour that is not there).
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    return (find_by_tag(snap, tag) or {}).get("style", {}).get("fill")


def as_hex(colour: dict) -> str:
    return "#{:02X}{:02X}{:02X}".format(colour["r"], colour["g"], colour["b"])


def a_heading_with_several_colours(published: dict) -> dict:
    """A heading whose kinds declare more than one colour.

    Read from the register rather than named here: with one colour every press
    below would be the same press, and which headings have several is the
    taxonomy's answer and has changed twice.
    """
    for row in published["groups"].values():
        if len(row["ring"]) >= 2:
            return row
    raise AssertionError(f"no heading declares several colours: {published['groups']}")


def body() -> None:
    with isolated_storage_dir("r2085_node_lab_palette") as sdir:
        # ────────────────────────────────────────────────── launch 1
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as app:
            banner("A — nobody has chosen anything")
            published = palette(app)
            ok(
                f"A: the register publishes every kind — {len(published['roles'])}",
                len(published["roles"]) >= 21,
            )
            ok(
                "A: and none of them carries a chosen colour",
                all(row["chosen"] is None for row in published["roles"].values()),
            )
            ok(
                "A: ★ every kind is drawn in the colour the taxonomy declares, "
                "which is what keeps this screen the canon's pixel for pixel",
                all(
                    row["ink"] == row["declared"] for row in published["roles"].values()
                ),
            )
            ok(
                "A: and no heading claims a colour its kinds do not agree on",
                all(
                    row["chosen"] is None and row["mixed"] is False
                    for row in published["groups"].values()
                ),
            )

            heading = a_heading_with_several_colours(published)
            label, chip, ring = heading["group"], heading["chip"], heading["ring"]
            kinds = heading["kinds"]
            print(f"[demo] {label}: {len(kinds)} kind(s), ring {ring}")

            banner("B — the chip is painted, and a POINT reaches it")
            ok(
                f"B: {chip} is painted",
                chip_fill(app, chip) is not None,
            )
            ok(
                "B: ★★ and the heading's words are a different address, so a "
                "press on them is not a press on the control",
                chip != heading["head"],
            )
            # ⚠ Through `abs_rects_of`, never `find_by_tag(...)["rect"]`: the
            # palette pane SCROLLS (R1662), so a node inside it carries a
            # scroll-LOCAL rectangle and a press aimed at that lands wherever
            # that rectangle happens to be on the window.
            rects = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
            x, y, w, h = rects[chip]
            at = (x + w // 2, y + h // 2)
            ok(
                f"B: ★★★★★ a point at {at} resolves to that heading's control — "
                "a control routed only by name is one a mouse cannot reach",
                app.invoke(f"{EXT}/point", f"{at[0]},{at[1]}") == f"group-ink:{label}",
            )

            banner("C — one press colours every kind under the heading")
            press_painted_tag(app, chip, VIEWPORT)
            app.tick_ms(16)
            published = palette(app)
            first = ring[0]
            ok(
                f"C: the heading now says one colour — {published['groups'][label]['chosen']}",
                published["groups"][label]["chosen"] == first,
            )
            ok(
                f"C: ★★★★★ all {len(kinds)} kind(s) under it took it, which is "
                "what makes this ONE gesture",
                all(published["roles"][kind]["chosen"] == first for kind in kinds),
            )
            painted_now = chip_fill(app, chip)
            ok(
                f"C: ★★★★★ and the chip's PIXELS are that colour — {as_hex(painted_now)} "
                f"on the frame against {first} on the wire",
                as_hex(painted_now) == first,
            )

            banner("D — what a person reads")
            said = app.query(f"{EXT}/toast")
            ok(
                f"D: the sentence names the heading and how many kinds moved: {said}",
                label in said and f"{len(kinds)} kinds" in said,
            )

            banner("E — the file backend wrote it")
            on_disk = sdir / PALETTE_KEY
            ok(f"E: {PALETTE_KEY} is under {sdir}", on_disk.exists())
            lines = [
                line for line in on_disk.read_text(encoding="utf-8").splitlines() if line
            ]
            assert_eq(
                len(lines),
                len(kinds),
                "★ one line per coloured kind — a store keyed by GROUP would "
                "flatten a distinction the canon draws, since its groups are "
                "not single-coloured",
            )
            ok(
                f"E: and every line carries the colour: {lines}",
                all(line.endswith(first) for line in lines),
            )

        # ────────────────────────────────────────────────── launch 2
        # A fresh process, the same storage directory.
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as app:
            banner("F — a fresh process opens with the palette a person chose")
            published = palette(app)
            ok(
                "F: ★★★★★ the choice came back with no press and no load — the "
                "claim no in-process test can make, because this screen's own "
                "suite stores in memory",
                all(published["roles"][kind]["chosen"] == first for kind in kinds),
            )
            ok(
                "F: and it is what the kinds are DRAWN in",
                all(published["roles"][kind]["ink"] == first for kind in kinds),
            )
            ok(
                f"F: the chip's pixels come back coloured too — {chip}",
                as_hex(chip_fill(app, chip)) == first,
            )
            ok(
                "F: ⚠ and the GRAPH is not auto-loaded, which is the asymmetry "
                "this round is deliberate about: a document decides every "
                "assertion about cards and links, a palette decides none",
                not (sdir / GRAPH_KEY).exists() and app.query(f"{EXT}/stored") == "",
            )

            banner("G — round the ring and back")
            # One press for each remaining colour, then one more: the state
            # after the last is the cleared one, so a person who presses once
            # too often is where they started rather than stuck in a colour
            # they would need a verb to undo.
            for step in range(len(ring)):
                press_painted_tag(app, chip, VIEWPORT)
                app.tick_ms(16)
                published = palette(app)
                want = ring[step + 1] if step + 1 < len(ring) else None
                ok(
                    f"G: press {step + 1} of the way round lands on {want}",
                    published["groups"][label]["chosen"] == want,
                )
            ok(
                "G: ★ back to the colours these kinds declare",
                all(
                    published["roles"][kind]["ink"]
                    == published["roles"][kind]["declared"]
                    for kind in kinds
                ),
            )
            kept = [
                line
                for line in (sdir / PALETTE_KEY).read_text(encoding="utf-8").splitlines()
                if line
            ]
            ok(
                f"G: and the store came back with it — {kept}",
                not kept,
            )

            banner("H — a colour under the floor is taken, and reported")
            said = app.invoke(f"{EXT}/tint_group", f"{label},#000000")
            ok(
                f"H: ★★★★★ it is TAKEN, and the sentence carries the floor it "
                f"misses: {said}",
                "under the" in said and "floor" in said,
            )
            published = palette(app)
            marks = [
                mark for kind in kinds for mark in published["roles"][kind]["marks"]
            ]
            ok(
                f"H: every mark is judged against the ground it lands on — {len(marks)}",
                len(marks) == 3 * len(kinds),
            )
            ok(
                "H: each verdict is its own two numbers",
                all(
                    mark["clears"] == (mark["ratio"] >= mark["asks"]) for mark in marks
                ),
            )
            ok(
                "H: ★ black on this screen's dark grounds clears nothing",
                all(mark["clears"] is False for mark in marks),
            )
            ok(
                "H: and the two floors are named rather than numbered here",
                {mark["floor"] for mark in marks} == {"boundary", "text"},
            )
            app.invoke(f"{EXT}/tint_group", f"{label},none")
            published = palette(app)
            ok(
                "H: `none` puts it back on the same channel",
                all(published["roles"][kind]["chosen"] is None for kind in kinds),
            )

    print(f"\n{len(CHECKS)} check(s) passed")


if __name__ == "__main__":
    sys.exit(
        run_demo("R2085 §5.11 §5.15 — a palette a person chose outlives the run", body)
    )
