//! Rust source emitter. R38 ratify (one file = one struct).
//!
//! ## R38.2d emission shape
//!
//! ```rust,ignore
//! use <path>;
//! use <other_path>;
//!
//! pub struct <Name> {
//!     pub <signal>: ::pinion_core::reactive::Signal<<ty>>,
//!     pub <computed>: ::pinion_core::reactive::Computed<<ty>>,
//!     pub <resource>: ::pinion_core::reactive::Resource<<ty>, <err>>,
//!     // ... binding children in declaration order
//! }
//!
//! impl <Name> {
//!     // signature variant A — no <resource> children:
//!     pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self { ... }
//!
//!     // signature variant B — at least one <resource> child:
//!     pub fn new<S>(_owner: &::pinion_core::reactive::Owner, spawner: &S) -> Self
//!     where S: ::pinion_core::reactive::LocalSpawner
//!     { ... }
//! }
//! ```
//!
//! ## Signature policy
//!
//! `<resource>` requires a `LocalSpawner` handle at construction
//! time to drive the initial fetch future. Documents with no
//! `<resource>` keep the simpler one-argument `new` so a downstream
//! that only uses signals/computeds is not forced to provide a
//! dummy spawner. The presence of any `<resource>` element widens
//! the signature; the choice is data-driven (children shape) rather
//! than user-toggled.
//!
//! Trade-off vs. always-spawner signature: long-term consistency at
//! the cost of a no-op argument. R38.2c keeps minimum surface; the
//! consistency-first variant is a carry-forward decision to revisit
//! once the dogfood corpus gives empirical signal.
//!
//! ## Capture policy
//!
//! `<computed>` and `<resource>` bodies may reference prior child
//! identifiers. The codegen emits an over-capture shadow block right
//! before the constructor call so the Rust borrow checker accepts the
//! body — runtime tracking (R26 push-pull) discovers the *actual*
//! dependency set at first use.
//!
//! `<computed>` uses `move ||` closure capture; `<resource>` uses
//! `async move { ... }` block capture — both rely on the caller body
//! using `move` semantics. pinion-forge does not wrap the body, so
//! authors must write `async move { ... }` explicitly inside `<resource>`
//! when prior captures are referenced.

use std::fmt::Write as _;

use crate::ast::{
    ComputedDecl, PinionChild, PinionDoc, PinionSpec, RendererBackend, ResourceDecl, SignalDecl,
    UseDecl, VelloAaMode,
};

const INDENT: &str = "    ";

/// Render `doc` to a self-contained Rust source string. Output is
/// valid `rustc` input and `cargo fmt`-stable (single trailing newline).
///
/// Dispatch is exhaustive on [`PinionSpec`]: adding a new kind variant
/// makes the compiler flag every callsite missing the new arm
/// (textbook ADT closure, mirrors `syn::Expr` / `serde_json::Value`
/// match-exhaustiveness usage).
#[must_use]
pub fn emit_rust(doc: &PinionDoc) -> String {
    match &doc.spec {
        PinionSpec::Reactive { children } => emit_reactive(&doc.name, children),
        PinionSpec::Renderer { backend } => emit_renderer(&doc.name, *backend),
    }
}

fn emit_reactive(name: &str, children: &[PinionChild]) -> String {
    let use_block = emit_use_block(children);
    let has_binding = children.iter().any(is_binding_child);
    let body = if has_binding {
        emit_struct_with_children(name, children)
    } else {
        emit_unit_struct(name)
    };
    format!("{use_block}{body}")
}

/// R46 §5.16 build slice 1 commit 2 — backend dispatch. Each backend
/// variant routes to its own emit template, consuming its variant-
/// specific payload (e.g. `aa` for Vello, future Headless / Softbuffer
/// arms attaching their own options). Adding a new variant is a single
/// arm here plus the matching template function; existing arms keep
/// their behavior.
fn emit_renderer(name: &str, backend: RendererBackend) -> String {
    match backend {
        RendererBackend::Vello { aa } => emit_renderer_vello(name, aa),
    }
}

/// R46.2 §5.16 Vello emit template (R46.2.1 added `aa` parameter).
/// Emits a self-contained Rust module wrapping
/// `vello::util::RenderContext` + `RenderSurface` + `vello::Renderer`
/// in a concrete type — zero virtual dispatch per §5.16 R45 (build-time
/// codegen per target). Vello 0.9 canonical pattern (Xilem reference
/// impl): `render_to_texture` → `blitter.copy` → present. Async `new`
/// because wgpu adapter+device acquisition is async; called once at
/// app boot per §6.3 boundary.
///
/// Substitutions: `__NAME__` / `__ERR_NAME__` (identifier + error type)
/// and `__AA_SUPPORT__` / `__AA_METHOD__` (Vello `AaSupport` struct
/// literal + `AaConfig` variant matching the manifest's `aa` attribute,
/// R46.2.1 forward-compat for §2#4 mode toggle).
fn emit_renderer_vello(name: &str, aa: VelloAaMode) -> String {
    let err_name = format!("{name}Error");
    VELLO_TEMPLATE
        .replace("__NAME__", name)
        .replace("__ERR_NAME__", &err_name)
        .replace("__AA_SUPPORT__", aa_support_literal(aa))
        .replace("__AA_METHOD__", aa_method_literal(aa))
}

/// Map [`VelloAaMode`] to the `vello::AaSupport` struct literal that
/// selects exactly one mode at build time (Vello compiles only that
/// mode's shaders → smaller binary, faster init). Uses Vello 0.9's
/// `AaSupport { area, msaa8, msaa16 }` pub-fields shape rather than the
/// `area_only()` helper so all three modes get a uniform emission.
/// R46.3.3 — fully-qualified `::vello::AaSupport` path so the emitted
/// code stays include!()-safe across consumer namespaces.
fn aa_support_literal(aa: VelloAaMode) -> &'static str {
    match aa {
        VelloAaMode::Area => "::vello::AaSupport { area: true, msaa8: false, msaa16: false }",
        VelloAaMode::Msaa8 => "::vello::AaSupport { area: false, msaa8: true, msaa16: false }",
        VelloAaMode::Msaa16 => "::vello::AaSupport { area: false, msaa8: false, msaa16: true }",
    }
}

/// Map [`VelloAaMode`] to the `vello::AaConfig` variant passed inside
/// `RenderParams` at each `render_to_texture` call. Must agree with
/// [`aa_support_literal`] — Vello panics at frame submission if the
/// runtime method is outside the compiled support set. R46.3.3 — fully-
/// qualified `::vello::AaConfig` path matches the template convention.
fn aa_method_literal(aa: VelloAaMode) -> &'static str {
    match aa {
        VelloAaMode::Area => "::vello::AaConfig::Area",
        VelloAaMode::Msaa8 => "::vello::AaConfig::Msaa8",
        VelloAaMode::Msaa16 => "::vello::AaConfig::Msaa16",
    }
}

/// Vello emit template body. Placeholders are all double-underscore
/// shaped (Rust-ident-style, can't collide with valid Rust syntax in
/// the surrounding template): `__NAME__` (struct ident), `__ERR_NAME__`
/// (error enum ident), `__AA_SUPPORT__` (Vello `AaSupport` struct
/// literal), `__AA_METHOD__` (Vello `AaConfig` variant).
///
/// R46.3.3 — every Vello / wgpu type is referenced via its absolute
/// path (`::vello::*`, `::vello::wgpu::*`). Matches the reactive emit
/// template's `::pinion_core::reactive::*` convention so consumers
/// can `include!()` the file at any module scope — no `use` items
/// leak into the surrounding scope, no `mod { ... }` wrap required
/// to isolate namespace.
const VELLO_TEMPLATE: &str = r#"// Generated by pinion-forge — DO NOT EDIT.
// kind="renderer" backend="vello"
//
// R46.2 §5.16 Vello first emit template — Linebender Vello as the
// UI-mode 2D rasterizer (R41 hybrid path C, Phase 1).
//
// R46.3.3 — fully-qualified type paths (no `use` items) so include!()
// at any module scope is namespace-safe (mirrors the reactive emit
// template's `::pinion_core::reactive::*` convention).

/// Vello-backed renderer emitted by pinion-forge for the
/// `kind="renderer" backend="vello"` manifest entry. Wraps Vello's
/// `RenderContext` + `RenderSurface` + `Renderer` in a single concrete
/// type so callers see zero virtual dispatch (§5.16 R45, compile-time
/// per target).
///
/// R1062 — the field names `context` / `surface` / `renderer` are a
/// LOAD-BEARING ABI: `pinion_shell::vello_renderer_impl!` (the macro
/// that bridges this struct into `VelloRenderer`) expands in this
/// struct's own module via `include!` and hands `&self.context` /
/// `&mut self.surface` / `&mut self.renderer` to
/// `vello_capture::capture_surface_rgba8`. Renaming a field here breaks
/// that macro at the downstream consumer's expansion site (a confusing
/// error far from this template) — so the names are a contract, not an
/// implementation detail. Keep the macro's field list in sync.
pub struct __NAME__ {
    context: ::vello::util::RenderContext,
    surface: ::vello::util::RenderSurface<'static>,
    renderer: ::vello::Renderer,
}

/// Errors returned by [`__NAME__`]. Closed enum so the caller can
/// `match` exhaustively; the `vello::Error` conversion is provided so
/// `?` propagation works for renderer init + frame submission.
#[derive(Debug)]
pub enum __ERR_NAME__ {
    /// Vello renderer init or frame submission failed.
    Vello(::vello::Error),
    /// Swapchain surface acquisition did not yield a presentable
    /// texture. The `&'static str` is the non-success
    /// [`::vello::wgpu::CurrentSurfaceTexture`] state (timeout /
    /// occluded / outdated / lost / validation) — a label rather than
    /// the enum itself so the error type cannot represent the success
    /// states (illegal-states-unrepresentable; wgpu 29 made
    /// `get_current_texture` return a status enum, not a `Result`).
    Surface(&'static str),
}

impl ::std::fmt::Display for __ERR_NAME__ {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::Vello(e) => write!(f, "vello error: {e}"),
            Self::Surface(reason) => write!(f, "wgpu surface acquisition failed: {reason}"),
        }
    }
}

impl ::std::error::Error for __ERR_NAME__ {
    fn source(&self) -> ::std::option::Option<&(dyn ::std::error::Error + 'static)> {
        match self {
            Self::Vello(e) => ::std::option::Option::Some(e),
            Self::Surface(_) => ::std::option::Option::None,
        }
    }
}

impl ::std::convert::From<::vello::Error> for __ERR_NAME__ {
    fn from(e: ::vello::Error) -> Self {
        Self::Vello(e)
    }
}

impl __NAME__ {
    /// Initialize Vello against a wgpu surface target. Accepts any
    /// type convertible into `wgpu::SurfaceTarget` — `Arc<Window>`
    /// from winit is the canonical input.
    ///
    /// Async because wgpu adapter + device acquisition is async; call
    /// from a `pollster::block_on` / `tokio::block_on` at the §6.3
    /// boundary (view-fn purity preserved — this runs at app boot,
    /// not inside a render closure).
    ///
    /// # Errors
    /// Returns [`__ERR_NAME__::Vello`] when surface creation or renderer
    /// init fails.
    pub async fn new<W>(target: W, width: u32, height: u32) -> ::std::result::Result<Self, __ERR_NAME__>
    where
        W: ::std::convert::Into<::vello::wgpu::SurfaceTarget<'static>>,
    {
        let mut context = ::vello::util::RenderContext::new();
        let mut surface = context
            .create_surface(target, width, height, ::vello::wgpu::PresentMode::AutoVsync)
            .await?;
        // R1060 §5.16 — enable COPY_SRC on the swapchain surface so the
        // live-surface capture (`scene/screenshot`, true present-stage
        // fidelity readback) can copy the exact texture the window
        // presents. Guarded by the surface's advertised usages: a surface
        // that cannot be a copy source keeps its default usage and
        // capture fails with a typed error, rather than a `configure`
        // validation error breaking all rendering.
        {
            let device_handle = &context.devices[surface.dev_id];
            if surface
                .surface
                .get_capabilities(device_handle.adapter())
                .usages
                .contains(::vello::wgpu::TextureUsages::COPY_SRC)
            {
                surface.config.usage |= ::vello::wgpu::TextureUsages::COPY_SRC;
                context.configure_surface(&surface);
            }
        }
        let device_handle = &context.devices[surface.dev_id];
        // R1049 §5.16 — absorb transient wgpu uncaptured errors (e.g. a
        // `create_view` on a momentarily-invalid swapchain texture right
        // after a surface invalidation) instead of letting wgpu's default
        // uncaptured-error handler hard-panic the process. `render`
        // reconfigures the surface on a non-presentable acquisition status,
        // so a logged transient error self-recovers on the next frame.
        // Installed once at device creation (wgpu 29: the handler is an
        // `Arc<dyn Fn(wgpu::Error) + Send + Sync>`).
        device_handle
            .device
            .on_uncaptured_error(::std::sync::Arc::new(|error| {
                ::std::eprintln!(
                    "pinion renderer: wgpu uncaptured error absorbed (not panicking): {error}"
                );
            }));
        let renderer = ::vello::Renderer::new(
            &device_handle.device,
            ::vello::RendererOptions {
                use_cpu: false,
                antialiasing_support: __AA_SUPPORT__,
                num_init_threads: ::std::option::Option::None,
                pipeline_cache: ::std::option::Option::None,
            },
        )?;
        ::std::result::Result::Ok(Self { context, surface, renderer })
    }

    /// Submit one Vello scene frame against the configured surface.
    /// Renders to the surface's intermediate texture, blits to the
    /// swapchain texture, presents — Vello 0.9 canonical pattern
    /// (Xilem reference impl).
    ///
    /// # Errors
    /// Returns [`__ERR_NAME__::Vello`] when frame submission fails or
    /// [`__ERR_NAME__::Surface`] when swapchain acquisition fails (lost,
    /// outdated, timeout).
    pub fn render(
        &mut self,
        scene: &::vello::Scene,
        base_color: ::vello::peniko::Color,
    ) -> ::std::result::Result<(), __ERR_NAME__> {
        let device_handle = &self.context.devices[self.surface.dev_id];
        self.renderer.render_to_texture(
            &device_handle.device,
            &device_handle.queue,
            scene,
            &self.surface.target_view,
            &::vello::RenderParams {
                base_color,
                width: self.surface.config.width,
                height: self.surface.config.height,
                antialiasing_method: __AA_METHOD__,
            },
        )?;
        // wgpu 29: `get_current_texture` returns a status enum instead
        // of `Result<_, SurfaceError>`. Present on Success/Suboptimal;
        // map the non-presentable states to a labelled surface error.
        //
        // R1049 §5.16 — recover the swapchain, don't just skip. A surface
        // that comes back Outdated / Lost / Validation stays broken until
        // it is reconfigured: every later frame re-fails the same way and,
        // worse, a stale-but-"Success" texture makes `create_view` raise a
        // wgpu validation error that the default handler turns into a hard
        // process panic. `configure_surface` (the Vello-blessed
        // `surface.configure(device, &config)`) re-establishes the
        // swapchain to the current config so the NEXT frame acquires a
        // fresh texture; this frame is still skipped (returns the labelled
        // `Surface` error the shell logs). Timeout (transient, retry) and
        // Occluded (window hidden) are not surface invalidations — a
        // reconfigure neither helps nor is needed, so they only skip.
        let surface_texture = match self.surface.surface.get_current_texture() {
            ::vello::wgpu::CurrentSurfaceTexture::Success(t)
            | ::vello::wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            ::vello::wgpu::CurrentSurfaceTexture::Timeout => {
                return ::std::result::Result::Err(__ERR_NAME__::Surface("timeout"));
            }
            ::vello::wgpu::CurrentSurfaceTexture::Occluded => {
                return ::std::result::Result::Err(__ERR_NAME__::Surface("occluded"));
            }
            ::vello::wgpu::CurrentSurfaceTexture::Outdated => {
                self.context.configure_surface(&self.surface);
                return ::std::result::Result::Err(__ERR_NAME__::Surface("outdated"));
            }
            ::vello::wgpu::CurrentSurfaceTexture::Lost => {
                self.context.configure_surface(&self.surface);
                return ::std::result::Result::Err(__ERR_NAME__::Surface("lost"));
            }
            ::vello::wgpu::CurrentSurfaceTexture::Validation => {
                self.context.configure_surface(&self.surface);
                return ::std::result::Result::Err(__ERR_NAME__::Surface("validation"));
            }
        };
        let mut encoder = device_handle.device.create_command_encoder(
            &::vello::wgpu::CommandEncoderDescriptor { label: ::std::option::Option::None },
        );
        self.surface.blitter.copy(
            &device_handle.device,
            &mut encoder,
            &self.surface.target_view,
            &surface_texture
                .texture
                .create_view(&::vello::wgpu::TextureViewDescriptor::default()),
        );
        device_handle.queue.submit([encoder.finish()]);
        surface_texture.present();
        ::std::result::Result::Ok(())
    }

    /// Resize the wgpu surface to match a new window dimension.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.context.resize_surface(&mut self.surface, width, height);
    }
}
"#;

/// Collect every `<use path="..."/>` into a single module-level
/// `use ...;` block at the top of the file, followed by one blank line
/// separating it from the struct definition. Returns an empty string
/// when the document has no `<use>` children (no leading blank line).
fn emit_use_block(children: &[PinionChild]) -> String {
    let mut out = String::new();
    for child in children {
        if let PinionChild::Use(UseDecl { path }) = child {
            let _ = writeln!(out, "use {path};");
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn is_binding_child(child: &PinionChild) -> bool {
    matches!(
        child,
        PinionChild::Signal(_) | PinionChild::Computed(_) | PinionChild::Resource(_)
    )
}

fn emit_unit_struct(name: &str) -> String {
    format!(
        "pub struct {name};\n\
         \n\
         impl {name} {{\n\
         {INDENT}#[must_use]\n\
         {INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n\
         {INDENT}{INDENT}Self\n\
         {INDENT}}}\n\
         }}\n"
    )
}

fn emit_struct_with_children(name: &str, children: &[PinionChild]) -> String {
    let mut fields = String::new();
    let mut bindings = String::new();
    let mut self_inits = String::new();

    // Names introduced by prior children — used as the over-capture set
    // for each subsequent <computed>/<resource> body. Order matters: the
    // user sees declarations evaluated top-to-bottom, so dependencies
    // must reference earlier children only.
    let mut prior_names: Vec<String> = Vec::new();

    for child in children {
        match child {
            PinionChild::Signal(s) => {
                emit_signal_into(s, &mut fields, &mut bindings, &mut self_inits);
                prior_names.push(s.name.clone());
            }
            PinionChild::Computed(c) => {
                emit_computed_into(c, &prior_names, &mut fields, &mut bindings, &mut self_inits);
                prior_names.push(c.name.clone());
            }
            PinionChild::Resource(r) => {
                emit_resource_into(r, &prior_names, &mut fields, &mut bindings, &mut self_inits);
                prior_names.push(r.name.clone());
            }
            PinionChild::Use(_) => {
                // <use> is emitted as a top-level `use` statement (see
                // emit_use_block); it does not produce a struct field,
                // a constructor binding, or a prior_names entry. The
                // import is visible to every closure body via Rust's
                // module-level scope, so over-capture is unnecessary.
            }
        }
    }

    let signature = if needs_spawner(children) {
        format!(
            "{INDENT}#[must_use]\n\
             {INDENT}pub fn new<S>(_owner: &::pinion_core::reactive::Owner, spawner: &S) -> Self\n\
             {INDENT}where\n\
             {INDENT}{INDENT}S: ::pinion_core::reactive::LocalSpawner,\n\
             {INDENT}{{\n"
        )
    } else {
        format!(
            "{INDENT}#[must_use]\n\
             {INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n"
        )
    };

    format!(
        "pub struct {name} {{\n\
         {fields}\
         }}\n\
         \n\
         impl {name} {{\n\
         {signature}\
         {bindings}\
         {INDENT}{INDENT}Self {{\n\
         {self_inits}\
         {INDENT}{INDENT}}}\n\
         {INDENT}}}\n\
         }}\n"
    )
}

fn needs_spawner(children: &[PinionChild]) -> bool {
    children
        .iter()
        .any(|c| matches!(c, PinionChild::Resource(_)))
}

fn emit_signal_into(
    s: &SignalDecl,
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    let _ = writeln!(
        fields,
        "{INDENT}pub {field}: ::pinion_core::reactive::Signal<{ty}>,",
        field = s.name,
        ty = s.ty,
    );
    let _ = writeln!(
        bindings,
        "{INDENT}{INDENT}let {field} = ::pinion_core::reactive::Signal::new({initial});",
        field = s.name,
        initial = s.initial,
    );
    let _ = writeln!(inits, "{INDENT}{INDENT}{INDENT}{field},", field = s.name);
}

fn emit_computed_into(
    c: &ComputedDecl,
    prior_names: &[String],
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    let _ = writeln!(
        fields,
        "{INDENT}pub {field}: ::pinion_core::reactive::Computed<{ty}>,",
        field = c.name,
        ty = c.ty,
    );

    if prior_names.is_empty() {
        let _ = writeln!(
            bindings,
            "{INDENT}{INDENT}let {field} = \
             ::pinion_core::reactive::Computed::new(move || {{ {body} }});",
            field = c.name,
            body = c.body,
        );
    } else {
        let (lhs, rhs) = capture_tuple(prior_names);
        let _ = writeln!(
            bindings,
            "{INDENT}{INDENT}let {field} = {{\n\
             {INDENT}{INDENT}{INDENT}#[allow(unused_variables, clippy::redundant_clone)]\n\
             {INDENT}{INDENT}{INDENT}let {lhs} = {rhs};\n\
             {INDENT}{INDENT}{INDENT}::pinion_core::reactive::Computed::new(move || {{ {body} }})\n\
             {INDENT}{INDENT}}};",
            field = c.name,
            body = c.body,
        );
    }

    let _ = writeln!(inits, "{INDENT}{INDENT}{INDENT}{field},", field = c.name);
}

fn emit_resource_into(
    r: &ResourceDecl,
    prior_names: &[String],
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    let _ = writeln!(
        fields,
        "{INDENT}pub {field}: ::pinion_core::reactive::Resource<{ty}, {err}>,",
        field = r.name,
        ty = r.ty,
        err = r.err,
    );

    // Initial state is Loading; the fetch_with call kicks off the
    // future immediately so the user's get() observes the eventual
    // Ready/Error transition.
    let _ = writeln!(
        bindings,
        "{INDENT}{INDENT}let {field} = \
         ::pinion_core::reactive::Resource::<{ty}, {err}>::loading();",
        field = r.name,
        ty = r.ty,
        err = r.err,
    );

    if prior_names.is_empty() {
        let _ = writeln!(
            bindings,
            "{INDENT}{INDENT}{field}.fetch_with(spawner, {body});",
            field = r.name,
            body = r.body,
        );
    } else {
        let (lhs, rhs) = capture_tuple(prior_names);
        let _ = writeln!(
            bindings,
            "{INDENT}{INDENT}{{\n\
             {INDENT}{INDENT}{INDENT}#[allow(unused_variables, clippy::redundant_clone)]\n\
             {INDENT}{INDENT}{INDENT}let {lhs} = {rhs};\n\
             {INDENT}{INDENT}{INDENT}{field}.fetch_with(spawner, {body});\n\
             {INDENT}{INDENT}}}",
            field = r.name,
            body = r.body,
        );
    }

    let _ = writeln!(inits, "{INDENT}{INDENT}{INDENT}{field},", field = r.name);
}

/// Build the over-capture `let` LHS and RHS as a parenthesized tuple.
/// Single-name uses the trailing-comma form (`(x,)`) for grammatical
/// uniformity with the multi-name case.
fn capture_tuple(prior_names: &[String]) -> (String, String) {
    debug_assert!(
        !prior_names.is_empty(),
        "capture_tuple called with no priors"
    );
    if prior_names.len() == 1 {
        let n = &prior_names[0];
        (format!("({n},)"), format!("({n}.clone(),)"))
    } else {
        let lhs = format!("({})", prior_names.join(", "));
        let rhs = format!(
            "({})",
            prior_names
                .iter()
                .map(|n| format!("{n}.clone()"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        (lhs, rhs)
    }
}
