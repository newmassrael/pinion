#!/usr/bin/env python3
"""R1585 §5.18 §5.7 §2 #7 — a method says how a window is named to it.

There are two spellings for naming a window to this dispatcher and they mean
different things: `params.window` is the dispatch SCOPE (whose state answers)
and `/window[<id>]/` is a prefix on a scene PATH (which subtree it runs
through). Which method takes which was published nowhere, and the cost of that
is on the record: R1581 tried `window[main]/scene/access`, met a bare
`-32601 Method not found`, and registered a debt saying `scene/access` could
not be asked about a window. It could, and had been able to all along; R1583
measured it and withdrew the debt a day later.

This round closes the gap from both ends. What this script checks, and why
each check discriminates:

* **The vocabulary is ON THE WIRE.** `rpc/methods` carries `window_doc`
  beside `occ_doc`, for the same reason `occ_doc` exists: a field name cannot
  hold a distinction and the rustdoc explaining it is not something an agent
  reads.
* **Every method carries its `window` class**, `scope` or `path`, and the
  class is PROVEN rather than parsed — see the crate test
  `r1585_the_window_column_is_what_the_methods_actually_do`, which probes all
  108 methods with a malformed prefix and compares the published column with
  what they do. A source census cannot answer this: a call graph keyed by
  function name merges four different `fn parse` and credits `font/parse` and
  `scene/displays` with a window prefix neither reads.
* **THE FAILURE THAT HAPPENED IS NOW SELF-CORRECTING.** The same call R1581
  made is still refused — it is still not a method — but the refusal names the
  method meant, the window named, and the spelling that works. The correction
  is DERIVED from the catalog, so a method added tomorrow is corrected with no
  edit, and a name that is not a method after stripping gets no invented
  advice.
* **The two spellings really are different**, shown rather than asserted: the
  scope reaches a method that takes no path at all, and the path prefix
  reaches a `path` method — each with the other spelling doing nothing.
* **PAST the toolkit**: meta-object publishes a method's name, parameter types and
  return type; there is no notion of a window-scoped introspection call at
  all, so nothing there can be asked what a call is addressed to, and a toolkit
  application cannot be interrogated over a wire in the first place.

Run from the workspace root:
    cargo build -p hello-dock-panels --release
    python3 tools/demos/r1585_a_method_says_how_a_window_is_named.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, assert_eq, run_demo  # noqa: E402

#: The methods whose own path argument may carry a `/window[<id>]/` prefix,
#: mirrored rather than imported — a demo that read the answer out of the code
#: under test could not catch it changing.
PATH_ADDRESSED = {
    "scene/bbox",
    "scene/draw_profile",
    "scene/dry_run",
    "scene/intervene",
    "scene/invoke",
    "scene/layout",
    "scene/query",
    "scene/rewind",
    "scene/screenshot",
    "scene/simulate",
    "scene/snapshot",
    "scene/waitFor",
}


def methods(tf: RpcSubprocess) -> dict:
    resp = tf.request("rpc/methods", {})
    assert resp is not None, "rpc/methods returned no response"
    assert isinstance(resp.result, dict), f"expected an object, got {resp.result!r}"
    return resp.result


def refusal(tf: RpcSubprocess, method: str, params: dict | None = None) -> dict:
    """Send a call that must be refused, and answer the error frame."""
    try:
        tf.request(method, params or {})
    except RpcError as err:
        return {"code": err.code, "data": err.data}
    raise AssertionError(f"{method} was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the catalog carries the vocabulary ──────────────────────────
        catalog = methods(tf)
        entries = catalog["methods"]
        assert entries, "A: the catalog is not empty"
        assert_eq(catalog["count"], len(entries), "A: the count is the length")
        legend = catalog.get("window_doc", "")
        assert legend, "A: the window legend is ON THE WIRE, not only in rustdoc"
        assert "params.window" in legend, "A: it names the scope spelling"
        assert "/window[" in legend, "A: and the path spelling"
        assert "never" in legend.lower(), (
            "A: and states the thing that was assumed and was false — a method "
            "NAME carries neither spelling"
        )
        assert catalog.get("occ_doc"), "A: the occ legend is still there (R1091)"

        # ── (B) every method declares its class, and only the two words ─────
        classes = {e["name"]: e["window"] for e in entries}
        assert_eq(len(classes), len(entries), "B: one class per method, no gaps")
        assert_eq(
            sorted(set(classes.values())),
            ["path", "scope"],
            "B: a closed vocabulary — an agent can match on it",
        )
        declared_path = {name for name, w in classes.items() if w == "path"}
        assert_eq(
            sorted(declared_path),
            sorted(PATH_ADDRESSED),
            "B: exactly the methods whose path argument takes a window prefix",
        )
        assert_eq(classes["scene/access"], "scope", "B: the method R1581 asked about")
        assert_eq(classes["scene/query"], "path", "B: and one that takes both")
        assert_eq(classes["rpc/methods"], "scope", "B: the catalog describes itself")

        # ── (C) the failure that produced a false debt now teaches ──────────
        error = refusal(tf, "window[main]/scene/access")
        assert_eq(error["code"], -32601, "C: it is still not a method")
        data = error["data"]
        assert isinstance(data, str), f"C: the refusal carries a sentence: {data!r}"
        assert "scene/access" in data, "C: it names the method the caller meant"
        assert "params.window" in data, "C: and the spelling that works"
        assert '"main"' in data, "C: and the window the caller named"
        assert "rpc/methods" in data, "C: and where to read the rule"

        # ── (D) the correction is DERIVED, never invented ───────────────────
        error = refusal(tf, "window[main]/scene/nonesuch")
        assert_eq(
            error["data"],
            "window[main]/scene/nonesuch",
            "D: a name that is not a method after stripping gets no advice — "
            "the correction comes from the catalog, so it cannot be a guess",
        )
        error = refusal(tf, "scene/nope")
        assert_eq(error["data"], "scene/nope", "D: an ordinary unknown is untouched")

        # ── (E) the two spellings are genuinely different ───────────────────
        # The scope reaches a method that has no path argument at all.
        resp = tf.request("scene/windows", {"window": "main"})
        assert resp is not None and resp.result is not None, (
            f"E: the scope is accepted by a method with no path: {resp}"
        )
        # An unknown scope is refused whatever the method, because the gate
        # runs BEFORE routing — which is what makes `scope` universal.
        error = refusal(tf, "rpc/methods", {"window": "no-such-window"})
        assert error["code"] in (-32602, -32601), f"E: unknown scope refused: {error}"
        # And the path prefix is read as a path by a `path` method.
        error = refusal(tf, "scene/query", {"path": "/window[]/anything"})
        assert_eq(
            error["data"],
            "EmptyWindowId",
            "E: a `path` method parses the prefix and names the syntax fault "
            "with the published word (rpc/errors)",
        )

        # ── (F) one failure, one word, whichever method meets it ────────────
        # `scene/waitFor` reaches its path through `query`, and used to answer
        # this with the bare wrapper name "Query" — the transport's own
        # classification published in place of the fact observed.
        error = refusal(
            tf,
            "scene/waitFor",
            {"path": "/window[]/anything", "target": "x", "max_attempts": 1},
        )
        assert_eq(
            error["data"],
            "EmptyWindowId",
            "F: the CAUSE, not the wrapper — the same word scene/query gives",
        )

        # ── (G) the catalog is stable and self-consistent ───────────────────
        again = methods(tf)
        assert_eq(again["methods"], entries, "G: the catalog is a constant")
        assert_eq(again["window_doc"], legend, "G: and so is its legend")
        names = [e["name"] for e in entries]
        assert_eq(names, sorted(names), "G: sorted, so a client may bisect it")
        assert_eq(len(set(names)), len(names), "G: and duplicate-free")


if __name__ == "__main__":
    sys.exit(
        run_demo(
            "R1585 §5.18 §5.7 §2 #7 — a method says how a window is named to it",
            body,
        )
    )
