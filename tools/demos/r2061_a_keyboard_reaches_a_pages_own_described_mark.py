#!/usr/bin/env python3
"""R2061 §5.38 §5.40 §5.39 §5.12 §2 #7 — **a reader with no pointer reaches a
mark the PAGE describes, and is told what it is for.**

# What this demo exists for

R1918 gave all six pages of the assembled tool a described mark and measured,
in the same round, that four of them were out of a keyboard reader's reach:
what such a reader could stand on at `packets`, `keys`, `logs` and `lab` was
the shell's chrome, and the sentence they were shown was the rail seat's ⇒
`debt-a-described-mark-is-out-of-a-keyboard-readers-reach`.

Re-measured this round by walking the ring with `focus/next` before touching
anything, which is what that debt asks its repaying round to do first. Two
facts, neither of them read off the source:

  * at all four pages every ring stop showed NOTHING, and the only sentence a
    keyboard could reach anywhere was `Go to Dashboard` — the rail's.
  * the reason is not only that the marks are not stops. The screens were
    asking their register *which mark is the reader resting on* with the STOP,
    and no mark that carries a sentence IS a stop — each lives inside one. The
    shell learned that at R1918; its mounted screens did not.

This walk is the first page repaid: the capture list's column headings.

# ★★★★★ Why the heading row and not the marks

The seven headings cannot each be a Tab stop — that is seven stops for one row,
and the composite pattern exists precisely so a row costs one. So the row is the
stop and the arrows move inside it, which is also what the accessibility tree
has announced since R1694: a `row` of `columnheader`s. Until this round nothing
painted that row, so the tree described a room with no floor.

# ⚠ The population is derived, never listed here

Which marks belong to the page comes from the mounted screen's own register
(`/packet_view/external/described`), and which stops exist comes from walking
the ring. A list of seven headings written here would be a second spelling of
the screen's column table, and an eighth column would leave this walk silently
complete.

# What this walk holds

  (A) with no pointer and nothing focused, the page shows no description — the
      control, without which a screen mounting one permanently would pass.
  (B) ★ walking the ring with the KEYBOARD ALONE reaches a stop whose sentence
      is one of the PAGE's own marks — not the chrome's. This is the debt's
      closing condition for this page.
  (C) the arrows move along the row and the sentence follows, so the row is
      walkable rather than a single door.
  (D) walking it REORDERS NOTHING — the row declares explicit activation, and a
      row that sorted once per arrow would run six orderings on the way to the
      seventh column.
  (E) but `Enter` does reorder, from the keyboard, at the column the cursor is
      on: a heading a reader can reach and cannot press is a control below the
      floor rather than above it.
  (F) the mark POINTS AT the sentence, so an assistive technology reads it out.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 \\
        tools/demos/r2061_a_keyboard_reaches_a_pages_own_described_mark.py
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
EXT = "/external"

# The destination this instalment repays. The other three the debt names keep
# their own rounds; naming one here is what makes this walk's claim exact.
PAGE = "packets"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def access(app: RpcSubprocess) -> list[dict]:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result.get("nodes", [])


def tooltips(app: RpcSubprocess) -> list[dict]:
    # ⚠ The wire spells the role LOWERCASE — the WAI-ARIA spelling rather than
    # the Rust variant's, a distinction R1916's walk recorded after a first
    # draft reported zero on a screen that was publishing one.
    return [n for n in access(app) if n.get("role") == "tooltip"]


def focused(app: RpcSubprocess) -> str | None:
    resp = app.request("focus/get")
    return (resp.result or {}).get("focused") if resp and resp.result else None


def destination(app: RpcSubprocess, key: str) -> dict:
    rows = js(app.query(f"{EXT}/destinations"))["destinations"]
    row = next((r for r in rows if r["key"] == key), None)
    assert row is not None, f"the roster has no destination {key}"
    return row


def page_register(app: RpcSubprocess, row: dict) -> dict[str, str]:
    """The mounted page's OWN described marks, as `{tag: sentence}`.

    The host's `chrome` half is deliberately not merged in: the whole point of
    the debt is that a reader was reaching only that half, so a walk that could
    not tell the two apart could not see the defect it exists to refuse.
    """
    theirs = js(app.query(f"{row['screen']['address']}/described"))
    return {m["tag"]: m["sentence"] for m in theirs["marks"]}


def shown(app: RpcSubprocess) -> tuple[str | None, str | None]:
    """`(the mark being described, the sentence)` — from the ANNOUNCEMENT.

    Read out of the accessibility tree rather than from the register, because
    the register is what the screen *could* say and this walk is about what a
    reader *is told*.
    """
    tips = tooltips(app)
    if len(tips) != 1:
        return None, None
    region = tips[0].get("tag")
    anchor = next((n for n in access(app) if n.get("described_by") == region), None)
    return (anchor or {}).get("tag"), tips[0].get("name")


def walk_ring(app: RpcSubprocess, limit: int = 40) -> list[str]:
    """Every stop the keyboard reaches from here, in Tab order."""
    ring: list[str] = []
    for _ in range(limit):
        app.request("focus/next")
        app.tick_ms(16)
        here = focused(app)
        if here is None or here in ring:
            break
        ring.append(here)
    return ring


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        row = destination(app, PAGE)
        app.intervene(f"{EXT}/nav", PAGE)
        app.tick_ms(16)
        ok(f"the journey reaches {PAGE}", app.query(f"{EXT}/nav") == PAGE)

        marks = page_register(app, row)
        ok(f"the page publishes described marks of its own — {len(marks)}", len(marks) >= 1)

        # (A) the control. A pointer that never left would make every later
        # sentence a hover's, and a screen that mounted a description
        # permanently would pass the whole of (B).
        banner("A — nothing is shown before a reader arrives")
        app.pointer_leave()
        app.request("focus/set", {"tag": None})
        app.tick_ms(16)
        ok("A: no description is announced", not tooltips(app))

        # (B) the closing condition for this page: the KEYBOARD alone.
        banner("B — the keyboard walks to a mark the page describes")
        ring = walk_ring(app)
        ok(f"the ring has stops here — {len(ring)}", len(ring) >= 2)
        print("    ring: " + ", ".join(ring))

        reached: list[tuple[str, str, str]] = []
        for stop in ring:
            app.request("focus/set", {"tag": stop})
            app.tick_ms(16)
            mark, sentence = shown(app)
            if mark in marks:
                reached.append((stop, mark, sentence or ""))
        ok(
            "B: ★★★★★ a reader with NO POINTER reaches a mark this page "
            f"describes — {len(reached)} of {len(ring)} stop(s)",
            len(reached) >= 1,
        )
        stop, mark, sentence = reached[0]
        print(f"    {stop} -> {mark}: {sentence!r}")
        ok(
            f"B: and the sentence is the register's own for {mark}",
            sentence == marks[mark],
        )
        ok(
            "B: the stop is not the mark — a mark that carries a sentence "
            f"lives INSIDE a stop ({mark} vs {stop})",
            mark != stop and mark.startswith(stop.rsplit(".", 1)[0]),
        )

        # (F) the pointing half, asserted where the reader is standing — and
        # the walk has to GO BACK there. Surveying the ring left focus on its
        # last stop, so asking the tree now would ask about the chrome; the
        # first draft did exactly that and reported the mark unreferenced.
        app.request("focus/set", {"tag": stop})
        app.tick_ms(16)
        anchor = next((n for n in access(app) if n.get("tag") == mark), None)
        ok(f"F: the mark itself is announced — {mark}", anchor is not None)
        ok(
            "F: ★ and it POINTS AT the description; a region nothing "
            "references is a region an assistive technology never reads out",
            anchor.get("described_by") == tooltips(app)[0].get("tag"),
        )

        # (C) the row is WALKABLE — one door is not a row.
        banner("C — the arrows move along the row and the sentence follows")
        app.request("focus/set", {"tag": stop})
        app.tick_ms(16)
        before = shown(app)
        app.key(path=stop, name="ArrowRight")
        app.tick_ms(16)
        after = shown(app)
        ok(f"C: the arrow moves to another mark — {before[0]} -> {after[0]}", after[0] != before[0])
        ok(f"C: which is also this page's — {after[0]}", after[0] in marks)
        ok("C: and its own sentence comes with it", after[1] == marks[after[0]])

        # (D)/(E) what the row does to the capture, which is the reason its
        # activation policy is explicit rather than following.
        banner("D/E — walking reorders nothing; pressing does")
        order = app.query(f"{row['screen']['address']}/sort")
        app.key(path=stop, name="ArrowRight")
        app.tick_ms(16)
        app.key(path=stop, name="ArrowLeft")
        app.tick_ms(16)
        ok(
            "D: ★★★★★ walking the row leaves the capture's order alone — "
            f"{order!r}",
            app.query(f"{row['screen']['address']}/sort") == order,
        )
        here = shown(app)[0]
        app.key(path=stop, name="Enter")
        app.tick_ms(16)
        after_press = app.query(f"{row['screen']['address']}/sort")
        ok(
            f"E: ★★★★★ and pressing it DOES reorder, at {here} — "
            f"{order!r} -> {after_press!r}",
            after_press != order,
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r2061_a_keyboard_reaches_a_pages_own_described_mark", body))
