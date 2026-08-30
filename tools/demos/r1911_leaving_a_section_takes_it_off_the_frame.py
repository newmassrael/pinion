#!/usr/bin/env python3
"""R1911 §5.34 §5.12 §2 #2 §2 #7 — **leaving a section takes that section off
the frame, and the assembled tool says where every section's marks are so a
client can check it.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This one is the structural half of
`debt-the-canon-is-one-app-and-we-are-many-binaries`: the behaviour canon is ONE
application whose sections come and go inside one shell, and the property that
makes that true — arriving paints a section and leaving takes it away — was
being asserted of four of this tool's six sections, thinly.

# ★★★★★ What the entry re-measurement found, and it was worse than the debt said

The debt carried R1784's honest remainder: what a host-painted page lacks
against a mounted screen is *its own paint root, hit testing, keys and an
accessibility subtree*, and the question is which of those it actually owes.
Measured this round by asking the assembled tool at each of its six open
destinations, over the wire at 1440x900:

    dashboard root=None   :: card.*(260) shell.palette(50) shell.subbar(5)
                             match.spark(2)  + host chrome
    settings  root=None   :: shell.settings(40)             + host chrome
    packets   root=Some("packet_view") :: packet_view(1) pv.*(292)
    keys      root=Some("key_patterns") :: key_patterns(1) kp.*(106)
    logs      root=Some("log_view")     :: log_view(1)     lv.*(98)
    lab       root=Some("node_lab")     :: node_lab(1)     lab.*(222)

Two findings, and the second was not in the debt at all:

1. The two pages the host paints itself have **no** paint root, so R1729's
   check — which walks `mounted_keys` — never included them. Nothing anywhere
   asserted that leaving the dashboard stops the dashboard being painted.
2. ★★★★★ **The mounted screens' marks are not under their root tag either.**
   `Screen::tag` is required to be on the scene *somewhere*; nothing requires
   the scene to hang beneath it, and none of the four does. So the away-check
   that did run was asserting about a **single marker node** while 292, 106, 98
   and 222 marks sat at an address it never looked at. It was true and nearly
   empty.

⚠ `lab.*` READS 222 HERE AND THE FIRST DRAFT OF THIS FILE SAID 112, and the
difference is not a re-count: it is R1911.1's repair. The node lab's inspector
was opening FOLDED, so 110 of its marks were not painted at all — which is the
same one line that had 33 demo walks red for three CI rounds.

⇒ the answer to the debt's question is **the paint root**, and it is the one of
the four the other three can only be asked *about*: there is no "does this
section own this press" or "is this subtree this section's" until "where are
this section's marks" has an answer.

# Why the runtime cannot infer this, and why a gate must

A judge is HANDED `Showing` rather than deriving away from finding nothing,
because R1761 refused that inference: a page that stopped painting half of
itself would report exactly what a page nobody is looking at reports. That is
right, and it leaves the handed-over claim untested — a claim a walk can check
is one a walk should check. This is the walk.

# The escape hatch, and what closes it

`WidgetCore::paint_stems` defaults to `vec![tag()]`, which is how the assumption
above survived. A default nobody checks is an escape hatch, so the roster also
publishes what the HOST paints at every destination, and this walk asserts that
**every mark on the frame belongs to some section or to that chrome**. A screen
that leaves its real family undeclared does not pass a thinner check; its marks
turn up here by name.

# Superior to the floor

The floor toolkit's paged container is addressed by ordinal, its current page
publishes no accessible value, and nothing on that class names which widgets
belong to which page — a client cannot ask "is page 3 still on the frame", only
"which index is set". Here every section publishes the stems its marks are
addressed under, the host publishes its own, and the two together make
"unaccounted for" an answerable question rather than an unstated assumption.

# What this walk holds

  (A) the assembled tool publishes, for EVERY open section, where that
      section's marks are — not only for the ones that are whole screens.
  (B) arriving at a section paints marks the tool said were that section's.
  (C) leaving takes them away: at each destination, no OTHER section's marks
      are on the frame. This is the claim that had no check at all for the two
      pages the host paints itself, and a nearly empty one for the four mounts.
  (D) the host's chrome survives every arrival — a page is a page, not a
      takeover.
  (E) nothing on any frame belongs to nobody, which is what keeps (C) from
      being satisfiable by declaring less.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1911_leaving_a_section_takes_it_off_the_frame.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
    walk_nodes,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
VIEWPORT = (1440, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def under(tag: str, stem: str) -> bool:
    """Whether `tag` is addressed at or beneath `stem`.

    The same rule `ScreenRoster::paints` applies, spelled once here because a
    client reading a published stem has to apply it too. A bare `startswith`
    would take `board.cardinal` for a `board.card` mark.
    """
    return tag == stem or tag.startswith(f"{stem}.")


def roster(app: RpcSubprocess) -> dict:
    answer = app.query(f"{EXT}/destinations")
    return answer if isinstance(answer, dict) else json.loads(answer)


def painted_tags(app: RpcSubprocess) -> set[str]:
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    return {found["tag"] for _, found in walk_nodes(snap) if found.get("tag")}


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        start = roster(app)

        banner("A — every open section says where its marks are")
        rows = start["destinations"]
        open_rows = [row for row in rows if row["open"]]
        chrome = start.get("chrome_paints")
        ok(
            f"A: the host publishes its own chrome stems — {chrome}",
            isinstance(chrome, list) and len(chrome) > 0,
        )
        silent = [row["key"] for row in open_rows if not row.get("paints")]
        ok(
            f"A: all {len(open_rows)} open section(s) publish their stems, "
            f"none silent",
            not silent,
        )
        mounted = [row["key"] for row in open_rows if row["mounted"]]
        ok(
            f"A: {len(open_rows)} open, {len(mounted)} of them mounted — so "
            f"this walk covers sections a mounted-only population misses",
            len(open_rows) > len(mounted),
        )
        # ★ The finding that is not in the debt: a mounted screen's marks are
        # not under its root tag, so a claim of exactly one stem would be the
        # thin check this walk replaces.
        thin = [
            row["key"]
            for row in open_rows
            if row["mounted"] and len(row["paints"]) < 2
        ]
        ok(
            f"A: every mounted section declares more than its root tag; a "
            f"one-stem claim would be the marker-node check — {thin} claim one",
            not thin,
        )

        stems = {row["key"]: row["paints"] for row in open_rows}
        print(f"    chrome: {chrome}")
        for key, held in stems.items():
            print(f"    {key}: {held}")

        for row in open_rows:
            key = row["key"]
            banner(f"B/C/D/E — at {key}")
            app.intervene(f"{EXT}/nav", key)
            app.tick_ms(16)
            at = app.query(f"{EXT}/nav")
            at = at if isinstance(at, str) else json.loads(at)
            ok(f"B: the tool is at {key}", at == key)

            tags = painted_tags(app)

            mine = sorted(
                tag
                for tag in tags
                if any(under(tag, stem) for stem in stems[key])
            )
            ok(
                f"B: {key} paints {len(mine)} mark(s) under the stems it "
                f"published, so arriving is distinguishable from not",
                bool(mine),
            )

            for other_key, other_stems in stems.items():
                if other_key == key:
                    continue
                trespass = sorted(
                    tag
                    for tag in tags
                    if any(under(tag, stem) for stem in other_stems)
                )
                ok(
                    f"C: at {key}, {other_key} is off the frame — "
                    f"{len(trespass)} of its marks painted",
                    not trespass,
                )

            for band in ("shell.appbar", "shell.rail", f"shell.rail.{key}"):
                ok(f"D: at {key}, the host still paints {band}", band in tags)

            orphans = sorted(
                tag
                for tag in tags
                if not any(under(tag, stem) for stem in chrome)
                and not any(
                    under(tag, stem)
                    for held in stems.values()
                    for stem in held
                )
            )
            ok(
                f"E: at {key}, every one of {len(tags)} painted mark(s) "
                f"belongs to a section or to the host's chrome — "
                f"unaccounted: {orphans}",
                not orphans,
            )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1911_leaving_a_section_takes_it_off_the_frame", body))
