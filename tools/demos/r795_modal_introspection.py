#!/usr/bin/env python3
"""R795 §5.16 §5.12 — modal open-state as a first-class RPC query.

Before R795 the four modal surfaces (dialog / drawer / file-open / file-save)
carried no introspect external: their open state was only *inferable* from
scene-tree panel presence (`scene/snapshot` archaeology), while the
[`ContextMenu`] popup already exposed a first-class `open` query. That was an
AI-first asymmetry the R794 audit flagged as a real gap — §2 #2 makes the RPC
plane the AI's *primary* path, so "is this modal up?" should be one `scene/query`,
answerable whether the modal is open *or closed*, not a walk of the paint tree.

R795 closes it: each modal's lifted [`ModalState`] (R788) gains a query-only
[`ModalIntrospect`] extra-external at `<tag>_state`, surfacing the open flag at
`/<...>_state/external/open`. It lives in the *state* scene (always present,
frozen at boot), so it answers even while the modal is closed — unlike the
panel, which only paints when open. No second source of truth: the flag still
lives once in `ModalState`; this node only reads it.

Crucially the slot is **read-only**. Opening / closing must move the
[`modal_scope_request`] focus trap in lockstep (the whole reason `ModalState`
exists) — a raw `intervene` write would raise the flag without installing the
trap, so the write is refused with `ReadOnly`. Opening goes through the trigger
(reducer), closing through Escape.

Driven across all four modal bindings:

  (A) closed → `open` queries False (answerable with the panel down — the
      asymmetry this round removes).
  (B) read-only → `intervene open=true` is refused (the focus-trap lockstep
      cannot be bypassed over the wire).
  (C) open via the trigger → `open` queries True (tracks the shared flag).
  (D) Escape → `open` queries False again (round-trips with the lifecycle).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

PAUSE = 0.12
VIEWPORT = (600, 600)

# (example, trigger tag, panel tag, modal-introspect tag)
MODALS = [
    ("hello-dialog", "open_dialog", "dialog_panel", "dialog_state"),
    ("hello-drawer", "drawer_trigger", "drawer_panel", "drawer_state"),
    ("hello-file-open-dialog", "open_file", "fileopen_panel", "fileopen_state"),
    ("hello-file-save-dialog", "save_file", "savefile_panel", "savefile_state"),
]


def open_path(state_tag: str) -> str:
    return f"/{state_tag}/external/open"


def drive(example: str, trigger: str, panel: str, state_tag: str) -> None:
    op = open_path(state_tag)
    with RpcSubprocess(example, boot_grace=1.5) as tf:
        # ── (A) closed → open is queryable as False ──────────────────
        assert_eq(
            tf.query(op),
            False,
            f"{example}: modal introspects as closed at boot "
            "(answerable with the panel down — the asymmetry R795 removes)",
        )

        # ── (B) the open slot is read-only ──────────────────────────
        refused = False
        try:
            tf.intervene(op, True)
        except RpcError:
            refused = True
        assert refused, (
            f"{example}: writing `open` is refused — opening must install the "
            "focus trap in lockstep, so it cannot be set by a raw rewind"
        )
        assert_eq(
            tf.query(op),
            False,
            f"{example}: the refused write left the flag untouched",
        )

        # ── (C) open via the trigger → open is True ─────────────────
        tf.click(path=trigger)
        time.sleep(PAUSE)
        assert_eq(
            tf.query(op),
            True,
            f"{example}: open() (via the trigger reducer) flips the introspected flag",
        )
        # The panel is now painted too — the scene-as-data path still works,
        # but the AI no longer has to walk for it.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)

        def present(node, tag) -> bool:
            if isinstance(node, dict):
                if node.get("tag") == tag:
                    return True
                return any(present(ch, tag) for ch in node.get("children") or [])
            return False

        assert present(snap, panel), f"{example}: panel paints while open"

        # ── (D) Escape dismisses → open is False again ──────────────
        tf.key(path=panel, name="Escape")
        time.sleep(PAUSE)
        assert_eq(
            tf.query(op),
            False,
            f"{example}: Escape closes — introspected flag round-trips with the lifecycle",
        )


def body() -> None:
    for example, trigger, panel, state_tag in MODALS:
        drive(example, trigger, panel, state_tag)


if __name__ == "__main__":
    sys.exit(run_demo("R795 §5.16 §5.12 — modal open-state RPC query", body))
