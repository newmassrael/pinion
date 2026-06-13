#!/usr/bin/env python3
"""R909 §5.38 §5.22 — selection-driven detail panel end-to-end RPC demo.

R909 adds the central editor interaction loop — *select an object,
inspect and edit its properties* — that the property grid (R836, one
fixed object) lacked. `hello-inspector` holds three heterogeneous scene
objects (Player / Main Camera / Sun Light), each with its own typed
[`CellValue`] property schema, plus a selection. The detail panel
reactively reflects the selected object; the `InspectorExternal` exposes
selection-relative addressing (`value.<i>` resolves against the selected
object) plus a `select` action.

The typed value model + write reuse `CellValue` wholesale — the 4th
consumer after the property grid, the data grid, and the node-editor
port defaults ([[abstraction-needs-second-consumer]] payoff).

Since R922 the inspector is a MULTI-select model; this demo drives the
cardinality-1 case (one object selected), where the panel's "common
properties" reduce to that single object's full schema. The multi-object
core (common properties / Multiple Values / write-all) is
`r922_inspector_multi.py`.

## The verification idea: scene-as-data, deterministic

Everything is introspectable over RPC (§2 #2 AI-first primary path), so
no pixels:

  - schema: object_count / selected / selection_summary / row_count, and the
    per-object object_name.<j>.
  - selection: invoke `/external/select {i}` and intervene
    `/external/selected {i}` both move the selection; the property set
    (name.<i> / kind.<i> / value.<i>) re-targets the new object.
  - typed editing: intervene `/external/value.<i>` edits the *selected*
    object via CellValue::with_intervene (Int / Bool / a Choice option
    index / a Color hex).
  - per-object isolation: editing Player.Health leaves Camera untouched,
    and persists across a re-select.
  - errors: out-of-range select, read-only axes, unknown window.

## Demo scope (>=30 assertions, sections A-H)

  (A) Boot schema + the object roster.
  (B) Player's typed property schema (bool / int / float / choice / color).
  (C) invoke select re-targets the property set.
  (D) intervene 'selected' moves the selection too.
  (E) typed editing of the selected object (int / bool).
  (F) choice + color editing via with_intervene.
  (G) per-object isolation across re-selects.
  (H) errors: OOR select, read-only axis, unknown window.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    wait_until,
)


def _q(tf: RpcSubprocess, key: str) -> Any:
    """Query a primary-external introspect key (`/external/<key>`)."""
    return tf.query(f"/external/{key}")


def _select(tf: RpcSubprocess, index: int) -> Any:
    return tf.invoke("/external/select", index)


def _wait_ready(tf: RpcSubprocess) -> None:
    """The primary external is queryable at boot (no paint needed), but
    poll once so a slow boot does not race the first hard read."""
    wait_until(
        lambda: True if _q(tf, "object_count") == 3 else None,
        desc="inspector external ready",
    )


def _assert_boot_schema(tf: RpcSubprocess) -> None:
    """(A) object roster + default selection. R922: the panel is now the
    common-property list of the (here single) selection, and the header is the
    selection summary; a single selection is the cardinality-1 case where the
    common list IS that object's full schema."""
    assert _q(tf, "object_count") == 3, "three scene objects"
    assert _q(tf, "selected") == 0, "Player is the cursor by default"
    assert _q(tf, "selection_count") == 1, "exactly one object selected at boot"
    assert _q(tf, "selection_summary") == "Player", "single-selection header is the name"
    assert _q(tf, "row_count") == 7, "Player has seven properties"
    assert _q(tf, "object_name.0") == "Player"
    assert _q(tf, "object_name.1") == "Main Camera"
    assert _q(tf, "object_name.2") == "Sun Light"


def _assert_player_schema(tf: RpcSubprocess) -> None:
    """(B) Player's typed property schema: the common actor base
    (Visible / Layer / Locked) then its type-specific tail."""
    assert _q(tf, "name.0") == "Visible" and _q(tf, "kind.0") == "bool"
    assert _q(tf, "value.0") is True, "Visible seed True"
    assert _q(tf, "name.1") == "Layer" and _q(tf, "kind.1") == "int"
    assert _q(tf, "value.1") == 1, "Layer seed 1"
    assert _q(tf, "name.2") == "Locked" and _q(tf, "kind.2") == "bool"
    assert _q(tf, "name.3") == "Health" and _q(tf, "kind.3") == "int"
    assert _q(tf, "value.3") == 100, "Health seed 100"
    assert _q(tf, "name.4") == "Speed" and _q(tf, "kind.4") == "float"
    assert abs(float(_q(tf, "value.4")) - 6.5) < 1e-9, "Speed seed 6.5"
    assert _q(tf, "name.5") == "Team" and _q(tf, "kind.5") == "choice"
    team = _q(tf, "value.5")
    assert isinstance(team, dict) and team["label"] == "Red", "Team seed = first option"
    assert team["options"] == ["Red", "Blue", "Neutral"], "Team option domain"
    assert _q(tf, "name.6") == "Tint" and _q(tf, "kind.6") == "color"
    tint = _q(tf, "value.6")
    assert isinstance(tint, dict) and (tint["r"], tint["g"], tint["b"]) == (0x4F, 0x9D, 0xFF), (
        f"Tint seed channels; got {tint!r}"
    )


def _assert_select_retargets(tf: RpcSubprocess) -> None:
    """(C) invoke select re-targets the whole property set."""
    assert _select(tf, 1) == 1, "select returns the new cursor index"
    assert _q(tf, "selected") == 1
    assert _q(tf, "selection_summary") == "Main Camera"
    assert _q(tf, "row_count") == 5, "Camera has five properties"
    assert _q(tf, "name.0") == "Visible", "Camera's first property is the common base"
    assert _q(tf, "name.3") == "Field of View"
    assert abs(float(_q(tf, "value.3")) - 60.0) < 1e-9, "Camera FoV seed"


def _assert_intervene_selected(tf: RpcSubprocess) -> None:
    """(D) intervene 'selected' also moves the selection."""
    tf.intervene("/external/selected", 2)
    assert _q(tf, "selected") == 2
    assert _q(tf, "selection_summary") == "Sun Light"
    assert _q(tf, "name.0") == "Visible"
    assert _q(tf, "name.3") == "Intensity"
    # back to Player for the editing section.
    _select(tf, 0)
    assert _q(tf, "selected") == 0


def _assert_typed_editing(tf: RpcSubprocess) -> None:
    """(E) int + bool editing of the selected (Player) object."""
    tf.intervene("/external/value.3", 42)  # Health int.
    assert _q(tf, "value.3") == 42, "Health edited to 42"
    tf.intervene("/external/value.0", False)  # Visible bool.
    assert _q(tf, "value.0") is False, "Visible toggled off"


def _assert_choice_and_color(tf: RpcSubprocess) -> None:
    """(F) Choice (option index) + Color (hex) edits via with_intervene."""
    tf.intervene("/external/value.5", 1)  # Team -> Blue.
    team = _q(tf, "value.5")
    assert team["selected"] == 1 and team["label"] == "Blue", f"Team -> Blue; got {team!r}"
    tf.intervene("/external/value.6", "#ff0000")  # Tint -> red.
    tint = _q(tf, "value.6")
    assert (tint["r"], tint["g"], tint["b"]) == (0xFF, 0x00, 0x00), f"Tint -> red; got {tint!r}"


def _assert_per_object_isolation(tf: RpcSubprocess) -> None:
    """(G) edits on the single-selected Player do not leak to Camera, and
    persist across a re-select (per-object state)."""
    _select(tf, 1)  # Camera.
    assert abs(float(_q(tf, "value.3")) - 60.0) < 1e-9, "Camera FoV untouched by Player edits"
    _select(tf, 0)  # back to Player.
    assert _q(tf, "value.3") == 42, "Player Health edit persisted"
    assert _q(tf, "value.0") is False, "Player Visible edit persisted"
    team = _q(tf, "value.5")
    assert team["label"] == "Blue", "Player Team edit persisted"


def _assert_errors(tf: RpcSubprocess) -> None:
    """(H) out-of-range select, read-only axis, unknown window."""
    # Out-of-range select via invoke: rejected, selection unchanged.
    before = _q(tf, "selected")
    raised = False
    try:
        _select(tf, 9)
    except RpcError:
        raised = True
    assert raised, "select(9) must be rejected"
    assert _q(tf, "selected") == before, "rejected select leaves the selection unchanged"

    # Out-of-range intervene 'selected'.
    raised = False
    try:
        tf.intervene("/external/selected", 99)
    except RpcError:
        raised = True
    assert raised, "intervene selected=99 must be rejected"

    # Read-only axis.
    raised = False
    try:
        tf.intervene("/external/object_count", 1)
    except RpcError:
        raised = True
    assert raised, "object_count must be read-only"

    # Unknown window id rejected (-32602) at dispatch entry.
    raised = False
    try:
        tf.request("scene/query", {"path": "/external/selected", "window": "ghost-window"})
    except RpcError as exc:
        raised = True
        assert exc.code == -32602, f"unknown window must be -32602; got {exc.code}"
    assert raised, "querying an unknown window must raise"


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        _wait_ready(tf)
        _assert_boot_schema(tf)          # (A)
        _assert_player_schema(tf)        # (B)
        _assert_select_retargets(tf)     # (C)
        _assert_intervene_selected(tf)   # (D)
        _assert_typed_editing(tf)        # (E)
        _assert_choice_and_color(tf)     # (F)
        _assert_per_object_isolation(tf) # (G)
        _assert_errors(tf)               # (H)


if __name__ == "__main__":
    sys.exit(run_demo("R909 selection-driven detail panel", body))
