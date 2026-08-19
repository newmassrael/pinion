#!/usr/bin/env python3
"""R1724 §5.16 §5.38 §5.40 §2 #2 #7 — **the analysis tool is one application.**

The behaviour reference this tool is modelled on is a single shell whose rail
switches between its sections. This tree assembled it as three executables, and
the shell's own rail said so: three of its seven seats were declared
`elsewhere` — *built, shipping, and not here* — an arm that exists only because
a destination here could be finished and still unreachable.

This round mounts the first of them. `hello-node-lab` — 20,655 lines, unedited —
is the node lab destination's page, through `pinion_screen::Mount<NodeLabView>`.
(R1728 renamed that seat from `catalog` to `lab`: the reference has a node graph
section and no catalogue section, so the page was right and the address was this
application's invention.)

What this script drives, on the running application:

* **A** — the rail. The node lab seat is open, and `elsewhere` is down from
  three to one.
* **B** — arriving paints the node lab. The lab's own panes are inside the page
  region at the node lab seat and absent at Dashboard.
* **C** — the lab lays out in the REGION, not the window. The page it paints
  fits inside the rectangle the shell placed it at, which is what
  `pinion_core::external::with_surface_extent` is for: before it, the in-view
  branch of `layout_size` answered the window, so a mounted screen's paint and
  its hit test would resolve against two different rectangles — R1700's defect
  class, and its own note said this was the case nothing could do better in.
* **D** — the screen that is not showing is not there. Measured at 6.11.1 by
  building a probe and running it: a page of the reference toolkit's paged
  container that is not current, sent a press, a key and a wheel, **counted all
  three**, and is reachable in the accessibility tree with its text field under
  it. Here its externals are not in the state scene at all.
* **E** — a press inside the page reaches the lab, and the shell's chrome still
  answers its own.
* **F** — the accessibility tree follows the rail: the lab's tree hangs under
  the page region while it is showing.
* **G** — leaving and returning is a return, not a restart.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1724_the_tool_is_one_application.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-analyzer-shell"
EXT = "/external"
REGION = "shell.canvas"
LAB_ROOT = "node_lab"
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def nodes_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/access").result["nodes"]}


def go(app: RpcSubprocess, key: str) -> None:
    app.intervene(f"{EXT}/nav", key)
    app.tick(16)
    assert_eq(q(app, "nav"), key, f"the journey reached {key}")


def press_at(app: RpcSubprocess, rect) -> None:
    app.request(
        "scene/click",
        {"button": "left", "at": {"x": rect[0] + rect[2] // 2, "y": rect[1] + rect[3] // 2}},
    )
    app.tick(16)


def lab_tags(rects: dict) -> set:
    return {tag for tag in rects if tag == LAB_ROOT or tag.startswith("lab.")}


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as app:
        # ── (A) the rail ──────────────────────────────────────────────────
        banner("A — the rail: a seat that used to say 'not here'")
        roster = q(app, "destinations")
        rows = {row["key"]: row for row in roster["destinations"]}
        ok("A: the shell publishes its roster", isinstance(roster, dict))
        assert_eq(rows["lab"]["open"], True, "A: the node lab is a place you arrive at")
        assert_eq(rows["lab"]["kind"], None, "A: and carries no closure reason")
        elsewhere = sorted(k for k, r in rows.items() if r["kind"] == "elsewhere")
        assert_eq(
            elsewhere,
            ["packets"],
            "A: ★ R1728 -- ONE seat is still on another surface. It read two "
            "until this round, and neither was a seat the reference has",
        )
        unbuilt = sorted(k for k, r in rows.items() if r["kind"] == "unbuilt")
        assert_eq(
            unbuilt,
            ["keys", "logs"],
            "A: ★ R1728 -- and two the reference has working that this build "
            "has not written. They were absent from the rail entirely before",
        )
        opens = sorted(k for k, r in rows.items() if r["open"])
        assert_eq(
            opens,
            ["dashboard", "lab", "settings"],
            "A: three destinations this ONE application hosts",
        )
        # ★★★★★ §2 #2 — which destinations are whole screens is PUBLISHED, not
        # inferred from tag prefixes. An agent that had to guess would be
        # guessing at a rule nobody wrote down.
        mounted = sorted(k for k, r in rows.items() if r["mounted"])
        assert_eq(mounted, ["lab"], "A: one section is a whole screen")
        assert_eq(
            rows["lab"]["screen"]["tag"],
            LAB_ROOT,
            "A: and it says how to address that screen's surfaces",
        )
        ok("A: an unmounted destination says so", rows["dashboard"]["screen"] is None)

        # ── (B) arriving paints the node lab ──────────────────────────────
        banner("B — arriving at the node lab seat shows the node graph lab")
        go(app, "dashboard")
        at_dashboard = abs_rects_of(app.snapshot(source="paint"))
        assert_eq(lab_tags(at_dashboard), set(), "B: no lab anywhere on the dashboard")

        go(app, "lab")
        at_lab = abs_rects_of(app.snapshot(source="paint"))
        painted = lab_tags(at_lab)
        ok("B: the lab's own root is painted", LAB_ROOT in painted)
        for pane in ("lab.palette", "lab.canvas", "lab.inspector"):
            ok(f"B: the lab's {pane} pane is painted", pane in painted)
        ok(
            f"B: the lab brought a whole screen with it ({len(painted)} regions)",
            len(painted) > 40,
        )
        print(f"[demo] the mounted lab paints {len(painted)} tagged regions")

        # The shell's own chrome is still there — this is a section of an
        # application, not a second window.
        for chrome in ("shell.appbar", "shell.rail", REGION, "shell.rail.lab"):
            ok(f"B: the shell's {chrome} is still painted", chrome in at_lab)

        # ── (C) the lab lays out in the REGION, not the window ────────────
        banner("C — a mounted screen reads the rectangle it was placed in")
        region = at_lab[REGION]
        root = at_lab[LAB_ROOT]
        window = q(app, "spec")["window"]
        ok("C: the region is narrower than the window", region[2] < window["w"])
        ok("C: and shorter than it", region[3] < window["h"])
        # ★ The height is the proof the grant is read: nothing clamps it, so
        # the lab is exactly as tall as the region and NOT as tall as the
        # window. Before `with_surface_extent` the in-view branch of
        # `layout_size` answered the window on both axes.
        assert_eq(root[3], region[3], "C: the lab is as tall as its REGION")
        assert_eq(root[2], region[2], "C: and exactly as wide")
        ok("C: and not as tall as the window it is inside", root[3] != window["h"])
        ok("C: nor as wide", root[2] != window["w"])
        # ★★★★★ …and nothing of it escapes the rectangle, because the region
        # gives the screen the recourse it declared. The lab's layout stops
        # reflowing at 1625 wide and this region is 1388, so there IS content
        # the region cannot show — `Recourse::Pan` is what happens to it.
        # Measured before that landed: 51 of the lab's regions were outside
        # this rectangle and its inspector ran from x=1365 to x=1677 in a
        # window that ends at 1440.
        outside = [
            tag
            for tag, r in at_lab.items()
            if (tag == LAB_ROOT or tag.startswith("lab."))
            and (r[0] + r[2] > region[0] + region[2] + 1 or r[1] + r[3] > region[1] + region[3] + 1)
        ]
        assert_eq(outside, [], "C: no part of the lab escapes the region it was placed in")

        # ── (D) the screen you are not at is not there ────────────────────
        banner("D — the section that is not showing has no surfaces")
        # The externals are read off the state scene rather than through a
        # bespoke method: a slot that is not in the scene cannot be queried at
        # all, which is the guarantee rather than a symptom of it.
        ok(
            "D: a lab slot answers while the lab is showing",
            lab_slot_answers(app),
        )
        go(app, "dashboard")
        ok(
            "D: and the same slot is unaddressable when it is not",
            not lab_slot_answers(app),
        )
        assert_eq(
            lab_tags(abs_rects_of(app.snapshot(source="paint"))),
            set(),
            "D: nothing of the lab is painted either",
        )
        tree = nodes_by_tag(app)
        assert_eq(
            sorted(t for t in tree if t == LAB_ROOT or t.startswith("lab.")),
            [],
            "D: and nothing of it is in the accessibility tree -- the row the "
            "reference toolkit fails, where a hidden page is walkable with its "
            "text field under it",
        )

        # ── (E) a press inside the page reaches the lab ───────────────────
        banner("E — the pointer reaches the section that is showing")
        go(app, "lab")
        rects = abs_rects_of(app.snapshot(source="paint"))
        cards = sorted(tag for tag in rects if tag.startswith("lab.node."))
        ok(f"E: the lab painted {len(cards)} node card(s)", len(cards) >= 2)
        # ★ Asked of the LAB's own wire rather than of the painted scene: "the
        # scene changed" is satisfied by a caret blink, and it is not satisfied
        # at all by pressing the card that is already selected — which is what
        # the first draft of this check did, and it failed for that reason
        # rather than for the reason it was written.
        selected = lab_selected(app)
        other = next(c for c in cards if c.rsplit(".", 1)[-1] != selected)
        press_at(app, rects[other])
        assert_eq(
            lab_selected(app),
            other.rsplit(".", 1)[-1],
            "E: a press inside the page reaches the SCREEN, and the screen "
            "acted on it -- the pointer crossed from the host into the mounted "
            "binding's own hit test",
        )

        # The shell's own rail still answers its own presses while a whole
        # other screen is showing.
        press_at(app, rects["shell.rail.dashboard"])
        assert_eq(q(app, "nav"), "dashboard", "E: the shell's rail is still the shell's")

        # ── (F) the tree follows the rail ─────────────────────────────────
        banner("F — the accessibility tree follows the rail")
        go(app, "lab")
        tree = nodes_by_tag(app)
        ok("F: the region is in the tree", REGION in tree)
        ok("F: the lab's root hangs under it", LAB_ROOT in tree[REGION]["children"])
        announced = [t for t in tree if t == LAB_ROOT or t.startswith("lab.")]
        ok(f"F: the lab announces {len(announced)} nodes of its own", len(announced) > 20)
        assert_eq(tree[REGION]["name"], "Node Lab", "F: the region names its destination")
        assert_eq(
            tree["shell.rail.lab"]["current"],
            "page",
            "F: and the seat is the current one",
        )

        # ── (G) leaving and returning is a return ─────────────────────────
        banner("G — a section keeps what it had")
        rects = abs_rects_of(app.snapshot(source="paint"))
        target = next(tag for tag in rects if tag.startswith("lab.node."))
        press_at(app, rects[target])
        chosen = abs_rects_of(app.snapshot(source="paint"))
        go(app, "dashboard")
        go(app, "lab")
        again = abs_rects_of(app.snapshot(source="paint"))
        assert_eq(
            frozenset(again),
            frozenset(chosen),
            "G: the lab came back showing what it was showing -- the one row "
            "the reference toolkit gets right, and this must not regress it",
        )

        print(f"\n[demo] {len(CHECKS)} named check(s)")
        ok("the tool is one application at three of its seven seats", True)


def lab_selected(app: RpcSubprocess) -> str:
    """Which node the mounted lab has selected, from the lab's own wire."""
    return str(app.query(f"/{LAB_ROOT}{EXT}/selected"))


def lab_slot_answers(app: RpcSubprocess) -> bool:
    """Whether the lab's own surface is addressable on the wire right now.

    Addressed by the external's tag, which is how a host's extra surfaces are
    reachable — and the whole point of `ScreenRoster::externals`: a screen the
    journey is not at contributes no external, so there is no node for this to
    resolve against.
    """
    try:
        app.query(f"/{LAB_ROOT}{EXT}/spec")
    except Exception:  # noqa: BLE001 - any refusal shape means "not addressable"
        return False
    return True


run_demo("R1724 the tool is one application", body)
