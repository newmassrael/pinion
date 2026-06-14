# Cross-platform native-surface integration strategy

> Status: **decision / plan (R932.2 design session).** Records the
> decided *direction* for how pinion integrates with each OS's native
> surfaces (menu, system tray, and the embedded target) without the
> platform asymmetry leaking into the core. Most of it is forward axis —
> only the parts marked **done** below are implemented today. This doc is
> the detailed companion to atomic-store **§5.53** (own-renderer surface
> strategy); §5.53 holds the formal decision, this holds the full reasoning.

## The goal — and the explicit anti-goal

The northern star is a self-hosted Unreal-class editor + AAA games, which
must ship on Windows / Linux / macOS (desktop) and, longer term, embedded /
console-class targets. So "abstracts well on every platform" is load-bearing,
not optional.

The **anti-goal** is "every platform behaves byte-identically." Forcing
identical behaviour everywhere is the classic failure (old Java Swing: alien
on every OS), and it is wrong — a macOS app should feel Mac-like, embedded has
no window manager at all. The achievable, correct goal is:

> **uniform core + narrow capability-honest trait seams + a native last-mile
> that follows each platform's conventions, with every difference
> introspectable.**

"Abstracts well" means *uniform core + native last-mile + visible seams*, not
*identical*.

## What already proves the model (verified codebase state, R932.2)

pinion's inv #1 (structured scene, no opaque paint callbacks) is what makes
this tractable: because pinion paints its **own** scene, the same post-layout
scene projects to many targets. This is already true today:

- **One scene → many render backends.** `Backend::{Gui, Tui, Rpc}`
  (`pinion-core/src/external.rs`) + `BackendSupport` / `BackendFallback`.
  GUI (vello/GPU), TUI (terminal), and RPC all dispatch one model.
- **Scene → PDF (R908, done)** — `pinion-pdf` renders the scene to a *vector*
  PDF document with **no rasterizer at all** (it lowers `Rect`/`BoxStyle`/
  `TextNode` to PDF page operators). Verified on the Linux dev box (17 tests +
  `r908_pdf_export.py`). This is the strongest existing proof that the
  abstraction holds across radically different targets.
- **Platform trait seams (done)** — the "core trait + `InMemory` fallback +
  platform impl" pattern is already the house style:
  - Storage: `Storage` + `InMemoryStorage` / `FileStorage` (`pinion-platform-storage`, R665)
  - Directory: `Directory` + `InMemoryDirectory` / `FsDirectory`
  - Print: `PrintBackend` + `InMemoryPrintBackend` / `CupsPrintBackend` (`pinion-platform-print`, R833)
  - Clipboard: `pinion-platform-clipboard` (arboard), File dialog: `pinion-platform-file-dialog`
- The shell is **winit**-based (X11 / Wayland directly; no gtk).

So the question is never "can we abstract across targets" (proven) — it is
"keep each platform's native last-mile *contained* behind these seams."

## The contract: one core, many backends behind trait seams

Five rules make the abstraction hold and are the binding part of this decision:

1. **Core has zero OS calls and zero `#[cfg]`.** Everything OS-facing goes
   through a trait (the §5.15 External 8-point contract is the template).
2. **Every platform trait has an `InMemory` / scripted fallback.** The core is
   testable headlessly on *any* machine; an unverified core never ships.
3. **Capability-honest traits.** A trait is the honest *intersection* of what
   platforms can do, plus *explicit capability flags*. A platform that lacks a
   feature **declares it absent** (queryable), never silently no-ops — that
   silent no-op is the `divergence-is-a-bug` failure.
4. **Native last-mile follows native convention.** Not forced identical:
   macOS global menu bar at screen top, Linux own-rendered in-window menu,
   embedded own-renders everything.
5. **Introspection is the uniformity contract.** The same RPC / scene-as-data
   works on every platform; what differs is *queryable*. The seams are visible
   and tested, not hidden — pinion's superpower for cross-platform.

The asymmetry that remains is confined to the OS last-mile *presentation*,
which is *supposed* to differ. The model, state, command routing, and
introspection stay platform-uniform.

## Per-platform native surfaces

### Menu (formalised in §5.53, R771/R771.1)

Decided: pinion's **own-renderer menu is 1st-class on ALL platforms** (the
game-engine/editor north star — Unreal Slate / Unity IMGUI / Blender / Godot
all draw their own menus). A native OS menu is an *optional* desktop-app
backend of the *same* menu model; macOS's top-of-screen global bar is its only
real win, and is an accepted trade-off to drop for editor/game apps.

On Linux the in-window menu pinion already own-renders **is** the native norm
for GNOME (which has no global app-menu); KDE's optional global menu is the
only Linux "native menu" surface, and it is **pure D-Bus** (`com.canonical.dbusmenu`),
not gtk — see below.

### Tray / status item (new axis — not yet specced before this session)

A self-hosted editor needs a system tray for long-running background work
(asset baking, game builds, a running server). Decided shape, mirroring the
R665 Storage / R833 Print pattern:

- a core `TrayBackend` trait + `InMemoryTrayBackend` fallback (so the tray
  model is testable headlessly),
- a `pinion-platform-tray` crate holding only the per-platform bridge,
- the tray **model + menu + state owned by pinion** (own-renderer contract);
  only the OS exposure is platform.

### The Linux question: pure D-Bus, NOT gtk

The off-the-shelf crates (`muda` for menu, `tray-icon` for tray, both from the
Tauri ecosystem) use **gtk** on their Linux backend, while pinion's shell is
**winit** (X11/Wayland, no gtk). The Linux native bridge is therefore **pure
D-Bus**, talking the freedesktop protocols directly:

- **Tray** = StatusNotifierItem (SNI) over D-Bus — e.g. the `ksni` crate
  (zbus-based, **no gtk**), or hand-rolled SNI behind the trait.
- **KDE global menu** = `com.canonical.dbusmenu` + `AppMenu.Registrar` (pure
  D-Bus, no gtk).

Win/macOS keep using `muda` / `tray-icon` (their native backends attach to a
winit window via `raw-window-handle`, so winit is *not* the problem there —
the gtk incompatibility is Linux-only).

## Why D-Bus, not gtk (symmetry + perf + verifiability)

**Symmetry.** The gtk path (init gtk + pump its loop each frame in
`about_to_wait`) makes the *Linux event loop* asymmetric — only Linux would
pump a gtk loop. The D-Bus path runs the SNI service on a background tokio task
and feeds events to the UI through the existing mpsc drain channel — the *same*
"backend → channel → UI" shape Win/macOS use. So D-Bus is **more** symmetric at
the architecture level, not less. (Swapping winit → tao, the gtk-integrated
winit fork, was also rejected: it drags gtk into the whole shell and
contradicts the lean own-render philosophy.)

**Performance.** Tray/menu is **control-plane, not hot-path**: register once,
update the icon on state-change (a few KB, or a themed-icon *name*), fire an
activation event on a human-paced click. It lives on a background tokio task
(epoll-driven, 0 CPU idle), so the 30–144fps render loop never touches D-Bus
synchronously — same as the JSON-RPC server and async IO already do. The one
discipline: **never update tray/menu state at frame rate** (update on change).
The dbusmenu menu-open has a few bounded D-Bus round-trips (its historical
"chattiness"), but that is per-open, sub-100ms, imperceptible, and only the
optional KDE global menu — the in-window menu is own-rendered (no D-Bus).

**Verifiability (the real gate).** A bare X11 `:0` dev box has no
StatusNotifierWatcher (panel) to host a tray, so you cannot *visually* confirm
it. But you can stand up a **test watcher on the session bus** and assert the
SNI item registers + its dbusmenu exports — headless, to the ZERO-FLAKE /
introspection bar, no visual panel. This is why D-Bus fits pinion: the seam is
testable as protocol state, not pixels.

## Capability honesty (the divergence guard)

The native surfaces are genuinely different feature sets, and the trait must be
honest about it rather than emulate:

- SNI has no macOS "app menu (About/Preferences/Quit)" concept.
- Notifications are a *separate* spec on Linux
  (`org.freedesktop.Notifications`), not part of the tray, vs part of the Win
  tray balloon — "tray notification" maps to different mechanisms.
- Threading differs: Win `Shell_NotifyIcon` / macOS `NSStatusItem` are
  **main-thread bound**; Linux SNI is a **background tokio task**. The platform
  crate absorbs this and presents the core *one event channel* regardless.

The trait is the intersection + explicit capability flags; a platform that
cannot do X reports X as absent (queryable), so the AI / app degrades
gracefully and the difference is never silent.

## Embedded as a backend (no_std + framebuffer)

Embedded (the user's explicit fourth target) is the hardest test, and is the
same pattern with more axes:

- **Core no_std.** `pinion-core` (scene / view-fn / SCE) must be `no_std`-able.
  It is already mostly pure Rust + structured data; today there is **no
  `#![no_std]`** anywhere (verified) — making the core (or a core subset)
  `no_std` + allocator-parameterised is the real work.
- **Framebuffer backend.** Scene → a software rasterizer / framebuffer (no GPU)
  — the same "one scene, N backends" the PDF backend already demonstrates (PDF
  needs no rasterizer; a framebuffer backend is its raster sibling).
- **Most platform-surface traits absent.** Embedded has no tray, no global
  menu, often no filesystem — these report *absent* by capability flag; the UI
  own-renders everything (which pinion does anyway).
- **Input.** Raw touch / GPIO → the same `Event` enum.

So embedded is not a new paradigm: *no_std core + framebuffer backend + most
platform-surface traits absent-by-capability*. It overlaps the Phase-C game
substrate's framebuffer / no_std concerns.

## Staged path (none scheduled before its consumer exists)

This is decided *direction*, deferred to real consumers + verifiable envs:

1. **Tray (Linux first)** — `TrayBackend` trait + `InMemoryTrayBackend` +
   `pinion-platform-tray` (ksni/SNI), verified via a D-Bus test watcher. The
   first concrete OS-native step that is Linux-doable *and* verifiable now.
2. **KDE global menu (dbusmenu)** — optional, after tray (shares the D-Bus
   machinery); GNOME stays own-rendered.
3. **Win / macOS native menu + tray** — `muda` / `tray-icon` behind the same
   traits, verified on those OSes' CI runners (the only blocker is having the
   OS, not the design).
4. **Embedded** — Phase-C-adjacent: core `no_std` + framebuffer backend; tray /
   menu absent-by-capability.

## Open questions / deferred

- Exact `TrayBackend` trait surface (the honest intersection) — pin at the
  implementation round, with capability flags for notifications / app-menu.
- Whether the KDE global menu earns its keep vs own-render-only on Linux
  (GNOME already gets the own-rendered in-window menu).
- `no_std` boundary in `pinion-core` (which subset; allocator strategy) — a
  Phase-C design round.
- Verification harness: a reusable D-Bus session + test-watcher fixture (the
  `ScriptedFileDialog` / `InMemory*` sibling for tray).
