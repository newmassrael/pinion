#!/usr/bin/env python3
"""hello-listbox snapshot dogfood (§5.49 R59, R51.194).

First dogfood that exercises the R51.194 `scene/snapshot` Container /
Scroll traversal. The R51.193 harness only saw the scene root; this
demo descends into a real `Scene::Container` wrapping a
`Scene::Scroll` wrapping another `Scene::Container` of 12 listbox
rows, and asserts the substrate fields that prove the visible window
is what the §5.45 R55 axis claims.

Walkthrough (matches `examples/hello-listbox/src/main.rs`):

  Container {                       # outer (BG_FILL + center flex)
    children: [
      Container {                   # R55.G.17 wrapper (tagged "main_list")
        tag: "main_list",
        children: [
          Scroll {                  # rows column
            tag: "main_list_scroll",
            viewport: { w: 220, h: 164 },
            offset_x: 0,
            offset_y: 0,
            content: Container {
              children: [<12 row Containers, each tagged "main_list#i">],
            }
          },
          Container {               # R55.D.4 scrollbar visual sibling
            children: [thumb],       # R55.D.6 absolute-positioned thumb
          }
        ]
      }
    ]
  }

The demo asserts each layer's discriminator / tag / dimensions /
children count, so a future regression in the snapshot traversal
(missing recursion, dropped tag, wrong wire shape) surfaces as a
typed `AssertionError` rather than a silent visual drift.

R51.195 carry — once `scene/wheel` injection lands, this demo
extends with a wheel event followed by a second snapshot that
checks `offset_y > 0`.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


VIEWPORT_W = 220
VIEWPORT_H = 5 * 28 + 4 * 6  # ROW_HEIGHT=28, ROW_GAP=6 — see main.rs
N_ROWS = 12


WIN_W = 360
WIN_H = 320


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        snap = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        assert_eq(snap.get("type"), "Container", "outer root type")

        outer_children = snap.get("children") or []
        assert_eq(len(outer_children), 1, "outer Container children count")

        # R55.G.17 §5.49 — Scroll is now wrapped in a Container tagged
        # `main_list`. The wrapper makes the composite root paint-
        # addressable via `{path: "main_list"}` and lets `rect_for_tag`
        # attach the listbox's AT bounds to the visible viewport area
        # rather than the full window.
        listbox_root = outer_children[0]
        assert_eq(listbox_root.get("type"), "Container", "listbox wrapper type")
        assert_eq(listbox_root.get("tag"), "main_list", "listbox wrapper tag")

        # R55.D.4 §5.45 — the listbox wrapper now holds two
        # children: the Scroll (rows) + the visible scrollbar peer
        # (track + thumb). The peer is paint-only for R55.D.4;
        # R55.D.5 will wire drag input through `ScrollBarExternal`.
        wrapper_children = listbox_root.get("children") or []
        assert_eq(len(wrapper_children), 2, "listbox wrapper children count")

        scroll = wrapper_children[0]
        assert_eq(scroll.get("type"), "Scroll", "scroll node type")
        assert_eq(scroll.get("tag"), "main_list_scroll", "scroll tag")

        # R55.D.4 §5.45 — scrollbar visual sibling. Outer Container
        # = track (fills SCROLLBAR_W × VIEWPORT_H with TRACK_FILL).
        # R55.D.6 §5.45 §5.21 — track holds one absolute-positioned
        # thumb Container; the pre-R55.D.6 spacer + flex-Column
        # workaround is retired now that `LayoutStyle::
        # with_absolute_position` lands the thumb at exact
        # `(0, thumb_y_offset)` without a sibling spacer.
        scrollbar = wrapper_children[1]
        assert_eq(scrollbar.get("type"), "Container", "scrollbar visual type")
        scrollbar_children = scrollbar.get("children") or []
        assert_eq(
            len(scrollbar_children), 1, "scrollbar visual children (absolute thumb)"
        )
        assert_eq(scroll.get("offset_x"), 0, "initial offset_x")
        assert_eq(scroll.get("offset_y"), 0, "initial offset_y")

        viewport = scroll.get("viewport") or {}
        assert_eq(viewport.get("w"), VIEWPORT_W, "viewport.w")
        assert_eq(viewport.get("h"), VIEWPORT_H, "viewport.h")

        content = scroll.get("content") or {}
        assert_eq(content.get("type"), "Container", "scroll content type")

        rows = content.get("children") or []
        assert_eq(len(rows), N_ROWS, "row container count")

        # Every row is itself a Container tagged "main_list#<i>" — the
        # snapshot recurses through each row so we can sanity-check
        # the tag without driving any input.
        for i, row in enumerate(rows):
            assert_eq(row.get("type"), "Container", f"row[{i}] type")
            assert_eq(row.get("tag"), f"main_list#{i}", f"row[{i}] tag")


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox snapshot tree", body))
