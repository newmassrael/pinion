#!/usr/bin/env python3
"""R1327 §5.39 §5.16 §2 #7 PR-53 — the window is named after its FOCUSED pane.

SCOPE — read this before the assertions. Focus reached a binding through exactly
two doors: `WidgetCore::apply_key(_, focused, …)` — only while a key is being
pressed — and `WidgetView::access_node_for_window(_, focused)` — only while an
a11y tree is being built. Neither can carry display state DERIVED from focus,
which is what "the window title names the active pane" (tmux / gnome-terminal /
VS Code, and the sprag terminal multiplexer that forced this PR) needs:

  * caching `apply_key`'s argument goes stale the moment focus moves WITHOUT a
    key — a click, a Tab, a `focus_request`, a modal opening — so the title would
    silently name the wrong pane until the user next typed;
  * writing display state from the a11y hook is a layer violation AND stops
    updating entirely when no assistive client is attached.

So the state was not awkward to derive — it was NOT DERIVABLE. R1327 publishes
the focused tag from the `FocusManager` itself (the state's owner, so no call
site can forget to) into a `pinion_core::focus_state` signal any binding can read
on the paint path, reactively.

The editor stands in for the terminal: each dock pane is a focus stop, and its
title-sync Effect subscribes BOTH the display-title map (R1318) and the focused
tag (R1327). The rule "main window = the active pane's name" is APP policy;
pinion supplies the focused tag and the live-title seam (R1319).

This drives it over the real wire and observes §2 #7 scene-as-data:

  (A) Boot — nothing focused → the window carries the app's own name.
  (B) ★ Focus a pane over RPC — NO key is pressed in this whole demo, which is
      precisely the case an `apply_key` cache cannot see — and the title follows.
  (C) Retitle the FOCUSED pane → the title follows again: focus and display title
      are independent live inputs, and the window is the composition of both.
  (D) Tab (`focus/next`) → the title follows the traversal, against the tab order
      the wire itself reports (no guessed enumeration).
  (E) Focusing a CONTROL inside a pane names the PANE — the active pane, not the
      active widget (what every DCC does).
  (F) A focus stop outside the dock workspace (a toolbar button) names no pane →
      the window falls back to the app name rather than lying.
  (G) Clearing focus falls back too.
  (H) ★ The SHELL drove the real OS window (the R1319 title pass' own trace).
  (I) Tearing off the focused pane hands the naming to its own floating window —
      one pane names exactly one window.

The live FEEL is HW-gated; this pins what is observable as scene-as-data.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_stderr,
    wait_until,
)

EXAMPLE = "hello-dock-panels-editor"
MAIN = "main"

TITLES = "/panel_titles/external/value"

OUTLINER = "outliner"
VIEWPORT = "viewport"
PROPERTIES = "properties"
CONSOLE = "console"

# Each pane's focus stop (the binding paints its content body `.with_focusable`).
BODY = {
    OUTLINER: "outliner_content_body",
    VIEWPORT: "viewport_content_body",
    PROPERTIES: "properties_content_body",
    CONSOLE: "console_content_body",
}
# A control INSIDE the viewport pane, and a control that belongs to NO pane (the
# toolbar is a fixed app frame, not part of the dock workspace).
VIEWPORT_BTN = "viewport_btn"
TOOLBAR_BTN = "float_policy_btn"

IDLE_TITLE = f"{EXAMPLE} — R685 5-pane editor"

VIM = "vim README"


def titled(display: str) -> str:
    """The main window's title while `display` is the active pane's name."""
    return f"{EXAMPLE} — {display}"


# ─── wire helpers ────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result["windows"]


def _declared_title(tf: RpcSubprocess, window_id: str) -> Optional[str]:
    """The window's DECLARED OS title (`scene/windows`) — the binding's spec.

    Sampled upstream of the shell's `set_title` pass, so on its own it proves the
    binding's Effect ran; section (H) asserts the shell's APPLY separately.
    """
    for w in _windows(tf):
        if w["id"] == window_id:
            return w.get("title")
    return None


def _focus(tf: RpcSubprocess) -> dict[str, Any]:
    """`focus/get` — the focused tag AND the live tab order, in one round trip."""
    resp = tf.request("focus/get", {})
    assert resp is not None and resp.result is not None, "focus/get must answer"
    return resp.result


def _set_focus(tf: RpcSubprocess, tag: Optional[str]) -> None:
    """`focus/set` — an RPC error (e.g. a tag that is not a focus stop) raises."""
    tf.request("focus/set", {"tag": tag})


def _set_titles(tf: RpcSubprocess, titles: dict[str, str]) -> None:
    """Retitle panes over the wire — the stand-in for a terminal child's `OSC 0`."""
    tf.intervene(TITLES, titles)


def _pane_of(tag: Optional[str]) -> Optional[str]:
    """The pane a focus stop belongs to — the demo's INDEPENDENT oracle.

    Re-derived here from the wire-visible tag names rather than imported from the
    binding, so an assertion below cannot pass by echoing the binding's own map.
    """
    if tag is None:
        return None
    for panel, body in BODY.items():
        if tag == body:
            return panel
    if tag == VIEWPORT_BTN:
        return VIEWPORT
    return None


def _expected_main_title(tf: RpcSubprocess, titles: dict[str, str]) -> str:
    """What the main window's title MUST read, derived from the wire alone."""
    pane = _pane_of(_focus(tf).get("focused"))
    if pane is None:
        return IDLE_TITLE
    return titled(titles.get(pane, pane))


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    # (R1319) Raise the shell's log level so its `tracing` title-apply line reaches
    # stderr (default filter is `warn`) — section (H)'s observable.
    with RpcSubprocess(
        EXAMPLE, boot_grace=1.5, env={"PINION_LOG": "pinion::shell=debug"}
    ) as tf:
        # ── (A) boot: no pane is active → the app names itself ───────────
        assert_eq(_focus(tf).get("focused"), None, "A.1 nothing is focused at boot")
        assert_eq(
            _declared_title(tf, MAIN),
            IDLE_TITLE,
            "A.2 the window carries the app name while no pane is active",
        )

        # ── (B) ★ focus a pane — with NO key press anywhere ──────────────
        # This is the path the two pre-R1327 doors cannot serve: `apply_key` never
        # runs, and no assistive client is attached, yet the title must follow.
        _set_focus(tf, BODY[OUTLINER])
        wait_until(
            lambda: _declared_title(tf, MAIN) == titled(OUTLINER),
            desc="B.1 ★the window is renamed after the newly-focused pane (no key was pressed)",
        )
        assert_eq(
            _focus(tf)["focused"],
            BODY[OUTLINER],
            "B.2 …and the framework's focused tag is what named it",
        )

        # ── (C) retitle the FOCUSED pane → the title follows again ───────
        # Focus and display title are two independent LIVE inputs; the window is
        # their composition, so a rename of the active pane re-titles the window.
        _set_titles(tf, {OUTLINER: VIM})
        wait_until(
            lambda: _declared_title(tf, MAIN) == titled(VIM),
            desc="C.1 ★renaming the ACTIVE pane renames the window (focus × display title)",
        )
        # …while a rename of an INACTIVE pane does not touch the window.
        _set_titles(tf, {OUTLINER: VIM, CONSOLE: "htop"})
        assert_eq(
            _declared_title(tf, MAIN),
            titled(VIM),
            "C.2 renaming an inactive pane leaves the window title alone",
        )

        # ── (D) Tab traversal drives it too ──────────────────────────────
        # Against the tab order the WIRE reports — not a guessed enumeration.
        state = _focus(tf)
        order = state["tab_order"]
        assert BODY[OUTLINER] in order, f"D.0 the panes are focus stops: {order}"
        expected_next = order[(order.index(state["focused"]) + 1) % len(order)]
        tf.request("focus/next", {})
        assert_eq(_focus(tf)["focused"], expected_next, "D.1 Tab advanced to the next stop")
        titles_now = {OUTLINER: VIM, CONSOLE: "htop"}
        wait_until(
            lambda: _declared_title(tf, MAIN) == _expected_main_title(tf, titles_now),
            desc="D.2 ★the window title follows a Tab traversal",
        )

        # ── (E) a control inside a pane names the PANE ───────────────────
        _set_focus(tf, VIEWPORT_BTN)
        wait_until(
            lambda: _declared_title(tf, MAIN) == titled(VIEWPORT),
            desc="E.1 ★focusing a control INSIDE a pane names the PANE (the active pane, not the widget)",
        )

        # ── (F) a focus stop that belongs to no pane ─────────────────────
        # The toolbar is a fixed app frame, outside the dock workspace: there is no
        # active pane, so the window says so rather than keeping a stale name.
        _set_focus(tf, TOOLBAR_BTN)
        wait_until(
            lambda: _declared_title(tf, MAIN) == IDLE_TITLE,
            desc="F.1 ★a focus stop outside the workspace names no pane (no stale title)",
        )

        # ── (G) clearing focus falls back ────────────────────────────────
        _set_focus(tf, BODY[PROPERTIES])
        wait_until(
            lambda: _declared_title(tf, MAIN) == titled(PROPERTIES),
            desc="G.1 a pane is active again",
        )
        _set_focus(tf, None)
        wait_until(
            lambda: _declared_title(tf, MAIN) == IDLE_TITLE,
            desc="G.2 ★clearing focus falls back to the app name (the cleared focus is published too)",
        )

        # ── (H) ★ the SHELL drove the real OS window ─────────────────────
        # Everything above is the binding's DECLARED spec, sampled upstream of the
        # shell's title pass. This trace fires INSIDE that pass, after
        # `Window::set_title` — without it the declared title would move and the OS
        # window would silently keep its boot title.
        _set_focus(tf, BODY[OUTLINER])
        wait_stderr(
            tf,
            "window title updated",
            desc="H.1 ★the shell APPLIED a focus-derived title to the live OS window",
        )
        applied = [ln for ln in tf.stderr_tail(200) if "window title updated" in ln]
        assert any(MAIN in ln and VIM in ln for ln in applied), (
            f"H.2 ★the apply names the MAIN window and the focused pane's title, got {applied}"
        )

        # ── (I) a torn-off pane names its OWN window ─────────────────────
        # `console` is focused, then torn off: it now titles its own floating window,
        # so the main window must stop naming a pane that no longer sits in it.
        _set_focus(tf, BODY[CONSOLE])
        wait_until(
            lambda: _declared_title(tf, MAIN) == titled("htop"),
            desc="I.1 the console pane is active",
        )
        tf.invoke(f"/{CONSOLE}/external/tear_off", None)
        floating = f"torn-{CONSOLE}"
        wait_until(
            lambda: _declared_title(tf, floating) == f"{EXAMPLE} — htop (floating)",
            desc="I.2 the torn-off pane names its own window",
        )
        assert_eq(
            _focus(tf)["focused"],
            BODY[CONSOLE],
            "I.3 the pane is still the focused one (it just moved windows)",
        )
        assert_eq(
            _declared_title(tf, MAIN),
            IDLE_TITLE,
            "I.4 ★…and the main window stops naming a pane it no longer shows "
            "(one pane names exactly one window)",
        )

        print(
            "[demo] r1327_focused_pane_window_title: all sections PASS "
            "(focus is readable on the paint path)"
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1327_focused_pane_window_title", body))
