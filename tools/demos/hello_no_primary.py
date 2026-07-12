#!/usr/bin/env python3
"""hello-no-primary — PR-51 no-primary binding over the JSON-RPC wire.

Proves the R1303 / R1306 / R1307 optional-primary seam end-to-end over the
REAL JSON-RPC transport (not in-process crate calls). A binding whose
`WidgetCore::primary_surface()` returns `None` composes a state scene of
dynamic extras with no primary; over the wire (§2 #2, the AI-primary path):

  * every pane is reachable + mutable by its explicit tag, and mutating one
    pane leaves the other isolated, and
  * the bare `/external` shorthand — which names "the primary" — rejects
    cleanly with `NoExternalAtPath` (§2 #7 self-describing) instead of
    silently resolving an arbitrary pane.

This is the forcing consumer for the seam: the resolution logic is unit-
tested in `pinion-rpc`, but only this demo drives it through the actual
JSON-RPC framing + error-code mapping on a live shell.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, assert_rpc_error, run_demo


def body() -> None:
    with RpcSubprocess("hello-no-primary") as app:
        # By-tag: both panes are reachable over the wire, no primary present.
        assert_eq(app.query("/pane_0/external/count"), 0, "pane_0 initial")
        assert_eq(app.query("/pane_1/external/count"), 0, "pane_1 initial")

        # Mutate pane_0 by its tag; the returned total is the new count.
        assert_eq(app.invoke("/pane_0/external/increment", 5), 5, "pane_0 += 5")
        assert_eq(app.query("/pane_0/external/count"), 5, "pane_0 after")
        # pane_1 is isolated — the by-tag write did not leak.
        assert_eq(app.query("/pane_1/external/count"), 0, "pane_1 isolated")

        # The bare `/external` shorthand names "the primary". This binding
        # has none, so the marked no-primary-head container rejects the
        # address cleanly (NoExternalAtPath) rather than silently resolving
        # pane_0 — self-describing on the AI-primary wire (§2 #7), on both
        # the READ and the MUTATING verbs.
        assert_rpc_error(lambda: app.query("/external/count"), data="NoExternalAtPath")
        assert_rpc_error(
            lambda: app.invoke("/external/increment", 1), data="NoExternalAtPath"
        )
        # The rejected bare invoke did NOT mutate pane_0.
        assert_eq(app.query("/pane_0/external/count"), 5, "pane_0 unchanged after reject")


if __name__ == "__main__":
    sys.exit(run_demo("hello-no-primary wire (PR-51 primary_surface==None)", body))
