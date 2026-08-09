#!/usr/bin/env python3
"""R833 §3 §5.53 §5.15 — own-rendered print dialog.

pinion paints its own print dialog (no native GTK/the toolkit dialog) and submits
through the pinion-core PrintBackend substrate. This box has no CUPS
destination, so the dialog runs on InMemoryPrintBackend (3 sample
printers); the real CupsPrintBackend (pinion-platform-print) is the
FileStorage-equivalent platform impl for a configured machine.

The whole pick-printer -> set-copies -> Print -> spool loop is RPC-driven
and verified through the `job` introspection node:
  /job/external/{printer_count, selected, selected_id, copies,
                 submit_count, last_printer, last_copies, last_job}

Controls (composed ButtonExternals): cycle printer / copies - / copies +
/ Print.

>= 30 assertions.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (460, 360)

SEL = "/job/external/selected"
SEL_ID = "/job/external/selected_id"
COPIES = "/job/external/copies"
SUBMITS = "/job/external/submit_count"
LAST_PRINTER = "/job/external/last_printer"
LAST_COPIES = "/job/external/last_copies"
LAST_JOB = "/job/external/last_job"


def _click(tf, tag: str) -> None:
    tf.click(path=tag)


def body() -> None:
    # Force the in-memory backend for a deterministic demo regardless of
    # the box's CUPS state (mirrors todomvc's PINION_STORAGE_DIR override).
    # A configured machine auto-selects CupsPrintBackend; here we pin memory
    # so the sample roster + recorded receipts are stable.
    os.environ["PINION_PRINT_BACKEND"] = "memory"
    with RpcSubprocess("hello-print", boot_grace=1.5) as tf:
        # ── (A) boot: dialog rendered, sample roster, clean state ────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, "printers") is not None, "printer list rendered"
        assert_eq(tf.query("/job/external/backend_kind"), "memory", "in-memory backend pinned")
        assert_eq(tf.query("/job/external/printer_count"), 3, "3 sample printers")
        assert_eq(tf.query(SEL), 0, "first printer selected at boot")
        assert_eq(tf.query(SEL_ID), "office-laser", "default printer is office-laser")
        assert_eq(tf.query(COPIES), 1, "copies boots at 1")
        assert_eq(tf.query(SUBMITS), 0, "no job submitted yet")
        assert_eq(tf.query(LAST_PRINTER), "", "no last printer yet")

        # ── (B) cycle printer selection (wraps) ──────────────────────
        _click(tf, "cycle")
        wait_until(lambda: tf.query(SEL) == 1, timeout=4.0, interval=0.03,
                   desc="cycle -> printer 1")
        assert_eq(tf.query(SEL_ID), "pdf-virtual", "second printer is pdf-virtual")
        _click(tf, "cycle")
        wait_until(lambda: tf.query(SEL) == 2, timeout=4.0, interval=0.03,
                   desc="cycle -> printer 2")
        assert_eq(tf.query(SEL_ID), "front-desk", "third printer is front-desk")
        _click(tf, "cycle")
        wait_until(lambda: tf.query(SEL) == 0, timeout=4.0, interval=0.03,
                   desc="cycle wraps back to printer 0")
        assert_eq(tf.query(SEL_ID), "office-laser", "wrapped back to office-laser")

        # ── (C) copies +/- (clamps at 1) ─────────────────────────────
        _click(tf, "inc")
        wait_until(lambda: tf.query(COPIES) == 2, timeout=4.0, interval=0.03,
                   desc="copies + -> 2")
        _click(tf, "inc")
        wait_until(lambda: tf.query(COPIES) == 3, timeout=4.0, interval=0.03,
                   desc="copies + -> 3")
        _click(tf, "dec")
        wait_until(lambda: tf.query(COPIES) == 2, timeout=4.0, interval=0.03,
                   desc="copies - -> 2")
        # Clamp: hammer dec below 1.
        for _ in range(5):
            _click(tf, "dec")
        wait_until(lambda: tf.query(COPIES) == 1, timeout=4.0, interval=0.03,
                   desc="copies clamps at 1")

        # ── (D) Print: submits the job, records the receipt ──────────
        # Set up a known job: printer 0 (office-laser), 2 copies.
        _click(tf, "inc")
        wait_until(lambda: tf.query(COPIES) == 2, timeout=4.0, interval=0.03,
                   desc="copies back to 2")
        _click(tf, "print")
        wait_until(lambda: tf.query(SUBMITS) == 1, timeout=4.0, interval=0.03,
                   desc="Print submits one job")
        assert_eq(tf.query(LAST_PRINTER), "office-laser", "job went to office-laser")
        assert_eq(tf.query(LAST_COPIES), 2, "job carried 2 copies")
        assert_eq(tf.query(LAST_JOB), "mem-1", "first job id is mem-1")
        # The status line reflects the receipt.
        texts = _all_text(tf)
        assert any("office-laser" in t for t in texts), "status names the printer"

        # ── (E) a second job to a different printer ──────────────────
        _click(tf, "cycle")  # -> pdf-virtual
        wait_until(lambda: tf.query(SEL_ID) == "pdf-virtual", timeout=4.0, interval=0.03,
                   desc="re-select pdf-virtual")
        _click(tf, "print")
        wait_until(lambda: tf.query(SUBMITS) == 2, timeout=4.0, interval=0.03,
                   desc="second Print submits another job")
        assert_eq(tf.query(LAST_PRINTER), "pdf-virtual", "second job went to pdf-virtual")
        assert_eq(tf.query(LAST_JOB), "mem-2", "second job id is mem-2")
        assert_eq(tf.query(LAST_COPIES), 2, "second job carried 2 copies")

        # ── (F) KEYBOARD: focus a button, activate with Enter ─────────
        # R834 regression guard — the original apply_key was dead code
        # (invoke("key") on a button is unhandled) and only `print` was
        # focusable. The dialog must be fully keyboard-operable.
        tf.request("focus/set", {"tag": "inc"})
        wait_until(lambda: tf.request("focus/get").result.get("focused") == "inc",
                   timeout=4.0, interval=0.03, desc="focus the copies + button")
        copies_before = tf.query(COPIES)
        tf.key(path="inc", name="Enter")
        wait_until(lambda: tf.query(COPIES) == copies_before + 1, timeout=4.0, interval=0.03,
                   desc="Enter on the focused + button increments copies")
        # Arrow roves focus to the next button (Print), Enter submits.
        submits_before = tf.query(SUBMITS)
        tf.key(path="inc", name="ArrowRight")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == "print",
                   timeout=4.0, interval=0.03, desc="ArrowRight roves focus inc -> print")
        tf.key(path="print", name="Enter")
        wait_until(lambda: tf.query(SUBMITS) == submits_before + 1, timeout=4.0, interval=0.03,
                   desc="Enter on the focused Print button submits")


def _all_text(tf) -> list[str]:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text" and isinstance(n.get("content"), str):
                out.append(n["content"])
            for c in n.get("children") or []:
                walk(c)

    walk(snap)
    return out


if __name__ == "__main__":
    sys.exit(run_demo("hello-print R833 §3 §5.53 own-rendered print dialog", body))
