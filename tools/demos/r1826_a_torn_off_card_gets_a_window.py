#!/usr/bin/env python3
"""R1826 §5.16 §5.41 §5.21 — **a torn-off card gets a real window.**

The analysis tool's board has had a tear-off since R1648 and a floating panel
since R1697. What it did not have is the other half of its own specification —
*widget = independent card (… tear off …) · multi-window (tear off -> independent
window, always-on-top option)*. Measured before this round, by driving the
running application: `scene/windows` declared exactly ONE window, `main`, at
boot AND after `act packet#0,tear_off`. The card left the board and appeared as
`float.packet#0`, a panel painted inside the canvas, while the screen said
"packet#0 -> detached window" in a sentence a reader could see and no window
existed to match.

What this script walks, and why each leg is here:

* **A** — the board before. One window; every specified card on it.
* **B** — tear off. The topology GROWS a window; the card leaves the board; and
  the new window paints THAT CARD. Read from the produced scene of the new
  window rather than from the state that asked for it: a binding that minted a
  spec and painted nothing would pass every assertion about the spec.
* **C** — the published correspondence. `detached` answers which window carries
  which card, so a client that never saw the gesture can find the window.
* **D** — redock. The window goes, the card comes back, and `detached` empties.

Against the reference toolkit at 6.11: a floated dock widget there is given a
top-level container, and the correspondence between panel and window lives in
whatever the caller wrote down — the nearest available signal is walking the
parent chain of a widget the caller already holds, which cannot answer for a
panel it does not hold. Leg C is the axis where this is not merely equal.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1826_a_torn_off_card_gets_a_window.py
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

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"
TORN = f"torn-{CARD}"
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def window_ids(app: RpcSubprocess) -> list[str]:
    resp = app.request("scene/windows", {})
    assert resp is not None, "scene/windows returned no response"
    return [w["id"] for w in resp.result["windows"]]


def invoke(app: RpcSubprocess, path: str, args: str) -> object:
    resp = app.request("scene/invoke", {"path": f"/analyzer_shell{EXT}/{path}", "args": args})
    assert resp is not None, f"{path} returned no response"
    return resp.result


def settle(app: RpcSubprocess) -> None:
    """Let the topology Effect run and the shell reconcile its windows."""
    for _ in range(6):
        app.tick_ms(16)


def board_cards(app: RpcSubprocess) -> list[str]:
    rects = abs_rects_of(app.snapshot(source="paint"))
    return sorted(t for t in rects if t.startswith("card.") and t.count(".") == 1)


def painted_in(app: RpcSubprocess, window: str) -> list[str]:
    return sorted(abs_rects_of(app.snapshot(source="paint", window=window)))


def detached(app: RpcSubprocess) -> list[dict]:
    return app.query(f"{EXT}/detached")


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # ── (A) before ────────────────────────────────────────────────────
        banner("A — the board before: one window, every card on it")
        assert_eq(window_ids(app), ["main"], "A: the application opens with one window")
        before = board_cards(app)
        ok(f"A: the board paints {len(before)} cards", f"card.{CARD}" in before)
        assert_eq(detached(app), [], "A: and nothing is detached")

        # ── (B) tear off ──────────────────────────────────────────────────
        banner("B — tearing a card off opens a window that carries it")
        invoke(app, "act", f"{CARD},tear_off")
        settle(app)
        assert_eq(
            window_ids(app),
            ["main", TORN],
            "B: ★★★★★ the declared topology GREW a window -- which is the half "
            "this assembly did not have: before this round it stayed ['main'] "
            "and the card became a picture inside the canvas",
        )
        after = board_cards(app)
        ok(
            "B: and the card left the board",
            f"card.{CARD}" not in after and len(after) == len(before) - 1,
        )
        # ★ Read from the NEW WINDOW'S OWN SCENE. A binding that minted a spec
        # and painted nothing into it would satisfy every assertion above.
        tags = painted_in(app, TORN)
        ok(f"B: ★★★★★ the new window paints the card ({len(tags)} regions)", f"torn.{CARD}" in tags)
        ok("B: including its detached badge", f"torn.{CARD}.badge" in tags)
        # ★ The card's OWN regions are `card.<id>.…` — the same addresses the
        # board painted, because it is the same body function. That is the
        # property a tear-off is for, so it is asserted rather than assumed:
        # a window that had re-drawn the card under new names would be a second
        # card wearing one id.
        mine = [t for t in tags if t.startswith(f"card.{CARD}.")]
        ok(
            f"B: ★★ and the card's own body came with it, under the addresses "
            f"the board used ({len(mine)} regions) -- a card is not a different "
            f"card for having left the board",
            len(mine) > 10,
        )
        strangers = [
            t
            for t in tags
            if t.startswith("shell.")
            or (t.startswith("card.") and not t.startswith(f"card.{CARD}."))
        ]
        ok(
            "B: ★★ and the window carries ONLY that card: the board, the rail "
            f"and the application bar are the main window's ({strangers})",
            not strangers,
        )

        # ── (C) the published correspondence ──────────────────────────────
        banner("C — the board says what is detached and where it went")
        assert_eq(
            detached(app),
            [{"card": CARD, "window": TORN}],
            "C: ★★★★★ the card names its window, so a client that never saw the "
            "gesture can snapshot it -- the axis the reference toolkit has no "
            "accessor for at all",
        )
        # And it is not a caption: the id it publishes is the id that answers.
        ok(
            "C: ★★ the published window id is the one `scene/windows` declares",
            detached(app)[0]["window"] in window_ids(app),
        )

        # ── (C2) the always-on-top option ─────────────────────────────────
        banner("C2 — the specification's always-on-top option, per panel")
        levels = {w["id"]: w["level"] for w in app.request("scene/windows", {}).result["windows"]}
        assert_eq(
            levels[TORN],
            "normal",
            "C2: a torn-off window arrives at ordinary stacking -- a window "
            "that landed on top of everything would be a behaviour a reader "
            "discovers rather than a decision they make",
        )
        assert_eq(invoke(app, "on_top", CARD), True, "C2: the option turns on")
        settle(app)
        levels = {w["id"]: w["level"] for w in app.request("scene/windows", {}).result["windows"]}
        assert_eq(
            levels[TORN],
            "always_on_top",
            "C2: ★★★★★ and the DECLARED level moved -- the second half of the "
            "census row's capability, which said `tear-off to an independent "
            "window, with an always-on-top option`",
        )
        assert_eq(
            levels["main"],
            "normal",
            "C2: ★★ and only that panel's -- the option is per detached card, "
            "because the point is watching ONE readout over other work",
        )
        assert_eq(invoke(app, "on_top", CARD), False, "C2: and it turns off again")
        settle(app)
        levels = {w["id"]: w["level"] for w in app.request("scene/windows", {}).result["windows"]}
        assert_eq(
            levels[TORN],
            "normal",
            "C2: ★★ a control that does a thing undoes it -- R1697's rule for "
            "the maximise control, applied to this one",
        )

        # ── (D) redock ────────────────────────────────────────────────────
        banner("D — putting it back closes the window")
        invoke(app, "redock", CARD)
        settle(app)
        assert_eq(
            window_ids(app),
            ["main"],
            "D: ★★★★★ the window is gone -- a tear-off that could not be undone "
            "would be a leak with a gesture in front of it",
        )
        assert_eq(detached(app), [], "D: and nothing is detached")
        assert_eq(
            board_cards(app),
            before,
            "D: ★★ and the board is exactly what it was -- the same set, not "
            "merely the same count",
        )

    print(f"\n[demo] {len(CHECKS)} named check(s)")
    ok("a torn-off card gets a window, says so, and gives it back", True)


run_demo("R1826 a torn-off card gets a window", body)
