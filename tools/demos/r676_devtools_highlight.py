#!/usr/bin/env python3
"""R676 §5.16 §5.49 §5.50 — DevTools highlight overlay + path-stable
indexing proof.

Drives `hello-multi-window` after R676's enhancements on top of R675:

  1. **Path-stable indexing** — `scene_to_tree_item`'s path scheme
     migrated from sibling-index (`"0/2/1"`) to Browser-DevTools
     canonical (CSS-selector mirror): `Type[tag-or-nth-of-type]`.
     Tagged elements (`main_btn`) keep the same path regardless of
     untagged-sibling shape churn (banner appears/disappears, button
     state changes, etc.). This is the **architectural debt R675
     surfaced** — selecting a row before vs. after the banner showed
     drove the same tagged path to point to different elements.
  2. **DevTools highlight overlay** — when the inspector window's
     selected tree row's path resolves against the live main scene
     shape (via the R676 atomic (1) `find_main_node_at_path` inverse
     walker), `view_main` wraps the matching node with a 2 px
     error-coloured stroked Container. Inspector-only ids (`"state"`,
     `"main"`) and stale paths route through the soft-signal early-
     return so the banner continues to paint without a highlight.
  3. **Inspector mirrors the unwrapped main scene** (`view_main_raw`),
     so inspector row paths remain stable across selection toggles.
     Otherwise the wrapper would appear in the inspector tree and
     shift the tagged-path resolution → stale selection oscillation.
     The split is the canonical DevTools-style separation between
     **underlying tree** and **overlay paint** that Chrome / Firefox /
     Safari inspectors also enforce.

R676 atomic (3) verification scope (≥ 30 assertions):

  (A) Path-stable proof — the inspector tree includes the tagged
      composite row `inspector_tree#Container/Container[main_btn]`
      both at baseline and after a selection that introduces the
      banner. The pre-R676 sibling-index scheme would have shifted
      the tagged path; the R676 scheme keeps it constant.
  (B) Inspector click on the tagged button row → main window paints
      a stroked Border (the highlight overlay wrapper).
  (C) Select an inspector-only id (`state`) → banner shows
      "Selected: state" but **no** stroked Border anywhere in main
      (the path doesn't resolve in the main scene shape).
  (D) Select the root (`Container`) → the outermost main container
      gets the stroked Border wrapper (root-level highlight).
  (E) Hover / button-state mutations (d / e keypress → Disabled /
      Enable) preserve the highlight — the path is keyed off the
      tag, not the visual state.
  (F) AI-driven selection via `/inspector_tree/external/click` typed
      shortcut produces the **same** highlight as a composite-tag
      Down/Up cycle through the substrate's send wire — bit-identical
      wire form per the substrate maturity.
  (G) Selection survives ButtonState mutations end-to-end (toggle
      Disable + Enable + verify highlight persists each cycle).
  (H) Border colour matches `ColorRole::Error` — the canonical M3
      token for destructive / attention paint; future palette swap
      flows through cleanly.
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


_VIEW_MAIN_ROOT_PATH = "Container"
_BUTTON_PATH = "Container/Container[main_btn]"
_LABEL_PATH = "Container/Container[main_btn]/Text[0]"


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


def _inspector_row_tags(snapshot) -> list[str]:
    """Enumerate every `inspector_tree#…` composite-tag in the
    inspector window snapshot. Mirrors the R675 demo helper.
    """
    tags: list[str] = []
    if isinstance(snapshot, dict):
        if snapshot.get("type") == "Container":
            tag = snapshot.get("tag")
            if isinstance(tag, str) and tag.startswith("inspector_tree#"):
                tags.append(tag)
        for child in snapshot.get("children") or []:
            tags.extend(_inspector_row_tags(child))
    return tags


def _scene_has_stroked_border(node) -> bool:
    """Depth-first walk for any Container whose style.border is a
    non-zero-width wire-form border. Returns True iff the R676
    highlight overlay wrapper is present anywhere in the painted
    main window scene.
    """
    if not isinstance(node, dict):
        return False
    if node.get("type") == "Container":
        style = node.get("style") or {}
        border = style.get("border")
        if isinstance(border, dict):
            width = border.get("width") or 0
            if width > 0:
                return True
    for child in node.get("children") or []:
        if _scene_has_stroked_border(child):
            return True
    return False


def _find_wrapper_container(node):
    """Return the first Container dict carrying a non-zero stroked
    Border (the R676 highlight overlay wrapper) — or None.
    """
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Container":
        style = node.get("style") or {}
        border = style.get("border")
        if isinstance(border, dict) and (border.get("width") or 0) > 0:
            return node
        for child in node.get("children") or []:
            found = _find_wrapper_container(child)
            if found is not None:
                return found
    return None


def _wrapper_child_tag(wrapper) -> str | None:
    """Tag of the first Container child inside the wrapper — i.e.,
    the underlying highlighted node. None if the wrapper's child
    isn't a tagged Container (e.g., when the root container itself
    is wrapped, the child is the inner untagged content area).
    """
    if not isinstance(wrapper, dict):
        return None
    children = wrapper.get("children") or []
    if not children:
        return None
    first = children[0]
    if isinstance(first, dict) and first.get("type") == "Container":
        tag = first.get("tag")
        return tag if isinstance(tag, str) else None
    return None


def _border_color(node) -> tuple[int, int, int, int] | None:
    """RGBA tuple of the stroked Border colour on `node`'s
    BoxStyle. None when the node has no stroked border.
    """
    if not isinstance(node, dict):
        return None
    style = node.get("style") or {}
    border = style.get("border")
    if not isinstance(border, dict):
        return None
    if (border.get("width") or 0) == 0:
        return None
    color = border.get("color") or {}
    return (
        int(color.get("r") or 0),
        int(color.get("g") or 0),
        int(color.get("b") or 0),
        int(color.get("a") or 0),
    )


def _snap_main(tf) -> dict:
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "window": "main", "from": "paint",
         "viewport": {"w": 320, "h": 200}},
    )
    assert resp is not None
    return resp.result


def _snap_inspector(tf) -> dict:
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "window": "inspector", "from": "paint",
         "viewport": {"w": 280, "h": 140}},
    )
    assert resp is not None
    return resp.result


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) Path-stable indexing baseline ─────────────────────
        # The inspector tree includes the tagged composite row
        # corresponding to the button. Pre-R676 the row would be
        # `inspector_tree#0/0`; post-R676 the row uses the tagged
        # path form so the row id is stable to untagged-sibling
        # churn.
        snap_insp_baseline = _snap_inspector(tf)
        assert find_by_tag(snap_insp_baseline, "inspector_tree") is not None, (
            "inspector_tree root must paint"
        )
        rows_baseline = _inspector_row_tags(snap_insp_baseline)
        # The button's composite row tag is built by view_inspector
        # from the forward walker's path string + the # separator.
        button_row_tag = f"inspector_tree#{_BUTTON_PATH}"
        assert button_row_tag in rows_baseline, (
            f"inspector tree must include the tagged button row "
            f"{button_row_tag!r}; got rows: {rows_baseline!r}"
        )
        # The deeper label row likewise.
        label_row_tag = f"inspector_tree#{_LABEL_PATH}"
        assert label_row_tag in rows_baseline, (
            f"inspector tree must include the deeper label row "
            f"{label_row_tag!r}; got rows: {rows_baseline!r}"
        )
        # And the root row.
        root_row_tag = f"inspector_tree#{_VIEW_MAIN_ROOT_PATH}"
        assert root_row_tag in rows_baseline, (
            f"inspector tree must include the root container row "
            f"{root_row_tag!r}; got rows: {rows_baseline!r}"
        )

        # Main baseline: no selection, no banner, no highlight.
        snap_main_baseline = _snap_main(tf)
        assert not _walk_for_text(snap_main_baseline, "Selected:"), (
            "fresh boot must not render a selection banner"
        )
        assert not _scene_has_stroked_border(snap_main_baseline), (
            "fresh boot must not paint a highlight overlay"
        )
        assert find_by_tag(snap_main_baseline, "main_btn") is not None, (
            "main_btn must be present at baseline"
        )

        # ── (B) Inspector click on the tagged button row ──────────
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        time.sleep(0.25)

        snap_main_b = _snap_main(tf)
        assert _walk_for_text(snap_main_b, f"Selected: {_BUTTON_PATH}"), (
            f"banner must reflect the selected button path {_BUTTON_PATH!r}"
        )
        assert _scene_has_stroked_border(snap_main_b), (
            "tagged-path selection must paint a stroked Border overlay"
        )
        wrapper_b = _find_wrapper_container(snap_main_b)
        assert wrapper_b is not None, "wrapper Container must be locatable"
        # The wrapper's single child must be the actual tagged
        # button — the wrap is non-destructive composition.
        assert _wrapper_child_tag(wrapper_b) == "main_btn", (
            f"wrapper must enclose the tagged button; child tag: "
            f"{_wrapper_child_tag(wrapper_b)!r}"
        )
        # The original button tag stays reachable underneath.
        assert find_by_tag(snap_main_b, "main_btn") is not None, (
            "main_btn must persist underneath the wrapper"
        )

        # ── (A2) Path-stable proof — after the banner appeared,
        # the inspector tree's row paths are unchanged ────────────
        snap_insp_after_select = _snap_inspector(tf)
        rows_after_select = _inspector_row_tags(snap_insp_after_select)
        assert button_row_tag in rows_after_select, (
            f"tagged button row must persist post-selection; rows: "
            f"{rows_after_select!r}"
        )
        # Pre-R676 the banner appearance would have shifted the
        # sibling index; verify the tagged path is unchanged.
        assert label_row_tag in rows_after_select, (
            "deeper label row must remain accessible by tagged path"
        )

        # ── (C) Inspector-only id `state` → no overlay, banner OK ──
        tf.invoke("/inspector_tree/external/click", "state")
        time.sleep(0.25)
        snap_main_c = _snap_main(tf)
        assert _walk_for_text(snap_main_c, "Selected: state"), (
            "banner must reflect 'state' selection"
        )
        assert not _scene_has_stroked_border(snap_main_c), (
            "inspector-only 'state' must not paint a highlight overlay"
        )
        assert find_by_tag(snap_main_c, "main_btn") is not None, (
            "main_btn unchanged when path doesn't resolve"
        )

        # ── (D) Root selection wraps the outer container ──────────
        tf.invoke("/inspector_tree/external/click", _VIEW_MAIN_ROOT_PATH)
        time.sleep(0.25)
        snap_main_d = _snap_main(tf)
        assert _walk_for_text(snap_main_d, f"Selected: {_VIEW_MAIN_ROOT_PATH}"), (
            f"banner must reflect root selection {_VIEW_MAIN_ROOT_PATH!r}"
        )
        assert _scene_has_stroked_border(snap_main_d), (
            "root selection must paint the highlight overlay"
        )
        # The outermost node must be the wrapper — its first child
        # is the raw root Container.
        assert isinstance(snap_main_d, dict)
        outer_border = (snap_main_d.get("style") or {}).get("border")
        assert isinstance(outer_border, dict) and (outer_border.get("width") or 0) > 0, (
            "root selection places the stroked Border on the outermost Container"
        )

        # ── (E) State mutation preserves highlight (rest goes to G) ──
        # Re-select the button for the subsequent state-mutation
        # tests so we can observe the wrap persisting under state
        # changes.
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        time.sleep(0.25)

        snap_main_e0 = _snap_main(tf)
        assert _scene_has_stroked_border(snap_main_e0), (
            "re-selected button must light up the highlight overlay"
        )

        # Disable via keybinding d.
        tf.request(
            "scene/key",
            {"window": "main", "key": "d", "path": "main_btn"},
        )
        time.sleep(0.20)
        snap_main_e1 = _snap_main(tf)
        # Button state change → button repaint with Disabled fill,
        # but the highlight wrapper stays.
        assert _scene_has_stroked_border(snap_main_e1), (
            "Disabled state change must NOT clear the highlight overlay"
        )
        wrapper_e1 = _find_wrapper_container(snap_main_e1)
        assert _wrapper_child_tag(wrapper_e1) == "main_btn", (
            "wrapper continues to enclose the tagged button across state mutation"
        )

        # Re-enable.
        tf.request(
            "scene/key",
            {"window": "main", "key": "e", "path": "main_btn"},
        )
        time.sleep(0.20)
        snap_main_e2 = _snap_main(tf)
        assert _scene_has_stroked_border(snap_main_e2), (
            "Enable state change must NOT clear the highlight overlay"
        )

        # ── (F) AI-driven send-wire produces the same highlight ───
        # Use the composite-tag substrate path (Down + Up cycle).
        # This is the underlying wire form the inspector-row click
        # routes through — proving the substrate's pointer state
        # machine reaches the same outcome as the typed `click`
        # shortcut.
        tf.invoke(
            "/inspector_tree/external/send",
            f"{_LABEL_PATH}:PointerDown",
        )
        pressed_armed = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_armed == _LABEL_PATH, (
            f"send-wire PointerDown must arm pressed_id; got: {pressed_armed!r}"
        )
        tf.invoke(
            "/inspector_tree/external/send",
            f"{_LABEL_PATH}:PointerUp",
        )
        time.sleep(0.25)
        pressed_after = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_after is None, (
            "pressed_id clears after matched Down/Up commit"
        )

        snap_main_f = _snap_main(tf)
        assert _walk_for_text(snap_main_f, f"Selected: {_LABEL_PATH}"), (
            f"banner must reflect the label-path selection {_LABEL_PATH!r}"
        )
        # The label text is a Scene::Text — it gets wrapped inside
        # the button container. The wrapper has a stroked Border;
        # the underlying scene tree still contains the button tag
        # (the wrap is internal to the button's children list).
        assert _scene_has_stroked_border(snap_main_f), (
            "label-path selection must paint a stroked Border overlay"
        )
        assert find_by_tag(snap_main_f, "main_btn") is not None, (
            "main_btn still reachable when the wrap is inside the button"
        )

        # ── (G) End-to-end selection survival cycle ───────────────
        # Re-select the button and toggle d/e/d/e — verify the
        # highlight overlay is present at every step.
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        time.sleep(0.20)
        for cycle in range(2):
            tf.request("scene/key", {"window": "main", "key": "d", "path": "main_btn"})
            time.sleep(0.10)
            snap_d = _snap_main(tf)
            assert _scene_has_stroked_border(snap_d), (
                f"highlight must survive Disable in cycle {cycle}"
            )
            tf.request("scene/key", {"window": "main", "key": "e", "path": "main_btn"})
            time.sleep(0.10)
            snap_e = _snap_main(tf)
            assert _scene_has_stroked_border(snap_e), (
                f"highlight must survive Enable in cycle {cycle}"
            )

        # ── (H) Border colour is non-default (Error-role) ─────────
        # The Material 3 Error role resolves to a chromatic red in
        # the light palette + a desaturated red in dark — both have
        # a non-trivial chromatic signature. Verify the border colour
        # is non-grey + opaque (the substrate canon for Error).
        wrapper_h = _find_wrapper_container(_snap_main(tf))
        assert wrapper_h is not None, "wrapper must be present at H"
        color_h = _border_color(wrapper_h)
        assert color_h is not None
        r_h, g_h, b_h, a_h = color_h
        assert a_h == 255, (
            f"highlight Border must be fully opaque (alpha=255); got: {a_h}"
        )
        # Error role distinguishes itself by red dominance; a
        # neutral grey would have r == g == b. Pin the chromatic
        # signature without binding to an exact RGB tuple (so a
        # palette refresh doesn't break the demo).
        assert r_h != g_h or r_h != b_h, (
            f"Error-role highlight must be chromatic; got: ({r_h},{g_h},{b_h})"
        )
        assert r_h > g_h and r_h > b_h, (
            f"Error-role highlight must be red-dominant (M3 canonical); "
            f"got: ({r_h},{g_h},{b_h})"
        )

        # ── (I) Soft-fail path — pre-R676 sibling-index form ──────
        # Selecting via a stale legacy path string ("0/0") doesn't
        # resolve in the new scheme; the banner shows the raw payload
        # but the overlay stays absent. This pins the soft-signal
        # contract — clients carrying stale state degrade gracefully.
        tf.invoke("/inspector_tree/external/click", "0/0")
        time.sleep(0.20)
        snap_main_i = _snap_main(tf)
        assert _walk_for_text(snap_main_i, "Selected: 0/0"), (
            "banner mirrors the raw payload regardless of resolution"
        )
        assert not _scene_has_stroked_border(snap_main_i), (
            "stale pre-R676 sibling-index path must not paint a highlight"
        )

        # ── (J) Selection clears via empty inspector-only id ──────
        # Re-establish baseline by selecting an inspector-only id
        # (highlight goes silent, banner stays). Demonstrates the
        # selection is volatile mutable state, not a one-way commit.
        tf.invoke("/inspector_tree/external/click", "main")
        time.sleep(0.20)
        snap_main_j = _snap_main(tf)
        assert _walk_for_text(snap_main_j, "Selected: main"), (
            "banner shows the new inspector-only selection"
        )
        assert not _scene_has_stroked_border(snap_main_j), (
            "switching to inspector-only id must clear the highlight overlay"
        )

        # ── (K) Substrate maturity — pressed_id slot intact ───────
        pressed_postsweep = tf.query("/inspector_tree/external/pressed_id")
        assert pressed_postsweep is None, (
            f"pressed_id slot intact post-demo-cycle; got: {pressed_postsweep!r}"
        )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R676 §5.16 §5.49 §5.50 — DevTools highlight overlay + path-stable indexing",
        body,
    ))
