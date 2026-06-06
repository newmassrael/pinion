# Renderer replacement roadmap (vello → pinion-owned)

> Status: **plan / Stage 0 investigated (R808+).** No replacement work is
> scheduled before Phase C. This doc records the design, the staged path,
> and the Stage-0 findings so the work is ready to pick up with full
> context. The 2D UI renderer (vello) is *not* the Phase C/D game renderer
> — replacing it is part of building the unified rendering substrate, not a
> bugfix detour.

## Why replace vello (eventually)

Four drivers converge on the same answer — build a pinion-owned renderer,
**staged**, not a big-bang swap:

1. **Bugs** — VELLO-001 (y=0 top-tile stroke flood) and VELLO-002 (wgpu 29
   GL `NoCompatibleDevice`); owning the renderer lets us fix them.
2. **Purity / dependency control** — a wgpu-based pure-Rust renderer.
3. **Phase C/D game engine** — vello is a *2D UI* renderer (no 3D / PBR /
   Nanite-class). The game phases need a unified wgpu renderer anyway; 2D
   UI rides on top and vello graduates naturally.
4. **Performance** — controlling the pipeline + specialising to pinion's
   `Scene` shape distribution.

## What pinion actually uses from vello

The dependency surface is small — **8 `vello::Scene` operations**:
`fill`, `stroke`, `push_clip_layer`, `pop_layer`, `draw_glyphs`,
`draw_image`, `draw_blurred_rounded_rect`, `append` — plus `peniko::Color`
and `kurbo` shapes. The replacement only needs these 8 ops on a new
backend, behind the existing seam (`paint_adapter` + the forge renderer
emit template + the TUI/GPU dual). Swapping backends does **not** break the
§2 invariants — the seam already exists.

## The hard core, and the pinion specialisation

Of the 8 ops, only **anti-aliased path rasterisation** (`fill`/`stroke`) is
genuinely hard — that is vello's years-of-research core (piet-gpu lineage),
and exactly where VELLO-001 lives. The rest (clip/layer, gradient, blur,
image blit, glyph atlas via parley/swash, affine, sRGB/linear colour) is
comparatively routine.

**pinion's specialisation angle**: pinion UI is ~99% rect / rounded-rect /
line / border / text — not arbitrary bezier soup. So a pinion-optimised
renderer can draw the common shapes with **analytic SDF coverage** and fall
back to tessellation only for rare arbitrary paths. Consequences:

- rounded-rect / border / focus-ring via SDF has **no tile-based raster**,
  so VELLO-001's y=0 flood is structurally impossible (bug fixed by design).
- no general path-pipeline overhead for the common case → can beat vello on
  UI workloads (a specialisation vello, being general-purpose, can't make).
- the genuinely-hard arbitrary-path AA raster is deferred to a rare path.

## Staging

| Stage | Scope | Drivers |
|---|---|---|
| **0 (now)** | Characterise + work around the upstream bugs in pinion's tree; decide vendoring. | 1 |
| **1 (Phase B tail)** | `pinion-render-wgpu` as a 2nd backend behind the seam, SDF-first for UI primitives; prove on the 8-op surface against the headless raster guards; pure Rust; measure vs vello. | 1, 2, 4 |
| **2 (Phase C)** | Extend the *same* renderer with 3D scene graph / PBR — the unified renderer. 2D UI rides on top; vello graduates. | 3 |

**De-risking asset we already have**: the deterministic headless raster
guards (offscreen wgpu readback, R806.1/R807/R808). Same `Scene` → vello vs
pinion-render → pixel compare = a ready conformance harness for the new
backend.

## SCE's role (honest)

- ✅ Renderer **lifecycle statechart** — surface-lost / resize /
  device-recovery / present-mode transitions (wgpu 29's
  `CurrentSurfaceTexture::Lost/Outdated` is literally statechart-shaped).
  SCE/Forge codegen + AI-introspection.
- ✅ Render **IR / command vocabulary** as SCE schema → render commands
  become scene-as-data for AI introspection (§2 #7 extended).
- ❌ The **rasteriser core** (SDF / tessellation / compute) is numeric
  algorithm work, not a state machine — SCE there is wrong-abstraction.
  SCE is the state + AI-bridge layer, not the pixel math.

## Stage 0 findings (R808)

- **Vendoring vello is unnecessary right now.** `vello::util::RenderContext`
  has public fields (`instance`, `devices`), so pinion can inject its own
  wgpu instance without forking vello. Vendor only when an in-shader patch
  (VELLO-001) or the replacement itself needs it.

- **VELLO-002 device-selection has a proven in-pinion fix** (verified R808,
  then reverted as currently inert — see below). Recipe, ready to apply:
  - In the forge Vello template's `new<W>`, replace
    `::vello::util::RenderContext::new()` (which builds the instance with
    `display: None`) with a display-aware instance:
    ```rust
    let mut desc =
        ::vello::wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    desc.display = Some(Box::new(target.clone())); // field type drives the
                                                   // unsizing coercion to the
                                                   // (unexported) WgpuHasDisplayHandle
    let instance = ::vello::wgpu::Instance::new(desc);
    let mut context = ::vello::util::RenderContext { instance, devices: Vec::new() };
    ```
  - Tighten `W`'s bound on `VelloRenderer::new` (the trait, the
    `vello_renderer_impl!` macro bridge, and `test_fixtures`) to
    `Into<SurfaceTarget<'static>> + vello::wgpu::rwh::HasDisplayHandle +
    Clone + Debug + Send + Sync + 'static`. `Arc<Window>` satisfies all of
    these (`raw_window_handle` impls `HasDisplayHandle` for `Arc<H>`).
  - Verified: with this, the shell **initialises the renderer under Xvfb +
    software-GL** (the `NoCompatibleDevice` boot death is gone), and the
    realgpu (`:0`+Vulkan) path still passes — no regression.

- **But the GL windowed path on vello 0.9 has a *second*, deeper problem**:
  after the renderer initialises, the event loop hangs — even a trivial
  `scene/pointer_leave` RPC (no render needed) times out, so the loop isn't
  pumping. Likely a wgpu-29 GL present/redraw block under Xvfb (worked on
  vello 0.6 / wgpu 26). Because of this the VELLO-002 device fix delivers
  **no end-to-end benefit today** (the realgpu sweep already works), so it
  was reverted rather than landed as inert, cross-cutting complexity. Apply
  the recipe above once the GL render-hang is also resolved.

- **Net Stage-0 outcome**: vello 0.9 stays; VELLO-001 keeps the
  `pinion_overlay::edge` geometry workaround; the headless demo sweep stays
  on `PINION_SWEEP_MODE=realgpu` (`:0`+Vulkan, 144/144). Both VELLO-001 and
  VELLO-002 (+ the GL render-hang) are upstream items to report/track; none
  blocks pinion on a real GPU.
