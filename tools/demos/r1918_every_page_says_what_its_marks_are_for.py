#!/usr/bin/env python3
"""R1918 §5.38 §5.40 §5.12 §2 #7 — **every page of the assembled tool has a
mark that says what it is for**, drawn and announced.

# What this demo exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1916 gave the tool its first described mark — the node lab's pins — and left
five pages saying nothing under a resting cursor. This closes
`debt-a-described-mark-exists-only-on-the-node-lab` on its own terms:

> 조립된 도구의 **모든 Page 목적지**에서, 설명을 나르는 마크가 하나 이상이고
> 그것이 walk 으로 구동된다. 한 화면이라도 0 이면 열려 있다.

# ★★★★★ Why the page's register and the chrome's are two values

The canon puts a `title` on twenty-five controls, and **nine of them are
chrome** — the icon rail's eight seats and the appearance toggle, drawn at every
destination. Reproducing those alone would satisfy "every destination has a
described mark" while every mounted page still said nothing, so the claim would
be true and worthless.

So the two populations are published separately (`chrome` / `page`) and this
walk asks only about `page`, and additionally checks that every mark it names is
painted **inside that destination's own page rectangle** — the rectangle the
host publishes as `page_at`. A page cannot satisfy this by pointing at the frame
around it.

# ⚠ The population is derived, never listed here

The destinations come from `/external/destinations`, which is the roster the
application navigates on. A hard-coded list of six would be a second spelling of
the rail, and a seat added later would leave this walk silently complete.

# What this walk holds

  (A) with the pointer away, no page shows a description — the control, without
      which a screen that mounts one permanently would pass.
  (B) EVERY open destination names at least one described mark of its OWN, and
      every one of them is painted inside that destination's page rectangle.
  (C) resting on one puts the sentence ON THE FRAME at that destination.
  (D) it is ANNOUNCED, and the mark POINTS AT it (`aria-describedby`).
  (E) the KEYBOARD half answers from the same register — a canon with zero key
      bindings is how a hover-only affordance quietly excludes a reader.
  (F) leaving takes it away, at every destination.

# ⚠ What (E) does NOT claim, measured

Driven this round by walking the Tab ring at all six destinations: a keyboard
reader is shown a description everywhere, and at four of the six the only ones
they can reach are the **chrome's** — the rail seats. The pages' own described
marks are mostly column headers, which are not Tab stops and are in no roster,
so a keyboard reader cannot rest on one. `settings` is the exception (its theme
segment is reachable). ⇒ registered as
`debt-a-described-mark-is-out-of-a-keyboard-readers-reach`; this walk asserts
the mechanism reaches the keyboard, not that every described mark does.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1918_every_page_says_what_its_marks_are_for.py
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
VIEWPORT = (1400, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def boxes(app: RpcSubprocess) -> dict:
    return abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))


def access(app: RpcSubprocess) -> list[dict]:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result.get("nodes", [])


def tooltips(app: RpcSubprocess) -> list[dict]:
    # ⚠ The wire spells the role LOWERCASE (`tooltip`) — the WAI-ARIA spelling
    # rather than the Rust variant's. R1916's walk recorded this after a first
    # draft compared against `Tooltip` and reported zero on a screen that was
    # publishing one.
    return [n for n in access(app) if n.get("role") == "tooltip"]


def destinations(app: RpcSubprocess) -> list[dict]:
    """Every OPEN destination, from the roster the application navigates on."""
    published = js(app.query(f"{EXT}/destinations"))
    return [row for row in published["destinations"] if row.get("open")]


def page_register(app: RpcSubprocess, row: dict) -> tuple[str, list[dict], tuple]:
    """`(region tag, the page's own described marks, the page rectangle)`.

    A mounted destination keeps its register on its own surface; a page this
    host paints itself keeps it in the host's `page` half. Two addresses, one
    question — and the host is what says which of the two a key is.
    """
    host = js(app.query(f"{EXT}/described"))
    at = tuple(host["page_at"])
    screen = row.get("screen")
    if screen:
        theirs = js(app.query(f"{screen['address']}/described"))
        return theirs["region"], theirs["marks"], at
    return host["region"], host["page"], at


def inside(rect: tuple, within: tuple) -> bool:
    x, y, w, h = rect
    wx, wy, ww, wh = within
    return x >= wx and y >= wy and x + w <= wx + ww and y + h <= wy + wh


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        rows = destinations(app)
        ok(
            f"the roster names the destinations this walk covers — {len(rows)}",
            len(rows) >= 2,
        )
        print("    open destinations: " + ", ".join(r["key"] for r in rows))

        for row in rows:
            key = row["key"]
            banner(f"{key}")
            app.intervene(f"{EXT}/nav", key)
            app.tick_ms(16)
            ok(f"{key}: the journey reaches it", app.query(f"{EXT}/nav") == key)

            # (A) the control, at EVERY destination rather than once: a screen
            # that mounted a description permanently would otherwise pass on
            # five of six.
            app.pointer_leave()
            app.tick_ms(16)
            region, marks, page_at = page_register(app, row)
            frame = boxes(app)
            ok(
                f"A {key}: no description region on the frame — {region}",
                region not in frame,
            )
            ok(f"A {key}: and none announced — {len(tooltips(app))}", not tooltips(app))

            # (B) the closing condition, and the half that keeps it honest.
            ok(
                f"B {key}: ★★★★★ the PAGE names at least one described mark of "
                f"its own — {len(marks)}",
                len(marks) >= 1,
            )
            painted = [m for m in marks if m["tag"] in frame]
            ok(
                f"B {key}: and they are painted here — {len(painted)} of "
                f"{len(marks)}",
                len(painted) >= 1,
            )
            for mark in painted:
                ok(
                    f"B {key}: {mark['tag']} is inside this page's own "
                    f"rectangle {page_at}, not on the chrome around it",
                    inside(frame[mark["tag"]], page_at),
                )
                ok(
                    f"B {key}: {mark['tag']} carries a sentence",
                    len(mark["sentence"]) > 0,
                )

            subject = sorted(painted, key=lambda m: m["tag"])[0]
            tag = subject["tag"]

            # (C)/(D) the mechanism, driven.
            app.hover(path=tag)
            app.tick_ms(16)
            ok(f"C {key}: ★ resting on {tag} puts a description on the frame", region in boxes(app))
            tips = tooltips(app)
            ok(f"D {key}: exactly one is announced — {len(tips)}", len(tips) == 1)
            said = tips[0].get("name") or ""
            print(f"    {tag} says {said!r}")
            ok(
                f"D {key}: and it is the sentence the register published",
                said == subject["sentence"],
            )
            anchor = next((n for n in access(app) if n.get("tag") == tag), None)
            ok(f"D {key}: the mark itself is announced", anchor is not None)
            ok(
                f"D {key}: ★★★★★ and it POINTS AT the description — a region "
                "nothing references is a region an AT never reads out",
                anchor.get("described_by") == tips[0].get("tag"),
            )

            # (F) leaving takes it away — the pointer leaving the window is a
            # different event from the pointer moving.
            app.pointer_leave()
            app.tick_ms(16)
            ok(f"F {key}: the pointer leaving takes it off the frame", region not in boxes(app))
            ok(f"F {key}: and nothing is announced", not tooltips(app))

        # (E) the keyboard half. The canon has ZERO key bindings, so a
        # hover-only description is how this affordance would quietly exclude a
        # reader — `Descriptions::shown` answers hover OR focus from one call,
        # and this is the assertion that the shell passes focus into it.
        #
        # ⚠ ★★★★★ **The stop is not the described mark, and that is the finding
        # this leg made.** A first draft called `focus/set` on a described mark
        # and was refused `tag_not_focusable`: every mark this application
        # describes — a rail seat, a card grip, a settings row — lives INSIDE a
        # Tab stop rather than being one. So the focus goes to the stop, and
        # what must answer is the stop's innermost active descendant, which is
        # the same thing the accessibility tree frames.
        banner("E — the keyboard reader is answered from the same register")
        app.intervene(f"{EXT}/nav", "dashboard")
        app.tick_ms(16)
        app.pointer_leave()
        app.tick_ms(16)
        ok("E: nothing is shown with the pointer away", not tooltips(app))
        described_here = {
            m["tag"] for m in js(app.query(f"{EXT}/described"))["chrome"]
        } | {m["tag"] for m in js(app.query(f"{EXT}/described"))["page"]}
        stop = "shell.rail"
        # An error is RAISED rather than returned, so reaching the next line is
        # the assertion that the stop took focus.
        app.request("focus/set", {"tag": stop})
        ok(f"E: focus reaches the stop {stop}", True)
        app.tick_ms(16)
        tips = tooltips(app)
        ok(
            "E: ★★★★★ a KEYBOARD reader is shown a sentence with no pointer "
            f"anywhere — {len(tips)}",
            len(tips) == 1,
        )
        anchor = next(
            (n for n in access(app) if n.get("described_by") == tips[0].get("tag")),
            None,
        )
        ok("E: the description names the mark it belongs to", anchor is not None)
        ok(
            f"E: and that mark is one the register describes — {anchor['tag']}",
            anchor["tag"] in described_here,
        )
        ok(
            f"E: it is INSIDE the stop, not the stop itself — {anchor['tag']} "
            f"vs {stop}",
            anchor["tag"] != stop and anchor["tag"].startswith(stop),
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1918_every_page_says_what_its_marks_are_for", body))
