#!/usr/bin/env python3
"""R1752 §5.16 — **a frame record says which function `render_us` measured.**

## What forced this demo

R1752 spent three profiles failing to answer whether font-table lookup is
still a top cost, and the answer came from the app's own frame accounting
instead. Reading that accounting is what exposed the defect this demo pins.

`render_us` brackets the submit, and the submit is an EITHER/OR: a frame runs
`capture_rgba8` or `render`, never both. So one field has been reporting two
different functions under one name, and nothing on the wire said which.

Measured while finding it, on one window, 60 frames each way: driven by ticks
the mean render was 8,452us; driven by screenshots, 10,344us. The same field,
22% apart, for a reason no consumer could see.

★ And the reading most likely to be taken was the one least interpretable: an
agent asks for screenshots, so an agent's frame timings were capture frames.

## What is proved here, over RPC

The two regimes are driven on ONE window and the flag is read back from
`scene/frame_timings` in each:

  * ticks paint without asking for pixels -> `captured` is False;
  * `scene/screenshot` copies the texture back -> `captured` is True;
  * the surrounding accounting is unchanged by the flag (build / encode /
    acquire keep their meaning, and the phase partition still holds).

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1752_a_frame_says_which_function_it_timed.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo

TICKS = 40
SHOTS = 40


def timings(tf) -> dict:
    """The `scene/frame_timings` payload as a plain dict."""
    r = tf.request("scene/frame_timings")
    return getattr(r, "result", r)


def check_last_shape(last: dict, where: str) -> None:
    """Every field this demo reads about, present and of the right kind."""
    for name in ("build_us", "encode_us", "acquire_us", "render_us", "total_us", "other_us"):
        assert name in last, f"{where}: `{name}` missing from the frame record"
        assert isinstance(last[name], int), f"{where}: `{name}` is not an integer"
    assert "captured" in last, f"{where}: `captured` missing — the flag IS the round"
    assert isinstance(last["captured"], bool), f"{where}: `captured` is not a boolean"


def body(tf) -> None:
    # ---- regime A: the app paints on its own clock ----------------------
    for _ in range(TICKS):
        tf.request("scene/tick", {"dt": 0.05})
    ticked = timings(tf)
    check_last_shape(ticked["last"], "ticked")
    # ★ Asserted on the WINDOW, not on `last`. The first draft read
    # `last["captured"]` and FAILED: `last` is the most recent frame recorded,
    # and an idle paint lands between the request and the read, so a client
    # cannot use it to prove anything about its own request. A count over the
    # window cannot be stolen that way -- which is why R1752 added it.
    assert_eq(
        ticked["window"]["captured_frames"], 0, "no ticked frame captured"
    )

    # The partition still holds, so the flag did not disturb the accounting
    # it exists to make readable.
    t = ticked["last"]
    assert_eq(
        t["total_us"],
        t["build_us"] + t["encode_us"] + t["acquire_us"] + t["render_us"] + t["other_us"],
        "ticked: total is its phases",
    )
    assert t["render_us"] > 0, "ticked: a painted frame spent time submitting"
    assert t["total_us"] >= t["render_us"], "ticked: the whole is at least the part"
    assert ticked["frame_count"] >= TICKS, "ticked: the window saw the frames driven"

    # ---- regime B: every frame also copies its texture back -------------
    for _ in range(SHOTS):
        tf.request("scene/screenshot", {"path": ""})
    shot = timings(tf)
    check_last_shape(shot["last"], "shot")
    assert shot["window"]["captured_frames"] > 0, (
        "screenshot frames must be counted as readbacks in the window"
    )

    s = shot["last"]
    assert_eq(
        s["total_us"],
        s["build_us"] + s["encode_us"] + s["acquire_us"] + s["render_us"] + s["other_us"],
        "shot: total is its phases",
    )
    assert s["render_us"] > 0, "shot: a captured frame spent time submitting"
    assert s["total_us"] >= s["render_us"], "shot: the whole is at least the part"
    assert shot["frame_count"] > ticked["frame_count"], "shot: more frames were driven"

    # ---- the discrimination itself --------------------------------------
    # ★ The point is not that one is slower. It is that the RECORD says which
    # function ran, so a reader never has to infer it from a duration — which
    # is what nobody could do before, and what a duration cannot support
    # anyway on a machine under different load.
    assert shot["window"]["captured_frames"] > ticked["window"]["captured_frames"], (
        "the two regimes are distinguishable on the wire"
    )
    assert (
        shot["window"]["captured_frames"] <= shot["window_len"]
    ), "a window cannot hold more readbacks than samples"

    # The phases that describe THIS frame's own work keep their meaning across
    # the two regimes — the flag discriminates the submit, not the build.
    assert t["build_us"] > 0, "ticked: the view ran"
    assert s["build_us"] > 0, "shot: the view ran"
    assert t["encode_us"] > 0, "ticked: the scene was encoded"
    assert s["encode_us"] > 0, "shot: the scene was encoded"

    # And the window aggregate still describes the same window it always did.
    for label, payload in (("ticked", ticked), ("shot", shot)):
        w = payload["window"]
        assert w["mean_total_us"] >= w["min_total_us"], f"{label}: mean is above the floor"
        assert w["max_total_us"] >= w["mean_total_us"], f"{label}: ceiling is above the mean"
        assert w["mean_render_us"] > 0, f"{label}: the window submitted frames"
        assert payload["window_len"] > 0, f"{label}: the window holds samples"

    print(
        f"[r1752] ticked mean_render={ticked['window']['mean_render_us']}us "
        f"captured_frames={ticked['window']['captured_frames']} | "
        f"shot mean_render={shot['window']['mean_render_us']}us "
        f"captured_frames={shot['window']['captured_frames']}"
    )


def main() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        body(tf)


if __name__ == "__main__":
    run_demo("r1752 a frame says which function it timed", main)
