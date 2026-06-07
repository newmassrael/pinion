#!/usr/bin/env python3
"""R825 §5.12 — `$schema` introspection discovery.

Drives `hello-tree-view` over JSON-RPC and verifies the R825 reserved
`$schema` introspect path: querying `/<tag>/external/$schema` returns an
external's **declared schema** (every queryable path + its type) as JSON,
the discovery primitive under the whole introspection surface. A plain
`scene/query` reads one *known* path; `scene/snapshot` shows the current
value of each *scalar* path — but neither reveals the contract's
**parametric** paths (`id_at` / `label_at` / `level_at` / `expanded_at`),
because `query("id_at")` without a `.<pos>` index resolves to `None` and so
those paths never appear in a snapshot. `$schema` is how an AI discovers
the full surface without hard-coded knowledge.

Verifies:
  (A) `$schema` on the tree-state introspect returns all 7 declared paths
      with their type tags.
  (B) the decisive value-add: the parametric paths are in `$schema` yet are
      absent from the same external's `scene/snapshot` introspect map.
  (C) every scalar path `$schema` declares is actually queryable.
  (D) discovery is uniform — `$schema` works for the button + click-router
      externals too.
  (E) the discovery flow: `scene/snapshot` lists the external tags, then
      `$schema` gives the contract for each.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

STATE_TAG = "file_tree_state"
CLICK_TAG = "file_tree"
ROOT_TAG = "tree_root"


def _walk(node, out):
    if not isinstance(node, dict):
        return
    if node.get("type") == "External":
        out.append(node)
    for child in node.get("children") or []:
        _walk(child, out)
    _walk(node.get("content"), out)  # Scroll wrapper


def externals(snap) -> list[dict]:
    out: list[dict] = []
    _walk(snap, out)
    return out


def introspect_names(snap, tag: str) -> set[str]:
    """The path names present in the snapshot's introspect map for `tag`.

    The snapshot serializes an external's introspect as a `{name: value}`
    object holding only the *scalar* paths whose bare-name `query` resolved
    (parametric paths like `id_at` resolve to `None` and are omitted)."""
    for ext in externals(snap):
        if ext.get("tag") == tag:
            intro = ext.get("introspect") or {}
            if isinstance(intro, dict):
                return set(intro.keys())
            # Fallback for a list-of-pairs encoding.
            return {pair[0] for pair in intro if isinstance(pair, list) and pair}
    return set()


def body() -> None:
    with RpcSubprocess("hello-tree-view", boot_grace=1.5) as tf:

        def schema(tag: str):
            return tf.query(f"/{tag}/external/$schema")

        # ── (A) $schema returns the full declared contract ──────────
        sc = schema(STATE_TAG)
        assert isinstance(sc, list), f"$schema is a JSON array of field descriptors, got {type(sc)}"
        paths = {f["path"]: f["type"] for f in sc}
        assert len(paths) == 7, f"7 declared paths, got {sorted(paths)}"
        for p in ("row_count", "cursor", "cursor_index", "id_at", "label_at", "level_at", "expanded_at"):
            assert p in paths, f"$schema declares {p}; got {sorted(paths)}"
        assert paths["row_count"] == "int", "row_count typed int"
        assert paths["cursor"] == "string", "cursor typed string"
        assert paths["cursor_index"] == "int", "cursor_index typed int"
        assert paths["level_at"] == "int", "level_at typed int"
        assert paths["expanded_at"] == "bool", "expanded_at typed bool"

        # ── (B) parametric paths: in $schema, absent from snapshot ──
        # The RPC-only introspect node paints nothing, so it lives in the
        # state scene (source="state"), not the paint scene.
        snap = tf.snapshot(source="state")
        snap_names = introspect_names(snap, STATE_TAG)
        assert "row_count" in snap_names, "snapshot shows the scalar paths"
        assert "cursor" in snap_names, "snapshot shows cursor"
        for p in ("id_at", "label_at", "level_at", "expanded_at"):
            assert p in paths, f"$schema reveals parametric {p}"
            assert p not in snap_names, f"snapshot omits parametric {p} (query(bare) -> None)"

        # ── (C) every scalar path the schema declares is queryable ──
        assert tf.query(f"/{STATE_TAG}/external/row_count") == 6, "row_count queryable per the contract"
        assert tf.query(f"/{STATE_TAG}/external/cursor") == "src", "cursor queryable per the contract"
        assert isinstance(tf.query(f"/{STATE_TAG}/external/cursor_index"), int), "cursor_index queryable"

        # ── (D) discovery is uniform across externals ───────────────
        click_paths = {f["path"] for f in schema(CLICK_TAG)}
        for p in ("pressed_id", "hovered_id", "send", "click", "hover"):
            assert p in click_paths, f"TreeRowClickExternal $schema declares {p}; got {sorted(click_paths)}"
        btn_paths = {f["path"] for f in schema(ROOT_TAG)}
        assert "state" in btn_paths, f"ButtonExternal $schema declares state; got {sorted(btn_paths)}"

        # ── (E) the discovery flow: snapshot tags → $schema contract ─
        tags = {e.get("tag") for e in externals(snap)}
        assert STATE_TAG in tags, "snapshot lists the tree-state external tag"
        assert CLICK_TAG in tags, "snapshot lists the click-router tag"
        assert ROOT_TAG in tags, "snapshot lists the button root tag"
        # Every discovered external resolves a $schema (uniform contract).
        for tag in (STATE_TAG, CLICK_TAG, ROOT_TAG):
            disc = schema(tag)
            assert isinstance(disc, list) and disc, f"$schema for discovered tag {tag} is a non-empty contract"


if __name__ == "__main__":
    sys.exit(run_demo("R825 §5.12 — $schema introspection discovery", body))
