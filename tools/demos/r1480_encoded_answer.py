#!/usr/bin/env python3
"""R1480 §5.15 §5.7 §2 #2 §2 #7 — an answer the producer already encoded reaches
the wire as the bytes it wrote, not as a `serde_json::Value` rebuilt from them.

`ExternalIntrospect::query` returns an `IntrospectValue`, and the only variant
able to carry a structured answer was `Json(serde_json::Value)` — a DOM. So a
producer holding a `Serialize` type built a tree of maps and vectors, and the
JSON-RPC envelope immediately walked that tree to write the text it was going
to send anyway. Neither end wanted the tree. It existed because the channel's
type demanded one. R1480 adds `IntrospectValue::Raw` and carries it end to end:
`query` / `invoke` are the two handlers whose answer IS the whole result, so
their payload can be spliced instead of parsed.

WHAT MAKES THIS OBSERVABLE. The claim is a NEGATIVE — that no tree was built —
and an absence cannot be read off the wire. So the demo uses a witness only the
tree can leave: `serde_json::Value`'s object is a `BTreeMap`, so a round trip
through it emits keys SORTED, while serde's derived `Serialize` emits them in
DECLARATION order. `hello-encoded-answer` declares its frame `w, h, rows` and
each row `y, dim, text` — neither sorted — and serves the same document twice:

  - `frame`          via `IntrospectValue::raw`  (the production slot)
  - `frame_via_dom`  via `IntrospectValue::json` (this example's control)

Key order survives `json.loads` (Python dicts keep insertion order), so the
demo reads the witness exactly where a real client would: in the parsed answer.
The two are the same JSON *value* — key order carries no meaning in JSON — which
is what makes the witness safe rather than a contract nobody may rely on.

ZERO-FLAKE: every assertion compares values produced within one run, and the
served frame is generated deterministically from its geometry, so two answers
taken at different moments are comparable with no settling step. Nothing here
waits on wall-clock, pixels, or a timing threshold — the cost this round is
about is REPORTED by the round's changelog from a separate measurement, never
asserted here, because a duration is not a fact a test can own.

Run from the workspace root:
    cargo build -p hello-encoded-answer --release
    python3 tools/demos/r1480_encoded_answer.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    run_demo,
)

EXAMPLE = "hello-encoded-answer"
TAG = "frame_source"
COLS, ROWS = 80, 24

# What each path must emit, at both depths.
DECLARED_FRAME_KEYS = ["w", "h", "rows"]
DECLARED_ROW_KEYS = ["y", "dim", "text"]
SORTED_FRAME_KEYS = sorted(DECLARED_FRAME_KEYS)
SORTED_ROW_KEYS = sorted(DECLARED_ROW_KEYS)


def q(tf: RpcSubprocess, slot: str) -> Any:
    return tf.query(f"{TAG}/external/{slot}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: the witness has teeth on this build ────────────────
        # If declaration order and sorted order ever coincided, every assertion
        # below would pass without distinguishing anything. Assert the premise
        # rather than assume it (R1476: a premise is about the fixture).
        assert DECLARED_FRAME_KEYS != SORTED_FRAME_KEYS, (
            "A: the frame's declared key order must differ from sorted"
        )
        assert DECLARED_ROW_KEYS != SORTED_ROW_KEYS, (
            "A: a row's declared key order must differ from sorted"
        )

        rows = int(q(tf, "rows"))
        assert_eq(rows, ROWS, "★A: the surface serves the frame it advertises")

        # ── (B) the production slot answers in the producer's own encoding ──
        frame = q(tf, "frame")
        assert isinstance(frame, dict), f"B: expected an object, got {type(frame)}"
        assert_eq(
            list(frame.keys()),
            DECLARED_FRAME_KEYS,
            "★B: the raw answer keeps declaration order — no tree was built",
        )
        assert_eq(frame["w"], COLS, "B: width")
        assert_eq(frame["h"], ROWS, "B: height")
        assert_eq(len(frame["rows"]), ROWS, "B: every row arrived")
        assert_eq(
            list(frame["rows"][0].keys()),
            DECLARED_ROW_KEYS,
            "★B: and keeps it at depth — nothing was rebuilt part way down",
        )
        assert_eq(
            list(frame["rows"][ROWS - 1].keys()),
            DECLARED_ROW_KEYS,
            "B: the last row too, so this is not a first-element accident",
        )
        assert_eq(len(frame["rows"][0]["text"]), COLS, "B: the row is full width")
        assert frame["rows"][0]["dim"] is False, "B: row 0 is not dim"
        assert frame["rows"][1]["dim"] is True, "B: row 1 is dim"

        # ── (C) the control answers through the tree, as it always did ──────
        # This is what "a producer that does not use the new variant sees no
        # change" means, checked rather than asserted in prose.
        dom = q(tf, "frame_via_dom")
        assert_eq(
            list(dom.keys()),
            SORTED_FRAME_KEYS,
            "★C: the DOM path still renders sorted keys",
        )
        assert_eq(
            list(dom["rows"][0].keys()),
            SORTED_ROW_KEYS,
            "★C: sorted at depth too",
        )

        # ── (D) …and the two are the same JSON value ────────────────────────
        # The half that makes (B) safe: the wire may pick either encoding
        # because no consumer can tell them apart by anything that MEANS
        # something. Compare as values (dict equality ignores key order) and
        # as canonical text (sorted both sides).
        assert_eq(frame, dom, "★D: one document, two encodings")
        assert_eq(
            json.dumps(frame, sort_keys=True),
            json.dumps(dom, sort_keys=True),
            "D: identical once both are put in the same order",
        )
        assert list(frame.keys()) != list(dom.keys()), (
            "D: …and they really did arrive differently — else (B) proved nothing"
        )

        # ── (E) the answer is the size the producer says it is ──────────────
        # `bytes` is measured by the producer on its own encoding. Checking it
        # against the client's re-encoding of the parsed answer closes the loop
        # from producer to consumer: a payload silently truncated or re-rendered
        # in transit would land here.
        declared = int(q(tf, "bytes"))
        client_side = len(json.dumps(frame, separators=(",", ":")))
        assert_eq(
            client_side,
            declared,
            "★E: what arrived is byte-for-byte the size the producer encoded",
        )
        assert declared > COLS * ROWS, f"E: the payload is substantial ({declared} B)"

        # ── (F) the action channel gets the same treatment ──────────────────
        # `scene/invoke` is the other handler whose answer is the whole result;
        # an action computing a large payload would otherwise pay for a tree.
        # NOTE the path form. `scene/query` resolves `<tag>/external/<field>`
        # and `scene/invoke` does not — measured on this build: the tagged form
        # answers a read and reports `NoExternalAtPath` for an action. That is a
        # read/write path-syntax asymmetry in path resolution, not in encoding,
        # so it is carried out of this round rather than worked around here; the
        # form below is the one `invoke` accepts.
        invoked = tf.invoke("/external/encode", None)
        assert_eq(
            list(invoked.keys()),
            DECLARED_FRAME_KEYS,
            "★F: invoke carries the producer's bytes too",
        )
        assert_eq(
            list(invoked["rows"][0].keys()),
            DECLARED_ROW_KEYS,
            "F: at depth, on the action channel as well",
        )
        assert_eq(invoked, frame, "F: the action answers the same document")

        # ── (G) a raw answer NESTED in a bigger frame materializes ──────────
        # The honest limit, asserted so it cannot quietly change: `scene/
        # snapshot` assembles one big `Value` and there is no splice point part
        # way down a tree, so a raw answer inside it becomes a tree like
        # everything else. What must NOT happen is it arriving as a JSON string
        # or falling through to `null`.
        snap = tf.request("scene/snapshot", {"path": "", "from": "state"})
        assert snap is not None
        nested = find_slot(snap.result, "frame")
        assert isinstance(nested, dict), f"G: expected an object, got {nested!r}"
        assert_eq(
            list(nested.keys()),
            SORTED_FRAME_KEYS,
            "★G: nested in a snapshot, a raw answer materializes into the tree",
        )
        assert_eq(nested, frame, "G: …carrying the same value it does at top level")

        # ── (H) the surface is honest about what it does not serve ──────────
        # Typed data, not just a code: each refusal must be refusing for the
        # reason its line claims.
        assert_rpc_error(lambda: q(tf, "ghost"), data="UnknownIntrospectPath")
        assert_rpc_error(
            lambda: tf.invoke("/external/ghost", None), data="UnknownInvokePath"
        )
        # Read-only: a served frame reports state, it is not a knob on it.
        assert_rpc_error(lambda: tf.intervene("/external/frame", 1), data="ReadOnly")

        # ── (I) reads stay free ─────────────────────────────────────────────
        # `scene/query` is classified Read, so none of the ~10 reads above may
        # have bumped the revision — a read that broadcast a change would wake
        # any client parked on `scene/waitFor` with its own poll, which is the
        # failure mode a frame-polling consumer is most exposed to.
        before = int(q(tf, "rows"))
        for _ in range(5):
            q(tf, "frame")
        assert_eq(int(q(tf, "rows")), before, "★I: polling the frame changes nothing")
        again = q(tf, "frame")
        assert_eq(again, frame, "★I: and answers identically every time")
        assert_eq(
            list(again.keys()),
            DECLARED_FRAME_KEYS,
            "I: the raw path is not a first-call special case",
        )


def find_slot(node: Any, slot: str) -> Any:
    """The value of `introspect.<slot>` on the first node that carries one."""
    if isinstance(node, dict):
        intro = node.get("introspect")
        if isinstance(intro, dict) and slot in intro:
            return intro[slot]
        for child in node.get("children") or []:
            found = find_slot(child, slot)
            if found is not None:
                return found
    return None


if __name__ == "__main__":
    sys.exit(run_demo("R1480 an answer that is already encoded", body))
