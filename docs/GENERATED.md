# GENERATED.md — atomic store derived view

this file `mnemosyne-cli generate-docs` output — direct no edit. atomic store (`docs/.atomic/workspace.atomic.json`) in mutate primitive (`set-section-*` / `append-changelog-entry-v2`) pass and then re-generate.

Source: `docs/.atomic/workspace.atomic.json`

---

## Sections

### §1. Vision


**Intent**: AI-native cross-platform GUI framework synthesized via SCE statechart and structured-scene DSL; Rust impl


**Rationale**:
- Existing GUI frameworks treat AI debugging as add-on, not first-class concern
- Token-efficient introspection requires structured scene, not opaque paint callbacks
- SCE statechart kind naturally fits widget/screen/gesture state machines
- Single source set multi-backend codegen pattern proven in watching-zenoh
- Qt commercial-only MCU GUI track creates demand for open synthesis alternative



**Inputs**:
- SCE statechart kind byte-golden 6-backend parity (watching-zenoh R15)
- Rust GUI ecosystem crates winit vello taffy cosmic-text accesskit
- Xilem-style view function pattern Raph Levien precedent
- Mnemosyne SSOT pattern for project-level audit trail
- First dogfood widget catalogue scope driver



**Outputs**:
- Framework Rust impl with invariants enforced at type/build level
- AI-introspectable RPC headless API exposed to MCP-compatible clients
- Cascade-emit GUI/TUI/RPC backends from canonical scene structure
- SCE Forge second domain demo (after watching-zenoh Zenoh protocol)








### §2. Settled invariants


**Intent**: v1 invariants: structured scene mandatory; RPC headless; dry_run; mode toggle; SCE-managed state; GUI/TUI dual; scene-as-data; SCE meta = AI authoring surface


**Rationale**:
- Structured scene enforced means AI introspect everywhere not pixel-blind
- Event-with-input contract collapses Tier-2 hypothetical-input awkwardness
- RPC headless = AI read path; SCE meta = AI write path; together = AI 1st-class
- dry_run primitive enables zero-cost scenario exploration via SCE determinism
- Mode toggle immediate vs retained = same view fn, two execution strategies
- SCE statechart kind has 6-backend byte-golden parity (watching-zenoh); other kinds extend the matrix
- Visual state (geometry, z-order, opacity stack) queryable as text, no pixels
- SCE-managed state spans statechart + signal/computed/resource + view-fn (statechart is one kind)



**Inputs**:
- SCE Forge multi-backend matrix (Rust, C11, Cpp, Python, Go, Kotlin)
- Mnemosyne atomic store pattern (audit-traced typed primitives)
- Xilem view function diffing pattern (immediate-feel author API)
- Vello declarative GPU renderer (dirty-only frame submission)



**Outputs**:
- Framework type-system invariants enforce 7 rules at compile time
- RPC interface schema (query, click, dry_run, snapshot, rewind, waitFor)
- Scene primitive type set (closed-form, no escape except Effect/External)
- Mode toggle runtime flag controlling diff vs full re-emit strategy



**Caveats**:
- R24.5: invariant #5 reworded (statechart → SCE-managed); #8 added (SCE as AI authoring surface).







### §3. Capability boundaries


**Intent**: Two opaque escape hatches recognized: Effect(shader) and External(content); scope excludes WebEngine and codec embed





**Caveats**:
- Effect(opaque) covers GPU shaders/blur/glow; pixel output excluded from introspection
- External(opaque) covers embedded SVG/PDF/video handles; declared boundary, no simulation
- Embedded WebEngine Chromium-class explicitly out of scope; no built-in browser widget
- Custom multimedia codec embedding out of scope; control surface only via External



**Alternatives rejected**:
- Universal dry_run claim — marketing-prone, hides genuine boundaries
- Imperative paint callback API — breaks visual introspection guarantee
- 3-tier widget purity classification — simplified to 2-tier via event-with-input rule
- Embedding Chromium/WebKit — ~30M LOC opaque subsystem, sublicensing cost
- Self-built video/audio codec embedding — scope creep vs framework focus






### §4. First dogfood: target application requirements


**Intent**: First dogfood must validate framework on real-world non-trivial GUI surface; autonomous schedule, no external deadline; widget catalogue criteria



**Inputs**:
- Designated first dogfood slide widget catalogue (Data Interp Viz Diag Util Sim)
- Designated wire spec subset for live capture decode (out-of-process)
- Approximately 12 core widgets and 6 domain-specific per slide analysis
- Network capture/replay/fuzz declared as external actor not framework widget



**Outputs**:
- First dogfood 90%+ widgets covered by Tier-1 pure (event-with-input contract)
- Framework MVP scope tied to actual customer-driven widget requirements
- Validates dry_run and RPC introspection on real-world non-trivial GUI surface
- Node graph topology widget tests Canvas + custom scene primitive coverage



**Caveats**:
- First dogfood autonomous; no delivery deadline binding framework cadence
- Framework MVP first then dogfood slice; not parallel deadline pressure
- Node graph widget largest single component; potentially 1-2 month sub-effort







### §5. Open implementation axes (Round 2 decomposition)


**Intent**: Open implementation axes formally decomposed: 10 sub-sections enumerate option sets, dependencies, and trade-offs deferred from Round 1 §5.X carry-forward


**Rationale**:
- Round 1 §1-§4 settled Vision/invariants/boundaries/dogfood; implementation form remains open
- Round 1 carry-forward names #1 #3 #5 #6 directly; #7-#10 cluster mentioned
- Round 2 ratifies axis set; option enumeration, deps, trade-offs; decisions deferred
- Mnemosyne audit grain = one decision per section; 10 sub-sections per axis



**Inputs**:
- Round 1 §2 settled invariants enumerate 7 commitments without implementation form
- Round 1 carry-forward bullets naming open axes directly and by cluster
- Mnemosyne audit-trail genre: each axis = independent section_id for cross-ref



**Outputs**:
- 10 sub-sections (§5.1-§5.10) enumerating options, trade-offs, dependencies
- Cross-ref graph from §1/§2/§3/§4 into §5.X via impact_scope
- Decision queue for Round 3+ (each axis resolved by superseding §5.X content)



**Caveats**:
- Sub-sections §5.2 and §5.4 are slot-inferred from §2 invariants; relabel possible in Round 3
- Axis options recorded as inputs/outputs/caveats; not alternatives_rejected (still open)




**Impact scope**: §1, §2, §3, §4




### §5.1. Strategic kickoff direction (framework-first vs dogfood-slice-first)


**Intent**: Decision: framework-first kickoff; common substrate (scene/RPC/dry_run) before first widget; ratified Round 3


**Rationale**:
- §1/§2/§4 cover Vision/invariants/dogfood but do not bind MVP construction direction
- Framework-first builds common substrate before first widget; slice-first inverts the dependency
- Choice gates §5.3 DSL form and §5.6 reuse path so must precede their decomposition
- watching-zenoh precedent succeeded with framework-first (SCE Forge canonical kind first)



**Inputs**:
- Option A framework-first: scene/RPC/dry_run substrate before any dogfood widget
- Option B dogfood-slice-first: target one dogfood widget, pull framework in as needed
- Rust GUI ecosystem maturity (winit/vello stable; taffy growing; cosmic-text young)
- First dogfood widget catalogue size (~12 core + 6 domain per slide)



**Outputs**:
- MVP scope binding for first 3-6 months of impl
- DSL form constraint propagates downstream to §5.3
- Reuse-path timing constraint propagates downstream to §5.6



**Caveats**:
- Decision must precede §5.3 and §5.6 to avoid back-propagation rework



**Alternatives rejected**:
- Dogfood-slice-first — first widget would shape framework; AI invariants too penetrating to retrofit
- Parallel both directions — dilutes design constraints and team focus



**Impact scope**: §5.3, §5.6




### §5.10. Mode toggle API surface (runtime flag vs build feature vs per-view)


**Intent**: Decision: runtime flag for immediate vs retained mode toggle; single binary; ratified Round 3


**Rationale**:
- §2 commits to mode toggle but binding mechanism is unspecified
- Determines author-facing API and runtime perf characteristic
- Xilem mode-toggle precedent uses runtime flag



**Inputs**:
- Option A runtime flag (single binary; runtime branch cost; max flexibility)
- Option B build feature (compile-time; no runtime cost; two binary variants)
- Option C per-view annotation (granular per-view choice; complex API)



**Outputs**:
- Author-facing API shape
- Runtime perf cost (zero for build feature, branch for runtime flag)
- Binary variant count (1 for runtime, 2 for build feature)



**Caveats**:
- Runtime flag simplest but pays branch cost forever
- Build feature cleanest perf but ships 2 binaries to users
- Per-view granular but overwhelming as default API
- R15: mode toggle (immediate/retained) declarable per-window via SCXML state attribute
- SCE-emit const per-window mode; no global runtime flag indirection



**Alternatives rejected**:
- Build feature — CI/distribution complexity doubled; two binary variants to ship
- Per-view annotation — author API too granular for default; cognitive overhead



**Impact scope**: §2




### §5.11. Scene primitive variant shape (minimal vs CSS-rich vs layered)


**Intent**: Decision: layered primitive shape (core variant + Style trait + Modifier composition); ratified Round 5


**Rationale**:
- §5.2 chose 7 variants but did not fix per-variant field shape
- Affects DSL ergonomics (§5.3), RPC payload size (§5.7), introspection cost
- taffy handles flexbox/grid layout; primitive shape orthogonal to layout



**Inputs**:
- Option A minimal: 5-10 fields per variant, basics only
- Option B CSS-rich: 30+ fields per variant, flexbox/grid/borders/effects native
- Option C layered: core variant struct + Style trait + Modifier composition
- Xilem composability via Modifier-style trait; AccessKit role taxonomy



**Outputs**:
- Rust enum/struct definitions for each variant
- DSL surface ergonomics constraint
- RPC schema variant payload size



**Caveats**:
- Minimal underserves first dogfood widget styling needs
- CSS-rich bloats type system and RPC payload
- Layered most flexible but steepest learning curve
- R17 BoxNode v0 schema: fill: u32 ARGB only; geometry/style settled by §5.3 DSL
- R17 BoxNode adds `Rect { x,y,w,h: u32 }` geometry; full DSL deferred to §5.3
- R17 ContainerNode v0 holds `children: Vec<Scene>`; taffy layout deferred to §5.3 DSL
- R17 TextNode v0 holds `content: String` + `rect: Rect`; font/size/colour deferred to §5.3
- R17 PathNode v0: `data: String` + `rect: Rect`; structured commands deferred to §5.3
- R17 ImageNode v0: `source: String` + `rect: Rect`; codec/loader deferred to §5.3 DSL
- R18 §5.20 adds tag: Option<Cow<'static, str>> to introspectable Scene variants
- R20 §5.3 lock: BoxNode.fill u32 → Color; PathNode.data → Vec<PathCommand>; *Style structs added.



**Alternatives rejected**:
- Minimal — 5-10 fields per variant; underserves first dogfood widget styling needs
- CSS-rich — 30+ fields per variant; type system bloat and RPC payload weight



**Impact scope**: §5.2, §5.3, §5.7




### §5.12. RPC method shape (generic query vs typed-per-action vs hybrid)


**Intent**: Decision: hybrid RPC shape (7 typed methods: query/click/dry_run/snapshot/rewind/waitFor/screenshot + path/filter sub-args); ratified Round 5, extended Round 7


**Rationale**:
- §2 names 6 methods; Round 7 adds 7th (screenshot) for pixel-level verification
- Per-method param/return shape specified per §5.12 hybrid pattern
- Affects AI client integration and schema discoverability
- Symbolic-first primary path, screenshot opt-in fallback for render pipeline bugs



**Inputs**:
- Option A generic query: single 'query' method + path arg + filter DSL
- Option B typed-per-action: one method per intent (query_scene, click_widget, ...)
- Option C hybrid: typed top-level + path/filter sub-args
- MCP wraps JSON-RPC; AI tooling expects typed methods typically



**Outputs**:
- JSON-RPC schema document covering 7 typed methods + path/filter sub-args
- Server method dispatch table
- AI client integration surface complexity
- Pixel screenshot channel for symbolic-vs-rendered diff verification



**Caveats**:
- Generic-query simpler server but path DSL becomes own language
- Typed methods cleaner client but proliferate (6 base × N variants)
- Hybrid pragmatic but two-paradigm cognitive cost
- v0 query path: /[window[id]/]external/<introspect_path>; full scene addressing pending §5.3 DSL
- v0 click: synthesize PointerEvent::Down at (x,y) and probe External::handles_event policy
- v0 rewind: ExternalIntrospect::intervene through /[window[id]/]external/<introspect_path>
- v0 snapshot: scene-root SnapshotNode; External::introspect schema fields enumerated when opted in
- v0 dry_run: save -> intervene -> snapshot -> rollback at /[window[id]/]external/<path>
- v0 waitFor: sync poll N attempts; deterministic across iterations until async event injection lands
- v0 screenshot: typed placeholder; RenderBackendUnavailable until §5.16 RHI/wgpu wires
- R17 scene/invoke 8th method (bidirectional RPC spec round); ratified 7-set extended
- R18 §5.20 scene/intents 9th RPC method; poll-form single-consumer v0
- R18 §5.20 slice 4: scene/intents 9th method (poll-form drain).
- R27 §5.23: scene/commands = 10th method; lists pending Commands from Update return.
- R28 §5.24: scene/semantic = 11th method; returns SemanticProps tree (role/state/actions).



**Alternatives rejected**:
- Generic-query single method with path DSL — DSL becomes own language; client complex
- Typed-per-action one method per intent — proliferates to N variants per base method



**Impact scope**: §2, §5.7



**Implementations**:
- crates/pinion-rpc/src/invoke.rs:invoke
- crates/pinion-rpc/src/intents.rs:drain_intents



### §5.13. Event model (closed enum vs open registry vs core+opaque)


**Intent**: Decision: closed core Event + opaque External event + logical DPI-aware coords; ratified Round 5


**Rationale**:
- §2 commits to event-with-input but variant set unspecified
- Affects view-fn input shape, RPC click method (§5.12), dry_run replay (§5.8)
- Coordinate system (logical vs physical) must be part of decision



**Inputs**:
- Option A closed Event enum (Click/Key/Touch/Gesture/Focus/Scroll/...)
- Option B open registry (user-extensible trait per §5.2 extensible parallel)
- Option C closed core + opaque External event (parallels §3 escape pattern)
- winit event surface; AccessKit event taxonomy reference



**Outputs**:
- Event Rust enum definition
- Input data per-variant shape (coords, modifiers, keycode, ...)
- view-fn signature input type



**Caveats**:
- Coordinate system: logical DPI-aware vs physical pixel — must be settled here
- Open registry weakens introspection like §5.2 reject did
- Forward-compat hedge: Event enum #[non_exhaustive] for SemVer minor variant additions
- Future variant slots: Gamepad/HID/Pointer3D addable without v2 bump; runtime zero-cost
- CoordSpace decoupled from variant: per-variant coord enum (Logical/World3D) for future 3D pointer
- R15: WindowEvent variants (Close/Focus/Resize/DpiChange) addable via R14 non_exhaustive hedge
- Window routing resolved at runtime layer; Event variants stay window-agnostic per §5.17
- R18 §5.20 Intent dual to Event: Event=input (winit→app), Intent=output (widget→app/RPC)



**Alternatives rejected**:
- Open Event registry — weakens introspection like §5.2 closed-form reject pattern
- Closed enum without External escape — cannot handle IME/drag-drop/OS-specific events
- Physical pixel coords — HiDPI requires 2x/3x handling everywhere; accessibility weak



**Impact scope**: §2, §5.8, §5.12




### §5.14. State containment topology (single root vs per-widget vs hierarchical)


**Intent**: Decision: hierarchical SCE topology (root + scoped child SCEs); ratified Round 5


**Rationale**:
- §5.4 chose SCE Forge Rust emit embedding; topology still open
- Affects state sharing, transition semantics, dry_run scope (§5.8)



**Inputs**:
- Option A single app-level SCE root (centralized state, simple dry_run)
- Option B per-widget SCE instance (composition, complex dry_run aggregation)
- Option C hierarchical (root + scoped child SCEs) — Xilem-style
- SCE Forge supports nested statechart kind natively



**Outputs**:
- Widget instantiation pattern (own state vs derived)
- dry_run scope semantics (whole app vs subtree)
- RPC snapshot/rewind granularity (§5.7 method shape impact)



**Caveats**:
- Single root limits per-widget reusability
- Per-widget complicates global transitions
- Hierarchical most flexible but boundary rules need clear spec
- R15: hierarchical SCE root is app.scxml; windows are <parallel> child states per §5.17
- Single-window app: app.scxml has 1 window state -> hierarchy collapses to single root SCE



**Alternatives rejected**:
- Single app-level SCE root — Redux-style; widget reusability loss; dry_run always app-wide
- Per-widget SCE instance — global state share difficult; widget-to-widget comm awkward



**Impact scope**: §5.4, §5.8, §5.12




### §5.15. External primitive integration contract


**Intent**: Decision: 8-point integration contract for External primitives (backend/repaint/thread/lifecycle/input/DPI/async/introspection); ratified Round 7


**Rationale**:
- §3 declared Effect/External as opaque escape but left integration contract unspecified
- Without contract, External authors implement ad-hoc → incompatible across projects
- Contract spans GUI/TUI dispatch, lifecycle, threading, repaint, input, DPI, async, introspection
- Game viewport, video player, PDF, native widget all share same general contract



**Inputs**:
- §3 boundary declaration (Effect/External opaque) without integration spec
- §5.9 trait-based Renderer needs backend dispatch info from External
- §5.14 hierarchical SCE allows External as scoped child node
- §5.12 RPC introspection extends optionally into External symbolic state



**Outputs**:
- 1. Backend support declaration (Gui/Tui/Rpc dispatch with fallback policy)
- 2. Repaint trigger ownership (framework layout vs External own render loop)
- 3. Thread ownership (UI thread sync vs own thread + sync channel)
- 4. Lifecycle event callbacks (mount/unmount/visibility/focus change)
- 5. Input forwarding policy (which events framework forwards vs External handles)
- 6. DPI/scale change notification + window resize
- 7. Async state change channel (External → framework state push)
- 8. Optional symbolic introspection (schema + query/intervene callbacks; opt-in)



**Caveats**:
- Contracts 1-7 mandatory; contract 8 (symbolic introspection) opt-in per External
- Non-conforming External rejected at scene composition time, not silently broken
- Game viewport is one consumer; video/PDF/native widget share same contract
- Item 8 opt-in via External::introspect()/introspect_mut() returning Option<&dyn ExternalIntrospect>
- R17 live RPC dogfood uses snapshot (read-only); bidirectional needs `Box<dyn External>` downcast
- R17 ExternalIntrospect.invoke: action channel returning IntrospectValue; §5.12 scene/invoke
- R17 hello-button: live bidirectional RPC via invoke; winit + JSON-RPC share one channel
- R18 §5.20 External adds drain_intents+is_dirty (9-point contract); default no-op




**Impact scope**: §3, §5.9, §5.10, §5.12, §5.14



**Implementations**:
- crates/pinion-core/src/widgets/button.rs:ButtonExternal
- crates/pinion-core/src/widgets/button.rs:ButtonStateSnapshot



### §5.16. GPU renderer architecture (runtime abstraction vs build-time codegen)


**Intent**: Decision (supersede Round 10 codegen): SCE Forge structural skeleton (SCXML state + Forge codec/buffer-pool/worker) + pinion thin RHI + naga; ratified Round 11


**Rationale**:
- Round 10 codegen-based decision technically incorrect — Futamura projection limit
- AAA workload overhead 25-240% in dynamic work; codegen scope mismatch (industry validated)
- SCE counter-proposal: ~70% surface already covered by Forge primitives (RFC reject accepted)
- Unreal/Unity/Frostbite/idTech/Source 2 all use runtime thin RHI, not codegen



**Inputs**:
- SCE Forge primitives: SCXML state, codec layout, buffer-pool, worker, sce:extern
- naga (gfx-rs) shader cross-compile (WGSL → SPIR-V/MSL/HLSL/DXIL)
- pinion thin RHI design (bgfx/makepad pattern; multi-threaded command, bindless, RDG)
- R15 watching-zenoh precedent: SCE consumer pattern with byte-golden parity



**Outputs**:
- pinion-render-core: SCXML-driven UI state, Forge codec/buffer-pool/worker integration
- pinion-render-rhi: thin RHI wrapping ash/metal-rs/windows-rs (bgfx/makepad scale)
- pinion-render-shader: naga-based WGSL → per-target shader emit
- Zero unjustified abstraction overhead; AAA-feasible via thin RHI + runtime techniques



**Caveats**:
- Canonical DSL design prerequisite (~3-6mo SCE Forge-class work)
- Codegen build cost per target (~6-12mo each)
- Dev iteration: codegen step adds build time; wgpu-fallback feature for dev
- Static pipeline first; dynamic resource lifecycle minimal layer ~zero
- SCE Forge must ship GPU codegen feature before pinion impl can proceed
- Round 11 supersede: codegen caveats above (1-2, 5) no longer apply per Round 11
- pinion thin RHI maintenance burden permanent; per-driver workaround responsibility
- AAA scale dynamic dispatch optimization is runtime engineering, not spec phase
- screenshot RPC method (§5.12 item 7) blocked on pinion-render-rhi delivery; v0 typed-only



**Alternatives rejected**:
- GPU pipeline codegen (full) — Futamura projection limit; dynamic dispatch out of scope
- wgpu-only runtime abstraction — 1-10% GUI / 25-240% AAA overhead, conflicts AAA aim
- Self-built RHI (Godot/UE pattern) — 3yr+ work, 1-5% residual overhead
- vello on wgpu — 2D only, 0.x maturity, scene model lock-in
- Per-platform native without abstraction — cross-platform self-build cost
- Self-built RHI without SCE skeleton — zenoh-proven SCE leverage pattern ignored



**Impact scope**: §1, §5.6, §5.9, §5.14, §6




### §5.17. Window topology (SCE-driven app statechart vs runtime registry)


**Intent**: Decision: app.scxml declares window topology; SCE Forge build-time emits WindowId/routing/lifecycle; single vs multi auto-branches from SCXML state count; zero runtime registry cost; ratified Round 15


**Rationale**:
- GUI framework primary scope mandates multi-window; game-engine evolution path also viable
- Runtime registry imposes single-window apps with cost they don't need (HashMap, Vec routing)
- SCE Forge already build-time emits state machines per §5.4; extend to window topology codegen
- Cargo feature gate splits build matrix; SCXML-driven emit avoids feature flag duality



**Inputs**:
- Option A runtime registry: Vec<Window> + HashMap routing for both single and multi apps
- Option B cargo feature multi-window: splits build matrix; LTO hope for single-window apps
- Option C chosen: app.scxml declares window states; SCE Forge emits enum/routing/lifecycle
- SCE Forge precedent: watching-zenoh R15 byte-golden parity SCXML emit pipeline
- winit native multi-window event loop already a de-facto dep per §6.4



**Outputs**:
- app.scxml convention: <parallel> of window <state>s; SCE Forge consumed at build.rs
- Build-time emit: WindowId enum, routing match, lifecycle hooks (onentry/onexit)
- Single-window app: 1 SCXML state -> minimal emit; no Application/registry code
- Multi-window app: N states -> full routing emit; runtime instances from static templates
- Dock undock: runtime instance creation from build-time-known templates only



**Caveats**:
- winit always compiles multi-window support; runtime cost unavoidable absent winit alternative
- Runtime template-only instantiation: cannot create windows with unforeseen topology at runtime
- Cross-window state share via SCXML datamodel or explicit channel; no implicit global state
- Dock-undock requires runtime window creation but only of build-time-declared templates



**Alternatives rejected**:
- Runtime registry (Vec+HashMap) — imposes single-window apps with multi-window cost
- Cargo feature multi-window — splits build matrix; doubles test/doc burden
- Type-level monomorphization (single/multi marker) — generic propagation API ergonomic cost



**Impact scope**: §5.4, §5.7, §5.10, §5.13, §5.14, §5.16




### §5.18. Multi-window RPC addressing (path prefix vs implicit first window)


**Intent**: Decision: RPC path optional /window[id]/ prefix; id matches SCE-emit const enum via perfect-hash; absent prefix routes to first SCE-declared window; single-window apps short-circuit; ratified Round 15


**Rationale**:
- §5.7 JSON-RPC ratified but path scheme unspecified for multi-window scope
- §5.17 SCE-emit WindowId enum provides static const space; perfect-hash dispatch viable
- Single-window apps must keep current path shape (no /window[0]/) for v1 RPC compat
- Dynamic string->WindowId lookup imposes runtime cost; SCE-emit perfect-hash is build-time



**Inputs**:
- Option A mandatory /window[id]/ prefix on all RPC paths -- breaks single-window v1 compat
- Option B chosen: optional prefix; absent prefix routes to first SCE-declared window
- Option C per-window RPC server instances -- transport bloat; multi-client coord cost
- §5.12 7 typed methods; path resolution is per-method server dispatch concern
- §5.17 SCE-emit WindowId enum names are the perfect-hash key set



**Outputs**:
- RPC path schema: /window[<id>]/<scene_path>; <id> matches SCE-emit enum variant
- Single-window: absent prefix -> SCE-emit first window const; zero parser branch cost
- Multi-window: build-time perfect-hash on enum names; runtime O(1) match
- v1 schema: existing paths (no prefix) remain valid; multi-window adds prefix opt-in



**Caveats**:
- Window id strings are SCE-emit const; runtime cannot register new window names
- Perfect-hash collision impossible; SCE-emit guarantees unique state ids in app.scxml
- Window-scoped ops (snapshot/rewind/dry_run) implicitly per-window when prefix present
- Interim reverse-map is linear scan over regions; perfect-hash codegen lands in pinion-core build.rs



**Alternatives rejected**:
- Mandatory /window[id]/ prefix — breaks v1 single-window RPC path compat
- Per-window RPC server instances — transport bloat; multi-client coordination cost
- Runtime HashMap lookup of window id strings — runtime cost vs build-time perfect-hash



**Impact scope**: §5.7, §5.12, §5.17




### §5.19. app.scxml convention (file location, declaration shape, build-time discovery)


**Intent**: app.scxml lives at consumer crate root; SCXML state set declares window topology; build.rs invokes sce_build at compile time


**Rationale**:
- Convention over configuration: fixed filename eliminates discovery ambiguity
- Crate-root placement keeps SCXML co-located with build.rs that consumes it
- SCE Forge emit at build.rs time consistent with widgets/button.scxml pattern (R12)
- Single root state means single-window app trivially; parallel root opt-in for multi-window



**Inputs**:
- §5.17 SCE-driven window topology decision (registry/feature/SCE-emit choice)
- §5.4 SCE Forge Rust emit pipeline (vendor/sce/sce-build)
- R12 widgets/button.scxml + build.rs strip-inner-attrs precedent
- W3C SCXML 1.0 syntax (parent statechart spec)



**Outputs**:
- File path: <consumer-crate>/app.scxml at crate root (sibling of Cargo.toml)
- Generated artifact: OUT_DIR/app_sm.rs consumed via include! after attr-strip
- Module name follows file stem: app.scxml -> mod app_sm (no user-mod collision)
- Pinion runtime re-exports SCE-emitted WindowId/routing/lifecycle for §5.18 path dispatch



**Caveats**:
- Single root state implies single-window app; no /window[id]/ RPC prefix per §5.18 short-circuit
- Multi-window declared via parallel root with N child states; each state id becomes WindowId variant
- view-fn purity preserved: app.scxml declares topology only; onentry/onexit must not call view-fn
- Generated app_sm.rs needs same inner-attr strip as button_sm.rs (include! disallows #![...])
- Convention is opt-in: crates without app.scxml skip build.rs invocation (no auto-discovery panic)
- Windows enumeration: get_parallel_regions(initial_state()) under parallel root; sole state otherwise



**Alternatives rejected**:
- pinion.toml declaring window count — duplicates SCXML semantics; rejected
- Macro attribute on view-fn declaring window — splits topology source across crate; rejected
- Per-window separate .scxml files — loses cross-window state share via SCXML datamodel



**Impact scope**: §5.4, §5.7, §5.17, §5.18, §6.3


**Examples**:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<scxml xmlns="http://www.w3.org/2005/07/scxml"
       version="1.0" datamodel="null"
       initial="main">
  <!-- Single-window: one root state. WindowId enum has one variant. -->
  <state id="main"/>
</scxml>
```

```rust
// In your build.rs (consumer crate):
fn main() {
    sce_build::compile_scxml(&["app.scxml"]);
    // Apply the same inner-attr/inner-doc strip as button_sm.rs
    // (see crates/pinion-core/build.rs precedent from R12).
}
```




### §5.2. Scene primitive type set (closed-form vs extensible)


**Intent**: Decision: closed-form primitive type set (Box/Text/Path/Image/Container/Effect/External); slot ratified Round 3


**Rationale**:
- §2 commits to structured scene mandatory; §3 commits to 2 opaque escapes (Effect/External)
- Closed-form set itself is unspecified in Round 1; binding scope contract pending
- Axis-slot inference from §2 invariants; ratify or supersede in Round 3
- Determines RPC schema discriminant union shape downstream of §2



**Inputs**:
- Option A closed-form: fixed type set (Box/Text/Path/Image/Container/Effect/External)
- Option B extensible: registry with Tier-2 declared primitives at framework boundary
- Xilem primitive set, Vello scene primitives, AccessKit role taxonomy as references



**Outputs**:
- Type set defines RPC schema discriminant union (query/snapshot return shape)
- Constrains author surface in §5.3 DSL form



**Caveats**:
- Closed-form binds scope forever; expanding requires major version
- Extensible weakens introspection guarantees per §3 capability boundaries
- Slot #2 inferred from §2 invariants; sub-section title may be relabeled in Round 3 ratify
- Forward-compat hedge: Rust enum #[non_exhaustive] enables SemVer minor variant additions
- RPC discriminant union: open-set kind semantics; clients route unknown kinds via fallback handler
- Hedge runtime cost: zero (match jump table identical); ergonomic tax: downstream forced _ arm
- Hedge intent: future game-engine evolution (Mesh/Camera/Light) addable without v2 major bump



**Alternatives rejected**:
- Extensible registry with Tier-2 declared primitives — weakens introspection per §3 boundary
- Free-form paint callback API — rejected in Round 1 §3 alternatives



**Impact scope**: §2, §3, §5.3




### §5.20. Intent system & bidirectional event flow


**Intent**: Bidirectional symbolic event channel: widgets emit Intent (tag+payload), runtime queue drains to app and RPC subscriber per-frame; one channel for human + AI input.


**Rationale**:
- view-fn purity (§6.3) + dry_run (§2 #3) require Intent be data, not callback
- Closed Scene (§5.2/§5.11) preserved: Scene non-generic; tag is Cow<'static, str> field
- AI-native bet (§2 #2): symbolic tag lets RPC introspect/replay without type knowledge
- Bidirectional symmetry: input via invoke (R17), output via Intent (R18 v0)
- dirty-poll + sink: avoid O(n_widgets x n_events) and 0-allocation per intent emit
- Derive macro (pinion-derive) bridges typed app enum <-> symbolic tag at compile time



**Inputs**:
- §5.13 Event enum (input channel from winit)
- §5.15 External 8-point contract (becomes 9-point with drain_intents/is_dirty)
- §5.12 RPC method set 8/8 -> 9 with scene/intents
- §5.11 Scene variant shape (gains tag: Option<Cow<'static, str>>)
- §6.3 view-fn purity (Intent values are pure data)



**Outputs**:
- Intent { tag: Cow<'static, str>, payload: IntrospectValue } closed shape v0
- External::drain_intents(sink) + is_dirty (default no-op; opt-in per External)
- scene/intents 9th RPC method (poll-form, single-consumer v0)
- pinion-derive crate: #[derive(IntentTag)] compile-time exhaustive at app boundary
- Runtime intent queue + per-frame drain (sync polling, view-fn purity preserved)
- Tag convention: <widget>.<kind> e.g. save_btn.click; IntentKind enum dropped



**Caveats**:
- R18 slice 1: Intent + IntentTag in pinion-core::intent; pinion-derive macro (scalar variants).
- R18 slice 2: 5 introspectable Scene variants gain optional tag field + with_tag builder.
- R18 slice 3: External::drain_intents/is_dirty defaults + runtime IntentQueue walk.
- R18 slice 5: ButtonExternal emits button.click intent on Pressed → Hover (PointerUp).
- R18 slice 6: hello-button drains intents after each event; logs to stderr; scene/intents RPC live.
- R22: ExternalNode.tag prefixes drained intent tag (widget.kind convention complete).
- R28 §5.24: §5.20 tag absorbed into SemanticProps.tag (richer role/state/actions schema).






**Implementations**:
- crates/pinion-core/src/intent.rs:Intent
- crates/pinion-core/src/intent.rs:IntentTag
- crates/pinion-derive/src/lib.rs:derive_intent_tag
- crates/pinion-core/src/scene.rs:BoxNode
- crates/pinion-core/src/external.rs:External::drain_intents
- crates/pinion-runtime/src/intent_queue.rs:walk_scene_and_drain
- crates/pinion-rpc/src/intents.rs:IntentsError
- crates/pinion-core/src/widgets/button.rs:ButtonExternal::send
- examples/hello-button/src/main.rs:App::drain_intents
- crates/pinion-core/src/scene.rs:ExternalNode



### §5.21. Layout system (taffy auto-layout, flex v0)


**Intent**: Auto-layout via taffy flexbox: ContainerNode + every leaf node carries LayoutStyle sidecar; pinion-runtime computes final Rect per node each frame before paint.


**Rationale**:
- §5.3 §5.11 v0 used absolute Rect coords — hello-button hardcodes BTN_RECT; not viable for real apps
- taffy is the de-facto Rust layout engine; same engine Iced/Dioxus/Floem use
- flex covers ~80% of real UI; grid additive in a later round, no v0 hard dep
- Auto-layout matches view-fn purity (§6.3): layout is a pure function of (LayoutStyle tree, viewport)
- Modifier {margin, padding, align} (R21) maps 1:1 onto taffy padding/margin/align → no dead code
- Single layout pass per frame populates rect; paint stays unchanged (reads rect like before)



**Inputs**:
- §5.2 Scene closed primitive set (5 introspectable variants gain layout sidecar)
- §5.3 R20/R21 schemas (BoxStyle/TextStyle/etc. unchanged; Layout is a new orthogonal sidecar)
- §5.11 layered shape decision (Layout is just another sidecar alongside Style + Modifier)
- §6.3 view-fn purity (layout pass is pure: same tree + viewport → same rects)
- Viewport dimensions from pinion-runtime WindowRouter (W, H per window)



**Outputs**:
- LayoutStyle { display: Flex, direction, justify, align, gap, size, padding } sidecar on every node
- pinion-runtime::layout::compute_layout(scene, viewport) — mutates Rect tree in place
- ContainerNode layout flows children top-down per taffy flexbox
- Modifier {margin, padding} fields fold into taffy style (Modifier.align kept for cross-axis anchor)
- hello-button BTN_RECT hardcoded coords removed; centered Button via flex justify_content=Center



**Caveats**:
- R23: LayoutStyle wraps taffy::Style — wrapper keeps taffy an impl detail, can swap if needed.
- R23: Scene 5 introspectable variants gain layout: LayoutStyle field (parallels Style/tag sidecars).
- R23: pinion-runtime::layout::compute_layout(&mut Scene, viewport_w, viewport_h) entry point.
- R23: Modifier {margin, padding} fold into taffy padding/margin; Modifier.align retained for anchor.
- R23: Grid display + transforms (translate/rotate/scale) explicit carry-forward; flex-only v0.



**Alternatives rejected**:
- Manual Rect-only (current R17) — non-starter at scale; hello-button already shows the brittleness
- Custom layout engine — months of work for a worse taffy
- morphorm / stretch — taffy succeeds both, no reason to revisit
- Grid in v0 — premature; flex must prove out before two-engine maintenance burden





**Implementations**:
- crates/pinion-core/src/style.rs:LayoutStyle
- crates/pinion-runtime/src/layout.rs:compute_layout
- examples/hello-button/src/main.rs:view



### §5.22. Reactive primitives (Signal / Computed / Resource)


**Intent**: Signal/Computed/Resource as fine-grained reactive state primitives; SCE meta declares signal graph; Forge generates Rust runtime; AI agent reads via scene/query and writes via Intent dispatch.


**Rationale**:
- view-fn purity (§5.3) requires sync read — Signal.get() is canonical pure access
- AI agent RPC introspection (§5.7) needs queryable state — Signal value via scene/query path
- dry_run (§2 #3) requires state snapshot — Signal clone for hypothetical exploration
- rewind (§5.12) requires state restore — Signal set() roundtrips prior value
- Solid/Vue3/SwiftUI consensus: fine-grained signal-based reactivity is textbook 2020s+
- Compose Snapshot rejected: thread-local context breaks out-of-process RPC introspection
- Hooks rejected: positional state breaks determinism + rules-of-hooks is anti-pattern
- Iced full-rebuild rejected: no derived state, perf ceiling, missing memoization layer



**Inputs**:
- §2 #8: SCE meta as AI authoring surface — Signal graph declared in SCE
- §5.20 intent system: Intent dispatch is the Signal write trigger via Update fn
- §5.3 view-fn: pure read of Signal/Computed values during scene rebuild
- §5.12 RPC: scene/query reads Signal value; rewind sets it; snapshot dumps graph



**Outputs**:
- pinion-core::reactive: Signal<T>, Computed<T>, Resource<T> primitives
- Owner/scope hierarchical lifecycle tree (thread-local v0)
- SCE schema: declarative signal graph with nested scope structure
- Forge codegen: SCE → Rust state struct + reactive wiring + introspect bindings
- ExternalIntrospect auto-generated for Signal values exposed at RPC paths



**Caveats**:
- R26: Signal<T> requires T: Clone + PartialEq; API = get()/set()/set_with(fn); eager init; sync read.
- R26: Resource<T,E> enum {Loading, Ready(T), Error(E)}; auto-refetch on dep change; cancel old task.
- R26: Owner tree thread-local v0; scope-based lifecycle; cleanup propagates to descendants on drop.
- R26: Batching = explicit batch(fn) closure; writes inside coalesce; propagation defers until exit.
- R26: Propagation = push-pull; push on write (mark dirty), pull lazy on read; topological order.
- R26: SCE schema = nested scopes; signal/computed/resource declarations inside scope; hierarchical.
- R26: Forge codegen target = Rust state struct + reactive wiring + introspect bindings auto-emitted.
- R26: dry_run snapshots Signal graph via Clone; rollback restores all signals to pre-mutation state.
- R26: Intent dispatch → Update mutates Signals; framework auto-propagates downstream Computed/Effect.
- R26: Concurrency v0 single-threaded; SyncSignal cross-thread variant carry-forward to later round.
- R26: view-fn read-only on Signals; Signal.get() inside view-fn auto-subscribes; writes forbidden.
- R26: Computed<T> lazy + cached dirty flag; pure fn contract; propagate only on value change.
- R26: RPC introspect = Signal<T:Serialize> via scene/query; rewind sets via deserialize.



**Alternatives rejected**:
- React hooks — positional state breaks determinism; rules-of-hooks anti-pattern
- Compose Snapshot — thread-local context breaks out-of-process RPC introspection
- Iced full-rebuild — no derived state, perf ceiling at 1k+ nodes, no memo layer
- Event sourcing only — orthogonal pattern, complements but doesn't replace fine-grained reactivity
- Vue refs — API close to Signal but no explicit Owner tree (lifecycle implicit)






### §5.23. Effect model (Effect / Command / handler)


**Intent**: Two-layer effects: Effect = reactive scope subscribing to Signals; Command<Intent> = declarative async/IO; Handler = dispatch impl. dry_run collects Commands without executing.


**Rationale**:
- Effect = reactive scope sibling of Computed; subscribes to Signal reads; closure on dep change
- Command<Intent> = Elm/Iced declarative async/IO description; serialize-friendly for inspection
- Handler trait (Roc capability pattern) separates description from dispatch for testability
- dry_run skips Command dispatch but collects pending for AI inspection — pinion-unique value
- Solid createEffect proves Signal-based reactive scope viable as framework primitive
- Conflating reactive scope + async (React useEffect) is anti-pattern; pinion separates the layers
- Structured concurrency: Commands scoped to Owner; cancellation on drop prevents orphan futures
- Update fn returns (new_model, Vec<Command>); framework dispatches outside Update purity



**Inputs**:
- §5.22 Signal: Effect subscribes to reads; Command may read for closure construction
- §5.20 Intent: Command<Intent> dispatches Intent back to Update reducer
- §5.3 view-fn: Effect cannot fire during view rebuild — read-only context
- §2 #8 SCE meta: Effect blocks + Command type tables + handler bindings authored in SCE



**Outputs**:
- pinion-core::reactive::Effect primitive (subscription scope, no return value)
- pinion-core::effect::Command<Intent> declarative struct
- Handler trait + registry (boot-time registration; swappable for testing)
- SCE schema: effect blocks + command type tables + handler bindings
- Forge codegen: Effect → closure registration; Command → struct + handler dispatch
- scene/commands RPC method addition (10th method; lists pending in-flight Commands)



**Caveats**:
- R27: Effect lazy registers on first Signal read inside scope; cleanup on Owner drop.
- R27: dry_run skips Effect side-effect; subscription still tracked for memo invalidation.
- R27: Command<Intent> requires Intent: Serialize for RPC inspection of in-flight commands.
- R27: Handler trait: async fn handle(Command) -> Intent; registered at boot; swappable.
- R27: Cancellation: new Command from same scope cancels prior in-flight (Solid pattern).
- R27: Update fn signature: Update(&mut Model, Intent) -> Vec<Command<Intent>>.
- R27: SCE schema: effect blocks within scope; command type tables; handler bindings.
- R27: Forge codegen: Effect closure + Command struct + handler dispatch all emitted.
- R27: scene/commands = 10th RPC method; lists pending in-flight Commands typed-introspectable.
- R27: view-fn no Effect/Command access; read-only Signal context; writes go via Intent.
- R27: Effect propagation = Owner tree topological order; sibling order = registration order.



**Alternatives rejected**:
- React useEffect — conflates reactive scope + async; positional lifecycle coupling
- raw async/await in Update — breaks determinism; dry_run can't skip side effects
- callback registration — imperative; not introspectable; orphan management hard
- IO monad (Haskell) — too abstract for AI authoring surface
- Free monad effects — powerful but compilation cost; over-engineered for GUI






### §5.24. Semantic tree (role / state / actions)


**Intent**: Semantic sidecar on every Scene node: role (button/text/list), state (enabled/focused), actions (invokable). Absorbs §5.20 tag; AccessKit/AT-SPI/UIA bridge target; AI agent 1st-class.


**Rationale**:
- Scene IR is already a semantic tree de facto; §5.20 tag was the first hint
- ARIA/AccessKit/AT-SPI/UIA/iOS all converge on role+state+actions triple
- AI agent needs semantic meaning (button), not visual (box with text)
- Compose Semantics tree is industry proof; SwiftUI same accessibilityElement pattern
- Single semantic surface drives accessibility AND AI introspection — no duplication
- Action handlers first-class invocables; map naturally to §5.20 intents
- Role enum closed-form per ARIA standard — textbook canonical taxonomy
- Live regions derive from Signal subscriptions automatically



**Inputs**:
- §5.20 intent + tag (R22): semantic tree absorbs tag as one field of richer struct
- §5.22 Signal: state fields backed by Signals enable reactive semantic updates
- §5.23 Effect/Command: actions dispatched via Command<Intent> on invoke
- ARIA spec (W3C) + AccessKit role taxonomy: canonical role enum source



**Outputs**:
- SemanticProps { role, state, actions, label, description } sidecar on every Scene node
- Role enum: closed-form ARIA taxonomy (Button/Heading/List/TextInput/etc.; ~30 variants)
- SemanticState bitflags (Enabled/Focused/Selected/Expanded/Checked/...)
- SemanticAction enum: Invoke/Increment/SelectChild/ExpandCollapse/etc.
- AccessKit bridge auto-generated from SemanticProps (later round impl)
- SCE schema: semantic annotations on Scene declarations; Forge emits SemanticProps



**Caveats**:
- R28: SemanticProps replaces §5.20 tag; tag becomes Option<Cow<str>> field within SemanticProps.
- R28: Role enum closed-form per ARIA; ~30 variants v0 (Button/Heading/List/TextInput/etc.).
- R28: SemanticState bitflags: Enabled/Focused/Selected/Expanded/Checked/Disabled/Hidden.
- R28: SemanticAction enum: Invoke/Increment/Decrement/SelectChild/Expand/Collapse/Custom.
- R28: Action handler = Command<Intent>; invoking action dispatches Command per §5.23.
- R28: SemanticProps {role, state, actions, label, description, tag}; default = Role::None.
- R28: scene/semantic = 11th RPC method; returns full SemanticProps tree (subset of snapshot).
- R28: Live region: SemanticProps.live_region: Polite/Assertive/Off; announces on Signal change.
- R28: AccessKit bridge derives from SemanticProps; no manual AT-binding code needed.
- R28: SCE schema: semantic block inline within Scene declaration; Forge emits SemanticProps init.



**Alternatives rejected**:
- String tag only (§5.20 R22) — too thin; AI must infer role from context
- Free-form key/value bag — no enum constraint; AI agents inconsistent
- Platform-native AT only (UIA/AT-SPI directly) — locks to one OS; misses AI use
- Compose Modifier.semantics chain — works but pinion uses field-on-node (simpler)
- Separate semantic tree parallel to Scene — two trees to sync; bug-prone






### §5.3. DSL surface form (file-based vs macro vs view-fn)


**Intent**: Decision: view function literal (Xilem-style); plain Rust functions, no separate DSL surface; ratified Round 3


**Rationale**:
- §1 mentions structured-scene DSL but surface form is unspecified
- Decision shape depends on §5.1 (framework-first frees DSL; slice-first constrains)
- Affects cascade-emit feasibility (§5.6) and tooling burden



**Inputs**:
- Option A file-based DSL (RON/YAML/custom syntax) — extern parser maintenance
- Option B Rust macro view!{} — compile-time check, Rust-only authoring
- Option C view function literal (Xilem-style) — no DSL, just Rust functions
- SCE Forge cascade-emit pattern; Xilem view-fn precedent



**Outputs**:
- Author-facing API surface (declarative vs imperative-feel)
- Tooling burden (LSP, syntax highlighting, formatter)
- Cascade-emit feasibility constraint (file-based easiest, view-fn hardest)



**Caveats**:
- File-based adds parser/LSP maintenance cost
- Macro-based ties to Rust-only authoring (no cascade to other langs)
- R20 §5.3 v0 schema lock: Color {r,g,b,a:u8} typed; ARGB u32 compat via from_argb/to_argb helpers.
- R20 §5.3: TextStyle {font_family:Option<Cow<str>>, font_size_px:u32, fg_color:Color}.
- R20 §5.3: ImageStyle {fit:Fit, tint:Option<Color>}; Fit enum {Fill,Contain,Cover,Tile}.
- R20 §5.3: taffy flexbox/grid integration deferred to follow-up spec round (carry-forward).
- R20 §5.3: BoxStyle {fill:Color, border:Border?, corner_radius:u32}; Border {color,width:u32}.
- R20 §5.3: PathCommand enum {MoveTo,LineTo,CurveTo,Close}; PathNode.data: Vec<PathCommand>.
- R20 §5.3: PathStyle {stroke:Stroke?, fill:Color?}; Stroke {color, width:u32, cap:StrokeCap}.
- R20 §5.3: Modifier {margin:Rect, padding:Rect, align:Align}; Align 9-pos (corners/edges/center).
- R23 §5.21 taffy lock: Modifier margin/padding consumed by layout pass; align retained.



**Alternatives rejected**:
- File-based DSL (RON/YAML) — too early hardening; parser/LSP maintenance cost
- Rust macro view!{} — Rust-only authoring; blocks cascade-emit to other langs
- Imperative paint callback — already rejected in Round 1 §3



**Impact scope**: §1, §5.1, §5.6



**Implementations**:
- crates/pinion-core/src/style.rs:Color
- crates/pinion-core/src/style.rs:BoxStyle
- crates/pinion-core/src/style.rs:Border
- crates/pinion-core/src/style.rs:TextStyle
- crates/pinion-core/src/style.rs:PathStyle
- crates/pinion-core/src/scene.rs:PathCommand
- crates/pinion-core/src/style.rs:ImageStyle
- crates/pinion-core/src/style.rs:Align
- crates/pinion-core/src/scene.rs:Modifier
- examples/hello-button/src/main.rs:paint_text



### §5.4. SCE backend embedding (Forge-emit vs FFI vs sce-rust crate)


**Intent**: Decision: embed SCE Forge Rust emit directly via vendor/sce submodule; slot ratified Round 3


**Rationale**:
- §2 commits to SCE state but embedding form is unspecified
- Determines build complexity, MCU compatibility, version pinning strategy
- Axis-slot inference from §2 invariants; ratify or supersede in Round 3
- Tightly coupled with §5.5 MCU scope (FFI route enables shared source)



**Inputs**:
- Option A embed SCE Forge Rust emit (vendor copy in tree, no_std variant possible)
- Option B FFI to SCE C11 emit (one source, AP and MCU share via C ABI)
- Option C cargo dep on official sce-rust crate (loose coupling, requires publish)
- vendor/sce submodule branch=main already wired (Round 1 scaffold)



**Outputs**:
- Build pipeline complexity (Rust-only vs Rust+C cross-compile)
- Version pinning surface (submodule SHA vs crate semver)
- MCU compatibility constraint (FFI most portable)



**Caveats**:
- FFI adds C boundary, debug overhead, but enables MCU shared source
- Rust-native simplest but ties to AP-only or no_std Rust target
- Slot #4 inferred from §2 SCE invariant; relabel possible in Round 3
- R15 scope expansion: SCE Forge also emits app.scxml window topology (WindowId/routing/lifecycle)
- SCE Forge role: widget statechart engine + app-level codegen backbone per §5.17
- First runtime exercise: examples/hello-button runs Engine<ButtonPolicy> in winit event loop



**Alternatives rejected**:
- FFI to SCE C11 emit — boundary overhead; MCU shared-source value moot under AP-only v1 (§5.5)
- Cargo dep on sce-rust crate — publish dependency; unsuitable for greenfield AP-only kickoff



**Impact scope**: §2, §5.5




### §5.5. MCU v1 backend scope (AP-only vs MCU-included)


**Intent**: Decision: AP-only v1 (Linux/Mac/Win); MCU deferred to v2+; ratified Round 3


**Rationale**:
- Round 1 carry-forward explicitly recommends AP-only first cut
- Defines no_std boundary and std::collections vs heapless choice
- MCU-first risk: Rust embedded GUI ecosystem too thin for greenfield MVP



**Inputs**:
- Option A AP-only v1 (Linux/Mac/Win; MCU deferred to v2 or later)
- Option B MCU-included v1 (Cortex-M class; no_std and heapless required)
- Rust ecosystem maturity: vello/winit/taffy AP-targeted; embedded-graphics for MCU
- Qt commercial-only MCU GUI track demand (§1 rationale) is real but later



**Outputs**:
- no_std boundary location in code
- Memory allocator strategy (std::alloc vs heapless vs custom)
- v1 deliverable surface (AP-only stack)



**Caveats**:
- AP-only does not preclude MCU later but binds early DI to std collections
- Switching to no_std post-MVP is a major refactor, not incremental



**Alternatives rejected**:
- MCU-first v1 — Rust embedded GUI ecosystem too thin for greenfield MVP per Round 1



**Impact scope**: §5.4




### §5.6. Reuse path (early cascade-emit vs Rust-native then port)


**Intent**: Decision: Rust-native MVP first, SCE-Forge-style cascade-emit layer added after canonical kind settles; ratified Round 3


**Rationale**:
- §1 outputs mention cascade-emit GUI/TUI/RPC backends; timing unspecified
- Watching-zenoh precedent succeeded with cascade from Round 15 onward
- Early cascade forces type-system perfection up front; late cascade risks ossification



**Inputs**:
- Option A cascade-emit from day 1 (force 6-backend byte-golden parity early)
- Option B Rust-native MVP then SCE-Forge-style codegen layer added later
- SCE Forge 6-backend byte-golden pattern as reference template
- First dogfood is Rust-only deliverable; cascade not on critical path



**Outputs**:
- Initial impl complexity multiplier (cascade ~3x effort up front)
- Multi-backend timing (when does C/Python/Kotlin emit come online)
- Type-system rigor required at scene primitive layer



**Caveats**:
- Early cascade demands all primitives translatable to lowest-common-denom backend
- Late cascade risks Rust-specific patterns leaking into canonical kind



**Alternatives rejected**:
- Cascade-emit from day 1 — narrows design space to multi-lang common denom too early



**Impact scope**: §1, §5.1




### §5.7. RPC headless protocol (MCP-native vs JSON-RPC vs gRPC)


**Intent**: Decision: JSON-RPC 2.0 transport; MCP and other AI tooling wrap on top; ratified Round 3


**Rationale**:
- §2 commits to RPC headless as AI primary path but protocol unspecified
- Protocol determines AI client compatibility surface
- MCP becoming standard for AI tooling integrations



**Inputs**:
- Option A MCP-native (Model Context Protocol direct; Anthropic SDK lifecycle)
- Option B JSON-RPC 2.0 (universal transport, simple schema)
- Option C gRPC (proto schema, strict typing, enterprise familiarity)
- Existing MCP servers in this project (Mnemosyne, Serena) — coupling precedent



**Outputs**:
- AI client compatibility surface
- Schema enforcement strictness (proto > JSON-RPC > MCP-loose)
- Tooling integration burden



**Caveats**:
- MCP-native couples to Anthropic SDK lifecycle, evolves with their protocol
- gRPC adds proto build dep and discovery complexity
- JSON-RPC simplest but weak schema enforcement
- R15 path schema: optional /window[id]/ prefix per §5.18; SCE-emit perfect-hash dispatch
- v1 single-window RPC paths remain valid; prefix absent routes to first SCE-declared window



**Alternatives rejected**:
- MCP-native direct — couples to Anthropic SDK lifecycle; MCP wraps JSON-RPC anyway
- gRPC — proto build dep and discovery complexity overweight for v1
- REST/HTTP — too stateless for interactive query/click/waitFor flow



**Impact scope**: §2




### §5.8. dry_run hook site (engine-level vs scene snapshot vs view rewind)


**Intent**: Decision: SCE engine-level hook for dry_run (intercept step function before commit); ratified Round 3


**Rationale**:
- §2 commits to dry_run via SCE determinism; mechanism unspecified
- Defines RPC dry_run API shape (input parameters and return)
- Couples to §5.4 SCE embedding (engine-level requires SCE internals access)



**Inputs**:
- Option A SCE engine-level hook (intercept SCE step function before commit)
- Option B scene-graph snapshot/restore (mutate then rollback)
- Option C view-fn rewind (re-run view with simulated event, diff result)
- SCE state determinism guarantee from watching-zenoh R15



**Outputs**:
- RPC dry_run API parameter shape (event + context vs scene-mutator)
- Performance characteristic (engine cheap, snapshot moderate, rewind expensive)



**Caveats**:
- Engine-level cleanest but couples to SCE internals (§5.4 impact)
- Rewind costliest but most modular (no SCE access required)
- Snapshot middle ground but scene-graph mutation cost unknown
- dry_run scope bounded to scene + SCE state; non-SCE simulation (physics/ECS/float counters) excluded
- Future game-engine subsystems must declare opt-out of dry_run determinism; no false guarantee
- v0 dry_run at External introspect level (test-and-rollback) until SCE engine-level step hook lands



**Alternatives rejected**:
- Scene-graph snapshot/restore — bypasses SCE determinism guarantee
- View-fn rewind — modular but cannot simulate engine-level steps reliably



**Impact scope**: §2, §5.4




### §5.9. GUI/TUI renderer split (trait-based vs separate pipelines)


**Intent**: Decision: trait-based Renderer abstraction (one scene → GUI/TUI via dispatch); ratified Round 3


**Rationale**:
- §2 commits to GUI/TUI dual but structural split is unspecified
- Determines code-share factor between GUI and TUI
- Affects scene-as-data query uniformity (single scene, two renders)



**Inputs**:
- Option A trait-based Renderer abstraction (one scene → both via dispatch)
- Option B separate pipelines sharing scene structure but distinct render passes
- Vello for GPU GUI; ratatui or custom for TUI



**Outputs**:
- Code-share factor (% logic reused between GUI and TUI)
- Performance ceiling per backend
- Author API uniformity across modes



**Caveats**:
- Trait-based easier code-share but performance ceiling capped by abstraction
- Separate pipelines max perf but code-duplication risk



**Alternatives rejected**:
- Separate pipelines — code duplication risk; weakens scene-as-data uniformity



**Impact scope**: §2




### §6. Bootstrap implementation choices (Tier 1 auto-ratified)


**Intent**: Tier 1 implementation choices auto-ratified Round 4 without axis split (ceremonial bloat avoidance); D/E/F/G handled as §5.11-§5.14 axes


**Rationale**:
- Tier 1 split into auto-ratified (A/B/C clear) vs axis-worthy (D/E/F/G genuinely open)
- Ceremonial axis decomposition for self-evident choices adds bloat without audit value
- Audit grain still preserved: each ratified choice gets own section (§6.1-§6.3)



**Inputs**:
- Round 3 ratified axes constrain choices (§5.1 framework-first, §5.5 AP-only, §5.7 JSON-RPC)
- Existing pinion repo state (vendor/sce wired, no Cargo.toml yet)
- Mnemosyne audit-grain pattern: one section per discrete decision



**Outputs**:
- Workspace skeleton ready for first commit (§5.1 framework-first)
- Toolchain pinning for reproducible builds
- Async model boundary clarified per §2 view-fn purity invariant





**Impact scope**: §5.1, §5.5, §5.7




### §6.1. Crate workspace structure


**Intent**: Decision: Cargo workspace with initial 4 crates (pinion-core, pinion-runtime, pinion-rpc, pinion-cli); pinion-* prefix; ratified Round 4


**Rationale**:
- Single crate would become monolith given §5.6 cascade-emit + §5.4 SCE embed scope
- 4 initial crates split by §5.1 framework layer + §5.7 RPC + §5.10 runtime
- Further split deferred until concrete crate boundaries surface from impl



**Inputs**:
- Option A: single crate (monolith risk for §5.4 + §5.6 scope)
- Option B (chosen): minimal workspace (pinion-core, runtime, rpc, cli)
- Option C: pre-emptive deep split (over-engineered before impl)



**Outputs**:
- Workspace root Cargo.toml with [workspace.members]
- Naming convention: pinion-* prefix for all crates
- Initial 4-crate skeleton committed in Round 5 implementation



**Caveats**:
- Inter-crate dep graph debugging cost; start shallow, deepen as boundaries emerge



**Alternatives rejected**:
- Single crate — would become monolith per §5.6 cascade and §5.4 SCE embed scope
- Pre-emptive deep split — over-engineered before concrete boundaries emerge



**Impact scope**: §5.1, §5.4, §5.6, §5.7, §5.10




### §6.2. Rust toolchain (MSRV + edition)


**Intent**: Decision: stable Rust, MSRV 1.85.0, edition 2024; ratified Round 4


**Rationale**:
- Stable Rust required for production framework distribution to downstream users
- Edition 2024 mature as of 2026; async closures and gen blocks aligned with §5.7 RPC
- 1.85.0 is minimum for edition 2024 support



**Inputs**:
- Stable vs nightly trade-off (nightly blocks downstream stable users)
- Edition 2024 feature surface (async closures, gen blocks, improved lifetimes)
- vello/winit current MSRV (typically lag latest stable by 1-2 versions)



**Outputs**:
- rust-toolchain.toml pinning stable channel + version
- Cargo.toml package.edition = '2024' across workspace
- MSRV documented in workspace metadata



**Caveats**:
- vello/winit may need nightly features; narrow-scope feature gate when encountered



**Alternatives rejected**:
- Nightly toolchain — blocks downstream stable users; not viable for framework distribution
- Edition 2021 — older feature surface; less aligned with current Rust async story






### §6.3. Async model


**Intent**: Decision: view-fn sync (purity invariant), RPC and IO async via tokio; ratified Round 4


**Rationale**:
- view-fn purity enables dry_run/diff guarantees per §2 invariant and §5.8 engine-level hook
- JSON-RPC server inherently async per §5.7 transport decision
- tokio is de-facto Rust async runtime with broadest ecosystem coverage



**Inputs**:
- view-fn sync vs async (sync chosen to preserve §2 dry_run purity)
- tokio vs smol vs async-std (tokio chosen for ecosystem and interop)
- Edition 2024 async features unlock cleaner pinion-rpc patterns



**Outputs**:
- tokio dep in pinion-rpc (default features narrow per IO needs)
- view-fn signature: fn(&State, &Frame) -> Scene (sync, read-only context slot)
- Async boundary at RPC server entry; view layer remains sync
- Frame: #[non_exhaustive] ZST in v1.0; ABI slot for future dt/frame_index w/o SemVer break



**Caveats**:
- Async view-fn could enable streaming/lazy scenes; defer until concrete need
- Frame ZST guarantee: size_of::<Frame>() == 0 in v1.0; LLVM elides &Frame from ABI; runtime zero-cost
- Frame must be read-only and side-effect-free to preserve §2 dry_run purity invariant



**Alternatives rejected**:
- Async view-fn — breaks dry_run purity (§2); complicates §5.8 SCE engine hook
- smol/async-std — smaller ecosystem; tokio interop required anyway for downstream



**Impact scope**: §2, §5.7, §5.8




### §6.4. Ecosystem default deps (winit, taffy, cosmic-text, accesskit, image, lyon, kurbo)


**Intent**: Decision: auto-ratify ecosystem default crates for window, layout, text, a11y, image, path, math; ratified Round 10


**Rationale**:
- Each crate has near-no real alternative in Rust ecosystem (de-facto choice)
- Axis decomposition for self-evident choices adds bloat without audit value
- Pattern consistent with §6.1-§6.3 auto-ratified bootstrap choices



**Inputs**:
- winit (window/event loop) — only viable Rust cross-platform option
- taffy (flexbox/grid layout) — Rust modern layout default
- cosmic-text + swash (text shaping) — mature shaping pipeline
- accesskit (a11y bridge) — only cross-platform a11y option for Rust
- image (image decoding) — standard PNG/JPEG/WebP crate
- lyon (path tessellation) — 6+yr mature, codegen input
- kurbo (vector math) — Linebender, stable, codegen input



**Outputs**:
- Cargo.toml workspace dependencies pinned
- Cross-cutting infrastructure crates wired into pinion-core/runtime/render
- Pipeline tessellation via lyon → codegen target
- Glyph atlas via cosmic-text → codegen target for text rendering



**Caveats**:
- Each crate evolves independently; minor version updates may need migration
- kurbo is Linebender-maintained (same group as vello); kurbo is standalone math, not vello



**Alternatives rejected**:
- Per-crate axis split — each has no real alternative; ceremonial bloat



**Impact scope**: §6, §5.16




## Changelog (atomic ledger)

### Round 1 — Initial pinion spec capture: 7 framework invariants, 2 opaque escapes, first dogfood, dual license, scaffold

**Changes**:
- §1 Vision: AI-native cross-platform GUI framework via SCE statechart + structured scene
- §2 Settled invariants: 7 binding rules (scene, RPC headless, dry_run, mode, SCE, dual, DSL)
- §3 Capability boundaries: Effect/External opaque escape; WebEngine/codec out of scope
- §4 First dogfood: target application (autonomous, no deadline, real widget surface)
- Project scaffold: SCE submodule branch=main, Mnemosyne workspace, .githooks copy
- License files: LICENSE + LICENSE-COMMERCIAL + LGPL-3 verbatim + GPL-3 verbatim
- .gitignore: GENERATED.md committed (greenfield doc surface, atomic-first design)
- .mcp.json: mnemosyne-mcp pointing at /home/coin/pinion workspace



**Verification**:
- validate-workspace baseline: docs=1/1, T1 orphan=0, round-trip=1/1, GENERATED.md=sync
- Pre-commit hook smoke test: exit=0 on clean baseline
- SCE submodule fetch: vendor/sce populated, .gitmodules branch=main tracking confirmed
- core.hooksPath set to .githooks, 3 hooks executable (commit-msg, pre-commit, pre-push)
- 4 atomic sections (§1-§4) authored via mnemosyne-cli with full required fields
- Sidecar writes verified: docs/.atomic/workspace.atomic.json grows monotonically per primitive



**Impact**: §1, §2, §3, §4


**Carry forward**:
- §5.X open axes formal decomposition (Round 2) with refined option sets
- Dependent axes clustering: #3 DSL depends on #1 first cut, #6 reuse follows #1
- Axis #1 decision (framework-first vs dogfood-slice-first) gating for #3 and #6
- CLAUDE.md authoring for pinion (SSOT contract + auto-kickoff trigger)
- Initial git commit (SCE submodule + Mnemosyne workspace + license + atomic + Round 1)
- Open axis #5 MCU v1 backend decision (recommend AP-only first cut)
- Open axis #7-#10 AI-native core invariants (RPC headless, dry_run, TUI dual)



### Round 10 — Round 10 — §5.16 codegen-based zero-overhead renderer + §6.4 ecosystem default deps; SCE Forge prerequisite

**Changes**:
- §5.16 new: codegen-based renderer (canonical DSL → per-target native code)
- §5.16 SCE Forge pattern applied to GPU layer (6-backend byte-golden precedent)
- §5.16 zero runtime abstraction; cross-platform preserved via build-time emit
- §5.16 targets: pinion-codegen-{vulkan, metal, dx12, webgpu}
- §5.16 wgpu retained as dev-iteration / web fallback feature
- §6.4 new: auto-ratify default deps (winit/taffy/cosmic-text/accesskit/image/lyon/kurbo)
- §6.4 pattern consistent with §6.1-§6.3 Tier 1 auto-ratify



**Verification**:
- 2 add_section + ~16 set/add mutations for §5.16 and §6.4
- T1 cross-ref pre-write check passed on all calls
- Pending: validate_workspace + verify_generated post-Round 10



**Impact**: §1, §5.6, §5.9, §6, §6.1, §6.2, §6.3, §5.16, §6.4


**Carry forward**:
- Pre-condition: SCE Forge ships GPU codegen feature (RFC to SCE Forge submitted)
- pinion implementation blocked on SCE Forge GPU codegen delivery
- After SCE Forge delivery: pinion-render-core canonical DSL design (~3-6mo)
- After SCE Forge delivery: pinion-codegen-{vulkan,metal,dx12} per-target work
- §5.17 Scene3D scope axis open (future)
- §5.18 3D renderer integration axis open (future, after §5.17)
- §5.19 canonical DSL detail axis open (after SCE Forge feature ratified)
- §5.20 codegen target list axis open (build matrix decision)
- pinion-render-tiny (tiny-skia CPU) for CI/headless: future Tier 2 axis



### Round 11 — Round 11 — §5.16 supersede (codegen reject 수락): SCE Forge skeleton + pinion thin RHI + naga; SCE counter-proposal accepted

**Changes**:
- §5.16 intent supersede: SCE Forge structural skeleton + thin RHI + naga (codegen removed)
- §5.16 alternatives expanded: GPU pipeline codegen (full) added as rejected
- §5.16 rationale: Futamura projection limit + AAA dynamic dispatch industry validation
- §5.16 outputs: pinion-render-{core,rhi,shader} crates with SCE Forge integration
- §5.16 caveats: append Round 11 supersede notes; retain Round 10 caveats for audit
- SCE Forge GPU codegen RFC withdrawn (counter-proposal accepted)
- Implementation unblocked: no longer waits for SCE Phase B (9-12mo saved)



**Verification**:
- §5.16 fields updated: intent/rationale/inputs/outputs/alternatives/impact_scope replaced
- 3 new caveats appended (Round 11 supersede notes); Round 10 caveats retained
- T1 cross-ref pre-write check passed; round-trip preserved
- Pending: validate_workspace + verify_generated post-Round 11



**Impact**: §1, §5.6, §5.9, §5.14, §5.16, §6


**Carry forward**:
- pinion-render-core crate: SCXML UI state model + Forge primitives integration
- pinion-render-rhi crate: thin RHI bgfx/makepad-scale; ash + metal-rs + windows-rs backends
- pinion-render-shader crate: naga-based WGSL → SPIR-V/MSL/HLSL/DXIL cross-compile
- Implementation Round 12+ unblocked (no SCE Phase B prerequisite)
- SCXML scope ratify (§5.14 + §5.16 alignment): widget state, navigation, animation, gesture
- Render graph DAG via SCXML parallel regions: stretch fit, prototype if needed
- Thin RHI design: GPU-driven rendering, bindless, multi-threaded command pre-record
- AAA scale runtime optimization (research-grade): culling, batching, dispatch, sync
- Round 10 §5.16 caveats 1-2-5 superseded by Round 11; retained as audit trail



### Round 12 — Round 12 — first implementation atomic: pinion-core Button widget SCXML state machine integration (SCE consumer pattern R15 동형)

**Changes**:
- crates/pinion-core/widgets/button.scxml: 4-state machine (idle/hover/pressed/disabled)
- crates/pinion-core/Cargo.toml: sce-rust-runtime + sce-build path deps (no-script feature)
- crates/pinion-core/build.rs: sce_build::compile_scxml invocation + post-process strip
- crates/pinion-core/src/widgets/button.rs: Button wrapper + 7 unit tests
- Cargo.toml workspace: vendor/sce excluded; unsafe_code forbid → deny (generated code needs)
- build.rs post-process: strip inner attributes (#![..]) and inner docs (//!) for include!() compat
- SCE Forge consumer pattern proven: SCXML → Rust state machine via sce-build crate



**Verification**:
- cargo test -p pinion-core: 7 tests passed (initial / hover / click / cancel / disable / enable / state)
- cargo check --workspace: clean
- SCE blocker validation: zero (sce-rust-runtime + sce-build both production)
- Generated button_sm.rs: 542 lines via sce-build minijinja templates



**Impact**: §5.14, §5.16, §6.1


**Carry forward**:
- Round 12.1+: additional widget SCXMLs (TextField, Checkbox, Toggle, Modal)
- Round 12.x: widget composition (nested widget tree)
- Round 12.x: Forge codec for 2D Transform UBO (RHI 진입 준비)
- Round 12.x: pinion-render-rhi 스켈레톤 (winit + ash device init)
- build.rs strip pattern: candidate upstream contribution to sce-build (include!() friendliness)
- unsafe_code = deny (was forbid) - acceptable trade-off for SCE generated code consumption



### Round 13 — Round 13 — project rename pinion-gui → pinion (lowercase brand, egui pattern); GitHub repo renamed via gh CLI; local filesystem mv pending

**Changes**:
- GitHub repo renamed: pinion-gui → pinion (gh repo rename); origin auto-updated to ssh
- Filesystem text sweep: Cargo.toml, LICENSE-COMMERCIAL, CLAUDE.md, COMMIT_FORMAT, mnemosyne.toml, .mcp.json, button.scxml
- Atomic store: §6 inputs + §5.16 caveat[4] direct edit; changelog publishable redact (4 entries)
- Crate names retained (pinion-core/runtime/rpc/cli) — Bevy brand-prefix pattern
- 4 [[publishable_override_ledger]] rows for R1/R2/R3/R10 (R13 reason); audit retained per R294



**Verification**:
- grep pinion-gui (case-insensitive, non-vendor, non-audit-half): 0 hits in working tree
- validate-workspace: T1=0, T2 RT=1/1, GENERATED.md=sync, divergence=7 entries / 12 ledger rows
- Pending: cargo check + Round 13 commit + filesystem mv to /home/coin/pinion



**Impact**: §1, §5.16, §6


**Carry forward**:
- Filesystem mv: /home/coin/pinion-gui → /home/coin/pinion (after Round 13 commit lands)
- MCP server restart may be needed after .mcp.json path change
- crates.io 'pinion' availability check before first publish (cargo publish point)
- Audit half retains pinion-gui per R294 design (frozen ledger preserved)



### Round 16 — Round 16 — §5.19 app.scxml convention spec'd + minimal example + pinion-core build.rs integration (first slice)

**Changes**:
- §5.19 new: app.scxml convention (file location, declaration shape, build-time discovery)
- crates/pinion-core/app.scxml: minimal single-window example (state id="main")
- crates/pinion-core/build.rs: generalized to compile [button.scxml, app.scxml] with shared strip pass
- crates/pinion-core/src/app.rs: AppState/AppEvent re-export via wrapped mod sm + include!
- crates/pinion-core/src/lib.rs: pub mod app added



**Verification**:
- validate_workspace: T1=0, T3 reject=0, GENERATED.md sync, sections 28 -> 29
- cargo check --workspace: pass
- cargo test --workspace: pass (7 button tests, 0 regressions)
- Generated app_sm.rs at OUT_DIR: AppState::Main + AppEvent::Null + AppPolicy emitted



**Impact**: §5.4, §5.17, §5.18, §5.19, §6.3


**Carry forward**:
- pinion-runtime: consume AppState/AppEvent for window routing (R16 next slice)
- pinion-rpc: §5.18 path parser short-circuit using AppState perfect-hash
- Multi-window example: parallel root + N states demonstrating WindowId enum



### Round 17 — Round 17 — R16 round close: §5.15 8-item External contract + §5.12 7/7 typed+wire RPC + topology runtime (16 slices)

**Changes**:
- Scene enum 7 variants + Style/Modifier + Event enum closed-core (§5.2 §5.13)
- External trait 8-point contract + ExternalIntrospect opt-in (§5.15 items 1-8)
- ExternalNode { handle: Box<dyn External> } wires §5.15 into Scene tree (§5.2)
- JSON-RPC 2.0 envelope + dispatch with -32700/-32600/-32601/-32602 codes (§5.7)
- 7 typed RPC methods: query/click/rewind/snapshot/dry_run/waitFor/screenshot (§5.12)
- WindowRouter + App + topology helpers; multi-window fixture (§5.17 §5.18)
- Scene drops Clone derive; non_exhaustive R14 hedge throughout new enums/structs
- serde + serde_json workspace deps added; first runtime deps beyond vendor/sce



**Verification**:
- cargo test --workspace: 110 pass (added +110 across 16 slices; 0 regressions)
- cargo clippy --workspace --all-targets: clean on every slice's new code
- validate_workspace per slice: T1=0 T3=0; sections 28 -> 29; entries 15 -> 17
- Mnemosyne mutations: 1 new section + 11 caveat appends across 8 sections



**Impact**: §5.2, §5.4, §5.7, §5.8, §5.12, §5.13, §5.15, §5.16, §5.17, §5.18, §5.19


**Carry forward**:
- tokio stdio transport: wrap dispatch in newline-delimited JSON loop (§6.3 boundary)
- Scene::Container traversal: real nested scene-tree addressing (§5.3 DSL prereq)
- R12 Button -> R14 view-fn fn(&State, &Frame) -> Scene migration
- SCE engine-level dry_run step hook to replace v0 External test-and-rollback (§5.8)
- Push-form async state channel for External (§5.15 item 7, §6.3 wiring)
- Pixel renderer: pinion-render-rhi delivery to unblock screenshot (§5.16)



### Round 18 — Round 18 — R17 round close: §5.11 v0 schema 5/5 + §5.15 widget bridging + §5.12 scene/invoke bidirectional RPC + live hello-button dogfood (8 slices)

**Changes**:
- §5.11 v0 schemas locked: BoxNode/TextNode/PathNode/ImageNode/ContainerNode 5/5 introspectable
- Rect{x,y,w,h:u32} geometry primitive added to scene; Box/Text/Path/Image carry it
- ButtonExternal + ButtonStateSnapshot: first §5.15 widget-bridging reference impls
- ExternalIntrospect.invoke trait method + InvokeError; query/intervene/invoke triad
- scene/invoke 8th method extends §5.12 7-set; pinion-rpc/src/invoke.rs typed dispatcher
- hello-button: §6.3 view-fn first live; bidirectional RPC — winit + JSON-RPC share invoke
- §2 invariant #2 (RPC headless as AI primary path) literally validated against live SCXML



**Verification**:
- cargo test --workspace: 110 -> 149 (+39 across 8 slices; 0 regressions)
- cargo clippy --workspace --all-targets: clean on every slice; pre-existing 9 untouched
- validate_workspace per slice: T1=0 T3=0 RT=1/1 GENERATED.md=sync
- Mnemosyne mutations: 8 caveat appends + 2 impl bindings across §5.11/§5.12/§5.15



**Impact**: §2, §5.2, §5.11, §5.12, §5.15, §6.3


**Carry forward**:
- cosmic-text Text rasterizer (Scene::Text currently skipped by paint)
- §5.3 DSL: Style trait fields, taffy layout, structured Path/Image schemas
- tokio stdio transport for production JSON-RPC framing/batching
- multi-window WindowRouter live dogfood (fixture-only today)
- vello/wgpu §5.16 first GPU backend; softbuffer eventual replacement
- on_click callback / intent system new spec round (Elm-style messages)
- SCE engine-level dry_run hook (replaces §5.8 v0 test-and-rollback)



### Round 19 — Round 19 — R18 round close: §5.20 intent system implementation; bidirectional event channel realized (6 slices)

**Changes**:
- pinion-derive crate added: #[derive(IntentTag)] macro for unit + scalar tuple variants (String/i64/f64/bool)
- pinion-core::intent module: Intent envelope (Cow tag + IntrospectValue payload) + IntentTag trait
- Scene tag fields: 5 introspectable variants gain Option<Cow<'static,str>> + with_tag builder per §5.20
- External::drain_intents + is_dirty defaults; CountedExternal + ButtonExternal opt in
- pinion-runtime::intent_queue: IntentQueue + walk_scene_and_drain recursive scene walker
- pinion-rpc::intents: scene/intents 9th JSON-RPC method (poll-form drain, single-consumer v0)
- ButtonExternal emits button.click intent on Pressed → Hover (PointerUp); winit + RPC share path
- hello-button live dogfood: walk_scene_and_drain after each event; intents log to stderr



**Verification**:
- cargo test --workspace: 149 → 194 (+45 across 6 slices; 0 regressions)
- cargo clippy --workspace --all-targets: clean on every slice; pre-existing 12 warnings untouched
- validate_workspace per slice: T1=0 T3=0 RT=1/1 GENERATED.md=sync throughout
- Mnemosyne mutations: 7 impl bindings + 5 caveats across §5.12 + §5.20; entries unchanged at 18



**Impact**: §2, §5.2, §5.11, §5.12, §5.15, §5.20, §6.3


**Carry forward**:
- async stream intent channel (sync poll v0 today)
- multi-consumer subscribe (single-consumer v0)
- interactivity layer separation (intents currently coupled to External drain)
- IntrospectValue::Object/Array expansion → struct + multi-field tuple variants in derive
- complex payload macro for compound (multi-field) intent variants
- cosmic-text Text rasterizer (Scene::Text still skipped by paint)
- §5.3 DSL: Style trait fields, taffy layout, structured Path/Image schemas
- vello/wgpu §5.16 first GPU backend; softbuffer eventual replacement
- tokio stdio transport for production JSON-RPC framing/batching
- multi-window WindowRouter live dogfood (fixture-only today)



### Round 2 — Round 2 open axes decomposition: §5 + §5.1-§5.10 enumerate options, trade-offs, deps; §5.2/§5.4 slots inferred from §2 invariants

**Changes**:
- §5 parent: open implementation axes container with rationale/inputs/outputs/caveats
- §5.1 strategic kickoff: framework-first vs dogfood-slice-first; gates §5.3 and §5.6
- §5.2 scene primitive type set: closed-form vs extensible (slot inferred from §2)
- §5.3 DSL form: file-based vs Rust macro vs view-fn literal; depends on §5.1
- §5.4 SCE engine embedding: Forge-emit vs FFI to C11 vs sce-rust crate (slot inferred)
- §5.5 MCU v1 backend: AP-only first cut recommended; MCU-included v1 rejected
- §5.6 reuse path: cascade-emit from day 1 vs Rust-native MVP then port
- §5.7 RPC headless protocol: MCP-native vs JSON-RPC vs gRPC; REST rejected
- §5.8 dry_run hook site: SCE engine-level vs scene snapshot vs view-fn rewind
- §5.9 GUI/TUI renderer split: trait-based vs separate pipelines
- §5.10 mode toggle API: runtime flag vs build feature vs per-view annotation
- Cross-ref graph wired via impact_scope across §1-§4 and §5.X



**Verification**:
- 11 add_section calls (§5 + §5.1-§5.10) executed without T1 reject
- Atomic store grew monotonically from 8357 bytes to ~25K bytes
- Each axis populated with intent/rationale/inputs/outputs/caveats
- 5 axes (§5.1 §5.2 §5.3 §5.5 §5.7) carry alternatives_rejected
- 11 impact_scope cross-refs set via typed primitive (T1 pre-write)
- Pending: validate_workspace re-run + verify_generated GENERATED.md sync



**Impact**: §1, §2, §3, §4, §5, §5.1, §5.2, §5.3, §5.4, §5.5, §5.6, §5.7, §5.8, §5.9, §5.10


**Carry forward**:
- Axis §5.1 decision: framework-first vs dogfood-slice-first; gates §5.3 and §5.6
- Axis §5.5 decision: AP-only v1 recommended; ratify or revise in Round 3
- Axes §5.2 §5.4 inferred slots: confirm or rename in Round 3 ratify pass
- Axes §5.7-§5.10 follow §2 invariants implementation: protocol/hook-site/split/binding
- CLAUDE.md authoring for pinion (SSOT contract + auto-kickoff trigger)
- Round 2 git commit (§5 atomic decomposition + settings.local.json bypass mode)
- Bash(*) and bypassPermissions added to .claude/settings.local.json this session



### Round 20 — Round 20 — §5.3 DSL v0 concrete schema lock: Color, BoxStyle, TextStyle, PathCommand, PathStyle, ImageStyle, Modifier (taffy deferred)

**Changes**:
- Color {r,g,b,a:u8} typed; ARGB u32 compat via from_argb/to_argb (replaces raw u32 fill)
- BoxStyle {fill, border, corner_radius}; Border {color, width:u32}
- TextStyle {font_family, font_size_px, fg_color} — unlocks cosmic-text rasterizer slice
- PathCommand enum {MoveTo, LineTo, CurveTo, Close}; PathNode.data: String → Vec<PathCommand>
- PathStyle {stroke, fill}; Stroke {color, width, cap:StrokeCap}
- ImageStyle {fit, tint}; Fit enum {Fill, Contain, Cover, Tile}
- Modifier {margin, padding, align}; Align 9-pos (corners/edges/center)
- 8 caveats on §5.3 + 1 cross-ref caveat on §5.11 capturing BoxNode/PathNode field shape evolution



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; entries 19 → 20
- atomic mutations: 9 add_section_caveat (8 on §5.3 + 1 on §5.11); 5 initial drafts rejected by 100-char cap, retried tightened
- no code changes this round (spec-only); R21 implementation follows
- section count unchanged at 30; §5.3 caveats grow from 0 → 8 capturing the v0 schema decisions



**Impact**: §5.2, §5.3, §5.11


**Carry forward**:
- taffy flexbox/grid integration (next spec round) — explicitly deferred from R20
- R21 implementation: typed Color in Scene; cosmic-text rasterizer; PathCommand/Fit/Style structs
- Modifier full layout model (offsets+alignment+constraints) — v0 covers margin/padding/align only
- Bold/italic/underline text style fields (font_weight/style) deferred; cosmic-text supports them
- Gradient + shadow fills on BoxStyle deferred
- StrokeCap variants beyond Butt/Round/Square (e.g. Square+offset, dash patterns) deferred
- Image codec/loader policy + decoded buffer cache strategy deferred
- BoxNode.fill u32 → Color migration affects 5+ test sites and hello-button view



### Round 21 — Round 21 — R20 round close: §5.3 v0 schema implementation + cosmic-text Text rasterizer (7 slices)

**Changes**:
- pinion-core::style module: Color, Border, BoxStyle, TextStyle, Stroke, StrokeCap, PathStyle, Fit, ImageStyle, Align
- BoxNode refactored: {rect, style:BoxStyle, tag}; BoxNode::filled(rect,color) shorthand; 21 call sites migrated
- TextNode gains style:TextStyle; new() defaults, styled() explicit
- PathNode rewritten: data:String → commands:Vec<PathCommand>; PathPoint(f32) sub-pixel space
- ImageNode gains style:ImageStyle; Fit policy {Fill,Contain,Cover,Tile}; optional tint
- Modifier expanded {margin, padding, align}; Align 9-pos enum (TopLeft default)
- hello-button paints Scene::Text via cosmic-text 0.12 (FontSystem/SwashCache held in App, source-over blend)
- cosmic-text dep limited to examples/hello-button — pinion-core stays free of rasterizer dependencies



**Verification**:
- cargo test --workspace: 194 → 220 (+26 across 7 slices; 0 regressions)
- cargo clippy --workspace --all-targets: clean on every slice; pre-existing 12 warnings untouched
- validate_workspace per slice: T1=0 T3=0 RT=1/1 GENERATED.md=sync throughout
- Mnemosyne mutations: 11 impl bindings on §5.3 (Color/BoxStyle/Border/TextStyle/PathStyle/PathCommand/ImageStyle/Align/Modifier/paint_text)



**Impact**: §5.2, §5.3, §5.11


**Carry forward**:
- taffy flexbox/grid integration spec round (still deferred from R20)
- Bold/italic/underline TextStyle fields (cosmic-text supports them; v0 covers font/size/color only)
- Gradient + shadow fills on BoxStyle deferred
- Path/Image actual painting (Path — vello/lyon rasterizer slice; Image — codec + cache)
- Modifier integration into paint (margin/padding/align affect Box and Container placement)
- cosmic-text font cache invalidation across DPI / scale changes
- Sub-pixel rect coordinates (currently Rect is u32; PathPoint already f32)
- EdgeInsets type if Rect-as-insets proves awkward in real layout code



### Round 22 — Round 22 — §5.20 R18 polish: ExternalNode tag prefixes drained intent tag (widget.kind convention complete)

**Changes**:
- ExternalNode gains tag: Option<Cow<'static,str>> + with_tag builder (mirrors the 5 introspectable variants)
- walk_scene_and_drain prefixes drained intent.tag with <scene-tag>.<intent-tag> when ExternalNode.tag is present
- ButtonExternal emits bare "click" (was "button.click"); widget identity decoupled from UI naming
- hello-button tags its ButtonExternal as "main_btn" — stderr log shows main_btn.click on full click cycle
- R22 caveat on §5.20: widget.kind tag convention fully wired (was "hardcoded" prefix before)



**Verification**:
- cargo test --workspace: 220 → 223 (+3 prefix-walk tests; 2 button tests updated for new "click" kind)
- cargo clippy --workspace --all-targets: clean; pre-existing 12 warnings untouched
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; entries 21 → 22
- Mnemosyne mutations: 1 impl binding (ExternalNode) + 1 caveat on §5.20



**Impact**: §5.20


**Carry forward**:
- Container.tag propagation to nested External (today only direct ExternalNode.tag prefixes)
- Tag concatenation policy for nested prefix chains (e.g. panel.toolbar.save_btn.click) — currently single-level
- IntrospectValue::Object payload — enables structured intent data beyond scalar kinds
- Async stream intent channel (sync poll v0 still in place)



### Round 23 — Round 23 — §5.21 new section: taffy auto-layout (flex v0) spec lock; auto-layout supersedes manual Rect

**Changes**:
- New §5.21 section: Layout system (taffy auto-layout, flex v0) under §5 parent
- Intent: auto-layout via taffy; ContainerNode + every leaf node carries LayoutStyle sidecar
- Rationale (6): real apps need flex; taffy is canonical Rust engine; Modifier R21 maps 1:1; pure pass per frame
- Inputs/outputs/alternatives populated; rejects manual Rect, custom engine, morphorm, grid in v0
- 5 §5.21 caveats: LayoutStyle wraps taffy::Style; 5 introspectable variants gain field; compute_layout entry
- §5.3 cross-ref caveat: Modifier margin/padding consumed by R23 layout pass; align retained as anchor



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 30 → 31; entries 22 → 23
- no code changes this round (spec only); R24 implementation follows
- atomic mutations: 1 add_section + 5 set_section_* + 6 add_section_caveat; all pass T3 cap
- spec round pattern mirrors R20 (§5.3 spec) + R21 (§5.3 impl) cadence



**Impact**: §5.2, §5.3, §5.11, §5.21, §6.3


**Carry forward**:
- R24: taffy dep in pinion-runtime; LayoutStyle wrapper + Scene sidecar fields; compute_layout pass
- R24: Modifier margin/padding → taffy mapping; hello-button auto-layout migration
- Grid display variant in a later spec round (flex must prove out first)
- Transform Modifier (translate/rotate/scale) future axis — not covered by taffy
- Layout cache invalidation strategy (per-frame vs change-detection) deferred
- Text auto-sizing within taffy nodes — cosmic-text shape feeds intrinsic size hint



### Round 24 — Round 24 — R23 round close: §5.21 taffy auto-layout flex v0 implementation (5 slices)

**Changes**:
- pinion-core::style: LayoutStyle + Display/FlexDirection/JustifyContent/AlignItems/Size/SizeValue types
- Scene 7 variants (Box/Text/Path/Image/Container/External + Effect) gain layout: LayoutStyle sidecar
- Container/External also gain rect: Rect for layout-pass output target (was Box-only before)
- pinion-runtime depends on taffy 0.7; layout.rs translates LayoutStyle → taffy::Style + walk
- LayoutStyle absorbs padding/margin (Modifier role unified — one sidecar drives the layout pass)
- ContainerNode gains style: BoxStyle (containers double as visually-styled "div" for backgrounds)
- hello-button BTN_RECT hardcode gone: root flex centers 160x80 button container
- compute_layout(scene, viewport_w, viewport_h) called from render() each frame; pure / cache-free



**Verification**:
- cargo test --workspace: 223 → 235 (+12 across 5 slices; 0 regressions including 5 new layout tests)
- cargo clippy --workspace --all-targets: clean on every slice; pre-existing 12 warnings untouched
- validate_workspace per slice: T1=0 T3=0 RT=1/1 GENERATED.md=sync throughout
- Mnemosyne mutations: 3 impl bindings on §5.21 (LayoutStyle/compute_layout/hello-button view)



**Impact**: §5.2, §5.3, §5.11, §5.21, §6.3


**Carry forward**:
- Grid display variant + transforms (translate/rotate/scale) — next layout spec round
- Position::Absolute support — useful for overlays, tooltips, modal dialogs
- Layout cache invalidation strategy (per-frame recompute today; change-detection later)
- Text auto-sizing via cosmic-text shape feeding taffy intrinsic measure callback
- BorderStyle full implementation (currently fill only — corner_radius and border unread in paint)
- EdgeInsets type if Rect-as-insets proves awkward in real layout code
- Modifier struct removal (its fields now in LayoutStyle; struct is vestigial)
- Sub-pixel rect coordinates (currently u32; taffy compute uses f32 but truncates back to u32)



### Round 25 — Round 25 — §2 invariant expansion: #5 reworded (statechart → SCE-managed state); #8 ratified (SCE meta = AI authoring surface)

**Changes**:
- §2 invariant #5 wording: "SCE statechart state" → "SCE-managed state" — SCE not statechart-only
- §2 invariant #8 ratified: "SCE meta = AI authoring surface" (write-side counterpart to RPC headless)
- §2 rationale bullet #3 reworded: RPC = AI read path, SCE meta = AI write path, together = AI 1st-class
- §2 rationale bullet #6 reworded: SCE statechart kind has byte-golden parity; other kinds extend matrix
- §2 rationale bullet #8 added: SCE-managed state spans statechart + signal/computed/resource + view-fn
- Unlocks R25+ multi-axis spec rounds (Signal/Effect/Semantic/Modifier/etc.) as SCE schema + Forge codegen + Rust runtime triples



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; entries 24 → 25; sections unchanged at 31
- no code changes this round (invariant-only); pinion-core/runtime/rpc/derive untouched
- atomic mutations: set_section_intent + set_section_rationale (replace) + add_section_caveat
- Effect on existing axes: §5.20 tag stays valid (subset of §5.25 semantic tree); §5.21 taffy stays valid



**Impact**: §2


**Carry forward**:
- R26 §5.22: Reactive primitives — Signal/Computed/Resource SCE schema + Forge codegen + Rust runtime
- R27 §5.23: Effect model — Command/handler SCE schema
- R28 §5.25: Semantic tree — role/state/actions SCE schema (§5.20 tag absorbed)
- R29 §5.26: Modifier chain — composition SCE schema
- R30 §5.27: Incremental layout + damage tracking (Rust-side primarily)
- R31 §5.16: GPU render backend (vello)
- R32 §5.28: Virtualization Scene variant
- R33 §5.29: Animation (spring physics) SCE schema
- R34 §5.30: Structured concurrency
- R35 §5.31: Accessibility (AccessKit bridge)
- R36 §5.32: Hot reload (signal serialization protocol)



### Round 26 — Round 26 — §5.22 new section: Reactive primitives (Signal/Computed/Resource) spec lock; SCE schema + Forge codegen + Rust runtime triple

**Changes**:
- New §5.22 section: Reactive primitives (Signal / Computed / Resource) under §5 parent
- Intent: fine-grained reactive state; SCE meta declares signal graph; Forge generates Rust runtime
- Rationale (8): view-fn purity, RPC introspection, dry_run/rewind, Solid/Vue3 consensus; Hooks/Snapshot/Iced rejected
- Inputs/outputs/alternatives populated; SCE schema design pattern established for R27+ axis batch
- 13 §5.22 caveats: Signal API + Computed lazy + Resource 3-state + Owner tree + Batching + push-pull propagation + SCE nesting + Forge codegen + RPC introspect + dry_run snapshot + Intent dispatch + view-fn read-only + concurrency v0
- Textbook canonical: signal-based fine-grained reactivity (Solid/Vue3/SwiftUI 2020s+ consensus)



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 31 → 32; entries 25 → 26
- no code changes this round (spec-only); R27 implementation follows in 3-tier SCE/Forge/Rust pattern
- atomic mutations: 1 add_section + 5 set_section_* + 13 add_section_caveat; 2 retries for 100-char cap
- Spec round pattern mirrors R20 (§5.3) + R23 (§5.21) cadence



**Impact**: §2, §5.3, §5.7, §5.12, §5.20, §5.22


**Carry forward**:
- R27 §5.22 impl: Signal<T> + Computed<T> + Resource<T,E> runtime (pinion-core::reactive)
- R27 §5.22 impl: Owner / scope hierarchical tree (thread-local v0)
- R27 §5.22 impl: SCE schema authoring + Forge codegen pipeline
- SyncSignal cross-thread variant — v0 single-threaded; carry-forward to concurrency round
- Effect (reactive scope) primitive — sibling of Computed; R28 §5.23 candidate
- Topological sort + glitch-free propagation algorithm — standard MobX/Solid pattern
- Signal<T> equality opt-out (skip_eq) for expensive types — future ergonomic



### Round 27 — Round 27 — §5.23 new section: Effect model (Effect / Command / Handler) spec lock; two-layer effect system separates reactive scope from declarative async

**Changes**:
- New §5.23 section: Effect model under §5 parent (Effect / Command<Intent> / Handler trio)
- Intent: Effect = reactive scope; Command = declarative async/IO; Handler = dispatch impl
- Rationale (8): Solid Effect + Iced Command + Roc Handler synthesis; React useEffect rejected as conflation
- Update fn signature finalized: Update(&mut Model, Intent) -> Vec<Command<Intent>>
- scene/commands ratified as 10th RPC method (§5.12 caveat cross-ref); dry_run-aware inspection
- 11 §5.23 caveats: lazy Effect register + dry_run skip + Serialize Command + Handler trait + cancellation + Update sig + SCE schema + Forge codegen + scene/commands + view-fn no-write + Owner topological order
- Textbook canonical: clean separation of reactive subscription (Effect) vs side-effect description (Command)



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 32 → 33; entries 26 → 27
- no code changes this round (spec-only); R27+ axis batch continues in 3-tier SCE/Forge/Rust pattern
- atomic mutations: 1 add_section + 5 set_section_* + 12 add_section_caveat (1 retry for 200-char intent cap)
- Spec round pattern mirrors R20/R23/R26 (§5.X new section ratify) cadence



**Impact**: §2, §5.3, §5.12, §5.20, §5.22, §5.23


**Carry forward**:
- R28 §5.24 Semantic tree (role/state/actions) — absorbs §5.20 tag into richer schema
- R29 §5.25 Modifier composition (chain pattern) — Signal-reactive modifiers
- R30 §5.26 Incremental layout + damage tracking — refines §5.21 taffy
- R31 §5.16 GPU render backend (vello) — existing axis ratify
- R32 §5.27 Virtualization Scene variant (VirtualList<T>)
- R33 §5.28 Animation (spring physics) — Signal<f32> + Effect substrate
- R34 §5.29 Structured concurrency — SyncSignal cross-thread + Owner scope
- R35 §5.30 Accessibility (AccessKit bridge) — depends on §5.24 semantic tree
- R36 §5.31 Hot reload — signal serialization protocol from §5.22
- Effect equality opt-out, granular Handler retry policy, Command priority queueing — future ergonomics



### Round 28 — Round 28 — §5.24 new section: Semantic tree (role/state/actions) spec lock; absorbs §5.20 tag into richer ARIA-aligned schema

**Changes**:
- New §5.24 section: Semantic tree under §5 parent (role+state+actions triple)
- SemanticProps sidecar on every Scene node: role + state + actions + label + description + tag
- Role enum closed-form per ARIA: ~30 variants v0 (Button/Heading/List/TextInput/etc.)
- SemanticState bitflags + SemanticAction enum for AT + AI introspection alike
- scene/semantic = 11th RPC method ratified (§5.12 caveat cross-ref)
- §5.20 tag absorbed into SemanticProps.tag (caveat cross-ref); semantic tree richer surface
- 10 §5.24 caveats lock concrete schema; AccessKit bridge derivation path established



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 33 → 34; entries 27 → 28
- no code changes this round (spec-only); R29+ axis batch continues
- atomic mutations: 1 add_section + 5 set_section_* + 12 add_section_caveat (3 retries for caps; 1 orphan fix)
- Single semantic surface drives accessibility AND AI introspection — no two-tree sync bugs



**Impact**: §2, §5.12, §5.20, §5.22, §5.23, §5.24


**Carry forward**:
- R29 §5.25 Modifier composition (chain pattern) — Signal-reactive modifiers
- R30 §5.26 Incremental layout + damage tracking — refines §5.21 taffy
- R31 §5.16 GPU render backend (vello) — existing axis ratify
- R32 §5.27 Virtualization Scene variant (VirtualList<T>)
- R33 §5.28 Animation (spring physics)
- R34 §5.29 Structured concurrency
- R35 §5.30 Accessibility (AccessKit bridge from §5.24)
- R36 §5.31 Hot reload (signal serialization)



### Round 3 — Round 3 — 9 axes ratified to single options; §5.2 §5.4 slot names confirmed; framework-first kickoff path locked

**Changes**:
- §5.1 ratified: framework-first kickoff; common substrate before first widget
- §5.2 ratified: closed-form primitive type set (Box/Text/Path/Image/Container/Effect/External)
- §5.3 ratified: view function literal (Xilem-style); plain Rust functions, no separate DSL
- §5.4 ratified: embed SCE Forge Rust emit via vendor/sce submodule
- §5.5 ratified: AP-only v1; MCU deferred to v2+
- §5.6 ratified: Rust-native MVP first; cascade-emit layer after canonical kind settles
- §5.7 ratified: JSON-RPC 2.0 transport; MCP wraps on top
- §5.8 ratified: SCE engine-level hook for dry_run
- §5.9 ratified: trait-based Renderer abstraction (one scene → GUI/TUI dispatch)
- §5.10 ratified: runtime flag for immediate vs retained mode toggle
- §5.2 §5.4 slot names confirmed (Round 2 inferences ratified, not renamed)



**Verification**:
- 10 set_section_intent + 9 set_section_alternatives mutations via typed primitive
- Atomic store grew monotonically; T1 pre-write passed on all 19 calls
- Each axis intent now reads 'Decision: X; ratified Round 3'
- Pending: validate_workspace + verify_generated post-Round 3 sync confirmation



**Impact**: §5.1, §5.2, §5.3, §5.4, §5.5, §5.6, §5.7, §5.8, §5.9, §5.10


**Carry forward**:
- CLAUDE.md authoring for pinion (SSOT contract + auto-kickoff trigger)
- Round 3 git commit covering §5 decisions + settings.local.json defaultMode=auto
- Initial Cargo workspace layout per framework-first §5.1 decision
- vendor/sce embed wiring per §5.4 (Rust emit subset, no_std-aware later)
- Scene primitive type set Rust enum sketch per §5.2 closed-form decision
- Author-facing view function API sketch per §5.3 decision
- JSON-RPC 2.0 schema draft per §5.7 decision (query/click/dry_run/snapshot/rewind/waitFor)



### Round 4 — Round 4 — Tier 1 bootstrap auto-ratified (§6 workspace/toolchain/async); 4 new open axes §5.11-§5.14 enumerated for next round decision

**Changes**:
- §6 parent: Tier 1 implementation choices auto-ratified group (ceremonial bloat avoidance)
- §6.1 ratified: Cargo workspace, 4 initial crates (pinion-core/runtime/rpc/cli)
- §6.2 ratified: stable Rust, MSRV 1.85.0, edition 2024
- §6.3 ratified: view-fn sync (purity), RPC and IO async via tokio
- §5.11 axis added: scene primitive variant shape (minimal vs CSS-rich vs layered)
- §5.12 axis added: RPC method shape (generic-query vs typed-per-action vs hybrid)
- §5.13 axis added: event model (closed enum vs open registry vs core+opaque) + coord system
- §5.14 axis added: state containment topology (single root vs per-widget vs hierarchical)
- Cross-ref graph extended: §6 → §5.{1,4,5,6,7,10}; §5.11-14 → prior §5.X deps



**Verification**:
- 8 new sections added (§6 + §6.1-§6.3 + §5.11-§5.14)
- §6.1-§6.3 carry alternatives_rejected for ratified choices (audit-traceable)
- §5.11-§5.14 carry option sets in inputs (still open, Round 5+ decision)
- Tier classification preserved: §6.X auto-ratified, §5.11-14 axis-worthy
- Pending: validate_workspace + verify_generated post-Round 4 sync



**Impact**: §2, §5.1, §5.2, §5.3, §5.4, §5.6, §5.7, §5.8, §5.10, §6, §6.1, §6.2, §6.3, §5.11, §5.12, §5.13, §5.14


**Carry forward**:
- §5.11 decision (Round 5): scene primitive variant shape — layered likely
- §5.12 decision (Round 5): RPC method shape — hybrid likely
- §5.13 decision (Round 5): event model + coordinate system (logical DPI-aware)
- §5.14 decision (Round 5): state containment topology — hierarchical likely
- Implementation Round 6: Cargo workspace skeleton commit per §6.1
- Implementation Round 6: rust-toolchain.toml + workspace Cargo.toml per §6.2
- CLAUDE.md authoring still outstanding (Round 3 carry-forward unresolved)
- Tier 2 axes inventory if needed (AccessKit, i18n, animation, hot reload, diagnostics)



### Round 5 — Round 5 — 4 axes ratified (§5.11-§5.14): layered primitives, hybrid RPC, core+opaque events, hierarchical SCE topology

**Changes**:
- §5.11 ratified: layered primitive shape (core variant + Style trait + Modifier composition)
- §5.12 ratified: hybrid RPC (6 typed top-level methods + path/filter sub-args)
- §5.13 ratified: closed core Event + opaque External event; logical DPI-aware coords
- §5.14 ratified: hierarchical SCE topology (root + scoped child SCEs)
- All 4 decisions align with Option C pattern: closed-form + opaque escape + Xilem/SCE hierarchical



**Verification**:
- 4 set_section_intent + 4 set_section_alternatives mutations via typed primitive
- T1 pre-write passed on all 8 calls
- Each axis intent now reads 'Decision: X; ratified Round 5'
- Pending: validate_workspace + verify_generated post-Round 5



**Impact**: §2, §5.2, §5.3, §5.4, §5.7, §5.8, §5.11, §5.12, §5.13, §5.14


**Carry forward**:
- Spec phase complete: all §5.X axes ratified (§5.1-§5.10 Round 3, §5.11-§5.14 Round 5)
- Implementation Round 6: Cargo workspace skeleton per §6.1 (pinion-core/runtime/rpc/cli)
- Implementation Round 6: rust-toolchain.toml + workspace Cargo.toml per §6.2
- Implementation Round 6+: scene primitive enum + Style trait per §5.2 §5.11
- Implementation Round 6+: JSON-RPC server skeleton per §5.7 §5.12
- Implementation Round 6+: Event enum + External opaque per §5.13
- Implementation Round 6+: SCE hierarchical embedding per §5.4 §5.14
- CLAUDE.md authoring (multi-round carry-over from Round 1 §1)
- First dogfood sequencing (§4) — after framework MVP shape
- Tier 2 axes inventory check (AccessKit, i18n, animation, hot reload, diagnostics)



### Round 6 — Round 6 — Cargo workspace skeleton realized: 4 crates (pinion-core/runtime/rpc/cli), Rust 1.85.0 stable, edition 2024; cargo check green

**Changes**:
- Cargo.toml workspace root: resolver=3, 4 members, workspace.package shared
- rust-toolchain.toml: stable channel 1.85.0 + rustfmt + clippy
- crates/pinion-core: scene/style/view types stub (lib.rs empty for now)
- crates/pinion-runtime: render+SCE state stub; depends on pinion-core
- crates/pinion-rpc: JSON-RPC server stub; depends on pinion-core
- crates/pinion-cli: binary entry; depends on all three lib crates
- workspace.lints: unsafe_code=forbid, clippy::pedantic=warn



**Verification**:
- cargo check --workspace: Finished dev profile in 0.40s with no warnings
- All 4 crates compile from empty source (skeleton baseline)
- Cargo 1.94.1 / rustc 1.94.1 confirmed for edition 2024 + resolver 3



**Impact**: §6.1, §6.2, §6.3


**Carry forward**:
- Round 7+: scene primitive enum + Style trait + Modifier per §5.2 §5.11 in pinion-core
- Round 7+: Event enum + External opaque per §5.13 in pinion-core
- Round 7+: view-fn signature types per §5.3 in pinion-core
- Round 7+: SCE hierarchical embedding per §5.4 §5.14 in pinion-runtime
- Round 7+: JSON-RPC server skeleton with 6 typed methods per §5.7 §5.12 in pinion-rpc
- Round 7+: tokio dep + async boundary per §6.3 (pinion-rpc only)
- Round 6 git commit: workspace skeleton + Round 6 atomic update
- CLAUDE.md still outstanding



### Round 7 — Round 7 — §5.15 External primitive integration contract (8 items) ratified; §5.12 extended with 7th RPC method screenshot for pixel verification

**Changes**:
- §5.15 new: External primitive integration contract (8 items)
- §5.15.1 backend support declaration (Gui/Tui/Rpc dispatch with fallback)
- §5.15.2 repaint trigger ownership (framework vs External own loop)
- §5.15.3 thread ownership (UI thread vs own thread + sync channel)
- §5.15.4 lifecycle event callbacks (mount/unmount/visibility/focus)
- §5.15.5 input forwarding policy (split between framework and External)
- §5.15.6 DPI/scale change + resize notification
- §5.15.7 async state change channel (External → framework push)
- §5.15.8 optional symbolic introspection (schema + query/intervene; opt-in)
- §5.12 extended: 7th RPC method screenshot for pixel-level verification
- Game viewport pattern emerges naturally without §1 vision change



**Verification**:
- 1 add_section + 8 set/add mutations for §5.15
- 3 set_section_* updates for §5.12 (intent/rationale/outputs)
- T1 pre-write passed on all calls; round-trip preserved
- Pending: validate_workspace + verify_generated post-Round 7



**Impact**: §3, §5.9, §5.10, §5.12, §5.14, §5.15


**Carry forward**:
- §1 vision update consideration: extend to 'introspection protocol for interactive apps' (Round 8+)
- External author example widgets (game / video / PDF) as reference impl (Round 8+)
- Tier 2 streaming axes: screenshot video, partial repaint subscription (Round 8+)
- Round 6 + Round 7 git commit covering workspace skeleton and External contract
- CLAUDE.md authoring (long-standing carry-over)
- pinion-core: External primitive type definition per §5.15 (Round 8+ impl)



### Round 8 — Round 8 — spec phase closing artifacts: CLAUDE.md AI-agent guide authored; git author scoped to repo-local override

**Changes**:
- CLAUDE.md authored: quick-start order, invariants, audit-trail summary, repo map, working contract
- CLAUDE.md resolves Round 3-7 long-standing carry-forward (AI agent operational guide)
- git config --local user.name newmassrael / user.email newmassrael@gmail.com (repo-scoped)
- Round 1 initial scaffold commit amended with --reset-author (SHA: 320fdb0 → eded7e0)
- Pre-commit hook verified active during amend (mnemosyne verify-generated passed)



**Verification**:
- CLAUDE.md follows reading-order pattern; references §2 §3 §5.15 invariants explicitly
- git log shows single commit by newmassrael <newmassrael@gmail.com>
- git config --local scoped; global config untouched per Round 8 audit pass
- Pre-commit hook executed mnemosyne validate (T1=0 T2=1/1 GENERATED.md=sync) during amend
- Pending: validate_workspace + verify_generated post-Round 8



**Impact**: §1, §2, §3, §5.15, §6.1, §6.2, §6.3


**Carry forward**:
- Round 9+: pinion-core scene primitive enum + Style trait + Modifier impl per §5.2 §5.11
- Round 9+: Event enum + External opaque per §5.13 in pinion-core
- Round 9+: JSON-RPC server skeleton + 7 typed methods per §5.7 §5.12 in pinion-rpc
- Round 9+: SCE hierarchical embedding per §5.4 §5.14 in pinion-runtime
- Round 9+: Tier 2 streaming axes (video screenshot, partial repaint subscription)
- Round 9+: External example widget (game viewport reference impl per §5.15)
- Round 6 + 7 + 8 git commit covering workspace skeleton + External contract + CLAUDE.md
- First dogfood sequencing per §4



### Round 9 — Round 9 — third-party identifier privacy redaction via mnemosyne R297 redact-term; publishable surface scrubbed; audit half retained per R294 design

**Changes**:
- §4 Section title/intent/inputs/outputs/caveats abstracted to generic 'first dogfood' requirements
- §1 §5.1 §5.6 §5.11 Section bullets abstracted (set_section_inputs, set_section_alternatives)
- §5.1 title retitled (Strategic kickoff direction (framework-first vs dogfood-slice-first))
- §4 §5.11 caveats edited directly in atomic JSON (no set_section_caveats primitive in 3ff92f3)
- Changelog publishable half redacted: Round 1/2/5/8 via 5 redact-term passes
- 6 [[publishable_override_ledger]] rows added to mnemosyne.toml with SHA256 anchors
- CLAUDE.md lines 55, 124 edited (human-facing artifact, direct edit)
- Audit half (Round 1/2/5/8 audit_*) retained per R294 design — frozen body preserved



**Verification**:
- mnemosyne-cli rebuilt from HEAD 3ff92f3 (76581f68 → 3ff92f32 binary swap)
- validate-workspace: T1=0 / T2 round-trip=1/1 / GENERATED.md=sync
- publishable / audit divergence: entries=4 ledger_rows=6 (all SHA256 match)
- GENERATED.md grep for third-party identifier patterns: 0 hits
- CLAUDE.md grep: 0 hits
- mnemosyne.toml grep: 0 hits (history comment cleaned)
- watching-zenoh / Zenoh protocol retained per separate ecosystem reference (not in scope)



**Impact**: §1, §4, §5.1, §5.6, §5.11


**Carry forward**:
- Section caveats setter primitive (set_section_caveats) not in mnemosyne 3ff92f3 — RFC follow-up
- Audit half retains identifiers per R294 design; atomic JSON inspection intentional surface
- watching-zenoh / Zenoh protocol retained (not in this scope); broader redaction would need new round
- Round 9 git commit (audit-traceable diff with redaction history)
- pinion-core implementation phase (carry-over from Round 7+)
- RFC follow-ups for mnemosyne: set_section_caveats, content-hash clarity for multi-step redact



### round-14 — Round 14: forward-compatibility hedges for future game-engine evolution path (§5.2 §5.13 §6.3 §5.8)

**Changes**:
- §5.2 caveat: scene enum #[non_exhaustive] + RPC open-set discriminant for SemVer minor variant addition
- §5.13 caveat: Event #[non_exhaustive] + per-variant CoordSpace for Gamepad/HID/Pointer3D future slots
- §6.3 outputs: view-fn signature changed to fn(&State, &Frame) -> Scene; Frame ZST in v1.0
- §6.3 caveat: Frame ZST guarantee (LLVM ABI elision, runtime zero-cost) + read-only purity constraint
- §5.8 caveat: dry_run scope bounded to scene + SCE state; non-SCE sim excluded from guarantee



**Verification**:
- Each hedge runtime cost = 0 (non_exhaustive match identical, Frame ZST elided by LLVM, doc-only for §5.8)
- Ergonomic tax limited to downstream _ => arms and Frame param slot in source
- Game-engine evolution (Mesh/Camera/Light/Gamepad/dt) addable via SemVer minor; no v2 major required
- §2 dry_run purity invariant preserved: Frame read-only, §5.8 scope explicit



**Impact**: §5.2, §5.8, §5.13, §6.3


**Carry forward**:
- R12 Button widget reuse: migrate to fn(&State, &Frame) -> Scene signature when next widget lands
- pinion-core: define Frame struct as #[non_exhaustive] empty ZST with new() constructor
- RPC schema doc: explicit open-set kind handling guidance for clients
- §5.16 thin RHI: no spec change needed; game-engine evolution still gated on §1 future round



### round-15 — Round 15: SCE-driven window topology (§5.17 §5.18 new + §5.4 §5.7 §5.10 §5.13 §5.14 caveats)

**Changes**:
- §5.17 new: app.scxml declares window topology; SCE Forge emits WindowId/routing/lifecycle
- §5.18 new: RPC path optional /window[id]/ prefix; SCE-emit perfect-hash dispatch
- §5.4 caveats: SCE Forge role expanded to app-level codegen backbone beyond widget statecharts
- §5.7 §5.10 §5.13 §5.14 caveats: window prefix, per-window mode, WindowEvent slot, SCE root scope
- Single vs multi-window auto-branches from SCXML state count; no cargo feature flag needed



**Verification**:
- Zero-cost single-window: SCXML 1 window state -> minimal emit; no Application/registry code
- Zero-cost multi-window: build-time perfect-hash routing; no runtime HashMap lookup
- Forward-compat: R14 non_exhaustive Event enum absorbs WindowEvent variants without v2 bump
- Hierarchical SCE topology (§5.14) preserved: windows as <parallel> children of app.scxml root
- winit multi-window dep cost honestly acknowledged as the one non-eliminable runtime overhead



**Impact**: §5.4, §5.7, §5.10, §5.13, §5.14, §5.17, §5.18


**Carry forward**:
- app.scxml convention spec: example template + SCE Forge build.rs integration path
- pinion-render-core: swapchain-per-window in §5.16 thin RHI implementation
- RPC server skeleton (§5.7): path parser short-circuit on absent /window[id]/ prefix
- Dock primitive (§5.2 DockArea Container variant) deferred until layout system lands
- MCU evolution path (§5.5) preserved: const tables + no allocator dep



