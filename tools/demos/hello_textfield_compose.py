#!/usr/bin/env python3
"""hello-textfield IME composition dogfood (R56.1.g.3 §5.22 §5.38).

End-to-end RPC self-verify for the R56.1.g IME composition cascade:

- invoke("composition", {"action": "start"}) drives BeginEdit + seeds
  preedit Some("") (W3C compositionstart canonical).
- invoke("composition", {"action": "update", "data": ...}) sets the
  preedit string (W3C compositionupdate canonical).
- invoke("composition", {"action": "end", "data": ...}) commits the
  preedit into the text buffer at the caret position, drives
  CommitEdit, and emits Intent("text_committed", Text(committed))
  (W3C compositionend canonical).
- invoke("composition", {"action": "cancel"}) discards the preedit
  silently (W3C cancel-shape).
- query("preedit") observes the live preedit state (Text(s) while
  composing, None when idle).
- intervene("preedit", Text(s)) auto-starts composition + sets the
  preedit (the AI-client-as-platform-IME use case).
- intervene("preedit", null) cancels the active composition.
- Korean multi-byte commit ("한" syllable) round-trips through the
  UTF-8 boundary code path.
- on_focus_change(false) with non-empty preedit commits the preedit
  as the W3C IME canonical commit-on-blur behaviour.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_query


TF_TAG = "main_textfield"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    """`focus/set` wrapper mirroring hello_textfield_select.py."""
    tf.request("focus/set", {"tag": tag})


def body() -> None:
    with RpcSubprocess("hello-textfield") as tf:
        run_body(tf)


def run_body(tf: RpcSubprocess) -> None:
    # Pre-flight: schema includes the R56.1.g.2 `preedit` slot;
    # initial state returns Null (no composition active).
    assert_eq(tf.query("/external/state"), "Idle", "initial /external/state")
    assert_eq(
        tf.query("/external/preedit"),
        None,
        "initial /external/preedit is null (not composing)",
    )

    # Focus the field so subsequent dispatch resolves the field
    # as the focused widget.
    focus_set(tf, TF_TAG)
    wait_query(tf, "/external/state", "Focused", desc="post-focus state")

    # ── R56.1.g.2 invoke composition lifecycle: start.
    assert_eq(
        tf.invoke("/external/composition", {"action": "start"}),
        "Editing",
        "compositionstart drives SCXML Focused -> Editing",
    )
    wait_query(
        tf, "/external/preedit", "",
        desc="post-start preedit is empty string (compositionstart-before-update)",
    )

    # ── compositionupdate: set preedit to "h".
    assert_eq(
        tf.invoke("/external/composition", {"action": "update", "data": "h"}),
        "Editing",
        "compositionupdate keeps SCXML in Editing",
    )
    wait_query(
        tf, "/external/preedit", "h",
        desc="preedit reflects the latest update",
    )

    # Successive updates replace (not append) the preedit content.
    tf.invoke("/external/composition", {"action": "update", "data": "hi"})
    wait_query(
        tf, "/external/preedit", "hi",
        desc="successive update replaces preedit content",
    )

    # ── compositionend with non-empty data commits the preedit.
    assert_eq(
        tf.invoke("/external/composition", {"action": "end", "data": "hi"}),
        "Focused",
        "compositionend drives Editing -> Focused",
    )
    wait_query(tf, "/external/text", "hi", desc="preedit committed into text")
    assert_eq(tf.query("/external/caret"), 2, "caret advanced by 2 bytes")
    assert_eq(
        tf.query("/external/preedit"),
        None,
        "preedit cleared post-commit",
    )

    # ── compositionend with empty data is the cancel-shape: clears
    # preedit, drives SCXML, no text inserted.
    tf.invoke("/external/composition", {"action": "start"})
    tf.invoke("/external/composition", {"action": "update", "data": "xyz"})
    wait_query(
        tf, "/external/preedit", "xyz",
        desc="preedit set before cancel-shape end",
    )
    assert_eq(
        tf.invoke("/external/composition", {"action": "end", "data": ""}),
        "Focused",
        "empty-data end transitions to Focused",
    )
    wait_query(
        tf, "/external/text", "hi",
        desc="no insertion on empty-data end (text unchanged)",
    )
    assert_eq(tf.query("/external/preedit"), None, "preedit cleared")

    # ── compositionend cancel via action="cancel" explicit path.
    tf.invoke("/external/composition", {"action": "start"})
    tf.invoke("/external/composition", {"action": "update", "data": "abc"})
    assert_eq(
        tf.invoke("/external/composition", {"action": "cancel"}),
        "Focused",
        "cancel drives Editing -> Focused",
    )
    wait_query(
        tf, "/external/text", "hi",
        desc="cancel preserves the text buffer (no insertion)",
    )
    assert_eq(tf.query("/external/preedit"), None, "preedit cleared on cancel")

    # ── Korean multi-byte commit: jamo -> syllable round-trip.
    # The substrate does not know jamo composition; the platform IME
    # composes, the substrate just inserts the committed string
    # verbatim ("한" = 3 bytes UTF-8 = 0xED 0x95 0x9C).
    tf.invoke("/external/composition", {"action": "start"})
    tf.invoke("/external/composition", {"action": "update", "data": "ㅎ"})
    tf.invoke("/external/composition", {"action": "update", "data": "하"})
    tf.invoke("/external/composition", {"action": "end", "data": "한"})
    wait_query(
        tf, "/external/text", "hi한",
        desc="Korean 3-byte syllable committed at caret",
    )
    assert_eq(
        tf.query("/external/caret"),
        5,
        "caret advanced by 3 bytes (2+3 = 5)",
    )

    # ── intervene preedit auto-starts composition.
    tf.intervene("/external/preedit", "compose")
    wait_query(
        tf, "/external/preedit", "compose",
        desc="intervene preedit Text auto-starts + updates",
    )
    # SCXML state unchanged by intervene (no BeginEdit drive).
    assert_eq(tf.query("/external/state"), "Focused", "intervene keeps SCXML stable")

    # ── intervene preedit null cancels composition.
    tf.intervene("/external/preedit", None)
    wait_query(
        tf, "/external/preedit", None,
        desc="intervene preedit Null cancels composition",
    )

    # ── commit-on-blur with non-empty preedit (R56.1.g.1 on_focus_change).
    # Drive a fresh composition then blur via focus_set(None); the
    # in-flight preedit must commit before SCXML reaches Idle.
    tf.invoke("/external/composition", {"action": "start"})
    tf.invoke("/external/composition", {"action": "update", "data": "x"})
    wait_query(tf, "/external/preedit", "x", desc="preedit before blur")
    focus_set(tf, None)
    wait_query(tf, "/external/state", "Idle", desc="post-blur state Idle")
    assert_eq(
        tf.query("/external/text"),
        "hi한x",
        "commit-on-blur appended the in-flight preedit",
    )
    assert_eq(tf.query("/external/preedit"), None, "preedit cleared post-blur")


if __name__ == "__main__":
    sys.exit(run_demo("hello-textfield composition", body))
