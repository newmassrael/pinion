#!/usr/bin/env python3
"""R1916 §5.38 §5.40 §5.12 §2 #7 — **resting on a pin of the assembled tool
shows a sentence about it**, drawn and announced.

# What this demo exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives two debts at once, and they are one mechanism read from two sides:

* `debt-the-assembled-tool-mounts-no-tooltip` — the behaviour canon puts a
  `title` on 25 of its controls; the assembled tool published **zero** nodes of
  that role across six pages;
* the engine census's `node::GetPinHoverText` and
  `schema::ConstructBasicPinTooltip` — a port that can say what it is for.

# ★★★★★ Why the framework having a tooltip was not enough, measured

R695 built a tooltip widget and its own module docs say what it left out:

> the cross-widget "attach a tooltip to an arbitrary existing widget" primitive
> is a future axis once a 2nd consumer needs it

⇒ the tooltip was **its own anchor**. It knew when *it* was hovered and there
was no way to say *that mark over there has a sentence*. So the debt's
`standing_because` — "the framework already has the widget, nothing blocks
this" — was half true, and the half that was not is what `pinion_core::describe`
is.

# ⚠ And the reference's own composition does not happen

Read this round in the engine's schema: `ConstructBasicPinTooltip(Pin,
PinDescription, out Tooltip)` takes the description **from outside** and its
base implementation is `TooltipOut = PinDescription.ToString()` — while its own
comment promises it "tacks on any other data important to the schema (things
like the pin's type, etc.)". Here the type's half and the port's half are
composed by `Document::port_tooltip`, in one place, and this walk reads that
composition off the running screen.

# What this walk holds

  (A) the assembled tool mounts the lab, and resting on nothing shows nothing —
      the control, without which a screen showing a tooltip always would pass.
  (B) resting on a pin puts a description ON THE FRAME.
  (C) it is ANNOUNCED, and the pin POINTS AT it (`aria-describedby`) — a region
      nothing references is a region an AT never reads out.
  (D) the sentence carries BOTH halves: what the type says it carries, and what
      the port itself says it is for.
  (E) a split member says what IT is for and says that it is a half — which the
      reference cannot, its sub-pins being pins.
  (F) leaving takes it away.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:1 python3 tools/demos/r1916_a_pin_says_what_it_is_for.py
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
SEAT = "lab"
VIEWPORT = (1400, 900)
TIP = "lab.tip"

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


def cards(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/cards"))


def boxes(app: RpcSubprocess) -> dict:
    return abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))


def access(app: RpcSubprocess) -> list[dict]:
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result.get("nodes", [])


def tooltips(app: RpcSubprocess) -> list[dict]:
    # ⚠ The wire spells the role LOWERCASE (`tooltip`), which is the WAI-ARIA
    # spelling rather than the Rust variant's. A first draft of this walk
    # compared against `Tooltip` and reported zero on a screen that was
    # publishing one — an absence probe that answers `absent` for a name it
    # spelled differently is the exact failure this project's censuses keep
    # warning about, met here in a demo.
    return [n for n in access(app) if n.get("role") == "tooltip"]


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — resting on nothing shows nothing")
        # ★ The control. Without it this walk would pass on a screen that mounts
        # a tooltip permanently, which is not what a tooltip is.
        ok(
            f"A: no description region on the frame — {list(boxes(app).keys()).count(TIP)}",
            TIP not in boxes(app),
        )
        ok(
            f"A: and none announced — {len(tooltips(app))}",
            not tooltips(app),
        )

        banner("B/C — resting on a pin draws one, and announces it")
        subject = sorted(cards(app, surface))[0]
        pin = f"lab.pin.{subject}.dial"
        ok(f"B: the pin is on the frame ({pin})", pin in boxes(app))
        app.hover(path=pin)
        app.tick_ms(16)
        ok("B: ★ a description region is on the frame now", TIP in boxes(app))
        tips = tooltips(app)
        ok(f"C: exactly one is announced — {len(tips)}", len(tips) == 1)
        sentence = tips[0].get("name") or ""
        print(f"    {pin} says {sentence!r}")
        anchor = next((n for n in access(app) if n.get("tag") == pin), None)
        ok(f"C: the pin itself is announced ({pin})", anchor is not None)
        ok(
            "C: ★★★★★ and it POINTS AT the description — a region nothing "
            "references is a region an AT never reads out",
            anchor.get("described_by") == tips[0].get("tag"),
        )

        banner("D — the sentence carries both halves")
        ok(
            f"D: the TYPE's half is in it — {sentence!r}",
            "address" in sentence,
        )
        ok(
            "D: ★★★★★ and the PORT's own half, which the reference's base "
            "implementation drops on the floor",
            "hands on" in sentence,
        )

        banner("E — a split member says what IT is for, and that it is a half")
        splittable = next(
            name
            for name, row in sorted(cards(app, surface).items())
            if row["pins"]["splits"].get("dial") == "yes"
        )
        app.invoke(f"{surface}/split_pin", f"{splittable},dial")
        app.tick_ms(16)
        half = f"lab.pin.{splittable}.dial.host"
        ok(f"E: the member pin is on the frame ({half})", half in boxes(app))
        app.hover(path=half)
        app.tick_ms(16)
        tips = tooltips(app)
        ok(f"E: one description for the half — {len(tips)}", len(tips) == 1)
        said = tips[0].get("name") or ""
        print(f"    {half} says {said!r}")
        ok(
            f"E: it says what the HALF is for — {said!r}",
            "where to reach it" in said,
        )
        ok(
            "E: ★★★★★ and that the reader is looking at a half, which the "
            "reference cannot say — its sub-pins are pins",
            "split" in said,
        )
        ok(
            "E: and it is a DIFFERENT sentence from the whole pin's",
            said != sentence,
        )

        banner("F — moving off takes it away, and so does leaving the window")
        empty = boxes(app)["lab.canvas"]
        app.hover(at=(empty[0] + 4, empty[1] + empty[3] - 4))
        app.tick_ms(16)
        ok("F: moved onto bare canvas, the region is off the frame", TIP not in boxes(app))
        ok("F: and nothing is announced", not tooltips(app))

        # ★★★★★ And the pointer LEAVING is a different event from the pointer
        # moving, so it is asserted separately. A screen that only cleared on a
        # move would leave a description hanging over a window nobody is
        # pointing at — which is exactly the state WCAG 1.4.13's persistence
        # rule is about the reader being able to end.
        app.hover(path=pin)
        app.tick_ms(16)
        ok("F: resting again brings it back", TIP in boxes(app))
        app.pointer_leave()
        app.tick_ms(16)
        ok(
            "F: ★ and the pointer leaving the window takes it away too",
            TIP not in boxes(app),
        )

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1916_a_pin_says_what_it_is_for", body))
