# Cross-desktop drag preview window — the Qt-ADS `CFloatingDragPreview` model

> Design of record for R1147 (the dock-completion campaign, directive
> [[dock-must-be-complete-before-any-pivot]]). Completes the R1146 release-only
> redesign: R1146 stopped the heavy panel window from moving per-frame; R1147
> adds the **lightweight cross-desktop preview** the user found missing.

## The gap R1146 left

R1146 made dock-drag window mutation **release-only** (the real floating window is
created/repositioned once, on release — `docs/dock-window-move-redesign.md`). The
during-drag affordance is the **R1113 drag-image** (`pinion_overlay::inject_drag_image`):
a chip injected as a **scene overlay inside the source window's surface**.

The user live-tested R1146 and confirmed the hang/freeze is gone — but found the
chip is **clipped to the source window**: it cannot leave the window, so a
tear-OUT (drag a panel onto the desktop / another window) shows no follower once
the cursor crosses the window edge. VS Code's tab tear-out chip roams the whole
desktop; pinion's cannot, because an in-window scene node is bounded by its
window's surface.

## Why a preview window (and why not the alternatives)

The audit (`[[app-driven-window-move-is-the-wrong-architecture]]`) converged on a
**lightweight floating preview window** — exactly Qt Advanced Docking System's
`CFloatingDragPreview`. Rejected alternatives:

- **OS-native drag-and-drop (XDND / `WM_DROPFILES`)** — an *inter-app data
  transfer* mechanism. No native toolkit (Qt/GTK), nor Chrome/Firefox for tab
  tear-off, uses it for *internal* panel drags; they use an internal preview
  window or a real window move. (VS Code "uses DnD" only because it is a browser:
  DOM DnD.) Misapplication + huge platform-specific cost. NOT the answer.
- **A transparent desktop-spanning overlay window** — depends on compositor
  transparency (fragile; opaque-black-screen failure mode) and fakes the drag
  image. A hack. Rejected.

**Principle correction (recorded in the memory):** "per-frame OS-window move is a
smell" was an over-generalization. Per-frame move of the **heavy, churning panel
window** is wrong (it caused the freeze + the R1119 oscillation). A **light,
static preview window** moved per-frame is the native-toolkit STANDARD (Qt does
exactly this). So R1147 **completes** R1146, it does not contradict it.

## The design

A **shell-private, persistent, reusable preview window** — the window *is* the
chip:

1. **Opaque + chip-sized** — the window's whole content is the chip (label on a
   filled rounded box). No compositor transparency needed (sidesteps the
   transparent-overlay hack).
2. **Borderless + always-on-top** — `WindowSpec`-free; created directly with
   `WindowAttributes::with_decorations(false).with_window_level(WindowLevel::AlwaysOnTop).with_visible(false)`
   (winit 0.30.13 has all three — verified).
3. **Created once, reused** — lazily on the first preview-eligible drag, then
   kept (hidden via `set_visible(false)` between drags). **No per-gesture window
   creation** → no R1144-class surface-churn freeze. (The only mid-drag creation
   is the very first one; live-test it — if it hiccups, move creation to
   `resumed()`. See "Live-test risks".)
4. **Moved by a direct `set_outer_position`** — NOT the reactive
   `Signal → reconcile_windows` path (that heavy per-frame path contributed to the
   wedge). The shell calls `Window::set_outer_position` straight on the preview
   window from the `CursorMoved` arm.
5. **Positioned from the DESKTOP cursor** — `desktop = source_origin + window_local_cursor`
   (the existing `pinion_shell::desktop_position_from`, fed by the R1120
   `source_window_origin` stamping pipeline — which is therefore **load-bearing,
   not dead**). The cursor is read, never the window's own moved position, so
   there is **no feedback loop**: the R1119 oscillation is structurally
   impossible here.
6. **Render-once, move-many** — the chip content (the label) is fixed for a given
   drag, so it is painted **once** when the drag begins; every subsequent cursor
   move is a pure `set_outer_position` with **no repaint**. Zero per-move GPU
   work.

### Introspection-invisibility is a §2 #7 decision

The preview window is a **transient shell affordance**, exactly like the R1113
overlay and the focus ring — NOT scene-as-data the binding declares. Therefore it
is **invisible to `scene/windows`**: it is kept in a shell-private field, NOT in
`AppShell::windows`, NOT in `spec_id_to_window_id`, NOT registered via
`ShellCore::register_window`, and NOT part of the `windows_signal` reconcile.
`declared_window_specs()` (the `scene/windows` source) reports only the binding's
declared windows — unchanged. The during-drag chip *is* introspectable, but
through the existing R1113 in-window overlay in `scene/snapshot` (the AI-first
path, §2 #7), which the preview window mirrors visually for the human.

This is the same call R1113 made: the drag follower is a render-time projection of
the live drag session, not a declared scene element. Putting the preview window in
the declared set would (a) leak a shell affordance into the AI's window model and
(b) risk the reconcile/dispatch paths touching it. Shell-private by construction
is the clean answer.

## How it plugs in (insertion points)

All anchors verified against `crates/pinion-shell/src/app.rs` @ R1146 HEAD.

- **New struct** `DragPreviewWindow<R>` — `{ window: Arc<Window>, renderer: Box<R>,
  vello_scene: VelloScene, window_id: WindowId, visible: bool, painted_label:
  Option<String> }`. A minimal cousin of `WindowSlot` (no accesskit / IME /
  fragment-cache / intrinsic-resize — a fixed chip needs none).
- **New field** `AppShell::drag_preview: Option<DragPreviewWindow<V::Renderer>>`
  (next to `windows`, `primary_window_id`).
- **Renderer-init helper** — extract the `pollster::block_on(<V::Renderer as
  VelloRenderer>::new(..))` block from `resume_spec` (app.rs:1943) into
  `fn build_renderer(window: &Arc<Window>) -> Result<Box<V::Renderer>, _>`, reused
  by `resume_spec` and the preview-window creation (the memory's "refactor
  `resume_spec`'s renderer init into a reusable helper"). Pure extraction, no
  behaviour change → regression-covered by the existing window suite.
- **Lazy create** `fn ensure_drag_preview_window(&mut self, event_loop) ` — build
  the borderless/always-on-top/hidden window + its renderer; store in
  `self.drag_preview`. Idempotent (no-op if already built).
- **Chip scene** — new `pinion_overlay::drag_chip_scene(label, style) -> (Scene,
  (u32, u32))`: a standalone **opaque** chip at origin + its `(w, h)` (so the
  shell sizes the preview window to fit). Refactor the chip-size math out of
  `drag_image_rect` into a shared `chip_size(label, style)`; reuse `DragImageStyle`
  (a `with_opaque()` / forced-`a=255` fill for the window-is-the-chip case).
- **Render path** `fn render_drag_preview(&mut self)` — build the chip scene for
  the current label, `to_vello`, submit. Routed from the `RedrawRequested` arm:
  `if Some(window_id) == self.drag_preview.window_id => render_drag_preview()`
  (before the `render_window` slot lookup, which would early-return for a
  non-`windows` id anyway).
- **Drive (live, Slice 2)** — in `handle_cursor_moved` (app.rs:524), after the
  existing `stamp_drag_source_origin`: if a preview-eligible drag is active for
  this window (`active_drag_label_for_window` + `V::drag_image_style(label)` is
  `Some`), ensure the window, (re)paint the chip iff the label changed, show it,
  and `set_outer_position(desktop_position_from(source_origin, (lx, ly)))`. On
  drag end (pointer-up arm / a post-event drain that finds no active drag while
  the preview is visible) → `set_visible(false)`.
- **Overlay suppression** — when the desktop preview is showing a drag, suppress
  the R1113 in-window overlay (`apply_drag_image`) so there is exactly one chip.
  Gate via an explicit `ShellCore` flag set by `AppShell` (NOT by reusing
  `PINION_HIDDEN_WINDOW` as a proxy). Headless / hidden-window mode never shows
  the preview, so the in-window overlay remains the introspection chip there.

## Headless vs live split (what each slice proves)

| Concern | Verified by | Gate |
|---|---|---|
| Preview window absent from `scene/windows` | RPC/unit test | headless |
| Chip scene: size, opaque fill, label | `pinion_overlay` unit | headless |
| Desktop-position math (`source_origin + cursor`) | pure-fn unit (R1107/R1120 lineage) | headless |
| Renderer-init helper = no regression on declared windows | existing window suite | headless (lavapipe) |
| Overlay-suppression gate (one chip, not two) | unit on the flag | headless |
| **Chip roams the desktop + follows the cursor** | **user live-test on `:0`** | **HW** |
| **No hang / no freeze under a fast live drag** | **user live-test on `:0`** | **HW** |
| **First-drag (mid-drag) window creation is smooth** | **user live-test on `:0`** | **HW** |

## Live-test risks (build incrementally; the user verifies each on `:0`)

1. **First-drag window creation** — the window is created lazily on the first
   eligible drag (mid-drag), the exact moment that froze R1144. The risk is far
   narrower here (a small, clean, *separate* window with a fixed scene, created
   once and not reconciled, vs R1144's churning declared floater painted through
   the full view-fn while being reconciled+moved per-frame) — but it is mid-drag
   creation, so live-test it. If it hiccups, move `ensure_drag_preview_window` to
   `resumed()` (eager, hidden), accepting one idle hidden window per app.
2. **`set_outer_position` cadence** — direct per-`CursorMoved` moves of a *light*
   window. This is the native-toolkit standard, but the WM apply-latency trail
   (the inherent ⑥ carry) still applies; it should be smaller for a tiny window.
   Verify no flood-wedge at fast drag speeds.
3. **Always-on-top** — verify the chip floats above all windows (including other
   apps) on mutter, and that hiding it (`set_visible(false)`) is clean.

## Slice plan

- **Slice 1 (R1147)** — substrate, zero live-risk (nothing moves a window
  per-frame yet): `drag_chip_scene` + `chip_size`; `build_renderer` extraction;
  `DragPreviewWindow` + `ensure_drag_preview_window` + `render_drag_preview` +
  RedrawRequested routing; the headless tests in the table above. Visible
  deliverable = the substrate + a headless demo asserting `scene/windows` excludes
  the preview and the chip scene builds.
- **Slice 2 (R1148)** — live wiring: show/move/hide from the drag session +
  overlay suppression + the `PINION_HIDDEN_WINDOW` show-gate. **User live-tests on
  `:0`**; iterate on feel (framework-principled, not reactive patches —
  [[reactive-patching-of-live-complaints-accretes-smells]]).

The dock-completion directive [[dock-must-be-complete-before-any-pivot]] stands:
keep going on the dock until the user says it works; no pivot.
