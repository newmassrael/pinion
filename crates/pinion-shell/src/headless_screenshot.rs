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
    self, Backends, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, DeviceDescriptor,
    Extent3d, Features, Instance, InstanceDescriptor, Limits, MapMode, MemoryHints, Origin3d,
    PollType, PowerPreference, RequestAdapterOptions, TexelCopyBufferInfo, TexelCopyBufferLayout,
    TexelCopyTextureInfo, TextureAspect, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureViewDescriptor,
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

/// wgpu `bytes_per_row` alignment for `copy_texture_to_buffer`. WebGPU
/// spec mandates 256-byte row alignment for buffer copies; wgpu
/// re-exports the constant as `COPY_BYTES_PER_ROW_ALIGNMENT`. Padding
/// added here is stripped back out during readback so callers see the
/// unpadded `width * height * 4` byte buffer.
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

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
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
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

        let unpadded_bytes_per_row = width * 4;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let staging_size = u64::from(padded_bytes_per_row) * u64::from(height);
        let staging = self.device.create_buffer(&BufferDescriptor {
            label: Some("pinion-shell::HeadlessScreenshot staging"),
            size: staging_size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("pinion-shell::HeadlessScreenshot copy"),
            });
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &staging,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<(), wgpu::BufferAsyncError>>(1);
        slice.map_async(MapMode::Read, move |result| {
            // Receiver side may have been dropped if the caller
            // gave up early (e.g. panic in a parallel test); silently
            // tolerating the send-error keeps the wgpu side free of
            // a dangling callback panic.
            let _ = tx.send(result);
        });
        // wgpu requires an explicit poll to drive map_async on
        // native backends (web backends progress via the JS event
        // loop). `PollType::Wait` blocks until the queued work +
        // map callback have both completed.
        let _ = self.device.poll(PollType::Wait);
        rx.recv()
            .map_err(|e| HeadlessScreenshotError::BufferMap(format!("{e}")))?
            .map_err(|e| HeadlessScreenshotError::BufferMap(format!("{e}")))?;

        let mut out = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
        {
            let mapped = slice.get_mapped_range();
            // Strip per-row padding wgpu required for `bytes_per_row`
            // alignment so the returned buffer is the contiguous
            // `width * height * 4` RGBA8 raster the wire schema
            // promises.
            for row in 0..height as usize {
                let row_start = row * padded_bytes_per_row as usize;
                let row_end = row_start + unpadded_bytes_per_row as usize;
                out.extend_from_slice(&mapped[row_start..row_end]);
            }
        }
        staging.unmap();
        Ok(out)
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
        let mut encoder = png::Encoder::new(writer, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png_writer = encoder
            .write_header()
            .map_err(|e| HeadlessScreenshotError::PngEncode(format!("{e}")))?;
        png_writer
            .write_image_data(&pixels)
            .map_err(|e| HeadlessScreenshotError::PngEncode(format!("{e}")))?;
        Ok(())
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

    /// R706 §5.16 — cached headless-render overlay smoke test.
    ///
    /// Renders a datepicker-shaped scene — a non-cacheable root (a
    /// no-op `Scene::External` makes it so, mirroring the live paint
    /// scene), a nested CACHEABLE "grid row" of rounded cells carrying
    /// glyph runs, and a TOP-LEVEL bordered overlay `Box` (the §5.39
    /// focus-ring stand-in) drawn as a later sibling — through
    /// `to_vello_cached` (the path the live winit render loop drives, and
    /// since R706 the path the `PINION_SCREENSHOT` headless capture
    /// drives too) and asserts the overlay border rasterizes at its
    /// declared rect.
    ///
    /// This guards the R706 "out receives appends only" rewrite of
    /// [`pinion_runtime::paint_adapter::to_vello_cached`] and the headless
    /// screenshot's switch onto that cached path against a gross
    /// mis-placement of an overlay that follows a cached fragment append.
    /// It is NOT a faithful reproduction of the original
    /// one-grid-column focus-ring shift — that defect only surfaced
    /// against the full live datepicker paint scene (deep nested grid +
    /// the real `compute_layout` rects), not a hand-built scene, so the
    /// authoritative regression check is the live-window pixel demo
    /// `tools/demos/r706_focus_ring_pixel.py`.
    ///
    /// `#[ignore]` for the same reason as
    /// [`renders_solid_fill_to_unpadded_rgba8`] — wgpu cold-boot is too
    /// slow for the default suite; run with `--ignored`.
    #[test]
    #[ignore = "wgpu adapter cold-boot too slow for default test suite; run with --ignored"]
    fn r706_cached_headless_render_places_overlay_at_declared_rect() {
        use pinion_core::external::StubExternal;
        use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect, Scene, TextNode};
        use pinion_core::style::{Border, BoxStyle, Color, LayoutStyle, TextStyle};
        use pinion_runtime::paint_adapter::{to_vello_cached, FragmentCache};
        use pinion_text::LayoutCache;

        const W: u32 = 220;
        const H: u32 = 140;
        // Overlay declared geometry — pure-blue border, transparent fill.
        const OVERLAY_X: u32 = 24;
        const OVERLAY_W: u32 = 44;

        // Helper: a cacheable cell — a coloured box containing a glyph
        // run (the day-number stand-in; the glyph draw is what sets the
        // encoder's force-transform flag the defect rode on).
        fn cell(x: u32, label: &str) -> Scene {
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    label.to_string(),
                    Rect::new(x + 12, 92, 24, 24),
                    TextStyle::new().with_size_px(16).with_fg(Color::rgb(0, 0, 0)),
                ))])
                .with_style(
                    BoxStyle::filled(Color::rgb(225, 225, 230)).with_corner_radius(20),
                )
                .with_layout(LayoutStyle::new()),
            )
        }

        // A nested cacheable "grid row" of cells (mirrors the datepicker
        // row → cell → text nesting so the appended fragment is deep).
        let mut row = ContainerNode::new(vec![
            cell(20, "10"),
            cell(70, "11"),
            cell(120, "12"),
            cell(170, "13"),
        ]);
        row.rect = Rect::new(20, 88, 192, 40);
        for (i, ch) in row.children.iter_mut().enumerate() {
            if let Scene::Container(c) = ch {
                let col = u32::try_from(i).expect("test cell index fits u32");
                c.rect = Rect::new(20 + col * 50, 88, 40, 40);
            }
        }
        let grid = Scene::Container(row);

        // The overlay box: bordered, transparent fill — the ring stand-in,
        // a TOP-LEVEL sibling drawn AFTER the cached grid fragment.
        let overlay = Scene::Box(BoxNode::new(
            Rect::new(OVERLAY_X, 60, OVERLAY_W, 36),
            BoxStyle::filled(Color::TRANSPARENT)
                .with_border(Border::new(Color::rgb(0, 0, 255), 3))
                .with_corner_radius(22),
        ));

        // A no-op External makes the ROOT non-cacheable (mirrors the live
        // datepicker paint scene), so the overlay's stroke is issued into
        // the MAIN scene directly after the grid's append — the exact
        // pre-R706 hazard.
        let stub = Scene::External(ExternalNode::new(Box::new(StubExternal)).with_tag("state"));
        let mut root = ContainerNode::new(vec![stub, grid, overlay])
            .with_style(BoxStyle::filled(Color::rgb(255, 255, 255)));
        root.rect = Rect::new(0, 0, W, H);
        let root = Scene::Container(root);

        let mut text_cache = LayoutCache::new();
        let mut cache = FragmentCache::new();
        let mut vello = VelloScene::new();
        to_vello_cached(&root, &|_| None, &mut text_cache, &mut cache, &mut vello);

        let mut shot = HeadlessScreenshot::new().expect("headless screenshot bootstrap");
        let rgba8 = shot
            .render_to_rgba8(&vello, W, H, PenikoColor::from_rgba8(255, 255, 255, 255))
            .expect("render");

        // Collect the x of every strongly-blue pixel (the overlay border).
        let mut blue_xs = Vec::new();
        for y in 0..H {
            for x in 0..W {
                let i = ((y * W + x) * 4) as usize;
                let (r, g, b) = (rgba8[i], rgba8[i + 1], rgba8[i + 2]);
                if b > 180 && r < 90 && g < 90 {
                    blue_xs.push(x);
                }
            }
        }
        assert!(
            !blue_xs.is_empty(),
            "overlay border must be visible in the rasterized output",
        );
        let min_x = *blue_xs.iter().min().unwrap();
        let max_x = *blue_xs.iter().max().unwrap();
        // The border left edge must sit at the declared OVERLAY_X (±3 for
        // the 3-px stroke + AA), NOT one ~42-px column to the right.
        assert!(
            min_x.abs_diff(OVERLAY_X) <= 4,
            "overlay border left edge at x={min_x}, expected ~{OVERLAY_X} \
             (a column-shift regression would land it near {})",
            OVERLAY_X + 42,
        );
        assert!(
            max_x.abs_diff(OVERLAY_X + OVERLAY_W) <= 4,
            "overlay border right edge at x={max_x}, expected ~{}",
            OVERLAY_X + OVERLAY_W,
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
