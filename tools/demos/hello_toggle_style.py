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
through a `ColorRole` against the active `Theme` (R57.0 §5.50):

  - Knob Off+Idle  -> `ColorRole::Outline`
  - "Dark mode"    -> `ColorRole::OnSurface`

★★★★★ R1841 — THE COLOURS ARE ASKED FOR, NOT WRITTEN DOWN. Until this
round the two roles' bytes were pinned here (`#C0C0C0`, `#1A1A1A`), and
a pinned byte cannot tell "the knob stopped sourcing its fill from the
Outline role" from "the Outline role changed colour" — only the first
is a defect. R1839 raised `Theme::light().outline` to clear the WCAG
3:1 boundary floor and this demo reported the repair as a regression
(`expected 192, got 148`). `scene/theme_tokens` publishes the
role-to-colour projection of the active palette, so the expectation now
comes from the running shell and the assertion is the one this file was
always about. See `role_rgb`.

Asserts:
  1. "main_toggle" Container.style.corner_radius == 16 (TRACK_RADIUS)
  2. The Container's first child is the knob Box with
     style.corner_radius == 12 (KNOB_RADIUS) and an opaque fill equal
     to the `Outline` role the shell reports — the M3 Switch idle-off
     thumb mapping.
  3. Walking the scene depth-first the first Text node carries
     `font_size_px == 18` and a foreground colour equal to the
     `OnSurface` role the shell reports.
  4. Those two roles are DIFFERENT colours, so neither assertion above
     can be satisfied by a lookup that merely echoes the scene back.

Exit 0 when every assertion holds, non-zero with a typed reason on
failure (so the workflow loop can short-circuit on regression).
"""

from __future__ import annotations

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo, walk_nodes


def role_rgb(tf: RpcSubprocess, role: str) -> tuple[int, int, int]:
    """The channel triple `Theme::light()` binds `role` to, ASKED not copied.

    ★★★★★ R1841 — this function is the round's repair. Every assertion below
    used to carry the palette's bytes written out by hand (`0xC0, 0xC0, 0xC0`
    for `Outline`), and a demo that copies a value the framework owns is a
    second declaration of it: when R1839 moved `Theme::light().outline` from
    `#c0c0c0` to `#949494` to clear the WCAG 3:1 boundary floor, this file went
    red saying `expected 192, got 148` — reporting a REPAIR as a regression.

    A pinned byte cannot tell "the knob stopped sourcing its fill from the
    Outline role" from "the Outline role changed colour", and only the first is
    a defect. `scene/theme_tokens` publishes the role-to-colour projection of
    both palettes, so the demo can ask the running shell what the role IS and
    keep asserting the thing it was always about: that the knob's fill is that
    role's colour, over the wire, as structured data.
    """
    res = tf.request("scene/theme_tokens", {})
    assert res.result, "scene/theme_tokens answered"
    active = res.result["active"]
    tokens = res.result["palettes"][active]
    hexes = {t["role"]: t["color"] for t in tokens}
    raw = hexes.get(role)
    assert raw is not None, f"the {active} palette publishes the {role!r} role"
    # `#rrggbb` when opaque, `#rrggbbaa` otherwise — the alpha tail is not
    # asserted here because each caller checks the painted alpha itself.
    assert raw.startswith("#") and len(raw) in (7, 9), f"{role} hex shape: {raw}"
    return (int(raw[1:3], 16), int(raw[3:5], 16), int(raw[5:7], 16))


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
        # `ColorRole::Outline` (the Material 3 Switch outlined-thumb
        # mapping). ★ R1841 — the expectation is ASKED of the running
        # shell rather than copied here; see `role_rgb`.
        outline = role_rgb(toggle, "outline")
        assert_eq(knob_fill.get("r"), outline[0], "knob fill.r (idle-off Outline role)")
        assert_eq(knob_fill.get("g"), outline[1], "knob fill.g (idle-off Outline role)")
        assert_eq(knob_fill.get("b"), outline[2], "knob fill.b (idle-off Outline role)")
        assert_eq(knob_fill.get("a"), 0xFF, "knob fill.a (opaque)")
        # ★ And the ask has to be able to FAIL, or it is a tautology: a
        # role that answered whatever the knob painted would pass this
        # whatever the knob did. The knob's role is Outline and the
        # label's is OnSurface, and those two are different colours in
        # this palette — so if `role_rgb` were echoing the scene back,
        # one of these two assertions could not hold.
        on_surface = role_rgb(toggle, "on_surface")
        assert on_surface != outline, (
            "the two roles this demo reads are distinct colours, which is "
            f"what makes each assertion refutable — got {outline} twice"
        )
        # 3) First Text in the scene is the "Dark mode" label —
        #    TextStyle.font_size_px == 18, fg_color resolves through
        #    `ColorRole::OnSurface` against `Theme::light()`, which
        #    binds to `#1A1A1A` (Material 3 onSurface, 18.5:1 contrast
        #    on the white surface).
        first_text = next(
            (n for _, n in walk_nodes(snap) if n.get("type") == "Text"),
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
        assert_eq(fg.get("r"), on_surface[0], "label fg_color.r (OnSurface role)")
        assert_eq(fg.get("g"), on_surface[1], "label fg_color.g (OnSurface role)")
        assert_eq(fg.get("b"), on_surface[2], "label fg_color.b (OnSurface role)")


if __name__ == "__main__":
    sys.exit(run_demo("hello-toggle style snapshot", body))
