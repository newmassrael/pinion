//! `scene/export_pdf` RPC method dispatch — R908 §3 §5.53 §5.7.
//!
//! Renders the addressed window's current paint scene to a vector PDF
//! document and returns it (plus its structural facts) over JSON-RPC, so
//! an AI agent can export "what the window currently shows" to a
//! portable document without scraping pixels.
//!
//! This is the AI-first surface of the [`pinion_pdf`] own-renderer
//! pipeline. It is the read peer of `scene/layout`: both consume the
//! same per-window [`DispatchContext::last_paint_scene`] borrow
//! (R890.1 — the projection runs only on this arm, never eagerly), so
//! the exported document describes the same frame `scene/layout` /
//! `scene/snapshot from: paint` report. Where `scene/screenshot` still
//! returns `RenderBackendUnavailable` (pixel rendering is gated on the
//! §5.16 RHI), PDF needs no rasterizer — the structured scene already
//! carries the geometry, fills, and text a vector page is built from
//! (the inv #1 "structured scene, no opaque paint" payoff).
//!
//! ## Wire shape
//!
//! Request (all params optional):
//!
//! ```json
//! {
//!   "jsonrpc": "2.0", "method": "scene/export_pdf",
//!   "params": { "page": "a4", "orientation": "landscape" }, "id": 1
//! }
//! ```
//!
//! - `page` — `"letter"` | `"a4"` (case-insensitive). Absent: the page
//!   is sized to the scene's own pixel bounds (WYSIWYG), one scene pixel
//!   per PDF point.
//! - `orientation` — `"portrait"` | `"landscape"`. Applies only to a
//!   named `page`; ignored for the WYSIWYG scene-bounds page (which
//!   already has the window's shape).
//!
//! Response:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0", "id": 1,
//!   "result": {
//!     "page_count": 1, "page_width_pt": 595, "page_height_pt": 842,
//!     "object_count": 8, "byte_len": 612,
//!     "document": "%PDF-1.7\n..."
//!   }
//! }
//! ```
//!
//! `document` is the complete PDF, fully ASCII by construction (binary
//! bytes octal-escaped), so it round-trips losslessly through the JSON
//! string. `object_count` / `byte_len` let a client sanity-check the
//! document without re-parsing it.
//!
//! ## Side-effect contract
//!
//! Read-only. Rendering the paint scene to PDF neither mutates the scene
//! nor schedules a repaint; like `scene/layout` it is a pure projection
//! of the stored frame.

use pinion_core::Scene;
use pinion_core::print::Orientation;
use pinion_pdf::{PageSize, RenderedPdf, render_scene};
use serde::{Deserialize, Serialize};

/// Request params for `scene/export_pdf`. Both fields optional; an empty
/// object exports the scene-bounds (WYSIWYG) page.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportPdfParams {
    /// Named page size: `"letter"` | `"a4"` (case-insensitive). Absent
    /// sizes the page to the scene's own pixel bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// `"portrait"` | `"landscape"`. Applies only to a named `page`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<String>,
}

/// Reasons [`export_pdf`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportPdfError {
    /// The addressed window has not produced a paint scene yet (winit
    /// has not rendered a frame, or the embedder did not install a
    /// `last_paint_scene` snapshot). The render peer of
    /// `scene/layout`'s `NoLastPaintLayout`.
    NoPaintScene,
    /// `params.page` was not a recognized page name. Carries the
    /// supplied token so an AI client can correct it.
    UnknownPageSize(String),
    /// `params.orientation` was not `"portrait"` or `"landscape"`.
    UnknownOrientation(String),
}

/// The exported document plus the structural facts a caller needs
/// without re-parsing the PDF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportPdfOutcome {
    /// Pages in the document (one in this slice).
    pub page_count: u32,
    /// `MediaBox` width in points.
    pub page_width_pt: u32,
    /// `MediaBox` height in points.
    pub page_height_pt: u32,
    /// Indirect objects, excluding the free object 0.
    pub object_count: u32,
    /// Document length in bytes (`document.len()` — it is ASCII).
    pub byte_len: u32,
    /// The complete PDF document, ASCII.
    pub document: String,
}

impl From<&RenderedPdf> for ExportPdfOutcome {
    fn from(pdf: &RenderedPdf) -> Self {
        Self {
            page_count: pdf.page_count,
            page_width_pt: pdf.page_width_pt,
            page_height_pt: pdf.page_height_pt,
            object_count: pdf.object_count,
            byte_len: u32::try_from(pdf.byte_len()).unwrap_or(u32::MAX),
            document: pdf.document.clone(),
        }
    }
}

/// Render `last_paint_scene` to a PDF per `params`.
///
/// # Errors
///
/// See [`ExportPdfError`] for the failure surface.
pub fn export_pdf(
    last_paint_scene: Option<&Scene>,
    params: &ExportPdfParams,
) -> Result<ExportPdfOutcome, ExportPdfError> {
    let scene = last_paint_scene.ok_or(ExportPdfError::NoPaintScene)?;
    let page = resolve_page(scene, params)?;
    let pdf = render_scene(scene, page);
    Ok(ExportPdfOutcome::from(&pdf))
}

/// Resolve the requested [`PageSize`]: a named page (optionally
/// landscape-flipped), or — when `page` is absent — the scene's own
/// pixel bounds. Orientation is ignored for the scene-bounds page.
fn resolve_page(scene: &Scene, params: &ExportPdfParams) -> Result<PageSize, ExportPdfError> {
    let Some(name) = params.page.as_deref() else {
        return Ok(PageSize::from_scene_bounds(scene));
    };
    let base = match name.to_ascii_lowercase().as_str() {
        "letter" => PageSize::LETTER,
        "a4" => PageSize::A4,
        _ => return Err(ExportPdfError::UnknownPageSize(name.to_owned())),
    };
    match params.orientation.as_deref() {
        None => Ok(base),
        // Parse through the canonical `print::Orientation` vocabulary
        // (the read peer of its `as_str` wire form) so the
        // "portrait"/"landscape" tokens cannot drift from the print
        // spool's. Lowercase first to stay forgiving of caller casing.
        Some(o) => match Orientation::from_wire(&o.to_ascii_lowercase()) {
            Some(Orientation::Portrait) => Ok(base),
            Some(Orientation::Landscape) => Ok(base.landscape()),
            None => Err(ExportPdfError::UnknownOrientation(o.to_owned())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{BoxNode, Rect};
    use pinion_core::style::Color;

    fn boxed_scene() -> Scene {
        Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 320, 240),
            Color::rgb(0xff, 0xff, 0xff),
        ))
    }

    #[test]
    fn r908_missing_paint_scene_errors() {
        assert_eq!(
            export_pdf(None, &ExportPdfParams::default()).unwrap_err(),
            ExportPdfError::NoPaintScene,
        );
    }

    #[test]
    fn r908_scene_bounds_page_is_wysiwyg() {
        let scene = boxed_scene();
        let out = export_pdf(Some(&scene), &ExportPdfParams::default()).unwrap();
        assert_eq!(out.page_width_pt, 320);
        assert_eq!(out.page_height_pt, 240);
        assert_eq!(out.page_count, 1);
        assert!(out.document.starts_with("%PDF-1.7"));
        assert_eq!(out.byte_len as usize, out.document.len());
    }

    #[test]
    fn r908_named_page_letter_and_a4() {
        let scene = boxed_scene();
        let letter = export_pdf(
            Some(&scene),
            &ExportPdfParams {
                page: Some("letter".into()),
                orientation: None,
            },
        )
        .unwrap();
        assert_eq!((letter.page_width_pt, letter.page_height_pt), (612, 792));

        let a4 = export_pdf(
            Some(&scene),
            &ExportPdfParams {
                page: Some("A4".into()),
                orientation: None,
            },
        )
        .unwrap();
        assert_eq!((a4.page_width_pt, a4.page_height_pt), (595, 842));
    }

    #[test]
    fn r908_landscape_swaps_named_page() {
        let scene = boxed_scene();
        let out = export_pdf(
            Some(&scene),
            &ExportPdfParams {
                page: Some("letter".into()),
                orientation: Some("landscape".into()),
            },
        )
        .unwrap();
        assert_eq!((out.page_width_pt, out.page_height_pt), (792, 612));
    }

    #[test]
    fn r908_unknown_page_and_orientation_rejected() {
        let scene = boxed_scene();
        assert_eq!(
            export_pdf(
                Some(&scene),
                &ExportPdfParams {
                    page: Some("tabloid".into()),
                    orientation: None
                },
            )
            .unwrap_err(),
            ExportPdfError::UnknownPageSize("tabloid".into()),
        );
        assert_eq!(
            export_pdf(
                Some(&scene),
                &ExportPdfParams {
                    page: Some("a4".into()),
                    orientation: Some("sideways".into()),
                },
            )
            .unwrap_err(),
            ExportPdfError::UnknownOrientation("sideways".into()),
        );
    }

    #[test]
    fn r908_orientation_ignored_for_scene_bounds_page() {
        // No named page -> scene bounds; a stray orientation is a no-op,
        // not an error (the WYSIWYG page already has the window's shape).
        let scene = boxed_scene();
        let out = export_pdf(
            Some(&scene),
            &ExportPdfParams {
                page: None,
                orientation: Some("landscape".into()),
            },
        )
        .unwrap();
        assert_eq!((out.page_width_pt, out.page_height_pt), (320, 240));
    }
}
