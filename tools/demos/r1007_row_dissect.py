#!/usr/bin/env python3
"""R1007 §5.27 §5.40 — master-detail row dissection E2E.

Drives `hello-row-dissect` via JSON-RPC. A master list of structured request
records up top; pick one and its payload explodes into a recursive field tree
below (Wireshark packet-detail / dlt message-detail). The new substrate is
`dissect_row`, which transduces a row's serde_json::Value into a DissectNode
tree; the mature tree-nav + tree-paint machinery windows / collapses it.

  * invisible primary at `dissect_root` (SCXML surface anchor).
  * two `TreeRowClickExternal` routers — `master` (a click selects a row) and
    `detail_tree` (a click toggles a branch).
  * the `RowDissectionExternal` at `dissect` — the scene-as-data witness:
    `selected` / `row_count` / `node_count` and per-visible-node
    `name.<i>` / `value.<i>` / `kind.<i>` / `depth.<i>` / `path.<i>`;
    `select` / `toggle` / `expand` / `collapse` / `clear` drive it.

  Row 0 = GET /api/users 200, dissected (alphabetical field order, the top-level
  headers{} and tags[] branches expanded one level deep on select):

      idx name          path                 value               kind   depth
      0   duration_ms   duration_ms          12.4                float  0
      1   headers       headers              (branch)            object 0
      2   accept        headers.accept       application/json    text   1
      3   host          headers.host         example.com         text   1
      4   method        method               GET                 text   0
      5   secure        secure               true                bool   0
      6   status        status               200                 int    0
      7   tags          tags                 (branch)            array  0
      8   [0]           tags[0]              api                 text   1
      9   [1]           tags[1]              list                text   1
      10  url           url                  /api/users          text   0

  (A) boot — row 0 seeded selected; master + tree paint; node_count = 11.
  (B) dissection witness — every visible node's name/value/kind/depth/path,
      agreeing with the table above; out-of-range -> Null.
  (C) collapse / expand — collapsing a branch drops its descendants; a leaf /
      unknown path is a no-op.
  (D) selection re-dissects — selecting a differently-shaped row rebuilds the
      tree; a deeper branch (error.attempts[]) expands on demand.
  (E) edges — an empty array is a childless branch; clear / out-of-range /
      intervene restore.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the master list + detail tree paint.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-row-dissect"
WIN = (660, 460)
MASTER_TAG = "master"
TREE_TAG = "detail_tree"
DISSECT_TAG = "dissect"


def q(d, path: str):
    return d.query(f"/{DISSECT_TAG}/external/{path}")


def inv(d, verb: str, arg):
    return d.invoke(f"/{DISSECT_TAG}/external/{verb}", arg)


# The expected row-0 dissection (idx -> name, path, value, kind, depth).
ROW0 = [
    ("duration_ms", "duration_ms", "12.4", "float", 0),
    ("headers", "headers", "", "object", 0),
    ("accept", "headers.accept", "application/json", "text", 1),
    ("host", "headers.host", "example.com", "text", 1),
    ("method", "method", "GET", "text", 0),
    ("secure", "secure", "true", "bool", 0),
    ("status", "status", "200", "int", 0),
    ("tags", "tags", "", "array", 0),
    ("[0]", "tags[0]", "api", "text", 1),
    ("[1]", "tags[1]", "list", "text", 1),
    ("url", "url", "/api/users", "text", 0),
]


def assert_node(d, i: int, name, path, value, kind, depth) -> None:
    assert_eq(q(d, f"name.{i}"), name, f"name.{i}")
    assert_eq(q(d, f"path.{i}"), path, f"path.{i}")
    assert_eq(q(d, f"value.{i}"), value, f"value.{i}")
    assert_eq(q(d, f"kind.{i}"), kind, f"kind.{i}")
    assert_eq(q(d, f"depth.{i}"), depth, f"depth.{i}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: row 0 seeded; master + detail paint ───────────
        assert MASTER_TAG in rects, "master list container present"
        assert TREE_TAG in rects, "detail tree container present"
        assert f"{MASTER_TAG}#0" in rects, "master row 0 painted (the seeded selection)"
        assert_eq(q(tf, "row_count"), 5, "five master records")
        assert_eq(q(tf, "selected"), 0, "boot seeds the first record")
        assert_eq(q(tf, "node_count"), 11, "row 0 dissects to 11 visible nodes")

        # ── (B) dissection witness — the AI-first scene-as-data ──────
        for i, (name, path, value, kind, depth) in enumerate(ROW0):
            assert_node(tf, i, name, path, value, kind, depth)
        assert_eq(q(tf, "name.11"), None, "one past the last node -> Null")
        assert_eq(q(tf, "value.99"), None, "far out-of-range node -> Null")
        # The expanded branches paint a composite row tag the click router hits.
        assert f"{TREE_TAG}#headers" in rects, "the headers branch row is painted"
        assert f"{TREE_TAG}#headers.host" in rects, "an expanded child row is painted"

        # ── (C) collapse / expand a branch ──────────────────────────
        assert_eq(inv(tf, "collapse", "headers"), 9, "collapsing headers hides its 2 children")
        # The headers row stays; its children are gone from the flatten.
        assert_eq(q(tf, "name.1"), "headers", "headers still visible after collapse")
        assert_eq(q(tf, "name.2"), "method", "headers.* children dropped (method now at 2)")
        assert_eq(inv(tf, "expand", "headers"), 11, "expand restores the children")
        assert_eq(inv(tf, "toggle", "tags"), 9, "toggle collapses tags")
        assert_eq(inv(tf, "toggle", "tags"), 11, "toggle re-expands tags")
        assert_eq(inv(tf, "collapse", "method"), 11, "collapsing a leaf is a no-op")
        assert_eq(inv(tf, "collapse", "no-such-path"), 11, "an unknown path is a no-op")

        # ── (D) selection re-dissects; deeper branch expands ────────
        # Row 4 = GET /api/metrics 500 with a nested error{ attempts[], code,
        # retryable }. Top-level: duration_ms, error, method, secure, status,
        # url (6); error expands (depth 0) -> attempts, code, retryable (3);
        # attempts is a depth-1 array, collapsed -> 9 visible.
        assert_eq(inv(tf, "select", 4), 9, "row 4 dissects to 9 visible nodes")
        assert_eq(q(tf, "selected"), 4, "selection moved to row 4")
        assert_eq(q(tf, "name.1"), "error", "error branch at index 1")
        assert_eq(q(tf, "kind.1"), "object", "error is an object branch")
        assert_eq(q(tf, "path.2"), "error.attempts", "error.attempts nested array")
        assert_eq(q(tf, "kind.2"), "array", "attempts is an array")
        assert_eq(q(tf, "depth.2"), 1, "attempts sits one level deep")
        # Expand the nested array -> its 3 elements appear.
        assert_eq(inv(tf, "expand", "error.attempts"), 12, "expanding attempts adds its 3 elements")
        assert_eq(q(tf, "path.3"), "error.attempts[0]", "first array element path")
        assert_eq(q(tf, "value.3"), "1", "first attempt value")
        assert_eq(q(tf, "kind.3"), "int", "array element kind")
        assert_eq(q(tf, "depth.3"), 2, "array element two levels deep")

        # ── (E) edges: empty array, clear, intervene restore ────────
        # Row 2 has an empty tags[] — a branch with no children (a childless
        # leaf in the flatten), and an if-none-match header.
        assert_eq(inv(tf, "select", 2), 8, "row 2 dissects to 8 visible nodes")
        # Find the tags node (empty array): it is a kind=array with empty value.
        tags_idx = next(
            i for i in range(8) if q(tf, f"name.{i}") == "tags"
        )
        assert_eq(q(tf, f"kind.{tags_idx}"), "array", "empty tags is still an array")
        assert_eq(q(tf, f"value.{tags_idx}"), "", "an empty array renders no scalar value")
        # clear deselects.
        assert_eq(inv(tf, "clear", None), 0, "clear empties the detail tree")
        assert_eq(q(tf, "selected"), None, "no selection after clear")
        # an out-of-range select deselects (never panics).
        assert_eq(inv(tf, "select", 99), 0, "out-of-range select deselects")
        assert_eq(q(tf, "selected"), None, "still no selection")
        # intervene restores a selection (the admin/restore path).
        tf.intervene(f"/{DISSECT_TAG}/external/selected", 1)
        assert_eq(q(tf, "selected"), 1, "intervene selects row 1")
        # Row 1 = POST /api/login with body{remember,user} + headers{content-type}:
        # 7 top-level + body's 2 + headers' 1 = 10.
        assert_eq(q(tf, "node_count"), 10, "row 1 dissects to 10 visible nodes")
        tf.intervene(f"/{DISSECT_TAG}/external/selected", None)
        assert_eq(q(tf, "selected"), None, "intervene null deselects")

    # ── Phase 2 — live pixels (boot frame: master + tree paint) ────
    snap, rects = _boot_snapshot_and_rects()
    assert f"{MASTER_TAG}#0" in rects, "selected master row painted"
    assert f"{TREE_TAG}#headers" in rects, "a detail tree row painted"
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != window {WIN}"
    mx, my, mw, mh = rects[f"{MASTER_TAG}#0"]
    tx, ty, tw, th = rects[f"{TREE_TAG}#headers"]
    m_col, t_col = sample_png_points(img, [(mx + mw // 2, my + mh // 2), (tx + tw // 2, ty + th // 2)])
    assert m_col is not None and t_col is not None, "master row + detail row sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1007-")) / "dissect.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R1007 §5.27 §5.40 — master-detail row dissection", body))
