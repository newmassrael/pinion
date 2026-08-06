#!/usr/bin/env python3
"""R1581 §5.40 §5.39 §2 #7 — a keyboard focus stop is a node in the AT tree.

R1518 made the a11y focus flag something the ASSEMBLER stamps, which killed
two whole classes of defect (a binding claiming focus it did not have, and two
bindings claiming it at once). It also left a residue it named and did not
close: 28-30 observations where the focus target named a tag that **is not a
node in that tree at all**. `AccessTreeBuilder::build` folds such a target onto
`ROOT_NODE_ID`, so a screen-reader user who Tabs to that control is told they
are on the window — the R1329/PR-53 failure shape, on controls nobody had
checked.

This script is the closing measurement, over the wire, per binding: walk
`focus/next` until the ring repeats, and for every stop it reaches assert that
`scene/access` carries a node with that tag AND that the node says what the
control is.

What the round measured, and what it corrected:

* The debt note listed **8 bindings**; walking them found **11 stops across 9**.
  `hello-dock-panels-editor` had two missing, not one, and
  `hello-window-refocus` had three.
* Two of the listed entries were **false positives**, and both for the same
  reason: their node lives in a window the wire cannot ask about.
  `scene/access` is not window-routable, so a probe reads the primary window's
  tree and reports a stop missing whatever a sibling window holds. Both are
  pinned by Rust tests instead — `hello-multi-window`'s has existed since R813.
* `settings-panel`'s note read "AT-invisible for v1 … carries to R668". R668 is
  nine hundred rounds past, and the one control the panel is navigated by was
  the one control a screen-reader user could not hear.

Every name asserted here comes from the same source the pixels do — the
readouts share one `readout_body_text`, the dock buttons take their label from
the button, the nav rail goes through the `navigation` landmark builder every
other rail uses — so what is spoken cannot drift from what is drawn.

Run from the workspace root:
    cargo build --release -p hello-dock-chart -p hello-floating-chart \\
        -p hello-dock-panels-editor -p hello-window-refocus \\
        -p hello-tabbed-chart -p hello-dock-panels -p settings-panel \\
        -p hello-window-focus-multi
    python3 tools/demos/r1581_a_focus_stop_is_a_node.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    run_demo,
    text_of_tag,
    wait_until,
)

#: `(example, {stop tag: (role, name)})` — every stop `focus/next` reaches in a
#: SINGLE-window binding, with what the AT must say about it. The names are the
#: painted ones; a node named something else would pass a "node exists" check
#: and still leave the user hearing the wrong thing.
EXPECTED: list[tuple[str, dict[str, tuple[str, str]]]] = [
    ("hello-dock-chart", {"readout_body": ("status", "chart pane readout")}),
    ("hello-floating-chart", {"readout_body": ("status", "floating-chart readout")}),
    ("hello-tabbed-chart", {"readout_body": ("status", "tab well readout")}),
    (
        "hello-dock-panels-editor",
        {
            "float_policy_btn": ("button", None),
            "viewport_btn": ("button", "Reset camera"),
        },
    ),
    ("hello-dock-panels", {"viewport_btn": ("button", "Click me (viewport)")}),
    ("settings-panel", {"nav_rail": ("navigation", "Settings sections")}),
    ("hello-window-focus-multi", {"main_btn": ("button", "focusable button")}),
]

#: The two stops whose node lives in a window `scene/access` cannot be asked
#: about. Named here so their ABSENCE from the wire reading is a stated fact
#: rather than a silent hole — see the module docstring.
OTHER_WINDOW = {
    ("hello-multi-window", "inspector_tree"),
    ("hello-window-refocus", "notes_pane"),
}

#: Enough to close the ring on every binding here; the walk stops on a repeat.
STEPS = 8


def settled_access(tf: RpcSubprocess, expect: str):
    """`scene/access` once its `focus` agrees with the ring, else falsy.

    The AT tree the shell emits is built at paint time, so it trails the focus
    manager by a frame. R1518 made the two single-sourced; this waits for the
    frame that carries it rather than asserting across the gap.
    """
    tf.tick(0.016)
    acc = tf.request("scene/access", {}).result or {}
    return acc if (acc.get("focus") or {}).get("tag") == expect else None


def walk_stops(tf: RpcSubprocess) -> tuple[list[str], str]:
    """Every distinct tag `focus/next` reaches, and where focus ENDED.

    The two are not the same, which is the trap this returns a pair to avoid: a
    walk that stops on a repeat has already moved focus PAST its last recorded
    stop, onto the one it recognised. Asserting the AT tree against `stops[-1]`
    therefore compares the tree to a control the ring left — which is what the
    first draft of this demo did, and it read as a defect in the binding.
    """
    seen: list[str] = []
    tag = ""
    for _ in range(STEPS):
        tf.request("focus/next", {})
        tf.tick(0.016)
        got = tf.request("focus/get", {}).result or {}
        tag = got.get("focused") or ""
        if not tag or tag in seen:
            break
        seen.append(tag)
    return seen, tag


def body() -> None:
    total_stops = 0
    for example, expected in EXPECTED:
        with RpcSubprocess(example, boot_grace=1.2) as tf:
            for _ in range(2):
                tf.tick(0.016)

            stops, resting = walk_stops(tf)
            assert stops, f"{example}: the focus ring reaches nothing at all"
            # The AT tree is built AT PAINT TIME, so it trails a `focus/next`
            # by a frame; polling is what makes this an observation of the
            # settled state rather than a race ([[zero-flake-policy]]).
            acc = wait_until(
                lambda: settled_access(tf, resting),
                desc=f"{example}: scene/access focus settles on {resting!r}",
            )

            for stop in stops:
                total_stops += 1
                # A composite stop (`grid#0_0`) is announced through its parent,
                # which is the shape R1518 canonicalised; the parent is the node
                # that has to exist.
                bearer = stop.split("#")[0]
                node = access_node_by_tag(acc, stop) or access_node_by_tag(acc, bearer)
                assert node is not None, (
                    f"{example}: focus/next reaches {stop!r} and the AT tree has "
                    f"no node for it — AccessTreeBuilder folds that onto the "
                    f"window root, so a screen reader announces the WINDOW"
                )
                if bearer in expected:
                    role, name = expected[bearer]
                    assert_eq(node.get("role"), role, f"{example}/{bearer} role")
                    if name is not None:
                        assert_eq(node.get("name"), name, f"{example}/{bearer} name")

            for bearer in expected:
                assert bearer in {s.split("#")[0] for s in stops}, (
                    f"{example}: {bearer!r} was expected to be a focus stop and "
                    f"the ring reached {stops!r} — the fixture is stale"
                )

            # The focus the tree reports is the focus the ring reports: R1518's
            # own property, re-asserted here because this round moved the nodes
            # it is derived from.

    assert total_stops >= 9, f"only {total_stops} stops walked"

    # ── The readouts say the SAME sentence they paint ────────────────────────
    #
    # A node that exists but announces something else is the defect one step on.
    # Each of these three shares one `readout_body_text` between the paint and
    # the AT node, so the two cannot be two sentences — asserted by reading the
    # painted text and the announced value and comparing them.
    for example in ("hello-dock-chart", "hello-floating-chart", "hello-tabbed-chart"):
        with RpcSubprocess(example, boot_grace=1.2) as tf:
            for _ in range(2):
                tf.tick(0.016)
            painted = text_of_tag(tf, "readout_body")
            assert painted, f"{example}: the readout paints text"
            acc = tf.request("scene/access", {}).result or {}
            node = access_node_by_tag(acc, "readout_body")
            assert node is not None, f"{example}: and it is a node"
            # `AccessValue::Text` travels tagged, so the sentence is under
            # `text` — unwrapped rather than compared to the envelope.
            assert_eq(
                (node.get("value") or {}).get("text"),
                painted,
                f"{example}: spoken == drawn",
            )
            assert_eq(node.get("role"), "status", f"{example}: role")

    # ── The nav rail is a LANDMARK with links, not a lone node ───────────────
    #
    # `settings-panel` goes through the same `navigation_link_nodes` builder
    # every other rail in the tree uses, so its shape cannot be a private one.
    with RpcSubprocess("settings-panel", boot_grace=1.2) as tf:
        for _ in range(2):
            tf.tick(0.016)
        acc = tf.request("scene/access", {}).result or {}
        rail = access_node_by_tag(acc, "nav_rail")
        assert rail is not None, "settings-panel: the rail is a node"
        assert_eq(rail.get("role"), "navigation", "settings-panel: landmark role")
        assert_eq(rail.get("name"), "Settings sections", "settings-panel: name")
        links = [
            access_node_by_tag(acc, f"nav_rail#{i}") for i in range(5)
        ]
        assert all(links), f"settings-panel: five links, got {links}"
        assert_eq(
            [n.get("role") for n in links],
            ["link"] * 5,
            "settings-panel: every section is a link",
        )
        current = [n.get("tag") for n in links if n.get("current")]
        assert_eq(current, ["nav_rail#0"], "settings-panel: one aria-current")
        # Mirrored, not imported: a fixture that read the labels out of the
        # code under test could not catch them changing.
        assert_eq(
            [n.get("name") for n in links],
            ["Theme", "Appearance", "Profile", "Notifications", "Actions"],
            "settings-panel: every link is named for its section",
        )

    # ── The dock buttons take their name from the button ─────────────────────
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=1.2) as tf:
        for _ in range(2):
            tf.tick(0.016)
        acc = tf.request("scene/access", {}).result or {}
        policy = access_node_by_tag(acc, "float_policy_btn")
        assert policy is not None, "editor: the policy toggle is a node"
        assert_eq(policy.get("role"), "button", "editor: policy role")
        painted = text_of_tag(tf, "float_policy_btn")
        assert painted, "editor: the policy toggle paints a label"
        assert policy.get("name") in painted, (
            f"editor: the spoken name {policy.get('name')!r} is the painted "
            f"label {painted!r}, so a mode change moves both"
        )

    # The two stops the wire cannot judge, stated rather than skipped silently.
    for example, stop in sorted(OTHER_WINDOW):
        with RpcSubprocess(example, boot_grace=1.2) as tf:
            for _ in range(2):
                tf.tick(0.016)
            stops, _ = walk_stops(tf)
            assert stop in stops, f"{example}: {stop!r} is still a focus stop"
            # Land ON it: the ring wraps, so a plain walk comes to rest wherever
            # the repeat was detected rather than on the stop under test.
            landed = False
            for _ in range(STEPS * 2):
                got = tf.request("focus/get", {}).result or {}
                if got.get("focused") == stop:
                    landed = True
                    break
                tf.request("focus/next", {})
                tf.tick(0.016)
            assert landed, f"{example}: could not bring focus to rest on {stop!r}"
            acc = wait_until(
                lambda: settled_access(tf, stop),
                desc=f"{example}: scene/access focus settles on {stop!r}",
            )
            assert access_node_by_tag(acc, stop) is None, (
                f"{example}: {stop!r} now reads from the primary window's tree — "
                f"if scene/access became window-routable, this demo's premise "
                f"changed and the exclusion above should go"
            )
            # `settled_access` already asserted the target is still PUBLISHED
            # as this tag; the fold to the window root happens inside the tree
            # the shell emits to AccessKit, which is what those bindings' own
            # Rust tests pin.
            assert_eq(
                (acc.get("focus") or {}).get("tag"),
                stop,
                f"{example}: the focus target is published",
            )

    print(
        f"[demo] {total_stops} focus stops over {len(EXPECTED)} bindings, every "
        f"one a named node; {len(OTHER_WINDOW)} excluded and why"
    )


if __name__ == "__main__":
    run_demo("r1581_a_focus_stop_is_a_node", body)
