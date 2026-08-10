#!/usr/bin/env python3
"""R1639 — a widget publishes the events it accepts, so a client never guesses one.

R1638 gave an action an argument grammar and applied it where the grammar was a
crate's own. It could not describe the commonest action in the tree: eleven
widgets spell `send` as ONE statechart event name, and the set of legal names is
per widget and existed only as a runtime `Vec` — reachable when a refusal was
being worded, and nowhere a schema could point at.

So the vocabulary was discoverable only by getting it wrong. R1564 made that
refusal name the accepted set, which is a good refusal and a poor contract: it
requires an agent to make a call it expects to fail before it can make one it
expects to succeed.

`#[derive(WidgetEventName)]` now emits `DRIVABLE_NAMES` as a `const`, projected
from the very `EXTERNALLY_DRIVABLE_EVENTS` the parser gates on — so the set a
client reads, the set a refusal advertises and the set `from_name` admits are
one list by construction, not by care.

What each block discriminates:

* **(A) the vocabulary is on the wire, per widget.** Not a shared list: a
  toggle's events and a slider's differ, and the schema carries each widget's
  own.
* **(B) every published name is accepted.** Driven one at a time through the
  real wire. A list that is too long promises an event the parser refuses.
* **(C) an unpublished name is refused, and the refusal still teaches.** The
  R1564 sentence stays — a client that guessed anyway is told the set — so the
  declaration ADDS a path rather than replacing one.
* **(D) internal events stay unforgeable.** A chart raises `ButtonActivate`
  itself; it is absent from the published set AND refused on the wire, which is
  the property that makes publishing the set safe at all.
* **(E) the composite widgets still declare the other grammar.** Six surfaces
  take either form and declare the delimited one; this checks the declaration is
  present and consistent, not that the shorthand is gone.

Run from the workspace root:
    cargo build -p hello-toggle -p hello-slider --release
    python3 tools/demos/r1639_a_widget_publishes_its_verbs.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    run_demo,
)

EXT = "/external"


def send_field(tf: RpcSubprocess, path: str = EXT) -> dict:
    for field in tf.query(f"{path}/$schema"):
        if field["path"] == "send":
            return field
    raise AssertionError(f"{path} declares no send action")


def published_events(field: dict) -> list[str]:
    assert_eq(field["arg_form"]["kind"], "scalar", "a bare-event send")
    assert_eq(len(field["args"]), 1, "one argument")
    domain = field["args"][0]["domain"]
    assert_eq(domain["kind"], "one_of", "and it names its vocabulary")
    return list(domain["values"])


def drive(tf: RpcSubprocess, example: str) -> list[str]:
    """(A)+(B)+(C)+(D) against one widget, entirely from its declaration."""
    field = send_field(tf)
    events = published_events(field)
    assert events, f"{example}: an empty vocabulary is a promise it cannot keep"

    # (B) every published name is accepted. The widget's state may or may not
    # change — what is under test is that the NAME is admitted, so a refusal is
    # the failure and any answer is a pass.
    for name in events:
        tf.invoke(f"{EXT}/send", name)

    # (C) a name the set does not contain is refused, and the refusal still
    # names the set — the declaration adds a path, it does not remove one.
    reason = assert_action_refused(
        lambda: tf.invoke(f"{EXT}/send", "NotAnEvent"), saying="NotAnEvent"
    )
    for name in events:
        assert name in reason, f"the refusal still lists {name!r}: {reason}"

    print(f"[demo] {example}: {len(events)} published events, all accepted")
    return events


def internal_events_stay_unforgeable(tf: RpcSubprocess) -> None:
    """(D) — a chart-raised event is neither published nor callable.

    This is what makes publishing the set safe: the list is the DRIVABLE const,
    not the variant list, so an internal `<raise>` cannot be forged by a caller
    who reads the schema and assumes every event of the machine is on it.
    """
    events = published_events(send_field(tf))
    assert "ButtonActivate" not in events, f"an internal event is not published: {events}"
    assert_action_refused(
        lambda: tf.invoke(f"{EXT}/send", "ButtonActivate"), saying="ButtonActivate"
    )
    print("[demo] the internal event is neither published nor callable")


def body() -> None:
    # Two different widgets, because the point is that the vocabulary is the
    # WIDGET's. One example would pass against a shared global list.
    with RpcSubprocess("hello-toggle") as tf:
        toggle = drive(tf, "toggle")
    with RpcSubprocess("hello-slider") as tf:
        slider = drive(tf, "slider")
    assert toggle != slider, (
        f"each widget publishes its own chart's events: {toggle} vs {slider}"
    )

    with RpcSubprocess("hello-button") as tf:
        internal_events_stay_unforgeable(tf)

    # (E) a composite widget declares the other grammar, and says which.
    with RpcSubprocess("hello-listbox") as tf:
        field = send_field(tf)
        assert_eq(field["arg_form"]["kind"], "delimited", "the composite wire")
        assert_eq(field["arg_form"]["separator"], ":")
        names = [a["name"] for a in field["args"]]
        assert_eq(names, ["key", "event", "modifiers", "buttons"], "four segments")
        assert not field["args"][0].get("optional"), "the key is required, possibly empty"
        assert field["args"][2].get("optional"), "the context segments are elidable"
        assert field["args"][3].get("optional")
        print("[demo] a composite send declares four segments and which may be elided")


if __name__ == "__main__":
    run_demo("R1639 — a widget publishes its verbs", body)
