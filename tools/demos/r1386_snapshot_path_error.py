#!/usr/bin/env python3
"""R1386 — PINION-PR66: scene/snapshot path errors teach the valid form.

The forcing consumer for the snapshot path-error DX fix. A failing
`scene/snapshot` path used to return a bare variant name (`"UnsupportedPath"`)
or a blanket `"Path"` — neither told an AI agent *what* was wrong or *what is
valid*, so recovery meant reading pinion's source. This demo drives the real
RPC wire and asserts the error `data` now:

  (A) baseline    — the two valid forms (empty string, `/window[<id>]`) are
                    genuinely accepted, so the error paths reject only what is
                    actually invalid.
  (B) R-66.1      — `{"path":"/"}` names the valid form (`/window[<id>]` / empty)
                    AND echoes the offending input `"/"`; NOT a bare
                    `"UnsupportedPath"`.
  (C) R-66.1      — a `/window[main]/…` tail keeps the full raw path and calls
                    out the offending remainder after the prefix.
  (D) R-66.2      — a mistyped window id surfaces the concrete PathError reason
                    tag (`UnknownWindow` / `EmptyWindowId` / `MalformedPrefix`),
                    never the collapsed blanket `"Path"`.
  (D2) R1387      — `UnknownWindow` echoes the offending id
                    (`UnknownWindow: "nope"`), so two mistyped ids give two
                    different messages; the tag stays a matchable prefix.
  (E) regression  — the sibling `params.from` error keeps its full context
                    (violating value + valid set): the fix does not regress the
                    bar it was measured against.
  (F) class proof — the SAME reason-tag improvement holds on sibling methods
                    (`scene/query`, `scene/simulate`), proving the fix was a
                    shared PathError SSOT (`PathError::wire_tag`) applied across
                    the family, not a snapshot-only patch.

Every assertion reads the failure channel of the real wire (§2 #7 scene-as-data
extends to the error surface: an agent recovers from the message, not the
source). >=30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-button"


def snapshot_data(tf: RpcSubprocess, path: str, *, source: str = "state") -> str:
    """Send `scene/snapshot` expecting failure; return the `error.data` string.

    Fails loudly if the call unexpectedly SUCCEEDS (a silent success would
    hide a regression) or if `error.data` is not a string.
    """
    try:
        tf.snapshot(path, source=source)
    except RpcError as exc:
        assert_eq(exc.code, -32602, f"snapshot {path!r}: JSON-RPC invalid-params code")
        assert isinstance(exc.data, str), f"snapshot {path!r}: error.data is a string, got {exc.data!r}"
        return exc.data
    raise AssertionError(f"snapshot {path!r} was expected to fail, but it succeeded")


def query_data(tf: RpcSubprocess, path: str) -> str:
    """Send `scene/query` expecting failure; return the `error.data` string."""
    try:
        tf.query(path)
    except RpcError as exc:
        assert_eq(exc.code, -32602, f"query {path!r}: JSON-RPC invalid-params code")
        assert isinstance(exc.data, str), f"query {path!r}: error.data is a string, got {exc.data!r}"
        return exc.data
    raise AssertionError(f"query {path!r} was expected to fail, but it succeeded")


def simulate_data(tf: RpcSubprocess, step_path: str) -> str:
    """Send a one-step `scene/simulate` expecting failure; return `error.data`."""
    params = {"steps": [{"path": step_path, "value": 1}]}
    try:
        tf.request("scene/simulate", params)
    except RpcError as exc:
        assert_eq(exc.code, -32602, f"simulate {step_path!r}: JSON-RPC invalid-params code")
        assert isinstance(exc.data, str), f"simulate {step_path!r}: error.data is a string, got {exc.data!r}"
        return exc.data
    raise AssertionError(f"simulate step {step_path!r} was expected to fail, but it succeeded")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) baseline: the valid forms are genuinely accepted ─────────────
        # The whole-tree dump: empty path and a bare window prefix both work,
        # so the error paths below reject only what is actually invalid.
        root_empty = tf.snapshot("", source="paint", viewport=(320, 200))
        assert isinstance(root_empty, dict) and root_empty.get("type"), "empty path dumps a tree"
        root_win = tf.snapshot("/window[main]", source="paint", viewport=(320, 200))
        assert_eq(root_win.get("type"), root_empty.get("type"), "/window[main] dumps the same root")

        # ── (B) R-66.1: `/` teaches the valid form AND echoes the input ──────
        d = snapshot_data(tf, "/")
        assert d != "UnsupportedPath", f"no bare variant name, got {d!r}"
        assert "/window[<id>]" in d, f"names the valid window form, got {d!r}"
        assert "empty" in d, f"names the empty-path option, got {d!r}"
        assert '"/"' in d, f"echoes the offending path, got {d!r}"

        # ── (C) R-66.1: window-prefixed tail echoes raw + offending tail ─────
        d = snapshot_data(tf, "/window[main]/external/count")
        assert '"/window[main]/external/count"' in d, f"echoes the raw path, got {d!r}"
        assert "scene tail" in d and '"/external/count"' in d, f"isolates the tail, got {d!r}"

        # ── (D) R-66.2: concrete PathError reason tag, never blanket "Path" ──
        for path, tag in (
            ("/window[nope]", 'UnknownWindow: "nope"'),  # R1387: echoes the offending id
            ("/window[]", "EmptyWindowId"),              # empty id between the brackets
            ("/window[main", "MalformedPrefix"),         # no closing bracket
        ):
            d = snapshot_data(tf, path)
            assert_eq(d, tag, f"snapshot {path!r} surfaces its concrete reason")
            assert not d.startswith("Path"), f"snapshot {path!r} is not the collapsed tag"

        # ── (D2) R1387: the UnknownWindow message ECHOES which id was rejected ─
        # Two different mistyped ids yield two different messages, each naming
        # exactly what the caller sent — an agent recovers from the message.
        d_a = snapshot_data(tf, "/window[dahsboard]")
        d_b = snapshot_data(tf, "/window[sesion]")
        assert d_a == 'UnknownWindow: "dahsboard"', f"echoes the id, got {d_a!r}"
        assert d_b == 'UnknownWindow: "sesion"', f"echoes the id, got {d_b!r}"
        assert d_a != d_b, "each offending id produces its own message"
        assert d_a.startswith("UnknownWindow"), "reason stays a matchable prefix"

        # ── (E) regression: the sibling `params.from` error keeps context ────
        try:
            tf.snapshot("", source="sideways")
        except RpcError as exc:
            fm = exc.data
            assert isinstance(fm, str), f"from-error data is a string, got {fm!r}"
            assert "state" in fm and "paint" in fm, f"from error names the valid set, got {fm!r}"
            assert "sideways" in fm, f"from error echoes the violating value, got {fm!r}"
        else:
            raise AssertionError("an invalid `from` was expected to fail")

        # ── (F) class proof: the same fix holds on sibling methods ───────────
        # `scene/query` AND `scene/simulate` shared the identical PathError
        # reason-collapse; the SSOT (`PathError::wire_tag`) fixed the family,
        # not just snapshot. Two distinct methods echo the concrete reason.
        assert_eq(query_data(tf, "/window[nope]/external/count"), 'UnknownWindow: "nope"',
                  "scene/query surfaces the concrete reason + id too")
        assert_eq(query_data(tf, "/window[/external/count"), "MalformedPrefix",
                  "scene/query malformed prefix surfaces its reason too")
        assert_eq(simulate_data(tf, "/window[nope]/external/count"), 'UnknownWindow: "nope"',
                  "scene/simulate surfaces the concrete reason + id too")


if __name__ == "__main__":
    sys.exit(run_demo("R1386 — snapshot path errors teach the valid form", body))
