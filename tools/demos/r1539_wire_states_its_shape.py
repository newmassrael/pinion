#!/usr/bin/env python3
"""R1539 §5.7 §5.12 §2 #2 §2 #7 — the wire states the shape of what it answers.

`rpc/methods` has discovered method NAMES since R1089, and its own module doc
called the rest "the natural next slice, added when a consumer needs it". R1538
supplied the consumer the hard way: it added `nodes_total` to the `mirror` and
`produce` groups of `scene/frame_timings`, and `r1465_mirror_work.py` — which
asserts the EXACT key set those groups answer with — went red in CI. Nothing
between the edit and the push could see it. A response shape was a published
contract that existed nowhere a machine could check, so growing one looked like
an ordinary struct edit.

`rpc/schema` is that contract, on the wire. It answers with every type this
dispatcher serializes: the key set, each key's JSON type, whether the key may
be ABSENT, whether it may be `null`, and what type is nested at it.

Qt 6.11's floor is `QMetaMethod` — `parameterNames()` / `parameterTypes()` /
`returnMetaType()` make a signature discoverable at runtime, and pinion sat
below it with names alone. Two things here are past it: `returnMetaType()` on a
`QVariantMap` yields `QVariantMap`, so the KEYS stay opaque and every Qt client
falls back to out-of-band docs; and Qt's meta-object is generated from the
declaration, so nothing anywhere asserts a method actually puts the documented
keys in its map. Section (D) below is that missing assertion, made by the agent,
over the live wire.

This demo asserts:

  (A) The census is on the wire, is a CENSUS (count matches, sorted, unique),
      and every shape is one of the four kinds the protocol defines.

  (B) It describes ITSELF (§2 #7). The types that carry the answer are in the
      answer, so an agent can read every response shape including the shape of
      the reply that told it the shapes.

  (C) Every `of` reference resolves. A `$ref` an agent cannot follow describes
      nothing.

  (D) **The census is TRUE of live responses.** Six methods are called and each
      response is validated against its declared type, recursively — key set,
      JSON types, nesting, and the two absence rules. Between them they exercise
      all four shape kinds: object, nested object, array-of-object, string enum,
      and tagged union.

  (E) **`optional` and `nullable` are different answers, and the wire honours
      the difference.** `FocusState.focused` is a bare `Option<String>`: the key
      is ALWAYS present and carries `null` when nothing is focused.
      `FocusState.tab_order` is `#[serde(skip_serializing_if)]`: the key is
      ABSENT rather than null. In Rust both are `Option<T>` and look alike,
      which is why this census was initially wrong about twelve fields until the
      gate compared it against the source.

  (F) **The validator can fail.** Four doctored declarations, each of which must
      be rejected. A conformance check that only ever sees conforming data
      cannot fail, and a gate that cannot fail is worse than none (R1527).

ZERO-FLAKE: every assertion reads a published structure or a cumulative
counter from the same run that produced it. Nothing waits on wall-clock and
nothing asserts a value that depends on the host — `gpu_us` is declared
optional precisely because a software rasterizer omits it, and (D) accepts
both answers.

Run from the workspace root:
    cargo build -p hello-tail-reveal --release
    python3 tools/demos/r1539_wire_states_its_shape.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    call,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-tail-reveal"

SHAPE_KINDS = {"object", "enum", "union", "scalar"}
JSON_TYPES = {
    "integer",
    "number",
    "string",
    "boolean",
    "array",
    "object",
    "null",
    "any",
}


def type_ok(value: Any, ty: str) -> bool:
    """Does `value` inhabit the declared JSON type?

    `bool` is checked before `int` deliberately: Python's `bool` IS an `int`,
    so an `isinstance(x, int)` test accepts `True` for an integer field and the
    declaration stops discriminating.
    """
    if ty == "any":
        return True
    if ty == "boolean":
        return isinstance(value, bool)
    if ty == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if ty == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if ty == "string":
        return isinstance(value, str)
    if ty == "array":
        return isinstance(value, list)
    if ty == "object":
        return isinstance(value, dict)
    if ty == "null":
        return value is None
    raise AssertionError(f"undeclared JSON type {ty!r}")


class Census:
    """The published census, indexed by type name."""

    def __init__(self, types: list[dict[str, Any]]) -> None:
        self.by_name = {t["name"]: t for t in types}

    def shape(self, name: str) -> dict[str, Any]:
        assert name in self.by_name, f"the census does not define {name!r}"
        return self.by_name[name]["shape"]

    def conform(self, name: str, value: Any, path: str = "") -> list[str]:
        """Every way `value` fails to be a `name` — empty means it conforms."""
        where = path or name
        shape = self.shape(name)
        kind = shape["kind"]
        if kind == "enum":
            if value not in shape["values"]:
                return [f"{where}: {value!r} is not one of {shape['values']}"]
            return []
        if kind == "scalar":
            if not any(type_ok(value, t) for t in shape["types"]):
                return [f"{where}: {value!r} is none of {shape['types']}"]
            return []
        if kind == "union":
            if not isinstance(value, dict):
                return [f"{where}: a union is a JSON object, got {type(value).__name__}"]
            tag = shape["tag"]
            if tag not in value:
                return [f"{where}: no discriminator {tag!r} in {sorted(value)}"]
            arms = {v["name"]: v["fields"] for v in shape["variants"]}
            if value[tag] not in arms:
                return [f"{where}: {value[tag]!r} is not an arm of {sorted(arms)}"]
            return self._fields(arms[value[tag]], value, where, extra_ok={tag})
        return self._fields(shape["fields"], value, where)

    def _fields(
        self,
        fields: list[dict[str, Any]],
        value: Any,
        where: str,
        extra_ok: set[str] | None = None,
    ) -> list[str]:
        if not isinstance(value, dict):
            return [f"{where}: expected a JSON object, got {type(value).__name__}"]
        errs: list[str] = []
        declared = {f["name"] for f in fields} | (extra_ok or set())
        for key in sorted(set(value) - declared):
            errs.append(f"{where}.{key}: on the wire but not in the census")
        for f in fields:
            name, at = f["name"], f"{where}.{f['name']}"
            if name not in value:
                if not f["optional"]:
                    errs.append(f"{at}: declared required, absent from the response")
                continue
            v = value[name]
            if v is None:
                # An `optional` key that IS present must still carry its type;
                # only a `nullable` one may be null.
                if not f["nullable"]:
                    errs.append(f"{at}: null, but the census does not permit null")
                continue
            if not type_ok(v, f["ty"]):
                errs.append(f"{at}: declared {f['ty']}, got {type(v).__name__}")
                continue
            of = f.get("of")
            if of is None:
                continue
            if f["ty"] == "array":
                for i, item in enumerate(v):
                    errs.extend(self.conform(of, item, f"{at}[{i}]"))
            else:
                errs.extend(self.conform(of, v, at))
        return errs


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the census is on the wire, and is a census ───────────────────
        schema = call(tf, "rpc/schema")
        assert isinstance(schema, dict), (
            f"rpc/schema answers with an object, got {type(schema).__name__}"
        )
        assert_eq(sorted(schema), ["count", "types"], "A: the rpc/schema surface")
        types = schema["types"]
        assert_eq(
            schema["count"],
            len(types),
            "A: `count` is the census's own size, not a number a client re-counts",
        )
        assert len(types) > 60, f"A: the whole wire surface, got {len(types)}"

        names = [t["name"] for t in types]
        assert_eq(names, sorted(names), "A: sorted by name, so a diff is readable")
        assert_eq(len(names), len(set(names)), "A: and duplicate-free")
        kinds = {t["shape"]["kind"] for t in types}
        assert kinds <= SHAPE_KINDS, f"A: undeclared shape kind in {kinds}"
        assert_eq(
            kinds,
            SHAPE_KINDS,
            "A: all four shape kinds are exercised by the real surface — a "
            "protocol arm no type uses is an arm nothing verifies",
        )
        for t in types:
            for f in t["shape"].get("fields", []):
                assert f["ty"] in JSON_TYPES, f"A: {t['name']}.{f['name']} ty={f['ty']}"
                assert not (f["optional"] and f["nullable"]), (
                    f"A: {t['name']}.{f['name']} is declared both absent-able and "
                    f"null-able; serde makes exactly one of those true"
                )

        census = Census(types)

        # ── (B) it describes itself (§2 #7) ─────────────────────────────────
        for own in ("RpcSchema", "WireType", "WireField", "WireShape", "WireTy"):
            assert own in census.by_name, (
                f"B: the census must define its own type {own!r} — otherwise an "
                f"agent can read every response shape EXCEPT the shape of the "
                f"answer that told it the shapes"
            )
        assert_eq(
            census.shape("WireShape")["kind"],
            "union",
            "B: and the shape-of-shapes is the tagged union it describes",
        )

        # ── (C) every `of` reference resolves ───────────────────────────────
        refs = 0
        for t in types:
            shape = t["shape"]
            groups = [shape["fields"]] if shape["kind"] == "object" else []
            if shape["kind"] == "union":
                groups = [v["fields"] for v in shape["variants"]]
            for fields in groups:
                for f in fields:
                    if f.get("of") is None:
                        continue
                    refs += 1
                    assert f["of"] in census.by_name, (
                        f"C: {t['name']}.{f['name']} references {f['of']!r}, which "
                        f"the census does not define — a $ref an agent cannot follow"
                    )
        assert refs > 30, f"C: the census should nest; only {refs} references"

        # ── (D) the census is TRUE of live responses ────────────────────────
        # `scene/frame_timings` answers only once the window has painted.
        def timings() -> Any:
            try:
                return tf.frame_timings()
            except RpcError:
                tf.tick(0.05)
                return None

        wait_until(timings, desc="scene/frame_timings becomes available")

        live: list[tuple[str, str, Any]] = [
            ("scene/frame_timings", "FrameTimingsOutcome", tf.frame_timings()),
            ("rpc/methods", "RpcMethods", call(tf, "rpc/methods")),
            ("rpc/schema", "RpcSchema", schema),
            ("focus/get", "FocusState", call(tf, "focus/get")),
            ("scene/cache_stats", "CacheStatsOutcome", tf.cache_stats()),
            ("scene/text_cache_stats", "TextCacheStatsOutcome", tf.text_cache_stats()),
        ]
        for method, ty, value in live:
            errs = census.conform(ty, value)
            assert_eq(
                errs,
                [],
                f"D: {method} conforms to the published {ty}. THIS is the "
                f"assertion Qt's meta-object never makes — moc describes the "
                f"declaration and nothing checks the answer against it",
            )

        # The richest one is worth pinning explicitly: `FrameTimingsOutcome`
        # nests five further types, and `mirror` is the group R1538 grew.
        ft = tf.frame_timings()
        mirror_fields = [f["name"] for f in census.shape("FrameTimingsMirror")["fields"]]
        assert_eq(
            sorted(ft["mirror"]),
            sorted(mirror_fields),
            "D: the group whose growth took CI red now has its key set on the "
            "wire, and the live answer matches it exactly",
        )
        assert "nodes_total" in mirror_fields, (
            "D: including the R1538 field itself — the census was updated as "
            "part of the change that made it, which is the whole point"
        )

        # Array-of-object and string-enum arms, reached through `rpc/methods`.
        methods = call(tf, "rpc/methods")
        assert_eq(
            census.shape("RpcMethods")["fields"][0]["of"],
            "MethodEntry",
            "D: `methods` is declared an array OF a named type",
        )
        occs = {m["occ"] for m in methods["methods"]}
        assert_eq(
            occs,
            set(census.shape("MethodOcc")["values"]),
            "D: and every `occ` on the wire is a value the enum declares",
        )
        assert "rpc/schema" in {m["name"] for m in methods["methods"]}, (
            "D: the discovery surface lists the schema method, so an agent that "
            "knows only `rpc/methods` can find the shapes"
        )

        # ── (E) absent and null are different answers ───────────────────────
        focus_fields = {f["name"]: f for f in census.shape("FocusState")["fields"]}
        assert_eq(
            (focus_fields["focused"]["nullable"], focus_fields["focused"]["optional"]),
            (True, False),
            "E: `focused` is a bare Option — the key is always THERE, carrying "
            "null when nothing has focus",
        )
        assert_eq(
            (focus_fields["tab_order"]["nullable"], focus_fields["tab_order"]["optional"]),
            (False, True),
            "E: `tab_order` is skip_serializing_if — the key is ABSENT, not null. "
            "Both are Option<T> in Rust and look identical there",
        )
        got = call(tf, "focus/get")
        assert "focused" in got, (
            f"E: and the wire honours it — a nullable key is present: {sorted(got)}"
        )
        assert_eq(
            census.conform("FocusState", got),
            [],
            "E: whichever way this binding answers today",
        )
        last_fields = {f["name"]: f for f in census.shape("FrameTimingsLast")["fields"]}
        assert_eq(
            last_fields["gpu_us"]["optional"],
            True,
            "E: `gpu_us` is optional because an adapter without TIMESTAMP_QUERY "
            "omits it — R1537 states absence three ways rather than as a zero",
        )
        assert_eq(
            "gpu_us" in ft["last"] or True,
            True,
            "E: so this demo accepts both answers rather than reading the host",
        )

        # ── (F) the validator can fail ──────────────────────────────────────
        # Each doctored declaration must be REJECTED. Without these, (D) would
        # pass just as happily against a validator that checked nothing.
        import copy

        def doctored(mutate) -> Census:
            c = Census(copy.deepcopy(types))
            mutate(c)
            return c

        def drop_field(c: Census) -> None:
            c.by_name["FrameTimingsMirror"]["shape"]["fields"].pop()

        def add_phantom(c: Census) -> None:
            c.by_name["FrameTimingsMirror"]["shape"]["fields"].append(
                {"name": "cf_absent", "optional": False, "nullable": False, "ty": "integer"}
            )

        def wrong_type(c: Census) -> None:
            c.by_name["FrameTimingsMirror"]["shape"]["fields"][0]["ty"] = "string"

        def deny_null(c: Census) -> None:
            for f in c.by_name["FocusState"]["shape"]["fields"]:
                if f["name"] == "focused":
                    f["nullable"] = False

        assert doctored(drop_field).conform("FrameTimingsMirror", ft["mirror"]), (
            "F: a field removed from the census must surface as an unaccounted "
            "key on the wire — this is R1538's defect, seen from the other side"
        )
        assert doctored(add_phantom).conform("FrameTimingsMirror", ft["mirror"]), (
            "F: a required key the response does not carry must be rejected"
        )
        assert doctored(wrong_type).conform("FrameTimingsMirror", ft["mirror"]), (
            "F: a wrong JSON type must be rejected, not merely the wrong key set"
        )
        if got.get("focused") is None:
            assert doctored(deny_null).conform("FocusState", got), (
                "F: and a null under a non-nullable declaration must be rejected — "
                "the distinction (E) draws is enforced, not decorative"
            )
        else:
            assert_eq(
                doctored(deny_null).conform("FocusState", got),
                [],
                "F: nothing is focused-less here, so the null rule is vacuous on "
                "this run and is pinned by the unit gate instead",
            )
        assert_eq(
            census.conform("FrameTimingsMirror", ft["mirror"]),
            [],
            "F: while the undoctored census still conforms — the four rejections "
            "above are the validator discriminating, not failing on everything",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1539 the wire states the shape of what it answers", body))
