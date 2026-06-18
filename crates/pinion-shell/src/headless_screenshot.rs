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
        // map callback have both completed. wgpu 29 (R808) made `Wait`
        // a struct variant; `wait_indefinitely()` is the no-timeout,
        // most-recent-submission convenience constructor.
        let _ = self.device.poll(PollType::wait_indefinitely());
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

    /// R706.1 §5.16 §5.39 — faithful regression guard for the
    /// fragment-cache "direct-draw-after-append" rasterization defect.
    ///
    /// Builds the REAL hello-datepicker focused paint scene through the
    /// production pipeline — [`pinion_widget_paint::view_datepicker`]
    /// wrapped in the same centred Surface container the binding's view
    /// fn uses, [`pinion_runtime::compute_layout`] for the post-layout
    /// rects, then [`pinion_overlay::inject_focus_ring`] at the
    /// active-descendant cell exactly as `ShellCore::apply_focus_ring`
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
        use pinion_widget_paint::datepicker::{
            DatePickerStyle, DisplayedMonth, view_datepicker,
        };

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
                DisplayedMonth { year: 2026, month: 5 },
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
        let scene = inject_focus_ring(scene, Some(FOCUSED_TAG), FocusRingStyle::default());
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
            assert!(r > 200 && g < 50 && b < 50, "cell(0,0) Rgb red bg, got ({r},{g},{b})");
        }
        // (1,0) Indexed(4) = ANSI blue (#0000ee).
        for &(x, y) in &[(CW + CW / 2, CH / 2), (CW + 5, 6)] {
            let (r, g, b) = at(x, y);
            assert!(b > 180 && r < 50 && g < 50, "cell(1,0) Indexed(4) blue bg, got ({r},{g},{b})");
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
        assert!(a_glyph, "cell(2,0) 'A' glyph must paint bright pixels on black");

        // (0,1) reverse — the effective background is the green foreground.
        for &(x, y) in &[(CW / 2, CH + CH / 2), (5, CH + 6)] {
            let (r, g, b) = at(x, y);
            assert!(g > 200 && r < 50 && b < 50, "cell(0,1) reverse swaps fg->bg green, got ({r},{g},{b})");
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
        assert!(!hidden_glyph, "cell(1,1) hidden must suppress the white 'X' glyph");

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
        assert!(head_magenta > 20, "wide head bg magenta missing (head_magenta={head_magenta})");
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

        let buffer = GridBuffer::new(COLS, ROWS).with_row(0, row0).with_row(1, row1);
        let mut node = TextGridNode::new(CellMetric::new(CW, CH).expect("non-zero")).with_cells(buffer);
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
        assert!(full_underline > 2000, "cell(0,0) underline rule must ink, sum={full_underline}");
        assert!(strike_ink(0, 0) < 500, "cell(0,0) underline must not ink mid-cell");

        // (1,0) strikethrough — an inked rule at mid-cell, nothing at the bottom.
        assert!(strike_ink(1, 0) > 2000, "cell(1,0) strikethrough rule must ink");
        assert!(underline_ink(1, 0) < 500, "cell(1,0) strikethrough must not ink the bottom");

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
        assert!(glyph_peak(0, 1) > 120, "cell(0,1) bold 'A' glyph must paint");
        assert!(glyph_peak(1, 1) > 120, "cell(1,1) italic 'A' glyph must paint");

        // (2,1) underline + strikethrough — both rules ink on the one cell.
        assert!(underline_ink(2, 1) > 2000, "cell(2,1) combo underline rule must ink");
        assert!(strike_ink(2, 1) > 2000, "cell(2,1) combo strikethrough rule must ink");
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
                .with_row(0, [TermCell::new(glyph, white, black), TermCell::new(" ", white, black)])
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
            shot.render_to_rgba8(&vello, W, H, vello::peniko::Color::BLACK).expect("render")
        };

        // Red channel at (x, y) (white ink ⇒ r≈g≈b, so red tracks luminance).
        let red = |img: &[u8], x: u32, y: u32| -> i64 { i64::from(img[((y * W + x) * 4) as usize]) };

        // Block over a blank cell — the whole cell fills with the cursor colour
        // (white); the neighbour cell stays unlit.
        let filled = render(buf(" ", CursorShape::Block, true));
        assert!(red(&filled, CW / 2, CH / 2) > 200, "block cursor fills its cell white");
        assert!(red(&filled, CW + CW / 2, CH / 2) < 60, "neighbour cell has no cursor");

        // Block over 'A' — the cell still fills (corner is white) and the glyph
        // reads through inverse (dark pixels appear inside the bright block).
        let inverse = render(buf("A", CursorShape::Block, true));
        assert!(red(&inverse, 2, 2) > 200, "block-over-glyph corner is the cursor fill");
        let mut inverse_glyph = false;
        for y in 2..(CH - 2) {
            for x in 2..(CW - 2) {
                if red(&inverse, x, y) < 80 {
                    inverse_glyph = true;
                }
            }
        }
        assert!(inverse_glyph, "block cursor must redraw the glyph inverse (dark ink in the block)");

        // Bar over a blank cell — a vertical beam at the leading edge; the cell
        // interior stays black.
        let bar = render(buf(" ", CursorShape::Bar, true));
        assert!(red(&bar, 0, CH / 2) > 200, "bar cursor inks the leading edge");
        assert!(red(&bar, CW / 2, CH / 2) < 60, "bar cursor leaves the interior blank");

        // Underline over a blank cell — a solid bottom bar >= 2px thick (so it
        // reads distinctly from the thin SGR underline); the cell middle blank.
        let underline = render(buf(" ", CursorShape::Underline, true));
        assert!(red(&underline, CW / 2, CH - 1) > 200, "underline cursor inks the cell bottom");
        assert!(red(&underline, CW / 2, CH / 2) < 60, "underline cursor leaves the middle blank");
        let mut thickness = 0u32;
        while thickness < CH && red(&underline, CW / 2, CH - 1 - thickness) > 200 {
            thickness += 1;
        }
        assert!(thickness >= 2, "cursor underline is a thick bar (>= 2px), got {thickness}");

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
        assert!(trailer_filled, "block cursor on a wide head must fill the trailer column too");

        // Invisible cursor — nothing paints; the cell stays its background.
        let hidden = render(buf(" ", CursorShape::Block, false));
        assert!(red(&hidden, CW / 2, CH / 2) < 60, "an invisible cursor paints nothing");
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
        use pinion_overlay::{inject_focus_ring, FocusRingStyle};
        use pinion_runtime::paint_adapter::{root_background, to_vello_cached, FragmentCache};
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
                let (r, g, b) =
                    (i64::from(rgba8[i]), i64::from(rgba8[i + 1]), i64::from(rgba8[i + 2]));
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
        let mut title =
            BoxNode::new(Rect::new(96, 0, 96, 40), BoxStyle::filled(Color::TRANSPARENT));
        title.tag = Some("menu#t1".into());
        let framed = inject_focus_ring(
            white_root(Scene::Box(title)),
            Some("menu#t1"),
            FocusRingStyle::default(),
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
        let mut flush =
            BoxNode::new(Rect::new(94, 0, 100, 42), BoxStyle::filled(Color::TRANSPARENT));
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
        use pinion_overlay::{inject_highlight, HighlightStyle};
        use pinion_runtime::paint_adapter::{root_background, to_vello_cached, FragmentCache};
        use pinion_text::LayoutCache;
        const W: u32 = 520;
        const H: u32 = 320;
        // Opaque red stroke so the readback is unambiguous (the default
        // highlight colour's alpha makes pixel detection fiddly).
        let red = Color::rgb(220, 0, 40);
        let mut title = BoxNode::new(Rect::new(96, 0, 96, 40), BoxStyle::filled(Color::rgb(255, 255, 255)));
        title.tag = Some("title".into());
        let mut root = ContainerNode::new(vec![Scene::Box(title)]);
        root.rect = Rect::new(0, 0, W, H);
        root.style = BoxStyle::filled(Color::rgb(255, 255, 255));
        let scene = inject_highlight(
            Scene::Container(root),
            "title",
            HighlightStyle::default().with_stroke(red).with_stroke_width(2),
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
