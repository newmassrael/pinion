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
//! `<resource>` requires a [`LocalSpawner`] handle at construction
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

use crate::ast::{
    ComputedDecl, PinionChild, PinionDoc, PinionSpec, RendererBackend, ResourceDecl, SignalDecl,
    UseDecl,
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
/// variant routes to its own emit template. Adding a new
/// [`RendererBackend`] variant (e.g. `Headless` for the §5.12 screenshot
/// RPC, `Softbuffer` for a CPU dev path) is a single arm here plus the
/// matching template function; existing arms keep their behavior.
fn emit_renderer(name: &str, backend: RendererBackend) -> String {
    match backend {
        RendererBackend::Vello => emit_renderer_vello(name),
    }
}

/// R46.2 §5.16 Vello first emit template. Emits a self-contained Rust
/// module wrapping `vello::util::RenderContext` + `RenderSurface` +
/// `vello::Renderer` in a concrete type — zero virtual dispatch per
/// §5.16 R45 (build-time codegen per target). The Vello 0.6 canonical
/// pattern (Xilem reference impl): `render_to_texture` → `blitter.copy`
/// → present. Async `new` because wgpu adapter+device acquisition is
/// async; called once at app boot per §6.3 boundary.
///
/// Substitution: `__NAME__` and `__ERR_NAME__` placeholders are
/// replaced with the manifest identifier and `<name>Error` respectively.
/// The literal-replace shape (rather than `format!()`) keeps the
/// template body addressable as a single const — the template itself
/// is *data*, not a format string with embedded substitutions.
fn emit_renderer_vello(name: &str) -> String {
    let err_name = format!("{name}Error");
    VELLO_TEMPLATE
        .replace("__NAME__", name)
        .replace("__ERR_NAME__", &err_name)
}

/// Vello first emit template body. `__NAME__` and `__ERR_NAME__` are
/// the only substitution placeholders (both Rust-ident shaped, can't
/// collide with valid Rust syntax in the surrounding template). The
/// emitted module compiles against the `vello` workspace dep (which
/// re-exports `wgpu`); the consumer crate adds `vello = { workspace =
/// true }` to its `Cargo.toml`. winit `Arc<Window>` is the canonical
/// surface target — any `Into<wgpu::SurfaceTarget<'static>>` works.
const VELLO_TEMPLATE: &str = r#"//! Generated by pinion-forge — DO NOT EDIT.
//! kind="renderer" backend="vello"
//!
//! R46.2 §5.16 Vello first emit template — Linebender Vello as the
//! UI-mode 2D rasterizer (R41 hybrid path C, Phase 1).

use vello::peniko::Color;
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

/// Vello-backed renderer emitted by pinion-forge for the
/// `kind="renderer" backend="vello"` manifest entry. Wraps
/// [`RenderContext`] + [`RenderSurface`] + [`Renderer`] in a single
/// concrete type so callers see zero virtual dispatch (§5.16 R45,
/// compile-time per target).
pub struct __NAME__ {
    context: RenderContext,
    surface: RenderSurface<'static>,
    renderer: Renderer,
}

/// Errors returned by [`__NAME__`]. Closed enum so the caller can
/// `match` exhaustively; conversions for [`vello::Error`] and
/// [`wgpu::SurfaceError`] are provided so `?` propagation works.
#[derive(Debug)]
pub enum __ERR_NAME__ {
    /// Vello renderer init or frame submission failed.
    Vello(vello::Error),
    /// wgpu surface acquisition failed (lost / outdated / timeout).
    Surface(wgpu::SurfaceError),
}

impl std::fmt::Display for __ERR_NAME__ {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vello(e) => write!(f, "vello error: {e}"),
            Self::Surface(e) => write!(f, "wgpu surface error: {e}"),
        }
    }
}

impl std::error::Error for __ERR_NAME__ {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Vello(e) => Some(e),
            Self::Surface(e) => Some(e),
        }
    }
}

impl From<vello::Error> for __ERR_NAME__ {
    fn from(e: vello::Error) -> Self {
        Self::Vello(e)
    }
}

impl From<wgpu::SurfaceError> for __ERR_NAME__ {
    fn from(e: wgpu::SurfaceError) -> Self {
        Self::Surface(e)
    }
}

impl __NAME__ {
    /// Initialize Vello against a wgpu surface target. Accepts any
    /// type convertible into [`wgpu::SurfaceTarget`] — `Arc<Window>`
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
    pub async fn new<W>(target: W, width: u32, height: u32) -> Result<Self, __ERR_NAME__>
    where
        W: Into<wgpu::SurfaceTarget<'static>>,
    {
        let mut context = RenderContext::new();
        let surface = context
            .create_surface(target, width, height, wgpu::PresentMode::AutoVsync)
            .await?;
        let device_handle = &context.devices[surface.dev_id];
        let renderer = Renderer::new(
            &device_handle.device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )?;
        Ok(Self { context, surface, renderer })
    }

    /// Submit one Vello [`Scene`] frame against the configured surface.
    /// Renders to the surface's intermediate texture, blits to the
    /// swapchain texture, presents — Vello 0.6 canonical pattern
    /// (Xilem reference impl).
    ///
    /// # Errors
    /// Returns [`__ERR_NAME__::Vello`] when frame submission fails or
    /// [`__ERR_NAME__::Surface`] when swapchain acquisition fails (lost,
    /// outdated, timeout).
    pub fn render(&mut self, scene: &Scene, base_color: Color) -> Result<(), __ERR_NAME__> {
        let device_handle = &self.context.devices[self.surface.dev_id];
        self.renderer.render_to_texture(
            &device_handle.device,
            &device_handle.queue,
            scene,
            &self.surface.target_view,
            &RenderParams {
                base_color,
                width: self.surface.config.width,
                height: self.surface.config.height,
                antialiasing_method: AaConfig::Area,
            },
        )?;
        let surface_texture = self.surface.surface.get_current_texture()?;
        let mut encoder = device_handle
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.surface.blitter.copy(
            &device_handle.device,
            &mut encoder,
            &self.surface.target_view,
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device_handle.queue.submit([encoder.finish()]);
        surface_texture.present();
        Ok(())
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
            out.push_str(&format!("use {path};\n"));
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
    children.iter().any(|c| matches!(c, PinionChild::Resource(_)))
}

fn emit_signal_into(s: &SignalDecl, fields: &mut String, bindings: &mut String, inits: &mut String) {
    fields.push_str(&format!(
        "{INDENT}pub {field}: ::pinion_core::reactive::Signal<{ty}>,\n",
        field = s.name,
        ty = s.ty,
    ));
    bindings.push_str(&format!(
        "{INDENT}{INDENT}let {field} = ::pinion_core::reactive::Signal::new({initial});\n",
        field = s.name,
        initial = s.initial,
    ));
    inits.push_str(&format!("{INDENT}{INDENT}{INDENT}{field},\n", field = s.name));
}

fn emit_computed_into(
    c: &ComputedDecl,
    prior_names: &[String],
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    fields.push_str(&format!(
        "{INDENT}pub {field}: ::pinion_core::reactive::Computed<{ty}>,\n",
        field = c.name,
        ty = c.ty,
    ));

    if prior_names.is_empty() {
        bindings.push_str(&format!(
            "{INDENT}{INDENT}let {field} = \
             ::pinion_core::reactive::Computed::new(move || {{ {body} }});\n",
            field = c.name,
            body = c.body,
        ));
    } else {
        let (lhs, rhs) = capture_tuple(prior_names);
        bindings.push_str(&format!(
            "{INDENT}{INDENT}let {field} = {{\n\
             {INDENT}{INDENT}{INDENT}#[allow(unused_variables, clippy::redundant_clone)]\n\
             {INDENT}{INDENT}{INDENT}let {lhs} = {rhs};\n\
             {INDENT}{INDENT}{INDENT}::pinion_core::reactive::Computed::new(move || {{ {body} }})\n\
             {INDENT}{INDENT}}};\n",
            field = c.name,
            body = c.body,
        ));
    }

    inits.push_str(&format!("{INDENT}{INDENT}{INDENT}{field},\n", field = c.name));
}

fn emit_resource_into(
    r: &ResourceDecl,
    prior_names: &[String],
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    fields.push_str(&format!(
        "{INDENT}pub {field}: ::pinion_core::reactive::Resource<{ty}, {err}>,\n",
        field = r.name,
        ty = r.ty,
        err = r.err,
    ));

    // Initial state is Loading; the fetch_with call kicks off the
    // future immediately so the user's get() observes the eventual
    // Ready/Error transition.
    bindings.push_str(&format!(
        "{INDENT}{INDENT}let {field} = \
         ::pinion_core::reactive::Resource::<{ty}, {err}>::loading();\n",
        field = r.name,
        ty = r.ty,
        err = r.err,
    ));

    if prior_names.is_empty() {
        bindings.push_str(&format!(
            "{INDENT}{INDENT}{field}.fetch_with(spawner, {body});\n",
            field = r.name,
            body = r.body,
        ));
    } else {
        let (lhs, rhs) = capture_tuple(prior_names);
        bindings.push_str(&format!(
            "{INDENT}{INDENT}{{\n\
             {INDENT}{INDENT}{INDENT}#[allow(unused_variables, clippy::redundant_clone)]\n\
             {INDENT}{INDENT}{INDENT}let {lhs} = {rhs};\n\
             {INDENT}{INDENT}{INDENT}{field}.fetch_with(spawner, {body});\n\
             {INDENT}{INDENT}}}\n",
            field = r.name,
            body = r.body,
        ));
    }

    inits.push_str(&format!("{INDENT}{INDENT}{INDENT}{field},\n", field = r.name));
}

/// Build the over-capture `let` LHS and RHS as a parenthesized tuple.
/// Single-name uses the trailing-comma form (`(x,)`) for grammatical
/// uniformity with the multi-name case.
fn capture_tuple(prior_names: &[String]) -> (String, String) {
    debug_assert!(!prior_names.is_empty(), "capture_tuple called with no priors");
    if prior_names.len() == 1 {
        let n = &prior_names[0];
        (format!("({n},)"), format!("({n}.clone(),)"))
    } else {
        let lhs = format!("({})", prior_names.join(", "));
        let rhs = format!(
            "({})",
            prior_names.iter().map(|n| format!("{n}.clone()")).collect::<Vec<_>>().join(", ")
        );
        (lhs, rhs)
    }
}

