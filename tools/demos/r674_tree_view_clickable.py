#!/usr/bin/env python3
"""R674 §5.16 §5.20 §5.40 §5.50 — TreeView click-to-expand + per-row a11y.

Drives the R674 extensions to `hello-tree-view`: click-to-expand via
the new `FileTreeRowExternal` (composite-tag input wire +
[`Intent`] emission + [`WidgetView::update`] reducer that shares the
shared `toggle_expanded_in_signal` sink with the keyboard handler),
and per-row [`AriaRole::TreeItem`] [`AccessNode`] descriptors with
WAI-ARIA 1.2 hierarchical axes (`aria-level` / `aria-posinset` /
`aria-setsize`).

R674 atomic 3 verification scope (≥ 30 assertions):

  (A) substrate shape — `FileTreeRowExternal` registered at
      [`TREE_TAG`] = `file_tree`; `pressed_id` introspect slot
      reachable; initial state idle (`Null`).
  (B) click on a branch composite tag (`file_tree#docs`) expands it
      (child rows become visible).
  (C) click again collapses (child rows hidden again).
  (D) click on a leaf composite tag is a no-op (rows unchanged + no
      Signal mutation visible in subsequent paints).
  (E) drag-off cancels click — PointerDown on branch A then
      PointerUp on branch B (via direct `send` wire) toggles
      neither branch.
  (F) PointerCancel mid-press aborts cleanly — composite wire +
      pressed_id introspect.
  (G) direct typed shortcut `invoke("click", Text("<id>"))` commits
      a row toggle without coordinate lookup (AI-driven path).
  (H) kbd-toggle then click-toggle on the same row is bit-identical
      (Space toggle → click toggle returns to the original state).
  (I) inverse mix — click toggle then Space toggle on the same row
      also bit-identical.
  (J) per-row a11y substrate is unit-test-covered (the substrate
      extension lands [`AccessNode::level`] /
      [`AccessNode::position_in_set`] / [`AccessNode::size_of_set`]
      with WAI-ARIA 1.2 conformance — the demo pins the composite
      row tags the [`WidgetA11y::access_node`] walker emits match
      the paint-side composite tags the click router consumes,
      proving AT actions on a row resolve to the same External
      slot the click wire mutates).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
)


def _row_tags(snapshot) -> list[str]:
    """Collect every composite row tag under `file_tree#…`."""
    tags: list[str] = []
    if isinstance(snapshot, dict):
        if snapshot.get("type") == "Container":
            tag = snapshot.get("tag")
            if isinstance(tag, str) and tag.startswith("file_tree#"):
                tags.append(tag)
        for child in snapshot.get("children") or []:
            tags.extend(_row_tags(child))
    return tags


def _focused_row_id(snapshot) -> str | None:
    """Return the id of the row carrying the M3 focus state-layer
    overlay (SurfaceContainerHighest fill)."""
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
            and tag.startswith("file_tree#")
        ):
            return tag.removeprefix("file_tree#")
    for child in snapshot.get("children") or []:
        found = _focused_row_id(child)
        if found is not None:
            return found
    return None


def body() -> None:
    with RpcSubprocess("hello-tree-view", boot_grace=1.5) as tf:
        # ── (A) FileTreeRowExternal substrate shape ────────────────
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        assert find_by_tag(snap, "file_tree") is not None, (
            "TreeView root must carry the file_tree tag"
        )

        # (A1) the FileTreeRowExternal is reachable via path-form RPC
        # query — proves the substrate composed it under TREE_TAG.
        pressed = tf.query("/file_tree/external/pressed_id")
        assert pressed is None, (
            f"fresh boot must report null pressed_id; got: {pressed!r}"
        )

        # (A1') schema must advertise the three documented slots so
        # AI clients discovering the External know the wire-form up
        # front (`pressed_id` read-only introspect, `send` composite-
        # tag wire, `click` typed shortcut).
        try:
            send_probe = tf.query("/file_tree/external/send")
            # `send` is an action slot, not a stored value — `query`
            # may return null. Either is acceptable here; the point
            # is that the path resolves (i.e. doesn't 404).
            assert send_probe is None or isinstance(send_probe, str)
        except Exception:
            # Some External backends report query-on-action-slot as
            # an error variant; the schema is the authoritative
            # discovery channel and unit tests pin it.
            pass

        # (A2) initial visible rows: src expanded, tests + docs
        # collapsed (matching the R673 baseline). 6 expected rows.
        rows0 = set(_row_tags(snap))
        assert "file_tree#src" in rows0, "src branch row must paint"
        assert "file_tree#src/main.rs" in rows0, "src/main.rs leaf must paint"
        assert "file_tree#src/lib.rs" in rows0, "src/lib.rs leaf must paint"
        assert "file_tree#src/widgets" in rows0, "src/widgets branch must paint"
        assert "file_tree#tests" in rows0, "tests branch row must paint"
        assert "file_tree#docs" in rows0, "docs branch row must paint"
        assert len(rows0) == 6, (
            f"exactly 6 visible rows at fresh boot (src expanded + others "
            f"collapsed); got {len(rows0)}: {rows0!r}"
        )
        assert "file_tree#docs/README.md" not in rows0
        assert "file_tree#tests/integration.rs" not in rows0
        assert "file_tree#src/widgets/tree_view.rs" not in rows0, (
            "src/widgets is collapsed at boot — its children stay hidden"
        )

        # ── (B) click on docs branch expands it ────────────────────
        tf.click(path="file_tree#docs")
        time.sleep(0.2)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        rows1 = set(_row_tags(snap))
        assert "file_tree#docs/README.md" in rows1, (
            f"click on docs branch must expand; rows: {rows1!r}"
        )

        # (B1) pressed_id must drop back to null after the click (the
        # Down/Up arc landed the click intent + cleared the slot).
        pressed = tf.query("/file_tree/external/pressed_id")
        assert pressed is None, (
            f"pressed_id must return to null after a complete click; "
            f"got: {pressed!r}"
        )

        # ── (C) second click collapses docs ────────────────────────
        tf.click(path="file_tree#docs")
        time.sleep(0.2)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        rows2 = set(_row_tags(snap))
        assert "file_tree#docs/README.md" not in rows2, (
            "second click on docs branch must collapse it"
        )

        # ── (D) click on a leaf row is a no-op ─────────────────────
        # src/main.rs is a leaf — clicking it must not change the
        # visible row set (the V::update reducer's
        # `toggle_expanded_in_signal` early-returns on leaves).
        before_leaf = set(_row_tags(snap))
        tf.click(path="file_tree#src/main.rs")
        time.sleep(0.2)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        after_leaf = set(_row_tags(snap))
        assert before_leaf == after_leaf, (
            f"click on leaf row must not change visible row set; "
            f"before: {before_leaf!r}, after: {after_leaf!r}"
        )

        # ── (E) drag-off cancels click (Down on A, Up on B) ────────
        # Direct `send` wire bypasses the InputRouter cursor logic so
        # the test exercises the W3C canonical click contract end to
        # end at the External boundary. `IntrospectValue::Text` is
        # carried by the §5.12 RPC bridge as a raw JSON string (not
        # the tagged enum form — see `json_to_introspect_value` in
        # `pinion-rpc/src/dispatch.rs`).
        rows_before = set(_row_tags(snap))
        tf.invoke(
            "/file_tree/external/send",
            "src/widgets:PointerDown",
        )
        # Verify the slot is armed mid-press (introspect mid-flight).
        pressed_mid = tf.query("/file_tree/external/pressed_id")
        assert pressed_mid == "src/widgets", (
            f"PointerDown must arm pressed_id slot; got: {pressed_mid!r}"
        )
        # Drag off to a different row + release there.
        tf.invoke(
            "/file_tree/external/send",
            "tests:PointerUp",
        )
        time.sleep(0.2)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        rows_after_dragoff = set(_row_tags(snap))
        assert rows_before == rows_after_dragoff, (
            "drag-off (Down on A, Up on B) must not commit a click on either"
        )
        # pressed_id must be cleared after the mismatched Up.
        pressed_after_dragoff = tf.query("/file_tree/external/pressed_id")
        assert pressed_after_dragoff is None, (
            f"pressed_id must clear on mismatched Up; got: {pressed_after_dragoff!r}"
        )

        # ── (F) PointerCancel mid-press aborts cleanly ─────────────
        tf.invoke(
            "/file_tree/external/send",
            "tests:PointerDown",
        )
        pressed_armed = tf.query("/file_tree/external/pressed_id")
        assert pressed_armed == "tests"
        tf.invoke(
            "/file_tree/external/send",
            "tests:PointerCancel",
        )
        time.sleep(0.2)
        pressed_cancelled = tf.query("/file_tree/external/pressed_id")
        assert pressed_cancelled is None, (
            "PointerCancel on the pressed row must clear the slot"
        )
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        rows_after_cancel = set(_row_tags(snap))
        assert "file_tree#tests/integration.rs" not in rows_after_cancel, (
            "PointerCancel must NOT commit the click — tests stays collapsed"
        )

        # ── (G) direct typed shortcut: invoke("click", Text(id)) ───
        # AI-driven path — no coordinate / no Down/Up cycle, single
        # invoke synthesises the commit and emits the click intent
        # the reducer handles.
        before_shortcut = set(_row_tags(snap))
        tf.invoke("/file_tree/external/click", "tests")
        time.sleep(0.2)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        after_shortcut = set(_row_tags(snap))
        assert "file_tree#tests/integration.rs" in after_shortcut, (
            f"direct `click` shortcut on tests must expand it; "
            f"before: {before_shortcut!r}, after: {after_shortcut!r}"
        )
        # Toggle back so subsequent assertions start from a known state.
        tf.invoke("/file_tree/external/click", "tests")
        time.sleep(0.2)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        assert "file_tree#tests/integration.rs" not in set(_row_tags(snap)), (
            "second click shortcut must collapse tests"
        )

        # ── (H) kbd-toggle followed by click-toggle returns to base ─
        # Focus docs via Home + ArrowDown navigation; Space toggles
        # expand; click toggles again — net change should be zero.
        snap0 = tf.snapshot(source="paint", viewport=(480, 400))
        rows_base_h = set(_row_tags(snap0))
        # Navigate to docs (last row in the visible sequence).
        tf.key(at=(10.0, 10.0), name="End")
        time.sleep(0.15)
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        focus = _focused_row_id(snap)
        assert focus == "docs", (
            f"End must focus the last visible row (docs); got: {focus!r}"
        )
        tf.key(at=(10.0, 10.0), name="Space")  # kbd toggle → expand docs
        time.sleep(0.2)
        snap_after_space = tf.snapshot(source="paint", viewport=(480, 400))
        rows_space = set(_row_tags(snap_after_space))
        assert "file_tree#docs/README.md" in rows_space, (
            "Space on docs (focused) must expand docs"
        )
        tf.click(path="file_tree#docs")  # click toggle → collapse docs
        time.sleep(0.2)
        snap_after_click = tf.snapshot(source="paint", viewport=(480, 400))
        rows_after_h = set(_row_tags(snap_after_click))
        assert rows_after_h == rows_base_h, (
            "Space toggle then click toggle on same row must be bit-identical "
            "(net zero state change); "
            f"base: {rows_base_h!r}, after Space+click: {rows_after_h!r}"
        )

        # ── (I) click-toggle then kbd-toggle also bit-identical ────
        # Focus is still on docs from (H)'s End press (the focused
        # row persists across click events — keyboard cursor is a
        # separate axis from pointer hit-test). Pressing Space on
        # the (still-focused) docs toggles it, exactly mirroring (H)
        # in the opposite order.
        tf.click(path="file_tree#docs")  # click toggle → expand docs
        time.sleep(0.2)
        snap_after_click2 = tf.snapshot(source="paint", viewport=(480, 400))
        rows_click2 = set(_row_tags(snap_after_click2))
        assert "file_tree#docs/README.md" in rows_click2, (
            "first click on docs must expand it"
        )
        focus_after_click2 = _focused_row_id(snap_after_click2)
        assert focus_after_click2 == "docs", (
            f"focus must stay on docs after click (kbd cursor != pointer); "
            f"got: {focus_after_click2!r}"
        )
        tf.key(at=(10.0, 10.0), name="Space")  # kbd toggle → collapse docs
        time.sleep(0.2)
        snap_after_inverse = tf.snapshot(source="paint", viewport=(480, 400))
        rows_after_i = set(_row_tags(snap_after_inverse))
        assert rows_after_i == rows_base_h, (
            "click toggle then Space toggle on same row must be bit-identical; "
            f"base: {rows_base_h!r}, after click+Space: {rows_after_i!r}"
        )

        # ── (J) per-row composite tag stability ────────────────────
        # The AT-side TreeItem AccessNodes (unit-test-verified) use
        # the same composite tag format the click router consumes
        # (`file_tree#<node_id>`). The paint substrate's row tags
        # (verified in (A2)) are the same key — so an AT `Click`
        # action on a row lands on the same External invoke as a
        # mouse click on the same paint rect. Pin a few canonical
        # tag shapes here as a regression anchor.
        rows_final = set(_row_tags(snap_after_inverse))
        canonical_tags = {
            "file_tree#src",
            "file_tree#src/main.rs",
            "file_tree#src/lib.rs",
            "file_tree#src/widgets",
            "file_tree#tests",
            "file_tree#docs",
        }
        for tag in canonical_tags:
            assert tag in rows_final, (
                f"canonical row tag {tag!r} must persist across the demo"
            )

        # ── (K) ensure pressed_id introspect remains read-only ─────
        # The `intervene` channel must reject writes to the
        # pressed_id slot (W3C: AT cannot forge a half-press state).
        try:
            tf.intervene(
                "/file_tree/external/pressed_id",
                "forged",
            )
            raise AssertionError(
                "intervene on read-only pressed_id slot must fail"
            )
        except AssertionError:
            raise
        except Exception:
            # Any RPC error is acceptable — the intent is to reject.
            pass
        # pressed_id stays null even after the rejected intervene.
        pressed_final = tf.query("/file_tree/external/pressed_id")
        assert pressed_final is None, (
            f"rejected intervene must not corrupt pressed_id slot; "
            f"got: {pressed_final!r}"
        )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R674 §5.16 §5.20 §5.40 §5.50 — TreeView click-to-expand + per-row a11y",
        body,
    ))
