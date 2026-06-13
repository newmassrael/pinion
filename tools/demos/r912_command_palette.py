#!/usr/bin/env python3
"""R912 §5.38 §5.22 — fuzzy command palette end-to-end RPC demo.

R912 adds the editor-universal command palette (VS Code Ctrl+P): type a
query, the command list filters by a subsequence-scored fuzzy match,
Arrow keys / clicks navigate, Enter / a click runs the selected command.
A fresh widget axis composing a primary `PaletteExternal` + the R910
composite-send click-to-select wire + a pure `fuzzy_score` matcher.

Everything is scene-as-data and RPC-drivable (§2 #2): `intervene query`
filters, `query result.<i>` / `selected_command` / `last_executed` read
the state, `invoke execute` (or a click / Enter) runs the selection. The
fuzzy match is a pure function, so the filtering + ranking are fully
deterministic — zero flake.

## Demo scope (>=30 assertions, sections A-G)

  (A) Boot: empty query lists every command.
  (B) Fuzzy filter: a query narrows + ranks the results.
  (C) Word-start ranking ("of" -> "Open Folder" first).
  (D) Execute via invoke records last_executed.
  (E) Keyboard nav (Arrow/Home/End select; Enter runs).
  (F) Click a result row runs it (select + execute on the edge).
  (G) Empty results + error paths.
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
    wait_query,
    wait_until,
)

_N_COMMANDS = 10


def _q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def _set_query(tf: RpcSubprocess, text: str) -> None:
    # intervene is a synchronous External mutate (no async paint), so a
    # following query reads the new state directly.
    tf.intervene("/external/query", text)


def _wait_ready(tf: RpcSubprocess) -> None:
    wait_until(
        lambda: True if _q(tf, "result_count") == _N_COMMANDS else None,
        desc="command palette ready",
    )


def _assert_boot(tf: RpcSubprocess) -> None:
    """(A) empty query lists all commands."""
    assert _q(tf, "query") == "", "query empty at boot"
    assert _q(tf, "result_count") == _N_COMMANDS, "all commands listed"
    assert _q(tf, "selected") == 0, "top result selected"
    assert _q(tf, "result.0") == "New File", "first command in roster order"
    assert _q(tf, "last_executed") is None, "nothing run yet"


def _assert_fuzzy_filter(tf: RpcSubprocess) -> None:
    """(B) a fuzzy query narrows + ranks."""
    _set_query(tf, "save")
    assert _q(tf, "query") == "save"
    assert _q(tf, "result_count") < _N_COMMANDS, "query narrows the list"
    assert _q(tf, "selected") == 0, "selection resets to the top match"
    assert _q(tf, "selected_command") == "Save All", "best match is Save All"
    assert _q(tf, "result.0") == "Save All"

    # A subsequence that is not contiguous still matches.
    _set_query(tf, "gtl")
    assert _q(tf, "result_count") >= 1
    assert _q(tf, "result.0") == "Go to Line", "G-t-L subsequence -> Go to Line"

    # Restore: empty query lists all again.
    _set_query(tf, "")
    assert _q(tf, "result_count") == _N_COMMANDS, "clearing the query restores all"


def _assert_word_start_ranking(tf: RpcSubprocess) -> None:
    """(C) word-start matches outrank incidental mid-word hits."""
    _set_query(tf, "of")
    assert _q(tf, "result_count") >= 1
    assert _q(tf, "result.0") == "Open Folder", "two word-starts rank first"
    _set_query(tf, "")


def _assert_execute(tf: RpcSubprocess) -> None:
    """(D) invoke execute records last_executed."""
    _set_query(tf, "theme")
    assert _q(tf, "selected_command") == "Toggle Theme"
    ran = tf.invoke("/external/execute", None)
    assert ran == "Toggle Theme", "execute returns the run command"
    assert _q(tf, "last_executed") == "Toggle Theme", "last_executed recorded"
    _set_query(tf, "")


def _assert_keyboard_nav(tf: RpcSubprocess) -> None:
    """(E) Arrow / Home / End select (no run); Enter runs."""
    _set_query(tf, "")
    # Select reset to 0 by the query above.
    tf.key(path="palette", name="ArrowDown")
    wait_query(tf, "/external/selected", 1, desc="ArrowDown -> 1")
    tf.key(path="palette", name="ArrowDown")
    wait_query(tf, "/external/selected", 2, desc="ArrowDown -> 2")
    tf.key(path="palette", name="Home")
    wait_query(tf, "/external/selected", 0, desc="Home -> first")
    tf.key(path="palette", name="End")
    wait_query(tf, "/external/selected", _N_COMMANDS - 1, desc="End -> last")
    # Enter runs the selected (last) command = "Reload Window".
    last_cmd = _q(tf, "selected_command")
    assert last_cmd == "Reload Window", "End selects the last command"
    tf.key(path="palette", name="Enter")
    wait_query(tf, "/external/last_executed", "Reload Window", desc="Enter runs the selection")


def _assert_click_runs(tf: RpcSubprocess) -> None:
    """(F) clicking a result row selects + runs it."""
    _set_query(tf, "")
    # Result row 4 in roster order = "Toggle Theme".
    assert _q(tf, "result.4") == "Toggle Theme"
    tf.click(path="palette#4")
    wait_query(tf, "/external/last_executed", "Toggle Theme", desc="click row 4 runs Toggle Theme")


def _assert_empty_and_errors(tf: RpcSubprocess) -> None:
    """(G) no-match query + error paths."""
    _set_query(tf, "zzqq")
    assert _q(tf, "result_count") == 0, "no command matches"
    assert _q(tf, "selected_command") is None, "no selected command when empty"
    # Execute on an empty list is a no-op returning null.
    assert tf.invoke("/external/execute", None) is None, "execute on empty list -> null"

    # Out-of-range select rejected.
    _set_query(tf, "reload")  # 1 result.
    assert _q(tf, "result_count") == 1
    raised = False
    try:
        tf.invoke("/external/select", 3)
    except RpcError:
        raised = True
    assert raised, "select past the result range is rejected"

    # Read-only axis.
    raised = False
    try:
        tf.intervene("/external/result_count", 1)
    except RpcError:
        raised = True
    assert raised, "result_count is read-only"


def body() -> None:
    with RpcSubprocess("hello-command-palette", boot_grace=1.5) as tf:
        _wait_ready(tf)
        _assert_boot(tf)                # (A)
        _assert_fuzzy_filter(tf)        # (B)
        _assert_word_start_ranking(tf)  # (C)
        _assert_execute(tf)             # (D)
        _assert_keyboard_nav(tf)        # (E)
        _assert_click_runs(tf)          # (F)
        _assert_empty_and_errors(tf)    # (G)


if __name__ == "__main__":
    sys.exit(run_demo("R912 fuzzy command palette", body))
