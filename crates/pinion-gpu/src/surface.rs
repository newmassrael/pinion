//! R1537 §5.16 — the swapchain surface and the intermediate target a
//! compute rasterizer draws into.

use crate::context::GpuError;

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
}

impl core::fmt::Debug for GpuSurface {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GpuSurface")
            .field("width", &self.config.width)
            .field("height", &self.config.height)
            .field("format", &self.format)
            .field("present_mode", &self.config.present_mode)
            .field("usage", &self.config.usage)
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
    pub(crate) fn new(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        present_mode: wgpu::PresentMode,
    ) -> Result<Self, GpuError> {
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
        let out = Self {
            surface,
            config,
            format,
            target_texture,
            target_view,
            blitter: wgpu::util::TextureBlitter::new(device, format),
        };
        out.configure(device);
        Ok(out)
    }

    /// The view the rasterizer draws into. Handed to
    /// `vello::Renderer::render_to_texture` as its target.
    #[must_use]
    pub fn target_view(&self) -> &wgpu::TextureView {
        &self.target_view
    }

    /// The intermediate texture behind [`Self::target_view`] — the source
    /// a readback copies from when it wants the rasterized frame rather
    /// than the presented one.
    #[must_use]
    pub fn target_texture(&self) -> &wgpu::Texture {
        &self.target_texture
    }

    /// Acquire the next presentable image.
    ///
    /// Returned raw (`wgpu`'s status enum, not a `Result`) because every
    /// non-success status wants a *different* response — reconfigure,
    /// retry, or skip — and flattening them into one error would erase the
    /// distinction the caller has to act on (R1049).
    #[must_use]
    pub fn acquire(&self) -> wgpu::CurrentSurfaceTexture {
        self.surface.get_current_texture()
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

    /// Whether the swapchain image can be the source of a copy — i.e.
    /// whether a present-stage readback is possible on this host.
    #[must_use]
    pub fn can_copy_from_surface(&self) -> bool {
        self.config.usage.contains(wgpu::TextureUsages::COPY_SRC)
    }

    pub(crate) fn configure(&self, device: &wgpu::Device) {
        self.surface.configure(device, &self.config);
    }

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
        self.configure(device);
    }
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
