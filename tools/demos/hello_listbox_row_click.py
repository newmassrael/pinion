#!/usr/bin/env python3
"""hello-listbox row click dogfood (§5.49 R59, R51.200 / R51.201).

First-consumer evidence for the R51.200 nested-scroll absolute-
coordinate translation: rows inside `Scene::Scroll.content` are
laid out in scroll-local space, but `scene/click {path: "main_list#3"}`
must still land on the row's window-absolute position. The walker's
`(viewport.{x,y} - offset_{x,y})` accumulation makes this work
without the caller knowing the scroll lives between the row tag
and the window.

Sequence:
  1. spawn hello-listbox
  2. assert initial `/external/selected_index` is null (nothing selected)
  3. scene/click `{path: "main_list#3"}` → R51.200 walker computes
     row 3's absolute rect from the scroll's content-local coord +
     scroll position; the InputRouter activation hits row 3
  4. assert `/external/selected_index` == 3
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_query


TARGET_ROW = 3


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        initial = listbox.query("/external/selected_index")
        assert_eq(initial, None, "initial selected_index")

        listbox.click(path=f"main_list#{TARGET_ROW}")
        # The deferred-input drain commits on the dispatcher's return
        # path; gate on the observed selection (R883 zero-flake).
        wait_query(
            listbox,
            "/external/selected_index",
            TARGET_ROW,
            desc=f"post-click selected_index (row {TARGET_ROW})",
        )


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox row click", body))
