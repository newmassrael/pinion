#!/usr/bin/env python3
"""R675 §5.16 §5.20 §5.45 §5.49 §5.50 — DevTools/Inspector first
interactive dogfood + TreeRowClickExternal substrate maturity proof.

Drives `hello-multi-window` after R675's enhancements:
  1. R674 binding-level `FileTreeRowExternal` lifted to substrate
     `pinion_widget_paint::tree_view::TreeRowClickExternal`. The
     hello-multi-window inspector is the **2nd consumer** (R675),
     satisfying the [[abstraction-needs-second-consumer]]
     Rule-of-Three gate that drove the lift.
  2. Cross-window state sync via shared `Signal<Option<String>>`
     `selected_path` (use_selected_path() in the binding) cached on
     the single ShellCore's Owner. Click on inspector tree row →
     V::update reducer mirrors into the signal → both windows
     repaint with the new selection visible (inspector row gets
     the M3 focus state-layer overlay; main window prepends a
     "Selected: {path}" banner above the button).
  3. The same RPC surface drives both human-side click and AI-side
     `scene/invoke /inspector_tree/external/click` typed shortcut
     — proving the AI-first northern-star axis on a Phase D editor
     dogfood prototype.

R675 atomic 3 verification scope (≥ 30 assertions):

  (A) substrate at the right module path — TreeRowClickExternal
      reachable via /inspector_tree/external/pressed_id query;
      composite row tag format matches the substrate emit pattern.
  (B) baseline: fresh boot has main window without selection
      banner; inspector window shows the read-only state tree
      with no focused row highlight.
  (C) drive cross-window selection via the AI-direct typed
      shortcut: `scene/invoke /inspector_tree/external/click`
      with payload "state" → V::update reducer fires → main
      window next paint shows "Selected: state" banner.
  (D) inspector window next paint shows the M3 focus state-layer
      on the inspector_tree#state row (selected highlight).
  (E) change selection via composite-tag wire (Down + Up cycle on
      different row "main") — main banner updates atomically to
      "Selected: main"; previous "state" row loses focus highlight.
  (F) drag-off cancels click — Down on "state" + Up on "main" does
      NOT mutate the selection (W3C canonical "drag-off cancels").
  (G) PointerCancel mid-press aborts cleanly — pressed_id clears,
      no selection change.
  (H) main window button click still works post-cross-window-wire
      (R670.B baseline preserved — the substrate composition didn't
      break the primary External's input routing).
  (I) main window button state changes propagate to inspector tree
      "State: …" leaf unchanged (R670.B + R671 baseline).
  (J) AI-side selection query: `scene/query` against the inspector
      tree returns the same path the visible banner shows
      (selection mirror is queryable end-to-end).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_until,
)


def _main_snap(tf):
    return tf.request(
        "scene/snapshot",
        {"path": "", "window": "main", "from": "paint",
         "viewport": {"w": 320, "h": 200}},
    ).result


def _wait_main_banner(tf, needle: str, desc: str):
    """Main-window snapshot once `needle` renders (R883 zero-flake)."""
    return wait_until(
        lambda: (lambda s: s if _walk_for_text(s, needle) else None)(_main_snap(tf)),
        desc=desc,
    )


def _walk_for_text(node, needle: str) -> bool:
    if not isinstance(node, dict):
        return False
    if node.get("type") == "Text":
        content = node.get("content")
        if isinstance(content, str) and needle in content:
            return True
    children = node.get("children")
    if isinstance(children, list):
        return any(_walk_for_text(child, needle) for child in children)
    return False


def _focused_inspector_row(snapshot) -> str | None:
    """Return the id of the inspector tree row carrying the M3 focus
    state-layer overlay (SurfaceContainerHighest fill). Mirror of
    `_focused_row_id` from r673/r674 demos for the inspector_tree
    prefix.
    """
    if not isinstance(snapshot, dict):
        return None
    if snapshot.get("type") == "Container":
        style = snapshot.get("style") or {}
        fill = style.get("fill") or {}
        alpha = int(fill.get("a") or 0)
        rgb_sum = (
            int(fill.get("r") or 0)
            + int(fill.get("g") or 0)
            + int(fill.get("b") or 0)
        )
        tag = snapshot.get("tag")
        if (
            alpha != 0
            and 0 < rgb_sum < 255 * 3
            and isinstance(tag, str)
            and tag.startswith("inspector_tree#")
        ):
            return tag.removeprefix("inspector_tree#")
    for child in snapshot.get("children") or []:
        found = _focused_inspector_row(child)
        if found is not None:
            return found
    return None


def _inspector_row_tags(snapshot) -> list[str]:
    tags: list[str] = []
    if isinstance(snapshot, dict):
        if snapshot.get("type") == "Container":
            tag = snapshot.get("tag")
            if isinstance(tag, str) and tag.startswith("inspector_tree#"):
                tags.append(tag)
        for child in snapshot.get("children") or []:
            tags.extend(_inspector_row_tags(child))
    return tags


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) TreeRowClickExternal substrate is reachable ────────
        # The R675 lift means the inspector External is the very
        # same one hello-tree-view uses. Query its pressed_id slot
        # via the substrate's path-form RPC; fresh boot = Null.
        pressed = tf.query("/inspector_tree/external/pressed_id")
        assert pressed is None, (
            f"fresh boot must report null pressed_id; got: {pressed!r}"
        )

        # (A1) inspector tree is paint-side reachable + carries the
        # composite-row tag prefix the click router consumes.
        snap_insp0 = tf.snapshot(
            source="paint",
            viewport=(280, 140),
        )
        # R670.B per-window dispatch — explicitly request inspector
        snap_insp0 = tf.snapshot(
            path="",
            source="paint",
            viewport=(280, 140),
        )
        # Query the inspector-specific window via the {window} param.
        # rpc_verify.snapshot doesn't expose window param directly;
        # build the raw request through .request for this targeted
        # read.
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        assert resp_insp is not None
        snap_insp0 = resp_insp.result
        assert find_by_tag(snap_insp0, "inspector_tree") is not None, (
            "inspector_tree root must paint"
        )
        rows0 = _inspector_row_tags(snap_insp0)
        # Inspector tree emits 'state' leaf + 'main' branch + main scene mirror.
        # At minimum we expect inspector_tree#state and inspector_tree#main.
        assert "inspector_tree#state" in rows0, (
            f"inspector_tree#state row must paint at baseline; rows: {rows0!r}"
        )
        assert "inspector_tree#main" in rows0, (
            f"inspector_tree#main row must paint at baseline; rows: {rows0!r}"
        )
        # No row has the focus highlight at baseline (no selection yet).
        focus0 = _focused_inspector_row(snap_insp0)
        assert focus0 is None, (
            f"fresh boot must show no inspector focus highlight; got: {focus0!r}"
        )

        # ── (B) main window baseline — no selection banner ────────
        resp_main = tf.request(
            "scene/snapshot",
            {"path": "", "window": "main", "from": "paint",
             "viewport": {"w": 320, "h": 200}},
        )
        assert resp_main is not None
        snap_main0 = resp_main.result
        assert not _walk_for_text(snap_main0, "Selected:"), (
            "fresh boot main window must NOT render the selection banner"
        )
        # Main button still present (R670.B baseline).
        assert find_by_tag(snap_main0, "main_btn") is not None, (
            "main_btn must still be present in main view post-R675"
        )

        # ── (C) AI-direct typed shortcut commits a selection ──────
        # /inspector_tree/external/click is the canonical AI-first
        # path — no coordinate, just an id. The reducer mirrors into
        # the shared selected_path Signal; the next main paint shows
        # the banner.
        tf.invoke("/inspector_tree/external/click", "state")
        snap_main1 = _wait_main_banner(
            tf, "Selected: state",
            "main window must render banner 'Selected: state' after invoke click",
        )
        assert find_by_tag(snap_main1, "main_btn") is not None, (
            "main_btn must still paint alongside the banner"
        )

        # ── (D) inspector shows focus highlight on the selected row ─
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        snap_insp1 = resp_insp.result
        focus1 = _focused_inspector_row(snap_insp1)
        assert focus1 == "state", (
            f"inspector must highlight the 'state' row after selection; "
            f"got: {focus1!r}"
        )

        # ── (E) change selection via composite-tag Down/Up wire ────
        # Direct send wire bypasses cursor logic; tests the End-to-end
        # External state machine + reducer + Signal + view-fn chain.
        tf.invoke("/inspector_tree/external/send", "main:PointerDown")
        pressed_armed = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_armed == "main", (
            f"pressed_id must arm mid-press; got: {pressed_armed!r}"
        )
        tf.invoke("/inspector_tree/external/send", "main:PointerUp")
        wait_until(
            lambda: tf.query("/inspector_tree/external/pressed_id") is None,
            desc="pressed_id must clear after matched Down/Up commit",
        )

        snap_main2 = _wait_main_banner(
            tf, "Selected: main",
            "main banner must update to 'Selected: main' after composite-tag click",
        )
        # Previous "state" banner must NOT linger.
        assert not _walk_for_text(snap_main2, "Selected: state"), (
            "old 'Selected: state' banner must be replaced atomically"
        )

        # Inspector focus highlight moves to the new selection.
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        snap_insp2 = resp_insp.result
        focus2 = _focused_inspector_row(snap_insp2)
        assert focus2 == "main", (
            f"inspector focus must move to 'main' row; got: {focus2!r}"
        )

        # ── (F) drag-off cancels click ────────────────────────────
        # Down on 'state' + Up on 'main' — neither commits.
        prev_selection = focus2  # currently "main"
        tf.invoke("/inspector_tree/external/send", "state:PointerDown")
        tf.invoke("/inspector_tree/external/send", "main:PointerUp")
        wait_until(
            lambda: tf.query("/inspector_tree/external/pressed_id") is None,
            desc="pressed_id must clear on mismatched Up (drag-off)",
        )
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        focus_dragoff = _focused_inspector_row(resp_insp.result)
        assert focus_dragoff == prev_selection, (
            f"drag-off must NOT mutate selection; "
            f"focus was {prev_selection!r}, became {focus_dragoff!r}"
        )

        # ── (G) PointerCancel mid-press aborts cleanly ────────────
        tf.invoke("/inspector_tree/external/send", "state:PointerDown")
        pressed_armed2 = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_armed2 == "state"
        tf.invoke("/inspector_tree/external/send", "state:PointerCancel")
        pressed_cancel = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_cancel is None, (
            "PointerCancel on pressed row must clear the slot"
        )
        # Selection still at "main" (no commit).
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        focus_cancel = _focused_inspector_row(resp_insp.result)
        assert focus_cancel == "main", (
            f"PointerCancel must NOT commit; focus stays {focus_cancel!r}"
        )

        # ── (H) main window button still works (R670.B preserved) ─
        # Click on main button — Idle → Hover (or further state
        # depending on InputRouter dispatch). The main view must
        # still react to its primary External.
        tf.click(path="main_btn")
        # Button state should NOT be Idle anymore (Hover or Pressed).
        wait_until(
            lambda: tf.query("/external/state") in ("Hover", "Pressed"),
            desc="main button click must advance state from Idle",
        )
        main_state_after_click = tf.query("/external/state")

        # ── (I) inspector tree reflects updated button state ──────
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        snap_insp3 = resp_insp.result
        assert _walk_for_text(snap_insp3, f"State: {main_state_after_click}"), (
            f"inspector 'State: …' leaf must reflect new button state "
            f"{main_state_after_click!r}"
        )

        # ── (J) AI-side selection observed on main banner ─────────
        # The TreeRowClickExternal's pressed_id slot was just used
        # for input wire; the selection itself lives in the
        # binding's selected_path Signal which view_main reads.
        #
        # R679 §5.16 §5.49 — the bidirectional bridge means a main-
        # button click now ALSO writes the Signal (with the button's
        # static raw path, MAIN_BUTTON_RAW_PATH =
        # "Container/Container[main_btn]"). So after section (H)'s
        # main-button click, the selection is no longer the inspector-
        # written "main" placeholder id but the button's resolved
        # path. Either banner is acceptable here — the test's intent
        # is "the inspector→main bridge keeps working through main-
        # window operations", which is still satisfied; the specific
        # selection value is a R679-driven enrichment.
        resp_main = tf.request(
            "scene/snapshot",
            {"path": "", "window": "main", "from": "paint",
             "viewport": {"w": 320, "h": 200}},
        )
        snap_main_final = resp_main.result
        has_inspector_banner = _walk_for_text(snap_main_final, "Selected: main")
        has_button_banner = _walk_for_text(
            snap_main_final, "Selected: Container/Container[main_btn]"
        )
        assert has_inspector_banner or has_button_banner, (
            "main banner must reflect either the inspector-written "
            "selection ('main') or the R679 button-click-written "
            "selection ('Container/Container[main_btn]') — bridge alive"
        )

        # ── (K) read-only contract on pressed_id slot ─────────────
        try:
            tf.intervene("/inspector_tree/external/pressed_id", "forged")
            raise AssertionError("intervene on pressed_id must fail")
        except AssertionError:
            raise
        except Exception:
            pass
        pressed_final = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_final is None, (
            "rejected intervene must not corrupt pressed_id slot"
        )

        # ── (L) substrate lockstep — invoke 'click' shortcut on
        # the lifted substrate uses the same wire as hello-tree-view's
        # FILE_TREE_CLICK_INTENT_TAG via INSPECTOR_CLICK_INTENT_TAG.
        # Pin the round-trip end-to-end by selecting a 3rd row id
        # and verifying both windows reflect it.
        tf.invoke("/inspector_tree/external/click", "0/0")
        _wait_main_banner(
            tf, "Selected: 0/0",
            "deep-path selection (0/0) must propagate to main banner",
        )
        resp_insp = tf.request(
            "scene/snapshot",
            {"path": "", "window": "inspector", "from": "paint",
             "viewport": {"w": 280, "h": 140}},
        )
        # The deep path "0/0" doesn't correspond to a visible
        # inspector tree row (inspector emits "state" + "main" +
        # nested main-scene rows with deeper paths). The shared
        # signal still updates — only the focus highlight goes
        # silent when no row matches.
        focus_deep = _focused_inspector_row(resp_insp.result)
        assert focus_deep is None or focus_deep == "0/0", (
            f"focus highlight on unknown row gracefully no-ops or "
            f"matches the row when present; got: {focus_deep!r}"
        )

        # ── (M) substrate consumer maturity — pinion-widget-paint
        # tree_view is now home for both view_tree* AND the
        # TreeRowClickExternal. Pin the substrate path remains
        # reachable through path-form RPC after a full demo cycle
        # (no state corruption / no slot drift).
        pressed_postsweep = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_postsweep is None, (
            f"pressed_id slot intact post-demo-cycle; got: {pressed_postsweep!r}"
        )

        # ── (N) idempotent selection — invoking click on the same
        # id twice in a row is a no-op on the visible banner (same
        # Signal value; reactive equality-skip + UI consistency).
        tf.invoke("/inspector_tree/external/click", "state")
        _wait_main_banner(
            tf, "Selected: state",
            "first idempotent click renders the banner",
        )
        tf.invoke("/inspector_tree/external/click", "state")
        assert _walk_for_text(_main_snap(tf), "Selected: state"), (
            "idempotent selection must stabilise on the last invoked id"
        )

        # ── (O) selection persists across paint cycles ──────────
        # Force several paint cycles by hammering snapshot — the
        # banner must keep rendering (Signal storage is the source
        # of truth, not transient).
        for _ in range(3):
            assert _walk_for_text(_main_snap(tf), "Selected: state"), (
                "selection banner must survive repeated paint cycles"
            )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R675 §5.16 §5.20 §5.45 §5.49 §5.50 — DevTools select bridge + substrate lift",
        body,
    ))
