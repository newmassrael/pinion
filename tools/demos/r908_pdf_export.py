#!/usr/bin/env python3
"""R908 §3 §5.53 — own-renderer Scene -> PDF export end-to-end RPC
verification demo.

R908 lands the documented-future axis `pinion_core::print` reserved: a
scene -> PDF page-render pipeline. pinion paints its own UI from a
structured scene (inv #1: no opaque paint callbacks), so `pinion-pdf`
walks that post-layout scene (rects / text / containers / scroll) into a
vector PDF byte stream — no rasterizer, no GPU, no font embedding
(standard-14 Helvetica). `scene/export_pdf` exposes it over JSON-RPC:
the document plus its structural facts (page box, object count, byte
length). Where `scene/screenshot` still returns RenderBackendUnavailable
(pixels gated on the §5.16 RHI), PDF needs no pixel backend — that is the
inv #1 payoff.

The example `hello-pdf-export` paints a document-style scene (title,
body text, three primary-colour rounded swatches, a bordered caption
box) plus a Toggle that flips a DRAFT <-> FINAL watermark, so the demo
can prove the export tracks live scene state.

## The verification idea: deterministic structure on a pure function

`Scene -> bytes` is pure — no wall-clock, no machine variance — so the
demo asserts the document's *structure* exactly:

  - **skeleton**: %PDF-1.7 header, %%EOF trailer, xref + startxref,
    the Catalog/Pages/Page object tree, /Count 1.
  - **page box**: MediaBox matches the requested page size; named pages
    (letter/a4) + landscape swap deterministically.
  - **objects**: the four standard-14 Helvetica font objects; object
    count + byte length self-consistent.
  - **content**: primary-colour fill operators (1 0 0 / 0 1 0 / 0 0 1
    rg) for the swatches, the bordered box's stroke, embedded text.
  - **live tracking**: the watermark text in the PDF flips with a
    toggle click (observed-state gated, zero fixed sleep -> zero flake).
  - **errors**: NoPaintScene before first paint, UnknownPageSize /
    UnknownOrientation, unknown-window rejection (-32602).

## Demo scope (>=30 assertions, sections A-H)

  (A) Wire shape — every top-level key present + typed.
  (B) PDF skeleton — header / trailer / xref / object tree.
  (C) Objects — standard-14 fonts + count/byte self-consistency.
  (D) Content — colour fills, rounded curves, border stroke, text.
  (E) Live tracking — watermark flips with a toggle click.
  (F) Page size — scene-bounds default, letter, a4, landscape.
  (G) Errors — UnknownPageSize / UnknownOrientation.
  (H) Unknown-window rejection (-32602) vs NoPaintScene.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    wait_until,
)

# The example's declared canonical viewport (WIN_W x WIN_H) — the
# WYSIWYG default page is sized to the scene's own pixel bounds.
_WIN_W = 420
_WIN_H = 540

_RESULT_KEYS = (
    "page_count",
    "page_width_pt",
    "page_height_pt",
    "object_count",
    "byte_len",
    "document",
)

# The four standard-14 Helvetica base fonts the renderer always declares.
_FONT_BASES = (
    "/BaseFont /Helvetica",
    "/BaseFont /Helvetica-Bold",
    "/BaseFont /Helvetica-Oblique",
    "/BaseFont /Helvetica-BoldOblique",
)


def _drive_redraw(tf: RpcSubprocess) -> None:
    """Click the "repaint" Toggle so its state flips -> the view re-runs
    -> AppShell paints -> last_paint_scene updates. Real paint cycles
    fire on winit's RedrawRequested, asynchronous to RPC dispatch, so
    the observed export, not wall-clock, is the gate."""
    try:
        tf.request("scene/click", {"path": "repaint"})
    except Exception:  # noqa: BLE001
        pass


def _wait_available(tf: RpcSubprocess) -> dict[str, Any]:
    """Poll until the primary window has painted at least one frame and
    `scene/export_pdf` returns a document (no longer NoPaintScene)."""
    def poll() -> Any:
        try:
            return tf.export_pdf()
        except RpcError:
            _drive_redraw(tf)
            return None

    return wait_until(poll, desc="scene/export_pdf becomes available")


def _watermark(doc: str) -> str | None:
    """Which watermark the exported document carries, if any."""
    has_draft = "(DRAFT) Tj" in doc
    has_final = "(FINAL) Tj" in doc
    if has_draft and not has_final:
        return "DRAFT"
    if has_final and not has_draft:
        return "FINAL"
    return None


def _assert_wire_shape(out: dict[str, Any]) -> None:
    """(A) every documented key present + correctly typed."""
    for key in _RESULT_KEYS:
        assert key in out, f"scene/export_pdf result must carry '{key}'; got {out.keys()}"
    assert isinstance(out["page_count"], int) and out["page_count"] == 1, (
        f"page_count must be 1 in this slice; got {out['page_count']!r}"
    )
    for key in ("page_width_pt", "page_height_pt", "object_count", "byte_len"):
        assert isinstance(out[key], int) and out[key] > 0, (
            f"{key} must be a positive int; got {out[key]!r}"
        )
    assert isinstance(out["document"], str) and out["document"], "document must be a non-empty str"
    # byte_len is the document length (the PDF is ASCII, 1 byte/char).
    assert out["byte_len"] == len(out["document"]), (
        f"byte_len ({out['byte_len']}) must equal document length ({len(out['document'])})"
    )
    assert out["document"].isascii(), "the document must be pure ASCII"


def _assert_skeleton(out: dict[str, Any]) -> None:
    """(B) header, trailer, xref, and the Catalog/Pages/Page tree."""
    doc = out["document"]
    assert doc.startswith("%PDF-1.7"), "PDF version header present"
    assert doc.rstrip().endswith("%%EOF"), "EOF trailer present"
    assert "xref\n" in doc, "xref table present"
    assert "startxref\n" in doc, "startxref present"
    assert "trailer\n" in doc, "trailer present"
    assert "/Type /Catalog" in doc, "document catalog present"
    assert "/Type /Pages" in doc, "page tree root present"
    assert "/Type /Page" in doc, "page object present"
    assert "/Count 1" in doc, "single-page count"
    # MediaBox matches the reported page size.
    media = f"/MediaBox [0 0 {out['page_width_pt']} {out['page_height_pt']}]"
    assert media in doc, f"MediaBox must match the page size: expected {media!r}"
    # /Root points at object 1, /Size lists object_count + 1 slots.
    assert "/Root 1 0 R" in doc, "trailer roots at the catalog (object 1)"
    assert f"/Size {out['object_count'] + 1}" in doc, (
        f"trailer /Size must be object_count+1 ({out['object_count'] + 1})"
    )


def _assert_objects(out: dict[str, Any]) -> None:
    """(C) the four standard-14 fonts + object-count self-consistency."""
    doc = out["document"]
    for base in _FONT_BASES:
        assert base in doc, f"standard-14 font must be declared: {base}"
    assert "/Encoding /WinAnsiEncoding" in doc, "fonts use WinAnsi encoding"
    # object_count counts indirect objects (excludes free obj 0). The
    # body declares "<n> 0 obj" for each; the highest must equal it.
    assert f"{out['object_count']} 0 obj" in doc, (
        f"the body must declare object {out['object_count']}"
    )
    assert f"{out['object_count'] + 1} 0 obj" not in doc, (
        "the body must not declare an object beyond object_count"
    )
    # A document with only fonts (no alpha ExtGState) has 8 objects;
    # this scene may add ExtGStates for theme overlays, so >= 8.
    assert out["object_count"] >= 8, (
        f"at least catalog+pages+page+contents+4 fonts = 8 objects; got {out['object_count']}"
    )


def _assert_content(out: dict[str, Any]) -> None:
    """(D) primary-colour fills, rounded curves, the border stroke, and
    embedded document text."""
    doc = out["document"]
    # Three primary-colour swatch fills (DeviceRGB, exact).
    assert "1 0 0 rg" in doc, "red swatch fill"
    assert "0 1 0 rg" in doc, "green swatch fill"
    assert "0 0 1 rg" in doc, "blue swatch fill"
    # Rounded corners -> bezier path (not a sharp `re`) for the swatches.
    assert " c\n" in doc, "bezier corner operator (rounded rects)"
    assert " m\n" in doc, "path moveto (rounded rects)"
    # The bordered caption box strokes.
    assert " w\n" in doc, "stroke line width set (bordered box)"
    assert "S\n" in doc, "stroke operator (bordered box border)"
    # Embedded text: the title, a body line, the caption.
    assert "(Quarterly Report) Tj" in doc, "title text embedded"
    assert "Tj" in doc and "BT\n" in doc and "ET\n" in doc, "text objects present"
    assert "/F2 " in doc, "bold font (F2) used by the title"


def _assert_live_tracking(tf: RpcSubprocess) -> None:
    """(E) the watermark text in the exported PDF flips with a toggle
    click. Observed-state gated: poll the export until the flip lands
    (no fixed sleep)."""
    before = tf.export_pdf()["document"]
    start = _watermark(before)
    assert start is not None, "the document must carry exactly one watermark"
    target = "FINAL" if start == "DRAFT" else "DRAFT"

    # One click flips the bit; the paint that reflects it is async, so
    # poll the export until the new watermark appears.
    _drive_redraw(tf)

    def flipped() -> Any:
        doc = tf.export_pdf()["document"]
        return doc if _watermark(doc) == target else None

    after = wait_until(flipped, desc=f"watermark flips {start} -> {target}")
    assert _watermark(after) == target, f"export must reflect the new watermark {target}"
    assert f"({start}) Tj" not in after, "the old watermark must be gone after the flip"


def _assert_page_sizes(tf: RpcSubprocess) -> None:
    """(F) scene-bounds default, named letter / a4, landscape swap."""
    default = tf.export_pdf()
    assert (default["page_width_pt"], default["page_height_pt"]) == (_WIN_W, _WIN_H), (
        f"the default page mirrors the scene bounds ({_WIN_W}x{_WIN_H}); "
        f"got {default['page_width_pt']}x{default['page_height_pt']}"
    )

    letter = tf.export_pdf(page="letter")
    assert (letter["page_width_pt"], letter["page_height_pt"]) == (612, 792), "US Letter portrait"
    assert "/MediaBox [0 0 612 792]" in letter["document"]

    a4 = tf.export_pdf(page="a4")
    assert (a4["page_width_pt"], a4["page_height_pt"]) == (595, 842), "A4 portrait"

    landscape = tf.export_pdf(page="letter", orientation="landscape")
    assert (landscape["page_width_pt"], landscape["page_height_pt"]) == (792, 612), (
        "landscape swaps width and height"
    )
    # Case-insensitive page name.
    upper = tf.export_pdf(page="A4")
    assert (upper["page_width_pt"], upper["page_height_pt"]) == (595, 842), "page name case-insensitive"


def _assert_param_errors(tf: RpcSubprocess) -> None:
    """(G) unknown page size / orientation are typed -32602 errors."""
    for params, tag in (
        ({"page": "tabloid"}, "UnknownPageSize"),
        ({"page": "a4", "orientation": "sideways"}, "UnknownOrientation"),
    ):
        raised = False
        try:
            tf.request("scene/export_pdf", params)
        except RpcError as exc:
            raised = True
            assert exc.code == -32602, f"{tag} must be -32602; got {exc.code}"
            assert exc.data == tag, f"error.data must be {tag!r}; got {exc.data!r}"
        assert raised, f"{params} must be rejected"


def _assert_unknown_window(tf: RpcSubprocess) -> None:
    """(H) a non-existent window id is rejected at dispatch entry
    (-32602), categorically different from NoPaintScene."""
    raised = False
    try:
        tf.export_pdf(window="ghost-window-does-not-exist")
    except RpcError as exc:
        raised = True
        assert exc.code == -32602, f"unknown window must be -32602; got {exc.code}"
    assert raised, "querying an unknown window id must raise, not succeed"


def body() -> None:
    with RpcSubprocess("hello-pdf-export", boot_grace=1.5) as tf:
        # ── (A) Wire shape ───────────────────────────────────────
        out = _wait_available(tf)
        _assert_wire_shape(out)

        # ── (B) PDF skeleton ─────────────────────────────────────
        _assert_skeleton(out)

        # ── (C) Objects ──────────────────────────────────────────
        _assert_objects(out)

        # ── (D) Content operators + text ─────────────────────────
        _assert_content(out)

        # ── (E) Live scene tracking (watermark flips) ────────────
        _assert_live_tracking(tf)

        # ── (F) Page size params ─────────────────────────────────
        _assert_page_sizes(tf)

        # The default window ("main") and the explicit id address the
        # same primary window.
        main = tf.export_pdf(window="main")
        _assert_wire_shape(main)
        assert (main["page_width_pt"], main["page_height_pt"]) == (_WIN_W, _WIN_H), (
            "explicit window='main' addresses the same primary window"
        )

        # ── (G) Param errors ─────────────────────────────────────
        _assert_param_errors(tf)

        # ── (H) Unknown-window rejection ─────────────────────────
        _assert_unknown_window(tf)


if __name__ == "__main__":
    sys.exit(run_demo("R908 own-renderer Scene -> PDF export", body))
