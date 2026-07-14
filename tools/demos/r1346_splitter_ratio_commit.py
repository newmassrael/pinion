#!/usr/bin/env python3
"""R1346 §5.20 §2 #2 §2 #7 — the splitter's drag-end commit reaches the binding.

SCOPE — read this before the assertions. R1346 gave `SplitterExternal` the
`onChangeEnd` channel its Slider/ScrollBar peers already had: a
`"ratio_committed"` intent carrying the settled ratio, emitted on the
`PointerUp` of a drag that actually moved the split. The consumer need is
sprag's (PINION-PR56): the split ratio is owned by a host process, so the
client must write the FINAL ratio once at the settle — it cannot round-trip at
pointer rate, and it cannot infer the settle from frame cadence because the
shell idles at `ControlFlow::Wait` and no frame need follow a release.

Crate tests prove the External queues the intent and that `walk_scene_and_drain`
harvests it. They do NOT prove the two things a consumer actually depends on,
which is what this demo is for:

  1. §2 #2 — the commit is reachable over the REAL RPC wire. `scene/drag`
     synthesizes press -> interpolated moves -> release into the very same
     `InputRouter` arm a physical mouse takes, so an AI client drives the
     settle exactly as a user does.
  2. The `CoreShell::tail` -> `DispatchTail.intents` -> `V::update` reducer
     link. `hello-dock-panels` now has a real `main_splitter.ratio_committed`
     arm writing a committed-layout mirror (count + last ratio) — the state a
     persisting consumer would ship to its host.

Observed as §2 #7 scene-as-data: the reducer's mirror is painted as the
`split_commit_log` text row in the property pane, so every assertion below
reads pixels-free structure, never a screenshot.

  (A) Boot — 0 commits (nothing has settled).
  (B) A real drag of the main splitter -> EXACTLY ONE commit, and the
      committed ratio is the one the drag settled on (cross-checked against
      the splitter's own live `ratio` slot, which the drag also moved).
  (C) A CLICK on the handle (zero-distance drag) -> still exactly one commit.
      This is the round's load-bearing regression: the press-time
      `pointer_move` the router forwards to every capture-opting widget
      (R51.35 click-to-position) arms the drag calibration, so the obvious
      gate — `DragCalibration::end()`'s bool — is TRUE for a bare click and
      would emit a spurious persist write here. Non-tautological: same widget,
      same wire verb, only the travel differs from (B).
  (D) A second real drag -> commits advance to two and the mirror tracks the
      new ratio (the release left no stale calibration that swallows the next
      settle).
  (E) Determinism — re-reading changes nothing (the commit is an edge, not a
      poll).

The live FEEL is HW-gated; this pins what is observable as scene-as-data.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

# The example's window is `SizeStrategy::Fixed { 880, 600 }`. These MUST match:
# the `InputRouter` hit-tests against the LIVE window's paint scene, so a
# snapshot taken at any other viewport reports rects the pointer path does not
# use. (Asking `scene/snapshot` for 1200x800 happily returns a re-render whose
# handle sits somewhere the real cursor can never land — the trap this demo hit
# on its first run.)
_MAIN_W = 880
_MAIN_H = 600

_SPLITTER = "main_splitter"
_COMMIT_LOG_TAG = "split_commit_log"
# The splitter External is registered as a dynamic extra under its paint tag.
_SPLITTER_RATIO = f"/{_SPLITTER}/external/ratio"

_LOG_RE = re.compile(r"committed split: (\d+) commits, last=([0-9.]+)")


# ─── helpers ─────────────────────────────────────────────────────────


def _find_tagged(node: Any, tag: str) -> Optional[dict]:
    """Depth-first search of a snapshot tree for the node carrying `tag`."""
    if isinstance(node, dict):
        if node.get("tag") == tag:
            return node
        for child in node.get("children") or []:
            hit = _find_tagged(child, tag)
            if hit is not None:
                return hit
    return None


def _commit_log(tf: RpcSubprocess) -> tuple[int, float]:
    """Read the reducer's committed-layout mirror as scene-as-data.

    Returns `(commit_count, last_committed_ratio)` parsed off the
    `split_commit_log` text row. Reading the PAINT scene (not `state`) because
    the mirror is a view projection of the reducer's signal — which is exactly
    the point: the number proves the intent completed the round trip through
    `V::update`, not merely that the External queued something.
    """
    paint = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H))
    node = _find_tagged(paint, _COMMIT_LOG_TAG)
    assert node is not None, f"no {_COMMIT_LOG_TAG!r} node in the paint scene"
    text = node.get("content") or node.get("text") or ""
    m = _LOG_RE.search(text)
    assert m is not None, f"unparsable commit log: {text!r}"
    return int(m.group(1)), float(m.group(2))


def _live_ratio(tf: RpcSubprocess) -> float:
    return float(tf.query(_SPLITTER_RATIO))


def _handle_center_x(tf: RpcSubprocess) -> float:
    """Live x of the splitter's drag handle.

    Read from the scene rather than computed as `ratio * width`, because the
    handle is a 4 px strip and the ratio applies to the *flexible* width (total
    minus the handle extent) — an arithmetic guess lands beside it. It must also
    be re-read after every drag: the handle MOVES, and a stale x would press the
    pane instead.

    Targeting it by coordinate (not `from_path`) is forced: the handle is
    deliberately untagged (R685 — a tag there would change the drag's coordinate
    frame), and the panes on either side ARE tagged precisely so they opt out of
    the splitter's pointer interception. So the handle is the only place a press
    reaches the drag wire, and only a coordinate can name it.
    """
    paint = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H))
    node = _find_tagged(paint, _SPLITTER)
    assert node is not None, "no main_splitter node in the paint scene"
    children = node.get("children") or []
    assert len(children) == 3, f"splitter must paint [left, handle, right]; got {len(children)}"
    rect = children[1]["rect"]
    assert children[1].get("tag") is None, "the handle must stay untagged (R685)"
    return float(rect["x"]) + float(rect["w"]) / 2.0


# ─── demo ────────────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # (A) Boot — nothing has settled yet.
        commits, _ = _commit_log(tf)
        assert commits == 0, f"(A) fresh boot must have 0 commits, got {commits}"
        boot_ratio = _live_ratio(tf)

        # (B) A real drag: press the handle, march right, release.
        handle_x = _handle_center_x(tf)
        target_x = handle_x + 180.0
        tf.drag(from_at=(handle_x, _MAIN_H / 2), to_at=(target_x, _MAIN_H / 2))

        commits, last = _commit_log(tf)
        assert commits == 1, f"(B) one real drag must commit exactly once, got {commits}"
        live = _live_ratio(tf)
        assert abs(live - last) < 1e-3, (
            f"(B) the committed ratio must be the one the drag settled on: "
            f"live={live} committed={last}"
        )
        assert last > boot_ratio, (
            f"(B) dragging right must settle a larger ratio: "
            f"{boot_ratio} -> {last}"
        )
        after_drag = last

        # (C) ★ A CLICK on the handle — press and release with no travel.
        # The press-time pointer_move still arms the calibration, so a gate on
        # "did a calibration exist" would fire a spurious persist write here.
        handle_x = _handle_center_x(tf)
        tf.drag(
            from_at=(handle_x, _MAIN_H / 2),
            to_at=(handle_x, _MAIN_H / 2),
            steps=1,
        )
        commits, last = _commit_log(tf)
        assert commits == 1, (
            f"(C) a click that settled nothing must NOT commit — expected the "
            f"count to stay at 1, got {commits} (spurious persist write: the "
            f"regression R1346's review caught)"
        )
        assert abs(last - after_drag) < 1e-6, (
            f"(C) a click must not rewrite the committed ratio: "
            f"{after_drag} -> {last}"
        )

        # (D) A second real drag — the settle edge still fires after a click.
        handle_x = _handle_center_x(tf)
        target_x = handle_x - 140.0
        tf.drag(from_at=(handle_x, _MAIN_H / 2), to_at=(target_x, _MAIN_H / 2))
        commits, last = _commit_log(tf)
        assert commits == 2, f"(D) the second drag must commit, got {commits}"
        assert last < after_drag, (
            f"(D) dragging left must settle a smaller ratio: "
            f"{after_drag} -> {last}"
        )
        live = _live_ratio(tf)
        assert abs(live - last) < 1e-3, (
            f"(D) mirror must track the live ratio: live={live} committed={last}"
        )

        # (E) Determinism — the commit is an edge; re-reading does not advance it.
        again = _commit_log(tf)
        assert again == (commits, last), f"(E) re-read drifted: {again} != {(commits, last)}"

        print(
            f"[demo] ok — 2 real drags committed twice, 1 click committed zero "
            f"times; final committed ratio={last:.3f}"
        )


if __name__ == "__main__":
    raise SystemExit(run_demo("r1346_splitter_ratio_commit", body))
