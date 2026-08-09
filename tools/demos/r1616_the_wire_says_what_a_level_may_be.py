#!/usr/bin/env python3
"""R1616 §5.12 §2 #7 — a key with a closed value set says so, and says which
values.

R1610 gave a window a declared LEVEL and refused a wrong spelling by name
(`UnknownLevel`). What it could not do was tell anyone what a RIGHT spelling
looks like. `rpc/errors` published the refusal word; `rpc/schema` published the
key's type as `string?`; and the only enumeration of the valid values anywhere
was `level_names` on this binding's own external — an application surface,
which another binding need not have and which is no contract at all. An agent
was left to guess a spelling, be refused, and guess again.

That is the same defect R1566 fixed one layer over, for error payloads: knowing
that matching is safe is worthless without knowing what to match. R1610 fixed
that gate and reintroduced the shape one field away.

`WireField.values` is the value half of `WireField.of`: `of` names a censused
TYPE, and only a typed Rust field can carry one — this key is deliberately a
string, because its own parse must run before any other axis in the same
message is applied rather than aborting the whole frame at deserialization.

This demo checks, over the wire and without reading pinion's source:

  * `rpc/schema` publishes `WindowDeclareParams.level` with its `values`.
  * Every published spelling is ACCEPTED by `scene/window_declare`, and lands.
  * A spelling outside the published set is refused, by the matchable word.
  * The binding's own `level_names` agrees with the framework's set — so the
    application surface is now a mirror rather than the only source.
  * The census describes itself: `WireField` publishes its own `values` key.

Run from the workspace root:
    cargo build -p hello-displays --release
    python3 tools/demos/r1616_the_wire_says_what_a_level_may_be.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    call,
    run_demo,
)

PANEL = "panel"


def field_of(schema: dict, type_name: str, field_name: str) -> dict:
    for t in schema["types"]:
        if t["name"] != type_name:
            continue
        for f in t.get("shape", {}).get("fields") or []:
            if f["name"] == field_name:
                return f
        raise AssertionError(f"{type_name} has no field {field_name!r}")
    raise AssertionError(f"rpc/schema does not describe {type_name!r}")


def windows(tf) -> dict:
    return {w["id"]: w for w in call(tf, "scene/windows")["windows"]}


def settle(tf) -> None:
    for _ in range(3):
        tf.tick(0.016)


def run(tf: RpcSubprocess) -> None:
    schema = call(tf, "rpc/schema")

    # ---- 1. the key declares a closed value set ---------------------------
    level = field_of(schema, "WindowDeclareParams", "level")
    assert_eq(level["ty"], "string", "a level is a string on the wire")
    assert_eq(level["optional"], True, "absent leaves the axis alone")
    assert "values" in level, (
        "the key's closed value set is published — without it a client can "
        "only learn a spelling by guessing one and being refused"
    )
    published = level["values"]
    assert isinstance(published, list) and published, "a non-empty list"
    assert_eq(len(published), len(set(published)), "no spelling twice")
    assert_eq(published, ["always_on_bottom", "normal", "always_on_top"])

    # ---- 2. the census describes its own new slot -------------------------
    values_key = field_of(schema, "WireField", "values")
    assert_eq(values_key["ty"], "array", "the slot is a list of spellings")
    assert_eq(values_key["optional"], True, "most keys take any string")
    # ...and `of` is a DIFFERENT slot, still there: one names a type, the
    # other a value set, and a key may have either.
    of_key = field_of(schema, "WireField", "of")
    assert_eq(of_key["ty"], "string", "`of` names a censused type")

    # ---- 3. every published spelling is accepted --------------------------
    for name in published:
        outcome = call(
            tf, "scene/window_declare", {"window_id": PANEL, "level": name}
        )
        assert_eq(
            outcome["applied"],
            ["level"],
            f"published spelling {name!r} is accepted and names the axis it touched",
        )
        settle(tf)
        assert_eq(
            windows(tf)[PANEL]["level"],
            name,
            f"...and {name!r} is what the window now reports",
        )

    # ---- 4. and a spelling outside the set is not -------------------------
    for bad in ["floating", "always_on_topp", "AlwaysOnTop", ""]:
        assert bad not in published, f"{bad!r} is genuinely outside the set"
        assert_rpc_error(
            lambda bad=bad: call(
                tf, "scene/window_declare", {"window_id": PANEL, "level": bad}
            ),
            data="UnknownLevel",
        )

    # ---- 5. the refusal word and the value set are both discoverable ------
    vocab = {w for e in call(tf, "rpc/errors")["errors"] for w in e["data_vocabulary"]}
    assert "UnknownLevel" in vocab, "the word a client matches is published"
    invalid_params = next(e for e in call(tf, "rpc/errors")["errors"] if e["code"] == -32602)
    assert "rpc/schema" in invalid_params["meaning"], (
        "and the entry says where the value set lives, so meeting the word "
        "leads somewhere instead of to pinion's source"
    )

    # ---- 6. the application surface is a mirror now, not the only source --
    app_names = [s for s in str(tf.query("/external/level_names")).split(",") if s]
    assert_eq(
        sorted(app_names),
        sorted(published),
        "this binding's own enumeration agrees with the framework's — the "
        "point being that a binding that publishes none is no longer opaque",
    )

    # ---- 7. the set survives a round trip through the axis ----------------
    # Read what the window reports, feed it straight back, and get the same
    # answer: a published spelling is the SAME string the read side answers
    # with, so a client can echo a value it read without translating it.
    for name in published:
        call(tf, "scene/window_declare", {"window_id": PANEL, "level": name})
        settle(tf)
        reported = windows(tf)[PANEL]["level"]
        call(tf, "scene/window_declare", {"window_id": PANEL, "level": reported})
        settle(tf)
        assert_eq(
            windows(tf)[PANEL]["level"],
            reported,
            "read and write speak one vocabulary",
        )

    print("[demo] the wire says what a level may be, and means it")


def body() -> None:
    with RpcSubprocess("hello-displays", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("r1616 the wire says what a level may be", body)
