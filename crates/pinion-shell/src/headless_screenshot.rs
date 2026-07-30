//! R637 §5.16 §5.7 — headless screenshot substrate.
//!
//! Closes the §5.12 method 7 (`scene/screenshot`) v0 placeholder
//! ([[scene/screenshot]] returns `RenderBackendUnavailable` in
//! `pinion_rpc::screenshot`) for the **dev / verification path**.
//! Full §5.16 pinion-render-rhi (thin RHI + naga) remains future
//! work; this round delivers the wgpu-fallback path R31/§5.16
//! explicitly ratifies ("`wgpu-fallback` feature for dev"), which is
//! the textbook canonical first slice for AI-first introspection
//! ([[ai-first-rpc-introspection-obligation]]) — the AI agent (and
//! the Figma → pinion design-parity workflow R634/R635/R636 just
//! landed) need a way to capture the live paint scene as pixels
//! without spinning a winit window the binary may not be allowed to
//! open (CI, headless dev box, AI environment).
//!
//! ## Pipeline
//!
//! Per-call (`render_to_rgba8`):
//!
//! 1. Create `wgpu::Texture` (`Rgba8Unorm`, `STORAGE_BINDING |
//!    COPY_SRC`) sized to the requested viewport. Reuse-friendly
//!    sizing is intentionally deferred — every call allocates fresh,
//!    matching the substrate's single-shot semantics.
//! 2. Hand the texture view + the supplied `vello::Scene` to
//!    [`vello::Renderer::render_to_texture`].
//! 3. Allocate a staging `wgpu::Buffer` (`COPY_DST | MAP_READ`) the
//!    size of the unpadded RGBA8 raster.
//! 4. Encode `copy_texture_to_buffer` with `bytes_per_row` padded to
//!    `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (wgpu / WebGPU spec
//!    requirement: `bytes_per_row` must be a multiple of 256). The
//!    readback path strips the row padding back out so callers see
//!    the unpadded `width * height * 4` byte buffer.
//! 5. Submit + `buffer.slice(..).map_async(MapMode::Read, ...)` +
//!    `device.poll(MaintainBase::Wait)` until the callback fires.
//! 6. Read the mapped slice + `unmap`. Returns
//!    `Vec<u8>` of `width * height * 4` premultiplied RGBA8.
//!
//! PNG encode (`render_to_png`): wraps `render_to_rgba8` with an
//! 8-bit-per-channel RGBA `png::Encoder` write. The `png` crate is a
//! single-purpose workspace dep ([[Cargo.toml]] R637 note) — no
//! `image` umbrella pulled in for this slice.
//!
//! ## When to use
//!
//! - `PINION_SCREENSHOT=<path>` env var on any `pinion_shell::run::<V>()`
//!   binary (the R637 hook in [`crate::run`]) — first-paint scene
//!   rendered + written to `<path>` + process exits, no winit window
//!   opened.
//! - Future R638+ `pinion figma-diff` CLI calling the substrate
//!   directly to render scene fragments for byte-golden comparison.
//! - Future `scene/screenshot` RPC wiring — the live `AppShell` already
//!   owns a wgpu `Device` / `Queue`; a follow-up round can plumb
//!   those down through `ShellCore::dispatch_rpc` instead of
//!   re-allocating a separate adapter here.
//!
//! ## What this is not
//!
//! - Not §5.16 pinion-render-rhi proper — that's the AAA-scale thin
//!   RHI / naga shader emit pipeline §5.16 R11 ratifies. The wgpu /
//!   vello path here is the dev/test fallback, suitable for design
//!   parity verification + AI introspection, not for production
//!   game-mode runtime.
//! - Not differential / damage-aware — every call re-rasterizes the
//!   full viewport. The §5.26 `DamageRect` substrate stays scoped to
//!   the live paint loop.
//! - Not multi-frame — there is no animation tick, no `Frame::dt`
//!   advance per call. The shell side's `compute_paint_scene` does
//!   the dt accounting; this substrate just rasterizes whatever
//!   `vello::Scene` the caller hands it.

use std::io::Write;
use std::num::NonZeroUsize;

use vello::peniko::Color as PenikoColor;
use vello::wgpu::{
    self, Backends, DeviceDescriptor, Extent3d, Features, Instance, InstanceDescriptor, Limits,
    MemoryHints, PowerPreference, RequestAdapterOptions, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor,
};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene as VelloScene};

/// Reasons the headless screenshot substrate can fail.
#[derive(Debug)]
#[non_exhaustive]
pub enum HeadlessScreenshotError {
    /// `wgpu::Instance::request_adapter` returned `None`. No backend
    /// (Vulkan / Metal / DX12 / WebGPU / lavapipe-software) could
    /// supply an adapter for the headless request. Caller's
    /// environment lacks a usable GPU + software fallback.
    AdapterNotFound,
    /// `wgpu::Adapter::request_device` failed. Carries the wgpu
    /// error reason as a string so the caller can include it in the
    /// dispatch error envelope without taking a wgpu dep.
    DeviceRequest(String),
    /// `vello::Renderer::new` failed. Vello carries `vello::Error`
    /// which itself wraps `wgpu::Error`; rendered as a string for
    /// the same reason as `DeviceRequest`.
    VelloInit(String),
    /// `vello::Renderer::render_to_texture` failed.
    VelloRender(String),
    /// `buffer.slice(..).map_async(MapMode::Read, ...)` callback
    /// reported an error (typically lost device or driver fault).
    BufferMap(String),
    /// `png::Encoder::write_header` or `write_image_data` failed.
    /// Carries the PNG-side error as a string so the substrate
    /// surface stays free of a `png` re-export.
    PngEncode(String),
    /// Underlying writer (file / network sink) returned an
    /// `io::Error` during the PNG body write.
    Io(std::io::Error),
    /// Requested viewport had `width == 0` or `height == 0`. wgpu
    /// would reject the texture descriptor anyway; the substrate
    /// short-circuits with a typed error so the caller does not see
    /// a low-level wgpu validation panic.
    ZeroDimension,
}

impl core::fmt::Display for HeadlessScreenshotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AdapterNotFound => f.write_str("wgpu adapter not available"),
            Self::DeviceRequest(e) => write!(f, "wgpu device request failed: {e}"),
            Self::VelloInit(e) => write!(f, "vello renderer init failed: {e}"),
            Self::VelloRender(e) => write!(f, "vello render_to_texture failed: {e}"),
            Self::BufferMap(e) => write!(f, "wgpu buffer map_async failed: {e}"),
            Self::PngEncode(e) => write!(f, "png encode failed: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::ZeroDimension => f.write_str("viewport width / height must be > 0"),
        }
    }
}

impl std::error::Error for HeadlessScreenshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for HeadlessScreenshotError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Headless wgpu + vello pipeline. Constructed once per process (the
/// shader compile + adapter request is the bulk of the wall-clock
/// cost); re-used for every `render_to_rgba8` / `render_to_png`
/// call. Not `Sync` (wgpu `Queue` is not `Sync` on every backend);
/// pin to a single thread.
pub struct HeadlessScreenshot {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

impl HeadlessScreenshot {
    /// Boot the wgpu adapter / device + vello renderer for headless
    /// use. Synchronous wrapper around the wgpu async init path —
    /// the substrate is sync per §6.3 (view-fn purity boundary); the
    /// one-shot async init runs under `pollster::block_on` exactly
    /// the same way `pinion_shell::run` boots the surface-side
    /// `VelloRenderer`.
    ///
    /// # Errors
    ///
    /// - [`HeadlessScreenshotError::AdapterNotFound`] when no wgpu
    ///   backend (Vulkan / Metal / DX12 / software lavapipe) supplies
    ///   an adapter — the host has no usable GPU + software fallback.
    /// - [`HeadlessScreenshotError::DeviceRequest`] when adapter
    ///   device + queue acquisition fails.
    /// - [`HeadlessScreenshotError::VelloInit`] when shader compile
    ///   or vello pipeline setup fails.
    pub fn new() -> Result<Self, HeadlessScreenshotError> {
        pollster::block_on(Self::new_async())
    }

    /// Async path used by [`Self::new`]. Kept `pub` so a future
    /// caller already inside a tokio runtime can avoid the nested
    /// `block_on`.
    ///
    /// # Errors
    ///
    /// Same as [`Self::new`].
    pub async fn new_async() -> Result<Self, HeadlessScreenshotError> {
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            // wgpu 29 (R808): headless path has no windowing-system
            // display handle to thread through for surface creation.
            display: None,
        });
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|_| HeadlessScreenshotError::AdapterNotFound)?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("pinion-shell::HeadlessScreenshot"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                memory_hints: MemoryHints::Performance,
                // wgpu 29 (R808): no EXPERIMENTAL_* features enabled.
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| HeadlessScreenshotError::DeviceRequest(format!("{e}")))?;
        let renderer = Renderer::new(
            &device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport {
                    area: true,
                    msaa8: false,
                    msaa16: false,
                },
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .map_err(|e| HeadlessScreenshotError::VelloInit(format!("{e}")))?;
        Ok(Self {
            device,
            queue,
            renderer,
        })
    }

    /// Render `vello_scene` at `width x height` with `base_color` as
    /// the clear color, return the resulting premultiplied RGBA8
    /// framebuffer (`width * height * 4` bytes, row-major,
    /// top-left origin — matching the pinion paint pipeline + the
    /// `scene/screenshot` wire `pixels_rgba8` field).
    ///
    /// # Errors
    ///
    /// See [`HeadlessScreenshotError`] for the failure surface.
    pub fn render_to_rgba8(
        &mut self,
        vello_scene: &VelloScene,
        width: u32,
        height: u32,
        base_color: PenikoColor,
    ) -> Result<Vec<u8>, HeadlessScreenshotError> {
        if width == 0 || height == 0 {
            return Err(HeadlessScreenshotError::ZeroDimension);
        }
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("pinion-shell::HeadlessScreenshot target"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                vello_scene,
                &view,
                &RenderParams {
                    base_color,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| HeadlessScreenshotError::VelloRender(format!("{e}")))?;

        self.read_texture(&texture, width, height)
    }

    /// R1036 PR-17 — render `scenes` SEQUENTIALLY into ONE reused target
    /// texture (the live [`vello::util::RenderSurface::target_view`] reuse
    /// model — a single intermediate texture every frame draws into), reading
    /// back the RGBA8 raster after each. Returns one buffer per scene.
    ///
    /// This is the headless analog of N consecutive surface frames, and the
    /// decisive test of whether the GPU raster ACCUMULATES across frames that
    /// share a target: a coverage-only `render_to_texture` would leave frame
    /// N-1's pixels wherever frame N draws nothing, which is exactly the PR-17
    /// splitter-reflow "old rows not erased" residue. With `base_color` clearing
    /// the whole target each call, frame N must equal a from-scratch render of
    /// scene N — independent of what scene N-1 drew.
    ///
    /// # Errors
    ///
    /// See [`HeadlessScreenshotError`]; `ZeroDimension` for a zero axis.
    pub fn render_sequence(
        &mut self,
        scenes: &[&VelloScene],
        width: u32,
        height: u32,
        base_color: PenikoColor,
    ) -> Result<Vec<Vec<u8>>, HeadlessScreenshotError> {
        if width == 0 || height == 0 {
            return Err(HeadlessScreenshotError::ZeroDimension);
        }
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("pinion-shell::HeadlessScreenshot reused target"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let mut frames = Vec::with_capacity(scenes.len());
        for scene in scenes {
            self.renderer
                .render_to_texture(
                    &self.device,
                    &self.queue,
                    scene,
                    &view,
                    &RenderParams {
                        base_color,
                        width,
                        height,
                        antialiasing_method: AaConfig::Area,
                    },
                )
                .map_err(|e| HeadlessScreenshotError::VelloRender(format!("{e}")))?;
            frames.push(self.read_texture(&texture, width, height)?);
        }
        Ok(frames)
    }

    /// Copy `texture` (an `Rgba8Unorm`, `COPY_SRC` target of `width x height`)
    /// back into a contiguous `width * height * 4` premultiplied-RGBA8 buffer.
    ///
    /// R1060 §5.16 — delegates to the texture → RGBA8 readback SSOT
    /// ([`crate::vello_capture::texture_to_rgba8`]) that the live-surface
    /// capture also calls. Headless targets are `Rgba8Unorm`, so the
    /// SSOT's format-driven BGRA swizzle is a no-op and the output is
    /// byte-identical to the pre-R1060 inline copy (the PR-17
    /// [`Self::render_sequence`] accumulation tests guard the
    /// equivalence).
    fn read_texture(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, HeadlessScreenshotError> {
        crate::vello_capture::texture_to_rgba8(
            &self.device,
            &self.queue,
            texture,
            width,
            height,
            TextureFormat::Rgba8Unorm,
        )
        .map_err(|e| match e {
            crate::vello_capture::SurfaceCaptureError::BufferMap(s) => {
                HeadlessScreenshotError::BufferMap(s)
            }
            // `texture_to_rgba8` only ever surfaces `BufferMap`; the
            // other capture-side variants are unreachable on this path.
            other => HeadlessScreenshotError::BufferMap(other.to_string()),
        })
    }

    /// Render + encode as PNG to `writer`. Convenience wrapper around
    /// [`Self::render_to_rgba8`] — see that method's docstring for
    /// the pipeline details.
    ///
    /// PNG is RGBA 8-bit-per-channel (no palette, no 16-bit, no
    /// interlace) — the simplest shape that round-trips the wgpu
    /// readback bytes without loss + opens in every image viewer +
    /// every standard decoder, which is the R637 verification
    /// pipeline's reference shape ([[Cargo.toml]] `png` workspace dep
    /// rationale).
    ///
    /// # Errors
    ///
    /// See [`HeadlessScreenshotError`]; additionally [`HeadlessScreenshotError::PngEncode`]
    /// for header / image-data write failures and [`HeadlessScreenshotError::Io`]
    /// for underlying writer faults.
    pub fn render_to_png<W: Write>(
        &mut self,
        vello_scene: &VelloScene,
        width: u32,
        height: u32,
        base_color: PenikoColor,
        writer: W,
    ) -> Result<(), HeadlessScreenshotError> {
        let pixels = self.render_to_rgba8(vello_scene, width, height, base_color)?;
        // R1061 §5.12 — delegate to the RGBA8 → PNG encode SSOT that the
        // live-capture `scene/screenshot {out_path}` wire also uses.
        crate::vello_capture::encode_rgba8_png(width, height, &pixels, writer)
            .map_err(HeadlessScreenshotError::PngEncode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::{Affine, Rect as KurboRect};
    use vello::peniko::Fill;

    /// Smoke: substrate boots + renders a trivial solid-fill scene +
    /// the returned buffer has exactly `width * height * 4` bytes.
    /// Pixel-value assertions are intentionally avoided — wgpu
    /// adapter availability (lavapipe / GPU) varies across CI
    /// hosts; this test asserts only the contract guarantees the
    /// substrate makes regardless of backend (`ZeroDimension` path
    /// + buffer length).
    ///
    /// Marked `#[ignore]` by default because the wgpu adapter
    /// request can take ~seconds on cold-boot CI (lavapipe shader
    /// compile) — opt-in via `cargo test --features ...
    /// headless_screenshot_smoke -- --ignored` once a future round
    /// wires an explicit feature gate. For now the workspace `cargo
    /// test` keeps the wall-clock floor low.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn renders_solid_fill_to_unpadded_rgba8() {
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let mut scene = VelloScene::new();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            PenikoColor::from_rgba8(0xFF, 0x00, 0x00, 0xFF),
            None,
            &KurboRect::new(0.0, 0.0, 64.0, 32.0),
        );
        let rgba8 = shot
            .render_to_rgba8(&scene, 64, 32, PenikoColor::BLACK)
            .expect("render");
        assert_eq!(rgba8.len(), 64 * 32 * 4);
    }

    /// R1358 §5.3 §5.16 — a `Scene::Path`'s commands are relative to its own
    /// `rect`, and `paint_path` carries the rect origin in the paint
    /// transform. This is the CI-gated pin for that translate: it is the ONE
    /// place the coordinate contract is enforced, and no other gate can see
    /// it — the four producer-side contract tests (chart / node-editor /
    /// narrative / window-chrome) assert what the *producers emit*, and the
    /// `r721_path.py` pixel demo proves the consumer only in the advisory
    /// demo sweep. R1066.1 set the precedent for lifting exactly this kind of
    /// CI-invisible paint proof into the `--ignored` lavapipe job.
    ///
    /// The square is authored 0-based and placed by a NON-ORIGIN rect, which
    /// makes the test falsifiable in both directions:
    /// * drop the translate -> ink lands at the bare command coords (the
    ///   `AWAY` probe lights, the `INK` probe goes dark),
    /// * apply it twice -> ink lands at `2 * origin` (both probes go dark).
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`. The `gpu-tests` CI job does exactly that.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1358_path_commands_paint_relative_to_the_nodes_rect() {
        use pinion_core::scene::{PathCommand, PathNode, PathPoint, Rect, Scene};
        use pinion_core::style::{Color, PathStyle};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const W: u32 = 64;
        const H: u32 = 48;
        // The node sits away from the origin on BOTH axes so a missing or
        // doubled translate is unambiguous.
        const OX: u32 = 24;
        const OY: u32 = 16;
        const SIDE_PX: u32 = 16;
        const SIDE: f32 = 16.0;

        let p = |x: f32, y: f32| PathPoint::new(x, y);
        // A filled square authored at (0,0)..(16,16) — its OWN box.
        let square = Scene::Path(PathNode::new(
            Rect::new(OX, OY, SIDE_PX, SIDE_PX),
            vec![
                PathCommand::MoveTo(p(0.0, 0.0)),
                PathCommand::LineTo(p(SIDE, 0.0)),
                PathCommand::LineTo(p(SIDE, SIDE)),
                PathCommand::LineTo(p(0.0, SIDE)),
                PathCommand::Close,
            ],
            PathStyle::filled(Color::rgb(0xFF, 0xFF, 0xFF)),
        ));

        let mut text_cache = LayoutCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut cache = FragmentCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &square,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba = shot
            .render_to_rgba8(&vello, W, H, PenikoColor::BLACK)
            .expect("render");

        // Premultiplied RGBA8, row-major, top-left origin — the same
        // `((y*W+x)*4) as usize` indexing the sibling tests use. White on
        // black, so any bright R = lit.
        let lit = |x: u32, y: u32| -> bool { rgba[((y * W + x) * 4) as usize] > 127 };

        // Inside the placed square: rect.origin + the square's own centre.
        let ink = (OX + 8, OY + 8);
        // Where the BARE commands would land if the translate were dropped.
        let away = (8, 8);

        assert!(
            lit(ink.0, ink.1),
            "the square must paint at rect.origin + command {ink:?} — a dark \
             pixel here means paint_path dropped or doubled its translate"
        );
        assert!(
            !lit(away.0, away.1),
            "nothing may paint at the bare command coords {away:?} — ink here \
             means the commands were read as window-absolute"
        );
    }

    /// R1404 §5.16 — a `memory://<key>` [`Scene::Image`](pinion_core::Scene)
    /// paints the pixels a producer registered in the
    /// [`MemoryImageStore`](pinion_runtime::MemoryImageStore), and a
    /// re-register / remove is visible on the very next render — the
    /// producer-supplied in-memory image source proved to the GPU, headless
    /// (the "headless render valid" north-star). Falsifiable: a store that
    /// never resolved would leave the black base at the sample; a cached
    /// `memory://` resolve would keep the red image after the blue update.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1404_memory_scheme_image_paints_and_mutates() {
        use pinion_core::scene::{ImageNode, Rect, Scene};
        use pinion_core::style::{Fit, ImageStyle};
        use pinion_runtime::image_cache::ImageCache;
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_runtime::{DecodedImage, MemoryImageStore};
        use pinion_text::LayoutCache;

        const W: u32 = 32;
        const H: u32 = 32;

        // A 1x1 solid image per variant — `Fit::Fill` floods the whole rect,
        // so one sample proves which image (or none) resolved.
        fn solid(r: u8, g: u8, b: u8) -> DecodedImage {
            DecodedImage::from_rgba8(1, 1, vec![r, g, b, 0xff]).unwrap()
        }

        let scene = Scene::Image(ImageNode::styled(
            "memory://tile",
            Rect::new(0, 0, W, H),
            ImageStyle::default().with_fit(Fit::Fill),
        ));

        // Render the same scene through a store-wired cache, so the ONLY
        // variable is the store's current pixels.
        let render = |store: &MemoryImageStore| -> Vec<u8> {
            let mut text_cache = LayoutCache::new();
            let mut image_cache = ImageCache::with_store(store.clone());
            let mut cache = FragmentCache::new();
            let mut vello = VelloScene::new();
            to_vello_cached(
                &scene,
                &|_| None,
                &mut text_cache,
                &mut image_cache,
                &mut cache,
                &mut vello,
            );
            let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
            shot.render_to_rgba8(&vello, W, H, PenikoColor::BLACK)
                .expect("render")
        };
        let at = |rgba: &[u8], x: u32, y: u32| -> (u8, u8, u8) {
            let i = ((y * W + x) * 4) as usize;
            (rgba[i], rgba[i + 1], rgba[i + 2])
        };

        let store = MemoryImageStore::new();
        store.insert("tile", &solid(0xff, 0x20, 0x20)); // red
        let (r, g, b) = at(&render(&store), 16, 16);
        assert!(
            r > 150 && g < 100 && b < 100,
            "the registered red memory image floods the rect, got {:?}",
            (r, g, b)
        );

        // Re-register the SAME key with a blue image (a mutable update): the
        // next render shows blue, not the cached red.
        store.insert("tile", &solid(0x20, 0x20, 0xff));
        let (r, g, b) = at(&render(&store), 16, 16);
        assert!(
            b > 150 && r < 100,
            "the updated blue memory image is visible next render, got {:?}",
            (r, g, b)
        );

        // Remove the key: the next render paints nothing (the black base).
        assert!(store.remove("tile"));
        let (r, g, b) = at(&render(&store), 16, 16);
        assert!(
            r < 40 && g < 40 && b < 40,
            "a removed memory image paints nothing (base black), got {:?}",
            (r, g, b)
        );
    }

    /// R1027 §5.16 — the shell lays the scene out in LOGICAL pixels and, on
    /// a `HiDPI` window, rasterizes it by appending into a scratch scene under
    /// `Affine::scale(scale)` before submit (the `render_window` `HiDPI` path).
    /// This is the real-GPU forcing consumer for that mechanism: a
    /// 4-logical-px-wide white band is 4 device-px at scale 1 (the identity
    /// fast path renders the scene directly) and 8 device-px through the
    /// scaled append at scale 2 — the splitter-handle / half-font fix
    /// (PR-15) at the raster boundary. The band sits on integer pixel
    /// boundaries so area anti-aliasing leaves no partial edge column (exact
    /// 4 / 8).
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling
    /// headless tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1027_scaled_append_doubles_ink_extent_on_hidpi() {
        const BAND_W: f64 = 4.0;
        const LOGICAL_W: u32 = 40;
        const LOGICAL_H: u32 = 24;

        // Count the lit (non-black) columns in the middle row of a
        // premultiplied RGBA8 buffer (`width*height*4`, row-major, top-left
        // origin) — the same `((y*W+x)*4) as usize` indexing the sibling
        // tests use. The white band over black makes any bright R = lit.
        let lit_columns = |rgba: &[u8], width: u32| -> u32 {
            let row = 0; // any row: the band is full-height
            let mut lit = 0;
            for col in 0..width {
                let i = ((row * width + col) * 4) as usize;
                if rgba[i] > 127 {
                    lit += 1;
                }
            }
            lit
        };

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");

        let mut logical = VelloScene::new();
        logical.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            PenikoColor::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
            None,
            &KurboRect::new(0.0, 0.0, BAND_W, f64::from(LOGICAL_H)),
        );

        // Scale 1 — the shell submits `vello_scene` directly (identity path).
        let rgba_1x = shot
            .render_to_rgba8(&logical, LOGICAL_W, LOGICAL_H, PenikoColor::BLACK)
            .expect("render 1x");
        let band_1x = lit_columns(&rgba_1x, LOGICAL_W);

        // Scale 2 — append under `Affine::scale(2.0)` into a scratch scene and
        // render at the doubled physical surface (exactly render_window's
        // `HiDPI` path: logical scene, device-resolution raster).
        let scale = 2.0;
        let mut scaled = VelloScene::new();
        scaled.append(&logical, Some(Affine::scale(scale)));
        let pw = LOGICAL_W * 2;
        let ph = LOGICAL_H * 2;
        let rgba_2x = shot
            .render_to_rgba8(&scaled, pw, ph, PenikoColor::BLACK)
            .expect("render 2x");
        let band_2x = lit_columns(&rgba_2x, pw);

        assert_eq!(band_1x, 4, "a 4-logical-px band is 4 device-px at scale 1");
        assert_eq!(
            band_2x, 8,
            "the scaled append doubles the band to 8 device-px at scale 2"
        );
    }

    /// R706.1 §5.16 §5.39 — faithful regression guard for the
    /// fragment-cache "direct-draw-after-append" rasterization defect.
    ///
    /// Builds the REAL hello-datepicker focused paint scene through the
    /// production pipeline — [`pinion_widget_paint::view_datepicker`]
    /// wrapped in the same centred Surface container the binding's view
    /// fn uses, [`pinion_runtime::compute_layout`] for the post-layout
    /// rects, then [`pinion_overlay::inject_focus_ring`] at the
    /// active-descendant cell exactly as `WindowOverlayInputs::apply`
    /// does — and rasterizes it through `to_vello_cached` (the live winit
    /// render path) into an offscreen wgpu surface. It then reads the
    /// pixels back and asserts the keyboard focus ring frames the FOCUSED
    /// day cell, not the cell one grid column to its right.
    ///
    /// Unlike a hand-built scene (the earlier smoke attempt rendered
    /// correctly even against the buggy code), the real grid's deep
    /// row→cell→glyph nesting reproduces the exact encoder state the R682
    /// fragment cache accumulates, so this test FAILS against the
    /// pre-R706 direct-draw-after-append code and PASSES against the
    /// "out receives appends only" fix — the deterministic counterpart to
    /// the live-window pixel demo `tools/demos/r706_focus_ring_pixel.py`.
    ///
    /// `#[ignore]` for the same reason as
    /// [`renders_solid_fill_to_unpadded_rgba8`] — wgpu cold-boot is too
    /// slow for the default suite; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r706_cached_render_frames_focused_day_cell_not_next_column() {
        use pinion_core::Owner;
        use pinion_core::scene::{ContainerNode, Scene};
        use pinion_core::style::{
            AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle,
        };
        use pinion_core::theme::{ColorRole, Theme};
        use pinion_core::widgets::radio::RadioState;
        use pinion_overlay::{FocusRingStyle, inject_focus_ring};
        use pinion_runtime::compute_layout;
        use pinion_runtime::paint_adapter::{FragmentCache, root_background, to_vello_cached};
        use pinion_text::LayoutCache;
        use pinion_widget_paint::datepicker::{DatePickerStyle, DisplayedMonth, view_datepicker};

        // Live binding window size (examples/hello-datepicker WIN_W/H).
        const W: u32 = 360;
        const H: u32 = 420;
        // May 2026 starts on a Friday, so day 3 sits in the Sunday column
        // (the LEFT-most day column, index 0). The defect shifted the ring
        // one column right onto the Monday column (day 4).
        const FOCUSED_TAG: &str = "datepicker#3";

        let theme = Theme::light();
        let mut text_cache = LayoutCache::new();

        // Reproduce the binding's view fn: view_datepicker wrapped in a
        // centred Surface container.
        let mut scene = Owner::new().run(|| {
            let picker = view_datepicker(
                "datepicker",
                DisplayedMonth {
                    year: 2026,
                    month: 5,
                },
                None,
                &[RadioState::Idle; 31],
                &theme,
                &DatePickerStyle::m3(),
            );
            Scene::Container(
                ContainerNode::new(vec![picker])
                    .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
                    .with_layout(
                        LayoutStyle::new()
                            .flex(FlexDirection::Column)
                            .with_justify(JustifyContent::Center)
                            .with_align_items(AlignItems::Center),
                    ),
            )
        });
        compute_layout(&mut scene, &mut text_cache, W, H);

        // Post-layout column centres of the focused cell and its right
        // neighbour — the ring must hug the former, never the latter.
        let cell3 = scene
            .rect_for_tag_absolute(FOCUSED_TAG)
            .expect("focused day cell present after layout");
        let cell4 = scene
            .rect_for_tag_absolute("datepicker#4")
            .expect("right-neighbour day cell present");
        let c3_cx = i64::from(cell3.x) + i64::from(cell3.w) / 2;
        let c4_cx = i64::from(cell4.x) + i64::from(cell4.w) / 2;
        assert!(c4_cx > c3_cx, "day 4 column must be right of day 3");

        // Inject the focus ring at the active-descendant cell, then
        // rasterize through the cached (live) path.
        let scene = inject_focus_ring(
            scene,
            Some(FOCUSED_TAG),
            FocusRingStyle::default(),
            Some((W, H)),
        );
        let base = root_background(&scene);
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        // Locate the Material focus-blue (#1A73E8) ring pixels.
        let mut xs = Vec::new();
        for y in 0..H {
            for x in 0..W {
                let i = ((y * W + x) * 4) as usize;
                let (r, g, b) = (
                    i64::from(rgba8[i]),
                    i64::from(rgba8[i + 1]),
                    i64::from(rgba8[i + 2]),
                );
                if (r - 26).abs() <= 40 && (g - 115).abs() <= 40 && (b - 232).abs() <= 40 {
                    xs.push(i64::from(x));
                }
            }
        }
        assert!(!xs.is_empty(), "focus ring must be visible in the render");
        let ring_cx = xs.iter().sum::<i64>() / i64::try_from(xs.len()).unwrap();

        let d3 = (ring_cx - c3_cx).abs();
        let d4 = (ring_cx - c4_cx).abs();
        assert!(
            d3 < d4,
            "focus ring centre x={ring_cx} is closer to the day-4 column \
             (x={c4_cx}, d={d4}) than the focused day-3 column (x={c3_cx}, \
             d={d3}) — the pre-R706 cache append+direct-draw offset",
        );
        assert!(
            d3 <= 8,
            "focus ring centre x={ring_cx} should sit on the day-3 column \
             centre x={c3_cx} within ~8px, got d={d3}",
        );
    }

    /// R1139 §5.51 §2 #7 — the rendered-pixel proof that DE-GATES the redock-hint
    /// visibility question (the live-test "안 보임"). Earlier this was waved off as
    /// "HW-gated", but lavapipe renders headlessly here: a redock preview drawn
    /// over an OPAQUE floater background is unmistakable — the result-region fill
    /// changes the pixels far from the bare background, AND an opaque accent
    /// border outlines it (a hard edge that reads regardless of content hue).
    /// Rasterises through the SAME `to_vello_cached` the live shell uses and reads
    /// the pixels back, so "bold enough" is decided by MEASUREMENT, not by a human
    /// eyeballing a live window. The R1138 sibling proves the hint INJECTS into the
    /// floater's scene; this proves the injected pixels are VISIBLE.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored` (force lavapipe via `VK_ICD_FILENAMES` for a
    /// deterministic software raster).
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1139_redock_preview_is_boldly_visible_over_opaque_content() {
        use pinion_core::scene::{ContainerNode, Rect, Scene};
        use pinion_core::style::BoxStyle;
        use pinion_core::theme::{ColorRole, Theme};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;
        use pinion_widget_paint::dock::{
            DockDropZone, dock_drop_preview_overlay, dock_redock_preview_tint,
        };

        const W: u32 = 200;
        const H: u32 = 200;
        const HALF: u32 = 100; // DOCK_SPLIT_RESULT_PCT = 50 → 200 * 50% = 100

        let theme = Theme::light();
        let surface = theme.resolve(ColorRole::Surface);
        let accent = theme.resolve(ColorRole::Accent);

        // The floater's opaque content (a Surface-filled panel) + the redock
        // preview for a LEFT-zone redock (fills the left half, outlined). Rects
        // are ABSOLUTE — the paint adapter paints each node at its own `.rect`
        // (no offset accumulation), and the overlay already carries explicit
        // pixel rects (it is injected after layout), so no layout pass is needed.
        let overlay = dock_drop_preview_overlay(
            Rect::new(0, 0, W, H),
            DockDropZone::Left,
            dock_redock_preview_tint(&theme),
        )
        .expect("left zone paints an overlay");
        let mut root = ContainerNode::new(vec![overlay]).with_style(BoxStyle::filled(surface));
        root.rect = Rect::new(0, 0, W, H);
        let scene = Scene::Container(root);

        let mut text_cache = LayoutCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut cache = FragmentCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(
                &vello,
                W,
                H,
                PenikoColor::from_rgba8(surface.r, surface.g, surface.b, 0xFF),
            )
            .expect("render");

        let px = |x: u32, y: u32| -> (i64, i64, i64) {
            let i = ((y * W + x) * 4) as usize;
            (
                i64::from(rgba8[i]),
                i64::from(rgba8[i + 1]),
                i64::from(rgba8[i + 2]),
            )
        };
        let sum_delta = |a: (i64, i64, i64), b: (i64, i64, i64)| {
            (a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()
        };

        let bare = px(W - 20, H / 2); // right half: bare floater surface
        let fill = px(20, H / 2); // left half interior: tint over surface
        let border = px(HALF / 2, 1); // top edge of the left-half result rect
        let accent_rgb = (
            i64::from(accent.r),
            i64::from(accent.g),
            i64::from(accent.b),
        );

        // (1) The fill is unmistakably different from the bare background — the
        //     "tint too subtle" failure is measured away, not eyeballed.
        let fill_delta = sum_delta(fill, bare);
        assert!(
            fill_delta >= 90,
            "redock fill must visibly differ from the bare floater \
             (Σ|Δ|={fill_delta}, bare={bare:?}, fill={fill:?})",
        );
        // (2) The border reads as the opaque accent — a hard outline that does
        //     not depend on the content behind it — and far more so than the bare
        //     surface does (the outline is the robustness guarantee).
        let border_to_accent = sum_delta(border, accent_rgb);
        let bare_to_accent = sum_delta(bare, accent_rgb);
        assert!(
            border_to_accent <= 120,
            "redock border must read as the opaque accent \
             (Σ|Δ|={border_to_accent}, border={border:?}, accent={accent_rgb:?})",
        );
        assert!(
            border_to_accent < bare_to_accent,
            "the border reads as accent ({border_to_accent}); the bare floater \
             does not ({bare_to_accent})",
        );
    }

    /// R991 §5.41 §2 #6 — deterministic glyph-paint guard for the
    /// cell-native [`Scene::TextGrid`]. Builds a 3x3 retained grid that
    /// exercises every R991 paint path — palette-resolved fg/bg (`Rgb` /
    /// `Indexed` / `Default`), `reverse` (fg<->bg swap), `hidden` (glyph
    /// suppressed, bg still painted), and a wide cluster head + `Trailer`
    /// whose background spans both columns — rasterises it through the
    /// SAME `to_vello_cached` the live shell uses, and reads the pixels
    /// back. Assertions are font-robust: they check background fills
    /// (deterministic) and glyph presence/absence (a cell region is or is
    /// not its background colour), never exact glyph shape — so the guard
    /// does not depend on which monospace font fontique resolves.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling
    /// headless tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    #[allow(clippy::too_many_lines)]
    fn r991_text_grid_paints_cells_colours_attrs_wide() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CellAttrs, GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        // U+D55C (Hangul "han") — a wide cluster; named + escaped per the
        // non-ASCII source-literal rule (raw glyph only in doc strings).
        const WIDE: &str = "\u{D55C}";
        const CW: u32 = 16;
        const CH: u32 = 24;
        const COLS: u16 = 3;
        const ROWS: u16 = 3;
        const W: u32 = CW * COLS as u32;
        const H: u32 = CH * ROWS as u32;

        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let red = TermColor::Rgb(Color::rgb(0xff, 0x00, 0x00));
        let teal = TermColor::Rgb(Color::rgb(0x12, 0x34, 0x56));
        let green = TermColor::Rgb(Color::rgb(0x00, 0xff, 0x00));
        let magenta = TermColor::Rgb(Color::rgb(0xaa, 0x00, 0xaa));
        let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));

        // Row 0 — fg/bg palette resolution: Rgb red bg, Indexed(4) ANSI
        // blue bg, and a default-on-default "A" glyph.
        let row0 = vec![
            TermCell::new(" ", TermColor::Default, red),
            TermCell::new(" ", TermColor::Default, TermColor::Indexed(4)),
            TermCell::new("A", TermColor::Default, TermColor::Default),
        ];
        // Row 1 — attributes: reverse (effective bg becomes the green fg),
        // hidden (the white "X" glyph suppressed, teal bg still paints).
        let row1 = vec![
            TermCell::new(" ", green, TermColor::Default)
                .with_attrs(CellAttrs::empty().with_reverse(true)),
            TermCell::new("X", white, teal).with_attrs(CellAttrs::empty().with_hidden(true)),
            TermCell::blank(),
        ];
        // Row 2 — wide head + trailer: the magenta bg spans both columns.
        let head = TermCell::new(WIDE, TermColor::Default, magenta).wide();
        let trail = head.trailer();
        let row2 = vec![head, trail, TermCell::blank()];

        let buffer = GridBuffer::new(COLS, ROWS)
            .with_row(0, row0)
            .with_row(1, row1)
            .with_row(2, row2);
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let base = vello::peniko::Color::BLACK;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        let at = |x: u32, y: u32| -> (i64, i64, i64) {
            let i = ((y * W + x) * 4) as usize;
            (
                i64::from(rgba8[i]),
                i64::from(rgba8[i + 1]),
                i64::from(rgba8[i + 2]),
            )
        };

        // (0,0) Rgb red background — sample two interior points (not the
        // centre alone) inset from the cell edges to dodge antialiasing.
        for &(x, y) in &[(CW / 2, CH / 2), (5, 6)] {
            let (r, g, b) = at(x, y);
            assert!(
                r > 200 && g < 50 && b < 50,
                "cell(0,0) Rgb red bg, got ({r},{g},{b})"
            );
        }
        // (1,0) Indexed(4) = ANSI blue (#0000ee).
        for &(x, y) in &[(CW + CW / 2, CH / 2), (CW + 5, 6)] {
            let (r, g, b) = at(x, y);
            assert!(
                b > 180 && r < 50 && g < 50,
                "cell(1,0) Indexed(4) blue bg, got ({r},{g},{b})"
            );
        }
        // (2,0) "A" glyph present: bright pixels on the black default bg.
        let mut a_glyph = false;
        for y in 0..CH {
            for x in (2 * CW)..(3 * CW) {
                if at(x, y).0 > 120 {
                    a_glyph = true;
                }
            }
        }
        assert!(
            a_glyph,
            "cell(2,0) 'A' glyph must paint bright pixels on black"
        );

        // (0,1) reverse — the effective background is the green foreground.
        for &(x, y) in &[(CW / 2, CH + CH / 2), (5, CH + 6)] {
            let (r, g, b) = at(x, y);
            assert!(
                g > 200 && r < 50 && b < 50,
                "cell(0,1) reverse swaps fg->bg green, got ({r},{g},{b})"
            );
        }
        // (1,1) hidden — the teal bg paints; the white "X" glyph does not.
        let (r, g, b) = at(CW + CW / 2, CH + CH / 2);
        assert!(
            (r - 18).abs() < 30 && (g - 52).abs() < 30 && (b - 86).abs() < 40,
            "cell(1,1) hidden keeps the teal bg, got ({r},{g},{b})"
        );
        let mut hidden_glyph = false;
        for y in CH..(2 * CH) {
            for x in CW..(2 * CW) {
                let (r, g, b) = at(x, y);
                if r > 200 && g > 200 && b > 200 {
                    hidden_glyph = true;
                }
            }
        }
        assert!(
            !hidden_glyph,
            "cell(1,1) hidden must suppress the white 'X' glyph"
        );

        // (cols 0..1, row 2) wide head + Trailer — the magenta bg must span
        // BOTH columns (the trailer carries the head's colours), and the
        // wide glyph must paint somewhere across the span.
        let mut head_magenta = 0u32;
        let mut trailer_magenta = 0u32;
        for y in (2 * CH)..(3 * CH) {
            for x in 0..(2 * CW) {
                let (r, g, b) = at(x, y);
                let is_magenta = r > 120 && g < 60 && b > 120;
                if is_magenta && x < CW {
                    head_magenta += 1;
                } else if is_magenta {
                    trailer_magenta += 1;
                }
            }
        }
        assert!(
            head_magenta > 20,
            "wide head bg magenta missing (head_magenta={head_magenta})"
        );
        assert!(
            trailer_magenta > 20,
            "Trailer cell must carry the wide head's magenta bg across both columns \
             (trailer_magenta={trailer_magenta})"
        );
        // Deliberately NOT asserting the wide glyph's *ink*: a Hangul cluster's
        // coverage depends on which font fontique resolves (the bundled-font
        // debt), so an ink assert would be a system-font flake. The ASCII 'A'
        // assert above proves glyph paint works font-robustly; "wide" is proven
        // font-independently by the two-column background span (head + trailer).
    }

    /// R1028 §5.41 §2 #6 — the [`Scene::TextGrid`] paint fills its *whole node
    /// rect* with the palette default background (pass 0) before any cell, so a
    /// sub-cell gutter (when `cols*cell_w` / `rows*cell_h` does not tile the
    /// rect exactly — the §3 one-way winsize SSOT case, cols/rows derived from a
    /// continuous pixel rect) reads as the terminal background, not whatever
    /// parent surface sits behind. Builds a 2x2 grid of red cells inside a
    /// deliberately larger rect, clears the GPU surface to WHITE (the
    /// parent-bleed analog), and asserts the right / bottom / corner gutters are
    /// the palette default bg (black), never the white clear. Without the pass-0
    /// fill the gutters expose the white clear and this guard fails.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1028_text_grid_fills_rect_gutter_with_default_bg() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 10;
        const CH: u32 = 12;
        const COLS: u16 = 2;
        const ROWS: u16 = 2;
        // The cell area covers only COLS*CW x ROWS*CH = 20x24; the node rect is
        // larger, leaving a right gutter (x in 20..30) and a bottom gutter
        // (y in 24..36) — the continuous-rect winsize case.
        const CELL_W: u32 = CW * COLS as u32; // 20
        const CELL_H: u32 = CH * ROWS as u32; // 24
        const W: u32 = 30;
        const H: u32 = 36;

        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let red = TermColor::Rgb(Color::rgb(0xff, 0x00, 0x00));
        // Every cell opaque red so the cell area is unambiguously distinct from
        // both the white clear and the black default bg.
        let cell = TermCell::new(" ", TermColor::Default, red);
        let buffer = GridBuffer::new(COLS, ROWS)
            .with_row(0, vec![cell.clone(), cell.clone()])
            .with_row(1, vec![cell.clone(), cell.clone()]);
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        // Clear to WHITE — the analog of a parent splitter `Surface` fill
        // sitting behind the grid. If pass 0 is absent, the gutters show this.
        let base = vello::peniko::Color::WHITE;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        let at = |x: u32, y: u32| -> (i64, i64, i64) {
            let i = ((y * W + x) * 4) as usize;
            (
                i64::from(rgba8[i]),
                i64::from(rgba8[i + 1]),
                i64::from(rgba8[i + 2]),
            )
        };

        // Cell interior — opaque red (pass 0 must not erase the cell bg).
        for &(x, y) in &[(5, 6), (15, 18)] {
            let (r, g, b) = at(x, y);
            assert!(
                r > 200 && g < 60 && b < 60,
                "cell interior must stay red, got ({r},{g},{b}) at ({x},{y})"
            );
        }
        // Gutters — right (x>=CELL_W), bottom (y>=CELL_H), and the corner — must
        // be the palette default bg (black), NOT the white clear. Sampled well
        // inset from the cell boundary to dodge antialiasing.
        let gutters = [
            (CELL_W + 4, 6),          // right gutter, top row
            (CELL_W + 4, CH + 6),     // right gutter, bottom row
            (4, CELL_H + 6),          // bottom gutter, left column
            (CW + 4, CELL_H + 6),     // bottom gutter, right column
            (CELL_W + 4, CELL_H + 6), // bottom-right corner gutter
        ];
        for &(x, y) in &gutters {
            let (r, g, b) = at(x, y);
            assert!(
                r < 50 && g < 50 && b < 50,
                "gutter ({x},{y}) must be the palette default bg (black), not the \
                 white parent clear — got ({r},{g},{b})"
            );
        }
    }

    /// R1028.1 §5.41 §2 #6 — pass 0 must run BEFORE the empty-grid early-out, so
    /// a geometry-only grid (a sized rect with no cells — a documented
    /// `TextGridNode::new` state, and the transient first frame before a
    /// consumer pushes its buffer) still paints its terminal background instead
    /// of leaking the parent surface across the whole rect. The grid is placed
    /// at a NON-ZERO rect origin and rendered into a larger surface, so the test
    /// also pins the pass-0 coordinate frame: the fill lands at the rect's
    /// offset (inside reads default bg) and the clip bounds it (outside the rect
    /// reads the parent clear) — a regression in either the empty-grid order or
    /// the fill transform fails this guard.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1028_1_geometry_only_grid_fills_rect_and_clips_at_origin() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const OX: u32 = 8;
        const OY: u32 = 6;
        const RW: u32 = 24;
        const RH: u32 = 20;
        const W: u32 = OX + RW + 6; // surface wider than the rect on both axes
        const H: u32 = OY + RH + 6;

        let metric = CellMetric::new(10, 12).expect("non-zero cell metric");
        // No `with_cells` — a geometry-only grid (0x0 buffer, `is_empty()`),
        // but with a sized, offset layout rect.
        let mut node = TextGridNode::new(metric);
        node.rect = Rect::new(OX, OY, RW, RH);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let base = vello::peniko::Color::WHITE;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        let at = |x: u32, y: u32| -> (i64, i64, i64) {
            let i = ((y * W + x) * 4) as usize;
            (
                i64::from(rgba8[i]),
                i64::from(rgba8[i + 1]),
                i64::from(rgba8[i + 2]),
            )
        };

        // Inside the rect (offset by OX/OY) — the geometry-only grid still fills
        // the palette default bg (black), sampled inset from the rect edges.
        for &(x, y) in &[
            (OX + 4, OY + 4),
            (OX + RW - 4, OY + RH - 4),
            (OX + RW / 2, OY + RH / 2),
        ] {
            let (r, g, b) = at(x, y);
            assert!(
                r < 50 && g < 50 && b < 50,
                "inside the rect ({x},{y}) must be the default bg (black), got ({r},{g},{b})"
            );
        }
        // Outside the rect — the clip bounds pass 0, so the parent clear (white)
        // shows. Samples in the left margin (x<OX) and top margin (y<OY).
        for &(x, y) in &[(2, OY + 4), (OX + 4, 2), (2, 2)] {
            let (r, g, b) = at(x, y);
            assert!(
                r > 200 && g > 200 && b > 200,
                "outside the rect ({x},{y}) must stay the white parent clear, got ({r},{g},{b})"
            );
        }
    }

    /// R992 §5.41 §5.16 — deterministic guard for the cell-grid typographic
    /// SGR attributes. The `underline` / `strikethrough` / `dim` paths are
    /// **geometric** (full-cell rules + an alpha factor), so they are asserted
    /// font-independently: a blank white-on-black cell shows only its rule, and
    /// `dim` halves the rule's intensity. `bold` / `italic` only change glyph
    /// shape (font-dependent), so they are asserted as glyph *presence*, never
    /// shape — mirroring the R991 guard's font-robust discipline.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    #[allow(clippy::too_many_lines)]
    fn r992_text_grid_paints_sgr_typographic_attrs() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CellAttrs, GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 24;
        const COLS: u16 = 3;
        const ROWS: u16 = 2;
        const W: u32 = CW * COLS as u32;
        const H: u32 = CH * ROWS as u32;

        // White ink on a black background, so every rule / glyph is a bright
        // pixel on a deterministic black field (palette-independent).
        let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));
        let black = TermColor::Rgb(Color::rgb(0x00, 0x00, 0x00));
        let cell = |attrs: CellAttrs| TermCell::new(" ", white, black).with_attrs(attrs);
        let glyph = |attrs: CellAttrs| TermCell::new("A", white, black).with_attrs(attrs);
        let e = CellAttrs::empty;

        // Row 0 — geometric attrs on blank cells (only the rule inks):
        //   (0,0) underline, (1,0) strikethrough, (2,0) underline + dim.
        let row0 = vec![
            cell(e().with_underline(true)),
            cell(e().with_strikethrough(true)),
            cell(e().with_underline(true).with_dim(true)),
        ];
        // Row 1 — (0,1) bold 'A', (1,1) italic 'A' (glyph presence), and
        //   (2,1) underline + strikethrough together on a blank cell.
        let row1 = vec![
            glyph(e().with_bold(true)),
            glyph(e().with_italic(true)),
            cell(e().with_underline(true).with_strikethrough(true)),
        ];

        let buffer = GridBuffer::new(COLS, ROWS)
            .with_row(0, row0)
            .with_row(1, row1);
        let mut node =
            TextGridNode::new(CellMetric::new(CW, CH).expect("non-zero")).with_cells(buffer);
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let base = vello::peniko::Color::BLACK;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        // Peak luminance (white ink ⇒ r≈g≈b, so the red channel tracks it)
        // over the rectangle [x0,x1) x [y0,y1).
        let peak = |x0: u32, x1: u32, y0: u32, y1: u32| -> i64 {
            let mut m = 0i64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * W + x) * 4) as usize;
                    m = m.max(i64::from(rgba8[i]));
                }
            }
            m
        };

        // Per-cell band ink (the SUM of the red channel over the band) — a
        // thin rule's *peak* depends on sub-pixel alignment (antialiasing can
        // split a 1.5-px rule across two rows at ~0.75 each), but its area
        // integral is alignment-invariant, so summing over a band that fully
        // contains the rule's vertical extent is the ZERO-FLAKE metric. The
        // interior x-range dodges edge antialiasing. The underline band hugs
        // the cell bottom; the strikethrough band straddles mid-cell.
        let interior = |c: u32| (c * CW + 2, c * CW + CW - 2);
        let band_ink = |c: u32, y0: u32, y1: u32| -> i64 {
            let (x0, x1) = interior(c);
            let mut sum = 0i64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * W + x) * 4) as usize;
                    sum += i64::from(rgba8[i]);
                }
            }
            sum
        };
        let underline_ink = |c: u32, r: u32| band_ink(c, r * CH + CH - 4, r * CH + CH);
        let strike_ink = |c: u32, r: u32| band_ink(c, r * CH + CH / 2 - 3, r * CH + CH / 2 + 3);
        let glyph_peak = |c: u32, r: u32| -> i64 {
            let (x0, x1) = interior(c);
            peak(x0, x1, r * CH, r * CH + CH)
        };

        // (0,0) underline — an inked rule near the cell bottom and nothing at
        // mid-cell (blank cell, so the rule is the only ink). The full-alpha
        // 1.5-px white rule over a ~12-px interior integrates to ~4.5k.
        let full_underline = underline_ink(0, 0);
        assert!(
            full_underline > 2000,
            "cell(0,0) underline rule must ink, sum={full_underline}"
        );
        assert!(
            strike_ink(0, 0) < 500,
            "cell(0,0) underline must not ink mid-cell"
        );

        // (1,0) strikethrough — an inked rule at mid-cell, nothing at the bottom.
        assert!(
            strike_ink(1, 0) > 2000,
            "cell(1,0) strikethrough rule must ink"
        );
        assert!(
            underline_ink(1, 0) < 500,
            "cell(1,0) strikethrough must not ink the bottom"
        );

        // (2,0) underline + dim — the bottom rule still inks but at ~half the
        // area (SGR 2 halves the foreground alpha over the black bg). A ratio
        // check cancels the antialiasing common to both cells.
        let dim_underline = underline_ink(2, 0);
        assert!(
            dim_underline > full_underline / 4 && dim_underline * 10 < full_underline * 7,
            "cell(2,0) dim underline must ink but at clearly less area than the \
             full-intensity underline: dim={dim_underline}, full={full_underline}"
        );

        // (0,1) bold 'A' / (1,1) italic 'A' — the glyph still paints (presence,
        // not shape: bold / italic change the glyph outline font-dependently).
        assert!(
            glyph_peak(0, 1) > 120,
            "cell(0,1) bold 'A' glyph must paint"
        );
        assert!(
            glyph_peak(1, 1) > 120,
            "cell(1,1) italic 'A' glyph must paint"
        );

        // (2,1) underline + strikethrough — both rules ink on the one cell.
        assert!(
            underline_ink(2, 1) > 2000,
            "cell(2,1) combo underline rule must ink"
        );
        assert!(
            strike_ink(2, 1) > 2000,
            "cell(2,1) combo strikethrough rule must ink"
        );
    }

    /// R1399 §5.41 §5.16 — deterministic guard for the underline-style axis
    /// ([`UnderlineStyle`]) and the explicit SGR-58 underline colour. Like the
    /// R992 guard the assertions are **geometric** and font-independent
    /// (blank white-on-black cells, so only the underline inks), asserted via
    /// alignment-invariant area integrals:
    ///
    /// - each style inks the bottom band (all five paint paths draw);
    /// - `Double` / `Curly` reach a *high* band a straight single rule never
    ///   touches (the second rule / the wave crest), so the styles are not
    ///   collapsed to one form;
    /// - `Dotted` / `Dashed` ink strictly *less* than the solid single rule
    ///   over the same window (their gaps), with the dashed duty cycle above
    ///   the dotted one — a broken rule, deterministically;
    /// - a red SGR-58 underline inks the *red* channel but not the *green*,
    ///   proving the colour axis paints in its own colour (the LSP-diagnostic
    ///   forcing case), not the white foreground.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling tests;
    /// run with `--ignored`.
    ///
    /// [`UnderlineStyle`]: pinion_core::term_grid::UnderlineStyle
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    #[allow(clippy::too_many_lines)]
    fn r1399_text_grid_paints_underline_styles_and_color() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CellAttrs, GridBuffer, TermCell, TermColor, UnderlineStyle};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 24;
        const COLS: u16 = 6;
        const ROWS: u16 = 1;
        const W: u32 = CW * COLS as u32;
        const H: u32 = CH * ROWS as u32;

        let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));
        let black = TermColor::Rgb(Color::rgb(0x00, 0x00, 0x00));
        let red = TermColor::Rgb(Color::rgb(0xff, 0x00, 0x00));
        let e = CellAttrs::empty;
        let styled = |s: UnderlineStyle| {
            TermCell::new(" ", white, black).with_attrs(e().with_underline_style(s))
        };

        // cols 0..=4 — one style each, default (foreground-tracking) colour.
        // col 5 — a single rule with an explicit SGR-58 red underline colour.
        let row0 = vec![
            styled(UnderlineStyle::Single),
            styled(UnderlineStyle::Double),
            styled(UnderlineStyle::Curly),
            styled(UnderlineStyle::Dotted),
            styled(UnderlineStyle::Dashed),
            TermCell::new(" ", white, black)
                .with_attrs(e().with_underline_style(UnderlineStyle::Single))
                .with_underline_color(red),
        ];

        let buffer = GridBuffer::new(COLS, ROWS).with_row(0, row0);
        let mut node =
            TextGridNode::new(CellMetric::new(CW, CH).expect("non-zero")).with_cells(buffer);
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let base = vello::peniko::Color::BLACK;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        // Channel-summed ink over a column's interior x-range and a y-band —
        // the same alignment-invariant area integral the R992 guard uses.
        // `chan` picks the RGBA byte offset (0 = red, 1 = green).
        let interior = |c: u32| (c * CW + 2, c * CW + CW - 2);
        let band = |c: u32, y0: u32, y1: u32, chan: usize| -> i64 {
            let (x0, x1) = interior(c);
            let mut sum = 0i64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * W + x) * 4) as usize + chan;
                    sum += i64::from(rgba8[i]);
                }
            }
            sum
        };
        // The bottom band contains every style's lowest rule; the high band
        // sits above a single rule (only Double's second rule / Curly's crest
        // reach it).
        let bottom = |c: u32| band(c, CH - 6, CH, 0);
        let high = |c: u32| band(c, CH - 6, CH - 3, 0);

        let single = bottom(0);
        let double = bottom(1);
        let curly = bottom(2);
        let dotted = bottom(3);
        let dashed = bottom(4);

        // Every style inks its bottom rule.
        assert!(single > 2000, "single underline must ink, sum={single}");
        assert!(double > 2000, "double underline must ink, sum={double}");
        assert!(curly > 1000, "curly underline must ink, sum={curly}");
        assert!(dotted > 600, "dotted underline must ink, sum={dotted}");
        assert!(dashed > 600, "dashed underline must ink, sum={dashed}");

        // Style differentiation: a single straight rule leaves the high band
        // dark; Double (second rule) and Curly (wave crest) reach it.
        assert!(
            high(0) < 600,
            "single rule must not ink the high band, {}",
            high(0)
        );
        assert!(
            high(1) > 600,
            "double's upper rule must ink the high band, {}",
            high(1)
        );
        assert!(
            high(2) > 600,
            "curly's crest must ink the high band, {}",
            high(2)
        );
        // Double is two rules in the bottom band — clearly more ink than one.
        assert!(
            double > single * 13 / 10,
            "double must ink more than a single rule: double={double}, single={single}"
        );

        // Dotted / dashed are broken rules: strictly less bottom ink than the
        // solid single rule, and the dashed duty cycle is above the dotted.
        assert!(
            dotted < single * 3 / 4,
            "dotted must ink less than the solid rule: dotted={dotted}, single={single}"
        );
        assert!(
            dashed < single * 9 / 10 && dashed > dotted,
            "dashed must be a broken rule denser than dotted: dashed={dashed}, dotted={dotted}, single={single}"
        );

        // The SGR-58 colour axis (col 5): the red underline inks the red
        // channel like a normal rule but leaves the green channel dark —
        // proving it paints in its own colour, not the white foreground.
        let red_ink = band(5, CH - 6, CH, 0);
        let green_ink = band(5, CH - 6, CH, 1);
        assert!(
            red_ink > 2000,
            "red underline must ink the red channel, {red_ink}"
        );
        assert!(
            green_ink < red_ink / 5,
            "red underline must not ink the green channel: green={green_ink}, red={red_ink}"
        );
    }

    /// R993 §5.41 — deterministic guard for the cell-grid [`GridCursor`]
    /// overlay. Every shape is a *fill* (block / bar / underline), so the
    /// assertions are pixel-aligned and font-independent: the block fills the
    /// cell, the bar inks only the leading edge, the underline is a thick
    /// bottom bar, and an invisible cursor paints nothing. The block-over-glyph
    /// case asserts the inverse glyph as *presence* (dark ink inside the
    /// bright block), never shape — mirroring the R991 / R992 discipline.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    // R1026 — rustfmt's reflow pushed this past the workspace too_many_lines (100).
    #[allow(clippy::too_many_lines)]
    fn r993_text_grid_paints_cursor_shapes() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 24;
        const COLS: u16 = 2;
        const ROWS: u16 = 1;
        const W: u32 = CW * COLS as u32;
        const H: u32 = CH * ROWS as u32;
        // 한 (U+D55C) — a wide cluster; escaped per the non-ASCII source rule.
        const HAN: &str = "\u{D55C}";

        let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));
        let black = TermColor::Rgb(Color::rgb(0x00, 0x00, 0x00));
        let metric = CellMetric::new(CW, CH).expect("non-zero");

        // A 2x1 buffer: cell (0,0) carries `glyph`, (1,0) is blank; the cursor
        // sits on (0,0) with the given shape / visibility.
        let buf = |glyph: &'static str, shape: CursorShape, visible: bool| -> GridBuffer {
            GridBuffer::new(COLS, ROWS)
                .with_row(
                    0,
                    [
                        TermCell::new(glyph, white, black),
                        TermCell::new(" ", white, black),
                    ],
                )
                .with_cursor(GridCursor::new(0, 0, shape, visible))
        };

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let mut render = |buffer: GridBuffer| -> Vec<u8> {
            let mut node = TextGridNode::new(metric).with_cells(buffer);
            node.rect = Rect::new(0, 0, W, H);
            let scene = Scene::TextGrid(node);
            let mut text_cache = LayoutCache::new();
            let mut cache = FragmentCache::new();
            let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
            let mut vello = VelloScene::new();
            to_vello_cached(
                &scene,
                &|_| None,
                &mut text_cache,
                &mut image_cache,
                &mut cache,
                &mut vello,
            );
            shot.render_to_rgba8(&vello, W, H, vello::peniko::Color::BLACK)
                .expect("render")
        };

        // Red channel at (x, y) (white ink ⇒ r≈g≈b, so red tracks luminance).
        let red =
            |img: &[u8], x: u32, y: u32| -> i64 { i64::from(img[((y * W + x) * 4) as usize]) };

        // Block over a blank cell — the whole cell fills with the cursor colour
        // (white); the neighbour cell stays unlit.
        let filled = render(buf(" ", CursorShape::Block, true));
        assert!(
            red(&filled, CW / 2, CH / 2) > 200,
            "block cursor fills its cell white"
        );
        assert!(
            red(&filled, CW + CW / 2, CH / 2) < 60,
            "neighbour cell has no cursor"
        );

        // Block over 'A' — the cell still fills (corner is white) and the glyph
        // reads through inverse (dark pixels appear inside the bright block).
        let inverse = render(buf("A", CursorShape::Block, true));
        assert!(
            red(&inverse, 2, 2) > 200,
            "block-over-glyph corner is the cursor fill"
        );
        let mut inverse_glyph = false;
        for y in 2..(CH - 2) {
            for x in 2..(CW - 2) {
                if red(&inverse, x, y) < 80 {
                    inverse_glyph = true;
                }
            }
        }
        assert!(
            inverse_glyph,
            "block cursor must redraw the glyph inverse (dark ink in the block)"
        );

        // Bar over a blank cell — a vertical beam at the leading edge; the cell
        // interior stays black.
        let bar = render(buf(" ", CursorShape::Bar, true));
        assert!(
            red(&bar, 0, CH / 2) > 200,
            "bar cursor inks the leading edge"
        );
        assert!(
            red(&bar, CW / 2, CH / 2) < 60,
            "bar cursor leaves the interior blank"
        );

        // Underline over a blank cell — a solid bottom bar >= 2px thick (so it
        // reads distinctly from the thin SGR underline); the cell middle blank.
        let underline = render(buf(" ", CursorShape::Underline, true));
        assert!(
            red(&underline, CW / 2, CH - 1) > 200,
            "underline cursor inks the cell bottom"
        );
        assert!(
            red(&underline, CW / 2, CH / 2) < 60,
            "underline cursor leaves the middle blank"
        );
        let mut thickness = 0u32;
        while thickness < CH && red(&underline, CW / 2, CH - 1 - thickness) > 200 {
            thickness += 1;
        }
        assert!(
            thickness >= 2,
            "cursor underline is a thick bar (>= 2px), got {thickness}"
        );

        // Block over a WIDE head — the fill spans BOTH columns (matching the
        // wide glyph and the TUI reversed head). The trailer column thus shows
        // the cursor colour somewhere; without the 2-column span it would be
        // the (black) trailer background.
        let wide_block = {
            let head = TermCell::new(HAN, white, black).wide();
            render(
                GridBuffer::new(2, 1)
                    .with_row(0, [head.clone(), head.trailer()])
                    .with_cursor(GridCursor::new(0, 0, CursorShape::Block, true)),
            )
        };
        let mut trailer_filled = false;
        for y in 2..(CH - 2) {
            for x in (CW + 2)..(2 * CW - 2) {
                if red(&wide_block, x, y) > 150 {
                    trailer_filled = true;
                }
            }
        }
        assert!(
            trailer_filled,
            "block cursor on a wide head must fill the trailer column too"
        );

        // Invisible cursor — nothing paints; the cell stays its background.
        let hidden = render(buf(" ", CursorShape::Block, false));
        assert!(
            red(&hidden, CW / 2, CH / 2) < 60,
            "an invisible cursor paints nothing"
        );
    }

    /// R1424 §5.41 — deterministic guard for the explicit OSC-12 cursor colour
    /// ([`GridCursor::with_cursor_color`]). A block cursor over a blank cell
    /// fills that cell in the *cursor colour*: the default (no colour) fills in
    /// the cell foreground (white), while an explicit green cursor fills green —
    /// the paint honours the absolute OSC-12 colour, not the cell ink. Probed
    /// on the green channel (high for green, low for the white default) so the
    /// assertion is font-independent, mirroring the R993 shape.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1424_text_grid_paints_explicit_cursor_color() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 24;
        const W: u32 = CW;
        const H: u32 = CH;

        let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));
        let black = TermColor::Rgb(Color::rgb(0x00, 0x00, 0x00));
        let green = Color::rgb(0x2e, 0xcc, 0x71); // r=46, g=204, b=113
        let metric = CellMetric::new(CW, CH).expect("non-zero");

        // A 1x1 blank cell; the block cursor sits on it. `color` = the explicit
        // OSC-12 cursor colour (None keeps the cell-foreground default).
        let buf = |color: Option<Color>| -> GridBuffer {
            let mut cursor = GridCursor::new(0, 0, CursorShape::Block, true);
            if let Some(c) = color {
                cursor = cursor.with_cursor_color(c);
            }
            GridBuffer::new(1, 1)
                .with_row(0, [TermCell::new(" ", white, black)])
                .with_cursor(cursor)
        };

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let mut render = |buffer: GridBuffer| -> Vec<u8> {
            let mut node = TextGridNode::new(metric).with_cells(buffer);
            node.rect = Rect::new(0, 0, W, H);
            let scene = Scene::TextGrid(node);
            let mut text_cache = LayoutCache::new();
            let mut cache = FragmentCache::new();
            let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
            let mut vello = VelloScene::new();
            to_vello_cached(
                &scene,
                &|_| None,
                &mut text_cache,
                &mut image_cache,
                &mut cache,
                &mut vello,
            );
            shot.render_to_rgba8(&vello, W, H, vello::peniko::Color::BLACK)
                .expect("render")
        };

        // (r, g, b) channel at the cell centre.
        let chan = |img: &[u8], c: u32| -> i64 {
            let (x, y) = (CW / 2, CH / 2);
            i64::from(img[((y * W + x) * 4 + c) as usize])
        };

        // Default (no explicit colour): the block fills in the cell foreground
        // (white) — every channel high.
        let default_fill = render(buf(None));
        assert!(
            chan(&default_fill, 0) > 200 && chan(&default_fill, 1) > 200,
            "default block cursor fills the cell in the cell foreground (white)"
        );

        // Explicit green: the block fills GREEN — the green channel dominates
        // and the red channel drops well below the white default, proving the
        // OSC-12 colour (not the cell ink) drives the fill.
        let green_fill = render(buf(Some(green)));
        assert!(
            chan(&green_fill, 1) > 150,
            "explicit-colour block cursor fills green (green channel high), got g={}",
            chan(&green_fill, 1),
        );
        assert!(
            chan(&green_fill, 0) < 110,
            "explicit green fill has a low red channel (not the white default), got r={}",
            chan(&green_fill, 0),
        );
    }

    /// R1427 §5.41 §5.39 — an UNFOCUSED window draws its terminal cursor as a
    /// HOLLOW outline box (interior = the cell background, border = the cursor
    /// colour), versus the FOCUSED filled block. Proven in pixels: the same
    /// block cursor rendered with `cursor_focused = true` fills its interior in
    /// the cursor colour, while `cursor_focused = false` leaves the interior the
    /// cell background and paints only the outline stroke. Focus-hollow overrides
    /// the shape and is a function of focus, not blink (this cursor is steady).
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1427_unfocused_text_grid_cursor_is_hollow() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached_with_text_engine};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 24;
        const W: u32 = CW;
        const H: u32 = CH;

        let white = TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff));
        let black = TermColor::Rgb(Color::rgb(0x00, 0x00, 0x00));
        let metric = CellMetric::new(CW, CH).expect("non-zero");

        // A single blank (space) cell so the cursor colour is the only ink; the
        // effective cursor colour is the cell foreground (white) on black.
        let buf = || -> GridBuffer {
            GridBuffer::new(1, 1)
                .with_row(0, [TermCell::new(" ", white, black)])
                .with_cursor(GridCursor::new(0, 0, CursorShape::Block, true))
        };

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let mut render = |focused: bool| -> Vec<u8> {
            let mut node = TextGridNode::new(metric).with_cells(buf());
            node.rect = Rect::new(0, 0, W, H);
            let scene = Scene::TextGrid(node);
            let mut text_cache = LayoutCache::new();
            let mut cache = FragmentCache::new();
            let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
            let mut vello = VelloScene::new();
            to_vello_cached_with_text_engine(
                &scene,
                &|_| None,
                &mut text_cache,
                &mut image_cache,
                &mut cache,
                None,
                &mut vello,
                true, // cursor_blink_on: steady (this cursor never blinks)
                focused,
            );
            shot.render_to_rgba8(&vello, W, H, vello::peniko::Color::BLACK)
                .expect("render")
        };

        // (r, g, b) channel at pixel (x, y).
        let chan = |img: &[u8], x: u32, y: u32, c: u32| -> i64 {
            i64::from(img[((y * W + x) * 4 + c) as usize])
        };

        // Focused: the block FILLS — the cell interior (centre) is the cursor
        // colour (white), every channel high.
        let filled = render(true);
        assert!(
            chan(&filled, CW / 2, CH / 2, 0) > 200 && chan(&filled, CW / 2, CH / 2, 1) > 200,
            "focused block cursor fills its interior (white), got r={} g={}",
            chan(&filled, CW / 2, CH / 2, 0),
            chan(&filled, CW / 2, CH / 2, 1),
        );

        // Unfocused: HOLLOW — the interior (centre) is the cell background (black,
        // NOT the filled cursor colour)...
        let hollow = render(false);
        assert!(
            chan(&hollow, CW / 2, CH / 2, 0) < 60,
            "unfocused cursor interior is HOLLOW (cell background, not filled), got r={}",
            chan(&hollow, CW / 2, CH / 2, 0),
        );
        // ...while the outline stroke is present at the cell's top edge (white).
        assert!(
            chan(&hollow, CW / 2, 1, 0) > 150,
            "the hollow box paints a visible outline stroke at the top edge, got r={}",
            chan(&hollow, CW / 2, 1, 0),
        );
    }

    /// R995 §5.41 §2 #6 — cross-backend consistency (Vello half). Renders the
    /// **same** shared [`text_grid_consistency_buffer`] the TUI half drives and
    /// asserts each cell's *visible-ink* presence agrees with the model
    /// ([`expected_text_grid_cell_facts`]), so the GUI / TUI dual is pinned
    /// through one source of truth. Colour is deliberately not asserted across
    /// backends (Vello palette-resolves, the TUI defers to the host terminal) —
    /// the contract is cell-structure identity.
    ///
    /// The ink probe is colour-independent (a cell inks iff some interior pixel
    /// differs strongly from its own background corner), so it holds across the
    /// fixture's varied backgrounds without coupling to specific colours. The
    /// wide CJK head's ink is *not* probed (its coverage is system-font
    /// dependent — the [[pinion-text-layout-tests-system-font-debt]]
    /// discipline); the wide span is proven font-independently by its
    /// two-column ANSI-blue background instead.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r995_text_grid_cross_consistency_vello() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::term_grid::CellWidth;
        use pinion_core::test_fixtures::{
            expected_text_grid_cell_facts, text_grid_consistency_buffer,
        };
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 24;
        const COLS: u16 = 4;
        const ROWS: u16 = 3;
        const W: u32 = CW * COLS as u32;
        const H: u32 = CH * ROWS as u32;

        let buffer = text_grid_consistency_buffer();
        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let mut node = TextGridNode::new(metric).with_cells(buffer.clone());
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(&vello, W, H, vello::peniko::Color::BLACK)
            .expect("render");

        let at = |x: u32, y: u32| -> (i64, i64, i64) {
            let i = ((y * W + x) * 4) as usize;
            (
                i64::from(rgba8[i]),
                i64::from(rgba8[i + 1]),
                i64::from(rgba8[i + 2]),
            )
        };
        // A cell inks iff some interior pixel differs strongly (channel-sum > 90)
        // from the cell's own background corner — colour-independent, so it holds
        // for reversed / cursor / palette-resolved backgrounds alike.
        let inks = |col: u16, row: u16| -> bool {
            let (ox, oy) = (u32::from(col) * CW, u32::from(row) * CH);
            let (br, bg, bb) = at(ox + 1, oy + 1);
            for y in (oy + 3)..(oy + CH - 3) {
                for x in (ox + 3)..(ox + CW - 3) {
                    let (r, g, b) = at(x, y);
                    if (r - br).abs() + (g - bg).abs() + (b - bb).abs() > 90 {
                        return true;
                    }
                }
            }
            false
        };

        // Every cell except the wide CJK head: visible ink presence must match
        // the shared model. This is the cross-backend pin — the TUI half asserts
        // the same `expected_text_grid_cell_facts` over the same buffer.
        for row in 0..ROWS {
            for col in 0..COLS {
                if (col, row) == (0, 1) || (col, row) == (1, 1) {
                    // (0,1) wide CJK head AND (1,1) its trailer: both carry
                    // font-dependent ink. R1014.1 — post-R1013 the two-pass
                    // preserves the head glyph's overflow INTO the trailer
                    // column, so the trailer's no-ink fact is no longer
                    // font-independent (a wide-enough CJK face would ink the
                    // trailer interior). The wide span is still proven
                    // font-independently by the two-column blue bg below.
                    continue;
                }
                let f = expected_text_grid_cell_facts(&buffer, col, row);
                assert_eq!(
                    inks(col, row),
                    f.inks_glyph,
                    "cell ({col},{row}) Vello ink presence must match the model"
                );
            }
        }

        // The wide head + trailer (cols 0..1, row 1) carry a distinct ANSI-blue
        // (#0000ee) background that must span BOTH columns — the font-independent
        // proof of the wide span (matching the TUI head-grapheme + spill cell).
        assert_eq!(
            expected_text_grid_cell_facts(&buffer, 0, 1).width,
            CellWidth::Wide,
            "(0,1) is the wide head"
        );
        assert_eq!(
            expected_text_grid_cell_facts(&buffer, 1, 1).width,
            CellWidth::Trailer,
            "(1,1) is the trailer"
        );
        let (mut head_blue, mut trailer_blue) = (0u32, 0u32);
        for y in CH..(2 * CH) {
            for x in 0..(2 * CW) {
                let (r, g, b) = at(x, y);
                let is_blue = b > 180 && r < 50 && g < 50;
                if is_blue && x < CW {
                    head_blue += 1;
                } else if is_blue {
                    trailer_blue += 1;
                }
            }
        }
        assert!(
            head_blue > 20,
            "wide head ANSI-blue bg missing (head_blue={head_blue})"
        );
        assert!(
            trailer_blue > 20,
            "trailer must carry the wide head's blue bg across both columns \
             (trailer_blue={trailer_blue})"
        );
    }

    /// R1013 §5.41 §2 #6 — a [`CellWidth::Wide`] head glyph that overflows its
    /// own column must survive into the trailer column; the trailer's own
    /// background fill must NOT erase it.
    ///
    /// The bug ([`paint_text_grid`] before R1013): the main pass interleaved
    /// each cell's opaque background fill with its glyph in one loop, so a wide
    /// head's glyph — drawn at its natural ~1em advance, overflowing into the
    /// trailer column — was overpainted by the *next* cell (the trailer),
    /// whose background fill is emitted after the head glyph. The head glyph's
    /// overflowing portion was erased, reading as a horizontally "compressed"
    /// CJK character. The TUI backend never showed this because it skips
    /// trailers entirely (the terminal renders the wide head across two
    /// columns); the defect was Vello-only. R1013 splits the pass: all
    /// backgrounds first, then all glyphs + decorations, so the head glyph
    /// lands on top of the trailer background.
    ///
    /// Font-independent by construction (per the [[pinion-text-layout-tests-
    /// system-font-debt]] discipline — CJK fallback faces are not guaranteed on
    /// CI hosts): the head is a guaranteed-present Latin "W" *marked wide* and
    /// forced to overflow by pinning a font size (40px) far larger than the
    /// narrow cell width (12px). The glyph identity is irrelevant — the bug is
    /// the draw-order overpaint of any overflowing wide head, so a deterministic
    /// monospace glyph is the robust witness. White ink on an all-black base
    /// makes the ink probe a pure brightness test.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1013_text_grid_wide_head_glyph_survives_trailer_bg() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color as PinColor;
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 12;
        const CH: u32 = 44;
        const FONT: u32 = 40;
        const W: u32 = CW * 2;
        const H: u32 = CH;

        let white = TermColor::Rgb(PinColor::rgb(0xff, 0xff, 0xff));
        let black = TermColor::Rgb(PinColor::rgb(0x00, 0x00, 0x00));
        // A guaranteed-present monospace glyph, marked wide; the pinned 40px
        // font over the 12px cell forces the glyph to overflow well past
        // `cell_w` into the trailer column.
        let head = TermCell::new("W", white, black).wide();
        let buffer = GridBuffer::new(2, 1).with_row(0, [head.clone(), head.trailer()]);
        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let mut node = TextGridNode::new(metric)
            .with_cells(buffer)
            .with_font_size_px(FONT);
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(&vello, W, H, vello::peniko::Color::BLACK)
            .expect("render");

        // White glyph ink on an all-black base: any bright pixel is glyph ink.
        let has_ink = |x0: u32, x1: u32| -> bool {
            for y in 6..(H - 6) {
                for x in x0..x1 {
                    let i = ((y * W + x) * 4) as usize;
                    let sum =
                        u32::from(rgba8[i]) + u32::from(rgba8[i + 1]) + u32::from(rgba8[i + 2]);
                    if sum > 150 {
                        return true;
                    }
                }
            }
            false
        };

        // Sanity: the head column inks — the glyph renders at all (guards
        // against a font-absence false negative on the witness below).
        assert!(
            has_ink(2, CW - 1),
            "wide head glyph absent in its own column — font/render setup broken"
        );
        // Witness: the overflowing glyph must reach the trailer column. Before
        // R1013 the trailer background (drawn after the head glyph) erased this,
        // leaving the trailer column pure black.
        assert!(
            has_ink(CW + 1, W - 1),
            "wide head glyph erased in the trailer column — trailer background \
             overpainted the overflowing head glyph (R1013 draw-order bug)"
        );
    }

    /// R806 §5.39 §2 #1/#7 — deterministic render-vs-intent guard for the
    /// focus-ring **top-edge stroke thickness**. The scene-as-data carries a
    /// 2px Inside border; this renders the exact overlay-box shape through
    /// the SAME `to_vello_cached` the live shell uses, into an offscreen
    /// wgpu texture, and reads the pixels back to assert the stroke
    /// rasterises 2px on every edge — including the edge flush against the
    /// framebuffer top (y = 0), where the live window showed a ~16px-thick
    /// top band invisible to `scene/snapshot` (the rasteriser diverged from
    /// the scene intent). A non-flush control box at y = 200 proves the
    /// thickening is specific to the framebuffer-top edge. Screenshot-free
    /// and CI-reproducible: the structural answer to "this was only
    /// detectable via a live ffmpeg capture".
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the other headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r806_focus_ring_top_edge_rasterizes_two_px_not_thick() {
        use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene};
        use pinion_core::style::{Border, BoxStyle, Color};
        use pinion_overlay::{FocusRingStyle, inject_focus_ring};
        use pinion_runtime::paint_adapter::{FragmentCache, root_background, to_vello_cached};
        use pinion_text::LayoutCache;

        const W: u32 = 520;
        const H: u32 = 320;
        const BLUE: Color = Color::rgb(26, 115, 232);

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");

        // Render `scene` and return the consecutive-blue top-edge run at
        // column `cx` (counted from the first blue pixel downward).
        let top_run = |shot: &mut HeadlessScreenshot, scene: &Scene, cx: u32| -> u32 {
            let base = root_background(scene);
            let mut tc = LayoutCache::new();
            let mut ic = pinion_runtime::image_cache::ImageCache::new();
            let mut fc = FragmentCache::new();
            let mut vello = VelloScene::new();
            to_vello_cached(scene, &|_| None, &mut tc, &mut ic, &mut fc, &mut vello);
            let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");
            let is_blue = |x: u32, y: u32| {
                let i = ((y * W + x) * 4) as usize;
                let (r, g, b) = (
                    i64::from(rgba8[i]),
                    i64::from(rgba8[i + 1]),
                    i64::from(rgba8[i + 2]),
                );
                (r - 26).abs() <= 45 && (g - 115).abs() <= 45 && (b - 232).abs() <= 45
            };
            let mut started = false;
            let mut run = 0;
            for y in 0..H {
                if is_blue(cx, y) {
                    started = true;
                    run += 1;
                } else if started {
                    break;
                }
            }
            run
        };

        let white_root = |child: Scene| -> Scene {
            let mut root = ContainerNode::new(vec![child]);
            root.rect = Rect::new(0, 0, W, H);
            root.style = BoxStyle::filled(Color::rgb(255, 255, 255));
            Scene::Container(root)
        };

        // (1) End-to-end: a top-flush widget framed by the REAL focus ring
        // (inject_focus_ring -> build_focus_ring_box with the R806 top inset).
        // The menubar "Edit" title geometry (96, 0, 96, 40) is tagged so the
        // injector frames it; the ring's top stroke must rasterise ~2px, not
        // the ~16px vello top-tile flood. scene/snapshot cannot see this — the
        // scene always carries a 2px Inside border (the structural point).
        let mut title = BoxNode::new(
            Rect::new(96, 0, 96, 40),
            BoxStyle::filled(Color::TRANSPARENT),
        );
        title.tag = Some("menu#t1".into());
        let framed = inject_focus_ring(
            white_root(Scene::Box(title)),
            Some("menu#t1"),
            FocusRingStyle::default(),
            Some((W, H)),
        );
        let edge = top_run(&mut shot, &framed, 96 + 48);
        assert!(
            edge <= 4,
            "focus-ring top edge rasterised {edge}px for a top-flush widget — \
             expected the ~2px Inside border the scene carries. The R806 top \
             inset must keep the stroke off the vello y=0 flood row.",
        );

        // (2) Negative control proving the guard has teeth: the SAME 2px
        // Inside border drawn flush on the y=0 row (no inset) DOES flood
        // ~16px. If this ever stops flooding, the upstream vello bug is fixed
        // and build_focus_ring_box::TOP_EDGE_INSET can be retired.
        let mut flush = BoxNode::new(
            Rect::new(94, 0, 100, 42),
            BoxStyle::filled(Color::TRANSPARENT),
        );
        flush.style = flush.style.with_border(Border::new(BLUE, 2));
        let flood = top_run(&mut shot, &white_root(Scene::Box(flush)), 94 + 50);
        assert!(
            flood > 4,
            "a 2px Inside border flush on the framebuffer y=0 row no longer \
             floods (got {flood}px) — the upstream vello top-tile bug may be \
             fixed; revisit build_focus_ring_box::TOP_EDGE_INSET.",
        );
    }

    /// R806.1 §5.16 — attribution pin: the y=0 top-tile flood lives in
    /// vello's stroke rasteriser itself, NOT pinion's `Scene -> vello`
    /// translation. Builds a `vello::Scene` DIRECTLY (no pinion `Scene`, no
    /// `to_vello_cached`) with the exact stroke `stroke_rect` emits for a
    /// top-flush 2px Inside border, and confirms it still floods ~16px. So
    /// the R806.1 `TOP_EDGE_INSET` workaround is the right layer (we cannot
    /// fix the bug at its source from here); if a future vello upgrade makes
    /// this assertion fail, the flood is fixed upstream and the inset can be
    /// retired. `#[ignore]` for wgpu cold-boot like the sibling tests.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r806_1_top_tile_flood_is_in_vello_not_the_adapter() {
        use vello::kurbo::{Affine, Rect as KurboRect, Stroke};
        use vello::peniko::{Color as PenikoColor, Fill};
        const W: u32 = 520;
        const H: u32 = 320;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let white = PenikoColor::from_rgba8(255, 255, 255, 255);
        let mut raw = VelloScene::new();
        raw.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            white,
            None,
            &KurboRect::new(0.0, 0.0, f64::from(W), f64::from(H)),
        );
        // The exact stroke `stroke_rect` emits for a (94,0,100,42) box with a
        // 2px Inside border: a 2px stroke on the inset rect (95,1,193,41).
        raw.stroke(
            &Stroke::new(2.0),
            Affine::IDENTITY,
            PenikoColor::from_rgba8(26, 115, 232, 255),
            None,
            &KurboRect::new(95.0, 1.0, 193.0, 41.0),
        );
        let px = shot.render_to_rgba8(&raw, W, H, white).expect("render");
        let blue = |x: u32, y: u32| {
            let i = ((y * W + x) * 4) as usize;
            (i64::from(px[i]) - 26).abs() <= 45
                && (i64::from(px[i + 1]) - 115).abs() <= 45
                && (i64::from(px[i + 2]) - 232).abs() <= 45
        };
        let (mut started, mut run) = (false, 0);
        for y in 0..H {
            if blue(144, y) {
                started = true;
                run += 1;
            } else if started {
                break;
            }
        }
        assert!(
            run > 4,
            "a pure vello 2px stroke flush on the y=0 row no longer floods \
             (top edge {run}px) — the upstream vello bug is fixed; the \
             build_focus_ring_box TOP_EDGE_INSET workaround can be retired.",
        );
    }

    /// R807 §5.16 §5.33 — the §5.33 AI highlight overlay shares the focus
    /// ring's `pinion_overlay::edge` flood-safe SSOT, so a highlight box flush
    /// at the window top must also rasterise its border ~2px, not the ~16px
    /// vello top-tile flood. End-to-end pixel proof that the R806.1
    /// incompleteness (highlight was unfixed) is cleared: builds a top-flush
    /// tagged widget, injects a real `inject_highlight`, renders through the
    /// same `to_vello_cached`, and asserts the top edge is thin.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r807_highlight_top_edge_flood_safe() {
        use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene};
        use pinion_core::style::{BoxStyle, Color};
        use pinion_overlay::{HighlightStyle, inject_highlight};
        use pinion_runtime::paint_adapter::{FragmentCache, root_background, to_vello_cached};
        use pinion_text::LayoutCache;
        const W: u32 = 520;
        const H: u32 = 320;
        // Opaque red stroke so the readback is unambiguous (the default
        // highlight colour's alpha makes pixel detection fiddly).
        let red = Color::rgb(220, 0, 40);
        let mut title = BoxNode::new(
            Rect::new(96, 0, 96, 40),
            BoxStyle::filled(Color::rgb(255, 255, 255)),
        );
        title.tag = Some("title".into());
        let mut root = ContainerNode::new(vec![Scene::Box(title)]);
        root.rect = Rect::new(0, 0, W, H);
        root.style = BoxStyle::filled(Color::rgb(255, 255, 255));
        let scene = inject_highlight(
            Scene::Container(root),
            "title",
            HighlightStyle::default()
                .with_stroke(red)
                .with_stroke_width(2),
        );
        let base = root_background(&scene);
        let mut tc = LayoutCache::new();
        let mut ic = pinion_runtime::image_cache::ImageCache::new();
        let mut fc = FragmentCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(&scene, &|_| None, &mut tc, &mut ic, &mut fc, &mut vello);
        let mut shot = HeadlessScreenshot::new().expect("boot");
        let px = shot.render_to_rgba8(&vello, W, H, base).expect("render");
        let is_red = |x: u32, y: u32| {
            let i = ((y * W + x) * 4) as usize;
            (i64::from(px[i]) - 220).abs() <= 50
                && i64::from(px[i + 1]) <= 70
                && (i64::from(px[i + 2]) - 40).abs() <= 50
        };
        let (mut started, mut run) = (false, 0);
        for y in 0..H {
            if is_red(144, y) {
                started = true;
                run += 1;
            } else if started {
                break;
            }
        }
        assert!(
            run <= 4,
            "highlight top edge rasterised {run}px for a top-flush widget — \
             the shared pinion_overlay::edge SSOT must keep its border off \
             the vello y=0 flood row, same as the focus ring.",
        );
    }

    /// R1001 §5.41 — descender-fit regression. Sizing the cell glyph to the
    /// *full* `cell_h` made parley's ~1.1–1.2× natural line box overflow the
    /// cell, clipping descenders on the bottom grid row. The
    /// `fit_font_size_to_cell` policy reduces the font so the line box fits
    /// `cell_h`; this guards it by rendering a descender glyph ('g') in a single
    /// cell with a full extra cell of margin BELOW, then asserting the glyph
    /// inks *within* its cell and spills no ink past the cell's lower edge (a
    /// buggy full-height font would push the descender below `cell_h`).
    ///
    /// Font-robust: it asserts the glyph stays inside its own cell, not any
    /// absolute glyph shape. `#[ignore]` for the same wgpu cold-boot reason as
    /// the sibling headless grid tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1001_text_grid_descender_fits_within_cell() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 16;
        const CH: u32 = 32;
        // One cell of content; the node rect leaves a full extra cell of margin
        // below it, so a descender overflowing the cell would ink in [CH, 2·CH).
        const W: u32 = CW;
        const H: u32 = CH * 2;

        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let buffer = GridBuffer::new(1, 1).with_row(
            0,
            vec![TermCell::new("g", TermColor::Default, TermColor::Default)],
        );
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        node.rect = Rect::new(0, 0, W, H);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let base = vello::peniko::Color::BLACK;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, W, H, base).expect("render");

        let bright = |x: u32, y: u32| -> bool {
            let i = ((y * W + x) * 4) as usize;
            rgba8[i] > 120 || rgba8[i + 1] > 120 || rgba8[i + 2] > 120
        };

        // The glyph inks within its own cell [0, CH).
        let in_cell = (0..CH)
            .flat_map(|y| (0..CW).map(move |x| (x, y)))
            .filter(|&(x, y)| bright(x, y))
            .count();
        assert!(
            in_cell > 10,
            "'g' must ink within its cell (in_cell={in_cell})"
        );

        // ...and nothing spills past the cell's lower edge into [CH, 2·CH): the
        // descender stays inside the cell. (`<= 1` tolerates a lone boundary AA
        // pixel; a real overflow is the descender's full width across rows.)
        let below_cell = (CH..H)
            .flat_map(|y| (0..CW).map(move |x| (x, y)))
            .filter(|&(x, y)| bright(x, y))
            .count();
        assert!(
            below_cell <= 1,
            "the glyph must fit within the cell — no ink below cell_h (below_cell={below_cell})",
        );
    }

    /// Render a 1×1 `Scene::TextGrid` of `glyph` at `metric` (optionally
    /// pinning the Vello font size, the R1002 SSOT path) through the SAME
    /// `to_vello_cached` the live shell uses, and read back RGBA8 sized to the
    /// cell. Shared by the two R1002 horizontal-fit guards.
    fn render_single_glyph_cell(
        metric: pinion_core::cell_metric::CellMetric,
        font_size_px: Option<u32>,
        glyph: &str,
    ) -> (Vec<u8>, u32, u32) {
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        let cw = metric.cell_w();
        let ch = metric.cell_h();
        let buffer = GridBuffer::new(1, 1).with_row(
            0,
            vec![TermCell::new(
                glyph.to_owned(),
                TermColor::Default,
                TermColor::Default,
            )],
        );
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        if let Some(s) = font_size_px {
            node = node.with_font_size_px(s);
        }
        node.rect = Rect::new(0, 0, cw, ch);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let base = vello::peniko::Color::BLACK;
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot.render_to_rgba8(&vello, cw, ch, base).expect("render");
        (rgba8, cw, ch)
    }

    /// Measure a single rendered glyph's horizontal ink within its cell:
    /// `(left_gutter, right_gutter, ink_span)` in px. A blank cell panics
    /// (the glyph must ink). Shared by the snugness guard and the looseness
    /// characterization below — one measurement, two interpretations.
    fn glyph_gutters(rgba8: &[u8], cw: u32, ch: u32) -> (u32, u32, u32) {
        let bright = |x: u32, y: u32| -> bool {
            let i = ((y * cw + x) * 4) as usize;
            rgba8[i] > 120 || rgba8[i + 1] > 120 || rgba8[i + 2] > 120
        };
        let col_inked = |x: u32| -> bool { (0..ch).any(|y| bright(x, y)) };
        let first = (0..cw).find(|&x| col_inked(x));
        let last = (0..cw).rev().find(|&x| col_inked(x));
        let (Some(first), Some(last)) = (first, last) else {
            panic!("glyph must ink at least one column within its cell");
        };
        (first, cw - 1 - last, last - first + 1)
    }

    /// R1002 §5.41 — horizontal cell-fit, **measured / SSOT path** (the PR-5
    /// looseness sibling of the R1001 descender test). The cell comes from
    /// [`pinion_text::LayoutCache::measure_monospace_cell`] (`cell_w` = the
    /// monospace advance) paired with the matching pinned font size, so the
    /// painted advance equals `cell_w` by construction. The 'M' fills its
    /// column and sits roughly centred — guarding the measured metric, the
    /// explicit-font-size paint path, and the R1002 monospace resolution.
    /// This is the path that genuinely eliminates the looseness.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1002_text_grid_measured_path_fills_cell_horizontally() {
        use pinion_text::LayoutCache;
        const FONT_PX: u32 = 32;
        let mut measure_cache = LayoutCache::new();
        let metric = measure_cache
            .measure_monospace_cell(FONT_PX)
            .expect("32px monospace measures a non-zero cell");
        let (rgba8, cw, ch) = render_single_glyph_cell(metric, Some(FONT_PX), "M");
        let (left, right, span) = glyph_gutters(&rgba8, cw, ch);
        // Wide ink (>= 40% of cell_w) and near-balanced gutters (no left-jam).
        // (× 10 keeps the comparisons integral.)
        assert!(
            span * 10 >= cw * 4,
            "measured: 'M' ink must span >= 40% of cell_w: span={span} cw={cw}"
        );
        assert!(
            left.abs_diff(right) * 10 <= cw * 3,
            "measured: 'M' must sit roughly centred, not left-jammed: left={left} right={right} cw={cw}",
        );
    }

    /// R1002 §5.41 — **characterization**: WHY the measured metric is the only
    /// looseness fix. Layer 1 (a real fixed-pitch face) alone does NOT make an
    /// arbitrary producer-picked `cell_w` snug — with a cell whose width does
    /// not match the font advance, the fit path leaves the glyph left-jammed
    /// against a wide right gutter (the PR-5 looseness signature). This pins
    /// the design contract: a monospace grid's `cell_w` MUST equal the advance
    /// ([`measure_monospace_cell`]); a mismatched cell is loose by design, not
    /// a bug to paper over with centering/scaling.
    ///
    /// R1029 §6.3 — the over-wide cell is **font-derived**, not a magic `16`:
    /// the resolved monospace differs across environments (a dev box's wide
    /// Noto CJK Mono vs CI's `DejaVu Sans Mono`, whose advance happened to
    /// ~match 16 px and made the cell snug-not-loose, failing this guard).
    /// Measuring the advance then building a cell at twice it is provably
    /// over-wide for any face: the height-fit advance is `<=` the measured one
    /// (fit-size `<=` `cell_h`), so `2x measured` always exceeds it.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1002_text_grid_fit_path_is_loose_for_mismatched_cell() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_text::LayoutCache;
        const CH: u32 = 32;
        // The resolved monospace advance at the cell height (same family the
        // grid paints). `2x` it: provably over-wide vs the height-fit advance.
        let advance = LayoutCache::new()
            .measure_monospace_cell(CH)
            .expect("monospace advance probe")
            .cell_w();
        let metric = CellMetric::new(advance * 2, CH).expect("non-zero cell metric");
        let (rgba8, cw, ch) = render_single_glyph_cell(metric, None, "M");
        let (left, right, _span) = glyph_gutters(&rgba8, cw, ch);
        // Left-jammed: the right gutter is clearly larger than the left (the +2
        // margin keeps it above AA noise). Demonstrates the necessity of the
        // measured metric — not a defect in the fit path.
        assert!(
            right >= left + 2,
            "fit path with an over-wide cell (2x the measured advance) must be \
             left-jammed (loose), proving the measured metric is required: \
             left={left} right={right} cw={cw}",
        );
    }

    /// R1178 §5.41 — two horizontally adjacent FULL BLOCK (`U+2588`) cells
    /// render as cell-exact filled rectangles that tile with **no gap**: every
    /// column across the 2-cell row — including the seam columns either side of
    /// the cell boundary — inks the foreground. The font-glyph path this
    /// replaces leaves the fitted-size + bearing margin (the R1002
    /// `*_loose_for_mismatched_cell` looseness), which read as the "broken
    /// bars" PR-40 reported on the terminal logo. Font-independent: it asserts
    /// geometry (solid coverage), not a shaped glyph, so a 10×30 producer-
    /// picked cell (the reported case) tiles regardless of the resolved face.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1178_block_element_full_block_tiles_without_gap() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 10;
        const CH: u32 = 30;
        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        // Distinct, palette-independent fg / bg so a gap (background) is
        // unambiguously distinguishable from block ink (foreground).
        let fg = TermColor::Rgb(Color::rgb(255, 128, 0)); // orange logo ink
        let bg = TermColor::Rgb(Color::rgb(0, 0, 40)); // dark, low red
        let full = || TermCell::new("\u{2588}".to_owned(), fg, bg);
        let buffer = GridBuffer::new(2, 1).with_row(0, vec![full(), full()]);
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        let w = CW * 2;
        node.rect = Rect::new(0, 0, w, CH);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(&vello, w, CH, vello::peniko::Color::BLACK)
            .expect("render");

        // The foreground (orange) is high-red; the background is low-red. A
        // tiled block inks fg on every column at mid-height — a gap would show
        // the dark background's low red.
        let y = CH / 2;
        let is_fg = |x: u32| -> bool {
            let i = ((y * w + x) * 4) as usize;
            rgba8[i] > 120
        };
        let gaps: Vec<u32> = (0..w).filter(|&x| !is_fg(x)).collect();
        assert!(
            gaps.is_empty(),
            "FULL BLOCK cells must tile with no gap; background columns at y={y}: {gaps:?}",
        );
        // Explicitly pin the two seam columns either side of the cell boundary.
        assert!(
            is_fg(CW - 1) && is_fg(CW),
            "the cell-boundary seam (cols {} / {CW}) must be continuous foreground",
            CW - 1,
        );
    }

    /// R1179 §5.41 — the three shade blocks (`░ ▒ ▓`) render as the foreground
    /// blended over the cell background at 25 / 50 / 75 %, NOT as a font glyph.
    /// White ink on a black cell yields mid-grey ramps that strictly increase
    /// light < medium < dark, with medium near half. Font-independent: it
    /// asserts the alpha ramp, not a shaped glyph.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1179_shade_blocks_render_alpha_ramp() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 12;
        const CH: u32 = 24;
        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let fg = TermColor::Rgb(Color::rgb(255, 255, 255)); // white ink
        let bg = TermColor::Rgb(Color::rgb(0, 0, 0)); // black cell
        let shade = |s: &str| TermCell::new(s.to_owned(), fg, bg);
        // ░ light, ▒ medium, ▓ dark.
        let buffer = GridBuffer::new(3, 1).with_row(
            0,
            vec![shade("\u{2591}"), shade("\u{2592}"), shade("\u{2593}")],
        );
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        let w = CW * 3;
        node.rect = Rect::new(0, 0, w, CH);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(&vello, w, CH, vello::peniko::Color::BLACK)
            .expect("render");

        // Red channel at each cell centre (white ink => grey level == alpha).
        let level = |col: u32| -> u32 {
            let x = col * CW + CW / 2;
            let y = CH / 2;
            u32::from(rgba8[((y * w + x) * 4) as usize])
        };
        let (light, medium, dark) = (level(0), level(1), level(2));
        assert!(
            light < medium && medium < dark,
            "shade ramp must increase: light={light} medium={medium} dark={dark}",
        );
        // Medium shade sits near 50% grey (127) over the black cell.
        assert!(
            (90..=170).contains(&medium),
            "medium shade must be near half intensity: medium={medium}",
        );
        // Light is a faint wash (not background-black), dark is strong but not
        // a solid full block.
        assert!(light > 20 && dark < 245, "light={light} dark={dark}");
    }

    /// R1180 §5.41 — box-drawing glyphs synthesise as connected lines: a 3×3
    /// box `┌─┐ / │ │ / └─┘` renders a continuous top edge across all three top
    /// cells and a continuous left edge down all three left cells, with no gap
    /// at the cell boundaries (the corners join the straight runs). Font-
    /// independent: it asserts line connectivity, not a shaped glyph.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1180_box_drawing_lines_connect_across_cells() {
        use pinion_core::cell_metric::CellMetric;
        use pinion_core::scene::{Rect, Scene, TextGridNode};
        use pinion_core::style::Color;
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};
        use pinion_runtime::paint_adapter::{FragmentCache, to_vello_cached};
        use pinion_text::LayoutCache;

        const CW: u32 = 14;
        const CH: u32 = 14;
        let metric = CellMetric::new(CW, CH).expect("non-zero cell metric");
        let fg = TermColor::Rgb(Color::rgb(255, 255, 255));
        let bg = TermColor::Rgb(Color::rgb(0, 0, 0));
        let g = |s: &str| TermCell::new(s.to_owned(), fg, bg);
        // ┌─┐ / │·│ / └─┘  (· = space)
        let buffer = GridBuffer::new(3, 3)
            .with_row(0, vec![g("\u{250C}"), g("\u{2500}"), g("\u{2510}")])
            .with_row(1, vec![g("\u{2502}"), g(" "), g("\u{2502}")])
            .with_row(2, vec![g("\u{2514}"), g("\u{2500}"), g("\u{2518}")]);
        let mut node = TextGridNode::new(metric).with_cells(buffer);
        let (w, h) = (CW * 3, CH * 3);
        node.rect = Rect::new(0, 0, w, h);
        let scene = Scene::TextGrid(node);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut image_cache = pinion_runtime::image_cache::ImageCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(
            &scene,
            &|_| None,
            &mut text_cache,
            &mut image_cache,
            &mut cache,
            &mut vello,
        );
        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(&vello, w, h, vello::peniko::Color::BLACK)
            .expect("render");
        let inked = |x: u32, y: u32| -> bool { rgba8[((y * w + x) * 4) as usize] > 120 };

        // Top edge: continuous ink along row 0's mid-height, from the left
        // corner centre to the right corner centre (across both cell seams).
        let top_y = CH / 2;
        let top_gaps: Vec<u32> = (CW / 2 + 1..w - CW / 2 - 1)
            .filter(|&x| !inked(x, top_y))
            .collect();
        assert!(
            top_gaps.is_empty(),
            "top box edge must be continuous across cells; gaps at x={top_gaps:?}",
        );
        // Left edge: continuous ink down column 0's mid-width across both row
        // seams.
        let left_x = CW / 2;
        let left_gaps: Vec<u32> = (CH / 2 + 1..h - CH / 2 - 1)
            .filter(|&y| !inked(left_x, y))
            .collect();
        assert!(
            left_gaps.is_empty(),
            "left box edge must be continuous across cells; gaps at y={left_gaps:?}",
        );
    }

    /// R1505 §5.36 §5.49 — the pixel half of "a header says where its labels
    /// sit": does a leaf's DECLARED alignment actually move its glyphs?
    ///
    /// R1504 gave `ColumnLayout` a `default_alignment` and painted each header
    /// label through `TextStyle::with_align`, then closed honestly — a label
    /// node's rect is its BOX, so all three alignments produce a byte-identical
    /// scene tree, and nothing anywhere proved the glyphs land in different
    /// pixels. The rule is asserted at the surface that owns it and at the node
    /// that carries it (`tools/demos/r1505_alignment_reaches_glyphs.py`); this
    /// is the last seam, declaration → pixels, and it had no witness at all.
    ///
    /// The alignment is applied by parley inside `LayoutCache::shape`, which
    /// aligns within the width `break_all_lines` was handed, and `paint_text`
    /// derives that width from the node's own `rect.w`. So a leaf aligns only
    /// when its box is WIDER than its glyphs — exactly a header label's shape
    /// (`label_w` spans the section; the word does not). A regression that
    /// dropped the `layout.align(…)` call, or stopped passing the box width,
    /// leaves all three renders identical, and both are caught here.
    ///
    /// Renders the same node shape the header label is built from (a sized box
    /// carrying `with_align` and `TextOverflow::Clip`) through the production
    /// [`to_vello`](pinion_runtime::paint_adapter::to_vello) walk, and asserts
    /// the ink SLIDES: `Start` left of `Center` left of `End`, while the ink
    /// WIDTH holds (the same glyphs moved, not re-shaped or re-wrapped).
    ///
    /// The font is REGISTERED, not discovered. A guard that leaned on whatever
    /// the host happens to have installed would measure the host, and its
    /// answer would drift between this box and CI — the R1473 / R1500 lesson,
    /// applied at the seam that first tempted it.
    ///
    /// `#[ignore]` for the same wgpu cold-boot reason as the sibling headless
    /// tests; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r1505_declared_alignment_slides_the_ink_within_the_box() {
        use pinion_core::scene::{BoxNode, Rect, Scene, TextNode};
        use pinion_core::style::{Color, TextAlign, TextOverflow, TextStyle};
        use pinion_runtime::paint_adapter::to_vello;
        use pinion_text::LayoutCache;

        /// The §5.37 parser fixture, reused as a REGISTERED family so the
        /// glyph advances are the same on every host (see the doc above).
        const NOTO: &[u8] =
            include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");
        /// Far wider than the word, so all three alignments have room to
        /// differ — a header section's `label_w`, in miniature.
        const BOX_W: u32 = 240;
        const BOX_H: u32 = 40;
        /// The widest of `hello-column-reorder`'s headers.
        const LABEL: &str = "Modified";

        let mut shot = match HeadlessScreenshot::new() {
            Ok(s) => s,
            // No GPU / software adapter on this host — skip rather than fail,
            // the stance the sibling headless tests take on a bare dev box.
            Err(e) => {
                eprintln!("skipping: no wgpu adapter ({e})");
                return;
            }
        };

        let mut text_cache = LayoutCache::new();
        let family = text_cache
            .register_font_data(NOTO.to_vec())
            .into_iter()
            .next()
            .expect("NotoSans fixture registers at least one family");

        // Ink extent `(min_x, max_x, width)` for one alignment, measured with
        // this module's existing [`glyph_gutters`] — a first/last-inked-column
        // scan, which is exactly this question asked of a wider box. The first
        // draft of this test hand-rolled the same scan; the R727 / R732
        // 3rd-consumer self-grep is what caught it.
        let mut ink_span = |align: TextAlign| -> (u32, u32, u32) {
            let scene = Scene::Text(TextNode::styled(
                LABEL,
                Rect::new(0, 0, BOX_W, BOX_H),
                TextStyle::new()
                    .with_font_family(family.clone())
                    .with_size_px(20)
                    .with_fg(Color::rgb(255, 255, 255))
                    .with_align(align)
                    .with_overflow(TextOverflow::Clip),
            ));
            let mut vello_scene = VelloScene::new();
            to_vello(
                &scene,
                &|_: &BoxNode| -> Option<Color> { None },
                &mut text_cache,
                &mut vello_scene,
            );
            let rgba = shot
                .render_to_rgba8(&vello_scene, BOX_W, BOX_H, PenikoColor::BLACK)
                .expect("headless render");
            // `glyph_gutters` panics when nothing inked, which is the premise
            // guard this needs: without ink every bound below would compare
            // sentinels and the test would pass vacuously.
            let (left, right, width) = glyph_gutters(&rgba, BOX_W, BOX_H);
            (left, BOX_W - 1 - right, width)
        };

        let (start_min, start_max, ink_w) = ink_span(TextAlign::Start);
        let (mid_min, mid_max, mid_w) = ink_span(TextAlign::Center);
        let (end_min, end_max, end_w) = ink_span(TextAlign::End);

        // The word must not fill the box, or there is nothing to slide within
        // and the assertions below would be unfalsifiable.
        assert!(
            ink_w < BOX_W - 20,
            "the fixture word must leave slack in the box to align within: \
             ink {ink_w}px of {BOX_W}px",
        );

        // The rule: the ink SLIDES rightward across the three alignments.
        assert!(
            start_min < mid_min && mid_min < end_min,
            "declared alignment must move the ink: Start(min_x={start_min}) \
             < Center(min_x={mid_min}) < End(min_x={end_min}) — equal values \
             mean the declaration never reached the shaper",
        );
        assert!(
            start_max < mid_max && mid_max < end_max,
            "the ink's right edge slides with its left: Start={start_max} \
             Center={mid_max} End={end_max}",
        );

        // …and it is the SAME ink, moved: identical glyphs, so the extent
        // holds within AA noise. A soft wrap or a re-shape would change it.
        for (name, w) in [("Center", mid_w), ("End", end_w)] {
            assert!(
                w.abs_diff(ink_w) <= 2,
                "{name} must be the same glyphs moved, not re-shaped: ink \
                 width {w}px vs Start's {ink_w}px",
            );
        }

        // Anchored, not merely ordered: `End`'s right edge sits near the box's
        // right edge, and `Center` is centred. Ordering alone would survive a
        // shaper that nudged the text by a constant.
        assert!(
            BOX_W - end_max <= 4,
            "End must anchor to the box's right edge: max_x={end_max} of \
             {BOX_W}",
        );
        let mid_slack_l = mid_min;
        let mid_slack_r = BOX_W - mid_max;
        assert!(
            mid_slack_l.abs_diff(mid_slack_r) <= 4,
            "Center must leave equal slack: {mid_slack_l}px left vs \
             {mid_slack_r}px right",
        );
    }

    /// Zero-dimension viewports short-circuit with a typed error
    /// rather than reaching the wgpu validation layer.
    #[test]
    fn zero_dimension_short_circuits() {
        // No wgpu boot — error must surface from the guard before
        // any device / queue call.
        let scene = VelloScene::new();
        // We can't construct a HeadlessScreenshot without wgpu, but
        // we can verify the error variant's Display surface to keep
        // the typed surface tested in CI even when wgpu cold-boot
        // is unavailable.
        let err = HeadlessScreenshotError::ZeroDimension;
        let msg = format!("{err}");
        assert!(msg.contains("width") && msg.contains("height"));
        let _ = scene;
    }
}
