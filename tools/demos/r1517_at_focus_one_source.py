#!/usr/bin/env python3
"""R1517 §5.39 §5.40 §2 #7 — the AT tree's focus flag has ONE source.

pinion answers "is this focused" on three channels (see the `read_button_focused`
doc): the per-External boolean a composite binding paints its own ring from, the
`focused: Option<&str>` argument the shell hands `access_node`, and the reactive
binding-wide tag `focus_state::focused()`. They answer different questions and
are not interchangeable — but nothing said which one the **accessibility tree**
speaks, and `hello-property-grid`'s asset-dialog buttons had picked the paint
channel: their `AccessState::focused` was re-derived by walking the painted scene
in `read_state`, on the paint's cadence rather than the a11y builder's.

That is not a wrong announcement today — measured over this very wire, the two
sources agreed in every dispatch reachable (a read dispatch cannot mutate focus,
so the snapshot the producer holds and the manager's tag are the same instant).
It is a second source: fresh by an ordering coincidence the a11y builder does not
control, and it makes "who is focused" answerable two ways inside one tree.
R1517 removes it, and this demo is the gate that keeps it removed — for the whole
class, not the one binding that had it.

The invariant, measured across four bindings before it was written (28 samples):
**every tag claiming `state.focused` is accounted for by the focus manager's
tag** — it is either that stop itself (`hello-combobox`'s trigger,
`hello-dialog`'s button) or a descendant of it (`hello-data-grid`'s roving cell
`data_grid#0_0` under the `data_grid` stop, the WAI-ARIA
`aria-activedescendant` shape). A flag on any other tag is a flag from a source
the shell did not authorise.

  (A) boot — nothing is focused, so NO node claims focus. The control: it makes
      every later assertion attributable to the focus move rather than to some
      node that simply always says `true`.
  (B) per binding, walk the focus ring with `focus/next` and assert at each stop
      that the claimed set is accounted for, is at most one tag, and that the
      `AccessFocus` target names the focused stop.
  (C) the asset dialog — the binding this round changed — driven directly: focus
      each of the modal's three stops in turn and assert the flag FOLLOWS. A
      snapshot-sourced flag cannot follow a focus move that repaints nothing.
  (D) close the modal and re-open: no stop carries a flag left over from the
      previous life of the same External (the sticky-source failure mode).

Run from the workspace root:
    cargo build --release -p hello-property-grid -p hello-combobox \\
        -p hello-dialog -p hello-data-grid
    python3 tools/demos/r1517_at_focus_one_source.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_focus_flags,
    access_node_by_tag,
    assert_eq,
    run_demo,
    wait_until,
)

# The bindings walked in (B). Chosen for distinct a11y focus shapes: an atomic
# stop that carries the flag itself, a composite that puts it on a descendant,
# a text box, and a stop that carries no flag at all. The last three are the
# three this gate caught sourcing a flag from something other than the shell's
# focus — each is kept here as its own regression.
WALKED = [
    "hello-combobox",
    "hello-dialog",
    "hello-data-grid",
    "hello-inspector",
    "hello-property-grid",
]
STEPS = 4

GRID = "property_grid"
ASSET_DIR = "asset_fb"
ASSET_OK = "asset_ok"
ASSET_CANCEL = "asset_cancel"
MESH = 1  # the Mesh leaf's flat-model value index (the asset slot)


def access(tf):
    return tf.request("scene/access").result


def focused_tag(tf):
    return tf.request("focus/get").result.get("focused")


def accounted_for(tag: str, stop: str) -> bool:
    """Is `tag`'s focus claim authorised by the focus manager holding `stop`?

    True for the stop itself, and for a descendant it addresses through the
    `aria-activedescendant` convention — pinion spells that parentage in the tag
    (`{stop}#{child}`), which is why this is a string relation and not a tree
    walk: the access tree is flat, and the tag IS the identity.
    """
    return tag == stop or tag.startswith(f"{stop}#")


def check_stop(tf, label: str, stop: str) -> int:
    """Assert the claimed-focus set is accounted for by `stop`. Returns the
    number of assertions made, so the demo can report its own coverage rather
    than the docstring asserting a count nobody checks."""
    acc = access(tf)
    flags = access_focus_flags(acc)
    made = 0
    for tag in sorted(flags):
        assert accounted_for(tag, stop), (
            f"{label}: {tag!r} claims AT focus while the focus manager holds "
            f"{stop!r} — a focus flag from a source the shell did not authorise"
        )
        made += 1
    assert len(flags) <= 1, (
        f"{label}: {sorted(flags)} — two nodes claim focus at once, so an AT "
        f"client is told focus is in two places"
    )
    made += 1
    target = acc.get("focus")
    if target is not None:
        assert_eq(target.get("tag"), stop, f"{label}: the AccessFocus target is the stop")
        made += 1
    return made


def open_picker(tf) -> None:
    tf.invoke("/external/begin", MESH)
    wait_until(
        lambda: tf.query("/asset_modal/external/open"),
        timeout=4.0,
        desc="the asset picker opened",
    )


def body() -> None:
    made = 0
    stops_walked = 0

    # ── (A) + (B) the focus ring of each binding ─────────────────────────────
    for example in WALKED:
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            # (A) control — nothing focused at boot, so nothing may claim it.
            assert focused_tag(tf) is None, f"{example}: nothing is focused at boot"
            boot_flags = access_focus_flags(access(tf))
            assert_eq(
                sorted(boot_flags), [], f"{example}: no node claims focus at boot"
            )
            made += 2

            # (B) walk the ring. A binding with one stop simply revisits it —
            # that is still a real sample of "the flag is where focus is".
            seen: set[str] = set()
            for step in range(STEPS):
                stop = tf.request("focus/next").result.get("focused")
                assert stop is not None, (
                    f"{example}: focus/next found no stop at step {step}; this "
                    f"demo's assertions would be vacuous"
                )
                made += 1
                seen.add(stop)
                made += check_stop(tf, f"{example} step {step}", stop)
                stops_walked += 1
            assert seen, f"{example}: at least one stop was visited"
            made += 1

    # ── (C) + (D) the asset dialog, the binding this round changed ───────────
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        open_picker(tf)
        made += 1
        opened_stop = focused_tag(tf)
        assert_eq(opened_stop, ASSET_DIR, "C: the opened modal focuses its file list")
        made += 1

        # (C) the flag follows focus across the modal's three stops. The two
        # action buttons are the pair whose flag used to come from `read_state`.
        for stop in (ASSET_CANCEL, ASSET_OK, ASSET_DIR):
            got = tf.request("focus/set", {"tag": stop}).result.get("focused")
            assert_eq(got, stop, f"C: focus/set moved focus to {stop}")
            made += 1
            made += check_stop(tf, f"C {stop}", stop)
            node = access_node_by_tag(access(tf), stop)
            assert node is not None, f"C: {stop} is in the open modal's AT tree"
            made += 1

        # Both buttons are present throughout, so (C)'s moves are between nodes
        # that coexist — the flag moving is a real transfer, not one node
        # appearing while another vanishes.
        acc = access(tf)
        for tag in (ASSET_OK, ASSET_CANCEL, ASSET_DIR):
            assert access_node_by_tag(acc, tag) is not None, (
                f"C: {tag} coexists with the others, so the transfer is between "
                f"simultaneously-present nodes"
            )
            made += 1

        # (D) close and re-open. The Externals outlive the modal's paint scene,
        # so a flag sourced from one of them could survive a focus it no longer
        # holds — the sticky-source failure a paint-cadence source invites.
        tf.key(path=ASSET_DIR, name="Escape")
        wait_until(
            lambda: not tf.query("/asset_modal/external/open"),
            timeout=4.0,
            desc="the asset picker closed",
        )
        made += 1
        open_picker(tf)
        reopened = focused_tag(tf)
        made += check_stop(tf, "D reopened", reopened)
        assert_eq(reopened, ASSET_DIR, "D: the re-opened modal focuses its file list")
        made += 1
        # R1518 — "stale" is a claim the reopened stop does not ACCOUNT FOR, not
        # merely one that is not the stop itself. This line used to subtract
        # `{reopened}`, which passed only because the re-opened list's cursor row
        # was in the MISSING class: `access_focus_target` named
        # `asset_fb#0` as the active descendant and no node echoed it. Now the
        # assembler stamps the bearer the target names, so the cursor row carries
        # the flag — authorised, and exactly what this demo's own
        # `accounted_for` has always allowed.
        stale = [
            tag
            for tag in sorted(access_focus_flags(access(tf)))
            if not accounted_for(tag, reopened)
        ]
        assert_eq(stale, [], "D: no flag survived from the modal's previous life")
        made += 1

    # ── (E) a roving cursor does not outlive the focus that authorised it ────
    # The sharpest case, and the one a boot-time check cannot reach: drive a
    # composite's internal cursor, then move the shell's focus AWAY. WAI-ARIA
    # defines `aria-activedescendant` only while the composite owns focus, so the
    # cursor row must stop claiming it. Ungated (measured before this round) the
    # property grid answered with TWO claimed tags at once — the row it had left
    # AND the search box it had moved to — while its own `access_focus_target`
    # correctly reported only the search box. A tree that contradicts its own
    # focus target is the failure this whole gate is named for.
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        tf.request("focus/set", {"tag": GRID})
        for _ in range(2):
            tf.key(path=GRID, name="ArrowDown")
        made += check_stop(tf, "E cursor armed", GRID)
        armed = access_focus_flags(access(tf))
        assert armed, (
            "E: the arrow keys put the cursor on a row that claims focus — "
            "without this the (E) assertions below would be vacuous"
        )
        made += 1

        search = tf.request("focus/next").result.get("focused")
        assert search != GRID, f"E: focus/next left the grid (went to {search!r})"
        made += 1
        made += check_stop(tf, "E focus left the grid", search)
        left_behind = access_focus_flags(access(tf)) & armed
        assert_eq(
            sorted(left_behind),
            [],
            "E: no row the grid had the cursor on still claims focus",
        )
        made += 1

    assert stops_walked >= len(WALKED) * STEPS, f"{stops_walked} stops walked"
    assert made >= 30, f"only {made} assertions made"
    print(f"[demo] {made} assertions over {stops_walked} focus stops")


if __name__ == "__main__":
    sys.exit(run_demo("R1517 the AT focus flag has one source", body))
