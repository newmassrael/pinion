//! `scene/screenshot` RPC method dispatch (§5.12 method 7 of 7, R16 slice 16).
//!
//! v0 is a **typed placeholder**: every call surfaces
//! [`ScreenshotError::RenderBackendUnavailable`] because pixel
//! rendering is gated on §5.16 (`pinion-render-rhi` thin RHI +
//! `pinion-render-shader` naga emit), which has not yet shipped.
//!
//! The reason this slice exists at all is *interface completeness* —
//! §5.12 ratifies 7 typed methods and AI clients are best served by a
//! uniform error path for "this is the method shape, the backend is
//! not yet wired" versus "this method does not exist". The
//! [`Screenshot`] payload struct (`width`/`height`/`pixels_rgba8`)
//! freezes the response shape so the future renderer drop-in lands
//! without disturbing the wire schema.

use pinion_core::Scene;

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

/// Reasons [`screenshot`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path carries an unsupported tail.
    UnsupportedPath,
    /// §5.16 RHI / wgpu fallback backend is not wired yet. v0 returns
    /// this for every well-formed request; a later slice replaces it
    /// with actual pixel output.
    RenderBackendUnavailable,
}

impl From<PathError> for ScreenshotError {
    fn from(err: PathError) -> Self {
        ScreenshotError::Path(err)
    }
}

/// Render a pixel screenshot of `scene`. v0 always errors with
/// [`ScreenshotError::RenderBackendUnavailable`] after validating the
/// path shape.
///
/// # Errors
///
/// See [`ScreenshotError`] for the failure modes. v0 surfaces
/// [`ScreenshotError::RenderBackendUnavailable`] for every well-formed
/// request.
pub fn screenshot(scene: &Scene, raw_path: &str) -> Result<Screenshot, ScreenshotError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    if !resolved.scene_path.is_empty() {
        return Err(ScreenshotError::UnsupportedPath);
    }

    let _ = scene;
    Err(ScreenshotError::RenderBackendUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::CountedExternal;
    use pinion_core::scene::ExternalNode;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn well_formed_request_returns_render_backend_unavailable() {
        let scene = counted_scene(0);
        let err = screenshot(&scene, "").unwrap_err();
        assert_eq!(err, ScreenshotError::RenderBackendUnavailable);
    }

    #[test]
    fn window_prefix_short_circuits_then_render_backend_unavailable() {
        let scene = counted_scene(0);
        let err = screenshot(&scene, "/window[main]").unwrap_err();
        assert_eq!(err, ScreenshotError::RenderBackendUnavailable);
    }

    #[test]
    fn scene_path_tail_rejected_before_backend_check() {
        let scene = counted_scene(0);
        let err = screenshot(&scene, "/external/count").unwrap_err();
        assert_eq!(err, ScreenshotError::UnsupportedPath);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = counted_scene(0);
        let err = screenshot(&scene, "/window[main").unwrap_err();
        assert_eq!(err, ScreenshotError::Path(PathError::MalformedPrefix));
    }
}
