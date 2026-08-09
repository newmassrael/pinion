#!/usr/bin/env python3
"""R1554 §5.39 §5.35 §5.40 §5.12 — a CONTAINER states that its subtree is inert.

`LayoutStyle` carried four interaction declarations before this round —
`pointer_transparent` (R705), `focusable` (R1020), `drop_target` (R1080),
`cursor` (R1196) — and every one of them describes the node that carries it
and nothing else. The toolkit's `setEnabled` is the one that does not: a
disabled widget makes its whole subtree non-interactive, which is how
`setCheckable(true)` gates a panel of controls from one checkbox
in its title, and how `<fieldset disabled>` gates a form.

pinion could not say it. Consequently there was no group container at all:
`grep -rn GroupBox` over 29 crates and 206 examples answered nothing.

The shape: ONE flag on the region (`LayoutStyle::with_disabled`), and the
cascade derives every consequence at the place that consequence is already
decided — the §5.39 focus enumeration, `Scene::hit_test`, the a11y
assembler's stamp, and the ink. It runs in `settle_to_fixed_point`, the one
loop every paint-scene producer in both backends passes through, so a window
and a terminal cannot disagree about which controls are inert.

Four things this proves that the toolkit 6.11 cannot answer:

  1. THE CAUSE, BY NAME. `scene/disabled` reports `declared_by` for every
     inert node. The toolkit's `isEnabled()` is a bool; `isEnabledTo(ancestor)`
     answers about an ancestor the caller has ALREADY picked, so it can
     confirm a guess but never produce one; `WA_ForceDisabled` separates
     self from inherited and names nobody. Which ancestor greyed a control
     is, in the toolkit, a `parentWidget()` walk in a debugger.

  2. THE SET, ENUMERATED. The toolkit has no way to ask a window "what is disabled
     right now" — the fact exists only as a bool per widget, so a driver
     would have to already know every widget in order to poll them all.

  3. A REFUSAL WITH A NAME. `focus/set` on a gated control answers
     `tag_disabled` and hands back the region to act on.
     `setFocus()` on a disabled widget returns `void` and does
     nothing: the caller cannot distinguish "focused it" from "refused it",
     let alone learn why.

  4. WHETHER THE INK FOLLOWED. `ink` states it per node. A disabled
     GL widget keeps drawing whatever it draws and the toolkit says nothing
     about that, so an agent comparing a screenshot against "this is
     disabled" has no way to know which regions will look unchanged.

And the structural claim, asserted directly: the derived half is
RECOMPUTED from the declarations every paint, never written into the
descendants. The toolkit's `setEnabled_helper` recursively sets
`WA_Disabled` on every descendant and must walk them again to take it back,
keeping N copies of one fact in step by procedure. Here the region is
toggled twice and the tree comes back to exactly its original state.

Run from the workspace root:
    cargo build -p hello-group-box --release
    python3 tools/demos/r1554_container_states_its_subtree.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-group-box"
VIEWPORT = (420, 260)

GROUP = "advanced"
GATE = "advanced_title"
REGION = "advanced_content"
MEMBERS = ("opt_verbose", "opt_trace")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def disabled(tf) -> list:
    return tf.request("scene/disabled", {}).result["disabled"]


def by_tag(rows: list, tag: str):
    return next((r for r in rows if r["tag"] == tag), None)


def gate_on(tf) -> bool:
    return tf.query(f"/{GATE}/external/checked")


def toggle_gate(tf) -> None:
    """Flip the gate WITHOUT moving focus.

    A click on the title band would also focus it, which would hide the
    property the demo is after: focus sitting on a member when the region goes
    inert must be DROPPED by the §5.39 stale-focus guard, not left dangling on
    a tag that is no longer in the order.
    """
    tf.invoke(f"/{GATE}/external/send", "KeyboardActivate")


def _rgba(c) -> tuple:
    return (c["r"], c["g"], c["b"], c["a"])


def inks(tf, tag: str) -> list:
    """Every colour painted inside the subtree tagged `tag`, in walk order.

    By subtree rather than by tag, because the ink that matters is on the
    UNTAGGED leaves — a checkbox's box border and its label's glyph colour.
    A by-tag map would have compared only the widget roots, which are
    transparent containers, and reported "nothing faded" for a frame that had.
    """
    node = find_by_tag(paint(tf), tag)
    assert node is not None, f"{tag} is painted"
    out: list = []

    def walk(n):
        st = n.get("style") or {}
        for key in ("fill", "fg_color"):
            c = st.get(key)
            if isinstance(c, dict):
                out.append(_rgba(c))
        border = st.get("border")
        if isinstance(border, dict) and isinstance(border.get("color"), dict):
            out.append(_rgba(border["color"]))
        for child in n.get("children") or []:
            walk(child)

    walk(node)
    return out


def _srgb_to_linear(u: int) -> float:
    n = u / 255.0
    return n / 12.92 if n <= 0.04045 else ((n + 0.055) / 1.055) ** 2.4


def _linear_to_srgb(v: float) -> int:
    v = max(0.0, min(1.0, v))
    s = v * 12.92 if v <= 0.0031308 else 1.055 * (v ** (1 / 2.4)) - 0.055
    return round(s * 255)


def m3_disabled(ink: tuple, backdrop: tuple) -> tuple:
    """The Material 3 disabled ink: `ink` lerped 38 % toward `backdrop` in
    LINEAR light — re-derived here from the spec rather than read back from
    the implementation, so the assertion is independent of the constant the
    code uses. 38 % is M3's token; linear-space is [[color-lerp-linear-space]].
    """
    ch = tuple(
        _linear_to_srgb(
            _srgb_to_linear(ink[i]) + (_srgb_to_linear(backdrop[i]) - _srgb_to_linear(ink[i])) * 0.38
        )
        for i in range(3)
    )
    return ch + (ink[3],)


def label_ink(tf, member: str) -> tuple:
    """The glyph colour of `member`'s label — an untagged leaf, so addressed
    through its widget root."""
    node = find_by_tag(paint(tf), member)
    for child in node.get("children") or []:
        style = child.get("style") or {}
        if isinstance(style.get("fg_color"), dict):
            return _rgba(style["fg_color"])
    raise AssertionError(f"{member} paints no text")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        tf.set_fps(0)

        # ── the group is painted, and it opens LIVE ──────────────────────
        snap = paint(tf)
        assert find_by_tag(snap, GROUP) is not None, "the group frame is painted"
        assert find_by_tag(snap, GATE) is not None, "so is its title band"
        assert find_by_tag(snap, REGION) is not None, "and its content region"
        for m in MEMBERS:
            assert find_by_tag(snap, m) is not None, f"{m} is painted"
        assert gate_on(tf), "the gate opens checked"

        # ── the method is discoverable and its shape is published ────────
        methods = tf.request("rpc/methods", {}).result["methods"]
        names = [m["name"] for m in methods]
        assert "scene/disabled" in names, "scene/disabled is enumerable"
        entry = next(m for m in methods if m["name"] == "scene/disabled")
        assert_eq(entry["occ"], "read", "asking what is inert mutates nothing")
        shapes = {t["name"] for t in tf.request("rpc/schema", {}).result["types"]}
        assert "DisabledOutcome" in shapes, "the response shape is published"
        assert "DisabledEntry" in shapes, "and so is one row's"

        # ── a live tree: an empty list, not an error ─────────────────────
        assert_eq(disabled(tf), [], "nothing is inert while the gate is on")

        # ── the whole group is reachable by keyboard ─────────────────────
        order_live = tf.request("focus/get", {}).result["tab_order"]
        assert_eq(
            order_live,
            [GATE, *MEMBERS],
            "three stops: the gate, then the two members it governs",
        )

        # ── a member accepts focus and a click while live ────────────────
        tf.request("focus/set", {"tag": MEMBERS[0]})
        assert_eq(
            tf.request("focus/get", {}).result["focused"],
            MEMBERS[0],
            "a live member takes focus",
        )
        before = tf.query(f"/{MEMBERS[0]}/external/checked")
        tf.click(path=MEMBERS[0])
        assert_eq(
            tf.query(f"/{MEMBERS[0]}/external/checked"),
            not before,
            "and a live member's click reaches it",
        )

        region_live = inks(tf, REGION)
        # The legend's glyph colour, not the whole band's ink: flipping the gate
        # changes the CHECKBOX's own rendering (accent fill and check glyph
        # appear and vanish), which is the widget working, not the cascade
        # reaching somewhere it should not.
        legend_live = label_ink(tf, GATE)
        label_live = label_ink(tf, MEMBERS[0])
        backdrop = _rgba(paint(tf)["style"]["fill"])
        assert backdrop[3] == 255, "the app root is the opaque backdrop"

        # ── clear the gate: ONE flag, and everything follows ─────────────
        toggle_gate(tf)
        assert not gate_on(tf), "the gate is now clear"

        # (1) THE CAUSE, BY NAME — the column the toolkit has no accessor for.
        rows = disabled(tf)
        assert_eq(
            [r["tag"] for r in rows],
            [REGION, *MEMBERS],
            "the region and both members, in paint order",
        )
        region_row = by_tag(rows, REGION)
        assert region_row["self_declared"], "the region carries the declaration"
        assert region_row["declared_by"] is None, "nothing above it is disabled"
        for m in MEMBERS:
            row = by_tag(rows, m)
            assert not row["self_declared"], f"{m} never declared anything"
            assert_eq(row["declared_by"], REGION, f"{m} names what to act on")

        # (4) WHETHER THE INK FOLLOWED.
        for r in rows:
            assert_eq(r["ink"], "faded", f"{r['tag']}'s ink followed")

        # (2) TAB ORDER SHRANK, with no list anywhere in the binding.
        assert_eq(
            tf.request("focus/get", {}).result["tab_order"],
            [GATE],
            "Tab cannot park inside an inert region — only the gate remains",
        )
        assert_eq(
            tf.request("focus/get", {}).result["focused"],
            None,
            "and the focus that WAS on a member is dropped, not left dangling "
            "on a tag the order no longer contains",
        )

        # (3) A REFUSAL WITH A NAME.
        try:
            tf.request("focus/set", {"tag": MEMBERS[0]})
            raise AssertionError("focus/set on a gated member must be refused")
        except RpcError as exc:
            assert_eq(exc.code, -32602, "an invalid-params refusal")
            assert_eq(exc.message, "tag_disabled", "named by CAUSE")
            assert_eq(
                exc.data,
                REGION,
                "and it hands back the region to act on, not the tag refused",
            )
        # The refusal is by cause, not by the generic "not a focus stop": a tag
        # that was never focusable in the first place still answers the other
        # name, so the two are distinguishable.
        try:
            tf.request("focus/set", {"tag": GROUP})
            raise AssertionError("the group FRAME is not a focus stop")
        except RpcError as exc:
            assert_eq(exc.message, "tag_not_focusable", "a live non-stop reads differently")

        # The pointer refuses too, and the press lands on the REGION rather
        # than on the control under the cursor (the toolkit hands such an event
        # to the parent) — and never on a peer painted beneath it.
        held = tf.query(f"/{MEMBERS[0]}/external/checked")
        tf.click(path=MEMBERS[0])
        assert_eq(
            tf.query(f"/{MEMBERS[0]}/external/checked"),
            held,
            "a gated member's click changes nothing",
        )
        tf.hover(path=MEMBERS[0])
        assert_eq(
            tf.query(f"/{MEMBERS[0]}/external/state"),
            "Idle",
            "a gated member does not even take the hover posture — the router "
            "never resolves a target inside the region",
        )

        # ── the accessibility tree agrees, and the binding never said so ──
        access = tf.request("scene/access", {}).result
        # `scene/access` omits a flag it would answer `false` for, so every
        # read here goes through `.get` — a missing key IS "not disabled".
        def at_disabled(node) -> bool:
            return bool((node.get("state") or {}).get("disabled"))

        gate_node = access_node_by_tag(access, GATE)
        assert gate_node is not None, "the gate is in the AT tree"
        assert not at_disabled(gate_node), "the gate stays actionable"
        assert_eq(
            gate_node.get("controls"),
            REGION,
            "and it publishes WHAT it governs (ARIA aria-controls; Qt's "
            "checkable QGroupBox publishes no such relation)",
        )
        for m in MEMBERS:
            node = access_node_by_tag(access, m)
            assert node is not None, f"{m} is in the AT tree"
            assert at_disabled(node), f"{m} is announced aria-disabled"
        group_node = access_node_by_tag(access, GROUP)
        assert_eq(group_node["role"], "group", "the frame is a role=group")

        # ── the ink faded, and only inside the region ────────────────────
        assert_eq(
            label_ink(tf, GATE),
            legend_live,
            "the legend is NOT faded — the title band is outside the region, "
            "which is what keeps the gate usable",
        )
        region_gated = inks(tf, REGION)
        assert region_gated != region_live, "the region's ink moved"
        assert_eq(
            len(region_gated),
            len(region_live),
            "and it is the SAME nodes, recoloured — nothing added or dropped",
        )
        # The exact value, re-derived from the M3 spec in this file rather than
        # read back from the implementation's constant.
        assert_eq(
            label_ink(tf, MEMBERS[0]),
            m3_disabled(label_live, backdrop),
            "a gated label lands on M3's 38% toward the backdrop, in linear "
            "light — the same ink a self-disabled checkbox gets from its own "
            "state layer, because both read one token",
        )
        # A colour nobody painted stays unpainted: the region's own container
        # fill is transparent and must not materialise ink.
        assert (0, 0, 0, 0) in region_gated, "a transparent fill stays transparent"

        # ── the derivation is RECOMPUTED, never written into descendants ──
        # the toolkit keeps N copies of this fact and re-walks them to undo it.
        # Toggle the gate back and the whole tree returns to its original
        # state.
        toggle_gate(tf)
        assert gate_on(tf), "the gate is back on"
        assert_eq(disabled(tf), [], "and the region is live again")
        assert_eq(
            tf.request("focus/get", {}).result["tab_order"],
            order_live,
            "the Tab order is exactly what it was",
        )
        assert_eq(inks(tf, REGION), region_live, "the region's ink is restored")
        assert_eq(label_ink(tf, MEMBERS[0]), label_live, "byte for byte")

        # ── and a member is addressable again ────────────────────────────
        tf.request("focus/set", {"tag": MEMBERS[1]})
        assert_eq(
            tf.request("focus/get", {}).result["focused"],
            MEMBERS[1],
            "focus/set is accepted again, with no un-gating call of any kind",
        )

        # ── IDEMPOTENCE: the fade is not applied twice ───────────────────
        # The settle loop may run several layout passes over one frame, and a
        # relative lerp applied twice is a different colour. Gate the region,
        # then force repeated paints and assert the ink holds still.
        toggle_gate(tf)
        # Settle first: the theme provider animates its palette
        # (`theme_animated`), so a colour can still be converging a few frames
        # after a state change. That drift is the animation working; asserting
        # over it would test the spring, not the cascade.
        for _ in range(30):
            tf.tick(1.0 / 60.0)
        first = inks(tf, REGION)
        for _ in range(4):
            tf.tick(1.0 / 60.0)
        assert_eq(
            inks(tf, REGION),
            first,
            "repeated paints of one gated region fade it exactly once",
        )
        assert_eq(
            label_ink(tf, MEMBERS[0]),
            m3_disabled(label_live, backdrop),
            "and the value is still ONE application of the token, not two",
        )


if __name__ == "__main__":
    run_demo("r1554 a container states its subtree is inert", body)
