#!/usr/bin/env python3
"""R1089 §5.7 §5.12 §2 #7 — `rpc/methods` self-describing wire surface.

Every other dispatch method is reachable only by an AI that already knows
its literal string. `rpc/methods` is the meta-method that returns the
catalog of method names the dispatcher routes, so an agent DISCOVERS the
surface (`scene/window_move`, `scene/windows`, …) instead of needing each
literal baked in — the §2 #7 scene-as-data principle applied to the
protocol itself. The catalog is verified-complete against the actual
routing match by a Rust cross-check test (`catalog_matches_dispatch_match_arms`).

Section roadmap (>=30 assertions across A-G):

  (A) `rpc/methods` boots — `{methods: [...], count: N}`, well-shaped.
  (B) Catalog shape — every entry is a lowercase `ns/method` string,
      sorted, duplicate-free; `count == len(methods)`.
  (C) Known methods present — the catalog lists the methods this round +
      R1087/R1088 added (`scene/windows`, `scene/window_move`) plus core
      ones, so discovery is real.
  (D) Namespaces — focus / font / rpc / scene / text are all represented.
  (E) Self-reference — `rpc/methods` lists itself (the meta-method is
      discoverable too).
  (F) Discovery loop — call a method the demo learned ONLY from the
      catalog and get a valid response (discover -> use).
  (G) Negative control — a name NOT in the catalog is genuinely unrouted
      (`-32601` method-not-found), proving the catalog reflects reality.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402


def _methods(tf: RpcSubprocess) -> dict:
    resp = tf.request("rpc/methods", {})
    assert resp is not None, "rpc/methods returned no response"
    result = resp.result
    assert isinstance(result, dict), f"rpc/methods result must be an object; got {result!r}"
    return result


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) rpc/methods boots, well-shaped ────────────────────
        cat = _methods(tf)
        methods = cat.get("methods")
        count = cat.get("count")
        assert isinstance(methods, list), f"methods must be a list; got {methods!r}"
        assert isinstance(count, int), f"count must be an int; got {count!r}"
        assert count == len(methods), f"count {count} must equal len(methods) {len(methods)}"
        assert count >= 70, f"the surface should have >=70 methods; got {count}"

        # ── (B) catalog shape: lowercase ns/method, sorted, unique ─
        for m in methods:
            assert isinstance(m, str) and m, f"each method must be a non-empty string; got {m!r}"
            assert m.count("/") == 1, f"each method must be one 'ns/method'; got {m!r}"
            ns, name = m.split("/")
            assert ns and name, f"both namespace and name must be non-empty; got {m!r}"
            assert m == m.lower(), f"method names are lowercase; got {m!r}"
            assert all(c.islower() or c.isdigit() or c in "_/" for c in m), (
                f"method names are [a-z0-9_/]; got {m!r}"
            )
        assert methods == sorted(methods), "the catalog must be sorted"
        assert len(set(methods)) == len(methods), "the catalog must be duplicate-free"

        method_set = set(methods)

        # ── (C) known methods are discoverable ────────────────────
        # The R1087/R1088 additions this PR-31 arc landed, plus core ones.
        for expected in (
            "scene/windows",       # R1087 read
            "scene/window_move",   # R1088 write peer
            "scene/query",
            "scene/snapshot",
            "scene/invoke",
            "scene/intervene",
            "font/parse",
            "focus/set",
            "text/normalize",
        ):
            assert expected in method_set, f"catalog must list {expected!r}; missing"

        # ── (D) every namespace is represented ────────────────────
        namespaces = {m.split("/")[0] for m in methods}
        for ns in ("focus", "font", "rpc", "scene", "text"):
            assert ns in namespaces, f"namespace {ns!r} must appear in the catalog"
        # scene/* is the bulk of the surface.
        scene_count = sum(1 for m in methods if m.startswith("scene/"))
        assert scene_count >= 50, f"scene/* should dominate the surface; got {scene_count}"

        # ── (E) the meta-method lists itself ──────────────────────
        assert "rpc/methods" in method_set, "rpc/methods must be discoverable through itself"

        # ── (F) discovery loop: learn a method from the catalog,
        #         then actually call it (discover -> use) ──────────
        assert "scene/windows" in method_set, "precondition for the discovery loop"
        # The demo calls scene/windows HAVING confirmed it via the catalog,
        # not from prior literal knowledge — the AI-first discovery path
        # (a global read needing no params).
        learned = "scene/windows"
        got = tf.request(learned, {})
        assert got is not None, f"a discovered method ({learned}) must be callable"
        assert isinstance(got.result, dict), f"{learned} must return a real result"
        assert "windows" in got.result, f"{learned} result must carry its payload; got {got.result!r}"

        # ── (G) negative control: an unlisted name is truly unrouted
        bogus = "rpc/does_not_exist"
        assert bogus not in method_set, "the bogus method must not be in the catalog"
        raised = False
        try:
            tf.request(bogus, {})
        except RpcError as e:
            raised = True
            assert e.code == -32601, (
                f"an unrouted method must be -32601 method-not-found; got {e.code} {e.message!r}"
            )
        assert raised, f"calling {bogus!r} must raise method-not-found, not succeed"

        # ── stability: a second read is identical (catalog is a const) ─
        again = _methods(tf)
        assert again.get("methods") == methods, "the catalog must be stable across reads"
        assert again.get("count") == count, "the count must be stable across reads"


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1089 §5.7 §5.12 §2 #7 — rpc/methods self-describing wire surface",
        body,
    ))
