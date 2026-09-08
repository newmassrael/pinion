//! R1537 §5.16 — the swapchain surface and the intermediate target a
//! compute rasterizer draws into.

use crate::context::GpuError;
use crate::health::{DeviceLiveness, Missed, Rung, SurfaceHealth};

/// How to make this window's surface **again**.
///
/// R1709 — a boxed closure rather than the window handle itself, because
/// what a surface can be made from is the embedder's business (an
/// `Arc<Window>` here; a raw handle or an offscreen target elsewhere) and
/// naming that type in this crate would drag winit into a crate whose whole
/// point is that it depends on nothing but `wgpu`. Captured once at
/// construction, which is what lets the recovery ladder have a heavy rung
/// at all: without it, [`Rung::Rebuilt`] would be unreachable and a surface
/// that a reconfigure cannot restore would be dead for the window's life.
type SurfaceSource =
    Box<dyn Fn(&wgpu::Instance) -> Result<wgpu::Surface<'static>, wgpu::CreateSurfaceError>>;

/// What a swapchain is being asked to be: its size in physical pixels and how
/// it should hand frames to the compositor.
///
/// R2088.1 — the three travel together (a size without a present mode
/// configures nothing) and they are the only part of [`GpuSurface::new`]'s
/// arguments that describes the *request* rather than the *device*. Grouping
/// them is what pays the argument-count lint with structure instead of an
/// allow, and it makes the call site read as one thing being asked for.
///
/// `Copy` because it is three plain numbers and a mode: taking it by
/// reference would make the caller name a lifetime for a value smaller than
/// the pointer to it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceRequest {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) present_mode: wgpu::PresentMode,
}

/// The presentable surface for one window, its configuration, the
/// intermediate storage texture the rasterizer writes, and the blitter
/// that copies that texture onto the acquired swapchain image.
///
/// # Why an intermediate texture at all
///
/// vello rasterizes with a **compute** shader, which needs
/// `STORAGE_BINDING` on its output. Swapchain textures generally cannot be
/// storage-bound (and on the GPUs where they can, drivers optimise on the
/// assumption that nobody will), so the canonical arrangement — vello's
/// own `util` module, Xilem's reference app — is render-to-texture then
/// blit. This type is that arrangement, with the device owned one level up
/// by [`GpuContext`](crate::GpuContext) instead of inside it.
///
/// # Relationship to `vello::util::RenderSurface`
///
/// Field-for-field the same, minus `dev_id`. That field was an index into
/// vello's device pool; with a single owned device there is nothing to
/// index, and carrying it would be carrying a number that can only ever be
/// `0` — the kind of vestigial state a later reader has to prove is
/// meaningless (R1513's "dead half").
pub struct GpuSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    format: wgpu::TextureFormat,
    target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,
    blitter: wgpu::util::TextureBlitter,
    /// R1709 — what [`Self::rebuild`] makes the replacement from.
    source: SurfaceSource,
    /// R1709 — the recovery ladder's memory. See [`SurfaceHealth`].
    health: SurfaceHealth,
    /// ★ R2088 — whether the most recent [`Self::configure`] **succeeded**,
    /// i.e. whether `wgpu` will answer `get_current_texture()` at all.
    ///
    /// Owned here because `wgpu` will not answer it: `Surface::configure`
    /// returns `()`, reports its failure only through the device's error
    /// sink, and exposes no "is this configured" accessor. A surface whose
    /// configure was refused is left *not configured for presentation*, and
    /// the next acquisition on it is reported through `handle_error_fatal`,
    /// which consults no uncaptured-error handler and **panics the
    /// process**. So this is not bookkeeping — it is the only place the
    /// invariant `get_current_texture()` requires can be held.
    ///
    /// ⚠ R2088.1 — necessary but **not sufficient**, and that is measured:
    /// the error scope this is derived from cannot see a device-lost
    /// refusal, so [`Self::liveness`] is the other half. See
    /// [`DeviceLiveness`].
    presentable: bool,
    /// R2088.1 — the device this surface presents on, and whether it is
    /// still there. Read at the acquire rather than cached at configure
    /// time, because the callback that writes it fires from `wgpu`'s
    /// maintain and not from the configure that failed.
    liveness: DeviceLiveness,
    /// ★ R2088.1 — why the most recent configure was refused, held until
    /// somebody reads it ([`Self::take_refusal`]).
    ///
    /// It exists because R2088 took an observability channel away without
    /// replacing it. Before that round a refused configure at least reached
    /// the embedder's uncaptured-error handler, which printed it; an error
    /// scope **captures** the error instead, and
    /// [`GpuContext::recover`](crate::GpuContext::recover) discards the
    /// `Result` it gets back. So the sentence `wgpu` wrote about why a window
    /// went dark had nowhere left to go — a quieter tree than the one the
    /// round set out to fix.
    last_refusal: Option<String>,
}

impl core::fmt::Debug for GpuSurface {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GpuSurface")
            .field("width", &self.config.width)
            .field("height", &self.config.height)
            .field("format", &self.format)
            .field("present_mode", &self.config.present_mode)
            .field("usage", &self.config.usage)
            .field("health", &self.health)
            .field("presentable", &self.presentable)
            // Textures, views and the blitter are opaque handles; the
            // surface's geometry and negotiated format are the state.
            .finish_non_exhaustive()
    }
}

impl GpuSurface {
    /// Configure a freshly created `wgpu::Surface` and build its
    /// intermediate target. Called by
    /// [`GpuContext::new`](crate::GpuContext::new); the surface must have
    /// come from the same instance the adapter did.
    ///
    /// # Errors
    ///
    /// [`GpuError::UnsupportedSurfaceFormat`] when the surface advertises
    /// no format the blit can present, and — R2088 —
    /// [`GpuError::SurfaceConfigure`] when the **first** configure is
    /// refused. That second one is fatal on purpose: a surface whose
    /// configure never succeeded cannot be acquired without killing the
    /// process, so handing one back as if it were a window's renderer is
    /// handing back a process-killer that has not gone off yet.
    pub(crate) fn new(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        surface: wgpu::Surface<'static>,
        source: SurfaceSource,
        liveness: DeviceLiveness,
        request: SurfaceRequest,
    ) -> Result<Self, GpuError> {
        let SurfaceRequest {
            width,
            height,
            present_mode,
        } = request;
        let capabilities = surface.get_capabilities(adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|it| {
                matches!(
                    it,
                    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
                )
            })
            .ok_or(GpuError::UnsupportedSurfaceFormat)?;

        // R1060 §5.16 — ask for COPY_SRC so the live-surface capture
        // (`scene/screenshot`) can read back the exact texture the window
        // presents. Guarded by the advertised usages: a surface that
        // cannot be a copy source keeps the default and capture fails with
        // a typed error, rather than a `configure` validation error taking
        // all rendering down with it.
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }

        let config = wgpu::SurfaceConfiguration {
            usage,
            format,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        let (target_texture, target_view) = create_target(device, width, height);
        let mut out = Self {
            surface,
            config,
            format,
            target_texture,
            target_view,
            blitter: wgpu::util::TextureBlitter::new(device, format),
            source,
            health: SurfaceHealth::default(),
            // Not yet: the configure below is what makes it true, and
            // starting from `true` would mean an unmeasured claim is the
            // default — which is the defect this field exists for.
            presentable: false,
            liveness,
            last_refusal: None,
        };
        out.configure(device)?;
        Ok(out)
    }

    /// The view the rasterizer draws into. Handed to
    /// `vello::Renderer::render_to_texture` as its target.
    #[must_use]
    pub fn target_view(&self) -> &wgpu::TextureView {
        &self.target_view
    }

    /// Acquire the next presentable image, or say why there is none.
    ///
    /// The `Err` side is [`Missed`], not one flattened error, because every
    /// non-success outcome wants a *different* response — reconfigure,
    /// retry, or skip — and that distinction is what the recovery ladder
    /// acts on (R1049).
    ///
    /// ★ R2088 — **an unconfigured surface is refused here rather than
    /// asked.** `wgpu` answers `get_current_texture()` on a surface whose
    /// `configure` never succeeded through `handle_error_fatal`, which
    /// consults no uncaptured-error handler and panics the process; there
    /// is no status to classify and no error to absorb, so the only place
    /// this can be survived is *before the call*. The refusal is a
    /// [`Missed::Unconfigured`], which is an invalidation, so the caller's
    /// ladder is what puts the surface back — the frame is skipped, not the
    /// window abandoned.
    ///
    /// # Errors
    ///
    /// The [`Missed`] this frame missed by: one of the statuses `wgpu`
    /// answered with, [`Missed::DeviceLost`] when the device is gone, or
    /// [`Missed::Unconfigured`] when the surface was not asked at all.
    pub fn acquire(&self) -> Result<wgpu::SurfaceTexture, Missed> {
        // ★ R2088.1 — the device first, and READ HERE rather than cached at
        // configure time. `wgpu` reports a lost device only through a
        // callback (see [`DeviceLiveness`]), and `configure_surface` does not
        // fire its user callbacks on the path where it fails — so the moment
        // the answer is trustworthy is the moment before asking for an image,
        // not the moment the configure returned. A `get_current_texture()` on
        // a lost device's never-configured surface is the process-fatal
        // report that no handler can absorb.
        if self.liveness.is_lost() {
            return Err(Missed::DeviceLost);
        }
        if !self.presentable {
            return Err(Missed::Unconfigured);
        }
        Missed::split(self.surface.get_current_texture())
    }

    /// R2088 — whether this surface is configured for presentation, i.e.
    /// whether the most recent configure succeeded.
    #[must_use]
    pub fn is_presentable(&self) -> bool {
        self.presentable
    }

    /// Record the copy from the intermediate target onto `destination`.
    pub fn blit(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::TextureView,
    ) {
        self.blitter
            .copy(device, encoder, &self.target_view, destination);
    }

    /// Current swapchain width in physical pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Current swapchain height in physical pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// The negotiated swapchain format.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// R1709 — what this window can say about putting frames on the screen.
    #[must_use]
    pub fn health(&self) -> SurfaceHealth {
        self.health
    }

    /// R1709 — record that a frame reached the screen. Call it after
    /// `present()`, on every path that presents.
    ///
    /// Without this the ladder never resets, so the first outage of a
    /// window's life would leave every later resize rebuilding the surface.
    pub fn note_presented(&mut self) {
        self.health.presented();
    }

    /// R1709 — record that a frame did NOT reach the screen, and answer the
    /// rung of the recovery ladder it earned.
    ///
    /// The rung is not performed here — [`crate::GpuContext::recover`] does
    /// that, because two of the three rungs need the device (and one needs
    /// the instance). Splitting the *decision* from the *action* is what
    /// keeps the decision unit-testable without a GPU.
    pub fn note_missed(&mut self, missed: Missed) -> Option<Rung> {
        self.health.missed(missed)
    }

    /// ★ R2088 — configure the swapchain **and read back whether it
    /// worked**, recording that in [`Self::is_presentable`].
    ///
    /// `wgpu::Surface::configure` returns `()`. Its only failure channel is
    /// the device's error sink, so "did that work?" is a question only an
    /// error scope can answer — and a caller that does not ask reads
    /// silence as success. Until this round nothing asked, on any of the
    /// four paths that configure (construction, resize, and both rungs of
    /// the recovery ladder).
    ///
    /// Measured on this host, deterministically, at R2088: a reconfigure
    /// refused with `SurfaceOutput must be dropped before a new Surface is
    /// made` left the surface *not configured for presentation*, and the
    /// very next acquisition raised exactly the error the intermittent
    /// sweep failures carried.
    ///
    /// # Errors
    ///
    /// [`GpuError::SurfaceConfigure`] carrying `wgpu`'s own rendering of
    /// the refusal. The surface is left marked not-presentable either way,
    /// so a caller that discards this `Result` still cannot present through
    /// it — the ladder answers the next frame instead.
    /// ⚠ R2088.1 — `Ok` here means *no scope-visible refusal*, which is
    /// weaker than *it worked*: a device-lost refusal reaches no scope at
    /// all. [`Self::acquire`] asks [`DeviceLiveness`] as well, and that
    /// second question is not optional.
    pub(crate) fn configure(&mut self, device: &wgpu::Device) -> Result<(), GpuError> {
        let refused = caught(device, || self.surface.configure(device, &self.config));
        self.presentable = refused.is_none() && !self.liveness.is_lost();
        match refused {
            None => Ok(()),
            Some(e) => {
                let why = format!("{e}");
                // R2088.1 — kept for a reader even when the caller discards
                // the `Result`, which both rungs of the recovery ladder do.
                self.last_refusal = Some(why.clone());
                Err(GpuError::SurfaceConfigure(why))
            }
        }
    }

    /// ★ R2088.1 — take the reason the most recent configure was refused, if
    /// one has not been reported yet.
    ///
    /// Taken rather than borrowed so each refusal is reported **once**: this
    /// is called every frame that misses, and a borrow would print the same
    /// sentence for as long as the window stayed dark.
    ///
    /// This exists to undo an observability regression R2088 introduced. An
    /// error scope captures the refusal, so the uncaptured-error handler that
    /// used to print it never sees it, and the recovery ladder throws the
    /// `Result` away — leaving no channel at all for the one sentence that
    /// says why a window is dark.
    pub fn take_refusal(&mut self) -> Option<String> {
        self.last_refusal.take()
    }

    /// R1709 — the heavy rung: throw this window's surface away and make
    /// another one.
    ///
    /// # Why the order of these three lines is the whole method
    ///
    /// The replacement is created while the old surface is still alive, the
    /// **assignment** drops the old one, and only then is the new one
    /// configured. Measured: configuring a second surface for the same
    /// window while the first is still alive fails outright — `wgpu`
    /// answers `Validation Error: In Surface::configure / Invalid surface`,
    /// which its default handler turns into a process panic. The first
    /// draft of this did exactly that.
    ///
    /// Returns whether a **presentable** replacement was obtained. `false`
    /// leaves the caller to fall back to the cheap rung — a surface that
    /// cannot be remade is still better than none, and the next frame will
    /// simply try again.
    ///
    /// ★ R2088 — `false` now also covers *the replacement was made and its
    /// configure was refused*, which used to answer `true`. That state is
    /// the worst one this type can be in and it was the one the old code
    /// reported as success: the old surface has been dropped, the new one
    /// has **never** been configured, and a never-configured surface is the
    /// only shape whose acquisition `wgpu` reports through
    /// `handle_error_fatal` — the one report an uncaptured-error handler
    /// cannot absorb. This is the heavy rung of a ladder a freshly created
    /// window climbs, which is why the failure was second-window-shaped.
    pub(crate) fn rebuild(&mut self, instance: &wgpu::Instance, device: &wgpu::Device) -> bool {
        let Ok(fresh) = (self.source)(instance) else {
            return false;
        };
        self.surface = fresh;
        self.configure(device).is_ok()
    }

    /// Resize the swapchain and its intermediate target.
    ///
    /// Infallible on purpose: a resize whose configure is refused is not
    /// fatal to the window, it just leaves the surface not presentable —
    /// and [`Self::acquire`] then answers [`Missed::Unconfigured`], which
    /// is what puts the ladder to work on the next frame.
    ///
    /// # Panics
    ///
    /// If `width` or `height` is zero — a zero-sized swapchain is a `wgpu`
    /// validation error, and the caller (which knows about minimised
    /// windows) is the layer that can decide to skip instead.
    pub(crate) fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        assert!(
            width > 0 && height > 0,
            "GpuSurface::resize needs a non-zero size; got {width}x{height}"
        );
        let (texture, view) = create_target(device, width, height);
        self.target_texture = texture;
        self.target_view = view;
        self.config.width = width;
        self.config.height = height;
        drop(self.configure(device));
    }
}

/// Run `f` and answer the `wgpu` error it raised, if any.
///
/// R2088 — the only way to learn whether a `()`-returning `wgpu` call
/// succeeded. Errors that reach no scope go to the device's
/// uncaptured-error handler, which prints and drops them; a caller reading
/// that silence as success is exactly how a surface came to be presented
/// before it was configured.
///
/// **All three filters are pushed**, because a scope catches its own filter
/// and nothing else: one filter would pass the other two kinds straight to
/// the uncaptured handler and this function would answer `None` for a call
/// that failed. They nest, so they are popped in the reverse order they
/// were pushed, and at most one can hold an error — an error is delivered
/// to the innermost scope whose filter matches it.
fn caught(device: &wgpu::Device, f: impl FnOnce()) -> Option<wgpu::Error> {
    let out_of_memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    f();
    // Popped eagerly rather than through `or_else`, so the three pops
    // happen in reverse push order whatever the outcome is: `wgpu` panics
    // on a scope popped out of order, and a lazily-skipped pop would leave
    // that ordering to drop-glue.
    let validation = pollster::block_on(validation.pop());
    let internal = pollster::block_on(internal.pop());
    let out_of_memory = pollster::block_on(out_of_memory.pop());
    validation.or(internal).or(out_of_memory)
}

/// The `Rgba8Unorm` storage texture the compute rasterizer writes.
///
/// `Rgba8Unorm` regardless of the swapchain's own format: the blit
/// converts, and pinning the intermediate means the rasterizer sees one
/// format everywhere instead of a per-host one.
fn create_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pinion-gpu intermediate target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        format: wgpu::TextureFormat::Rgba8Unorm,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
