#!/usr/bin/env python3
"""R1464 §5.16 §5.39 §2 #2 — a focus miss reports what it re-derived.

Drives `hello-window-refocus` — R1463's own two-window forcing consumer —
over JSON-RPC, and prices the fix that round landed.

R1463 widened the focus miss-retry to re-derive EVERY window the binding
has painted, because a painted window answers the enumeration from a
harvested cache the dispatch may have just invalidated. It documented the
cost as "one view run per painted window, paid on a miss, never per
frame". That sentence was prose, and prose is not a bound: nothing on any
surface could count the view runs, so the claim could not be checked and a
regression in it could not be seen. `r1463_window_refocus.py` says so
itself, under "What this demo does NOT cover".

`scene/frame_timings` now carries the pair:

    focus.derivations_total   view runs the focus enumeration performed
    focus.retries_total       requests that missed and forced a re-derive

Both are cumulative and binding-wide, read by DIFFERENCING across a call —
the same contract R1460 gave the `produce` totals. `derivations / retries`
IS the sentence above, as a number.

Verification scope (33 assertion sites, 41 executed — `_cost_of` carries
two that run on every call):

  (A) premise — both windows have really PAINTED (so both enumerations
      are caches, which is what makes the widening necessary), and the
      counters are binding-wide: identical from either window's snapshot.
  (B) the steady state is FREE — a `focus/set` naming an enumerated tag
      moves focus and costs nothing. Without this control, "a miss costs
      two" is indistinguishable from "every request costs two".
  (C) THE ROUND — a miss whose node appears in the SECONDARY window costs
      exactly 1 retry and 2 derivations: one view run per painted window.
  (D) the twin — the same miss in the PRIMARY window costs the SAME, and
      that is the point. The bound is the painted-window count, not the
      window that happens to own the node.
  (E) the ratio, stated directly, against the window count the demo
      measured for itself in (A).
  (F) orthogonality — reads move NEITHER off-frame counter, so an agent
      differencing them measures the binding rather than itself. The
      converse (a re-derive is not producer work) is deliberately left to
      the unit test and the reason is asserted rather than asserted
      around: driving a miss needs a path-addressed input, and that input
      is itself hit-tested against a produced scene.

## Counterfactual

Reverting R1463 (re-derive the primary only) leaves every focused-tag
assertion in `r1463_window_refocus.py` failing at (E) — and here, section
(C) fails on the NUMBER, `1` where `2` is required, before any focus
outcome is consulted. That is the difference this round buys: the defect
becomes a measurement rather than an absence.

Run from the workspace root:

    python3 tools/demos/r1464_focus_work.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-window-refocus"

MAIN = "main"
NOTES = "notes"
MAIN_VIEWPORT = (420, 260)
NOTES_VIEWPORT = (360, 220)

EDIT_TITLE = "edit_title"
EDIT_NOTE = "edit_note"
TITLE_EDITOR = "title_editor"
NOTE_EDITOR = "note_editor"
NOTES_PANE = "notes_pane"

#: Windows this binding paints. (A) asserts this is the real count rather
#: than trusting it, because it is the divisor every later section uses.
PAINTED_WINDOWS = 2


def _painted(tf: RpcSubprocess, window: str) -> bool:
    """True once `window` has produced a real frame.

    `scene/frame_timings` raises `FrameTimingsUnavailable` until the window
    paints, which makes it the honest witness for this demo's premise. A
    `{window}`-scoped snapshot cannot say it — that path RE-RUNS the view
    and answers even for a window never on screen (R1463).
    """
    try:
        return int(tf.frame_timings(window=window)["frame_count"]) >= 1
    except RpcError:
        return False


def _focus_work(tf: RpcSubprocess, window: str = MAIN) -> tuple[int, int]:
    """`(derivations_total, retries_total)` as of now."""
    focus = tf.frame_timings(window=window)["focus"]
    return int(focus["derivations_total"]), int(focus["retries_total"])


def _focused(tf: RpcSubprocess) -> Optional[str]:
    return tf.request("focus/get").result.get("focused")


def _snap(tf: RpcSubprocess, window: str) -> Any:
    viewport = MAIN_VIEWPORT if window == MAIN else NOTES_VIEWPORT
    return tf.snapshot(source="paint", viewport=viewport, window=window)


def _cost_of(tf: RpcSubprocess, action) -> tuple[int, int]:
    """`(derivations, retries)` spent by `action` — the difference-across-a-call
    reading the wire documents, performed the way a client would."""
    before_d, before_r = _focus_work(tf)
    action()
    after_d, after_r = _focus_work(tf)
    assert after_d >= before_d, "a cumulative total never decreases"
    assert after_r >= before_r, "a cumulative total never decreases"
    return after_d - before_d, after_r - before_r


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: both windows painted; counters binding-wide ──
        assert wait_until(
            lambda: _painted(tf, NOTES),
            desc="the notes window produces a real frame",
        ), "the secondary window has painted"
        assert _painted(tf, MAIN), "the primary window has painted"

        windows = tf.request("scene/windows", {}).result["windows"]
        declared = {w["id"] for w in windows}
        assert MAIN in declared, "the main window is declared"
        assert NOTES in declared, "the notes window is declared"
        assert_eq(
            len([w for w in (MAIN, NOTES) if _painted(tf, w)]),
            PAINTED_WINDOWS,
            "the painted-window count this demo divides by is MEASURED, not "
            "assumed — it is the bound every section below asserts against",
        )

        main_work = _focus_work(tf, MAIN)
        notes_work = _focus_work(tf, NOTES)
        assert_eq(
            main_work,
            notes_work,
            "the totals are BINDING-WIDE, not per-window: one focus "
            "enumeration spans every window, so both snapshots carry the "
            "same pair. A per-window split would be a different (wrong) "
            "claim about what the enumeration is",
        )
        assert main_work[0] >= 1, (
            "the boot seed already derived at least once — the enumeration "
            "exists before any paint (R26)"
        )

        # ── (B) the steady state is FREE ──────────────────────────────
        # `focus/set` names a tag the enumeration already holds. It moves
        # focus, so the call is not a no-op; it just costs no view run.
        tf.request("focus/set", {"tag": NOTES_PANE})
        assert_eq(_focused(tf), NOTES_PANE, "precondition: the set landed")

        hit_cost = _cost_of(tf, lambda: tf.request("focus/set", {"tag": EDIT_TITLE}))
        assert_eq(_focused(tf), EDIT_TITLE, "precondition: this set landed too")
        assert_eq(hit_cost, (0, 0), "a hit runs no view and is not a retry")

        # A pure read is free as well — introspection must not be able to
        # inflate the number a client is using to price its own input.
        read_cost = _cost_of(tf, lambda: _focused(tf))
        assert_eq(read_cost, (0, 0), "reading focus/get costs no focus work")

        # ── (C) THE ROUND — a miss in the SECONDARY window ────────────
        # One click writes `editing = Note` and requests NOTE_EDITOR. The
        # node is painted only as a RESULT of that dispatch, so the request
        # misses the enumeration and the re-derive is what makes it land.
        derivations, retries = _cost_of(tf, lambda: tf.click(path=EDIT_NOTE))
        assert_eq(
            retries,
            1,
            "exactly one request missed — the click's own focus_request",
        )
        assert_eq(
            derivations,
            PAINTED_WINDOWS,
            "and it re-derived EVERY painted window. `1` is the pre-R1463 "
            "primary-only refresh, the defect R1463 fixed and this round "
            "made visible; a number above the window count would mean the "
            "fold re-derived a window whose cache had just been written",
        )
        assert_eq(
            _focused(tf),
            NOTE_EDITOR,
            "and the work bought the outcome: the secondary window's "
            "just-appeared editor holds focus",
        )

        # ── (D) the twin — the same miss in the PRIMARY window ────────
        tf.key(path=EDIT_NOTE, name="Escape")
        assert_eq(_focused(tf), EDIT_NOTE, "the close names its trigger")

        derivations, retries = _cost_of(tf, lambda: tf.click(path=EDIT_TITLE))
        assert_eq(retries, 1, "one missed request, same as its twin")
        assert_eq(
            derivations,
            PAINTED_WINDOWS,
            "the SAME cost, though this node appears in the primary. The "
            "bound is the painted-window count, because the retry cannot "
            "know which window will answer until it has asked them all",
        )
        assert_eq(_focused(tf), TITLE_EDITOR, "the primary's editor holds focus")

        tf.key(path=EDIT_TITLE, name="Escape")
        assert_eq(_focused(tf), EDIT_TITLE, "back to the base state")

        # ── (E) the ratio, stated ─────────────────────────────────────
        before_d, before_r = _focus_work(tf)
        tf.click(path=EDIT_NOTE)
        tf.key(path=EDIT_NOTE, name="Escape")
        tf.click(path=EDIT_TITLE)
        tf.key(path=EDIT_TITLE, name="Escape")
        after_d, after_r = _focus_work(tf)
        spent_d, spent_r = after_d - before_d, after_r - before_r
        assert spent_r >= 2, f"the two opens both missed (saw {spent_r} retries)"
        assert_eq(
            spent_d,
            spent_r * PAINTED_WINDOWS,
            "over a whole interaction the identity holds exactly: "
            "derivations == retries * painted windows. This is R1463's "
            "documented bound, as arithmetic a client can check on any "
            "binding without reading pinion's source",
        )
        assert_eq(_focused(tf), EDIT_TITLE, "and the interaction ended where it began")

        # ── (F) orthogonality — three producers, three counters ───────
        # Reading must not move ANY work counter. An agent prices its own
        # input by differencing these, so a counter that its measuring
        # calls inflate would report the observer, not the binding.
        timings = tf.frame_timings(window=MAIN)
        frames_before = int(timings["frame_count"])
        produce_before = int(timings["produce"]["passes_total"])
        focus_before = _focus_work(tf)

        _snap(tf, MAIN)
        _snap(tf, NOTES)
        tf.request("scene/layout", {"viewport": None})
        _focused(tf)

        timings = tf.frame_timings(window=MAIN)
        assert_eq(
            _focus_work(tf),
            focus_before,
            "introspection derives no focus enumeration. Were the two folded "
            "together, an agent's own reads would price as if the user had "
            "missed a focus request",
        )
        assert_eq(
            int(timings["produce"]["passes_total"]),
            produce_before,
            "and these reads answered from the committed frame rather than "
            "producing a new one (R890.1), which the producer's own counter "
            "is what says",
        )
        # NOT asserted here: that these reads manufactured no FRAME. A live
        # window repaints on its own schedule, so `frame_count` drifts
        # between two RPC calls for reasons this demo does not control, and
        # an equality check on it would be measuring the compositor. The
        # claim is real and is pinned deterministically instead, by
        # `pinion_rpc::frame_timings::tests::r1464_focus_work_is_reported_and_is_neither_frames_nor_produce`
        # against an injected snapshot.

        # The converse — that a focus re-derive is not PRODUCER work — is
        # not isolable from here: driving a miss needs an input, and a
        # path-addressed input is itself hit-tested against a produced
        # scene, so the click moves both counters for two different
        # reasons. Measured, not assumed: the click below advances
        # `produce.passes_total` on its own account.
        produce_before = int(tf.frame_timings(window=MAIN)["produce"]["passes_total"])
        derivations, retries = _cost_of(tf, lambda: tf.click(path=EDIT_NOTE))
        assert_eq((derivations, retries), (PAINTED_WINDOWS, 1), "a miss, priced")
        assert int(tf.frame_timings(window=MAIN)["produce"]["passes_total"]) > produce_before, (
            "the click produced a scene to hit-test against — which is WHY "
            "this section cannot isolate the converse, and is the honest "
            "reason it is left to the unit test rather than approximated "
            "with a check that would pass for the wrong reason"
        )
        tf.key(path=EDIT_NOTE, name="Escape")

        # The frame the user DID see still reports its own work, untouched
        # by either off-frame counter.
        timings = tf.frame_timings(window=MAIN)
        assert int(timings["last"]["settle_passes"]) >= 1, (
            "a recorded frame ran at least one pass (0 is the never-measured "
            "sentinel and must not reach a sample)"
        )
        assert int(timings["frame_count"]) > frames_before, (
            "and the clicks above DID paint — so the frame ring and the "
            "off-frame counters were moving independently all along"
        )
        assert_eq(
            _focus_work(tf, NOTES),
            _focus_work(tf, MAIN),
            "the binding-wide reading still holds at the end of the run",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1464 §5.16 §5.39 — a focus miss reports what it re-derived", body))
