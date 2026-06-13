#!/usr/bin/env python3
"""R911 §5.53 §3 — own-renderer print spools a vector PDF (not text).

R833 built the own-rendered print dialog (printer pick / copies / Print)
but spooled a plain-text blob — the `pinion_core::print` doc flagged the
scene -> PDF page render as a future axis. R908 landed that renderer
(`pinion-pdf`). R911 wires it in: the Print action now renders the
document to a vector PDF through the §5.53 own-renderer pipeline and
spools *that* — the 2nd consumer of the `pinion-pdf` substrate
([[substrate-incompleteness-signal]]), completing the own-renderer print
story (the dialog AND the document are pinion-rendered).

The submitted document is exposed at `/job/external/last_content`, so an
AI agent verifies the spooled payload is a real PDF carrying the report —
no pixels, deterministic (Scene -> PDF is a pure function).

## Demo scope (>=30 assertions, sections A-E)

  (A) Boot: in-memory backend, nothing submitted, no content yet.
  (B) Print -> the submitted content is a valid PDF skeleton.
  (C) The PDF embeds the rendered document (title + body + the rule).
  (D) Print metadata (printer / copies / count) tracks the submit.
  (E) A second Print (more copies) re-renders a fresh PDF.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
    wait_query,
)

SUBMITS = "/job/external/submit_count"
LAST_PRINTER = "/job/external/last_printer"
LAST_COPIES = "/job/external/last_copies"
LAST_CONTENT = "/job/external/last_content"

# The report the dialog renders (mirrors hello-print's DOC_TITLE/DOC_BODY).
_TITLE = "Quarterly Report"
_BODY = ("Revenue up 12% QoQ", "Operating margin steady at 18%", "Prepared by pinion")


def _assert_pdf_skeleton(doc: str) -> None:
    """(B) the spooled content is a structurally valid PDF."""
    assert isinstance(doc, str) and doc, "spooled content present"
    assert doc.startswith("%PDF-1.7"), "spooled payload is a PDF, not text"
    assert doc.rstrip().endswith("%%EOF"), "PDF trailer present"
    assert "/Type /Catalog" in doc, "document catalog"
    assert "/Type /Pages" in doc, "page tree root"
    assert "/Type /Page" in doc, "page object"
    assert "/MediaBox [0 0 612 792]" in doc, "US Letter page box"
    assert "/Count 1" in doc, "single page"
    assert "xref\n" in doc and "startxref\n" in doc and "trailer\n" in doc, "xref + trailer"
    assert "/BaseFont /Helvetica" in doc, "standard-14 font declared"
    assert "/Contents 4 0 R" in doc, "page references the content stream"
    assert "stream\n" in doc and "endstream" in doc, "content stream delimited"
    assert "/Root 1 0 R" in doc, "trailer roots at the catalog"
    assert doc.isascii(), "the PDF document is pure ASCII"


def _assert_document_embedded(doc: str) -> None:
    """(C) the rendered report content is in the PDF."""
    assert f"({_TITLE}) Tj" in doc, "title text embedded as a show operator"
    for line in _BODY:
        assert f"({line}) Tj" in doc, f"body line embedded: {line!r}"
    assert "/F2 " in doc, "bold font used (the title)"
    # The accent rule under the title fills a rect.
    assert "re\nf\n" in doc, "accent rule rect filled"
    # A blue accent (0x2f/0x6f/0xed normalized) fill color appears.
    assert " rg\n" in doc, "a fill colour is set"


def body() -> None:
    # Force the in-memory backend so the spooled job is recorded
    # deterministically (this box has no CUPS destination anyway).
    os.environ["PINION_PRINT_BACKEND"] = "memory"
    with RpcSubprocess("hello-print", boot_grace=1.5) as tf:
        # ── (A) Boot ─────────────────────────────────────────────
        assert tf.query("/job/external/backend_kind") == "memory", "in-memory backend"
        assert tf.query(SUBMITS) == 0, "nothing submitted yet"
        assert tf.query(LAST_CONTENT) == "", "no spooled content yet"

        # ── (B) Print -> a PDF is spooled ────────────────────────
        tf.click(path="print")
        wait_query(tf, SUBMITS, 1, desc="Print -> submit_count 1")
        doc = tf.query(LAST_CONTENT)
        _assert_pdf_skeleton(doc)

        # ── (C) The PDF embeds the rendered document ─────────────
        _assert_document_embedded(doc)

        # ── (D) Print metadata tracks the submit ─────────────────
        assert tf.query(LAST_PRINTER) == "office-laser", "spooled to the default printer"
        assert tf.query(LAST_COPIES) == 1, "one copy"
        assert tf.query("/job/external/selected_id") == "office-laser", "selection unchanged"
        assert tf.query("/job/external/last_job"), "backend assigned a job id"

        # ── (E) Bump copies, print again -> fresh PDF ────────────
        tf.click(path="inc")
        wait_query(tf, "/job/external/copies", 2, desc="copies -> 2")
        tf.click(path="print")
        wait_query(tf, SUBMITS, 2, desc="second Print -> submit_count 2")
        assert tf.query(LAST_COPIES) == 2, "second job is two copies"
        doc2 = tf.query(LAST_CONTENT)
        _assert_pdf_skeleton(doc2)
        _assert_document_embedded(doc2)
        # The document is the same regardless of copy count -> Scene -> PDF
        # is a pure, deterministic function (copies is a spool param, not
        # part of the rendered page).
        assert doc2 == doc, "the rendered document is byte-identical across submits"


if __name__ == "__main__":
    sys.exit(run_demo("R911 own-renderer print spools a vector PDF", body))
