#!/usr/bin/env python3
"""hello-toggle style snapshot dogfood (§5.49 R59, R55.G.8).

Proves the R55.G.8 BoxStyle / TextStyle snapshot exposure works
end-to-end against a real running shell. Snapshots hello-toggle and
asserts that the painted chrome a sighted user sees — track corner
radius, knob corner radius, status text colour — survives the JSON-RPC
wire as structured data (no OCR required).

Mirrors the first-consumer pattern used in `hello_listbox_row_click.py`
for R51.200's nested-scroll translation: the new substrate surfaces
needs at least one running-shell demo to satisfy
[[ai-first-rpc-introspection-obligation]] before the round closes.

R57.X.toggle theme retrofit — every visible color now resolves
through a `ColorRole` against the active `Theme` (R57.0 §5.50).
The pinned RGB values reflect the Material 3 Switch role mapping in
the canonical light palette (`Theme::light()`):

  - Knob Off+Idle  -> `ColorRole::Outline`   = `#C0C0C0`
  - "Dark mode"    -> `ColorRole::OnSurface` = `#1A1A1A`

Asserts:
  1. "main_toggle" Container.style.corner_radius == 16 (TRACK_RADIUS)
  2. The Container's first child is the knob Box with
     style.corner_radius == 12 (KNOB_RADIUS) and an opaque
     `Outline`-role fill (R, G, B = 0xc0, 0xc0, 0xc0) — the M3 Switch
     idle-off thumb mapping in the light palette.
  3. Walking the scene depth-first the first Text node carries
     `font_size_px == 18` and a foreground colour matching
     `Theme::light().on_surface` = `Color::rgb(0x1a, 0x1a, 0x1a)`.

Exit 0 when every assertion holds, non-zero with a typed reason on
failure (so the workflow loop can short-circuit on regression).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Iterator

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo


def walk(node: Any) -> Iterator[dict]:
    """Depth-first walk yielding every node dict in the snapshot tree.

    Container / Scroll descend via `children` / `content`; leaf
    primitives yield only themselves. R51.198 §5.49 shape.
    """
    if not isinstance(node, dict):
        return
    yield node
    children = node.get("children")
    if isinstance(children, list):
        for child in children:
            yield from walk(child)
    content = node.get("content")
    if isinstance(content, dict):
        yield from walk(content)


def body() -> None:
    with RpcSubprocess("hello-toggle") as toggle:
        # Paint-mode snapshot — the External wrapper around the Toggle
        # widget hides the view fn output by default; paint mode runs
        # `V::view` and dumps the actual painted scene so we can read
        # the Container/Box/Text nodes' style sidecars.
        snap = toggle.snapshot(source="paint")
        toggle_node = find_by_tag(snap, "main_toggle")
        if toggle_node is None:
            raise AssertionError("main_toggle Container not found in snapshot")
        # 1) Track corner_radius — proves BoxStyle wire shape on
        #    Container.
        track_style = toggle_node.get("style")
        assert_eq(
            isinstance(track_style, dict),
            True,
            "main_toggle.style is an object",
        )
        assert_eq(
            track_style.get("corner_radius"),
            16,
            "main_toggle track corner_radius",
        )
        # 2) Knob = first child Box of main_toggle. R55.G.8 BoxStyle
        #    must round-trip {fill: {r,g,b,a}, corner_radius}.
        children = toggle_node.get("children") or []
        knob = next((c for c in children if c.get("type") == "Box"), None)
        if knob is None:
            raise AssertionError("knob Box not found inside main_toggle")
        knob_style = knob.get("style") or {}
        assert_eq(
            knob_style.get("corner_radius"),
            12,
            "knob corner_radius",
        )
        knob_fill = knob_style.get("fill") or {}
        # R57.X.toggle — knob Off+Idle sources its fill from
        # `ColorRole::Outline`, which `Theme::light()` binds to
        # `#C0C0C0` (Material 3 Switch outlined-thumb mapping).
        assert_eq(knob_fill.get("r"), 0xC0, "knob fill.r (idle-off Outline role)")
        assert_eq(knob_fill.get("g"), 0xC0, "knob fill.g (idle-off Outline role)")
        assert_eq(knob_fill.get("b"), 0xC0, "knob fill.b (idle-off Outline role)")
        assert_eq(knob_fill.get("a"), 0xFF, "knob fill.a (opaque)")
        # 3) First Text in the scene is the "Dark mode" label —
        #    TextStyle.font_size_px == 18, fg_color resolves through
        #    `ColorRole::OnSurface` against `Theme::light()`, which
        #    binds to `#1A1A1A` (Material 3 onSurface, 18.5:1 contrast
        #    on the white surface).
        first_text = next(
            (n for n in walk(snap) if n.get("type") == "Text"),
            None,
        )
        if first_text is None:
            raise AssertionError("no Text node in snapshot")
        text_style = first_text.get("style") or {}
        assert_eq(
            text_style.get("font_size_px"),
            18,
            "label TextStyle.font_size_px",
        )
        fg = text_style.get("fg_color") or {}
        assert_eq(fg.get("r"), 0x1A, "label fg_color.r (OnSurface role)")
        assert_eq(fg.get("g"), 0x1A, "label fg_color.g (OnSurface role)")
        assert_eq(fg.get("b"), 0x1A, "label fg_color.b (OnSurface role)")


if __name__ == "__main__":
    sys.exit(run_demo("hello-toggle style snapshot", body))
