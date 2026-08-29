#!/usr/bin/env python3
"""R1412 §5.49 §2 #2 #7 — a named dock-layout PRESET manager, over RPC.

A professional editor (the engine / the DCC / an IDE) saves a window arrangement
under a name and switches between saved layouts. pinion's dock topology is a
serde-serializable `DockTopology` in a reactive Signal, so a preset is a stored
topology and applying one is a `Signal::set`. `examples/hello-dock-presets` is
that manager, and this demo drives it end-to-end with NO pixels:

  * The store seeds three built-in layouts (`editor` | `wide` | `tall`) plus a
    `corrupt` witness blob (a duplicate panel id). Presets are stored as
    SERIALIZED `DockTopology` JSON, exactly as a persistence layer keeps them.
  * Applying a preset runs `serde_json::from_str::<DockTopology>` — the R1412
    seam. `DockTopology::Deserialize` is hand-written to route the parsed tree
    through `try_new`, so a corrupt blob is REJECTED, not reconstructed as an
    invalid topology.

Sections:

  (A) boot — the `editor` layout (outline rail | canvas-over-props); the RPC
      surface reports names / active / count / the live blob; the panels have
      real, distinct rects.
  (B) apply `wide` — the three panels go side by side (x increases, full
      height); the live blob changes.
  (C) apply `tall` — the three panels stack (y increases, full width).
  (D) HEADLINE (the R1412 seam) — apply `corrupt`: the validated Deserialize
      REJECTS it (`duplicate panel_id`), the status says so, and the live
      topology + painted layout are UNCHANGED. A derived Deserialize would have
      applied a broken layout.
  (E) save -> apply round-trip — save the live layout as `mine`, switch away,
      apply `mine`: it reconstructs the saved layout through serde.
  (F) delete — remove `mine`; the live layout survives; deleting a missing
      preset is a benign "no preset" status, not a crash.

Run from the workspace root:
    cargo build -p hello-dock-presets --release
    python3 tools/demos/r1412_dock_presets.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    rect_of,
    run_demo,
    wait_until,
)

VP = (920, 600)
PANELS = ("outline", "canvas", "props")
PRESETS = "/presets/external"


def q(tf: RpcSubprocess, field: str):
    return tf.query(f"{PRESETS}/{field}")


def apply(tf: RpcSubprocess, name: str):
    return tf.invoke(f"{PRESETS}/apply", name)


def snap(tf: RpcSubprocess):
    return tf.snapshot(source="paint", viewport=VP)


def panel_rect(s, panel: str) -> dict:
    node = find_by_tag(s, panel)
    assert node is not None, f"panel '{panel}' is present in the dock surface"
    return rect_of(node)


def rects(s) -> dict[str, dict]:
    return {p: panel_rect(s, p) for p in PANELS}


def right(r: dict) -> float:
    return float(r["x"]) + float(r["w"])


def bottom(r: dict) -> float:
    return float(r["y"]) + float(r["h"])


def bar_text(s, tag: str) -> str:
    node = find_by_tag(s, tag)
    assert node is not None, f"the preset bar carries the {tag!r} line"
    return node.get("content") or ""


def access_group(tf: RpcSubprocess) -> dict:
    """The §2 #7 accessibility node for the preset manager — what an AT / AI
    discovers about the surface."""
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access answers"
    nodes = resp.result.get("nodes")
    assert isinstance(nodes, list), f"scene/access.nodes is a list; got {resp.result!r}"
    for n in nodes:
        if n.get("tag") == "presets":
            return n
    raise AssertionError("the preset manager announces a 'presets' access node")


def assert_side_by_side(s, label: str) -> int:
    """The three panels are laid left-to-right, each ~full dock height. Returns
    the assertion count (all discriminating — a stacked layout fails every one)."""
    r = rects(s)
    n = 0
    assert float(r["outline"]["x"]) < float(r["canvas"]["x"]) < float(r["props"]["x"]), (
        f"{label}: outline left-of canvas left-of props (x increases)"
    )
    n += 1
    # No horizontal overlap: each panel ends at or before the next one starts.
    assert right(r["outline"]) <= float(r["canvas"]["x"]) + 1.0, f"{label}: outline | canvas"
    n += 1
    assert right(r["canvas"]) <= float(r["props"]["x"]) + 1.0, f"{label}: canvas | props"
    n += 1
    # All three share a row: each is much taller than a third of the dock (a
    # stacked layout would give each ~1/3 the height).
    for p in PANELS:
        assert float(r[p]["h"]) > 300.0, f"{label}: {p} spans the full row height, got {r[p]['h']}"
        n += 1
    return n


def assert_stacked(s, label: str) -> int:
    """The three panels are laid top-to-bottom, each ~full dock width."""
    r = rects(s)
    n = 0
    assert float(r["outline"]["y"]) < float(r["canvas"]["y"]) < float(r["props"]["y"]), (
        f"{label}: outline above canvas above props (y increases)"
    )
    n += 1
    assert bottom(r["outline"]) <= float(r["canvas"]["y"]) + 1.0, f"{label}: outline / canvas"
    n += 1
    assert bottom(r["canvas"]) <= float(r["props"]["y"]) + 1.0, f"{label}: canvas / props"
    n += 1
    for p in PANELS:
        assert float(r[p]["w"]) > 600.0, f"{label}: {p} spans the full column width, got {r[p]['w']}"
        n += 1
    return n


def body() -> None:
    with RpcSubprocess("hello-dock-presets", boot_grace=2.0) as tf:
        checks = 0

        # ── (A) boot — the editor layout + the RPC surface ───────────────
        # ★ R1895 — ordered BY NAME, not by the order they were seeded. The
        # store is `pinion_core::workspace::Workspaces` now, which sorts, so the
        # menu a person reads does not depend on the order somebody saved
        # things in — two sessions that saved the same layouts in different
        # orders used to show different menus.
        assert q(tf, "names") == ["corrupt", "editor", "tall", "wide"], (
            "(A) the store seeds the three built-ins + the corrupt witness, in name order"
        )
        checks += 1
        assert q(tf, "active") == "editor", "(A) the editor layout is active at boot"
        checks += 1
        assert q(tf, "count") == 4, "(A) four presets are stored"
        checks += 1
        assert q(tf, "status") == "ready", "(A) the status line starts at 'ready'"
        checks += 1
        boot_blob = q(tf, "active_blob")
        assert isinstance(boot_blob, str) and '"id":"e0"' in boot_blob, (
            f"(A) the live blob is the editor topology (split id e0): {boot_blob[:60]!r}"
        )
        checks += 1

        s = snap(tf)
        r = rects(s)
        # editor = outline rail | (canvas over props): outline is a NARROW,
        # full-height left rail; canvas + props share the right column, canvas
        # above props.
        assert float(r["outline"]["x"]) == 0.0, "(A) the outline rail is flush left"
        checks += 1
        assert float(r["outline"]["w"]) < float(r["canvas"]["w"]), (
            "(A) the outline rail is narrower than the canvas"
        )
        checks += 1
        assert float(r["outline"]["h"]) > 400.0, "(A) the outline rail is full height"
        checks += 1
        assert float(r["canvas"]["x"]) >= right(r["outline"]) - 1.0, (
            "(A) the canvas is to the right of the outline rail"
        )
        checks += 1
        assert abs(float(r["canvas"]["x"]) - float(r["props"]["x"])) < 2.0, (
            "(A) canvas and props share the right column (same x)"
        )
        checks += 1
        assert float(r["canvas"]["y"]) < float(r["props"]["y"]), (
            "(A) the canvas sits above props in the right column"
        )
        checks += 1

        # The preset bar reports the manager state as scene data.
        assert "dock preset manager" in bar_text(s, "status_line"), "(A) the bar names the tool"
        checks += 1
        assert "active: editor" in bar_text(s, "active_line"), "(A) the bar names the active preset"
        checks += 1
        assert "corrupt" in bar_text(s, "active_line"), "(A) the bar lists every stored preset"
        checks += 1

        # §2 #7 — the manager announces itself so an AI can discover it.
        grp = access_group(tf)
        assert grp.get("role") == "group", "(A) the preset manager is an announced group"
        checks += 1
        grp_value = (grp.get("value") or {}).get("text", "")
        assert "editor" in grp_value, "(A) the a11y value names the active preset"
        checks += 1

        # ── (B) apply wide — side by side ────────────────────────────────
        assert apply(tf, "wide") == "applied 'wide'", "(B) applying wide reports success"
        checks += 1
        assert q(tf, "active") == "wide", "(B) wide is now the active preset"
        checks += 1
        wide_blob = q(tf, "active_blob")
        assert '"id":"w0"' in wide_blob, "(B) the live blob is now the wide topology (split id w0)"
        checks += 1
        assert wide_blob != boot_blob, "(B) applying a preset changed the live topology"
        checks += 1
        wait_until(
            lambda: float(panel_rect(snap(tf), "outline")["x"])
            < float(panel_rect(snap(tf), "props")["x"]),
            desc="the wide layout lays the panels side by side",
        )
        checks += assert_side_by_side(snap(tf), "(B)")

        # ── (C) apply tall — stacked ─────────────────────────────────────
        assert apply(tf, "tall") == "applied 'tall'", "(C) applying tall reports success"
        checks += 1
        assert q(tf, "active") == "tall", "(C) tall is now active"
        checks += 1
        assert '"id":"t0"' in q(tf, "active_blob"), "(C) the live blob is the tall topology"
        checks += 1
        checks += assert_stacked(snap(tf), "(C)")

        # ── (D) HEADLINE — the R1412 seam: corrupt is REJECTED ───────────
        live_before = q(tf, "active_blob")
        active_before = q(tf, "active")
        tall_rects_before = rects(snap(tf))
        result = apply(tf, "corrupt")
        assert "rejected 'corrupt'" in result, f"(D) the corrupt blob is rejected: {result!r}"
        checks += 1
        assert "duplicate panel_id" in result, (
            f"(D) the rejection names the try_new invariant it violated: {result!r}"
        )
        checks += 1
        assert q(tf, "active") == active_before == "tall", (
            "(D) a rejected apply does NOT change the active preset"
        )
        checks += 1
        assert q(tf, "active_blob") == live_before, (
            "(D) a rejected apply does NOT change the live topology (the SEAM: a "
            "derived Deserialize would have applied an invalid layout)"
        )
        checks += 1
        # The painted layout is still the tall one — the reject never reached paint.
        assert rects(snap(tf)) == tall_rects_before, (
            "(D) the painted panels are unchanged after the rejected apply"
        )
        checks += 1
        assert "rejected 'corrupt'" in bar_text(snap(tf), "status_line"), (
            "(D) the status bar surfaces the rejection to the user"
        )
        checks += 1

        # ── (E) save -> apply round-trip through serde ───────────────────
        assert apply(tf, "wide") == "applied 'wide'", "(E) set a known live layout (wide)"
        checks += 1
        assert tf.invoke(f"{PRESETS}/save", "mine") == "saved 'mine'", "(E) save the live layout"
        checks += 1
        assert "mine" in q(tf, "names"), "(E) the saved preset appears in the store"
        checks += 1
        assert q(tf, "count") == 5, "(E) the store grew by one"
        checks += 1
        assert apply(tf, "tall") == "applied 'tall'", "(E) switch away to tall"
        checks += 1
        assert apply(tf, "mine") == "applied 'mine'", "(E) apply the saved copy"
        checks += 1
        assert q(tf, "active") == "mine", "(E) the saved preset is active"
        checks += 1
        # It must reconstruct the WIDE layout it was saved from — the serde
        # round-trip (serialize on save, validated deserialize on apply).
        checks += assert_side_by_side(snap(tf), "(E) round-tripped")

        # ── (F) delete ───────────────────────────────────────────────────
        live_wide = q(tf, "active_blob")
        assert tf.invoke(f"{PRESETS}/delete", "mine") == "deleted 'mine'", "(F) delete the preset"
        checks += 1
        assert "mine" not in q(tf, "names"), "(F) the deleted preset is gone from the store"
        checks += 1
        assert q(tf, "count") == 4, "(F) the store shrank back"
        checks += 1
        assert q(tf, "active_blob") == live_wide, "(F) the live layout survives its preset's deletion"
        checks += 1
        checks += assert_side_by_side(snap(tf), "(F) live-after-delete")
        # Deleting a missing preset is a benign status, not a crash / RPC error.
        #
        # ★ R1895 — and the status now NAMES the set. It used to read
        # `no preset 'nope'`, which told a caller nothing it could act on; the
        # framework's refusal lists what would have worked.
        missing = tf.invoke(f"{PRESETS}/delete", "nope")
        assert "editor" in missing and "wide" in missing, (
            f"(F) deleting a missing preset names the arrangements that exist: {missing!r}"
        )
        checks += 1

        # ── (G) R1895 — what this example SHIPS is not a person's to remove ──
        #
        # ★★★★★ This is what adopting `pinion_core::workspace` bought. The old
        # store was a `Vec<(String, String)>`, in which the four seeded presets
        # were indistinguishable from one somebody saved — so every one of them
        # could be deleted, INCLUDING the `corrupt` blob that leg (D) needs. A
        # `retain` cannot tell them apart; a provenance can.
        before = q(tf, "names")
        for shipped in ("editor", "corrupt"):
            refusal = tf.invoke(f"{PRESETS}/delete", shipped)
            assert "ships" in refusal and shipped in refusal, (
                f"(G) deleting a shipped arrangement is refused, naming it: {refusal!r}"
            )
            checks += 1
        assert q(tf, "names") == before, "(G) and every refused delete left the set intact"
        checks += 1
        # Saving over one is refused for the same reason: a built-in that can be
        # overwritten stops being one the moment somebody saves over it.
        over = tf.invoke(f"{PRESETS}/save", "editor")
        assert "ships" in over, f"(G) saving over a shipped arrangement is refused: {over!r}"
        checks += 1
        # ★ And the rows say so BEFORE a caller tries: name, provenance, and
        # whether the row offers a delete — the same shape the analysis shell
        # publishes, which is the point of lifting the axis at all.
        rows = q(tf, "arrangements")
        # Leg (F) deleted `mine`, so what is left is exactly what this example
        # ships — which makes the assertion stronger than a mixed set would: all
        # four say built-in, and all four say they offer no delete.
        assert len(rows) == 4, f"(G) every arrangement is a row: {rows!r}"
        checks += 1
        assert all(
            r["provenance"] == "built-in" and r["deletable"] is False for r in rows
        ), f"(G) the four this example ships advertise that they offer no delete: {rows!r}"
        checks += 1
        assert [r["name"] for r in rows] == q(tf, "names"), (
            "(G) the rows and the names slot are one set, not two"
        )
        checks += 1

        print(
            f"[demo] ok — a named dock-layout preset manager: apply / save / "
            f"delete + the validated-Deserialize corrupt-blob reject, {checks} assertions"
        )


if __name__ == "__main__":
    raise SystemExit(run_demo("r1412_dock_presets", body))
