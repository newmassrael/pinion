#!/usr/bin/env python3
"""R1449 §5.27 §5.38 §5.40 §2#7 — a completer attached to a plain text input.

Qt reference: `QCompleter`. It is not a widget — it hangs off any input and
answers which candidates match what has been typed (`setFilterMode` x
`setCaseSensitivity`), which one is current (`currentCompletion`), and how to
present the answer (`setCompletionMode`: popup / unfiltered popup / inline).
pinion had none of that configurable: every typeahead in tree hard-codes
`label.to_lowercase().contains(&needle)`, so a prefix-only completion, a
case-sensitive one, or an inline completion was unwritable without editing each
consumer.

And one place Qt is weaker: `currentCompletion()` answers C++ and nobody else.
The popup list lives inside a `QAbstractItemView`, the inline completion lives
in the widget's text selection, and the three knobs are setters with no wire
form. Here the whole model is one External, so the completion a human sees is
the one an agent reads and drives.

What this asserts:

  (A) BOOT — the model reports its candidates and the three knob defaults, and
      an empty prefix accepts every candidate (no special case in the rule).
  (B) TYPING — keystrokes through the character arc (-> apply_key, the path a
      real keyboard takes) filter the popup and land the cursor.
  (C) THE CASE KNOB — driven over `scene/intervene`, the SAME candidate list
      answers 3 or 2.
  (D) THE FILTER KNOB — starts_with / contains / ends_with give three different
      answers over that one list, each cross-checked against the PAINTED rows,
      so the popup and the model cannot drift.
  (E) THE UNFILTERED POPUP — every candidate is listed AND the best match is
      current: one cursor rule covering both popup shapes.
  (F) INLINE COMPLETION — the readout answers the moment the mode changes while
      the field is NOT rewritten (a knob must never type for the user); then a
      keystroke completes the field in place with the appended part SELECTED,
      and the next keystroke types over it (Qt's inline model, on pinion's
      type-to-replace path).
  (G) THE WIRE CURSOR — next / jump over `scene/invoke`, then Enter commits.
  (H) DISCRIMINATORS — an unknown rule token is REFUSED with a typed reason and
      leaves the rule unchanged (a silent fallback would make every knob
      assertion above meaningless); a derived readout is ReadOnly, not
      UnknownPath; an out-of-range completion is present-but-empty.

ZERO-FLAKE: every action->assert edge polls published state (`wait_until` /
`wait_query`); no wall-clock sleeps.

Run from the workspace root:
    python3 tools/demos/r1449_completer.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

VIEWPORT = (640, 520)

INPUT = "comp_input"
MODEL = "comp_model"
OPTIONS = "comp_options"
PANEL = "comp_panel"
STATUS = "comp_status"
CURRENT = "comp_current"

CANDIDATES = [
    "renderScene",
    "renderTarget",
    "RenderPass",
    "sceneGraph",
    "SceneNode",
    "targetBuffer",
    "depthBuffer",
    "presentSurface",
]
N = len(CANDIDATES)


def opt(source: int) -> str:
    """Popup row tag — the SOURCE index, stable across filter changes."""
    return f"{OPTIONS}#{source}"


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _present(snap, tag: str) -> bool:
    return find_by_tag(snap, tag) is not None


def _text_of(tf, tag: str) -> str:
    node = find_by_tag(_paint(tf), tag)
    assert node is not None, f"{tag} node present"
    return node.get("content") or ""


def _m(tf, path: str):
    """Query the completion model (the R666 v1 extra-External path form)."""
    return tf.query(f"/{MODEL}/external/{path}")


def _set(tf, path: str, value) -> None:
    tf.intervene(f"/{MODEL}/external/{path}", value)


def _painted_rows(tf) -> list[str]:
    """The candidate texts the popup is actually painting, in row order."""
    snap = _paint(tf)
    return [CANDIDATES[i] for i in range(N) if _present(snap, opt(i))]


def _wait_count(tf, expected: int, desc: str) -> None:
    wait_query(tf, f"/{MODEL}/external/completion_count", expected, desc=desc)


def _field_text(tf) -> str:
    return tf.query(f"/{INPUT}/external/text")


def body() -> None:
    with RpcSubprocess("hello-completer", boot_grace=1.5) as tf:
        # ── (A) boot: the model and its three knob defaults ──────────
        wait_until(lambda: _present(_paint(tf), INPUT), desc="the input paints")
        assert_eq(_m(tf, "count"), N, "boot: eight candidates")                          # 1
        assert_eq(_m(tf, "filter"), "starts_with", "boot filter mode")                   # 2
        assert_eq(_m(tf, "case"), "insensitive", "boot case sensitivity")                # 3
        assert_eq(_m(tf, "mode"), "popup", "boot completion mode")                       # 4
        assert_eq(_m(tf, "prefix"), "", "boot prefix is empty")                          # 5
        assert_eq(_m(tf, "completion_count"), N,
                  "an empty prefix accepts every candidate")                             # 6
        assert not _present(_paint(tf), PANEL), "no popup before anything is typed"      # 7
        assert "starts_with" in _text_of(tf, STATUS), "the status row paints the rule"   # 8

        # ── (B) typing through the character arc ─────────────────────
        tf.request("focus/set", {"tag": INPUT})
        assert_eq(tf.request("focus/get").result.get("focused"), INPUT, "input focused") # 9
        tf.text("render", path=INPUT)
        _wait_count(tf, 3, "'render' matches three candidates")                          # 10
        assert_eq(_m(tf, "prefix"), "render", "the prefix follows the field")            # 11
        assert_eq(_m(tf, "current_completion"), "renderScene",
                  "the cursor lands on the first completion")                            # 12
        assert_eq(_m(tf, "current"), 0, "cursor position 0")                             # 13
        assert_eq(_painted_rows(tf),
                  ["renderScene", "renderTarget", "RenderPass"],
                  "the popup paints exactly the three completions")                      # 14
        assert_eq(_m(tf, "completion.2"), "RenderPass",
                  "RenderPass is the third completion (matched case-insensitively)")     # 15

        # ── (C) the case knob, over the wire ─────────────────────────
        _set(tf, "case", "sensitive")
        _wait_count(tf, 2, "case-sensitive drops RenderPass")                            # 16
        assert_eq(_painted_rows(tf), ["renderScene", "renderTarget"],
                  "and the popup drops its row too")                                     # 17
        assert_eq(_m(tf, "completion.2"), None,
                  "the third completion is gone: present-but-empty")                     # 18
        _set(tf, "case", "insensitive")
        _wait_count(tf, 3, "insensitive brings it back")                                 # 19

        # ── (D) the filter knob: three answers over ONE list ─────────
        _set(tf, "prefix", "Scene")
        _wait_count(tf, 2, "starts_with 'Scene': sceneGraph + SceneNode")                # 20
        _set(tf, "filter", "contains")
        _wait_count(tf, 3, "contains 'Scene': renderScene joins them")                   # 21
        assert_eq(_painted_rows(tf), ["renderScene", "sceneGraph", "SceneNode"],
                  "the popup follows the rule change")                                   # 22
        _set(tf, "filter", "ends_with")
        _set(tf, "prefix", "Buffer")
        _wait_count(tf, 2, "ends_with 'Buffer': the two buffers")                        # 23
        assert_eq(_m(tf, "completion.0"), "targetBuffer", "first is targetBuffer")       # 24
        assert_eq(_painted_rows(tf), ["targetBuffer", "depthBuffer"],
                  "a suffix search paints its own two rows")                             # 25

        # ── (E) the unfiltered popup: list everything, mark the match ─
        _set(tf, "filter", "starts_with")
        _set(tf, "mode", "unfiltered_popup")
        _set(tf, "prefix", "scene")
        _wait_count(tf, N, "an unfiltered popup lists every candidate")                  # 26
        assert_eq(len(_painted_rows(tf)), N, "and paints every row")                     # 27
        assert_eq(_m(tf, "current_completion"), "sceneGraph",
                  "with the best match current — the same cursor rule")                  # 28
        assert_eq(_m(tf, "current"), 3, "its position in the FULL list, not the 0th")    # 29

        # ── (F) inline completion ────────────────────────────────────
        # Clear the field from the keyboard so the field and the prefix agree
        # again (C-E drove the model directly, which the field never saw).
        for _ in range(len(_field_text(tf))):
            tf.key(path=INPUT, name="Backspace")
        wait_query(tf, f"/{INPUT}/external/text", "", desc="the field is empty")         # 30
        _set(tf, "mode", "inline")
        wait_query(tf, f"/{MODEL}/external/mode", "inline", desc="mode switched")        # 31
        assert not _present(_paint(tf), PANEL), "inline mode has no popup"               # 32
        assert_eq(_m(tf, "inline"), "renderScene",
                  "the readout answers immediately: an empty prefix appends it all")     # 33
        assert_eq(_field_text(tf), "",
                  "but a knob change never types for the user")                          # 34
        tf.text("s", path=INPUT)
        wait_query(tf, f"/{INPUT}/external/text", "sceneGraph",
                   desc="the keystroke completes the field in place")                    # 35
        assert_eq(_m(tf, "prefix"), "s", "the prefix stays what was TYPED")              # 36
        assert_eq(_m(tf, "inline"), "ceneGraph", "the appended part")                    # 37
        assert '"ceneGraph"' in _text_of(tf, CURRENT), "the readout row shows it too"    # 38
        # The next keystroke types over the selected suffix — Qt's inline model.
        tf.text("c", path=INPUT)
        wait_query(tf, f"/{MODEL}/external/prefix", "sc",
                   desc="typing replaced the selection instead of appending")            # 39
        assert_eq(_field_text(tf), "sceneGraph", "and the field re-completed")           # 40

        # ── (G) the wire cursor, then a commit ───────────────────────
        _set(tf, "mode", "popup")
        _set(tf, "prefix", "render")
        _wait_count(tf, 3, "back to a filtered popup")                                   # 41
        # The popup comes back with the popup mode: switching *presentation* was
        # never a dismissal (Escape and the barrier are), and this RPC path
        # reaches exactly the state a toolbar click reaches — the binding derives
        # "is a popup showing" instead of writing it on one of the two paths.
        wait_until(lambda: _present(_paint(tf), PANEL),
                   desc="the popup returns with the popup mode")
        assert_eq(tf.invoke(f"/{MODEL}/external/next", None), "renderTarget",
                  "invoke next walks the list")                                          # 42
        assert_eq(tf.invoke(f"/{MODEL}/external/jump", 2), "RenderPass",
                  "invoke jump lands on a position")                                     # 43
        # An out-of-range jump does not move the cursor, and the invoke returns
        # the cursor READOUT (where it is), not a success flag — the row_search
        # contract this External shares. "Null" would claim there is no current
        # completion, which would be a lie.
        assert_eq(tf.invoke(f"/{MODEL}/external/jump", 99), "RenderPass",
                  "an out-of-range jump reports the unmoved cursor")                     # 44
        assert_eq(_m(tf, "current_completion"), "RenderPass",
                  "and the cursor really did not move")                                  # 45
        tf.key(path=INPUT, name="ArrowDown")  # walks the shown popup: 2 -> 0 (wrap)
        wait_query(tf, f"/{MODEL}/external/current_completion", "renderScene",
                   desc="ArrowDown wraps to the first")                                  # 46
        tf.key(path=INPUT, name="Enter")
        wait_query(tf, f"/{INPUT}/external/text", "renderScene",
                   desc="Enter commits the current completion into the field")           # 47
        assert not _present(_paint(tf), PANEL), "committing closes the popup"            # 48

        # ── (H) discriminators ───────────────────────────────────────
        # If an unknown token silently fell back to some rule, every knob
        # assertion above would be worthless. It must be refused outright.
        assert_rpc_error(lambda: _set(tf, "filter", "nonsense"), data="InterveneTypeMismatch")
        assert_eq(_m(tf, "filter"), "starts_with", "the refused write changed nothing")  # 50
        assert_rpc_error(lambda: _set(tf, "completion_count", 3), data="ReadOnly")
        assert_rpc_error(lambda: _set(tf, "no_such_knob", "x"), data="UnknownIntervenePath")
        assert_eq(_m(tf, "completion.99"), None,
                  "an out-of-range completion is present-but-empty, never absent")       # 53


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1449 §5.38 §2#7 — QCompleter parity: filter x case x completion mode",
        body,
    ))
