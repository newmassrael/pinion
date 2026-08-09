#!/usr/bin/env python3
"""R1575 §5.3 §2 #7 — a graph states which of its links it AUTHORED and which it OBSERVED.

`hello-graph-diff` draws one graph in two layers. Five links were drawn by a
user; an observation reports five links that are not the same five. The
difference is not stored anywhere — `LinkKind` is a function of which of the
two sets a link is in — and the difference is what the picture is *for*.

Until this round pinion could not draw that picture at all: `Stroke` carried
colour, width and cap, and `StrokeCap`'s own doc said dash patterns were
carry-forward. So "solid = drawn and confirmed, dashed = drawn and absent,
dotted = present and undrawn" was inexpressible, and the R1575 `Dash` is the
primitive this binding forces.

What this script checks, and why each check discriminates:

* **The kind really is derived.** The same binding is driven onto a second
  observation (`converged`) that happens to equal the authored set, and the
  difference goes to zero without anything having edited a link. A demo that
  loaded only one observation would assert the derivation's *output* and could
  not tell it from a hard-coded answer.
* **The paint agrees with the model, read independently.** `query missing_ids`
  is the model's answer; `scene/snapshot` is the painter's. The script reads
  BOTH and requires that exactly the links the model calls missing are the
  paths carrying the dashed rhythm — two readings of one fact, which is the
  only way a drawing is checkable without pixels.
* **PAST the toolkit 6.11 (1): the dash is readable at all.** `style.stroke.dash` is
  `null` for a solid stroke and `{on, off, offset, period}` otherwise. A pen
  is an argument to a paint call and lives nowhere afterwards: nothing can ask
  a canvas scene which of its edges are dashed, so the same question in the toolkit
  is answerable only by rasterizing and looking.
* **PAST the toolkit 6.11 (2): the two unmatched kinds are told apart as DATA.** Missing
  and drift differ in rhythm AND in ink, and the script asserts the rhythms are
  different values rather than merely both non-null.
* **PAST the toolkit 6.11 (3): the animation is drivable and canonical.** `flow` is
  stepped over the wire, read back off the model AND off every dashed path's
  `offset`, and driven one full period to land back where it started. The toolkit's
  `dashOffset` is a `qreal` on a pen driven by a timer that no external
  client can address, and the toolkit keeps whatever number it was handed — 12 and 2
  over a period of 10 are different values there and one value here.
* **The refusals name what they refused.** An unknown observation, a malformed
  link argument, a well-formed argument naming a link in neither layer, and a
  write to a derived path each answer with a distinct, specific error rather
  than a bare failure.
* **The whole picture reaches assistive technology.** The accessible value
  names the missing and drifted links BY NAME — the reading a sighted user gets
  from solid-versus-dashed. A toolkit pen's dash reaches the screen and nothing
  else, so the same distinction there is invisible to a screen reader by
  construction.

Run from the workspace root:
    cargo build -p hello-graph-diff --release
    python3 tools/demos/r1575_graph_states_its_layers.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    run_demo,
    walk_nodes,
)

VIEWPORT = (720, 460)
VIEW_TAG = "graph_diff"
LINK_PREFIX = "link."

#: The fixture, mirrored rather than imported — a demo that read the expected
#: answer out of the code under test could not catch that code changing.
AUTHORED = {"peer-a>hub", "peer-b>hub", "leaf-1>peer-a", "leaf-2>peer-a", "leaf-3>peer-b"}
OBSERVED_PARTIAL = {"peer-a>hub", "peer-b>hub", "leaf-1>peer-a", "leaf-2>peer-a", "leaf-2>hub"}

MISSING = sorted(AUTHORED - OBSERVED_PARTIAL)
DRIFT = sorted(OBSERVED_PARTIAL - AUTHORED)
MATCHED = sorted(AUTHORED & OBSERVED_PARTIAL)


def q(tf: RpcSubprocess, path: str):
    """One `query` against the primary External."""
    return tf.query(f"/external/{path}")


def ids(raw: str) -> list[str]:
    return [s for s in str(raw).split(",") if s]


def link_paths(tf: RpcSubprocess) -> dict[str, dict]:
    """Every painted link path, keyed by `"<from>><to>"`.

    The tag is `link.<from>-<to>`; the id the model publishes is
    `<from>><to>`. Two spellings of one pair is exactly the drift this
    function exists to make visible, so it converts rather than assuming.
    """
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    assert snap, "paint snapshot returned no result"
    out: dict[str, dict] = {}
    for _, node in walk_nodes(snap):
        tag = str(node.get("tag") or "")
        if not tag.startswith(LINK_PREFIX):
            continue
        pair = tag[len(LINK_PREFIX) :]
        from_, _, to = pair.partition("-")
        # Node names contain '-' themselves (`peer-a`), so split on the LAST
        # boundary that yields two declared node names.
        for cut in range(len(pair) - 1, 0, -1):
            if pair[cut] != "-":
                continue
            a, b = pair[:cut], pair[cut + 1 :]
            if a in NODE_NAMES and b in NODE_NAMES:
                from_, to = a, b
                break
        out[f"{from_}>{to}"] = node
    return out


NODE_NAMES: set[str] = set()


def dash_of(node: dict) -> dict | None:
    style = node.get("style") or {}
    stroke = style.get("stroke")
    assert stroke is not None, f"a link path strokes: {node.get('tag')}"
    return stroke.get("dash")


def run(tf: RpcSubprocess) -> None:
    # ---- 0. premise: the binding is the fixture this script describes ----
    NODE_NAMES.update(ids(q(tf, "node_names")))
    assert_eq(int(q(tf, "node_count")), 6, "declared nodes")
    assert_eq(sorted(ids(q(tf, "authored_ids"))), sorted(AUTHORED), "the authored layer")
    assert_eq(sorted(ids(q(tf, "observed_ids"))), sorted(OBSERVED_PARTIAL), "the observed layer")
    assert_eq(q(tf, "scenario"), "partial", "the observation loaded at boot")

    # ---- 1. the difference, derived ----
    assert_eq(int(q(tf, "matched")), len(MATCHED), "matched count")
    assert_eq(int(q(tf, "missing")), len(MISSING), "missing count")
    assert_eq(int(q(tf, "drift")), len(DRIFT), "drift count")
    assert_eq(ids(q(tf, "missing_ids")), MISSING, "WHICH links are missing")
    assert_eq(ids(q(tf, "drift_ids")), DRIFT, "WHICH links drifted")
    assert_eq(
        int(q(tf, "link_count")),
        len(AUTHORED | OBSERVED_PARTIAL),
        "the union is what is drawn — a link in either layer is on screen",
    )
    print(
        f"[demo] two layers: {len(MATCHED)} matched, "
        f"{len(MISSING)} missing {MISSING}, {len(DRIFT)} drift {DRIFT}"
    )

    # The per-link read, by name, including both refusal shapes.
    for link in MISSING:
        a, b = link.split(">")
        assert_eq(tf.invoke("/external/link_kind", f"{a},{b}"), "missing", f"{link} kind")
    for link in DRIFT:
        a, b = link.split(">")
        assert_eq(tf.invoke("/external/link_kind", f"{a},{b}"), "drift", f"{link} kind")

    # ---- 2. PAST the toolkit: the paint publishes its dash, and it agrees
    # ----
    painted = link_paths(tf)
    assert_eq(
        sorted(painted), sorted(AUTHORED | OBSERVED_PARTIAL), "every link in either layer is drawn"
    )
    solid = sorted(k for k, n in painted.items() if dash_of(n) is None)
    dashed = {k: dash_of(n) for k, n in painted.items() if dash_of(n) is not None}
    assert_eq(solid, MATCHED, "the matched links are the ones drawn SOLID")
    assert_eq(sorted(dashed), sorted(MISSING + DRIFT), "and the rest carry a dash")

    missing_rhythm = (dashed[MISSING[0]]["on"], dashed[MISSING[0]]["off"])
    drift_rhythm = (dashed[DRIFT[0]]["on"], dashed[DRIFT[0]]["off"])
    assert missing_rhythm != drift_rhythm, (
        "missing and drift must be distinguishable from EACH OTHER, not only "
        f"from matched — both read {missing_rhythm}"
    )
    for link, dash in dashed.items():
        assert_eq(
            dash["period"],
            dash["on"] + dash["off"],
            f"{link}'s period is derived rather than a second number",
        )
    print(
        f"[demo] paint agrees with the model: dashed={sorted(dashed)} "
        f"missing rhythm {missing_rhythm} vs drift {drift_rhythm}"
    )

    # ---- 3. PAST the toolkit: the animation is data, and it is canonical ----
    period = int(q(tf, "flow_period"))
    assert_eq(int(q(tf, "flow")), 0, "the flow starts unshifted")
    tf.intervene("/external/flow", 3)
    assert_eq(int(q(tf, "flow")), 3, "a written offset reads back")
    tf.tick(0.016)
    painted_offsets = sorted(
        {
            dash["offset"]
            for node in link_paths(tf).values()
            if (dash := dash_of(node)) is not None
        }
    )
    assert_eq(
        painted_offsets,
        [3],
        "and it reached every dashed path in the PAINT — the model's number and "
        "the painter's are read separately and must agree",
    )
    # A full period is the identity — the property that makes the animation a
    # finite cycle rather than an ever-growing number.
    tf.intervene("/external/flow", period)
    assert_eq(int(q(tf, "flow")), 0, f"a write of one full period ({period}px) canonicalises to 0")
    stepped = int(tf.invoke("/external/advance_flow", str(period - 1)))
    assert_eq(stepped, period - 1, "advance_flow answers with where it landed")
    assert_eq(int(tf.invoke("/external/advance_flow", "1")), 0, "and one more closes the cycle")
    print(f"[demo] flow is drivable and canonical over its {period}px period")

    # ---- 4. the derivation MOVES: a second observation ----
    tf.intervene("/external/scenario", "converged")
    tf.tick(0.016)
    assert_eq(q(tf, "scenario"), "converged", "the observation swapped")
    assert_eq(int(q(tf, "missing")), 0, "nothing authored is absent under converged")
    assert_eq(int(q(tf, "drift")), 0, "and nothing present is undrawn")
    assert_eq(int(q(tf, "matched")), len(AUTHORED), "every authored link is confirmed")
    after = link_paths(tf)
    assert_eq(
        [k for k, n in after.items() if dash_of(n) is not None],
        [],
        "so NOTHING is drawn dashed — the picture followed the model with no "
        "code having edited a link",
    )
    print("[demo] the difference is derived: a second observation empties it")

    # ---- 5. the picture reaches assistive technology ----
    # Run BEFORE `adopt`, while the difference still exists: an a11y assertion
    # over an empty difference passes on a binding that says nothing.
    tf.intervene("/external/scenario", "partial")
    tf.tick(0.016)
    acc = tf.request("scene/access", {}).result or {}
    node = access_node_by_tag(acc, VIEW_TAG)
    assert node is not None, "the view is in the accessibility tree"
    value = str(node.get("value") or "")
    for link in MISSING:
        assert link in value, (
            "PAST QT: the accessible value names the MISSING link by name — the "
            "reading a sighted user gets from the dash. A QPen's dash reaches "
            f"the screen and nothing else: {value!r}"
        )
    for link in DRIFT:
        assert link in value, f"and the drifted one too: {value!r}"
    assert "matched" in value, f"and says which are confirmed: {value!r}"
    print(f"[demo] a11y names the difference: ...{value[value.index('missing'):][:64]}...")

    # ---- 6. every refusal names what it refused ----
    def refused(call, what: str) -> str:
        try:
            call()
        except Exception as exc:  # noqa: BLE001 — the message IS the subject
            return str(exc)
        raise AssertionError(f"{what} was expected to be refused and was not")

    bad_scenario = refused(
        lambda: tf.intervene("/external/scenario", "nope"), "an unknown observation"
    )
    assert "partial" in bad_scenario and "converged" in bad_scenario, (
        "an unknown observation is refused by NAMING the known ones, not with a "
        f"bare rejection: {bad_scenario!r}"
    )
    malformed = refused(lambda: tf.invoke("/external/link_kind", "leaf-3"), "a malformed pair")
    assert "from" in malformed or "expected" in malformed, (
        f"a malformed pair says what shape was expected: {malformed!r}"
    )
    absent = refused(
        lambda: tf.invoke("/external/link_kind", "hub,leaf-1"), "a link in neither layer"
    )
    assert "either layer" in absent, (
        "a WELL-FORMED pair naming a link in neither layer is a different fact "
        f"from a malformed one, and says so: {absent!r}"
    )
    read_only = refused(lambda: tf.intervene("/external/missing", 1), "a write to a derived path")
    assert "read" in read_only.lower() or "ReadOnly" in read_only, (
        f"a derived path refuses a write as read-only: {read_only!r}"
    )
    print("[demo] four refusals, four different sentences")

    # ---- 7. the verb, and what it answers. LAST, because it is the one call
    # that rewrites the fixture: adopting makes the authored layer say what was
    # observed, and every later reading would then be of a converged graph.
    resolved = int(tf.invoke("/external/adopt", None))
    assert_eq(
        resolved,
        len(MISSING) + len(DRIFT),
        "adopt answers with how many differences it resolved, so a caller need "
        "not re-read three paths to find out",
    )
    assert_eq(int(q(tf, "missing")) + int(q(tf, "drift")), 0, "and there are none left")
    assert_eq(
        sorted(ids(q(tf, "authored_ids"))),
        sorted(OBSERVED_PARTIAL),
        "adopting made the authored layer say what was observed",
    )
    tf.tick(0.016)
    assert_eq(
        [k for k, n in link_paths(tf).items() if dash_of(n) is not None],
        [],
        "and the paint followed one assignment: nothing is dashed any more",
    )
    print(f"[demo] adopt resolved {resolved} differences in one assignment")


def body() -> None:
    with RpcSubprocess("hello-graph-diff", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("r1575 a graph states its layers", body)
