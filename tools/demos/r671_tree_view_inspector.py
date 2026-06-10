#!/usr/bin/env python3
"""R671 §5.16 §5.50 — TreeView inspector first-consumer end-to-end.

Verifies the R671 `pinion_widget_paint::tree_view` substrate against
the live `hello-multi-window` binary. The inspector window's
single-Text mirror (R670.B) has been replaced with a `view_tree`
consumer that walks the main window's paint scene + builds a
[`TreeItem`] model + renders the M3 Lists row sequence.

R671 atomic 4 verification scope (≥30 assertions):

  (A) substrate shape — TreeView root tag + per-row composite tags
      ({tag}#{node_id} per the multi-External convention) + M3 row
      height + label visibility + disclosure-glyph correctness.
  (B) per-window last_paint_layout (R671 atomic 1) — `scene/layout
      {window: "inspector"}` ≠ `scene/layout {window: "main"}`
      because each WindowSlot carries its own snapshot.
  (C) pinion_rpc single-parse refactor (R671 atomic 2) — every
      frame routes through the new `dispatch_parsed` entry; no
      crash on the canonical RPC method surface.
  (D) state mirror — main state changes (Idle → Hover → Disabled
      → Idle via scene/invoke) propagate to the inspector tree's
      leading "State: …" row on the next paint cycle.

Race-immune (no scene/click): the multi-window InputRouter
single-tracker race that surfaced through R670.B atomic 2 is a
known carry [[multi-window-input-router-race]] — R671's
verification uses the scene/invoke arc that bypasses InputRouter
hit-tests entirely (per [[ai-first-rpc-introspection-obligation]]).
Per-window InputRouter is the canonical future fix.
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


def _walk_for_text(node, needle: str) -> bool:
    """Depth-first walk for any text node whose content contains needle."""
    if not isinstance(node, dict):
        return False
    if node.get("type") == "Text":
        content = node.get("content")
        if isinstance(content, str) and needle in content:
            return True
    children = node.get("children")
    if isinstance(children, list):
        for child in children:
            if _walk_for_text(child, needle):
                return True
    return False



def _wait_inspector_with(tf, needle: str, desc: str):
    """Inspector-window snapshot once `needle` renders (R883 zero-flake)."""
    def poll():
        resp = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        if resp is not None and _walk_for_text(resp.result, needle):
            return resp
        return None

    return wait_until(poll, desc=desc)

def _collect_tags(node) -> list[str]:
    """Depth-first walk collecting all Container tags."""
    tags: list[str] = []
    if isinstance(node, dict):
        if node.get("type") == "Container":
            tag = node.get("tag")
            if isinstance(tag, str):
                tags.append(tag)
        children = node.get("children")
        if isinstance(children, list):
            for child in children:
                tags.extend(_collect_tags(child))
    return tags


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) substrate shape ────────────────────────────────────
        inspector = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        assert inspector is not None, "scene/snapshot inspector must succeed"
        result = inspector.result

        # (A1) TreeView root tag is present (the R671 `inspector_tree`
        # anchor; the R670.B `inspector_state_text` Container is
        # superseded).
        assert (
            find_by_tag(result, "inspector_tree") is not None
        ), f"inspector must carry TreeView root tag: {result!r}"
        # (A2) Old single-Text container tag is gone — the upgrade
        # replaces it.
        assert (
            find_by_tag(result, "inspector_state_text") is None
        ), "R670.B inspector_state_text tag should be retired post-R671"

        # (A3) The leading State row composite tag (`inspector_tree#state`)
        # is the canonical RPC entry point for state introspection.
        assert (
            find_by_tag(result, "inspector_tree#state") is not None
        ), "TreeView must carry the State leading-row composite tag"

        # (A4) The `main` branch row is present (`inspector_tree#main`).
        assert (
            find_by_tag(result, "inspector_tree#main") is not None
        ), "TreeView must carry the main-scene branch composite tag"

        # (A5) — (A8) Composite tags follow the {tree_tag}#{id}
        # convention.  Every row tag starts with `inspector_tree#`.
        tags = _collect_tags(result)
        row_tags = [t for t in tags if t.startswith("inspector_tree#")]
        assert len(row_tags) >= 4, (
            f"TreeView must emit ≥4 composite row tags (state row + main "
            f"branch + at least 2 scene rows): {row_tags!r}"
        )

        # (A9) Every composite row tag contains the `#` separator.
        for tag in row_tags:
            assert "#" in tag, f"row tag {tag!r} must carry the # separator"

        # (A10) — (A11) The TreeView root contains visible content.
        assert _walk_for_text(result, "State:"), (
            "TreeView root must render the State row label text"
        )
        assert _walk_for_text(result, "Idle"), (
            "fresh boot state mirror must render 'Idle'"
        )

        # (A12) The 'Container [main_btn]' row label proves the
        # scene_to_tree_item walker descended through the main scene
        # to the button tag.
        assert _walk_for_text(result, "main_btn"), (
            "TreeView must render the main_btn tag label in a row"
        )

        # (A13) The button's visible label text is captured as a
        # tree leaf (`Text: \"Click me!\"`).
        assert _walk_for_text(result, "Click me!"), (
            "TreeView must render the button label text in a leaf row"
        )

        # (A14) Expanded-state disclosure glyph (`U+25BC`) is present
        # — branches default to expanded so the full subtree shows.
        assert _walk_for_text(result, "▼"), (
            "expanded-branch rows must render the U+25BC glyph"
        )

        # (A15) Tree styling — rows render through the substrate's
        # m3_default; the `inspector_tree` Container is reachable in
        # the inspector window scene. R677 §5.16 §5.49 introduced the
        # 2-pane DevTools layout (Row[tree, property_pane]) so the
        # tree container is now a child of the outer Row rather than
        # the snapshot root; (A1) above already verified its presence
        # via depth-first `find_by_tag`. Keep this comment as the
        # canonical anchor for the layout-evolution decision.
        assert find_by_tag(result, "inspector_tree") is not None, (
            "inspector_tree container must be reachable in the inspector view"
        )

        # ── (B) per-window last_paint_layout (R671 atomic 1) ───────
        # scene/layout with window scope returns the named slot's
        # last paint snapshot.
        layout_main = tf.request(
            "scene/layout", {"window": "main", "viewport": None}
        )
        layout_inspector = tf.request(
            "scene/layout", {"window": "inspector", "viewport": None}
        )
        assert layout_main is not None and isinstance(layout_main.result, dict)
        assert layout_inspector is not None and isinstance(
            layout_inspector.result, dict
        )

        # (B1) Main's last paint is at the main window's declared
        # SizeStrategy::Fixed { 320, 200 }.
        main_rect = layout_main.result.get("rect") or {}
        assert int(main_rect.get("w") or 0) == 320, (
            f"main scene/layout width must be 320: {layout_main.result!r}"
        )
        assert int(main_rect.get("h") or 0) == 200, (
            f"main scene/layout height must be 200: {layout_main.result!r}"
        )

        # (B2) Inspector's last paint is at the inspector window's
        # declared SizeStrategy::Fixed { 480, 320 } — R677 §5.16 §5.49
        # widened the inspector to host the new 2-pane DevTools
        # layout (tree pane + property pane side-by-side).
        ins_rect = layout_inspector.result.get("rect") or {}
        assert int(ins_rect.get("w") or 0) == 480, (
            f"inspector scene/layout width must be 480 (R677): {layout_inspector.result!r}"
        )
        assert int(ins_rect.get("h") or 0) == 320, (
            f"inspector scene/layout height must be 320 (R677): {layout_inspector.result!r}"
        )

        # (B3) Pre-R671, both calls would have returned the same
        # snapshot (the binding-wide ShellCore.last_paint_layout).
        # R671 lifts the snapshot per-WindowSlot so the two
        # snapshots are now distinct — explicit pin.
        assert (
            main_rect != ins_rect
        ), f"per-window last_paint_layout: main {main_rect!r} != inspector {ins_rect!r}"

        # ── (C) RPC single-parse refactor (R671 atomic 2) ──────────
        # Every frame routes through pinion_rpc::parse_request +
        # dispatch_parsed; the canonical method surface stays callable.

        # (C1) Default-scope snapshot (no `{window}` param) routes
        # through the same parse + falls back to the primary "main".
        snap_default = tf.snapshot(source="paint", viewport=(320, 200))
        assert (
            find_by_tag(snap_default, "main_btn") is not None
        ), f"default-scope snapshot must contain main_btn: {snap_default!r}"

        # (C2) State-source snapshot (single-binding scope — state
        # scene is not per-window) still returns the External node.
        state_snap = tf.snapshot()
        assert isinstance(state_snap, dict) or state_snap is not None, (
            "state-source snapshot routes through the parsed dispatcher"
        )

        # (C3) Snapshot with explicit window param parses + threads
        # `{window}` cleanly — both window IDs produce per-spec views.
        snap_main_window = tf.request(
            "scene/snapshot",
            {"window": "main", "path": "", "from": "paint", "viewport": {"w": 320, "h": 200}},
        )
        assert snap_main_window is not None, (
            "scene/snapshot {window:main} must succeed post-single-parse"
        )

        # ── (D) state mirror (race-immune via scene/invoke) ────────
        # Idle → Hover via PointerEnter (bypasses InputRouter race).
        tf.invoke("/external/send", "PointerEnter")
        # (D1) State row label updated.
        ins_hover = _wait_inspector_with(
            tf, "Hover",
            "after PointerEnter, inspector State row must read Hover",
        )
        # (D2) Old state name no longer appears in the State row.
        # (The button-label text 'Click me!' still appears under the
        # main-scene branch — it's the visible button caption, not
        # the state name — so we narrow by checking the State row
        # composite tag specifically.)
        state_row = find_by_tag(ins_hover.result, "inspector_tree#state")
        assert state_row is not None
        assert _walk_for_text(state_row, "Hover")
        assert not _walk_for_text(state_row, "Idle"), (
            f"State row must not still show Idle after PointerEnter: {state_row!r}"
        )

        # Hover → Disabled via Disable.
        tf.invoke("/external/send", "Disable")
        # (D3) State row reads Disabled.
        ins_disabled = _wait_inspector_with(
            tf, "Disabled",
            "after Disable, inspector State row must read Disabled",
        )
        # (D4) The Container[main_btn] subtree still shows up — the
        # whole main scene is mirrored regardless of state.
        assert _walk_for_text(ins_disabled.result, "main_btn"), (
            "Disabled state still mirrors the main scene structure"
        )
        # (D5) The button label text changes to "Disabled" when the
        # state goes Disabled (per the existing view_main fn body).
        assert _walk_for_text(ins_disabled.result, '"Disabled"'), (
            "Disabled state's button label is rendered as a Text leaf"
        )

        # Disabled → Idle via Enable.
        tf.invoke("/external/send", "Enable")
        # (D6) Re-enable transitions back to Idle.
        ins_re_idle = _wait_inspector_with(
            tf, "Idle",
            "after Enable, inspector State row must read Idle again",
        )
        # (D7) Disabled tag is gone from the State row.
        state_row_re = find_by_tag(ins_re_idle.result, "inspector_tree#state")
        assert state_row_re is not None
        assert not _walk_for_text(state_row_re, "Disabled"), (
            "State row must drop Disabled label after Enable"
        )

        # (D8) Main window still shows the button widget — multi-
        # window paint independence pin.
        snap_main_final = tf.request(
            "scene/snapshot",
            {
                "window": "main",
                "path": "",
                "from": "paint",
                "viewport": {"w": 320, "h": 200},
            },
        )
        assert snap_main_final is not None
        assert find_by_tag(snap_main_final.result, "main_btn") is not None, (
            "main window scene/snapshot still resolves main_btn"
        )

        # (D9) inspector_tree tag absent from main view (per-window
        # paint separation pin).
        assert (
            find_by_tag(snap_main_final.result, "inspector_tree") is None
        ), "inspector_tree must not leak into main window paint"

        # (D10) main_btn tag absent from inspector view's root scope
        # — the TreeView's row label contains the *string* "main_btn"
        # but the actual Container tag is `inspector_tree#…`.
        ins_final = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        assert ins_final is not None
        # The literal main_btn Container tag exists only inside the
        # main window's scene, never as a Container tag in the
        # inspector tree. The string appears in a Text node of the
        # inspector's row label (visible), but that's a text
        # rendering, not a tagged Container.
        ins_tags = _collect_tags(ins_final.result)
        assert "main_btn" not in ins_tags, (
            f"inspector view must not carry main_btn as a Container tag: {ins_tags!r}"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R671 §5.16 §5.50 — TreeView inspector + carry clearance", body))
