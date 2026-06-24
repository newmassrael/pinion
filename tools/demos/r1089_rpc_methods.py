#!/usr/bin/env python3
"""R1089/R1090 §5.7 §5.12 §2 #7 — `rpc/methods` self-describing wire surface.

Every other dispatch method is reachable only by an AI that already knows
its literal string. `rpc/methods` is the meta-method that returns the
catalog of methods the dispatcher routes, so an agent DISCOVERS the surface
(`scene/window_move`, `scene/windows`, …) instead of needing each literal
baked in — the §2 #7 scene-as-data principle applied to the protocol
itself. The catalog is verified-complete against the actual routing match by
a Rust cross-check test (`catalog_matches_dispatch_match_arms`).

R1090 adds a per-method `occ` ("read" | "mutate") = the SceneRevision
optimistic-concurrency class (mirrors each arm's HandlerKind), cross-checked
against the source. **`occ` is NOT a side-effect flag**: `read` only means
the dispatcher does not bump the OCC token — deferred-input / async-effect /
focus / self-bumping methods are `read` yet DO change state. This demo
asserts that honest semantics directly.

Section roadmap (>=30 assertions across A-H):

  (A) `rpc/methods` boots — `{methods: [{name, occ}], count}`, well-shaped.
  (B) Entry shape — every entry is `{name: ns/method, occ: read|mutate}`;
      names sorted + unique; `count == len`.
  (C) Known methods present (R1087/R1088 additions + core ones).
  (D) Namespaces — focus / font / rpc / scene / text all represented.
  (E) Self-reference — `rpc/methods` lists itself, occ read.
  (F) occ honesty — effecting methods (`scene/window_move`, `scene/key`,
      `focus/set`) are `read` (no OCC bump) despite changing state; a
      synchronous mutate (`scene/set_text`) is `mutate`.
  (G) Discovery loop — call a method learned ONLY from the catalog.
  (H) Negative control — an unlisted name is genuinely unrouted (-32601).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402

_OCC = {"read", "mutate"}


def _methods(tf: RpcSubprocess) -> dict:
    resp = tf.request("rpc/methods", {})
    assert resp is not None, "rpc/methods returned no response"
    result = resp.result
    assert isinstance(result, dict), f"rpc/methods result must be an object; got {result!r}"
    return result


def _occ_of(methods: list[dict], name: str) -> str:
    for m in methods:
        if m.get("name") == name:
            return m.get("occ")
    raise AssertionError(f"catalog must list {name!r}")


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

        # ── (B) entry shape: {name: ns/method, occ: read|mutate} ──
        names: list[str] = []
        for m in methods:
            assert isinstance(m, dict), f"each entry must be an object; got {m!r}"
            name = m.get("name")
            occ = m.get("occ")
            assert isinstance(name, str) and name, f"entry name must be a non-empty string; got {m!r}"
            assert name.count("/") == 1, f"name must be one 'ns/method'; got {name!r}"
            assert name == name.lower(), f"names are lowercase; got {name!r}"
            assert occ in _OCC, f"occ must be read|mutate; got {occ!r} for {name!r}"
            names.append(name)
        assert names == sorted(names), "the catalog must be sorted by name"
        assert len(set(names)) == len(names), "names must be duplicate-free"

        name_set = set(names)

        # ── (C) known methods are discoverable ────────────────────
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
            assert expected in name_set, f"catalog must list {expected!r}; missing"

        # ── (D) every namespace is represented ────────────────────
        namespaces = {n.split("/")[0] for n in names}
        for ns in ("focus", "font", "rpc", "scene", "text"):
            assert ns in namespaces, f"namespace {ns!r} must appear in the catalog"
        scene_count = sum(1 for n in names if n.startswith("scene/"))
        assert scene_count >= 50, f"scene/* should dominate the surface; got {scene_count}"

        # ── (E) the meta-method lists itself, occ read ────────────
        assert "rpc/methods" in name_set, "rpc/methods must be discoverable through itself"
        assert _occ_of(methods, "rpc/methods") == "read", "rpc/methods is a read meta-method"

        # ── (F) occ honesty: read != side-effect-free ─────────────
        # These DO change state but are occ:read (deferred / async / out of
        # OCC) — a consumer must not read "read" as "free of effects".
        for effecting_read in ("scene/window_move", "scene/key", "scene/wheel", "focus/set"):
            assert _occ_of(methods, effecting_read) == "read", (
                f"{effecting_read} is occ:read (no OCC bump) despite changing state"
            )
        # A synchronous, revision-bumping mutate, for contrast.
        assert _occ_of(methods, "scene/set_text") == "mutate", "scene/set_text bumps OCC = mutate"
        assert _occ_of(methods, "scene/invoke") == "mutate", "scene/invoke bumps OCC = mutate"
        # Both classes are populated (not a degenerate all-read catalog).
        occs = {m["occ"] for m in methods}
        assert occs == _OCC, f"both read and mutate must appear; got {occs!r}"
        mutate_count = sum(1 for m in methods if m["occ"] == "mutate")
        assert 5 <= mutate_count < count, f"mutate count should be a real subset; got {mutate_count}"

        # ── (G) discovery loop: learn a method, then call it ──────
        assert "scene/windows" in name_set, "precondition for the discovery loop"
        learned = "scene/windows"  # learned from the catalog, not prior literal knowledge
        got = tf.request(learned, {})
        assert got is not None, f"a discovered method ({learned}) must be callable"
        assert isinstance(got.result, dict) and "windows" in got.result, (
            f"{learned} must return its payload; got {got.result!r}"
        )

        # ── (H) negative control: an unlisted name is unrouted ────
        bogus = "rpc/does_not_exist"
        assert bogus not in name_set, "the bogus method must not be in the catalog"
        raised = False
        try:
            tf.request(bogus, {})
        except RpcError as e:
            raised = True
            assert e.code == -32601, (
                f"an unrouted method must be -32601 method-not-found; got {e.code} {e.message!r}"
            )
        assert raised, f"calling {bogus!r} must raise method-not-found, not succeed"

        # ── stability: a second read is identical (catalog is const) ─
        again = _methods(tf)
        assert again.get("methods") == methods, "the catalog must be stable across reads"
        assert again.get("count") == count, "the count must be stable across reads"


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1089/R1090 §5.7 §5.12 §2 #7 — rpc/methods self-describing wire surface (names + occ)",
        body,
    ))
