#!/usr/bin/env python3
"""R2088 §5.16 §5.41 — **a second window says it reached the screen.**

The analysis tool's specification gives its cards a tear-off into an
independent window (R1826 built it, R1907 gave the detached panel a home a
hand can change). Every walk over that gesture so far has read the *encoded*
side of it: which windows are declared, which addresses the new window
paints, where it sits. None of them ever asked the new window the one
question a second window can answer differently from the first — **are your
frames actually reaching the screen?**

That gap is not academic. It is exactly where a defect has lived since R1934,
and — measured at R2088, by running the full sweep rather than assuming —
where it STILL lives: the round below repaired one layer of it and the class
survived, so this walk is a standing question rather than a victory lap. See
`debt-a-second-windows-surface-is-presented-before-it-is-configured`, which is
open.

* a full sweep failed ONE walk per run and a DIFFERENT walk each run, always
  one that detaches a panel into a second window — a population that is a
  command rather than a number (`grep -lE "tear_off|undock|detach"
  tools/demos/*.py`: 29 of 109 when the defect was registered, 47 of 725 at
  R2088), because a count written into prose is stale from the moment it is
  written;
* the harness saw `timeout waiting for response to scene/tick`, so it read as
  a slow machine;
* the log said the renderer had panicked inside `wgpu` with `Surface is not
  configured for presentation`. The timeout was the symptom; the cause was a
  **dead process**.

Measured at R2088 against `wgpu` 29's own source: `Surface::configure`
returns `()` and reports its refusal only through the device's error sink, so
nothing in pinion could tell a configure that worked from one that did not; a
refused configure leaves the surface *not configured for presentation*; and
acquiring on a surface that has never been configured successfully is
reported through `handle_error_fatal`, which consults no uncaptured-error
handler and aborts. The recovery ladder's heavy rung makes exactly such a
surface — which is why a freshly created window was the shape that died.

So the round made the outcome of every configure a measured fact, and this
walk is the half that reads it from the assembled application:

* **A** — the board before. One window, and it says it is presenting.
* **B** — tear off. The second window answers `scene/render_fidelity` **for
  itself** — proved by the viewport it reports, which is its own and not the
  board's — and what it answers is that it reached the screen.
* **C** — four generations. Tear off and redock repeatedly: each generation
  is a *new* surface created, configured and destroyed for a second window,
  which is the churn the defect lived in. Every generation is asked, and the
  process is still answering at the end — which, for a defect whose signature
  was a dead renderer, is itself the assertion.
* **D** — the vocabulary. A published miss is one the framework names,
  `unconfigured` and `device_lost` among them; and no window ends this walk in
  it. The set is checked in the direction that fails loudly: a name this walk
  does not know makes it red rather than quiet.

Against the reference toolkit at 6.11: a floated dock widget there is given a
top-level container whose paint device offers no per-window statement of
whether it is presenting at all — the nearest signal is the window's own
`isVisible`, which is about the window manager and not about the swapchain.
Section B is the axis where this is not merely equal.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r2088_a_second_window_says_it_reached_the_screen.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"
TORN = f"torn-{CARD}"

#: The framework's own roster of reasons a frame did not reach the screen —
#: `pinion_gpu::Missed::as_str`, whose spellings are pinned by that crate's
#: `the_wire_spellings_are_what_clients_read`. Kept here by hand, and that is
#: acceptable only because of the DIRECTION it rots in: a name added there and
#: not here makes section D fail loudly, never pass quietly.
#:
#: `unconfigured` and `device_lost` are R2088's additions and are the two arms
#: pinion raises itself, before asking `wgpu` — because asking is what aborted
#: the process. `device_lost` is separate from `lost` on purpose: a lost
#: swapchain is remade by the recovery ladder and a lost device is not remade
#: by anything, and a reader who cannot tell them apart cannot act on either.
MISSED_NAMES = {
    "outdated",
    "lost",
    "validation",
    "timeout",
    "occluded",
    "unconfigured",
    "device_lost",
}
RUNG_NAMES = {"reconfigured", "rebuilt", "repeated"}

#: How many tear-off / redock generations section C drives. Each one is a
#: whole second-window surface lifetime: created, configured, presented,
#: destroyed.
GENERATIONS = 4

CHECKS: list[str] = []

#: Every `(window, reason, rung)` this run actually SAW published. Printed at
#: the end, because section D can only judge what this host produced and a run
#: that observed none has proved the naming of nothing. Saying so is the
#: difference between a quiet gate and a vacuous one.
OBSERVED: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    assert condition, what
    CHECKS.append(what)


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
    for _ in range(8):
        app.tick_ms(16)


def fidelity(app: RpcSubprocess, window: str) -> dict:
    """The presented-frame record of ONE window.

    `scene/render_fidelity` is window-scoped, and this is the first walk to
    use that: every earlier reader asked the process and got the board.
    """
    resp = app.request("scene/render_fidelity", {"window": window})
    assert resp is not None and resp.result is not None, (
        f"scene/render_fidelity answers for {window!r}"
    )
    return resp.result


def reached_the_screen(app: RpcSubprocess, window: str, when: str) -> dict:
    """This window's own statement that its frames are being presented.

    Every clause is a relation between two published numbers rather than a
    restatement of one, so none of it can pass by the record simply being
    absent — `health` is asserted present first.
    """
    report = fidelity(app, window)
    health = report.get("health")
    ok(f"{when}/{window}: the frame record publishes presentability", isinstance(health, dict))
    missed, broken = health["missed_in_a_row"], health["broken_in_a_row"]
    ok(
        f"{when}/{window}: `presenting` agrees with the miss count "
        f"(presenting={health['presenting']}, missed={missed})",
        health["presenting"] == (missed == 0),
    )
    ok(
        f"{when}/{window}: breakages are a subset of misses ({broken} <= {missed})",
        broken <= missed,
    )
    reason, rung = health.get("last_missed"), health.get("last_rung")
    ok(
        f"D/{when}/{window}: the published reason is a named one ({reason!r})",
        reason is None or reason in MISSED_NAMES,
    )
    ok(
        f"D/{when}/{window}: the published rung is a named one ({rung!r})",
        rung is None or rung in RUNG_NAMES,
    )
    if reason is not None or rung is not None:
        OBSERVED.append(f"{when}/{window}: {reason}/{rung}")
    ok(
        f"D/{when}/{window}: the window is not sitting in a state whose "
        f"acquisition used to abort the process ({reason!r})",
        reason not in ("unconfigured", "device_lost"),
    )
    return report


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # ── (A) the board before ──────────────────────────────────────────
        banner("A — the board opens with one window, and it is on the screen")
        assert_eq(window_ids(app), ["main"], "A: the application opens with one window")
        board = reached_the_screen(app, "main", "A")
        ok(
            "A: ★★ and it says it is presenting -- the baseline every later "
            "reading is compared against",
            board["health"]["presenting"],
        )

        # ── (B) the second window answers for itself ──────────────────────
        banner("B — tearing a card off makes a window that reports its own screen")
        invoke(app, "act", f"{CARD},tear_off")
        settle(app)
        assert_eq(window_ids(app), ["main", TORN], "B: the topology grew a window")
        torn = reached_the_screen(app, TORN, "B")
        # ★★★★★ NON-VACUITY. `scene/render_fidelity` could ignore the window
        # scope and answer the board's record for any id, which would make
        # every assertion above a statement about `main` wearing another
        # name. The viewport in the record settles it: the record is one
        # object holding both, written on the winit paint path of the window
        # it belongs to, so a viewport that is not the board's is not the
        # board's record.
        ok(
            f"B: ★★★★★ the record is the TORN WINDOW'S -- it reports its own "
            f"viewport ({torn['viewport_w']}x{torn['viewport_h']}), not the "
            f"board's ({board['viewport_w']}x{board['viewport_h']})",
            (torn["viewport_w"], torn["viewport_h"])
            != (board["viewport_w"], board["viewport_h"]),
        )
        ok(
            "B: ★★★★★ and the second window says its frames REACHED THE SCREEN "
            "-- the question no walk over this gesture had ever asked, and the "
            "one whose answer used to be a panic rather than a frame",
            torn["health"]["presenting"],
        )
        ok(
            "B: ★★ with nothing having broken to get there (rebuilds="
            f"{torn['health']['rebuilds']}) -- a window that only presented "
            "after remaking its surface would be this defect, recovered from "
            "rather than absent",
            torn["health"]["rebuilds"] == 0 and torn["health"]["broken_in_a_row"] == 0,
        )
        invoke(app, "redock", CARD)
        settle(app)
        assert_eq(window_ids(app), ["main"], "B: and putting it back closes the window")

        # ── (C) four whole second-window surface lifetimes ────────────────
        banner(f"C — {GENERATIONS} generations of a second window, each one asked")
        for generation in range(1, GENERATIONS + 1):
            invoke(app, "act", f"{CARD},tear_off")
            settle(app)
            assert_eq(
                window_ids(app),
                ["main", TORN],
                f"C{generation}: the window is there",
            )
            fresh = reached_the_screen(app, TORN, f"C{generation}")
            ok(
                f"C{generation}: ★★★★★ generation {generation} of this window's "
                f"surface reached the screen -- a surface created, configured "
                f"and presented from scratch, which is the churn the intermittent "
                f"failure lived in",
                fresh["health"]["presenting"],
            )
            # The board is not collateral: a second window breaking used to
            # take the whole process with it, so the first window's own
            # statement is part of the evidence.
            reached_the_screen(app, "main", f"C{generation}")
            invoke(app, "redock", CARD)
            settle(app)
            assert_eq(
                window_ids(app),
                ["main"],
                f"C{generation}: and it is given back",
            )

        # ── (E) the process is still here ─────────────────────────────────
        banner("E — the application is still answering")
        # ★★ For a defect whose signature was a renderer that PANICKED, an
        # application that is still holding a conversation after eight second
        # -window lifetimes is not ceremony: on the pre-R2088 tree this is the
        # leg that could not be reached, because the abort arrived as an RPC
        # timeout and the walk simply stopped.
        final = reached_the_screen(app, "main", "E")
        ok(
            f"E: ★★★★★ the process survived {GENERATIONS + 1} second-window "
            f"lifetimes and the board is still presenting (paint_seq="
            f"{final['paint_seq']})",
            final["health"]["presenting"] and final["paint_seq"] > board["paint_seq"],
        )

    print(f"\n[demo] {len(CHECKS)} named check(s)")
    print(f"[demo] published misses observed: {OBSERVED or 'none — nothing broke on this host'}")


run_demo("R2088 a second window says it reached the screen", body)
