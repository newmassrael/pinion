#!/usr/bin/env python3
"""R889 §5.49 §5.16 — window-known predicate SSOT: the unknown-window gate.

R888.1's adversarial session review found a cross-cutting category error:
per-window READ-axis availability piggybacked on the InputRouter registry
("has painted at least once") while writes consulted nothing, and the
production GUI shell silently aliased unknown window ids onto the primary
(`resolve_spec_id`) — so the substrate's honesty gates were unreachable
in production and `scene/set_fps {window: "bogus", fps: 0}` FROZE THE
PRIMARY's game loop.

R889 gives window-known-ness ONE home: `CoreShell::window_owners` becomes
the window registry (seeded with the primary; secondaries registered at
OS-window creation in `AppShell::resume_spec`, removed by the reconcile
drop pass), `CoreShell::is_window_known` is the named predicate, and the
dispatcher rejects a request scoped to an unknown window with `-32602
unknown_window` BEFORE method routing — READ and WRITE share the gate.
`resolve_spec_id` (the silent-alias judgment site) is deleted outright.

Verification scope (33 assertions, counted exactly; gates per
[[zero-flake-policy]] — action→assert edges poll observed state):

  (A) known windows answer the per-window READ axes — main AND the
      secondary inspector window (pacing default-policy, input-state
      with the full key set).                                          (8)
  (B) per-window pacing isolation round-trip — overrides install,
      read back, and clear PER WINDOW without crosstalk.              (10)
  (C) unknown window READ rejection — pacing_state / input_state /
      cache_stats / scene/query all reject `window: "bogus"` with
      -32602 unknown_window naming the supplied id.                    (8)
  (D) unknown window WRITE rejection + primary protection — the
      pre-R889 production bug: set_fps {window:"bogus", fps:0} errors
      and the primary's pacing stays untouched.                        (4)
  (E) wire edges — empty-string id rejected; the gate leaves no
      registration residue (known windows still answer afterwards).   (3)
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
    assert_input_axes,
    run_demo,
    wait_until,
)


def _input_state(tf: RpcSubprocess, window: Optional[str] = None) -> dict[str, Any]:
    params: dict[str, Any] = {}
    if window is not None:
        params["window"] = window
    resp = tf.request("scene/input_state", params)
    assert resp is not None and isinstance(resp.result, dict)
    return resp.result


def _expect_unknown_window(label: str, supplied: str, fn) -> None:
    """One unknown-window edge = 2 assertions (code+message, data)."""
    try:
        fn()
    except RpcError as err:
        assert_eq(
            (err.code, err.message),
            (-32602, "unknown_window"),
            f"{label}: rejected with the gate's error shape",
        )
        assert_eq(err.data, supplied, f"{label}: error data names the supplied id")
        return
    raise AssertionError(f"{label}: expected unknown_window rejection, got success")


#: R1627 — the `scene/input_state` axes this demo actually reads. Declared here
#: so the assertion names its own dependency; the whole-set census lives beside
#: the emitter (`pinion_rpc::dispatch::INPUT_STATE_AXES`).
USES = ("cursor", "held_keys", "key_dispatch", "modifiers")


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) known windows answer the per-window READ axes ────────
        r_main = tf.pacing_state(window="main")
        assert_eq(sorted(r_main.keys()), ["fps"], "main pacing carries the fps axis")   # 1
        assert_eq(r_main["fps"], None, "main boots on the default policy")              # 2
        r_insp = tf.pacing_state(window="inspector")
        assert_eq(sorted(r_insp.keys()), ["fps"], "inspector pacing axis present")      # 3
        assert_eq(r_insp["fps"], None, "inspector boots on the default policy")         # 4
        s_main = _input_state(tf, "main")
        # R1627 — the axes THIS demo reads (see r885 for why not equality).
        assert_input_axes(s_main, needs=USES, label="main input_state")                  # 5
        assert_eq(s_main["held_keys"], [], "no chord key held at boot (main)")          # 6
        s_insp = _input_state(tf, "inspector")
        # The point here is that a KNOWN window answers at all rather than
        # refusing — so the assertion is that the axes arrived, not that the
        # set has never grown.
        assert_input_axes(
            s_insp,
            needs=USES,
            label="inspector input_state available (known window, NOT Unavailable)",
        )                                                                               # 7
        assert_eq(s_insp["held_keys"], [], "no chord key held at boot (inspector)")     # 8

        # ── (B) per-window pacing isolation round-trip ───────────────
        tf.set_fps(30, window="inspector")
        wait_until(
            lambda: tf.pacing_state(window="inspector")["fps"] == 30,
            desc="inspector override 30 reads back",
        )                                                                               # 9
        assert_eq(
            tf.pacing_state(window="inspector")["fps"], 30,
            "inspector override installed",
        )                                                                               # 10
        assert_eq(
            tf.pacing_state(window="main")["fps"], None,
            "main unaffected by the inspector write (per-window isolation)",
        )                                                                               # 11
        tf.set_fps(144, window="main")
        wait_until(
            lambda: tf.pacing_state(window="main")["fps"] == 144,
            desc="main override 144 reads back",
        )                                                                               # 12
        assert_eq(tf.pacing_state(window="main")["fps"], 144, "main override installed")  # 13
        assert_eq(
            tf.pacing_state(window="inspector")["fps"], 30,
            "inspector keeps its own override (no crosstalk)",
        )                                                                               # 14
        tf.set_fps(None, window="inspector")
        wait_until(
            lambda: tf.pacing_state(window="inspector")["fps"] is None,
            desc="inspector clear restores default policy",
        )                                                                               # 15
        assert_eq(
            tf.pacing_state(window="main")["fps"], 144,
            "main override survives the inspector clear",
        )                                                                               # 16
        tf.set_fps(None, window="main")
        wait_until(
            lambda: tf.pacing_state(window="main")["fps"] is None,
            desc="main clear restores default policy",
        )                                                                               # 17
        assert_eq(
            tf.pacing_state(window="inspector")["fps"], None,
            "both windows back on default policy",
        )                                                                               # 18

        # ── (C) unknown window READ rejection ────────────────────────
        _expect_unknown_window(
            "pacing_state", "bogus", lambda: tf.pacing_state(window="bogus")
        )                                                                          # 19, 20
        _expect_unknown_window(
            "input_state", "bogus", lambda: _input_state(tf, "bogus")
        )                                                                          # 21, 22
        _expect_unknown_window(
            "cache_stats", "bogus", lambda: tf.cache_stats(window="bogus")
        )                                                                          # 23, 24
        _expect_unknown_window(
            "scene/query", "bogus",
            lambda: tf.request("scene/query", {"path": "/", "window": "bogus"}),
        )                                                                          # 25, 26

        # ── (D) unknown WRITE rejection + primary protection ─────────
        # THE pre-R889 production bug: this exact frame froze the
        # primary's game loop via the silent alias.
        _expect_unknown_window(
            "set_fps fps:0", "bogus", lambda: tf.set_fps(0, window="bogus")
        )                                                                          # 27, 28
        assert_eq(
            tf.pacing_state(window="main")["fps"], None,
            "primary pacing untouched by the rejected bogus-window write",
        )                                                                               # 29
        assert_eq(
            tf.pacing_state(window="inspector")["fps"], None,
            "secondary pacing untouched as well",
        )                                                                               # 30

        # ── (E) wire edges ───────────────────────────────────────────
        _expect_unknown_window(
            "empty-string id", "", lambda: tf.pacing_state(window="")
        )                                                                          # 31, 32
        # The gate leaves no registration residue: after all the bogus
        # traffic both real windows still answer normally.
        assert_eq(
            (tf.pacing_state(window="main")["fps"],
             tf.pacing_state(window="inspector")["fps"]),
            (None, None),
            "known windows unaffected after rejected traffic",
        )                                                                               # 33


if __name__ == "__main__":
    sys.exit(run_demo("r889_window_known_gate", body))
