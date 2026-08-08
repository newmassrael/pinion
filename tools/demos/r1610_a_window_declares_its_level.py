#!/usr/bin/env python3
"""R1610 §5.16 §5.41 §2 #7 — a window declares its LEVEL, and says what became of it.

A place has a second half a saved layout needs: not only where on the desk, but
how far forward. A monitoring tool's torn-off readout is supposed to stay
visible over the application being watched, and until this round `WindowSpec`
had no way to say so — while the capability was already proven in-tree, since
the shell's own drag-preview window has hard-coded an always-on-top level since
R1147. The capability was there; the vocabulary was not.

What this script checks, and why each check discriminates:

* **A level is DECLARED, changed at runtime, and readable on the wire** — the
  three things the debt this round closes named. `scene/windows` publishes it,
  `invoke("level", …)` and `scene/window_declare` both change it on a window
  that is already open, and the change is visible in the same read.
* **PAST THE USUAL SHAPE (1): the level is one value with three arms.** The
  conventional encoding is two independent bits in a window-flags word, so
  "on top and on bottom" is a value a program can build and each platform
  backend resolves it differently — one sets both stacking hints on a mapped
  window while its show path takes only the first match, one warns and picks
  top, and one ignores the bottom bit entirely. Here the contradiction is not
  representable, which the wire proves by refusing every spelling that is not
  one of the three.
* **PAST THE USUAL SHAPE (2): the levels are ENUMERABLE.** `level_names`
  answers what the levels are, so a level picker is buildable from the wire
  alone. Two bits inside a flags word of twenty unrelated hints carry no
  enumeration of themselves, and the exclusivity rule lives in the platform
  backends rather than in the type.
* **PAST THE USUAL SHAPE (3): a dropped declaration is REPORTED.** A flags
  accessor returns the value the program stored, so a stacking bit a platform
  backend never reads still reads back as set. `level_outcome` here is derived
  against the windowing system actually running, and `applied` / `unsupported`
  / `unknown` are three distinct answers.
* **PAST THE USUAL SHAPE (4): changing it costs one call.** A flags-word
  encoding makes the change reparent and hide the window, and on one platform
  backend destroy and recreate it. This script drives the level back and forth
  and asserts the window is still there, still painting, and still at the same
  place afterwards.
* **PAST THE USUAL SHAPE (5): the whole declaration has ONE write verb.**
  Before this round `scene/windows` published five axes and exactly one of them
  (`position`) could be written back. `scene/window_declare` is a patch over
  every LIVE axis, so an agent moves a panel to another monitor AND pins it in
  one message rather than in two calls with an intermediate state.
* **A patch distinguishes absent, null and a value.** "Leave the position
  alone" and "hand the window back to the window manager" are different
  messages, and this asserts they are.
* **The declaration and the paint cannot drift.** The level the wire reports,
  the level the binding's own `query` reports and the level the panel window
  PAINTS are re-derived from one spec field; the script reads all three.
* **The refusals name what they refused.** An unknown level spelling, an
  unknown window, an empty patch and a write to a derived path each answer
  distinctly.

Everything here holds on any host: no assertion depends on which windowing
system is running. Where the answer legitimately differs — `level_outcome` is
`applied` on X11 and `unsupported` on Wayland — the script asserts the RELATION
between the outcome and the backend it names, which is the fact worth pinning.

Run from the workspace root:
    cargo build -p hello-displays --release
    python3 tools/demos/r1610_a_window_declares_its_level.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    assert_rpc_error,
    call,
    run_demo,
)

#: Mirrored from the framework rather than imported — a demo that read its
#: expected answers out of the code under test could not catch that code
#: changing.
LEVELS = ["always_on_bottom", "normal", "always_on_top"]
PANEL = "panel"
MAIN = "main"

#: Which backends drive a stacking request, and which cannot. The window
#: backend's Wayland implementation is an empty body — the core shell protocol
#: has no stacking request.
DRIVING_BACKENDS = {"x11", "macos", "windows"}


def q(tf: RpcSubprocess, path: str):
    """One `query` against the primary External."""
    return tf.query(f"/external/{path}")


def windows(tf: RpcSubprocess) -> dict[str, dict]:
    return {w["id"]: w for w in call(tf, "scene/windows")["windows"]}


def declare(tf: RpcSubprocess, **patch) -> dict:
    return call(tf, "scene/window_declare", patch)


def settle(tf: RpcSubprocess, frames: int = 2) -> None:
    """Let the reconcile passes the signal write fired actually run."""
    for _ in range(frames):
        tf.tick(0.016)


def run(tf: RpcSubprocess) -> None:
    # ---- 0. the write verb is DISCOVERABLE, not merely callable ----------
    catalogue = {m["name"] for m in call(tf, "rpc/methods")["methods"]}
    assert "scene/window_declare" in catalogue, (
        "an agent finds the write surface through rpc/methods, not by knowing "
        "the name in advance"
    )
    assert "scene/windows" in catalogue, "and the read peer it confirms against"

    # ---- 1. every window declares a level, and it is never null ----------
    specs = windows(tf)
    assert MAIN in specs and PANEL in specs, f"both windows are declared: {list(specs)}"
    for wid, w in specs.items():
        assert "level" in w, f"{wid} must carry a level: {w!r}"
        assert w["level"] in LEVELS, f"{wid} level {w['level']!r} is one of {LEVELS}"
    assert_eq(
        specs[PANEL]["level"],
        "normal",
        "a window boots at the normal level — every pre-R1610 window is "
        "byte-identical",
    )
    assert_eq(specs[MAIN]["level"], "normal", "and so does the main window")

    # ---- 2. the OUTCOME is a second fact, derived against the backend ----
    outcome = specs[PANEL]["level_outcome"]
    assert outcome is not None, (
        "a windowed surface stamped a backend, so the outcome is not null"
    )
    backend = outcome["backend"]
    print(f"[demo] windowing backend: {backend}, level outcome kind {outcome['kind']!r}")
    assert outcome["kind"] in ("applied", "unsupported", "unknown"), (
        f"three distinct answers, got {outcome['kind']!r}"
    )
    assert_eq(
        outcome["kind"],
        "applied",
        "the NORMAL level asks the window manager for nothing, so every "
        "backend honours it — including one that can express nothing",
    )
    assert_eq(
        outcome["declared"],
        "normal",
        "the outcome names the level asked for, on ALL THREE kinds — one "
        "field answers 'what did I request' whatever became of it",
    )
    # Both windows are on the same windowing system.
    assert_eq(
        specs[MAIN]["level_outcome"]["backend"],
        backend,
        "one process, one windowing system",
    )

    # ---- 3. the binding and the wire read ONE declaration ----------------
    assert_eq(
        str(q(tf, "panel_level")),
        specs[PANEL]["level"],
        "the binding's own read and the wire's read are the same spec field — "
        "two copies would be two things to disagree",
    )
    assert_eq(
        [s for s in str(q(tf, "level_names")).split(",") if s],
        LEVELS,
        "the levels are ENUMERABLE over the wire, so a level picker is "
        "buildable from the wire alone, where two bits in a flags word of "
        "twenty unrelated hints carry no enumeration of themselves",
    )

    # ---- 4. the level changes at RUNTIME, on an already-open window ------
    before_pos = specs[PANEL]["position"]
    before_size = specs[PANEL]["declared_size"]
    tf.invoke("/external/level", "always_on_top")
    settle(tf)
    specs = windows(tf)
    assert_eq(
        specs[PANEL]["level"],
        "always_on_top",
        "pinning a live panel is a spec write — no recreate, no unmap, where "
        "a flags-word encoding hides the window and one backend rebuilds it",
    )
    assert_eq(
        specs[MAIN]["level"],
        "normal",
        "and it touched exactly one window",
    )
    assert_eq(specs[PANEL]["position"], before_pos, "the level did not move it")
    assert_eq(specs[PANEL]["declared_size"], before_size, "nor resize it")

    # The window is still there and still painting after the level change —
    # which is the whole difference from a destroy-and-recreate.
    snap = tf.snapshot(source="paint", window=PANEL)
    assert "always_on_top" in str(snap), (
        "the panel PAINTS its level, so the declaration and the pixels come "
        "from one field"
    )

    # ---- 5. the outcome follows the DECLARATION, not the other way round -
    outcome = specs[PANEL]["level_outcome"]
    assert_eq(
        outcome["backend"], backend, "the backend did not change under us"
    )
    if backend in DRIVING_BACKENDS:
        assert_eq(
            outcome["kind"],
            "applied",
            f"{backend} drives a stacking request",
        )
        assert_eq(outcome["declared"], "always_on_top")
    elif backend == "wayland":
        assert_eq(
            outcome["kind"],
            "unsupported",
            "the core shell protocol has no stacking request, and a "
            "declaration the platform drops is reported rather than believed",
        )
        assert_eq(
            outcome["declared"],
            "always_on_top",
            "an unsupported outcome keeps the DECLARATION — a layout preset "
            "that silently rewrote itself could never be corrected",
        )
    else:
        assert_eq(
            outcome["kind"],
            "unknown",
            "an unmeasured backend is neither a known failure nor a known "
            "success, and says so",
        )
    # Whatever the host, the declaration reads back exactly as written.
    assert_eq(
        outcome["declared"],
        "always_on_top",
        "the outcome always names the level that was declared — flattened, so "
        "a client reads one field rather than branching on kind to find it",
    )
    assert set(outcome) == {"kind", "declared", "backend"}, (
        f"the outcome is exactly three fields on every kind: {outcome!r}"
    )

    # ---- 6. the framework's OWN write verb drives the same axis ----------
    out = declare(tf, window_id=PANEL, level="always_on_bottom")
    assert_eq(out["window_id"], PANEL)
    assert_eq(out["applied"], ["level"], "the outcome echoes the axes it understood")
    settle(tf)
    assert_eq(
        windows(tf)[PANEL]["level"],
        "always_on_bottom",
        "scene/window_declare is a framework verb — an agent needs no "
        "application-specific invoke to pin a window",
    )
    assert_eq(
        str(q(tf, "panel_level")),
        "always_on_bottom",
        "and the binding sees the framework's write, because there is one spec",
    )

    # ---- 7. one message patches SEVERAL axes at once ---------------------
    out = declare(
        tf,
        window_id=PANEL,
        level="always_on_top",
        title="KPI (pinned)",
        position=[64, 48],
    )
    assert_eq(
        out["applied"],
        ["title", "position", "level"],
        "a patch names every axis it carries, in declaration order",
    )
    settle(tf)
    panel = windows(tf)[PANEL]
    assert_eq(panel["level"], "always_on_top")
    assert_eq(panel["title"], "KPI (pinned)")
    assert_eq(panel["position"], [64, 48])
    assert_eq(
        panel["anchored"]["kind"],
        "on_declared",
        "an absolute placement resolves onto the desk it was measured in",
    )

    # ---- 8. absent, null and a value are three different messages --------
    declare(tf, window_id=PANEL, title="KPI")
    settle(tf)
    panel = windows(tf)[PANEL]
    assert_eq(panel["position"], [64, 48], "a patch that says nothing about the "
              "position leaves it alone")
    assert_eq(panel["level"], "always_on_top", "and leaves the level alone")

    declare(tf, window_id=PANEL, position=None)
    settle(tf)
    panel = windows(tf)[PANEL]
    assert_eq(
        panel["position"],
        None,
        "an explicit null hands the window back to the window manager — the "
        "state every window boots in, and the one a builder can never reach",
    )
    assert_eq(
        panel["anchored"],
        None,
        "an unplaced window declares nothing, which is distinct from a "
        "placement that resolved to nowhere",
    )
    assert_eq(panel["level"], "always_on_top", "clearing a place is not unpinning")

    # ---- 9. the OTHER live axes are writable too, which is the asymmetry -
    #        this method closed: five readable axes, one writable, before.
    declare(tf, window_id=PANEL, decorations=False)
    settle(tf)
    assert_eq(
        windows(tf)[PANEL]["decorations"],
        False,
        "decorations was readable since R1115 and writable by nobody",
    )
    declare(tf, window_id=PANEL, decorations=True)
    settle(tf)
    assert_eq(windows(tf)[PANEL]["decorations"], True, "and back again")

    # ---- 10. window_move still works, through the SAME write path --------
    call(tf, "scene/window_move", {"window_id": PANEL, "x": 30, "y": 70})
    settle(tf)
    panel = windows(tf)[PANEL]
    assert_eq(panel["position"], [30, 70], "the R1088 verb is unchanged on the wire")
    assert_eq(
        panel["level"],
        "always_on_top",
        "and a move patches POSITION and nothing else — the risk of "
        "expressing it as a patch is that it quietly touches another axis",
    )
    assert_eq(panel["title"], "KPI", "nor the title")

    # ---- 11. going back to normal is a change like any other -------------
    declare(tf, window_id=PANEL, level="normal")
    settle(tf)
    assert_eq(
        windows(tf)[PANEL]["level"],
        "normal",
        "un-pinning must reach the window manager too",
    )

    # ---- 12. the refusals each name what they refused --------------------
    assert_rpc_error(
        lambda: call(tf, "scene/window_declare", {"window_id": "ghost", "level": "normal"}),
        data="UnknownWindow",
    )
    assert_rpc_error(
        # Every axis key misspelled arrives as a patch naming no axis. A
        # cheerful success would teach the client that the typo worked.
        lambda: call(tf, "scene/window_declare", {"window_id": PANEL, "levl": "normal"}),
        data="NoAxisDeclared",
    )
    for bad in ["always_on_topp", "top", "AlwaysOnTop", ""]:
        # A misspelled level is REFUSED BY NAME, never defaulted to `normal`,
        # which would make a typo indistinguishable from asking for ordinary
        # stacking. And the refusal happens before anything is written, which
        # the next section proves.
        assert_rpc_error(
            lambda bad=bad: call(
                tf, "scene/window_declare", {"window_id": PANEL, "level": bad}
            ),
            data="UnknownLevel",
        )
    # The valid spellings stay discoverable — from the binding's own enumerated
    # `level_names` (asserted in section 3) and from the error vocabulary that
    # rpc/errors publishes, rather than from the refusal string itself.
    vocab = {w for e in call(tf, "rpc/errors")["errors"] for w in e["data_vocabulary"]}
    assert "UnknownLevel" in vocab, (
        "a refusal a client may match on must be in the published vocabulary, "
        "or the client can only learn it by reading pinion's source"
    )

    # ---- 12b. a bad level does not half-apply the patch it came with -----
    before = windows(tf)[PANEL]
    assert_rpc_error(
        lambda: call(
            tf,
            "scene/window_declare",
            {"window_id": PANEL, "title": "SHOULD NOT LAND", "level": "sideways"},
        ),
        data="UnknownLevel",
    )
    settle(tf)
    assert_eq(
        windows(tf)[PANEL]["title"],
        before["title"],
        "a refused patch writes NOTHING — the good axes in the same message "
        "must not land, or a client retrying sees a half-applied declaration",
    )
    assert_action_refused(
        lambda: tf.invoke("/external/level", "sideways"),
        saying="is not a window level",
    )
    assert_rpc_error(
        lambda: tf.intervene("/external/panel_level", "always_on_top"),
        data="ReadOnly",
    )

    # ---- 13. the contradiction a two-flag encoding allows is unsayable ---
    #        Two independent bits make "on top AND on bottom" a value a program
    #        can build, and each platform backend resolves it differently. Here
    #        there is no spelling for it at all.
    for impossible in [
        "always_on_top|always_on_bottom",
        "always_on_top,always_on_bottom",
        "both",
    ]:
        assert_rpc_error(
            lambda impossible=impossible: call(
                tf, "scene/window_declare", {"window_id": PANEL, "level": impossible}
            ),
            data="UnknownLevel",
        )

    # ---- 14. the level survives a placement change -----------------------
    #        A monitoring readout that quietly dropped behind the app when the
    #        user moved it to the other monitor is the bug this prevents.
    declare(tf, window_id=PANEL, level="always_on_top")
    settle(tf)
    tf.invoke("/external/apply", "primary")
    settle(tf)
    panel = windows(tf)[PANEL]
    assert_eq(
        panel["level"],
        "always_on_top",
        "a placement preset rewrites the placement and must not touch the level",
    )
    assert panel["anchored"] is not None, "and the preset did place it"

    # ---- 15. and a placement survives a level change ---------------------
    place_before = (panel["position"], panel["display"], panel["anchored"]["kind"])
    declare(tf, window_id=PANEL, level="normal")
    settle(tf)
    panel = windows(tf)[PANEL]
    assert_eq(
        (panel["position"], panel["display"], panel["anchored"]["kind"]),
        place_before,
        "the two declarations are orthogonal in both directions",
    )

    # ---- 16. the whole thing reached assistive technology -----------------
    access = call(tf, "scene/access")
    assert "display(s)" in str(access), "the desk still reaches AT"
    print("[demo] the level is declared, changed at runtime, and read off the wire")


def body() -> None:
    with RpcSubprocess("hello-displays", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("r1610 a window declares its level", body)
