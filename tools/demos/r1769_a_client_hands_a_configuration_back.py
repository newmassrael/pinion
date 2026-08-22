#!/usr/bin/env python3
"""R1769 §5.15 §5.8 §2 #2 — **a client reads a widget's statechart configuration
over the wire and hands it back, and the widget arrives there without the
history that first put it there.**

# What this demo exists for

`pinion-rpc`'s `rewind` module carried this clause since it was written: the
symbolic snapshot/restore primitive anchors §5.8 `dry_run` *"once an
engine-level hook is wired in"*. R1768 built the hook in `pinion_core::resume`
and left the wire leg unbuilt, registering it rather than carrying it silently.
This is the wire leg, and everything below goes through `scene/query` and
`scene/invoke` — what an AI client has, and what §2 #2 makes the primary path.

# ★ The property, and where each half of it can be seen

Driving a widget to a state and resuming it to that state end in the same place,
so `state` alone cannot tell them apart. What separates them is that the drive
runs every `<onentry>` on the way in and the resume runs none.

* The ENGINE half — that no entry action fires — is measured in Rust, on a chart
  whose entry action raises, using the engine's own truncation counter
  (`pinion_core::resume`'s tests). A wire client cannot see that counter, and
  this demo does not pretend to.
* The half a client CAN see is a **widget whose activation edge leaves a mark
  the wire reads**, and `hello-toggle` is one: crossing `Pressed → Hover` flips
  its `value` sidecar. Driving there flips it; resuming to the same
  configuration does not, because the resume never crossed the edge. Same
  statechart state, different `value` — the observable shadow of the same fact,
  and simultaneously the caveat every adopting schema states: `resume` restores
  the MACHINE and not the sidecar, which has its own slot.

⚠ The first draft of this demo used `scene/intents` for that comparison and it
FAILED at the premise: driving through the activation edge and then asking
returned nothing. That is `debt-intents-are-empty-at-rpc-cadence` (R1569, whose
diagnosis is recorded as unverified), re-measured here by accident. The
comparison moved to a surface the wire can actually see rather than being
dropped, because a demo that quietly stopped asserting the round's own claim
would have gone green while proving nothing.

# What it drives

* **A** — the pair is DECLARED, and on the two different channels, so a client
  discovers which call to make instead of being told.
* **B** — a read is free of side effects: twice in a row, same answer, machine
  unmoved.
* **C** — the round trip, value passed through untouched. Nothing in this file
  constructs a configuration, because a client could not either.
* **D** — ★ the same state, reached two ways, with different intent streams.
* **E** — a resumed machine is alive: a transition out of it is taken.
* **F** — two refusals, kept apart because the fix differs, both as sentences.
* **G** — ★ a configuration from ANOTHER widget is refused, and the refused
  widget does not move. That is the one an in-process test cannot make
  convincing: two live applications, one value.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcError, RpcSubprocess, assert_eq, run_demo


def _refusal_of(fn) -> str:
    """Run `fn`, require it to be refused, and return what the refusal said."""
    try:
        fn()
    except RpcError as err:
        return f"{err.message} {err.data!r}"
    raise AssertionError("expected a refusal, the call succeeded")


def body() -> None:
    # ── A. the pair is declared, on two channels ──────────────────────────
    with RpcSubprocess("hello-button") as btn:
        schema = btn.query("/external/$schema")
        by_path = {f["path"]: f for f in schema}

        assert "configuration" in by_path, f"no `configuration`: {sorted(by_path)}"
        assert "resume" in by_path, f"no `resume`: {sorted(by_path)}"
        # R1504's rule: `channel` rides only where it is NOT the default, so a
        # read field is the one with no key. Asserting the absence is asserting
        # the wire's own rule rather than a spelling this demo invented.
        assert_eq(
            by_path["configuration"].get("channel"),
            None,
            "configuration is a READ, which the wire says by omitting `channel`",
        )
        assert_eq(by_path["resume"].get("channel"), "invoke", "resume channel")
        assert_eq(by_path["configuration"].get("type"), "json", "configuration type")
        assert_eq(by_path["resume"].get("type"), "json", "resume type")
        assert by_path["resume"].get("args"), "and `resume` declares what it takes"

        # ── B. a read does not move the machine ───────────────────────────
        assert_eq(btn.query("/external/state"), "Idle", "a fresh button starts Idle")
        once = btn.query("/external/configuration")
        twice = btn.query("/external/configuration")
        assert_eq(once, twice, "two reads agree — a snapshot never drives")
        assert_eq(btn.query("/external/state"), "Idle", "and the state is unmoved")

        # ── F. two refusals, kept apart because the fix differs ───────────
        shape = _refusal_of(lambda: btn.invoke("/external/resume", "Idle"))
        assert "mismatch" in shape.lower() or "type" in shape.lower(), (
            f"a bare state name is refused on SHAPE — a client bug: {shape}"
        )

        stale = _refusal_of(lambda: btn.invoke("/external/resume", {"nope": 1}))
        assert "button.resume" in stale, f"the refusal names the surface: {stale}"
        assert "configuration" in stale, f"and says what to hand back: {stale}"
        assert "Rejected(" not in stale, f"and it is a sentence, not Debug: {stale}"
        assert_eq(btn.query("/external/state"), "Idle", "a refusal does not move it")

    # ── C/D. the comparison, on a widget whose activation leaves a mark ───
    with RpcSubprocess("hello-toggle") as driven:
        assert_eq(driven.query("/external/state"), "Idle", "starts Idle")
        assert_eq(driven.query("/external/value"), False, "and Off")

        driven.invoke("/external/send", "PointerEnter")
        driven.invoke("/external/send", "PointerDown")
        driven.invoke("/external/send", "PointerUp")

        assert_eq(driven.query("/external/state"), "Hover", "released into Hover")
        assert_eq(
            driven.query("/external/value"),
            True,
            "★ the premise of the whole comparison: reaching Hover BY DRIVING "
            "crosses the activation edge, and this widget's sidecar records it",
        )

        saved = driven.query("/external/configuration")
        assert isinstance(saved, dict), f"a configuration is an object: {saved!r}"
        # ★ The value is STAMPED with the widget kind. This demo is what found
        # that it had to be — see block G, and `widget_core::widget_configuration`.
        assert_eq(saved.get("widget"), "toggle", "it says which widget it came from")
        cfg = saved.get("configuration")
        assert isinstance(cfg, dict), f"and carries the configuration: {saved!r}"
        assert "states" in cfg and "current" in cfg, f"both halves: {cfg!r}"
        assert_eq(cfg["current"], "Hover", "it names the leaf")
        assert cfg["states"], "and holds the chain that leaf sits in"

    with RpcSubprocess("hello-toggle") as fresh:
        assert_eq(fresh.query("/external/state"), "Idle", "the premise: starts Idle")
        assert_eq(fresh.query("/external/value"), False, "and Off")

        answered = fresh.invoke("/external/resume", saved)

        assert_eq(answered, "Hover", "the action answers where it ended up")
        assert_eq(fresh.query("/external/state"), "Hover", "and the read agrees")
        assert_eq(
            fresh.query("/external/configuration"),
            saved,
            "the configuration it reports is the one it was handed, unchanged",
        )
        assert_eq(
            fresh.query("/external/value"),
            False,
            "★★★★★ THE HEADLINE, and `state` alone cannot see it: the SAME "
            "Hover, reached without crossing the activation edge, left the "
            "sidecar untouched. The driven one reads True. Restore is not "
            "replay — and this is simultaneously the caveat every adopting "
            "schema states, that `resume` restores the MACHINE and not the "
            "sidecar, which has its own slot",
        )

        # ── E. a resumed machine is alive, not merely positioned ──────────
        fresh.invoke("/external/send", "PointerLeave")
        assert_eq(
            fresh.query("/external/state"),
            "Idle",
            "a transition out of the resumed state is taken, so the machine is "
            "running rather than parked",
        )

        # And its own configuration round-trips, so the surface is not simply
        # refusing everything that reaches it.
        own = fresh.query("/external/configuration")
        fresh.invoke("/external/resume", own)
        assert_eq(fresh.query("/external/configuration"), own, "own value accepted")

    # ── G. another widget's configuration is not this widget's ────────────
    #
    # ★★★★★ THIS BLOCK IS WHY THE VALUE IS STAMPED. Written expecting a refusal,
    # it went GREEN on the first run: `ButtonState` and `ToggleState` come from
    # the same statechart template, so their variant names are identical and the
    # toggle's configuration is a structurally VALID configuration of the
    # button's document. The engine was right to accept it — nothing at that
    # layer can tell two documents apart when their vocabularies coincide.
    #
    # It is still the wrong outcome one layer up: a client restoring a session
    # knows which widget the snapshot came from, and a wire form that drops that
    # turns a mixed-up restore into a machine that looks fine and is elsewhere.
    # So `widget_configuration` stamps the kind and `resume_widget` checks it
    # BEFORE parsing, and this assertion is the one that demanded it.
    with RpcSubprocess("hello-button") as btn2:
        crossed = _refusal_of(lambda: btn2.invoke("/external/resume", saved))
        assert "button.resume" in crossed, f"refused by the BUTTON: {crossed}"
        assert "toggle" in crossed, f"and it names whose value it was: {crossed}"
        assert_eq(
            btn2.query("/external/state"),
            "Idle",
            "★ and the button did not move — the refusal lands before any "
            "mutation, so a refused resume never half-enters, which is the one "
            "outcome a caller could not detect afterwards",
        )

        # An unstamped value — the shape every configuration had before this
        # demo found the hole — is refused too, rather than silently restored.
        unstamped = _refusal_of(
            lambda: btn2.invoke("/external/resume", {"states": ["Idle"], "current": "Idle"})
        )
        assert "does not say which widget" in unstamped, unstamped


if __name__ == "__main__":
    run_demo("r1769 a client hands a configuration back", body)
