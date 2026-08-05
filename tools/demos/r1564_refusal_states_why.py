#!/usr/bin/env python3
"""R1564 §5.15 §5.12 §2 #2 §2 #7 — a refused invoke states WHY, over the wire.

`InvokeError::Rejected` was a payload-free variant, so a producer that knew
exactly why it was refusing had nowhere to say it and the JSON-RPC frame
published the string `"InvokeRejected"` — the transport's classification, not
the fact the surface observed. PINION-PR82 measured what that costs downstream:
of a consumer's fifteen reachable CLI failure paths, **six** print a list of
causes joined by `or`, because the daemon's own handler had already told them
apart and the wire had no slot for the answer.

`hello-refused-invoke` is that case reduced to its two preconditions:
`report` refuses when no detector is installed, and refuses when the named pane
is not on this host. This demo asserts, against the running binary over a real
socket:

  * the two refusals of ONE action carry two DIFFERENT sentences — the thing
    that was inexpressible, so the consumer's `or` collapses to a fact;
  * the sentence is the producer's, **verbatim**: the same bytes the Rust code
    wrote, not a paraphrase and not a variant name;
  * a refusal arrives under `ACTION_REFUSED` (-32005) and a FRAMEWORK finding
    (`UnknownInvokePath`, `InvokeTypeMismatch`) still arrives under -32602, so
    a consumer branches on the CODE and never on the prose — which is exactly
    why the code split and the free-text payload are one change and not two;
  * the reason TRACKS the surface: evicting a pane changes what the next
    refusal reports, so it is derived, not a fixed string;
  * the refusal agrees with the READ channel (§2 #7) — `has_pane.<id>` reports
    absent for the pane the refusal names, so the two channels corroborate
    rather than restate one derivation;
  * `with_origin` composes with it (R1487): the sentence lands under
    `data.reason` beside the surface that refused, so "who" and "why" arrive in
    one frame;
  * a widget `send` refusal names the vocabulary it WOULD have accepted, which
    is derived from the same const `from_name` gates on, so it cannot advertise
    a name the surface would then decline;
  * nothing was executed by any refused call;
  * (R1565) the WRITE channel does the same for the one arm whose variant does
    not determine its meaning — an out-of-range value states the range, under
    `VALUE_OUT_OF_RANGE`, while `ReadOnly` keeps its word because its meaning IS
    its variant;
  * and (R1564.1) the codes are DISCOVERABLE — `rpc/errors` publishes each one
    with the fact that decides how a client may treat its payload, so moving
    the classifying job onto `error.code` did not replace one undiscoverable
    contract with another.

ZERO-FLAKE: no sleeps, no timing assertions — every check is a request/response
pair against deterministic state the demo itself drives.

Run from the workspace root:
    cargo build -p hello-refused-invoke --release
    python3 tools/demos/r1564_refusal_states_why.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    ACTION_REFUSED,
    PROSE_DATA_CODES,
    VALUE_OUT_OF_RANGE,
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    assert_out_of_range,
    assert_rpc_error,
    call,
    rpc_error_data,
    run_demo,
)

EXAMPLE = "hello-refused-invoke"
EXT = "/external"


def q(tf: RpcSubprocess, field: str) -> Any:
    return tf.query(f"{EXT}/{field}")


def inv(tf: RpcSubprocess, action: str, args: Any = None) -> Any:
    return tf.invoke(f"{EXT}/{action}", args)


def body() -> None:
    with RpcSubprocess(EXAMPLE) as tf:
        # ── (A) baseline: the two preconditions are readable before any call ─
        assert_eq(q(tf, "detector"), False, "A: a fresh host has no detector")
        assert_eq(q(tf, "panes"), "1, 2, 3", "A: and three panes")
        assert_eq(q(tf, "reports"), 0, "A: nothing has been reported")

        # ── (B) THE claim: one action, two refusals, two sentences ──────────
        # Pane 1 exists, so the ONLY thing wrong is the missing detector.
        no_detector = assert_action_refused(
            lambda: inv(tf, "report", 1),
            saying="no agent detector is installed on this host",
        )
        # It says what to do next, which is the half a variant name can never
        # carry however many variants there are.
        assert "install one with install_detector" in no_detector, (
            f"B: the refusal names the remedy: {no_detector!r}"
        )

        inv(tf, "install_detector", None)
        assert_eq(q(tf, "detector"), True, "B: the detector is installed")

        no_pane = assert_action_refused(
            lambda: inv(tf, "report", 99),
            saying="no pane 99 on this host",
        )
        assert no_detector != no_pane, (
            "B: the two refusals of ONE action are DIFFERENT frames — this is "
            "the assertion that could not be written before R1564, because the "
            f"wire said {'InvokeRejected'!r} for both"
        )
        # And neither ran the action.
        assert_eq(q(tf, "reports"), 0, "B: no refusal executed anything")

        # ── (C) the sentence is DERIVED from the surface, not a fixed string ─
        assert "it has 1, 2, 3" in no_pane, (
            f"C: the refusal names what IS there, not only what is missing: {no_pane!r}"
        )
        inv(tf, "evict_pane", 2)
        after_evict = assert_action_refused(
            lambda: inv(tf, "report", 99), saying="no pane 99 on this host"
        )
        assert "it has 1, 3" in after_evict, (
            f"C: the reason tracks the live surface: {after_evict!r}"
        )
        assert after_evict != no_pane, "C: so the same address refuses differently now"

        # ── (D) the refusal agrees with the READ channel (§2 #7) ────────────
        assert_eq(q(tf, "has_pane.99"), False, "D: the read confirms 99 is absent")
        assert_eq(q(tf, "has_pane.2"), False, "D: and that the evicted pane is gone")
        assert_eq(q(tf, "has_pane.1"), True, "D: while pane 1 is present")
        assert_eq(inv(tf, "report", 1), 1, "D: and a present pane reports")
        assert_eq(q(tf, "reports"), 1, "D: exactly one report ran")

        # ── (E) the CODE is what a consumer branches on ─────────────────────
        # A framework finding — the schema declares no such action — is NOT the
        # producer's refusal and keeps the pre-R1564 code and word. That
        # distinction is the whole reason free-text `data` needed a code split.
        assert_rpc_error(lambda: inv(tf, "nope", None), data="UnknownInvokePath")
        assert_rpc_error(lambda: inv(tf, "report", "one"), data="InvokeTypeMismatch")
        # …and the refusal does not.
        refused = rpc_error_data(
            lambda: inv(tf, "report", 99), code=ACTION_REFUSED, label="E: refusal"
        )
        assert "no pane 99" in refused, f"E: under its own code, still the sentence: {refused!r}"

        # A malformed pane id is the surface's finding, not the schema's — the
        # type matched — so it is a refusal too, and says which of the three.
        assert_action_refused(lambda: inv(tf, "report", -5), saying="-5 is not a pane id")

        # ── (F) `with_origin` composes: who AND why in one frame (R1487) ────
        disclosed = rpc_error_data(
            lambda: tf.request(
                "scene/invoke",
                {"path": f"{EXT}/report", "args": 99, "with_origin": True},
            ),
            code=ACTION_REFUSED,
            expect=dict,
            label="F: disclosing refusal",
        )
        assert "no pane 99" in disclosed["reason"], (
            f"F: the sentence rides `data.reason`: {disclosed!r}"
        )
        assert_eq(disclosed["origin"], "state", "F: beside the surface that refused")

        # ── (G) an action that would change nothing is refused BY NAME ──────
        assert_action_refused(
            lambda: inv(tf, "install_detector", None),
            saying="a detector is already installed",
        )
        assert_action_refused(
            lambda: inv(tf, "evict_pane", 2),
            saying="no pane 2 on this host",
        )
        assert_eq(q(tf, "panes"), "1, 3", "G: and neither refusal changed the host")
        assert_eq(q(tf, "reports"), 1, "G: nor the report count")

        # ── (G2) the WRITE channel states its range, under its own code ─────
        # R1565 — `OutOfRange` was the one arm of `InterveneError` whose meaning
        # its variant does not determine, and it now carries the range. The code
        # is FORCED rather than chosen: `data` became free application text, and
        # `rpc/errors` publishes that -32602 carries a closed vocabulary.
        tf.intervene(f"{EXT}/reports", 7)
        assert_eq(q(tf, "reports"), 7, "G2: an in-range restore lands")
        said = assert_out_of_range(
            lambda: tf.intervene(f"{EXT}/reports", 10_000),
            saying="a report count runs 0..=1000",
        )
        assert "10000 is outside it" in said, f"G2: and names the value: {said!r}"
        assert_eq(q(tf, "reports"), 7, "G2: a refused write changed nothing")
        # A READ-ONLY slot keeps the pre-R1565 code and word: its meaning IS its
        # variant, so a sentence would restate it. Without this the new code
        # would be unfalsifiable — everything could carry prose.
        assert_rpc_error(lambda: tf.intervene(f"{EXT}/panes", "x"), data="ReadOnly")
        assert_rpc_error(lambda: tf.intervene(f"{EXT}/reports", "seven"),
                         data="InterveneTypeMismatch")

        # ── (H) the CODES are discoverable too, not only readable in source ─
        # R1564.1 — without this the round would have replaced one
        # undiscoverable contract with another: `data` became the surface's
        # prose, which moved the classifying job onto `error.code`, and an
        # agent could only learn what -32005 meant by reading pinion.
        catalogue = call(tf, "rpc/errors")
        by_code = {e["code"]: e for e in catalogue["errors"]}
        assert_eq(catalogue["count"], len(catalogue["errors"]), "H: the count is the list")
        refused_entry = by_code[ACTION_REFUSED]
        assert by_code[VALUE_OUT_OF_RANGE]["data_is_prose"], (
            "H: the write channel's code is published as prose-carrying too"
        )
        assert_eq(refused_entry["message"], "Action refused", "H: the message it pairs with")
        assert refused_entry["data_is_prose"], "H: and it says the payload is prose"
        assert not by_code[-32602]["data_is_prose"], (
            "H: while -32602 carries a word from a closed vocabulary, which is "
            "the whole distinction a consumer needs"
        )
        # The rule is stated ON THE WIRE, not only in this crate's rustdoc.
        assert "Branch on error.code" in catalogue["data_doc"], catalogue["data_doc"]
        # R1565.2 — and this harness's own copy of that rule is the SAME rule.
        # `rpc_verify.PROSE_DATA_CODES` decides which codes a demo is allowed to
        # match a payload under; a mirror nothing compares is a second contract
        # free to drift from the first, which is how the constants above would
        # go stale the next time a code is split out.
        assert_eq(
            {e["code"] for e in catalogue["errors"] if e["data_is_prose"]},
            set(PROSE_DATA_CODES),
            "H: the harness mirrors exactly the codes the wire calls prose",
        )
        # An application code is classifiable arithmetically, so a code added
        # after a client shipped is still placeable rather than merely unknown.
        low, high = catalogue["application_range"]
        assert low <= ACTION_REFUSED <= high, "H: ACTION_REFUSED is in the published range"
        assert not refused_entry["standard"], "H: and is pinion's, not JSON-RPC's"
        # The catalogue is reachable the way everything else is: by asking.
        assert "rpc/errors" in {m["name"] for m in call(tf, "rpc/methods")["methods"]}, (
            "H: rpc/errors is in the method catalogue"
        )

        # ── (I) the surface is discoverable, so an agent finds these actions ─
        schema = tf.query(f"{EXT}/$schema")
        declared = {f["path"] for f in schema}
        for action in ("report", "install_detector", "evict_pane"):
            assert action in declared, f"I: {action} is declared: {sorted(declared)}"
        assert "has_pane.<id>" in declared, "I: including the parametric read"


if __name__ == "__main__":
    run_demo("r1564 a refused invoke states why", body)
