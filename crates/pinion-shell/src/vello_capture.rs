//! R1060 §5.16 §5.12 — GPU pixel readback SSOT for the Vello renderer
//! kind, plus the live-surface capture that backs the `scene/screenshot`
//! RPC.
//!
//! ## Why this module exists
//!
//! Two code paths need to copy a wgpu texture back into a CPU-side
//! RGBA8 buffer:
//!
//! - the headless offscreen rasterizer ([`crate::headless_screenshot`])
//!   — renders the paint scene into an offscreen `Rgba8Unorm` texture
//!   (the §5.16 dev / CI fallback, R637); and
//! - the live-surface capture ([`capture_surface_rgba8`]) — reads the
//!   **swapchain surface texture the window actually presented** so the
//!   AI can introspect present-stage render fidelity (a white / stale
//!   surface the encoded scene cannot reveal — see
//!   [`crate::headless_screenshot`] vs `scene/render_fidelity`).
//!
//! The texture → RGBA8 copy (staging buffer + 256-byte row-alignment
//! strip + BGRA↔RGBA swizzle) is byte-for-byte the same operation in
//! both, so it lives here once as `texture_to_rgba8` — the single
//! source of truth both call.
//!
//! The capture additionally re-runs the Vello present cycle
//! (`render_to_texture` → `get_current_texture` + R1049 recovery → blit,
//! with a `copy_texture_to_buffer` spliced in before `present`). That
//! parallels the pinion-forge template `render()` present pattern, and
//! the ~40 parallel lines (most acutely the surface-recovery match) are
//! a duplication this module DOES NOT lift — not by choice but by
//! constraint: the two live on opposite sides of the codegen boundary.
//! `pinion-forge` emits a vello/wgpu-only Rust string `include!`-ed into
//! ~130 example binaries that must NOT depend on `pinion-shell` (e.g.
//! `ai-introspect-demo` uses the emitted renderer with no shell dep), so
//! the template `render()` cannot call into this module, and there is no
//! shared-symbol lift point across that boundary (a vello-only leaf crate
//! both call would force the dep onto every example's Cargo.toml). The
//! standing obligation is therefore mirror-not-lift: any present-pattern
//! change (e.g. a future R1049-class surface-recovery fix) MUST land in
//! BOTH `codegen.rs::render` and `capture_surface_rgba8` or they drift.

use pinion_gpu::{FrameTimer, GpuContext, GpuSurface};
use vello::peniko::Color as PenikoColor;
use vello::wgpu::{
    self, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Device, Extent3d, MapMode,
    Origin3d, PollType, Queue, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo,
    Texture, TextureAspect, TextureFormat, TextureViewDescriptor,
};
use vello::{AaConfig, RenderParams, Renderer};

/// wgpu `bytes_per_row` alignment for `copy_texture_to_buffer`. WebGPU
/// mandates 256-byte row alignment for buffer copies; wgpu re-exports
/// the constant. Padding added for the copy is stripped back out so
/// callers see the unpadded `width * height * 4` byte buffer.
const ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// Reason a GPU texture readback or live-surface capture can fail.
#[derive(Debug)]
#[non_exhaustive]
pub enum SurfaceCaptureError {
    /// Requested viewport had `width == 0` or `height == 0`.
    ZeroDimension,
    /// `surface.get_current_texture()` returned a non-presentable
    /// status (`timeout` / `occluded` / `outdated` / `lost` /
    /// `validation`). Carries the status label. `outdated` / `lost` /
    /// `validation` trigger a surface reconfigure exactly like the
    /// forge template `render()` recovery (R1049); the capture itself
    /// still fails so the caller does not return a stale frame.
    SurfaceUnavailable(&'static str),
    /// `vello::Renderer::render_to_texture` failed.
    VelloRender(String),
    /// `buffer.slice(..).map_async(MapMode::Read, ...)` reported an
    /// error (typically a lost device or driver fault).
    BufferMap(String),
}

impl core::fmt::Display for SurfaceCaptureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroDimension => f.write_str("surface width / height must be > 0"),
            Self::SurfaceUnavailable(s) => write!(f, "surface texture unavailable: {s}"),
            Self::VelloRender(e) => write!(f, "vello render_to_texture failed: {e}"),
            Self::BufferMap(e) => write!(f, "wgpu buffer map_async failed: {e}"),
        }
    }
}

impl std::error::Error for SurfaceCaptureError {}

/// Copy an `Rgba8Unorm` / `Bgra8Unorm`, `COPY_SRC` texture of
/// `width x height` into a contiguous `width * height * 4`
/// premultiplied-RGBA8 buffer (row-major, top-left origin).
///
/// Strips the wgpu `bytes_per_row` 256-alignment padding and, when
/// `format` is BGRA, swizzles B↔R per pixel so the returned buffer is
/// always RGBA channel order regardless of the source texture's native
/// order (offscreen targets are `Rgba8Unorm`; swapchain surfaces are
/// frequently `Bgra8Unorm`).
///
/// The single source of truth for texture → RGBA8 readback, shared by
/// [`crate::headless_screenshot`] (offscreen, RGBA, no swizzle) and
/// [`capture_surface_rgba8`] (live surface, format-dependent swizzle).
///
/// # Errors
///
/// [`SurfaceCaptureError::BufferMap`] when the staging-buffer map fails.
pub(crate) fn texture_to_rgba8(
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    width: u32,
    height: u32,
    format: TextureFormat,
) -> Result<Vec<u8>, SurfaceCaptureError> {
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let staging_size = u64::from(padded_bytes_per_row) * u64::from(height);
    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("pinion-shell::vello_capture readback staging"),
        size: staging_size,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("pinion-shell::vello_capture readback copy"),
    });
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture,
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
    queue.submit(std::iter::once(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<(), wgpu::BufferAsyncError>>(1);
    slice.map_async(MapMode::Read, move |result| {
        // Receiver may have been dropped if the caller gave up early;
        // tolerating the send error keeps wgpu free of a dangling
        // callback panic.
        let _ = tx.send(result);
    });
    // wgpu requires an explicit poll to drive `map_async` on native
    // backends; `wait_indefinitely` blocks until the queued work + the
    // map callback both complete.
    let _ = device.poll(PollType::wait_indefinitely());
    rx.recv()
        .map_err(|e| SurfaceCaptureError::BufferMap(format!("{e}")))?
        .map_err(|e| SurfaceCaptureError::BufferMap(format!("{e}")))?;

    let swizzle_bgra = matches!(
        format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    );
    let mut out = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    {
        let mapped = slice.get_mapped_range();
        for row in 0..height as usize {
            let row_start = row * padded_bytes_per_row as usize;
            let row_end = row_start + unpadded_bytes_per_row as usize;
            if swizzle_bgra {
                for px in mapped[row_start..row_end].chunks_exact(4) {
                    out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            } else {
                out.extend_from_slice(&mapped[row_start..row_end]);
            }
        }
    }
    staging.unmap();
    Ok(out)
}

/// Premultiplied-RGBA8 capture of a live window surface: the exact
/// pixels the window presented, plus the dimensions the wire needs.
///
/// `Debug`-only: production moves the frame end-to-end (`last_capture`
/// `take()` → wire conversion), never clones or compares the multi-MB
/// buffer, so the speculative value-type derives are intentionally
/// omitted (R1062).
#[derive(Debug)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` premultiplied RGBA8, row-major, top-left.
    pub rgba8: Vec<u8>,
    /// R1361.5 §5.16 — µs this capture spent BLOCKED in its own
    /// `get_current_texture()`.
    ///
    /// The capture path does not go through the forge template's `render`,
    /// so it neither resets nor records that template's
    /// `last_acquire_us`. Before R1361.5 a screenshot frame therefore
    /// inherited the *previous* render's block, and the shell — which
    /// subtracts the block from every recorded frame, capture included —
    /// clamped it to the capture span and recorded the frame as ~100%
    /// acquire, 0 render. Every demo script in this repo drives
    /// screenshots, so that poisoned the very window it measured.
    ///
    /// Reported here rather than through the renderer's field because this
    /// is a free function over borrowed wgpu handles: the value rides out
    /// with the frame it belongs to.
    pub acquire_us: u64,
}

/// Render `scene` to the live Vello surface and read back the **exact
/// swapchain texture that is presented** as premultiplied RGBA8.
///
/// This is the true-fidelity (b) path: unlike re-rasterizing the scene
/// offscreen, it observes blit / surface-config / swapchain-staleness
/// defects (a white or stale presented surface) that the encoded scene
/// is correct about — the residue `scene/render_fidelity` documents it
/// cannot see. Requires the surface to have been configured with
/// `TextureUsages::COPY_SRC` (the pinion-forge Vello template adds it).
///
/// The render → acquire → blit sequence mirrors the forge template
/// `render()` by necessity (the swapchain texture is only readable
/// between blit and present); see the module docs for why it is not
/// lifted. Renders at the surface's current configured size.
///
/// # Errors
///
/// See [`SurfaceCaptureError`]. `outdated` / `lost` / `validation`
/// reconfigure the surface (R1049) and fail this capture; the next
/// frame acquires a fresh texture.
pub fn capture_surface_rgba8(
    context: &GpuContext,
    surface: &mut GpuSurface,
    renderer: &mut Renderer,
    mut timer: Option<&mut FrameTimer>,
    scene: &vello::Scene,
    base_color: PenikoColor,
) -> Result<CapturedFrame, SurfaceCaptureError> {
    let width = surface.width();
    let height = surface.height();
    if width == 0 || height == 0 {
        return Err(SurfaceCaptureError::ZeroDimension);
    }

    let device = context.device();
    let queue = context.queue();

    // R1537 §5.16 — a captured frame is a real GPU frame: it rasterizes
    // and blits exactly as `render()` does, and an agent driving the window
    // over `scene/screenshot` gets no other paints. So it is timed here
    // too, in the same order and for the same reason — the module doc's
    // mirror-not-lift obligation, which this is the first change to
    // exercise since it was written.
    if let Some(t) = timer.as_mut() {
        t.collect(device);
        let mut open = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("pinion frame timer open (capture)"),
        });
        if t.begin(&mut open) {
            queue.submit([open.finish()]);
        }
    }

    // Rasterize into the surface's intermediate target (same as the
    // forge template `render()` first step).
    renderer
        .render_to_texture(
            device,
            queue,
            scene,
            surface.target_view(),
            &RenderParams {
                base_color,
                width,
                height,
                antialiasing_method: AaConfig::Area,
            },
        )
        .map_err(|e| SurfaceCaptureError::VelloRender(format!("{e}")))?;

    // Acquire the swapchain texture, mirroring the template `render()`
    // recovery (R1049): outdated / lost / validation reconfigure the
    // surface and fail this capture rather than reading a stale frame.
    // R1361.5 §5.16 — this path performs its own swapchain acquire; time it
    // so the shell can attribute the block instead of billing it to render
    // work (or, worse, inheriting a stale block from the last `render` call,
    // which this path never makes). Bound at the acquire itself, so every
    // path that reaches the `Ok` below carries THIS frame's block.
    let __acquire_start = std::time::Instant::now();
    let __acquired = surface.acquire();
    let acquire_us = u64::try_from(__acquire_start.elapsed().as_micros()).unwrap_or(u64::MAX);
    let surface_texture = match __acquired {
        wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
        wgpu::CurrentSurfaceTexture::Timeout => {
            return Err(SurfaceCaptureError::SurfaceUnavailable("timeout"));
        }
        wgpu::CurrentSurfaceTexture::Occluded => {
            return Err(SurfaceCaptureError::SurfaceUnavailable("occluded"));
        }
        wgpu::CurrentSurfaceTexture::Outdated => {
            context.configure_surface(surface);
            return Err(SurfaceCaptureError::SurfaceUnavailable("outdated"));
        }
        wgpu::CurrentSurfaceTexture::Lost => {
            context.configure_surface(surface);
            return Err(SurfaceCaptureError::SurfaceUnavailable("lost"));
        }
        wgpu::CurrentSurfaceTexture::Validation => {
            context.configure_surface(surface);
            return Err(SurfaceCaptureError::SurfaceUnavailable("validation"));
        }
    };

    // Blit the intermediate target into the swapchain texture (same as
    // `render()`), then copy that swapchain texture out before present
    // so the readback is the literal presented image.
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("pinion-shell::vello_capture present blit"),
    });
    surface.blit(
        device,
        &mut encoder,
        &surface_texture
            .texture
            .create_view(&TextureViewDescriptor::default()),
    );
    // R1537 — close the span on the blit encoder, before it is finished.
    // The readback below is deliberately OUTSIDE the span: it is the cost
    // of introspecting the frame, not the cost of drawing it, and billing
    // it to `gpu_us` would make every screenshot look like a slow frame.
    if let Some(t) = timer.as_mut() {
        t.end(&mut encoder);
    }
    queue.submit([encoder.finish()]);
    if let Some(t) = timer.as_mut() {
        t.after_submit();
    }

    let rgba8 = texture_to_rgba8(
        device,
        queue,
        &surface_texture.texture,
        width,
        height,
        surface.format(),
    )?;
    surface_texture.present();

    Ok(CapturedFrame {
        width,
        height,
        rgba8,
        acquire_us,
    })
}

/// R1061 §5.12 — encode a premultiplied-RGBA8 buffer (`width * height * 4`
/// bytes, row-major, top-left) as an 8-bit RGBA PNG to `writer`.
///
/// The single source of truth for RGBA8 → PNG, shared by the headless
/// screenshot path ([`crate::headless_screenshot::HeadlessScreenshot`])
/// and the live-capture `scene/screenshot {out_path}` wire (which writes
/// the captured frame to a file instead of returning a multi-MB
/// `pixels_rgba8` JSON array). The shape is RGBA 8-bit, no palette / no
/// 16-bit / no interlace — the simplest lossless round-trip every decoder
/// opens.
///
/// # Errors
///
/// Returns the `png` header / image-data write error rendered as a
/// string (the substrate surface stays free of a `png` re-export).
pub(crate) fn encode_rgba8_png<W: std::io::Write>(
    width: u32,
    height: u32,
    rgba8: &[u8],
    writer: W,
) -> Result<(), String> {
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder.write_header().map_err(|e| format!("{e}"))?;
    png_writer
        .write_image_data(rgba8)
        .map_err(|e| format!("{e}"))?;
    Ok(())
}
