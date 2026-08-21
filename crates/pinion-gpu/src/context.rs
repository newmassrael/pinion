//! R1537 §5.16 — the wgpu instance / adapter / device / queue pinion owns.

use crate::health::{Missed, Rung};
use crate::surface::GpuSurface;

/// Why a [`GpuContext`] could not be built.
///
/// Deliberately a small closed enum rather than a boxed error: every arm
/// is a distinct thing the host can be missing, and a caller that logs the
/// variant has said something actionable. `vello::Error` collapses the
/// first two into `NoCompatibleDevice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuError {
    /// No adapter matched the instance's backends and the surface (if one
    /// was supplied). On Linux this is the shape a missing Vulkan ICD
    /// takes; see VELLO-002 for the GL-plus-X11 case.
    NoAdapter,
    /// An adapter was found but refused a device with the requested
    /// features or limits.
    NoDevice(String),
    /// The window handle could not be turned into a `wgpu::Surface`.
    SurfaceCreation(String),
    /// The surface advertised no format this renderer can present. The
    /// intermediate target is `Rgba8Unorm` and the blit needs a
    /// byte-compatible swapchain, so an exotic-only surface is fatal.
    UnsupportedSurfaceFormat,
}

impl core::fmt::Display for GpuError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoAdapter => write!(f, "no compatible wgpu adapter"),
            Self::NoDevice(e) => write!(f, "wgpu device request failed: {e}"),
            Self::SurfaceCreation(e) => write!(f, "wgpu surface creation failed: {e}"),
            Self::UnsupportedSurfaceFormat => {
                write!(f, "surface offers no Rgba8Unorm/Bgra8Unorm format")
            }
        }
    }
}

impl core::error::Error for GpuError {}

/// The GPU pinion is rendering on: one instance, one adapter, one device,
/// one queue.
///
/// # Why one device and not a pool
///
/// `vello::util::RenderContext` keeps a `Vec<DeviceHandle>` and picks a
/// compatible entry per surface, which is the right shape for a library
/// that does not know how many windows or displays its host has. pinion
/// does know: [`crate::GpuSurface`] is created *with* the context, from the
/// same adapter, so the "which device is this surface on?" indirection
/// (vello's `dev_id`) has exactly one answer and is not stored. A second
/// adapter — a discrete/integrated split, a second X screen — is a second
/// `GpuContext`, which is both simpler and the thing multi-window would
/// actually need.
///
/// # Features
///
/// The device is asked for the intersection of what the adapter offers
/// with what pinion can use:
///
/// - `CLEAR_TEXTURE` and `PIPELINE_CACHE` — what vello's own device
///   request asks for; kept identical so swapping the owner did not
///   silently change the rasterizer's environment.
/// - `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS` — R1537, what
///   [`crate::FrameTimer`] needs. The second is the one that matters:
///   plain `TIMESTAMP_QUERY` only permits timestamps at render/compute
///   *pass* boundaries, and the work being timed here spans three
///   submissions (a compute rasterizer's passes, then a blit), so the
///   writes have to sit on the encoder itself.
///
/// Intersecting rather than requiring means an adapter without timestamps
/// still boots — it just reports no GPU time, which
/// [`crate::FrameTimer::new`] discovers by reading the device back.
pub struct GpuContext {
    /// Held because dropping the instance invalidates every surface
    /// created from it, and because a future multi-window path creates
    /// further surfaces from this same instance.
    ///
    /// R1709 — it is also what [`GpuContext::recover`]'s heavy rung asks
    /// for a replacement surface, so this field stopped being inert.
    instance: wgpu::Instance,
    /// Kept for [`GpuContext`]'s `Debug`, which answers the one question a
    /// log line about a GPU wants: WHICH one. `wgpu` keeps the adapter
    /// alive behind the device regardless, so this is not a lifetime prop.
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl core::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // R1754 — the same accessor a consumer reads, so a log line and the
        // published fact cannot disagree about which GPU this is.
        let info = self.adapter_info();
        f.debug_struct("GpuContext")
            .field("adapter", &info.name)
            .field("backend", &info.backend)
            .field("device_type", &info.device_type)
            .field(
                "timestamps",
                &self
                    .device
                    .features()
                    .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            )
            // The wgpu handles themselves have no readable Debug and no
            // identity a log line could act on; what a reader wants from a
            // context is which GPU it is.
            .finish_non_exhaustive()
    }
}

impl GpuContext {
    /// Create the context together with the surface it renders to.
    ///
    /// The two are built in one call because adapter selection *depends on
    /// the surface*: `wgpu` picks an adapter that can present to this
    /// window, and asking for a device first would be asking before the
    /// constraint is known. This is why vello's `create_surface` is also
    /// the thing that lazily creates the device.
    ///
    /// Async because adapter and device acquisition are; call it from the
    /// §6.3 boundary at app boot, never inside a view fn.
    ///
    /// # Errors
    ///
    /// See [`GpuError`]. A failure here is fatal to the window — there is
    /// no rendering without a device.
    /// `W: Clone` (R1709) because the surface has to be **re-creatable**:
    /// the recovery ladder's heavy rung remakes it from the same target,
    /// and a target that could only be consumed once would make that rung
    /// unreachable. `Arc<Window>` — the canonical input — already is.
    pub async fn new<W>(
        target: W,
        width: u32,
        height: u32,
        present_mode: wgpu::PresentMode,
    ) -> Result<(Self, GpuSurface), GpuError>
    where
        W: Into<wgpu::SurfaceTarget<'static>> + Clone + 'static,
    {
        // Mirrors `vello::util::RenderContext::new` exactly — the env-var
        // backend/flag overrides are how `WGPU_BACKEND=gl` and friends are
        // driven in CI and in the VELLO-002 investigation, and changing
        // device selection was not this round's business.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            display: None,
            backends: wgpu::Backends::from_env().unwrap_or_default(),
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::from_env_or_default(),
        });
        // R1709 — one closure makes the surface now AND makes it again
        // later, so the two can never drift into asking for different
        // things. It is the only place the target type is named.
        let source = {
            let target = target.clone();
            move |instance: &wgpu::Instance| instance.create_surface(target.clone().into())
        };
        let surface = source(&instance).map_err(|e| GpuError::SurfaceCreation(format!("{e}")))?;
        let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&surface))
            .await
            .map_err(|_| GpuError::NoAdapter)?;

        // Both timestamp features or neither: `TIMESTAMP_QUERY` alone cannot
        // express what is being measured here (see the type doc), so asking
        // for it on its own would yield a device that can create a query set
        // no write can target. Whether the request succeeded is not stored —
        // `FrameTimer::new` reads it back off the device, so there is one
        // source for "can this host time the GPU" rather than two that can
        // disagree.
        let required_features = adapter.features()
            & (wgpu::Features::CLEAR_TEXTURE
                | wgpu::Features::PIPELINE_CACHE
                | wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("pinion-gpu device"),
                required_features,
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| GpuError::NoDevice(format!("{e}")))?;

        let surface = GpuSurface::new(
            &adapter,
            &device,
            surface,
            Box::new(source),
            width,
            height,
            present_mode,
        )?;
        Ok((
            Self {
                instance,
                adapter,
                device,
                queue,
            },
            surface,
        ))
    }

    /// ★ R1754 — **which adapter this window is actually rendering on.**
    ///
    /// Adapter selection is constrained by the *surface* (see [`Self::new`]),
    /// so which one arrives is a property of the window rather than of the
    /// host, and no caller can read it off its own environment. Before this it
    /// lived only inside this type's `Debug` — reachable by a log line and by
    /// nothing else — while every duration pinion publishes is a measurement
    /// of it.
    ///
    /// ⚠ Measured R1754, and worth stating because the opposite is the
    /// intuitive guess: a virtual framebuffer here does **not** force a
    /// software adapter. Both an `Xvfb` display and the real one selected the
    /// same discrete GPU over Vulkan, while the frame times differed
    /// ninety-six-fold. So this answers *what rendered*, and by itself will
    /// not explain why two windows on one host disagree.
    ///
    /// Returns `wgpu`'s own struct rather than a pinion vocabulary because
    /// this crate deliberately depends on nothing but `wgpu`; the mapping onto
    /// a backend-agnostic vocabulary belongs to the shell, beside
    /// `present_health_of`.
    #[must_use]
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }

    /// The device every GPU resource in this window is created from.
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue every command buffer for this window is submitted to.
    ///
    /// One queue is what makes [`crate::FrameTimer`]'s two timestamps
    /// comparable at all: `wgpu` orders execution within a queue, so a
    /// timestamp written in an earlier submission is guaranteed to be
    /// taken before one written in a later submission. Across queues it
    /// would be neither ordered nor on a shared clock.
    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Install a handler for `wgpu` errors that reach no `Result`.
    ///
    /// R1049 §5.16 — a transient uncaptured error (a `create_view` on a
    /// momentarily-invalid swapchain texture just after a surface
    /// invalidation) otherwise hard-panics the process through wgpu's
    /// default handler, even though the renderer reconfigures and
    /// self-recovers on the next frame. Kept as an explicit call rather
    /// than done inside [`Self::new`]: what to do with an unattributable
    /// GPU error is the embedder's policy, not this crate's.
    pub fn on_uncaptured_error(&self, handler: impl Fn(wgpu::Error) + Send + Sync + 'static) {
        self.device
            .on_uncaptured_error(std::sync::Arc::new(handler));
    }

    /// R1709 §5.16 — answer a frame that did not reach the screen: record
    /// it, and take the rung of the recovery ladder it earns.
    ///
    /// Returns the rung taken, or `None` when the miss was a *wait*
    /// (occluded, timed out) rather than the surface breaking — nothing is
    /// rebuilt for a window that is merely not being looked at.
    ///
    /// # What this replaced, and why it had to
    ///
    /// Until R1709 this was `configure_surface`: one line, called on every
    /// non-presentable acquisition, whose doc asserted that it
    /// "re-establishes the swapchain so the NEXT frame acquires a fresh
    /// texture". Nothing verified that claim, and it is measurably false
    /// on at least one real window — a never-mapped X11 window on this
    /// host's driver, where after a resize *every* acquire comes back
    /// outdated, the reconfigure runs again, and the window never presents
    /// again. Isolated in a standalone reproducer: it is `configure`
    /// itself that poisons that swapchain, and making a **new surface** is
    /// what restores it.
    ///
    /// The ladder makes "did the previous response work?" structural. A
    /// rung is only reached when the cheaper one was already taken and the
    /// very next frame failed anyway; a frame that presents resets it. So
    /// the ordinary case (a resized, mapped window) still costs exactly one
    /// reconfigure, and the case a reconfigure cannot fix is no longer
    /// silent — [`GpuSurface::health`] says how long the window has been
    /// dark, why, and what has been tried.
    pub fn recover(&self, surface: &mut GpuSurface, missed: Missed) -> Option<Rung> {
        let rung = surface.note_missed(missed)?;
        match rung {
            Rung::Reconfigured | Rung::Repeated => surface.configure(&self.device),
            Rung::Rebuilt => {
                if !surface.rebuild(&self.instance, &self.device) {
                    // The replacement could not be made. Fall back to the
                    // cheap rung rather than leaving the frame unanswered:
                    // the ladder has already recorded that the heavy one
                    // was owed, so this is visible rather than silent.
                    surface.configure(&self.device);
                }
            }
        }
        Some(rung)
    }

    /// Resize the swapchain and the intermediate target to a new window
    /// size.
    ///
    /// # Panics
    ///
    /// If `width` or `height` is zero — a zero-sized swapchain is a wgpu
    /// validation error, and the caller (which knows about minimised
    /// windows) is the layer that can decide to skip instead.
    pub fn resize_surface(&self, surface: &mut GpuSurface, width: u32, height: u32) {
        surface.resize(&self.device, width, height);
    }
}
