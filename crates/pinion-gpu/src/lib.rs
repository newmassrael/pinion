//! pinion-gpu — the wgpu device, swapchain surface and GPU frame timer
//! pinion owns, per R1537 §5.16.
//!
//! ## Why this crate exists
//!
//! Until R1537 the emitted renderer (`pinion-forge`'s `kind="renderer"
//! backend="vello"` template) delegated device acquisition to
//! `vello::util::RenderContext`. That type creates its device inside a
//! *private* `new_device`, with a fixed feature request — vello's own
//! `CLEAR_TEXTURE | PIPELINE_CACHE` — and hands it back inside a
//! `DeviceHandle` whose `adapter` field is private, so a consumer can
//! neither influence the request nor construct a substitute.
//!
//! The consequence is not a missing convenience. A `wgpu` device that was
//! not created with `wgpu::Features::TIMESTAMP_QUERY` can never answer
//! how long the GPU took, because a query set cannot be created on it at
//! all. So for as long as the device came from `RenderContext`, pinion's
//! `render_us` could only ever be **CPU submit cost** — the time spent
//! *recording and handing work to the driver* — and the frame had no way
//! to state the thing a pro tool states first (the engine's `stat gpu`).
//!
//! That is what the pro-tool-performance axis recorded as its largest
//! remaining gap, and it was written down as an upstream blocker. It is
//! not one. `vello::Renderer::new` takes a `&Device` the caller owns,
//! and `vello::util::RenderSurface`'s fields are all public, so nothing
//! upstream requires `RenderContext` — only our template did. This crate
//! is the device that template now uses.
//!
//! ## What it is not
//!
//! It is not a rendering crate and does not depend on `vello`. It answers
//! three questions — *what device*, *what surface*, *how long did the GPU
//! take* — none of which is a 2D-rasterizer question. `vello::Renderer`
//! is constructed by the caller from [`GpuContext::device`] and draws into
//! [`GpuSurface::target_view`]; this crate never names it.
//!
//! ## Shape
//!
//! - [`GpuContext`] — instance + adapter + device + queue, created once at
//!   app boot (§6.3 async boundary). Requests the timestamp features when
//!   the adapter offers them, and records whether it got them.
//! - [`GpuSurface`] — the swapchain surface, its configuration, the
//!   intermediate `Rgba8Unorm` storage texture a compute rasterizer needs,
//!   and the blitter that copies it to the presented image. The same
//!   arrangement `RenderContext::create_surface` built, minus the device
//!   ownership.
//! - [`FrameTimer`] — two timestamps around one frame's GPU work, resolved
//!   and read back **without blocking the frame that wrote them**.
//!
//! ## Absence is a value here
//!
//! Every timestamp-facing API is `Option`-shaped, and the `None` is load
//! bearing: an adapter without `TIMESTAMP_QUERY` (WebGL, some mobile
//! tiles, a driver with the feature disabled) reports *no GPU time*, which
//! is a different claim from *zero GPU time*. A zero would be a lie that
//! reads as a fast frame. See [`GpuFrameClock`].

mod context;
mod frame_timer;
mod surface;

pub use context::{GpuContext, GpuError};
pub use frame_timer::{FrameTimer, GpuFrameClock};
pub use surface::GpuSurface;
