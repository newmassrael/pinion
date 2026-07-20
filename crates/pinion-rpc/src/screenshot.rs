//! `scene/screenshot` RPC method types (§5.12 method 7 of 7).
//!
//! Carries the [`Screenshot`] response payload + [`ScreenshotError`]
//! taxonomy + the `validate_screenshot_path` path-shape SSOT. The
//! actual pixels are supplied by the embedder: the pinion-shell
//! `AppShell` renders the addressed window through
//! `VelloRenderer::capture_rgba8` (reading back the presented swapchain
//! texture, R1060 §5.16) and hands a [`Screenshot`] into the
//! `DispatchContext`; `handle_scene_screenshot` returns it (inline
//! `pixels_rgba8`, or `out_path` PNG file mode, R1061). When no embedder
//! supplied a frame (the headless / single-window dispatch entry, which
//! holds no live surface), the handler returns
//! [`ScreenshotError::RenderBackendUnavailable`].

use crate::path::{self, PathError};

/// Raw pixel screenshot payload (§5.12 method 7 output, frozen by R16
/// for future fill driven by §5.16).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    /// Pre-multiplied RGBA8 byte buffer, `width * height * 4` bytes.
    /// Empty in the [`Self::out_path`] file-output mode.
    pub pixels_rgba8: Vec<u8>,
    /// R1061 §5.12 — when the request carried `{out_path: "….png"}` the
    /// embedder wrote the captured frame to that file as PNG and
    /// `pixels_rgba8` is empty; the wire returns the path instead of a
    /// multi-MB pixel array. `None` = inline `pixels_rgba8` mode.
    pub out_path: Option<String>,
}

impl Screenshot {
    /// R1060 §5.12 — inline-pixel captured-frame payload. The struct is
    /// `#[non_exhaustive]`, so out-of-crate producers (the pinion-shell
    /// `AppShell` live-surface capture) build it through this
    /// constructor rather than a struct literal.
    #[must_use]
    pub fn new(width: u32, height: u32, pixels_rgba8: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels_rgba8,
            out_path: None,
        }
    }

    /// R1061 §5.12 — file-output payload: the captured frame was already
    /// written to `out_path` as PNG by the embedder, so `pixels_rgba8` is
    /// empty and the wire stays small regardless of window size.
    #[must_use]
    pub fn new_file(width: u32, height: u32, out_path: String) -> Self {
        Self {
            width,
            height,
            pixels_rgba8: Vec::new(),
            out_path: Some(out_path),
        }
    }
}

/// Reasons a `scene/screenshot` dispatch can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path carries an unsupported tail.
    UnsupportedPath,
    /// No embedder-supplied frame for this dispatch — the headless /
    /// single-window entry holds no live surface to read back, so the
    /// well-formed request cannot be answered with pixels.
    RenderBackendUnavailable,
}

impl From<PathError> for ScreenshotError {
    fn from(err: PathError) -> Self {
        ScreenshotError::Path(err)
    }
}

/// R1062 §5.12 — validate the `scene/screenshot` `path` param shape: an
/// optional `/window[id]/` prefix (consumed upstream by the `AppShell`
/// entry to pick which surface to capture) followed by an EMPTY
/// scene-path tail. The single source of truth for the path-shape
/// contract, called by `handle_scene_screenshot` (which then returns the
/// embedder's pre-captured pixels or `RenderBackendUnavailable`).
///
/// # Errors
///
/// [`ScreenshotError::Path`] for a malformed `/window[...]` prefix;
/// [`ScreenshotError::UnsupportedPath`] when a non-empty scene-path tail
/// is present (a screenshot addresses a whole window, not a sub-node).
pub(crate) fn validate_screenshot_path(raw_path: &str) -> Result<(), ScreenshotError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;
    if resolved.scene_path.is_empty() {
        Ok(())
    } else {
        Err(ScreenshotError::UnsupportedPath)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_is_valid() {
        assert_eq!(validate_screenshot_path(""), Ok(()));
    }

    #[test]
    fn window_prefix_with_empty_tail_is_valid() {
        assert_eq!(validate_screenshot_path("/window[main]"), Ok(()));
    }

    #[test]
    fn scene_path_tail_is_unsupported() {
        assert_eq!(
            validate_screenshot_path("/external/count"),
            Err(ScreenshotError::UnsupportedPath)
        );
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        assert_eq!(
            validate_screenshot_path("/window[main"),
            Err(ScreenshotError::Path(PathError::MalformedPrefix))
        );
    }
}
