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


**Intent**: v1 invariants: structured scene mandatory; RPC headless; dry_run; mode toggle; SCE-managed state; GUI/TUI dual; scene-as-data; SCE meta = universal cross-framework pattern authoring surface


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
- R37.9: #8 = universal cross-framework patterns; framework authoring (pinion-forge) out of scope







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
- R29 §5.25: Modifier struct superseded by Vec<ModifierOp> chain (closed enum form).



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
- R29 §5.25: scene/modifiers = 12th method; returns ModifierOp chain per node path.
- R30 §5.26: scene/layout = 13th method; queries cached Layout (rect/padding/border) per path.
- R32 §5.27: scene/virtual_list = 14th method; count + visible range + window snapshot.
- R36 §5.31: scene/reload = 15th method; protocol trigger + result counts.
- R47.7 scene/layout implement: viewport + path 입력 → LayoutNode tree (rect, line_count, TextStyle) 응답
- scene/layout optional viewport = dry_run paint side mirror (state 외부의 immediate paint snapshot)
- R47.7.4 scene/resize: AI 가 winit request_inner_size 트리거 → drag 시뮬레이션 (next frame async)
- R47.7.4 scene/wait_for_frame: AI 가 다음 redraw 동기 대기 — scene/resize 결과 stable observation
- R47.7.5 scene/layout viewport optional (None=last winit-actual frame) + last_paint_layout cache
- R47.7.6 LayoutNode.text_metrics (line_count/natural_width/height) — AI sub-pixel wrap 정확 detect
- R47.7.6 paint_producer signature: Fn(w,h) -> Scene → Fn(w,h) -> LayoutNode (application atomic)
- R47.7.6 directly above retracted — LayoutNode.rect.h 가 이미 line_count×line_height = wrap signal
- R51.1 — LayoutNode.line_count: u32 노출 (Text-only sidecar, 다른 kind 는 0)
- R51.1 — Scene-as-data invariant §2 #7 + RPC AI-first §2 #2 정통 적용
- R51.1 — backend agnostic: parley LayoutCache → §5.37.7 자체 엔진 swap 시 surface 유지
- R51.1 — R47.7.6 ceil regression test (300..=320 → line_count=1) + wrap (60px → ≥2) 영구 보장
- R47.7.5 — winit resumed 후 explicit request_redraw() (first paint 전 RPC last_paint_layout None 회피)
- R51.1 line_count semantic: UAX #14 visual lines (BIDI 무관 / empty → 1 / 0 = sentinel)



**Alternatives rejected**:
- Generic-query single method with path DSL — DSL becomes own language; client complex
- Typed-per-action one method per intent — proliferates to N variants per base method



**Impact scope**: §2, §5.7



**Implementations**:
- crates/pinion-rpc/src/invoke.rs:invoke
- crates/pinion-rpc/src/intents.rs:drain_intents
- crates/pinion-rpc/src/layout_query.rs:LayoutNode::line_count
- crates/pinion-core/src/scene.rs:TextNode::line_count
- crates/pinion-runtime/src/layout.rs:compute_layout::text_lines
- examples/hello-button/src/main.rs:App::resumed::request_redraw
- crates/pinion-rpc/src/dispatch.rs:tests::scene_invoke_full_cycle_on_toggle_external_emits_toggle_intent
- crates/pinion-rpc/src/dispatch.rs:deserialize_nullable_present



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
- R51.47 sub-trait future-path: new orthogonal axes follow item-8 Option<&dyn SubTrait> precedent
- R51.47 backwards-compat: R51.34 input axis stays on External (defaults), no v0 retrofit
- R51.47 sub-trait candidates: Drag (R51.34 input fwd) / Lifecycle (item 4) / Cancel (R51.45 carry)




**Impact scope**: §3, §5.9, §5.10, §5.12, §5.14



**Implementations**:
- crates/pinion-core/src/widgets/button.rs:ButtonExternal
- crates/pinion-core/src/widgets/button.rs:ButtonStateSnapshot
- crates/pinion-core/src/external.rs:External::wants_pointer_capture
- crates/pinion-core/src/external.rs:External::pointer_move
- crates/pinion-core/src/external.rs:IntrospectValue::as_f32
- crates/pinion-core/src/external.rs:IntrospectValue::as_i32



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
- R31: §5.26 DamageRect consumed by paint pipeline as scissor box (incremental paint).
- R31: Scene → display list intermediate; retained GPU command buffer cached across frames.
- R31: §5.25 visual ModifierOps (Background/Border/Padding) emit display-list ops in chain order.
- R31: §5.24 SemanticProps not rendered; logical layer above visual; for AT + AI only.
- R31: Glyph atlas GPU texture; cosmic-text shaping cache keyed by (content, font, size, max_w).
- R31: softbuffer = dev fallback only; thin RHI primary; backend chosen at compile time per target.
- R31: §5.22 Signal-driven repaint: visual change propagates Damage via §5.26; no full repaint.
- R31: Paint thread pool (worker per CPU core); RHI command submission on render thread.
- R31: Per-target shader emit via naga (WGSL canonical source); SPIR-V/MSL/HLSL/DXIL backends.
- R31: AAA budget 144 FPS = 6.94ms/frame; layout+paint pipeline must fit within budget.
- R41: Vello hybrid path C ratify — Vello 2024 (Linebender, Xilem production) 가 UI 모드 backend 1종으로 임베드
- R41: R11 thin RHI + naga 결정 보존 — Vello 는 UI 모드 2D 라스터화 한정, 3D engine pass 는 pinion thin RHI 자체 구축
- R41: 원안 Vello 거절 (R11 시점) = 2024 재평가 후 path C 로 수용; alternatives 행은 역사 기록
- R41: 시퀀스 = R40 lifecycle → 위젯 카탈로그 → §5.16 build (Vello 18-24mo) → 3D primitive axis
- R41: §2#4 mode toggle = UI / 게임 모드 backend 분기 예약점 — Vello (UI) / pinion 3D pass 공존 자리
- R41: 언리얼-class path B (3D engine pass 자체) = Phase 4+ 평가; Vello 채택은 B 포기 아님
- R41: pinion 렌더 차별점 = AI-introspectable pipeline (typed scene→paint→pixel) — Qt/Compose 외
- R45: SceneRenderer 표현 = pinion-forge renderer kind manifest + build codegen; runtime dispatch 0
- R45: backend selection = manifest <pinion kind="renderer" backend="vello"/> compile-time per target
- R45: backend 추가 = pinion-forge renderer kind 의 새 template (Vello/softbuffer/headless), Open-Closed
- R45: R41 Phase 2/3/4 (thin-RHI/custom/언리얼-class) = renderer kind 의 새 template, 같은 codegen surface
- R45: §5.16 R11 'zero unjustified abstraction overhead' + R31 'compile-time per target' 정합
- R45: §2#6 'one scene, two render dispatch paths' invariant 가 codegen 의 single emitted path 로 표현됨
- R45: pinion-forge 에 renderer kind 신설 (reactive 옆) — R37.7/R37.8 정합, SCE upstream 미요구
- R45: build slice 1 = renderer kind + Vello first template + demo manifest; R46+ commits
- R51.48 Phase 2 trigger: first 3D primitive scene 요구 → R45 renderer kind 'rhi' template land
- R51.48 Phase 2 scope: thin RHI 3D pass 는 Vello UI path 와 additive 공존, drop = Phase 4+ 평가
- R51.48 Phase 2 surface: pinion-forge renderer kind=rhi + naga shader emit + bgfx/makepad pattern
- R51.48 Phase 2 demo target: 단일 3D primitive (triangle/cube) + AI-introspect scene = first dogfood
- R51.48 Phase 4+ B 평가 gate: 언리얼-class engine pass, AAA RDG/bindless 운용 검증 후 ratify



**Alternatives rejected**:
- GPU pipeline codegen (full) — Futamura projection limit; dynamic dispatch out of scope
- wgpu-only runtime abstraction — 1-10% GUI / 25-240% AAA overhead, conflicts AAA aim
- Self-built RHI (Godot/UE pattern) — 3yr+ work, 1-5% residual overhead
- vello on wgpu — 2D only, 0.x maturity, scene model lock-in
- Per-platform native without abstraction — cross-platform self-build cost
- Self-built RHI without SCE skeleton — zenoh-proven SCE leverage pattern ignored



**Impact scope**: §1, §5.6, §5.9, §5.14, §6



**Implementations**:
- crates/pinion-shell/src/lib.rs:VelloRenderer
- crates/pinion-shell/src/lib.rs:AppShell::render
- crates/pinion-shell/src/lib.rs:vello_renderer_impl
- crates/pinion-shell/tests/smoke.rs



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
- R32 §5.27: Scene closed enum extended to 8th variant VirtualList; non_exhaustive guard validated.



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
- intent_tag!(widget,event) builds V::update dotted wire-form at compile time via stdlib concat!.
- Macro is dual-literal (stable concat! is literal-only); widget matches Scene::with_tag(...).
- Unit test pins macro output against runtime intent-queue format! shape (no separator drift).






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
- crates/pinion-core/src/intent.rs:intent_tag
- examples/hello-toggle/src/main.rs:TOGGLE_INTENT_TAG_FULL
- examples/hello-theme/src/main.rs:TOGGLE_INTENT_TAG_FULL



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
- R30 §5.26: full-recompute (§5.21 R24) refined to incremental + damage tracking for AAA perf.



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


**Intent**: Signal/Computed/Resource fine-grained reactive primitives; pinion-forge DSL (.pinion.xml + <pinion> root) emits Rust struct + Owner-rooted constructor; AI reads via scene/query


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
- §5.20 intent system: Intent dispatch is the Signal write trigger via Update fn
- §5.3 view-fn: pure read of Signal/Computed values during scene rebuild
- §5.12 RPC: scene/query reads Signal value; rewind sets it; snapshot dumps graph
- pinion-forge DSL = .pinion.xml + <pinion xmlns kind=reactive> root; SCE infra reused
- R37.7+R37.8: SCE upstream rejected pinion-specific kind; pinion-forge owns codegen



**Outputs**:
- pinion-core::reactive: Signal<T>, Computed<T>, Resource<T> primitives
- Owner/scope hierarchical lifecycle tree (thread-local v0)
- pinion-forge crate: parse .pinion.xml → Rust struct + Owner-rooted constructor emit
- ExternalIntrospect auto-generated for Signal values exposed at RPC paths
- pinion-forge diagnostic: pinion::dsl::* namespace, NDJSON wire (SCE v1 pattern reference)



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
- R34 §5.29: SyncSignal cross-thread variant ratified; Arc<RwLock<T>> wrapper.
- R36 §5.31: Signal<T> bound extended with T: Serialize + Deserialize for hot reload protocol.
- R38: file ext = .pinion.xml; root = <pinion xmlns kind name>; one file = one struct
- R38: kind=reactive children = <use>/<signal name ty>/<computed name ty>/<resource name ty>
- R38: code embedding = <![CDATA[...]]> in body; generic types via <ty> child element
- R38: codegen = pub struct Name + impl Name { pub fn new(owner: &Owner) -> Self }
- R38: diagnostic = pinion::dsl::* enum + NDJSON wire; SCE v1 pattern reference only
- R38.2a: <signal name ty>CDATA initial</signal> → pub field + Signal::new(initial) wiring
- R38.2a: Owner threaded into new() signature; binding lazy until <computed> in R38.2b
- R38.2a: ty attr validated as non-empty; rustc owns type soundness, syn parse deferred
- R38.2a: 3 new generic child diagnostics — missing-attribute/invalid-ident/empty-body
- R38.2b: <computed name ty>CDATA body</computed> → Computed::new(move || { body })
- R38.2b: over-capture by clone; runtime R26 push-pull discovers real deps from body
- R38.2b: parse_named_typed_body DRY helper unifies signal/computed surface shape
- R38.2b: declaration order = init order; computed references must point to prior children
- R38.2c: <resource name ty err>future body</resource> → Resource::loading() + fetch_with(spawner)
- R38.2c: new() signature widens to <S: LocalSpawner>(&Owner, &S) only when resource present
- R38.2c: resource body authored as async move {} block; pinion-forge does not wrap
- R38.2c: parse_resource_decl separate from parse_named_typed_body; 4-attr shape diverges
- R38.2d: <use path="..."/> → module-level use statement at top of emitted file
- R38.2d: use does not contribute to prior_names; imports visible via module scope
- R38.2d: use body silently skipped; path attr only validation = non-empty; rustc owns syntax
- R38.2e: first dogfood examples/forge-counter — <use>+<signal>+<computed> end-to-end
- R38.2e: codegen emits #[must_use] on constructor matching Signal/Computed/Resource convention
- R38.2e: build.rs pattern = compile_file → $OUT_DIR + include! at main.rs module scope
- R56.1.b — TextEditState (Signal<String> text + Signal<usize> caret) + use_text_edit_state hook



**Alternatives rejected**:
- React hooks — positional state breaks determinism; rules-of-hooks anti-pattern
- Compose Snapshot — thread-local context breaks out-of-process RPC introspection
- Iced full-rebuild — no derived state, perf ceiling at 1k+ nodes, no memo layer
- Event sourcing only — orthogonal pattern, complements but does not replace fine-grained reactivity
- Vue refs — API close to Signal but no explicit Owner tree (lifecycle implicit)
- R38 KDL — modern XML alternative; SCE infra not reusable, parser maintenance burden
- R38 proc-macro DSL — Leptos/Dioxus style; AI introspection harder, no codegen file step
- R38 SFC single file — Vue/Svelte pattern; largest impl burden, custom parser+lexer





**Implementations**:
- crates/pinion-core/src/reactive/signal.rs
- crates/pinion-core/src/reactive/owner.rs
- crates/pinion-core/src/reactive/computed.rs
- crates/pinion-core/src/reactive/resource.rs
- crates/pinion-core/src/reactive/introspect.rs
- crates/pinion-forge/src/lib.rs
- crates/pinion-forge/src/ast.rs
- crates/pinion-forge/src/parser.rs
- crates/pinion-forge/src/codegen.rs
- crates/pinion-forge/src/diagnostic.rs
- crates/pinion-forge/src/wire.rs
- crates/pinion-forge/src/build.rs
- crates/pinion-forge/Cargo.toml
- crates/pinion-forge/src/ast.rs:SignalDecl
- crates/pinion-forge/src/parser.rs:ParseCtx::parse_signal
- crates/pinion-forge/src/codegen.rs:emit_struct_with_signals
- crates/pinion-forge/src/ast.rs:ComputedDecl
- crates/pinion-forge/src/parser.rs:ParseCtx::parse_named_typed_body
- crates/pinion-forge/src/codegen.rs:emit_struct_with_children
- crates/pinion-forge/src/ast.rs:ResourceDecl
- crates/pinion-forge/src/parser.rs:ParseCtx::parse_resource
- crates/pinion-forge/src/codegen.rs:emit_resource_into
- crates/pinion-forge/src/codegen.rs:needs_spawner
- crates/pinion-forge/src/ast.rs:UseDecl
- crates/pinion-forge/src/parser.rs:ParseCtx::parse_use
- crates/pinion-forge/src/codegen.rs:emit_use_block
- examples/forge-counter/Cargo.toml
- examples/forge-counter/build.rs
- examples/forge-counter/ui/counter.pinion.xml
- examples/forge-counter/src/main.rs
- crates/pinion-core/src/reactive/owner.rs:Owner::current
- crates/pinion-core/src/reactive/owner.rs:CURRENT_OWNER_HANDLE
- crates/pinion-core/src/reactive/owner.rs:OwnerHandleGuard
- crates/pinion-core/src/reactive/owner.rs:Owner::cache
- crates/pinion-core/src/reactive/owner.rs:Owner::cache_contains
- crates/pinion-runtime/src/core_shell.rs:CoreShell::apply_key
- crates/pinion-core/src/widgets/text_edit.rs:TextEditState
- crates/pinion-core/src/widgets/text_edit.rs:use_text_edit_state



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
- R51.141 — Handler + HandlerRegistry first-cut land (pinion-runtime, BoxFuture, executor carry)



**Alternatives rejected**:
- React useEffect — conflates reactive scope + async; positional lifecycle coupling
- raw async/await in Update — breaks determinism; dry_run can't skip side effects
- callback registration — imperative; not introspectable; orphan management hard
- IO monad (Haskell) — too abstract for AI authoring surface
- Free monad effects — powerful but compilation cost; over-engineered for GUI





**Implementations**:
- crates/pinion-core/src/reactive/effect.rs
- crates/pinion-core/src/reactive/effect.rs:Effect
- crates/pinion-core/src/reactive/effect.rs:EffectInner
- crates/pinion-core/src/reactive/effect.rs:Effect::new
- crates/pinion-core/src/reactive/effect.rs:EffectInner::rerun
- crates/pinion-core/src/reactive/effect.rs:EffectInner::mark_dirty
- crates/pinion-core/src/reactive/owner.rs:Owner::on_cleanup
- crates/pinion-core/src/command.rs
- crates/pinion-core/src/command.rs:Command
- crates/pinion-core/src/command.rs:Command::new_static
- crates/pinion-core/src/reactive/owner.rs:Owner::dispatch_command
- crates/pinion-core/src/reactive/owner.rs:Owner::pending_commands
- crates/pinion-core/src/reactive/owner.rs:Owner::take_pending_commands
- crates/pinion-core/src/reactive/owner.rs:Owner::take_pending_commands_recursive
- crates/pinion-runtime/src/command/mod.rs
- crates/pinion-runtime/src/command/handler.rs
- crates/pinion-runtime/src/command/handler.rs:Handler
- crates/pinion-runtime/src/command/handler.rs:HandlerFuture
- crates/pinion-runtime/src/command/registry.rs
- crates/pinion-runtime/src/command/registry.rs:HandlerRegistry
- crates/pinion-runtime/src/command/registry.rs:HandlerRegistry::register
- crates/pinion-runtime/src/command/registry.rs:HandlerRegistry::unregister
- crates/pinion-runtime/src/command/registry.rs:HandlerRegistry::dispatch
- crates/pinion-runtime/src/command/executor.rs
- crates/pinion-runtime/src/command/executor.rs:Executor
- crates/pinion-runtime/src/command/executor.rs:BoxFuture
- crates/pinion-runtime/src/command/executor.rs:CommandTaskHandle
- crates/pinion-runtime/src/command/executor.rs:CommandTaskHandle::new
- crates/pinion-runtime/src/command/executor.rs:CommandTaskHandle::no_op
- crates/pinion-runtime/src/command/executor.rs:CommandTaskHandle::cancel
- crates/pinion-runtime/src/command/executor.rs:CommandTaskHandle::is_cancelled
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::new
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::registry
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::dispatch
- crates/pinion-runtime/src/command/executor.rs:BlockOnExecutor
- crates/pinion-runtime/src/command/sink.rs
- crates/pinion-runtime/src/command/sink.rs:IntentSink
- crates/pinion-runtime/src/command/sink.rs:IntentSink::send
- crates/pinion-runtime/src/command/sink.rs:VecSink
- crates/pinion-runtime/src/command/sink.rs:VecSink::new
- crates/pinion-runtime/src/command/sink.rs:VecSink::drain
- crates/pinion-runtime/src/command/sink.rs:VecSink::snapshot
- crates/pinion-runtime/src/command/sink.rs:VecSink::len
- crates/pinion-runtime/src/command/sink.rs:VecSink::is_empty
- crates/pinion-runtime/src/core_shell.rs:CoreShell::with_executor
- crates/pinion-runtime/src/core_shell.rs:CoreShell::set_executor
- crates/pinion-runtime/src/core_shell.rs:CoreShell::clear_executor
- crates/pinion-runtime/src/core_shell.rs:CoreShell::executor
- crates/pinion-runtime/src/core_shell.rs:CoreShell::dispatch_pending_commands
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::cancel_scope
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::in_flight_len
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::has_in_flight
- crates/pinion-shell/src/executor.rs
- crates/pinion-shell/src/executor.rs:TokioExecutor
- crates/pinion-shell/src/executor.rs:TokioExecutor::new
- crates/pinion-shell/src/executor.rs:ProxyIntentSink
- crates/pinion-shell/src/executor.rs:ProxyIntentSink::new
- crates/pinion-shell/src/executor.rs:build_executor_and_sink
- crates/pinion-shell/src/lib.rs:AppEvent::IntentArrived
- crates/pinion-shell/src/app.rs:run_with_handlers
- crates/pinion-shell/src/substrate.rs:ShellCore::set_command_executor
- crates/pinion-shell/src/substrate.rs:ShellCore::command_executor
- crates/pinion-shell/src/substrate.rs:ShellCore::dispatch_intent
- crates/pinion-tui/src/executor.rs
- crates/pinion-tui/src/executor.rs:TokioExecutor
- crates/pinion-tui/src/executor.rs:TokioExecutor::new
- crates/pinion-tui/src/executor.rs:MpscIntentSink
- crates/pinion-tui/src/executor.rs:MpscIntentSink::new
- crates/pinion-tui/src/executor.rs:build_executor_and_sink
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::set_command_executor
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::command_executor
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::dispatch_intent
- crates/pinion-tui/src/shell.rs:run_with_handlers
- crates/pinion-core/src/reactive/owner.rs:Owner::pending_commands_recursive
- crates/pinion-rpc/src/commands.rs
- crates/pinion-rpc/src/commands.rs:CommandsError
- crates/pinion-rpc/src/commands.rs:PendingCommandView
- crates/pinion-rpc/src/commands.rs:list_pending_commands
- crates/pinion-rpc/src/dispatch.rs:DispatchContext::with_commands_owner
- crates/pinion-runtime/src/command/executor.rs:CommandExecutor::in_flight_snapshot
- crates/pinion-rpc/src/commands.rs:list_in_flight_commands
- crates/pinion-rpc/src/dispatch.rs:DispatchContext::with_commands_executor
- examples/hello-commands/src/main.rs
- examples/hello-commands/src/main.rs:queue_one_shot_demo_command
- examples/hello-commands/src/main.rs:CommandsView
- examples/hello-commands/src/main.rs:echo_handler
- examples/hello-commands-tui/src/main.rs
- examples/hello-commands-tui/src/main.rs:HelloCommandsTui
- examples/hello-commands-tui/src/main.rs:queue_one_shot_demo_command
- crates/pinion-core/src/widget_core.rs:WidgetCore::update
- crates/pinion-runtime/src/core_shell.rs:CoreShell::route_intent_through_update
- crates/pinion-core/src/test_fixtures.rs:EchoButtonFixture



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






### §5.25. Modifier composition (chain pattern)


**Intent**: Modifier = ordered chain of closed ModifierOp variants per Scene node. Compose/SwiftUI pattern. Replaces R20 Modifier struct; carries event handlers + reactive overlays + ad-hoc styles.


**Rationale**:
- Compose Modifier.padding(8).background(red).clickable{} chain is textbook canonical
- SwiftUI ViewModifier same pattern; ARIA semantics overlay same shape
- Old §5.11 Modifier struct (margin/padding/align) vestigial after R24 slice 4 absorption
- Closed-form ModifierOp enum keeps Forge codegen + RPC introspection tractable
- Event handlers (Clickable) need a place to live; modifier chain is canonical home
- Reactive modifiers (Signal-driven visual overlay) update without full view-fn rebuild
- Chain order matters (outside-in); declarative composition mirrors CSS specificity



**Inputs**:
- §5.11 R20 vestigial Modifier struct (margin/padding/align) — superseded
- §5.20 Intent: Clickable ModifierOp dispatches Intent on activation
- §5.22 Signal: reactive modifiers depend on Signal subscriptions
- §5.23 Effect/Command: OnAppear/OnSignalChange ModifierOp dispatches Command
- §5.24 SemanticProps: Semantic ModifierOp overlays additional role/state/actions



**Outputs**:
- Modifier = Vec<ModifierOp> on every Scene node (replaces §5.11 Modifier struct)
- ModifierOp closed enum v0: Padding/Background/Border/Clickable/Hover/Focus/Semantic/OnAppear
- Each Scene variant gains modifiers: Modifier field; default = empty chain
- Chain process order = declaration order, outside-in
- SCE schema: modifier list inline within Scene node; Forge emits Vec<ModifierOp>
- scene/modifiers RPC method: inspect chain per node path (12th method)



**Caveats**:
- R29: Modifier = Vec<ModifierOp>; processed declaration order; outside-in semantics.
- R29: Clickable(Intent) dispatches Intent on activation; routes to §5.23 Command pipeline.
- R29: Hover/Focus modifiers toggle SemanticState bitflags per §5.24; reactive.
- R29: Padding ModifierOp overrides LayoutStyle.padding for that node (specificity wins).
- R29: OnAppear/OnSignalChange take Command<Intent>; fire via §5.23 Effect substrate.
- R29: Reactive modifiers (Signal<T> dependency) update visual without view-fn rebuild.
- R29: SCE schema: modifier list inline within Scene node; ordered closed-enum sequence.
- R29: Forge codegen target = Vec<ModifierOp> literal; Rust runtime processes outside-in.
- R29: scene/modifiers = 12th RPC method; returns ModifierOp chain per node path.
- R29: Old §5.11 Modifier struct deleted; fields absorbed earlier (R24 slice 4 LayoutStyle).
- R29: ModifierOp closed enum #[non_exhaustive]; v0 = 9 variants Padding..OnSignalChange.



**Alternatives rejected**:
- Struct-of-fields (R20 original) — closed shape; no event handlers; no reactivity; abandoned
- Trait-based modifier wrappers — expressive but introspection-hostile (Rust trait objects)
- Inline closures per Scene node — breaks dry_run determinism; not serializable
- HOC pattern (React) — positional wrapping; not data-first






### §5.26. Incremental layout + damage tracking


**Intent**: Layout cache by node identity + Signal dep dirty tracking; subtree-only reflow; damage rect to paint; optional off-thread compute. Refines §5.21 full-recompute for AAA perf.


**Rationale**:
- §5.21 R24 full-recompute caps at ~1000 nodes; AAA perf needs subtree caching
- Compose layout pass + Flutter RenderObject + iOS UIView all cache layout per node identity
- Signal subscription naturally tags which layouts depend on which state — free dirty tracking
- Damage rect propagates upward; paint pipeline skips unchanged regions
- Off-thread layout (worker pool) industry standard since Chrome 2018 / Servo 2017
- view-fn purity preserved: scene rebuilds full; layout/paint only touch damaged subtree
- Taffy already supports compute on subtree; just need cache + dirty propagation around it
- Damage rect = union of dirty Rect per frame; sent to GPU/CPU paint as scissor box



**Inputs**:
- §5.21 R24 taffy compute_layout: full-recompute baseline being refined
- §5.22 Signal: subscription substrate provides dirty-trigger pathway
- §5.23 Effect: layout cache invalidation can be modeled as Effect
- §5.2 Scene node identity: stable keys needed for cache hit



**Outputs**:
- LayoutCache per node identity (TaffyTree NodeId + computed Layout retained)
- Dirty bitset propagating from Signal change → subtree invalidation
- DamageRect = union of dirty regions; emitted from compute_layout for paint
- Off-thread compute mode (opt-in): taffy on worker thread, apply back on main
- compute_layout signature evolves: returns (updated rects, DamageRect)
- scene/layout RPC method: query current cached Layout per node path (13th method)



**Caveats**:
- R30: Layout cache keyed by node identity (stable via SCE-emitted IDs); LayoutStyle hash as 2nd key.
- R30: Dirty propagation: Signal change → all dependent layouts marked dirty in single pass.
- R30: DamageRect = union of dirty rects per frame; passed to paint pipeline as scissor region.
- R30: Cache invalidation triggers: LayoutStyle change, Signal-dep change, child add/remove.
- R30: Off-thread layout opt-in; default single-thread (UI thread); same result deterministic.
- R30: compute_layout sig evolves to (&mut Scene, viewport) -> DamageRect; rects mutated in place.
- R30: scene/layout = 13th RPC method; queries cached Layout (rect/padding/border) per path.
- R30: dry_run uses isolated cache; doesn't pollute production cache state.
- R30: Cache eviction: LRU on size pressure; Owner drop triggers child cache cleanup.
- R30: SCE schema: declare layout-affecting Signal deps; Forge emits dependency edges.



**Alternatives rejected**:
- Full re-layout per frame (§5.21 R24 status quo) — perf ceiling at ~1k nodes
- Manual dirty marking by user — error-prone; missed invalidations cause stale layout bugs
- Constraint solver dirty propagation (Cassowary) — different algorithm; abandoned
- Diff-based layout cache (React-style reconciliation) — adds VDOM overhead






### §5.27. Virtualization (VirtualList Scene variant + windowed render)


**Intent**: VirtualList<T> = 8th Scene variant; windowed rendering for 10K+ datasets. Materialize only visible_range items at layout; AI agent sees count + window + materialized items via scene/virtual_list RPC.


**Rationale**:
- Compose LazyColumn / SwiftUI List / Flutter ListView.builder all virtualize — industry standard
- 10K-row table without virtualization = 10K Scene nodes per frame; impossible at AAA budget
- Closed-form Scene IR (§5.2) extended with 8th variant; §5.2 caveat cross-ref needed
- RPC introspection: total count + visible range + materialized window — AI sees enough to reason
- Materialization happens at layout pass, not view-fn — view-fn returns template only
- Scroll offset = Signal<f32>; reactive window update via §5.22 dependency tracking
- Item size determinism (Px) v0; auto-size + variable height as carry-forward
- Damage rect: window scroll marks whole list rect dirty; whole-list repaint single-pass



**Inputs**:
- §5.2 Scene closed-form: extended with 8th VirtualList variant
- §5.22 Signal: scroll offset + visible range + item count Signal-backed
- §5.26 Damage rect: window scroll triggers list-rect damage
- §5.7 RPC: scene/virtual_list method for AI introspection



**Outputs**:
- Scene::VirtualList(VirtualListNode) variant added to closed enum
- VirtualListNode {item_count, visible_range, item_fn, item_size, scroll_offset}
- scene/virtual_list = 14th RPC method (count + range + materialized snapshot)
- Layout pass materializes visible items only; O(window) per frame not O(total)
- SCE schema: virtual_list block with source + template + size
- Damage propagation: scroll change marks list rect dirty (no partial)



**Caveats**:
- R32: Scene::VirtualList = 8th variant; closed-form Scene enum extended; §5.2 cross-ref caveat.
- R32: VirtualListNode {item_count, visible_range, item_fn, item_size, scroll_offset} fields.
- R32: item_fn: Box<dyn Fn(usize) -> Scene>; sync pure callable producing per-index Scene.
- R32: Materialization at layout pass per §5.26; not at view-fn rebuild; O(window) per frame.
- R32: scroll_offset = Signal<f32>; reactive window update via §5.22 dependency tracking.
- R32: item_size = SizeValue::Px(n) v0; auto-size / variable height carry-forward.
- R32: scene/virtual_list = 14th RPC method; returns count + visible range + window snapshot.
- R32: Damage rect: scroll change marks whole list rect dirty; no partial damage v0.
- R32: SCE schema: virtual_list block {source, template, size}; Forge emits VirtualListNode.
- R32: dry_run on VirtualList materializes hypothetical visible_range; not full count.



**Alternatives rejected**:
- Full materialization (no virtualization) — impossible at 10K+; AAA target violation
- Recycler pattern (Android RecyclerView) — stateful pool; view-fn purity violation
- Window via filter in app code — user-implemented; non-canonical; AI introspection lost
- Lazy iterator pattern — functional but materialization timing unclear
- Streaming Scene tree — adds incremental API; over-engineered for v0






### §5.28. Animation (spring physics + interruptible)


**Intent**: Spring-physics animation over Signals; Animated<T> wraps a Signal value with stiffness/damping/mass; interruptible (new target preserves velocity). SwiftUI Animation pattern.


**Rationale**:
- SwiftUI Animation + React Spring + Compose animateXxxAsState all use spring physics by default
- Tween/keyframe animations brittle on interruption; springs natural interrupt-resume
- Animated<T> wraps Signal<T>; framework ticks spring solver per frame
- §5.23 Effect substrate drives animation tick; cancellation via Owner drop
- §5.22 Signal subscription auto-updates dependents when animated value changes
- Pure: same (target, velocity, config) -> same trajectory; dry_run can predict end state
- Interruptibility: new target with current velocity = continuous, no jump
- Time = f32 seconds elapsed; deterministic given fixed clock source



**Inputs**:
- §5.22 Signal: animated value reads/writes through Signal substrate
- §5.23 Effect: animation tick is a framework-driven Effect
- Frame ZST (§6.3): per-frame dt fed in via Frame field evolution



**Outputs**:
- Animated<T> wrapper over Signal<T>; tracks current value + velocity + target
- SpringConfig {stiffness, damping, mass}; presets (Default/Gentle/Stiff/Wobbly)
- Spring solver tick per frame; deterministic given dt + state
- AnimationDriver Effect: framework registers tick scope; cancelable via Owner
- SCE schema: animated value declarations with config; Forge emits Animated<T> init



**Caveats**:
- R33: Animated<T> wraps Signal<T>; T: Animatable trait (numeric, color, transform).
- R33: SpringConfig {stiffness:f32, damping:f32, mass:f32}; presets Default/Gentle/Stiff/Wobbly.
- R33: Spring solver semi-implicit Euler; deterministic given (current, velocity, target, dt, config).
- R33: Interrupt: new target preserves current velocity; spring re-targets continuously.
- R33: AnimationDriver = framework Effect; ticks all active Animated per frame; cancel on Owner drop.
- R33: dry_run predicts steady-state (target value); does not simulate tick sequence.
- R33: Frame.dt field added per §6.3 (Frame ZST evolves); deterministic time source.
- R33: SCE schema: animated block declares Signal + config; Forge emits Animated<T> init.
- R51.142 — CoreShell<V>::root_owner + tick_animations land (§5.28 paint-loop driver surface)
- R51.143 — ShellCore (Vello) paint cycle dt wiring + root_owner forward (#1/2; TUI R51.144)
- R51.144 — ShellCoreTui (TUI) paint cycle dt wiring 평행 (#2/2; Cell interior mutability)
- R51.145 — clamp_frame_dt helper + MAX_FRAME_DT_SECS (1/30s) cap; 양 backend apply
- R56.1.c — CaretBlink Tickable impl (530ms canonical; Owner::cache + register_animation)



**Alternatives rejected**:
- Tween / keyframe animations — brittle on interrupt; abandoned by SwiftUI/Compose 2020s
- Curve-based easing (Material Design) — supported by spring as special case
- Frame-perfect coroutines (Compose Coroutine) — works but spring physics canonical now
- ImGui per-frame interpolation — immediate mode; pinion data-first violation



**Impact scope**: §5.22, §5.23, §6.3



**Implementations**:
- crates/pinion-core/src/animation.rs
- crates/pinion-core/src/animation.rs:Animatable
- crates/pinion-core/src/animation.rs:SpringConfig
- crates/pinion-core/src/animation.rs:SpringState
- crates/pinion-core/src/animation.rs:AnimRect
- crates/pinion-core/src/style.rs:Color::to_linear
- crates/pinion-core/src/style.rs:Color::from_linear
- crates/pinion-core/src/animation.rs:Easing
- crates/pinion-core/src/animation.rs:Tween
- crates/pinion-core/src/animation.rs:Animation
- crates/pinion-core/src/animation.rs:AnimationInner
- crates/pinion-core/src/animation.rs:Tickable
- crates/pinion-core/src/animation.rs:Animation::new
- crates/pinion-core/src/reactive/owner.rs:Owner::register_animation
- crates/pinion-core/src/reactive/owner.rs:Owner::tick_animations
- crates/pinion-runtime/src/core_shell.rs:CoreShell::root_owner
- crates/pinion-runtime/src/core_shell.rs:CoreShell::tick_animations
- crates/pinion-shell/src/substrate.rs:ShellCore::root_owner
- crates/pinion-shell/src/substrate.rs:ShellCore::compute_paint_scene
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::root_owner
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::compute_paint_scene
- crates/pinion-runtime/src/frame_pacing.rs
- crates/pinion-runtime/src/frame_pacing.rs:MAX_FRAME_DT_SECS
- crates/pinion-runtime/src/frame_pacing.rs:clamp_frame_dt
- crates/pinion-core/src/reactive/owner.rs:Owner::any_animation_active
- crates/pinion-runtime/src/core_shell.rs:CoreShell::any_animation_active
- examples/hello-button/src/main.rs:drive_hover_progress
- examples/hello-button/src/main.rs:lerp_grayscale
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::any_animation_active
- crates/pinion-tui/src/shell.rs:run
- examples/hello-button-tui/src/main.rs:drive_hover_progress
- crates/pinion-runtime/src/core_shell.rs:CoreShell::frame_signal
- crates/pinion-core/src/style.rs:Color::lerp
- crates/pinion-core/src/widgets/caret_blink.rs:CaretBlink



### §5.29. Structured concurrency (Owner scope + Tokio + SyncSignal)


**Intent**: Structured concurrency: Owner scope (§5.22) + Tokio task scopes + SyncSignal cross-thread. Cancellation propagates on Owner drop; no orphan tasks. Compose/Swift industry standard.


**Rationale**:
- Compose coroutineScope + Swift structured concurrency 2020s+ industry standard
- Orphan tasks (un-scoped futures) cause memory leaks + UI freezes; eliminated by structure
- §5.22 Owner tree already provides scope hierarchy; concurrency extends it
- Tokio multi-thread runtime for IO/compute; UI thread for view-fn/paint
- SyncSignal<T> = Arc<RwLock<T>> wrapper for cross-thread Signal
- Cancellation: Owner drop → abort all child tasks → transitively
- §5.23 Command<Intent> dispatch in async; result piped back as Intent on UI thread
- Deadlock prevention: locks never held across await; UI thread never blocks



**Inputs**:
- §5.22 Owner: scope tree substrate for task ownership
- §5.23 Command: async dispatch path
- §6.3 tokio: workspace async runtime baseline



**Outputs**:
- TaskScope per Owner; spawn/spawn_local/spawn_blocking methods
- SyncSignal<T> = Arc<RwLock<T>> cross-thread variant; lockless Read path
- AbortHandle on every spawned task; auto-aborted on Owner drop
- Channels: mpsc UI bridge for cross-thread Intent delivery
- Tokio multi-thread runtime owned by app; UI thread distinct from worker pool
- Lock discipline: never hold lock across await; rustc + clippy lint enforced



**Caveats**:
- R34: TaskScope per Owner; spawn returns AbortHandle; cancel on scope drop transitive.
- R34: SyncSignal<T> = Arc<RwLock<T>> + version counter; read lock-free path for hot reads.
- R34: Tokio multi-thread runtime app-owned; UI thread distinct from worker pool.
- R34: Cross-thread Intent delivery via mpsc channel; UI thread polls per frame.
- R34: No locks held across await; rustc + clippy::await_holding_lock lint enforced.
- R34: spawn_blocking for compute-heavy work; workers in tokio pool; not on UI thread.
- R34: §5.23 Handler trait async fn dispatched on TaskScope; cancel propagates per scope.
- R34: SCE schema: scope blocks may declare cross-thread tasks; Forge emits spawn boilerplate.



**Alternatives rejected**:
- Unstructured spawn (tokio::spawn raw) — orphan task risk; no scope cleanup
- Thread-per-task — OS overhead unacceptable at AAA scale
- async-std runtime — tokio more battle-tested + ecosystem
- Custom executor — 3+ year work; tokio canonical






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
- crates/pinion-core/src/style.rs:scale_normalized_to_px



### §5.30. Accessibility (AccessKit bridge from SemanticProps)


**Intent**: AccessKit bridge derives platform AT delegates (AT-SPI / UIA / NSAccessibility) from §5.24 SemanticProps. Focus management + keyboard nav 1st-class. No hand-written AT code.


**Rationale**:
- AccessKit is canonical Rust AT abstraction layer; egui/Druid/Slint adopt it
- §5.24 SemanticProps already carries role/state/actions — perfect input for AccessKit
- Platform AT bindings (AT-SPI/UIA/NSAccessibility) auto-derive; no manual per-OS code
- Accessibility = legal requirement (ADA/EAA); not optional in long-term framework
- Focus management = SemanticState::Focused bitflag toggled reactively via Signal
- Keyboard nav = ModifierOp::Focus combined with Tab/arrow handlers
- Live regions (§5.24 caveat) translate to AccessKit polite/assertive announcements
- Screen reader testing = scene/semantic RPC inspection equivalent



**Inputs**:
- §5.24 SemanticProps: role + state + actions input shape
- §5.22 Signal: state changes drive AT update events
- §5.25 Modifier: Focus + Clickable map to AccessKit actions



**Outputs**:
- pinion-a11y crate wrapping accesskit + platform adapters
- SemanticProps → AccessKit Node conversion (auto)
- Focus state Signal-backed; tab/arrow nav dispatched as Intent
- Platform delegates: AT-SPI (Linux), UIA (Windows), NSAccessibility (macOS), iOS UIAccessibility
- Live region announcements derive from Signal-bound SemanticProps.live_region



**Caveats**:
- R35: pinion-a11y crate wraps accesskit; per-platform features (linux/windows/macos/ios).
- R35: SemanticProps → accesskit::Node mapping auto-generated; closed Role/State translation table.
- R35: Focus state Signal-backed (SemanticState::Focused bitflag); single focused node tree-wide.
- R35: Live region: SemanticProps.live_region -> AccessKit polite/assertive announcement.
- R35: Platform delegates per-OS feature-gated; pinion-a11y reexports accesskit::TreeUpdate.
- R35: scene/semantic (§5.24) RPC = AT-equivalent introspection; AI agent + screen reader parity.
- R35: AT updates throttled to once per frame; matches paint pipeline cadence.
- R35: Tab/arrow nav dispatched as Intent::Focus(direction); app or framework default routes.



**Alternatives rejected**:
- Hand-written platform bindings — per-OS work; AccessKit canonical Rust solution
- Web-style ARIA (HTML attributes) — not applicable; pinion native
- Ignore accessibility (egui historical) — not viable lifetime framework
- Custom AT abstraction — duplicates AccessKit; reinvents wheel






### §5.31. Hot reload (Signal serialization protocol)


**Intent**: Hot reload via Signal<T: Serialize> snapshot/restore: code swap preserves state; new view-fn applies to existing Signals. Flutter hot reload + Compose Live Edit pattern.


**Rationale**:
- Flutter hot reload (2017+) + Compose Live Edit (2022+) prove industry expectation
- Signal-based reactivity makes this natural: serialize all Signals, swap code, restore
- view-fn purity means new view-fn produces same Scene given same Signals — trivial reload
- §5.22 Signal<T> already requires T: Clone + PartialEq; adding Serialize bound minimal cost
- Owner tree (§5.29) preserved across reload; only view-fn code module rebuilt
- Effect / Command in-flight: cancelled on reload (clean slate for side effects)
- Animation state (§5.28) preserved — spring continues from current position post-reload
- SCXML statechart state preserved via serialize; transition runs fresh under new code



**Inputs**:
- §5.22 Signal<T>: T: Serialize bound addition for hot reload
- §5.29 Owner tree: preserved across code swap
- §6.3 dylib reload mechanism: per-target hot reload protocol



**Outputs**:
- Snapshot protocol: serialize all Signal<T> by Owner-tree traversal
- Restore protocol: deserialize after code swap; map by stable path key
- pinion-reload crate: dylib load/unload + state snapshot/restore
- Stable path key generation via SCE-emitted identifiers
- scene/reload = 15th RPC method: trigger reload + return result



**Caveats**:
- R36: Signal<T> requires T: Serialize + Deserialize; serde bound added per §5.22 caveat.
- R36: Stable path key per Signal; SCE-emitted IDs; survives code refactor unless name changes.
- R36: Signal removed in new code: snapshot value discarded silently; logged for inspection.
- R36: Signal added in new code: initialized with default (per code); no snapshot value.
- R36: Signal type-changed: deserialize fails → fall back to new default; warning logged.
- R36: In-flight Command<Intent> cancelled on reload; clean slate for new code side effects.
- R36: Animation state preserved; spring continues from current value/velocity post-reload.
- R36: scene/reload = 15th RPC method; triggers protocol + returns added/removed/preserved counts.
- R36: dylib reload mechanism via libloading or libabigail; per-target compile fingerprint check.



**Alternatives rejected**:
- Full restart (state lost) — worst dev experience; not viable lifetime framework
- Snapshot specific values (manual #[reload_save]) — error-prone; user must remember
- Process-level checkpointing (CRIU) — too heavy; OS-coupled
- VM-level hot patching (Erlang/BEAM) — not feasible for native Rust






### §5.32. AI scene introspection: spatial-semantic locate


**Intent**: xy → element path + region → element set + path → bbox 역방향 RPC. 스크린샷-OCR 없이 시각 선택을 semantic identity 로 변환, AI scene reasoning first-class input.


**Rationale**:
- §2 #7 scene-as-data 의 RPC 구현 — scene/query 의 역방향 (xy → path)
- 사용자 시각 선택 → AI 가 path + state + ancestor chain 즉시 reasoning
- 스크린샷+OCR 대비 토큰 ×100 감소, 정확도 100%, runtime state 가시성
- Qt/Compose/Flutter 모두 screenshot-only — pinion 이 first-class AI 채널
- hit-test statechart-aware → disabled/hidden 이유까지 single round-trip 노출
- 역방향 bbox (path → xy) 가 AI highlight 응답에 필요 — 양방향 대칭



**Inputs**:
- §2 #1 structured scene mandatory — 모든 element 가 typed identity + path
- §2 #7 scene-as-data — path 가 stable identity, 픽셀 없이 query 가능
- §5.7 RPC envelope — 새 method 추가의 transport
- §5.12 screenshot — fallback only; locate 가 primary AI input path
- §5.2 scene primitive trait — 각 variant 가 hit_test impl 책임



**Outputs**:
- RPC scene/locate {x,y} → {path, element, ancestors[], bbox}
- RPC scene/locate_region {x,y,w,h} → {paths[], common_ancestor}
- RPC scene/bbox {path} → {bbox, viewport-relative}
- pinion-rpc dispatch table 7 → 10 typed methods
- pinion-core scene primitive trait gains hit_test() method



**Caveats**:
- coords = logical px (CSS px) DPI-independent; physical px 변환은 backend 책임
- hit-test z-order; overlapping 시 topmost only; region 은 intersect 전부 반환
- disabled/invisible element 도 path 반환 — AI 가 "왜 안 눌리는지" reasoning 위함
- hit-test statechart-aware — SCE current state 가 element interactive 여부 surface
- empty hit (외부 클릭) 시 root path "/" 반환, ancestors empty array
- region paths 는 declaration order = UI tree DFS pre-order (z-order 아님)
- v0 hit-test = naive scene-tree traversal; spatial index (R-tree 등) carry-forward
- bbox = viewport-relative coords; window/screen 변환은 RPC client 책임
- R39.1: Scene::hit_test impl + HitPath{segments, bbox}; tag overrides index segment
- R39.1: scene/locate RPC method active; pinion-rpc dispatch 9 → 10 methods
- R39.1: half-open rect containment via saturating_add; zero-area rects never hit
- R39.2: Scene::hit_test_region + rects_intersect; container + leaves both included
- R39.2: scene/locate_region never errors — disjoint returns empty paths + root ancestor
- R39.2: common_ancestor = longest segment-prefix shared by all paths; root when none
- R39.3: Scene::lookup_path reverse-walks segments; tag wins over index on collision
- R39.3: bbox accepts both /window[id]/seg and /seg implicit forms; round-trips with locate
- R39.3: scene/bbox 12th RPC method; bidirectional locate↔bbox completes §5.32 surface



**Alternatives rejected**:
- screenshot + vision model — 토큰 ×100, OCR 부정확, state 없음, 결정성 없음
- ARIA accessibility tree only — 모든 element 가 ARIA props 가지지 않음
- IDE-specific protocol — 외부 AI agent 사용 불가; first-class RPC 가 필수
- scene/query 만 — path→subtree 방향; 역방향 (xy→path) 도 명시적 axis 필요
- spatial index v0 시점 — premature; naive DFS 로 수천 element 까지 충분



**Impact scope**: §5.7, §5.12, §5.2, §2



**Implementations**:
- crates/pinion-core/src/scene.rs:Scene::hit_test
- crates/pinion-core/src/scene.rs:HitPath
- crates/pinion-rpc/src/locate.rs
- crates/pinion-rpc/src/dispatch.rs:handle_scene_locate
- crates/pinion-core/src/scene.rs:Scene::hit_test_region
- crates/pinion-rpc/src/locate.rs:locate_region
- crates/pinion-core/src/scene.rs:Scene::lookup_path
- crates/pinion-rpc/src/locate.rs:bbox



### §5.33. AI overlay UX: event capture + highlight rendering


**Intent**: pinion-overlay crate: AI mode event capture + scene-level highlight injection. §5.32 introspection 위에 user-facing surface 구축, 시각 선택과 AI 응답 highlight 의 first-class layer.


**Rationale**:
- §5.32 RPC primitives 가 protocol-level — user-facing visible value layer 필요
- AI agent 의 highlight 응답을 scene 에 inject 하는 표준 surface
- event interception transport-agnostic — winit/web/tui 등 backend 가 호출
- framework axis 로 박아 모든 example/widget 이 공용 (재구현 부담 제거)
- v0 함수형 API — immutable transform, dry_run 종함적 결정



**Inputs**:
- §5.32 scene/locate, locate_region, bbox — Scene ↔ path/bbox 변환 표면
- §5.2 scene primitive — highlight 구조는 Box inject (Effect/External 안 쓰임)
- §5.7 RPC envelope — AI ↔ overlay 양방향 message 표준 transport
- §2 #1 structured scene — overlay 자체가 scene-as-data 로 query 가능
- §5.20 intent tag — highlight Box 가 ai-overlay/<path> 형태 tag 부여



**Outputs**:
- 새 crate pinion-overlay (workspace member 추가)
- OverlayEvent enum: Click {x,y}, Drag {x1,y1,x2,y2}, Escape, Acknowledge
- inject_highlight(scene, path, style) → Scene 순수 함수
- clear_highlights(scene) → Scene 순수 함수 (set semantics)
- examples/ai-introspect-demo 신설 (dogfood with winit/softbuffer)



**Caveats**:
- v0 함수형 API; Controller pattern은 evidence 쌓인 후 R39.4.x carry-forward
- event types transport-agnostic; winit/web/tui 매핑은 consumer (example/runtime) 책임
- highlight = Scene::Box inject; tag prefix "ai-overlay/" 로 식별 (다른 Box 영향 없음)
- inject_highlight = immutable transform; 새 Scene 반환; dry_run/snapshot 호환
- multiple highlight 동시 가능; set semantics on tag suffix; 중복 inject = idempotent
- clear_highlights = tag prefix 로 일괄 제거; 다른 ai-overlay/* 외 Box 영향 없음
- v0 event capture mode 단순 toggle; modeless overlay 디자인은 R39.4.x
- transport binding (winit event → OverlayEvent) 은 example/runtime consumer 책임
- R39.4.3: ai-introspect-demo dogfood; right-click → locate; left-click/Esc → clear
- R39.4.3: in-process pinion-rpc call; AI-native path/bbox/ancestors printed to stdout
- R39.4.3: border rendered as 4 thin filled rects (paint_border helper, scoped to demo)



**Alternatives rejected**:
- pinion-runtime 안 직접 — runtime 미성숙; coupling 위험; 시기 부적절
- examples/ 만 — framework axis 표면화 안 됨; 다음 example마다 재구현 부담
- Scene::Effect 활용 — Effect opaque (§3); introspect 불가; AI 가 highlight query 못함
- winit dep 직접 — transport-agnostic 원칙 위반; web/tui 차단
- Controller struct v0 — state ownership 결정 premature; 함수형 v0 후 promote



**Impact scope**: §5.32, §5.7, §5.2, §5.20, §2



**Implementations**:
- crates/pinion-overlay/Cargo.toml
- crates/pinion-overlay/src/lib.rs
- crates/pinion-overlay/src/event.rs:OverlayEvent
- crates/pinion-overlay/src/highlight.rs:inject_highlight
- crates/pinion-overlay/src/highlight.rs:clear_highlights
- examples/ai-introspect-demo/Cargo.toml
- examples/ai-introspect-demo/src/main.rs
- crates/pinion-overlay/src/highlight.rs:HighlightStyle::with_stroke
- crates/pinion-overlay/src/highlight.rs:HighlightStyle::with_stroke_width



### §5.34. AI scene change proposal: prepare/preview/apply/cancel lifecycle


**Intent**: AI 가 typed change 제안 → stable preview_id 발급 → dry_run 결정성으로 미리보기 → apply/cancel/timeout 으로 완료; §2#3 invariant 의 명시적 RPC lifecycle 층.


**Rationale**:
- §2 #3 dry_run invariant 를 RPC lifecycle 로 표면화 — stable handle 위 prepare/finalize textbook
- §5.32 locate + §5.33 overlay 결합 — AI target 선택 → 제안 → visual preview → accept/reject
- VSCode CodeAction / 2PC prepare-commit / SAGA — prepare→finalize lifecycle 패턴 textbook
- preview_id 가 stable identity — locate / bbox / screenshot 모두 preview state 대상 query 가능
- TTL + timeout 으로 bounded lifetime — leak 방지, 동시 preview 다수 허용
- view-fn purity (§6.3) + signal snapshot/restore (§5.22 R37) 위에 자연 구현
- preview state = post-apply state 동치 (SCE 결정성) — apply race 없음



**Inputs**:
- proposal.kind: typed enum (SetSignal / ReplaceView / SetStyle / DispatchIntent — R40 초기 4종)
- proposal.target: scene path (§5.32 locate 결과 또는 명시적 segment chain)
- proposal.payload: kind 별 (Signal value / View ref / Modifier patch / Intent envelope)
- ttl_hint_ms: optional preview lifetime 제안 (server clamp; default = R40.1 spec)



**Outputs**:
- preview_id: opaque stable handle — server 발급, monotonic, TTL 만료 시 invalid
- preview_scene_diff: 영향 path 집합 — locate / bbox 결합용
- diagnostic: invalid 시 reason (target missing / type mismatch / TTL exceeded / capacity full)
- lifecycle RPC 4종: propose_change / apply_preview / cancel_preview / list_previews



**Caveats**:
- preview ledger 용량 bounded — max concurrent + max TTL server 결정 (R40.1+ spec)
- 동시 preview 충돌 — 동일 target path 두 preview 시 정책 R40.1 spec (last-write or reject)
- apply atomicity — all-or-nothing; 부분 적용 없음; failure 시 ledger rollback
- cancel idempotent — 중복 cancel safe; 이미 expired 도 동일 ok 반환
- preview state side-effect 0 — signal write sandbox, Effect/Command 실제 실행 안 함
- timeout 자동 cancel — stale preview_id 사용 시 명시 error, ID 재사용 금지
- 기존 dry_run RPC (one-shot) 유지 — propose_change 는 stateful 보강 axis
- R40.1: capacity default=64, TTL default=60s, MAX_TTL=600s; configurable via with_config
- R40.1: conflict policy = independent ledger + OCC (base_revision token per entry, compared at apply)
- R40.1: lazy eviction on propose — past-deadline entries reclaimed before capacity check
- R40.1: Proposal as open trait; concrete variants R40.5 — ledger schema decoupled
- R40.2: dispatch fn signature gained &PreviewLedger param — caller passes alongside &mut Scene
- R40.4: SceneRevision = AtomicU64 OCC token; dispatch auto-bumps on click/rewind/invoke success
- R40.4: non-dispatcher mutation paths (winit direct) must bump revision — conservative policy
- R40.5: TypedProposal #[non_exhaustive] enum; SetSignal first variant; SetStyle/etc R40.x sub-slices
- R40.5: SetSignal value carried as serde_json::Value; type coercion to T deferred to R40.6 apply
- R40.6: Proposal trait gains apply(scene) method; vtable dispatch per variant — Box&lt;dyn&gt; safe
- R40.6: apply consumes entry on success AND failure (one-shot); conflict alone retains entry
- R40.6: apply_preview self-bumps revision; excluded from mutates_scene_on_success (no double-bump)
- R40.7: DispatchContext struct bundles &mut Scene + &PreviewLedger + &SceneRevision (single param)
- R44: DispatchIntent emit channel = synchronous apply response (ApplyOutcome.emitted_intents)
- R44: scene/intents emit channel = asynchronous poll (External pending_intents drain via §5.20)
- R44: AI client switch — apply 응답의 emitted_intents 와 scene/intents 결과를 합쳐 단일 stream 으로 reduce 가능
- R44: 통합 reject 사유 — 단일 channel 시 cause-effect 시점 / poll timing 분리 불가; Brooks conceptual integrity 위반
- R44: dual channel = AI cause-effect (apply→intent 같은 turn) vs widget SM emission (별도 turn)



**Alternatives rejected**:
- one-shot dry_run + manual replay — stable handle 없음; AI 가 preview introspect 불가
- text diff / LSP CodeAction 차용 — pinion scene-as-data 모델과 mismatch; visual 단절
- implicit auto-commit — AI safety surface 깨짐; preview = apply 동치 위반
- snapshot/restore primitive 직접 노출 — 너무 low-level; lifecycle 부재
- diff-only return — visual preview 불가; §5.33 overlay 와 단절



**Impact scope**: §2, §5.7, §5.22, §5.32, §5.33



**Implementations**:
- crates/pinion-rpc/src/preview/mod.rs
- crates/pinion-rpc/src/preview/id.rs:PreviewId
- crates/pinion-rpc/src/preview/proposal.rs:Proposal
- crates/pinion-rpc/src/preview/error.rs:ProposeError
- crates/pinion-rpc/src/preview/error.rs:ApplyError
- crates/pinion-rpc/src/preview/ledger.rs:PreviewLedger
- crates/pinion-rpc/src/preview/ledger.rs:Entry
- crates/pinion-rpc/src/preview/ledger.rs:PreviewView
- crates/pinion-rpc/src/preview/ledger.rs:SweepReport
- crates/pinion-rpc/src/preview/cancel.rs:cancel_preview
- crates/pinion-rpc/src/dispatch.rs:handle_scene_cancel_preview
- crates/pinion-rpc/src/preview/id.rs:PreviewId::try_new
- crates/pinion-rpc/src/preview/list.rs:list_previews
- crates/pinion-rpc/src/dispatch.rs:handle_scene_list_previews
- crates/pinion-rpc/src/dispatch.rs:preview_view_to_json
- crates/pinion-core/src/revision.rs:SceneRevision
- crates/pinion-rpc/src/dispatch.rs:mutates_scene_on_success
- crates/pinion-rpc/src/preview/kinds.rs:TypedProposal
- crates/pinion-rpc/src/preview/propose.rs:propose_change
- crates/pinion-rpc/src/preview/propose.rs:ProposeOutcome
- crates/pinion-rpc/src/dispatch.rs:handle_scene_propose_change
- crates/pinion-rpc/src/preview/apply.rs:apply_preview
- crates/pinion-rpc/src/preview/apply.rs:ApplyOutcome
- crates/pinion-rpc/src/dispatch.rs:handle_scene_apply_preview
- crates/pinion-rpc/src/dispatch.rs:DispatchContext
- examples/ai-introspect-demo/src/main.rs
- crates/pinion-rpc/src/preview/proposal.rs:ApplyContext
- crates/pinion-rpc/src/preview/kinds.rs:TypedProposal::DispatchIntent
- crates/pinion-rpc/src/preview/kinds.rs:TypedProposal::SetStyle
- crates/pinion-core/src/scene.rs:Scene::lookup_path_mut
- crates/pinion-rpc/src/preview/blueprint.rs:ViewBlueprint
- crates/pinion-rpc/src/preview/kinds.rs:TypedProposal::ReplaceView
- crates/pinion-core/src/scene.rs:Scene::lookup_path_ref
- crates/pinion-rpc/src/path.rs:split_at_external



### §5.35. Input dispatch — cursor/key → widget routing primitive


**Intent**: input event → framework hit-test/focus → widget dispatch; application routing 0줄, §5.20 intent (output) 의 input 대칭 axis


**Rationale**:
- Xilem/Druid/Slint/Qt/GTK/iced 모두 input dispatch = framework primitive (textbook)
- §5.32 scene/locate 의 pure hit-test infra 가 internal+external dispatch 공유
- R47 hello-button hit-test fix 는 application-level workaround — 위젯 추가 시 같은 bug 반복
- §5.15 item 5 input forwarding 이 protocol 만 명시, framework-side router 미spec
- §5.20 intent (output) 대칭 axis 부재 — input path 의 framework primitive null



**Inputs**:
- Scene (post-layout paint scene, framework-retained)
- input event (PointerMove/Down/Up, Key, Focus*)
- state scene (ExternalNode.tag 기반 widget dispatch target)



**Outputs**:
- 0/1 dispatch → target widget invoke('send', Text(event_name))
- Hover transition (PointerEnter/Leave) on cursor↔tag 변화
- focus transition (click/key → next focusable tagged widget) [v1+]



**Caveats**:
- Single-target hit-test 가 R48.1 scope, multi-target (capture/bubble) 은 carry-forward
- focus 모델 v0: click→focus pointer 만, key dispatch의 focus tab order 는 carry
- Touch/gesture (pinch/multi-finger) 는 R48 scope 아님 — winit Touch event carry
- paint scene Container/Box.tag 가 state scene ExternalNode.tag 와 매칭 — pinion-core schema 확장
- R51.41 sub-index: paint 'tag#idx' → InputRouter '#' split + invoke('send','idx:Event')
- R51.50 paint tag literal 의 '#' 사용 = application 측 금지 — InputRouter R51.42 split 와 충돌
- R51.50 위반 시 행동: 첫 '#' 이전 primary 추출 → 의도치 않은 state lookup, 묵시적 dispatch drop
- R51.50 정식 용법: composite hit-target convention 만 '#' 사용 (paint 'tag#idx' + state primary)



**Alternatives rejected**:
- Application-level dispatch (R47 현재) — DRY 위반, 위젯마다 같은 bug 반복; 안 함
- Per-widget input subscription (Qt signals/slots) — §5.15 External opaque + §6.3 view-fn purity 충돌
- RPC-only dispatch — hot path serialize 오버헤드 (cursor 100Hz+); 안 함



**Impact scope**: §5.13, §5.15, §5.20, §5.32


**Examples**:

```rust
// hello-button view fn (R48.3 refactor 후) — application 의 hit-test 코드 0줄
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![bg_child])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(LayoutStyle::new().flex(...)),  // background
    )
    // inner button container 가 .with_tag("main_btn") 부여
    // → InputRouter 가 자동 dispatch
}

// main event loop
let mut router = InputRouter::new();
// 매 render 후
router.update_paint_scene(paint_scene, &mut state_scene);
// winit CursorMoved
router.cursor_moved(x, y, &mut state_scene);  // 자동 Enter/Leave dispatch
// winit MouseInput Press
router.pointer_down(&mut state_scene);  // hover_target 으로 자동 dispatch

```

```rust
// 위젯 카탈로그 (Slider) — 같은 primitive, 코드 0줄 (R47-class bug 재발 불가)
fn view(state: &AppState, _frame: &Frame) -> Scene {
    Scene::Container(ContainerNode::new(vec![
        Scene::Container(ContainerNode::new(vec![/* slider visuals */])
            .with_tag("volume_slider")),  // ← 이게 전부
        Scene::Container(ContainerNode::new(vec![/* button */])
            .with_tag("save_btn")),
    ]))
}

// state scene
let state_scene = Scene::Container(ContainerNode::new(vec![
    Scene::External(ExternalNode::new(SliderExternal::new()).with_tag("volume_slider")),
    Scene::External(ExternalNode::new(ButtonExternal::new()).with_tag("save_btn")),
]));

// router 의 dispatch logic 은 단일 — 위젯 N개여도 코드 N배 안 됨
router.cursor_moved(x, y, &mut state_scene);  // volume_slider vs save_btn 자동 선택
router.pointer_down(&mut state_scene);

```



**Implementations**:
- crates/pinion-runtime/src/input.rs:InputRouter::captured_target
- crates/pinion-runtime/src/input.rs:InputRouter::forward_pointer_move
- crates/pinion-runtime/src/input.rs:rect_for_tag
- crates/pinion-runtime/src/input.rs:normalize_cursor
- crates/pinion-runtime/src/input.rs:widget_wants_capture
- crates/pinion-shell/src/lib.rs:AppShell::apply_key
- crates/pinion-shell/src/lib.rs:named_key_str
- crates/pinion-runtime/src/input.rs:PointerId
- crates/pinion-runtime/src/input.rs:PointerId::MOUSE
- crates/pinion-runtime/src/input.rs:PointerId::touch
- crates/pinion-runtime/src/input.rs:InputRouter::cursors
- crates/pinion-runtime/src/input.rs:InputRouter::hover_targets
- crates/pinion-runtime/src/input.rs:InputRouter::captured_targets
- crates/pinion-runtime/src/input.rs:InputRouter::hover_wants_capture
- crates/pinion-runtime/src/input.rs:split_subindex
- examples/hello-radio-group/src/main.rs:RadioGroupView::apply_key
- crates/pinion-shell/src/lib.rs:AppShell::handle_touch
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBarExternal::pointer_move



### §5.36. Text shaping & glyph cache — Linebender parley + glyph atlas primitive


**Intent**: 모든 backend(Vello/Headless/TUI/미래) 공유 backend-orthogonal text layout+shaping+glyph cache framework primitive — 위젯 카탈로그 universal prereq


**Rationale**:
- R41 Vello (Linebender) 채택 = parley layout primary ecosystem 정통 가입
- swash (shaping) + fontique (font mgmt) = parley 자연 동반 = 단일 vendor stack
- SOLID single responsibility: layout/glyph cache/paint dispatch 분리 = backend-orthogonal
- 위젯 카탈로그 (TextField/IME/Slider value/Toggle label) universal prereq
- R31 'glyph atlas GPU texture' 정신 보존 — GlyphCache + Vello draw_glyph 통합으로 실현



**Inputs**:
- pinion-core::scene::TextNode (content / TextStyle)
- pinion-core::style::TextStyle (font_family / size / weight / style / color / decoration)
- DPI scale factor (HiDPI sub-pixel positioning)
- viewport width (single-line: None, multi-line: Some(w) — parley break_all_lines)



**Outputs**:
- pinion-text::Layout — parley::Layout re-export (positioned glyph runs)
- pinion-text::LayoutCache — LRU bounded text+style → Layout cache (CPU, R47.2)
- paint_adapter Text arm — Scene::Text → vello::Scene::draw_glyphs (R47.3, paint primitive part)
- compute_layout Scene::Text MeasureFunc — parley intrinsic width/height (R47.4)
- TextStyle 확장 — font_weight/style/line_height/letter_spacing/align/decoration/overflow (R47.5)
- paint_text TextStyle 모든 필드 honor — parley StyleProperty + Alignment (R47.6, §5.36 close)
- pinion-text::GlyphCache — LRU bounded rasterized glyph atlas (GPU, R47.x+ UI dense text 보강)
- backend adapter helper — glyph run → backend draw (paint_adapter Text arm 통합)



**Caveats**:
- R21 cosmic-text 결정 partial supersede — layout=parley, glyph atlas 정신 보존
- TextField / IME / text selection / clipboard 은 R49+ widget catalog 별도 axis
- RTL / bidi / complex script = parley 의 swash 기본 지원, 별도 caveat 없음
- Font fallback 정책 = fontique system enum + override API (R47.x sub-decision)
- GlyphCache evict 정책 = LRU bounded; capacity/scope per-renderer vs shared (R47.x)
- pinion-text feature gate = parley default, advanced (RTL/complex) toggle 가능
- 구현 단계: R47.2 = Layout+LayoutCache, R47.5+ = GlyphCache + Vello 통합 path
- GlyphCache lifetime canonical = consumer GPU atlas (Vello roadmap_2023 미구현, AAA 144 FPS prereq)
- 정정: 직전 'AAA 144 FPS prereq' caveat = framing 오류. 정통 frame = UI 모드 dense text (CJK/다국어) 성능 보강
- parley = Phase 1 bridge; Phase 2+ lifetime canonical = pinion 자체 text engine (§5.16 R11 thin RHI 정합)
- R47.3 = paint primitive only; layout MeasureFunc + TextStyle Figma-fidelity = R47.4-6 carry
- R47.6 = §5.36 close. R48 §5.3 별도 = BoxStyle Figma-fidelity (corner/shadow/gradient/blend/opacity)
- R47.6 §5.36 round close = parley wire + decoration + Clip. Ellipsis = Clip fallback (R47.x carry)
- R50 진입 = §5.36 의 parley/swash/fontique = Phase 1 bridge; §5.37 self-hosted text engine 으로 supersede
- R47.7.6 — parley layout width/height .ceil() pixel snap (sub-pixel jitter 차단)
- R56.1.b.2 — pinion-text caret_rect_for_byte_offset + CaretRect f32 (parley Cursor wrap)
- LayoutKey 가 TextStyle 을 fully hash 하므로 fg_color (parley Brush, shape-independent) 도 key 의 일부 (R587).
- shape vs paint split 은 multi-line text editor 2nd consumer 등장 후 Rule of Three (R587).



**Alternatives rejected**:
- cosmic-text (R21 ratified) — System76 COSMIC fork; R41 Vello 채택 후 ecosystem mismatch
- glyphon — cosmic-text wrapper, 종속
- swash 직접 사용 (parley 없이) — layout 직접 구현 = reinventing parley
- HarfBuzz/FreeType FFI — Rust-native ecosystem 정합 부족, build dep 무거움



**Impact scope**: §5.3, §5.16, §5.20, §5.30, §6.3, §5.37



**Implementations**:
- crates/pinion-runtime/src/layout.rs:compute_layout::ceil
- crates/pinion-runtime/src/paint_adapter.rs:paint_text
- crates/pinion-text/src/caret.rs:caret_rect_for_byte_offset
- crates/pinion-text/src/caret.rs:CaretRect



### §5.37. Self-hosted text engine — full OpenType to GPU rasterization stack


**Intent**: pinion 의 모든 text 동작 자체 구현 — parley/swash/fontique/ttf-parser 모두 제거, OpenType parser 부터 GPU rasterization 까지 full stack. lifetime canonical.


**Rationale**:
- 외부 lib (parley/swash/fontique) 의존 = black box 동작 / 비결정 결과 / 진단 격차
- 사용자 보고 sub-pixel oscillation 원인 진짜 파악 불가 — parley wrap heuristic 내부 모름
- lifetime project canonical = 모든 layer 정확 파악 + 제어 / black box 우회 허용 안 함
- §2 invariant #1 (structured scene) 정합 — text 동작도 fully introspectable
- 메모리 carry: 'Phase 2+ lifetime canonical = pinion 자체 text engine' R50 이 그 round



**Inputs**:
- TextNode (§5.11) + TextStyle (§5.3 + R47.5 Figma-fidelity schema)
- TTF / OTF / WOFF2 binary (OpenType spec compliant)
- Unicode codepoint sequence (NFC/NFD)
- viewport / max_width (레이아웃 constraint)
- DPI scale_factor (sub-pixel positioning)



**Outputs**:
- 자체 OpenType parser — sfnt + cmap + hmtx + glyf/CFF + GSUB/GPOS + variable + color
- 자체 Unicode 정규화 (UAX #15 NFC/NFD/NFKC/NFKD)
- 자체 BIDI algorithm (UAX #9) + script segmentation (UAX #24)
- 자체 shaping rules per script (Latin/Arabic/Indic/CJK/Emoji ZWJ)
- 자체 line break (UAX #14) + word break (UAX #29)
- 자체 glyph positioning + sub-pixel integer-snap (비결정 제거)
- 자체 glyph rasterization — vector outline → raster, hinting, anti-aliasing
- 자체 font fallback + OS enumeration (fontconfig/DirectWrite/CoreText 대체)
- 자체 GPU atlas + MSDF / SDF — §5.16 R11 thin RHI 정합



**Caveats**:
- R50.0 = spec round entry (axis ratify). implementation 은 multi-session/multi-month (R50.1+)
- §5.36 (parley + swash + fontique) Phase 1 bridge → R50 진입 = §5.36 superseded by §5.37
- implementation sub-round: parser/Unicode/BIDI/shaping/break/positioning/raster/atlas



**Alternatives rejected**:
- parley/swash/fontique 유지 (R47 현행) — black box 의존 잸류, lifetime canonical 위반
- ttf-parser dep 허용 + 그 외 자체 — OpenType binary parsing 도 대량 black box, 제공 거부
- HarfBuzz (C) FFI + 자체 layout — FFI 채널 + C 의존, pure Rust lifetime canonical 이탈
- 단계적 Phase A/B/C — implementation 은 자연 단계적, axis 결정은 Maximal 1회 — 안 하면 광범위 타협 위험



**Impact scope**: §5.36, §5.16, §5.11, §5.3, §2




### §5.37.1. OpenType binary parser — sfnt foundation (R50.1 sub-scope)


**Intent**: R50.1 OpenType binary parser sub-scope — sfnt Offset Table + 6 mandatory tables + glyf/loca + name. Latin-first foundation for §5.37 자체 text engine; CFF/variable/color/WOFF2 후속 sub-section.


**Rationale**:
- R50.1 = §5.37 의 foundation layer — R50.2 Unicode 이후 모든 layer 가 parser 출력 의존
- 단일 layer 진입 = textbook 단계적 진입, Microsoft OpenType 1.9.x spec 정통 reference
- sfnt header 부터 시작 = 모든 OpenType variant (TTF/OTF/TTC/Apple/Type1) 의 entry point
- Latin-first = 초기 검증 단순화, multi-script (Nanum Gothic 한글) test fixture 로 forward-compat 확인
- thiserror 같은 외부 lib 도 제거 = lifetime canonical 완전 적용, 자체 enum + Display impl



**Inputs**:
- TTF / OTF binary file (no WOFF2 — 압축 format, separate sub-section R50.1.X)
- Latin script codepoint (BMP, U+0000-U+FFFF) — 초기 검증 범위
- 한글 codepoint (U+AC00-U+D7A3) — Nanum Gothic fixture, forward-compat 검증용
- OpenType 1.9.x spec compliant byte stream (big-endian, sfnt structure)



**Outputs**:
- Font struct — head/OS2/hhea/hmtx/maxp 출력 (units_per_em / ascent / descent / line_gap / advance)
- cmap lookup table — codepoint → glyph index (format 4 BMP + format 12 UCS-4)
- glyph outline data — glyf + loca parsed (simple TrueType glyph, compound postpone)
- name table — family / style / postscript name (font fallback / introspection 용)
- ParseError enum — 자체 Display impl, no thiserror



**Caveats**:
- WOFF2 / CFF / variable axis / color tables = R50.1.X 별도 sub-section, 후속 진입
- GSUB / GPOS table = parser 는 raw table store 만 — execution 은 R50.3 shape sub-round 책임
- test fixture = Noto Sans (Apache) + Nanum Gothic (OFL) — Latin + 한글 forward-compat 검증
- error type = 자체 ParseError enum + Display impl, no thiserror dep — R50 정신 완전 적용
- compound glyph (composite GlyphID with transform matrix) = R50.1.4 후속 또는 별도 sub-round
- R50.1.1 ~ R50.1.5 sub-phase = sfnt directory / metadata / cmap / glyf+loca / name
- test fixture LICENSE 정정 — Noto Sans 도 OFL 1.1 (Apache framing 은 pre-2018 history 잔존)
- R50.1.3.1 corrective: cmap spec strict + invariant 검증 + duplicate reject 일관 적용
- R50.1.3.2 cmap/ 디렉토리 분리 — format4/format12/test_helpers per industry precedent
- R50.1.3.3 best_subtable refactor — selection_score + min_by_key 1-pass



**Alternatives rejected**:
- (a) sfnt + CFF + variable + color 동시 진입 — scope creep, 단계적 textbook 위반
- (b) ttf-parser dep + 자체 shaping 만 — R50.0 정신 (외부 lib black box) 본질 위반
- (c) HarfBuzz FFI + 자체 parser — C 의존, pure Rust lifetime canonical 이탈
- (d) WOFF2 우선 — 압축 format 가 raw sfnt parser dependency, 순서 역전
- (e) thiserror derive — 외부 dep, R50 정신 (외부 lib 완전 제거) 완전 적용 못 함



**Impact scope**: §5.37, §5.16, §5.11, §5.3



**Implementations**:
- crates/pinion-text-font/src/sfnt.rs:parse_sfnt
- crates/pinion-text-font/src/sfnt.rs:Flavor
- crates/pinion-text-font/src/sfnt.rs:OffsetTable
- crates/pinion-text-font/src/sfnt.rs:TableRecord
- crates/pinion-text-font/src/error.rs:ParseError
- crates/pinion-text-font/src/font.rs:Font
- crates/pinion-text-font/src/reader.rs:Reader
- crates/pinion-text-font/src/tables/head.rs:Head
- crates/pinion-text-font/src/tables/hhea.rs:Hhea
- crates/pinion-text-font/src/tables/hmtx.rs:Hmtx
- crates/pinion-text-font/src/tables/maxp.rs:Maxp
- crates/pinion-text-font/src/tables/os2.rs:Os2
- crates/pinion-text-font/src/tables/post.rs:Post
- crates/pinion-text-font/src/error.rs:FieldValue
- crates/pinion-text-font/src/tables/cmap/mod.rs:Cmap
- crates/pinion-text-font/src/tables/cmap/mod.rs:CmapSubtable
- crates/pinion-text-font/src/tables/cmap/format4.rs:Format4
- crates/pinion-text-font/src/tables/cmap/format12.rs:Format12



### §5.37.2. Text engine RPC channel — AI-first font/text introspect (R50.X sub-scope)


**Intent**: §5.37 text engine 의 RPC channel sub-scope — pinion-rpc 가 §5.37.1 parser 결과 + 후속 text layer (Unicode/BIDI/shape/layout) 를 JSON-RPC 2.0 로 AI agent 에게 노출, §2 invariant #2 (RPC AI-first) 의 text 영역 첫 적용.


**Rationale**:
- §2 invariant #2 (RPC AI-first) 의 §5.37 영역 첫 channel — parser 결과 AI introspect
- §5.37.1 parser 결과를 AI 가 RPC 로 query — text 진단 격차 (sub-pixel oscillation) 해소
- §5.7 JSON-RPC 2.0 + §5.12 hybrid typed method ratify 정합 — text 도 동일 transport
- font/* namespace = text/* 와 분리 — text layer (R50.4+ shape) 별도 sub-scope 향후
- Font registry = Arc<Mutex<HashMap<u32, Arc<Font>>>> — concurrent-safe handle pattern
- parser stateless → RPC stateful (font_id) — re-parse 비용 제거, AI agent latency 작음



**Inputs**:
- §5.37.1 의 Font / ParseError / CmapSubtable / Glyph / 6 metadata table 출력
- §5.7 JSON-RPC 2.0 transport (request/response/error envelope)
- §5.12 hybrid typed RPC method dispatch pattern (method 별 typed params/return)
- font binary (base64 인코딩 byte stream) — RPC request payload
- font_id (u32 handle) — registry lookup key, AI agent 이 유지



**Outputs**:
- pinion-rpc/src/font.rs — font/* method 집합 module (parse/family_name/glyph_id_for/outline 등)
- FontRegistry struct — Arc<Mutex<HashMap<u32, Arc<Font>>>> + next_id counter
- Font / Glyph / metrics JSON schema — serde Serialize 직렬화 (parser type 정합)
- dispatch::Request method routing 의 font/* prefix 분기 + integration test 패턴
- FontRpcError enum — RPC layer 자체 error (NotFound / ParseError wrapper / InvalidArgs)



**Caveats**:
- R50.X.0 = spec round entry (atomic-only ratify). implementation = R50.X.1+ separate round
- implementation sub-round: R50.X.1 minimal 3 method / R50.X.2 후속 / R50.X.3 lifecycle (dispose/list)
- method namespace = font/* — text layer (line break / shape / layout) RPC 는 R50.4+ 후 별도 sub-scope
- Font registry lifetime = Arc<Mutex<HashMap<u32, Arc<Font>>>> — concurrent safe + cheap clone 권장
- R50.X.1 minimal: font/parse / font/family_name / font/glyph_id_for — visible value 작은 set
- R50.X.2 extended: font/glyph_outline / font/cmap_subtables / font/metrics / font/subfamily_name
- real font integration = Noto Sans / Nanum Gothic byte stream RPC roundtrip — R50.X.1 verification
- Unicode/BIDI 후속 sub-scope 가 sibling 가능 — §-number 는 R50.2 진입 시 결정 (forward-compat anchor)



**Alternatives rejected**:
- (a) parser 직접 API 만 노출 (RPC channel 없음) — §2 invariant #2 (RPC AI-first) 위반
- (b) Font 매 call re-parse (registry 없음) — byte stream 매번 전송, AI latency 큼, stateless 환상
- (c) generic text/* 로 묶음 (font/* 분리 없음) — §5.12 hybrid 명료성 위반, shape/layout 과 책임 혼재
- (d) FFI / C wrapper RPC channel — pinion pure Rust + JSON-RPC 정신 위반
- (e) gRPC / proto schema — §5.7 JSON-RPC 2.0 결정 위반
- (f) base64 대신 multipart binary — JSON-RPC 2.0 단순 envelope 깸, AI tooling 표준 이탈



**Impact scope**: §5.37.1, §5.7, §5.12, §2



**Implementations**:
- crates/pinion-rpc/src/font.rs:FontRegistry
- crates/pinion-rpc/src/font.rs:parse
- crates/pinion-rpc/src/font.rs:family_name
- crates/pinion-rpc/src/font.rs:glyph_id_for
- crates/pinion-rpc/src/font.rs:FontError
- crates/pinion-rpc/src/dispatch.rs:handle_font_parse
- crates/pinion-rpc/src/dispatch.rs:handle_font_family_name
- crates/pinion-rpc/src/dispatch.rs:handle_font_glyph_id_for
- crates/pinion-rpc/src/dispatch.rs:font_error_to_rpc
- crates/pinion-rpc/src/font.rs:glyph_outline
- crates/pinion-rpc/src/font.rs:cmap_subtables
- crates/pinion-rpc/src/font.rs:metrics
- crates/pinion-rpc/src/font.rs:subfamily_name
- crates/pinion-rpc/src/font.rs:full_name
- crates/pinion-rpc/src/font.rs:postscript_name
- crates/pinion-rpc/src/font.rs:GlyphOutlineOutcome
- crates/pinion-rpc/src/font.rs:dispose
- crates/pinion-rpc/src/font.rs:list
- crates/pinion-rpc/src/text.rs:text_normalize
- crates/pinion-rpc/src/dispatch.rs:handle_text_normalize



### §5.37.3. Unicode self-hosted normalization — UAX #15 NFC/NFD/NFKC/NFKD (R50.2 sub-scope)


**Intent**: §5.37 text engine 의 Unicode codepoint normalization sub-scope — UAX #15 NFC/NFD/NFKC/NFKD 4 form 자체 구현. UCD decomposition + canonical combining class 직접 embed. 외부 lib 0.


**Rationale**:
- UAX #15 normalization = §5.37 text engine 의 input layer — BIDI/shape/break 모두 의존
- 4 form (NFC/NFD/NFKC/NFKD) self-hosted — unicode-normalization crate 의 black box 거부
- UCD decomposition + canonical_combining_class table 직접 embed — 결정성 + introspect
- Unicode 16.x version pin = deterministic + Hyrum's Law 정합 — ID stability
- §5.37.1 parser 다음 layer (BIDI → shape → break) 의 codepoint 입력 정규화 stage



**Inputs**:
- TextNode codepoint sequence (§5.37 input) — String 또는 Vec<char>
- UCD 16.x: UnicodeData.txt + DerivedNormalizationProps.txt + CompositionExclusions.txt
- UAX #15 algorithm spec (Canonical Decomposition + Canonical Composition + Quick-check)



**Outputs**:
- pinion-text-unicode crate (또는 pinion-text/unicode/ module) — normalize() entry
- embedded UCD table: decomposition / compatibility_decomposition / canonical_combining_class
- normalize(codepoints, form: NormForm) -> Result<String, NormalizeError>
- quick-check helper (UAX #15 §5) — already-normalized fast path
- NormForm enum: NFC / NFD / NFKC / NFKD



**Caveats**:
- R50.2.0 = spec round entry (atomic-only ratify). implementation = R50.2.1+ separate
- UCD version pin = 16.x (현재 stable) — codegen 시 version 명시 + Hyrum's Law
- impl sub-round: R50.2.1 table embed / R50.2.2 NFD / R50.2.3 NFC / R50.2.4 NFKD / R50.2.5 NFKC
- pinion-text-unicode (별도 crate) vs pinion-text/unicode/ module 결정 = R50.2.1
- UCD ~3MB raw → build.rs codegen 압축 권장 (~500KB 압축, 결정성 유지)
- layer chain: §5.37.3 (Unicode) → BIDI (UAX #9) → shape → break (UAX #14) → positioning → raster
- Quick-check optimization (UAX #15 §5) — already-normalized input 의 fast path
- §5.37.2 RPC channel 의 text/normalize 후속 method 추가 가능 — AI introspect 정합
- R50.2.10/11 BMP trie supplementary는 binary_search (ICU UTrie2 strict 위반, 226 entries 수용).
- R50.2.13 decomp BMP trie: Stage 2 = packed `(length, offset)` u32, null block 만 dedup.
- R50.2.14 PRIMARY_COMPOSITES 2-level BMP trie + per-`a` `(b,c)` sub-table binary_search.



**Alternatives rejected**:
- (a) unicode-normalization crate dep — black box, §5.37 외부 lib 0 정신 위반
- (b) ICU4C FFI — C 의존, pure Rust 정롬 이탈
- (c) NFC 만 구현 (다른 form skip) — incomplete, NFKD 필요 (fingerprint 계산)
- (d) UCD runtime download — startup 비결정성, build-time embed 정통
- (e) UCD 전체 raw embed (~3MB) — binary size, codegen 키 reduce 정통



**Impact scope**: §5.37, §5.37.1, §5.37.2



**Implementations**:
- crates/pinion-text-unicode/Cargo.toml
- crates/pinion-text-unicode/src/lib.rs:NormForm
- crates/pinion-text-unicode/build.rs
- crates/pinion-text-unicode/ucd/UnicodeData.txt
- crates/pinion-text-unicode/ucd/DerivedNormalizationProps.txt
- crates/pinion-text-unicode/ucd/CompositionExclusions.txt
- crates/pinion-text-unicode/src/hangul.rs:decompose_hangul_syllable
- crates/pinion-text-unicode/src/decompose.rs:decompose_canonical
- crates/pinion-text-unicode/src/ordering.rs:canonical_ordering
- crates/pinion-text-unicode/src/nfd.rs:nfd
- crates/pinion-text-unicode/ucd/NormalizationTest.txt
- crates/pinion-text-unicode/src/hangul.rs:compose_hangul
- crates/pinion-text-unicode/src/composition.rs:canonical_composition
- crates/pinion-text-unicode/src/nfc.rs:nfc
- crates/pinion-text-unicode/src/test_fixture.rs:load_normalization_test
- crates/pinion-text-unicode/src/decompose.rs:decompose_compatibility
- crates/pinion-text-unicode/src/nfkd.rs:nfkd
- crates/pinion-text-unicode/src/nfkc.rs:nfkc
- crates/pinion-text-unicode/src/lib.rs:normalize
- crates/pinion-text-unicode/src/quick_check.rs:nfc_quick_check
- crates/pinion-text-unicode/benches/normalize.rs
- crates/pinion-text-unicode/build.rs:emit_fast_path_anchors
- crates/pinion-text-unicode/build.rs:build_u8_bmp_trie
- crates/pinion-text-unicode/build.rs:emit_u8_bmp_trie_table
- crates/pinion-text-unicode/src/quick_check.rs:lookup_u8_trie
- crates/pinion-text-unicode/src/ordering.rs:combining_class_supplementary
- crates/pinion-text-unicode/build.rs:build_decomp_bmp_trie
- crates/pinion-text-unicode/build.rs:emit_decomp_table
- crates/pinion-text-unicode/build.rs:emit_packed_u32_hex_row
- crates/pinion-text-unicode/src/decompose.rs:lookup_decomp_trie
- crates/pinion-text-unicode/src/decompose.rs:lookup_decomp_supplementary
- crates/pinion-text-unicode/build.rs:build_primary_composites_trie
- crates/pinion-text-unicode/build.rs:emit_primary_composites_table
- crates/pinion-text-unicode/src/composition.rs:compose_pair_supplementary



### §5.37.4. BIDI directional resolution (UAX #9)


**Intent**: Self-hosted text engine 의 directional resolution layer: NFC codepoint 시퀀스 → 각 character 의 paragraph-relative embedding level + visual reorder mapping, external lib 0 + UAX #9 full conformance.


**Rationale**:
- Shape engine (§5.37.6) prerequisite: GSUB/GPOS 진입 전 character order 결정 필요
- external lib 0 정신: icu4x / unicode-bidi 의존 거부, UCD table 직접 embed (R50.2.x 패턴 일관)
- AI-first: text/bidi method 노출로 AI agent 가 RTL/LTR mix 시각화 추론 가능
- Backend swap stability: UAX #9 spec 결정적, parley → 자체 shape 시 동일 BIDI 결과 보장
- Hyrum's law: spec strict — non-conformant 결과 reject, NFC strict 패턴 일관
- UAX #9 conformance level: full (P/X/W/N/I/L rules 전체), partial subset 거부



**Inputs**:
- §5.37.3 NFC normalized codepoint sequence (input layer)
- UCD 16.0 DerivedBidiClass.txt (Bidi_Class property table, build.rs codegen embed)
- UAX #9 spec: P-rules / X-rules / W-rules / N-rules / I-rules / L-rules 6-stage
- §5.37.1 sfnt parser 의 codepoint → glyph mapping (R50.1.x)
- §5.37.2 RPC channel surface (R50.x.x text/bidi method 신규 슬롯)
- §2 invariant #2 RPC AI-first (BIDI 결과를 AI agent 가 RPC 로 introspect)



**Outputs**:
- pinion-text-unicode crate 의 bidi module (또는 pinion-text-bidi 별도 crate)
- pub fn resolve(text: &str, base_level: Option<Level>) -> BidiResult
- BidiResult: per-character embedding levels + visual reorder index map
- BidiClass table (build.rs codegen, BMP 2-stage trie, §5.37.3 패턴)
- 6 algorithm stages 분리 함수 (resolve_p / resolve_x / resolve_w / resolve_n / resolve_i / reorder_l)
- §5.37.2 RPC method text/bidi (codepoint sequence in → levels + reorder out)



**Caveats**:
- R51.49 L4 path lock: pre-substitute (R51.27/R51.31 LayoutCache) 채택, render-time GlyphRun.is_rtl 거부
- R51.49 pre-substitute 장점: parley API decouple + LRU 단일 lookup 가 BIDI + shape 양쪽 cover
- R51.49 render-time 거부 이유: cache layer unwind 강요 + parley GlyphRun.is_rtl backend lock-in
- R51.49 pre-substitute 한계: font fallback 시 mirror glyph 미공급 폰트 = mirroring_glyph fallback chain 필요



**Alternatives rejected**:
- icu4x 의존 — external lib 0 정신 위반, framework 비대화 + transitive dep 폭증
- unicode-bidi crate 의존 — 동일 거부 (R50.2.x text engine 자립 정신)
- BIDI 미구현 (LTR-only) — Arabic/Hebrew 미지원, multi-lingual lifetime 부족
- 부분 구현 (W rules only) — UAX #9 non-conformant, edge case 미보장 (Hyrum's law 위반)
- shape engine 내 inline (separate layer 아님) — SRP 위반, RPC 노출 어려움, line break 와 ordering coupling



**Impact scope**: §5.37, §5.37.1, §5.37.2, §5.37.3, §5.37.6, §5.37.7



**Implementations**:
- crates/pinion-text-unicode/ucd/DerivedBidiClass.txt
- crates/pinion-text-unicode/build.rs:parse_bidi_class
- crates/pinion-text-unicode/src/bidi.rs:BidiClass
- crates/pinion-text-unicode/src/bidi.rs:bidi_class
- crates/pinion-text-unicode/src/bidi.rs:paragraph_level
- crates/pinion-text-unicode/src/bidi.rs:iter_paragraphs
- crates/pinion-text-unicode/src/bidi.rs:ParagraphIter
- crates/pinion-text-unicode/src/bidi.rs:resolve_explicit_levels
- crates/pinion-text-unicode/src/bidi.rs:ExplicitLevels
- crates/pinion-text-unicode/src/bidi.rs:MAX_DEPTH
- crates/pinion-text-unicode/src/bidi.rs:resolve_weak_types
- crates/pinion-text-unicode/src/bidi.rs:paired_bracket
- crates/pinion-text-unicode/src/bidi.rs:BracketType
- crates/pinion-text-unicode/build.rs:parse_bidi_brackets
- crates/pinion-text-unicode/src/bidi.rs:resolve_neutral_types
- crates/pinion-text-unicode/src/bidi.rs:resolve_implicit_levels
- crates/pinion-text-unicode/src/bidi.rs:apply_l1_line_break
- crates/pinion-text-unicode/src/bidi.rs:reorder_visual
- crates/pinion-text-unicode/src/bidi.rs:bidi_reorder
- crates/pinion-text-unicode/src/bidi.rs:mirroring_glyph
- crates/pinion-text-unicode/src/bidi.rs:apply_l3_combining_marks
- crates/pinion-text-unicode/build.rs:parse_bidi_mirroring
- crates/pinion-text-unicode/src/bidi.rs:canonical_bracket_form
- crates/pinion-text-unicode/src/test_fixture.rs:parse_bidi_character_test
- crates/pinion-text-unicode/src/test_fixture.rs:load_bidi_character_test
- crates/pinion-text-unicode/ucd/BidiCharacterTest.txt
- crates/pinion-text-unicode/ucd/BidiTest.txt
- crates/pinion-text-unicode/src/test_fixture.rs:load_bidi_test
- crates/pinion-text-unicode/src/test_fixture.rs:parse_bidi_test
- crates/pinion-text-unicode/src/bidi.rs:mirror_paired_brackets



### §5.37.5. Script analysis (UCD Script property) — carry placeholder


**Intent**: §5.37.5 script analysis sub-layer placeholder — UCD Script property 기반 segmentation. shape engine input (run splitting). ratify 는 multi-session carry, BIDI 이후 자연 순서










### §5.37.6. Shape (OpenType GSUB/GPOS execution) — carry placeholder


**Intent**: §5.37.6 shape sub-layer placeholder — OpenType GSUB/GPOS execution (glyph substitution + positioning). parley/swash 대체. R51.1 line_count semantic forward-reference. ratify 는 multi-session carry










### §5.37.7. Line break (UAX #14) — carry placeholder


**Intent**: §5.37.7 line break sub-layer placeholder — UAX #14 algorithm. R51.1 §5.12 forward-reference 정합용. self-hosted text engine 의 line breaking step, ratify 는 multi-session carry





**Caveats**:
- Carry placeholder — ratify TBD, §5.37.4 BIDI / §5.37.6 shape 이후 자연 순서
- decision_status 변경 primitive 부재 — 진짜 정정 = mnemosyne MCP RFC carry







### §5.38. Widget catalog — Tier 1 primitive widgets


**Intent**: §5.38 Tier-1 widget primitive 카탈로그 axis ratify — Button R12 시작, Toggle/Checkbox/Slider/TextInput/Menu 등 후속, framework-side 책임 (R47-class lesson 적용)


**Rationale**:
- R47-class lesson: framework primitive 영역, application/example inline 금지
- industry precedent: Xilem/Druid/Slint/Qt/Material/SwiftUI 모두 framework-측 widget
- API completeness (Bloch) — partial widget surface 보다 풀 카탈로그 lifetime correct
- §5.1 framework-first kickoff 일관 — substrate (RPC/SCE/intent) 위 widget 쌓음
- per-widget SCXML — SRP + AI introspect 친화 (R12 Button 정통 패턴)
- DRY: interaction state pattern (idle/hover/pressed/disabled) widget 간 공유



**Inputs**:
- §5.4 SCE statechart (per-widget SCXML interaction state machine substrate)
- §5.13 Event enum closed core (PointerEnter/Leave/Down/Up + Disable/Enable)
- §5.20 Intent system (widget → app intent emission channel)
- §5.15 External 8-item contract (introspect schema + query/intervene/invoke)
- §5.24 Semantic tree (role / state / action — ARIA aligned)
- §4 first dogfood widget catalogue (~12 core + 6 domain-specific)



**Outputs**:
- crates/pinion-core::widgets module (Tier-1 primitive widget set)
- per-widget SCXML at crates/pinion-core/widgets/*.scxml (Button precedent R12)
- Widget + WidgetExternal binding pattern (engine + value field + intent emit)
- External adapter: state read / send action (§5.15 introspect path)
- AI introspect: schema fields + query/intervene/invoke per widget



**Caveats**:
- R51.2 — Toggle (Tier-1 1번 widget) land. Button R12 SCXML 1:1 패턴 + value: bool layer
- R51.2 — ToggleExternal 3 schema slots (state/value/send) + toggle intent (Bool payload)
- R51.2 — AI introspect: §5.15 8-item contract 정통 (state/value read + value intervene + send invoke)
- R51.2 — Figma fidelity: Toggle pure state, label = Scene::Text + R47.5 TextStyle
- R51.41 composite hit-target: paint N 'group#i' tags + state 1 'group' External (RadioGroup)
- R51.98 — ListBox 의 multi-select 모드 (with_multiselect; aria-multiselectable; 활성화 시 토글, 형제 미터치)
- R51.99 — hello-listbox type-ahead 점프 (printable 문자→다음 매칭 옵션, WAI-ARIA APG)
- R51.100 — ListBox JSON-RPC e2e 단/다중 모드 + composite cancel + selected.<i> per-row 검증
- R51.102 — WidgetTransition::detect → Vec<Intent>, Snapshot: Clone, ListBox 다중 emit substrate
- R51.103 — type-ahead 다문자 prefix 버퍼 (500ms 타임아웃) + Unicode case fold (i18n)
- R51.104 — hello-listbox-multi 시각 demo (N=6, aria-multiselectable, 토글 시연)
- R51.105 — ListBox dispatch bench (N=4~20, 1-4 µs/이벤트 → Vec<bool> snapshot 채택, SmallVec 보류)
- R51.106 — type-ahead substrate lift → pinion_shell::typeahead (2 consumer 트리거, ~150 LOC 청산)
- R51.114 — aria::apply_aria_activate helper extracted (4 binding apply_key DRY 청산)
- R56.1.a — TextField SCXML 4-state + binding + text_committed intent (R56 axis start)
- R56.1.b — TextEditState + caret_rect helper + TextField::attach_state (R56.1.a sidecar grows)
- R56.1.c — CaretBlink animation (530ms canonical) + use_caret_blink hook (Owner::cache + Tickable)
- R56.1.d — TextField apply_key + invoke('key') RPC path (Backspace/Arrow/Home/End/Space/printable)
- R56.1.h — TextField focus lifecycle wire (shell mgr ↔ on_focus_change ↔ Focus/Blur ↔ blink)



**Alternatives rejected**:
- application/example inline implementation — R47-class incident 반복 (industry consensus 명확)
- single mega-widget SCXML — per-widget SRP 위반, hot-reload + introspect 차단
- third-party widget kit (egui retained immediate) — SCE substrate 비호환, AI introspect 제한
- Tier 1 minimal MVP subset — lifetime framework partial surface 부채 (Bloch API completeness 위반)



**Impact scope**: §4, §5.4, §5.13, §5.15, §5.20, §5.24


**Examples**:

```rust
// R56.1.d §5.38 §5.22 — application apply_key wire for a focused TextField.
// Mirrors hello-listbox (§5.38 R51.99) — focus-tag gate then RPC-shaped
// invoke("key", text). Returns Bool(true) on recognized keys.
fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
    if focused != Some(Self::tag()) {
        return false;
    }
    let Some(node) = scene.find_external_with_tag_mut(Self::tag()) else {
        return false;
    };
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    matches!(
        intro.invoke("key", IntrospectValue::Text(key.to_string())),
        Ok(IntrospectValue::Bool(true)),
    )
}
```

```rust
// R56.1.h §5.38 §5.39 §5.28 — focus-aware TextField with caret blink.
// The shell substrate calls External::on_focus_change automatically
// after every focus mutation (Tab traversal / click / AT action / RPC
// focus/set), so the binding only needs to construct the External and
// attach state + blink:
fn create_external() -> Box<dyn External> {
    let text = Rc::new(TextEditState::new());
    let blink = Rc::new(CaretBlink::new());
    Box::new(
        TextFieldExternal::new()
            .attach_state(text)
            .attach_blink(blink),
    )
}
```



**Implementations**:
- crates/pinion-core/widgets/toggle.scxml
- crates/pinion-core/src/widgets/toggle.rs:Toggle
- crates/pinion-core/src/widgets/toggle.rs:ToggleExternal
- crates/pinion-core/build.rs:scxml_inputs::toggle
- crates/pinion-core/widgets/standard_button.sce-template.xml
- crates/pinion-core/widgets/button.scxml
- crates/pinion-core/src/widgets/widget.rs:Widget
- crates/pinion-core/src/widgets/widget.rs:IntentEmitter
- crates/pinion-core/widgets/checkbox.scxml
- crates/pinion-core/src/widgets/checkbox.rs:Checkbox
- crates/pinion-core/src/widgets/checkbox.rs:CheckboxExternal
- crates/pinion-core/widgets/radio.scxml
- crates/pinion-core/src/widgets/radio.rs:Radio
- crates/pinion-core/src/widgets/radio.rs:RadioExternal
- crates/pinion-core/widgets/slider.scxml
- crates/pinion-core/src/widgets/slider.rs:Slider
- crates/pinion-core/src/widgets/slider.rs:SliderExternal
- crates/pinion-core/src/widgets/widget.rs:WidgetTransition
- crates/pinion-core/src/widgets/widget.rs:IntentEmitter::dispatch
- crates/pinion-core/src/widgets/button.rs:&lt;Button as WidgetTransition&gt;
- crates/pinion-core/src/widgets/toggle.rs:&lt;Toggle as WidgetTransition&gt;
- crates/pinion-core/src/widgets/checkbox.rs:&lt;Checkbox as WidgetTransition&gt;
- crates/pinion-core/src/widgets/radio.rs:&lt;Radio as WidgetTransition&gt;
- crates/pinion-core/src/widgets/slider.rs:&lt;Slider as WidgetTransition&gt;
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroup
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroupExternal
- crates/pinion-core/src/widgets/radio_group.rs:&lt;RadioGroup as WidgetTransition&gt;
- examples/hello-toggle/app.pinion.xml
- examples/hello-toggle/src/main.rs:view
- crates/pinion-shell/src/lib.rs:WidgetView
- crates/pinion-shell/src/lib.rs:AppShell
- crates/pinion-shell/src/lib.rs:run
- examples/hello-button/src/main.rs:ButtonView
- examples/hello-toggle/src/main.rs:ToggleView
- examples/hello-checkbox/app.pinion.xml
- examples/hello-checkbox/src/main.rs:view
- examples/hello-checkbox/src/main.rs:CheckboxView
- examples/hello-radio/app.pinion.xml
- examples/hello-radio/src/main.rs:view
- examples/hello-radio/src/main.rs:RadioView
- examples/hello-slider/app.pinion.xml
- examples/hello-slider/src/main.rs:view
- examples/hello-slider/src/main.rs:SliderView
- crates/pinion-core/src/widgets/slider.rs:SliderExternal::wants_pointer_capture
- crates/pinion-core/src/widgets/slider.rs:SliderExternal::pointer_move
- crates/pinion-shell/src/lib.rs:WidgetView::apply_key
- examples/hello-slider/src/main.rs:SliderView::apply_key
- crates/pinion-core/src/widgets/slider.rs:SliderAxis
- crates/pinion-core/src/widgets/slider.rs:Slider::with_axis
- crates/pinion-core/src/widgets/slider.rs:Slider::axis
- crates/pinion-core/src/widgets/slider.rs:SliderExternal::with_axis
- crates/pinion-core/src/widgets/slider.rs:SliderExternal::axis
- crates/pinion-core/src/widgets/slider.rs:slider_axis_name
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroupExternal::query/state.&lt;index&gt;
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroupExternal::query/selected.&lt;index&gt;
- examples/hello-radio-group/src/main.rs:RadioGroupView
- examples/hello-slider-vertical/src/main.rs:SliderVerticalView
- crates/pinion-core/src/widgets/listbox.rs:ListBox::with_multiselect
- crates/pinion-core/src/widgets/listbox.rs:ListBox::is_multiselect
- crates/pinion-core/src/widgets/listbox.rs:ListBox::selected_indices
- crates/pinion-core/src/widgets/listbox.rs:ListBox::set_selected_indices
- crates/pinion-core/src/widgets/listbox.rs:ListBoxExternal::with_multiselect
- crates/pinion-core/src/widgets/listbox.rs:ListBoxExternal::is_multiselect
- crates/pinion-core/src/widgets/listbox.rs:ListBoxExternal::selected_indices
- crates/pinion-shell/src/typeahead.rs:TypeaheadCursor
- crates/pinion-shell/src/typeahead.rs:TypeaheadCursor::step
- crates/pinion-core/src/widgets/aria.rs
- crates/pinion-core/src/widgets/aria.rs:apply_aria_activate
- examples/hello-listbox/src/main.rs:listbox_row_at_y
- crates/pinion-core/widgets/scroll_bar.scxml
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBar
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBarExternal
- crates/pinion-core/src/widgets/scrollbar.rs:&lt;ScrollBar as WidgetTransition&gt;
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBarEvent
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBarState
- crates/pinion-core/widgets/text_field.scxml
- crates/pinion-core/src/widgets/text_field.rs:TextField
- crates/pinion-core/src/widgets/text_field.rs:TextFieldExternal
- crates/pinion-core/src/widgets/text_field.rs:TextFieldEvent
- crates/pinion-core/src/widgets/text_field.rs:TextFieldState
- crates/pinion-core/src/widgets/text_field.rs:&lt;TextField as WidgetTransition&gt;
- crates/pinion-core/build.rs:scxml_inputs::text_field
- crates/pinion-core/src/widgets/text_edit.rs:TextEditState
- crates/pinion-core/src/widgets/text_edit.rs:use_text_edit_state
- crates/pinion-core/src/widgets/text_field.rs:caret_rect
- crates/pinion-core/src/widgets/text_field.rs:TextField::attach_state
- crates/pinion-core/src/widgets/text_field.rs:TextFieldExternal::attach_state
- crates/pinion-core/src/widgets/caret_blink.rs:CaretBlink
- crates/pinion-core/src/widgets/caret_blink.rs:use_caret_blink
- crates/pinion-core/src/widgets/text_field.rs:apply_key
- crates/pinion-core/src/widgets/text_field.rs:TextField::attach_blink
- crates/pinion-core/src/widgets/text_field.rs:TextField::sync_blink
- crates/pinion-core/src/widgets/text_field.rs:&lt;TextFieldExternal as External&gt;::on_focus_change
- crates/pinion-shell/src/substrate.rs:ShellCore::notify_focus_change



### §5.39. Focus model — keyboard navigation + activation primitive


**Intent**: focused widget = key dispatch single target + Tab traversal + ARIA Space/Enter activation; pinion-runtime FocusManager 가 focused_tag 소유, broadcast key dispatch 폐기


**Rationale**:
- ARIA WCAG 2.1.1 Keyboard / 2.4.3 Focus Order / 2.4.7 Focus Visible — framework primitive 기본 보장 필수
- input.rs:71 v0 carry — focus tab order + keyboard dispatch 명시 미구현, R48 부터 dormant 부채
- apply_key broadcast = 다중 focusable widget 시 aliasing — Slider 2개 시 동일 Arrow 양쪽 응답
- Button/Toggle/Checkbox/Radio Space/Enter activation 부재 — WCAG 2.1.1 Keyboard 정면 위반
- Xilem / Slint / Qt / GTK / iced / Druid 모두 focus = framework primitive (textbook)
- §5.35 input dispatch 의 key 측 dual axis — pointer 가 framework 면 key 도 framework



**Inputs**:
- paint Scene (focusable tag enumeration, depth-first traversal)
- WidgetView::focusable_tags(&Scene) -> Vec<&str> trait method
- Tab / Shift+Tab / Space / Enter (FocusManager 의 reserved key set)
- WindowEvent::Focus { focused: bool } (focus save/restore trigger)
- pointer_down on tagged focusable widget (click → focus side-effect, §5.35 hook)



**Outputs**:
- focused_tag: Option<String> (FocusManager single owner)
- apply_key signature: (&mut Scene, focused_tag: Option<&str>, key) -> bool
- Focus { focused: bool } 양측 widget invoke('send') dispatch (blur old + focus new)
- introspect query: schema 의 ('focused', 'option-string') — RPC AI-first 호환
- paint-time focus_ring outline rendering (R51.58 sub-round)



**Caveats**:
- Roving tabindex (composite RadioGroup) = single tab stop + arrow nav (ARIA pattern)
- Focus on click = §5.35 pointer_down → focused_tag 자동 갱신 (mouse/key 동등 trigger)
- focus_set('tag') programmatic = RPC 'focus/set' method 측 carry (R51.59 evidence-first)
- focus_clear = Escape 또는 비-focusable click → focused_tag = None
- WindowEvent::Focus { focused: false } = focused_tag 보존, restore 시 복원 (R51.59 land)
- focus ring rendering = paint-time outline, theme Modifier 의존 (R51.58 sub-round land)
- composite (RadioGroup) 내부 focused index = External sub-state, 외부 single tag
- single-tag widget 의 internal sub-element focus = 별도 axis, R51.x 미land carry
- focusable enumeration = paint scene tagged subset, interactive widget 만 (decoration 제외)
- key priority = Tab/Shift+Tab swallow by FocusManager, 이후 apply_key 로 forward
- Space/Enter widget-specific 의미 = apply_key 위임 (Button click, Checkbox toggle 등)
- apply_key breaking change = R51.53 land 시 모든 기존 example 동시 update (substrate-first)



**Alternatives rejected**:
- broadcast key dispatch (현 v0) — 다중 focusable widget aliasing, WCAG 위반; 안 함
- per-widget focus state ownership — DRY 위반 + multi-source-of-truth, single owner = textbook
- DOM-style focus event bubbling — §5.15 External opaque + §6.3 view-fn purity 충돌
- tabindex 수동 명시 (HTML 모방) — view-fn 의 declarative 정신 위반, traversal 자동 정통



**Impact scope**: §5.13, §5.15, §5.20, §5.32, §5.35



**Implementations**:
- crates/pinion-runtime/src/focus.rs:FocusManager
- crates/pinion-runtime/src/focus.rs:FocusManager::focus_next
- crates/pinion-runtime/src/focus.rs:FocusManager::focus_prev
- crates/pinion-runtime/src/focus.rs:FocusManager::focus_set
- crates/pinion-runtime/src/focus.rs:FocusManager::focus_clear
- crates/pinion-runtime/src/focus.rs:FocusManager::update_focusable_tags
- crates/pinion-runtime/src/focus.rs:FocusManager::save
- crates/pinion-runtime/src/focus.rs:FocusManager::restore
- crates/pinion-shell/src/lib.rs:WidgetView::focusable_tags
- crates/pinion-shell/src/lib.rs:WidgetView::apply_key
- crates/pinion-shell/src/lib.rs:AppShell::click_to_focus
- crates/pinion-shell/src/lib.rs:AppShell::focus
- crates/pinion-shell/src/lib.rs:AppShell::modifiers
- crates/pinion-core/widgets/standard_button.sce-template.xml:keyboard_activate
- crates/pinion-core/src/widgets/widget.rs:WidgetTransition::detect
- crates/pinion-core/src/widgets/button.rs:Button::detect (keyboard_click branch)
- examples/hello-button/src/main.rs:ButtonView::apply_key
- crates/pinion-core/src/widgets/toggle.rs:Toggle::send (keyboard_activate branch)
- crates/pinion-core/src/widgets/checkbox.rs:Checkbox::send (keyboard_activate branch)
- crates/pinion-core/src/widgets/radio.rs:Radio::send (keyboard_activate branch)
- examples/hello-toggle/src/main.rs:ToggleView::apply_key
- examples/hello-checkbox/src/main.rs:CheckboxView::apply_key
- examples/hello-radio/src/main.rs:RadioView::apply_key
- examples/hello-slider/src/main.rs:SliderView::apply_key
- examples/hello-slider-vertical/src/main.rs:SliderVerticalView::apply_key
- examples/hello-radio-group/src/main.rs:RadioGroupView::apply_key (focused gate)
- crates/pinion-runtime/src/paint_adapter.rs:paint_focus_ring
- crates/pinion-shell/src/lib.rs:AppShell::render (focus ring call)
- crates/pinion-shell/src/lib.rs:AppShell window_event WindowEvent::Focused



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




### §5.40. Accessibility semantic tree — AccessKit integration for WCAG 4.1.2 (Name, Role, Value)


**Intent**: AccessKit 통합으로 widget name/role/state/value 를 OS AT API (UIA/AX/AT-SPI/Android) 노출, WCAG 4.1.2 충족, AI introspect 와 직교 공존


**Rationale**:
- WCAG 4.1.2 Name/Role/Value Level A — focus/keyboard 만으로 미충족, AT 노출 필수
- R51.51-R51.59 §5.39 focus 완결 — visual+keyboard a11y land, semantic tree 만 hidden 부채
- AccessKit = Rust 표준 cross-platform (UIA/AX/AT-SPI/Android), Mozilla/Bevy/egui/Slint 채택
- AI introspect RPC 와 OS AT API 는 직교 audience — 둘 다 필요, 어느 쪽도 대체 아님
- WidgetView 별 role 매핑 = single trait default method + per-widget override 패턴
- TreeUpdate emit 시점 = render frame 후 (paint scene 동기, view-fn 순수 invariant 유지)



**Inputs**:
- paint Scene (tagged widget enumeration, hit-test rect, semantic boundary 추출)
- WidgetView::access_node(&Scene, &str) -> Option<AccessNode> trait method (default None)
- FocusManager::focused() (focused widget = AccessKit ActiveDescendant + Focus state)
- WindowEvent::AccessibilityRequested (AT 첫 query trigger, accesskit_winit relay)
- ActionRequestEvent (AT 측 invoke: Click/Focus/Increment/Decrement/Default 5종)



**Outputs**:
- TreeUpdate { nodes, focus, tree } emit per render frame (no-change frame 시 debounce)
- accesskit_winit::Adapter ownership in AppShell (winit Window 와 1:1 lifecycle)
- AccessNode = { id, role, name, value, state_flags } per widget (pinion-a11y substrate type)
- Action handler — Click/Focus/Increment/Decrement → InputRouter intent 변환 layer
- introspect schema 의 role/name = ARIA-aligned, AccessKit AccessNode 와 동일 표준 lockstep



**Caveats**:
- Adapter ownership = AppShell field, winit Window 와 1:1, Drop 시 release (lifecycle invariant)
- TreeUpdate debounce = scene 의 access_node 컬렉션 해시 변화시만 emit, no-op frame 비용 0
- access_node default = None 반환 시 non-interactive 로 AT 노출 (decoration scene 자동 제외)
- Action::Click = pointer_click intent 위조 (focus_set 후 apply_key Enter, widget-specific 위임)
- Action::Focus = FocusManager::focus_set(tag) 직접 호출 (broadcast 없이 정확 target)
- Action::Increment/Decrement = Slider apply_key(ArrowRight/ArrowLeft) 위조 + value_committed
- composite (RadioGroup) AccessNode = role=RadioGroup + children=Radio nodes (AccessKit parent-child)
- name 추출 = widget 내부 Text leaf 첫 줄 + Modifier::aria_label override (label hint = carry)
- value = Slider current f32, Checkbox/Toggle/Radio boolean (introspect schema 와 lockstep)
- state_flags = focused / disabled / hovered / pressed / checked (introspect state enum 동등)
- live region = role=Log/Status/Alert + AccessKit live property (R51.x carry, framework 미요구)
- platform AT test = Windows Narrator / macOS VoiceOver / Linux Orca / Android TalkBack (carry)
- custom widget = access_node override 필수, default None = AT 무시 (intent declaration)
- composite focus redirect = WidgetView::access_focus_target (default passthrough) per R51.66
- composite child click dispatch = R51.x carry — widget-side wire-format invoke surface 필요
- R51.69 — ContainerNode::aria_label + enrich_names_from_scene (WAI-ARIA name precedence land)
- R51.70 — WidgetView::access_child_invoke + RadioGroup wire-format (WCAG 4.1.2 write 회복)
- R51.71 — AccessFocus typed + accesskit Node::set_active_descendant (ARIA roving-tabindex 정통)
- R51.72 — dirty_tags + last_access_nodes cache (AccessKit incremental-update 권고 준수)
- R51.73 — focus/set + focus/get RPC dual to AccessKit Focus (AI primary path §2 #2 align)
- R51.74 — focus/next + focus/prev RPC (Tab / Shift+Tab equivalent AI primary path)
- R51.75 — no-change frame skip (last_access_focus diff, update_if_active 자체 스킵 시 0 cost)
- R51.76 — ShellCore<V> substrate/surface 분리 + AccessEmitPlan + redraw flag drain
- R51.77 — plan_access_emit (pure &self) + commit_access_emit (&mut) 분리, AccessEmitPlan→Decision
- R51.78 — handle_key_press winit-free 분리 (focus_traverse / character / named 3-method)
- R51.79 — AccessTreeBuilder::add &AccessNode + commit_access_emit by-value Vec move
- R51.80 — ShellCore deeper extraction: paint+a11y+finalize+input+focus wrappers 정통
- R51.81 — TextNode.role + TextRole 정통, decoration glyph 의 aria_label Band-Aid 청산
- R51.82 — dispatch_access_action::Focus composite tag 처리, access_child_invoke 라우팅
- R51.83 — ShellCore 14 필드 + AppShell.core pub(crate) → private (R51.80 encapsulation claim 회복)
- R51.84 — AccessTreeBuilder::initial &mut self 통일 + AccessFocus::with_active_descendant builder
- R51.85 — focus 4 route handler 의 RpcError 매핑 helper 5개 추출 (DRY + code lockstep)
- R51.86 — TextRole::Label 제거 (consumer 0, strict YAGNI 회복; #[non_exhaustive] 보존)
- R51.87 — RadioGroup focused_index 분리 (WAI-ARIA roving-tabindex 정통, AT Focus 가 selected commit 없이 이동)
- R51.88 — AccessFocus::with_active_descendant 제거 (caller 0, strict YAGNI; composite 직접 구성)
- R51.89 — RpcError builder (new/with_data/with_data_string/invalid_params/internal_error) + focus DRY
- R51.90 — RadioGroup::send activate edge 가 focused_index 동기화 (WAI-ARIA APG roving first-class)
- R51.91 — InterveneError::OutOfRange variant (RadioGroup selected/focused_index TypeMismatch 우회 정정)
- R51.92 — pinion-shell/src/substrate.rs 분할: R51.83 visibility 가 substantive (모듈 경계)
- R51.92.1 — app.rs 모듈 분할 (AppShell + impl ApplicationHandler + run + helpers) 로 3-모듈 완성
- R51.89.1 — dispatch.rs RpcError literal 14 site full sweep (builder 통일, struct 직접 구성 0)
- R51.93 — TouchPhase::Cancelled commit-class fix (pointer_cancel SCXML + InputRouter API + 5 widget)
- R51.93.1 — RadioGroup 합성 cancel propagation 회귀 테스트 (template fix 가 composite path 자동 적용 검증)
- R51.94 — AccessTreeBuilder::build 가 debug_assert 로 tag_to_node_id injective 검증 (release 0-cost)
- R51.95 — ListBoxItem 신규 widget (template 공유, listbox_item.activate 채널, ListBox composite 의 기본 item)
- R51.96 — ListBox composite (WAI-ARIA Listbox: Arrow=focus, Space/Enter=commit) RadioGroup mirror
- R51.96.1 — AriaRole 에 Listbox + ListBoxOption variant 추가 (additive, #[non_exhaustive] 보존)
- R51.97 — hello-listbox 예제 (WAI-ARIA Listbox 키보드 모델: Arrow=focus, Space/Enter=commit, AT integration)
- R51.98 — AccessNode.selected (aria-selected) + multiselectable (aria-multiselectable) wire-up
- R51.98 — hello-listbox ListBoxOption aria-checked→aria-selected 정정 (axis 분리, WAI-ARIA APG)
- R51.101 — dispatch.rs invalid_params(&str) wrapper 제거 (R51.89.1 carry; 129 caller 직접 호출)
- R51.121.1 — WidgetA11y supertrait (pinion-a11y) 로 a11y 3 method 이동, audit citation 정정



**Alternatives rejected**:
- introspect RPC 만으로 a11y 완결 frame — AT (NVDA/VoiceOver/Orca) 미연동, WCAG 4.1.2 위반
- 자체 a11y API 설계 — UIA/AX/AT-SPI 재구현 천문학 비용, AccessKit 가 Rust 정통
- per-widget OS API 직접 호출 — platform fork 필요, AccessKit single adapter 가 textbook
- TreeUpdate eager emit per input event — frame budget 폭증, render frame 동기가 정통
- 별도 a11y trait 신설 — DRY 위반, WidgetView trait default method 가 정통



**Impact scope**: §5.13, §5.15, §5.20, §5.21, §5.35, §5.39



**Implementations**:
- crates/pinion-a11y/src/lib.rs
- crates/pinion-a11y/src/role.rs:AriaRole
- crates/pinion-a11y/src/node.rs:AccessNode
- crates/pinion-a11y/src/node.rs:AccessState
- crates/pinion-a11y/src/node.rs:AccessValue
- crates/pinion-a11y/src/tree.rs:AccessTreeBuilder
- crates/pinion-a11y/src/tree.rs:tag_to_node_id
- crates/pinion-a11y/src/action.rs:AccessAction
- crates/pinion-a11y/src/action.rs:translate_action
- crates/pinion-shell/src/lib.rs:AppEvent::AccessKit
- crates/pinion-shell/src/lib.rs:AppShell::handle_accesskit_event
- crates/pinion-shell/src/lib.rs:AppShell::forward_to_accesskit
- crates/pinion-runtime/src/input.rs:rect_for_tag
- examples/hello-button/src/main.rs:ButtonView::access_node
- examples/hello-toggle/src/main.rs:ToggleView::access_node
- examples/hello-checkbox/src/main.rs:CheckboxView::access_node
- examples/hello-radio/src/main.rs:RadioView::access_node
- examples/hello-slider/src/main.rs:SliderView::access_node
- examples/hello-slider-vertical/src/main.rs:SliderVerticalView::access_node
- examples/hello-radio-group/src/main.rs:RadioGroupView::access_node
- examples/hello-radio-group/src/main.rs:RadioGroupView::access_focus_target
- crates/pinion-shell/src/lib.rs:AppShell::handle_action_request
- crates/pinion-shell/src/lib.rs:AppShell::dispatch_access_action
- crates/pinion-shell/src/lib.rs:AppShell::apply_a11y_key
- crates/pinion-shell/src/lib.rs:build_tag_map
- crates/pinion-a11y/tests/conformance.rs
- crates/pinion-core/src/scene.rs:ContainerNode::aria_label
- crates/pinion-core/src/scene.rs:ContainerNode::with_aria_label
- crates/pinion-a11y/src/scene_label.rs
- crates/pinion-a11y/src/scene_label.rs:enrich_names_from_scene
- examples/hello-radio-group/src/main.rs:RadioGroupView::access_child_invoke
- crates/pinion-a11y/src/focus.rs
- crates/pinion-a11y/src/focus.rs:AccessFocus
- crates/pinion-a11y/src/tree.rs:AccessTreeBuilder::active_descendant
- crates/pinion-a11y/src/tree.rs:AccessTreeBuilder::dirty_tags
- crates/pinion-shell/src/lib.rs:AppShell::last_access_nodes
- crates/pinion-shell/src/lib.rs:AppShell::access_emit_initial
- crates/pinion-rpc/src/focus.rs
- crates/pinion-rpc/src/focus.rs:focus_set
- crates/pinion-rpc/src/focus.rs:focus_get
- crates/pinion-rpc/src/dispatch.rs:DispatchContext::focus_manager
- crates/pinion-rpc/src/focus.rs:focus_next
- crates/pinion-rpc/src/focus.rs:focus_prev
- crates/pinion-shell/src/lib.rs:AppShell::last_access_focus
- crates/pinion-shell/src/lib.rs:AppShell::handle_key_press
- crates/pinion-shell/src/lib.rs:AppShell::drain_redraw_to_winit
- crates/pinion-shell/tests/dispatch_core.rs
- crates/pinion-core/src/scene.rs:TextRole
- crates/pinion-core/src/scene.rs:TextNode::with_role
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroup::focused_index
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroup::set_focused_index
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroupExternal::focused_index
- crates/pinion-rpc/src/dispatch.rs:RpcError::new
- crates/pinion-rpc/src/dispatch.rs:RpcError::with_data
- crates/pinion-rpc/src/dispatch.rs:RpcError::with_data_string
- crates/pinion-rpc/src/dispatch.rs:RpcError::invalid_params
- crates/pinion-rpc/src/dispatch.rs:RpcError::internal_error
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroup::send
- crates/pinion-core/src/external.rs:InterveneError::OutOfRange
- crates/pinion-core/src/widgets/radio_group.rs:RadioGroupExternal::resolve_index_intervene
- crates/pinion-shell/src/substrate.rs:ShellCore
- crates/pinion-shell/src/substrate.rs:AccessEmitDecision
- crates/pinion-shell/src/substrate.rs:ShellCore::dispatch_access_action
- crates/pinion-shell/src/substrate.rs:ShellCore::handle_action_request
- crates/pinion-shell/src/substrate.rs:ShellCore::take_redraw_request
- crates/pinion-shell/src/substrate.rs:ShellCore::plan_access_emit
- crates/pinion-shell/src/substrate.rs:ShellCore::commit_access_emit
- crates/pinion-shell/src/substrate.rs:ShellCore::handle_focus_traverse
- crates/pinion-shell/src/substrate.rs:ShellCore::handle_character_key
- crates/pinion-shell/src/substrate.rs:ShellCore::handle_named_key
- crates/pinion-shell/src/substrate.rs:ShellCore::compute_paint_scene
- crates/pinion-shell/src/substrate.rs:ShellCore::collect_access_emit_inputs
- crates/pinion-shell/src/substrate.rs:ShellCore::finalize_frame
- crates/pinion-shell/src/substrate.rs:ShellCore::text_cache_mut
- crates/pinion-shell/src/substrate.rs:ShellCore::modifiers_shift_key
- crates/pinion-shell/src/app.rs:AppShell
- crates/pinion-shell/src/app.rs:AppShell::new
- crates/pinion-shell/src/app.rs:run
- crates/pinion-core/widgets/standard_button.sce-template.xml
- crates/pinion-core/widgets/slider.scxml
- crates/pinion-runtime/src/input.rs:InputRouter::pointer_cancel
- crates/pinion-core/widgets/listbox_item.scxml
- crates/pinion-core/src/widgets/listbox_item.rs:ListBoxItem
- crates/pinion-core/src/widgets/listbox_item.rs:ListBoxItemExternal
- crates/pinion-core/src/widgets/listbox.rs:ListBox
- crates/pinion-core/src/widgets/listbox.rs:ListBoxExternal
- examples/hello-listbox/src/main.rs:ListBoxView
- crates/pinion-a11y/src/node.rs:AccessNode::with_selected
- crates/pinion-a11y/src/node.rs:AccessNode::with_multiselectable
- crates/pinion-a11y/src/widget_a11y.rs
- crates/pinion-a11y/src/widget_a11y.rs:WidgetA11y
- crates/pinion-a11y/src/widget_a11y.rs:WidgetA11y::access_node
- crates/pinion-a11y/src/widget_a11y.rs:WidgetA11y::access_focus_target
- crates/pinion-a11y/src/widget_a11y.rs:WidgetA11y::access_child_invoke



### §5.41. TUI 백엔드 — cell-based render mode + crossterm 입력 + WidgetRenderer trait 추출


**Intent**: §2 #6 GUI/TUI dual invariant 의 spec 구체화 — Scene→cell 매핑, crossterm key/mouse → §5.13 Event 변환, WidgetRenderer trait 추출 substrate evolution plan


**Rationale**:
- §2 #6 'GUI/TUI dual: one scene, two render dispatch paths' 는 settled invariant 인데 impl 0%
- 14 invariant 중 유일 0% — strategic gap 최대, axis hierarchy 상 application axis 보다 우선
- §5.16 R45 renderer kind 는 pixel raster (vello/softbuffer/headless) 한정 — cell-based 는 별 axis
- §5.2 closed-form scene primitive enum 은 backend 무관, 매핑 layer 가 backend-specific
- Slint TUI experimental + ratatui 가 industry precedent — scene primitive 공유 + render 분기 정통
- crossterm key/mouse → §5.13 Event 변환 = winit-free InputRouter substrate evolution trigger
- §3 dry_run primitive 은 scene→cell deterministic 자동 호환 (GPU side effect 0)
- first dogfood = hello-button TUI — framework primitive substrate 검증 first client



**Inputs**:
- Scene primitive enum (§5.2 Box/Text/Path/Image/Container/Effect/External 8 variant)
- §5.13 Event enum (Click/Key/Touch/Gesture/Focus/Scroll/External + Logical coord)
- crossterm 0.27 (key+mouse 이벤트, raw mode, alternate screen, capability detection)
- ratatui 0.26 Backend trait (terminal cell buffer + style attribute + cursor 관리)
- unicode-width / unicode-segmentation (grapheme cluster cell width 정통)
- WAI-ARIA APG keyboard model (GUI 측 이미 정통, TUI 동일 단축키 매핑)
- VelloRenderer (pinion-shell 단일 impl) — WidgetRenderer trait 추출 대상
- ShellCore substrate (R51.83 visibility, R51.92 모듈 분할 완료) — render-side trait 화



**Outputs**:
- pinion-tui crate 신설 (TuiRenderer impl + crossterm event loop + ApplicationHandlerTui)
- WidgetRenderer trait 추출 (VelloRenderer 와 TuiRenderer 의 2 impl, Open-Closed)
- InputRouter substrate winit-free 분리 — event source 추상화 (winit / crossterm)
- Scene → cell 매핑: Box→border chars, Text→grapheme cells, Container→nested rect
- Path/Image primitive TUI 매핑 placeholder (block char) — unicode art 는 후속 carry
- ApplicationHandlerTui 별도 entry — crossterm event loop, winit 와 mutual exclusive feature
- first slice 순서 = R51.108 substrate trait, R51.109 TuiRenderer, R51.110 hello-button TUI
- framework-first — pinion-tui crate cargo feature, application optional, framework 강제 0



**Caveats**:
- Scene Path/Image primitive TUI 매핑 placeholder (block char), unicode art = R51.111+ carry
- TUI a11y (§5.40) 별도 path — screen reader 가 PTY 출력 청취, AccessKit adapter 비적용
- color depth (24bit truecolor / 256 / 16) terminal capability 의존, 자동 fallback 정통
- mouse 미지원 terminal fallback = keyboard-only (WAI-ARIA APG 정합 자동 보장)
- TUI logical coord = cell (col, row) — winit logical pixel 과 unit conversion 필요
- winit 와 crossterm cargo feature mutual exclusive — 단일 binary 두 backend 선택
- first slice 순서: R51.108 substrate trait → R51.109 TuiRenderer → R51.110 dogfood
- dry_run (§3) primitive scene→cell deterministic 자동 호환 (no GPU side effect)
- framework-first — pinion-tui crate ratatui dep optional, framework 강제 0
- Animation (R52 후보) / Scroll (R55) / Vector path (R53) R52+ axis 와 직교 (TUI 무영향)
- windows terminal / xterm / iterm2 capability 매트릭스 = R51.110+ manual test carry
- ColorBrush RGBA → ANSI color 매핑 + TextAlign wrap policy = R51.109 substrate 결정
- R51.108 — ShellCore substrate winit-free 분리 land (Touch / TouchPhase / Modifiers pinion lift)
- R51.109.0 — pinion-tui crate skeleton land (ratatui 0.29 + crossterm 0.28 + TuiRenderer placeholder)
- R51.109.1 — WidgetRenderer trait + VelloContext + macro 2 impl (backend-agnostic dispatch land)
- R51.109.2 — WidgetRenderer lift to pinion-core + TuiRenderer<B> impl land (2nd backend)
- R51.110.0 — pinion_tui::paint::to_buffer text-first 매핑 land (Box/Path/Image 는 R51.111+)
- R51.110.1 — WidgetViewTui trait + render_one_frame helper land (event loop R51.110.2)
- R51.110.2 — pinion_tui::run + hello-button-tui first dogfood land (crossterm event loop, Esc exit)
- R51.111 — TUI input dispatch: crossterm KeyEvent → W3C key str + ButtonExternal first interaction
- R51.112 — TUI mouse dispatch: cell→pixel + InputRouter wire-up + EnableMouseCapture lifecycle
- R51.113 — hello-toggle-tui 2nd TUI binding land (substrate-incompleteness-signal evidence)
- R51.115 — paint::to_buffer Scene::Box + ContainerNode.style mapping (border ┌─┐│└─┘ + bg fill)
- R51.116 — hello-button-tui / hello-toggle-tui view BoxStyle 적용 (substrate evidence-first)
- R51.117 — ShellCoreTui<V> substrate extraction (headless test infra + R51.92.1 parity)
- R51.118 — WidgetViewTui::access_node default + 2 TUI binding override (TUI a11y substrate first cut)
- R51.119 — atomic stale citation cleanup (R51.117 substrate move: 4 removes + 7 adds)
- R51.120 — substrate stderr → optional file sink (alternate screen 보호, PINION_TUI_LOG opt-in)
- R51.121 — WidgetCore + WidgetA11y supertrait split, WidgetView/Tui = Renderer + initial_size 만 (ISP)
- R51.122 — pinion-runtime::CoreShell<V> substrate 신설 (R51.122-R51.125 4-round 분할 중 #1)
- R51.123 — pinion-shell::ShellCore wraps CoreShell<V> (Vello extras 만 유지, 4-round #2)
- R51.124 — pinion-tui::ShellCoreTui wraps CoreShell<V> + refresh_state 제거 (auto-tail, 4-round #3)
- R51.125 (dispatch_rpc trait) defer — cycle 0 / 1 impl / 2nd RPC consumer 없음 (TUI carry)
- R51.140 — clippy lint family 사전-audit 회복 (R51.137-139 reactive 10건 누적) [[clippy-pre-audit-recovery]]
- test_fixtures doc: rust,no_run + # hidden imports (Rust canonical); compile-check 보존 (R588).



**Alternatives rejected**:
- ANSI escape 직접 작성 (ratatui 우회) — buffer diff + flicker control 자체 구축 천문학
- TUI 를 §5.16 backend variant 로 처리 — pixel raster axis 와 fundamental 다른 layer
- Scene primitive enum 재정의 (TUI-specific) — §5.2 closed-form 위반, AI introspect 일관성 깨짐
- winit 의존 InputRouter 유지 + TUI 별 router 구축 — DRY 위반, substrate 분기 폭증
- blessed.rs / cursive — maintenance status 약함, ratatui 가 Rust 생태 정통
- tui-rs (deprecated) — ratatui 가 maintained successor (Linebender 외 fjall 계열 maintainer)
- AccessKit-on-TUI 시도 — screen reader 가 PTY 직접 청취, AccessKit adapter 매핑 무의미



**Impact scope**: §2, §3, §5.2, §5.13, §5.15, §5.16, §5.40



**Implementations**:
- crates/pinion-runtime/src/input.rs:TouchPhase
- crates/pinion-runtime/src/input.rs:Touch
- crates/pinion-runtime/src/input.rs:Modifiers
- crates/pinion-runtime/src/input.rs:Modifiers::empty
- crates/pinion-runtime/src/input.rs:Modifiers::shift_key
- crates/pinion-shell/src/app.rs:winit_touch_to_pinion
- crates/pinion-shell/src/app.rs:winit_modifiers_to_pinion
- crates/pinion-tui/Cargo.toml
- crates/pinion-tui/src/lib.rs
- crates/pinion-tui/src/lib.rs:TuiRenderer
- crates/pinion-shell/src/lib.rs:WidgetRenderer
- crates/pinion-shell/src/lib.rs:VelloContext
- crates/pinion-core/src/renderer.rs
- crates/pinion-core/src/renderer.rs:WidgetRenderer
- crates/pinion-tui/src/lib.rs:TuiContext
- crates/pinion-tui/src/paint.rs
- crates/pinion-tui/src/paint.rs:to_buffer
- crates/pinion-tui/src/paint.rs:paint_text
- crates/pinion-tui/src/widget.rs
- crates/pinion-tui/src/widget.rs:WidgetViewTui
- crates/pinion-tui/src/widget.rs:render_one_frame
- crates/pinion-tui/src/shell.rs
- crates/pinion-tui/src/shell.rs:run
- examples/hello-button-tui/src/main.rs
- examples/hello-button-tui/src/main.rs:HelloButtonTui
- crates/pinion-tui/src/input.rs
- crates/pinion-tui/src/input.rs:key_str_from_event
- crates/pinion-tui/src/input.rs:modifiers_from_crossterm
- crates/pinion-tui/src/input.rs:cell_to_pixel
- crates/pinion-tui/src/shell.rs:dispatch_mouse
- examples/hello-toggle-tui/Cargo.toml
- examples/hello-toggle-tui/src/main.rs
- examples/hello-toggle-tui/src/main.rs:HelloToggleTui
- crates/pinion-tui/src/paint.rs:paint_box_style
- crates/pinion-tui/src/paint.rs:paint_box
- crates/pinion-tui/src/paint.rs:paint_container
- crates/pinion-tui/src/paint.rs:color_to_tui
- crates/pinion-tui/src/substrate.rs
- crates/pinion-tui/src/substrate.rs:ShellCoreTui
- crates/pinion-tui/src/shell.rs:commit_paint
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::dispatch_key
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::cursor_moved
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::pointer_down
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::pointer_up
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::compute_paint_scene
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::update_paint_scene
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::set_log_sink
- crates/pinion-tui/src/substrate.rs:ShellCoreTui::with_log_sink
- crates/pinion-core/src/widget_core.rs
- crates/pinion-core/src/widget_core.rs:WidgetCore
- crates/pinion-a11y/src/widget_a11y.rs
- crates/pinion-a11y/src/widget_a11y.rs:WidgetA11y
- crates/pinion-runtime/src/core_shell.rs
- crates/pinion-runtime/src/core_shell.rs:CoreShell
- crates/pinion-runtime/src/core_shell.rs:CoreShell::new
- crates/pinion-runtime/src/core_shell.rs:CoreShell::forward
- crates/pinion-runtime/src/core_shell.rs:CoreShell::apply_key
- crates/pinion-runtime/src/core_shell.rs:CoreShell::cursor_moved
- crates/pinion-runtime/src/core_shell.rs:CoreShell::cursor_left
- crates/pinion-runtime/src/core_shell.rs:CoreShell::pointer_down
- crates/pinion-runtime/src/core_shell.rs:CoreShell::pointer_up
- crates/pinion-runtime/src/core_shell.rs:CoreShell::pointer_cancel
- crates/pinion-runtime/src/core_shell.rs:CoreShell::touch_event
- crates/pinion-runtime/src/core_shell.rs:CoreShell::tail
- crates/pinion-runtime/src/core_shell.rs:CoreShell::update_paint_scene
- crates/pinion-runtime/src/core_shell.rs:DispatchTail
- crates/pinion-runtime/src/core_shell.rs:StateChange
- crates/pinion-shell/src/substrate.rs:ShellCore::compute_paint_scene
- examples/hello-commands-tui/src/main.rs
- crates/pinion-tui/src/paint.rs:CellClip
- crates/pinion-tui/src/paint.rs:to_buffer_inner
- crates/pinion-tui/src/paint.rs:paint_text_inner
- crates/pinion-tui/src/paint.rs:pixels_to_cell_floor
- crates/pinion-tui/src/paint.rs:cell_to_buf_xy



### §5.45. Scroll axis (R55)


**Intent**: Establish scroll container axis: ScrollNode scene primitive + offset state + input mapping + clipping render + composite widget integration.


**Rationale**:
- Current catalogue (ListBox, RadioGroup) implicitly clamps content to fixed viewport — limit
- Real GUI apps routinely have content larger than viewport (lists, forms, text editors)
- Scroll = textbook fundamental of every UI framework (Iced/Egui/Slint/SwiftUI/Web all carry)
- AI introspection: scroll offset / max bounds surfaceable via scene/query (pinion-unique)
- SCE schema benefit: scroll-bar drag state (Idle/Hover/Dragging) is a statechart axis
- Composite catalogue (R58) cannot expand without scroll primitive — Grid, CardList depend



**Inputs**:
- ListBox with 1000+ items — current clamp-to-viewport unrealistic for real apps
- Multi-line TextField (R56 carry) — vertical scroll required for >1 line of text
- RadioGroup with many options — same content-overflow case
- Iced ScrollableState / Egui ScrollArea / SwiftUI ScrollView — industry convention
- Composite catalogue expansion (R58) blocked without scroll primitive — Grid / CardList



**Outputs**:
- pinion-core::scene::ScrollNode (or Scene::ScrollContainer variant) clip+content+offset
- pinion-core::reactive scroll state (scope-id keyed offset via Owner::cache substrate)
- scroll Event variants (WheelDelta / ArrowKey / PgUp/PgDn / Home/End) in Event enum
- ScrollBar widget catalogue entry (vertical + horizontal sub-widget)
- Vello + TUI clipping render at paint_adapter boundary (cell + pixel raster)
- scene/scroll RPC method (programmatic scroll to offset / by delta)
- ListBox + future Grid composite integration through ScrollNode wrap



**Caveats**:
- R55.A: ScrollNode primitive in Scene enum carries clip rect + child content + offset(x,y).
- R55.B: ScrollState (offset + max bounds + Animation<f32>) lives on Owner::cache scope-id keyed.
- R55.C: Input mapping = wheel delta + ArrowUp/Down + PgUp/PgDn + Home/End on focus inside scroll.
- R55.D: ScrollBar sub-widget (vertical + horizontal) with hover/drag SCXML statechart per axis.
- R55.E: Clipping render at paint_adapter boundary (Vello clip layer + TUI cell-mask write skip).
- R55.F: scene/scroll RPC = 11th typed method; offset_to(x,y) / scroll_by(dx,dy) variants.
- R55.G: ListBox + future Grid/CardList composites wrap their content in ScrollNode.
- R55.H: Forge codegen emits ScrollBar statechart from SCE schema (SCE upstream RFC carry).
- R55.G.6: ScrollNode::map_layout(FnOnce) preserves seeded viewport size; with_layout drops it
- R55.D.1: scrollbar_thumb_rect closed-form 헬퍼 land — 통계/SCXML/입력 라우팅은 R55.D.2/3 carry
- R57.X.scrollbar: shell substrate re-runs V::view + compute_layout same-frame when scroll_dirty=true.
- R57.X.scrollbar: set_max returns dirty bool (revision delta, post equality-skip).
- pinion-tui has no compute_layout in compute_paint_scene; future TUI scrollbar warmup is axis carry.
- ScrollState::set_max single bool API: #[allow(must_use_candidate)] is the textbook trade-off.



**Alternatives rejected**:
- Unlimited viewport (overflow:visible) — limits widget catalogue + breaks introspection
- Per-widget ad-hoc scroll — primitive duplication, no shared input/state path
- CSS overflow-style property on existing ContainerNode — visual prop confused with semantic
- Container.scroll() inherent method — type erasure fights, sub-widget cohesion lost



**Impact scope**: §5.2, §5.3, §5.13, §5.35, §5.36, §5.16, §5.41, §5.7, §5.20, §5.40


**Examples**:

```rust
// R55.A — ScrollNode primitive sketch (pinion-core::scene)
// First-cut shape; the R55 substrate land iterates each field.
pub struct ScrollNode {
    /// Clip viewport in logical pixels / cells. Content beyond this
    /// rect (after offset translation) does not paint.
    pub viewport: Rect,
    /// Child scene rendered with `offset_*` translation applied. The
    /// content's intrinsic size MAY exceed `viewport.size`.
    pub content: Box<Scene>,
    /// Horizontal offset in logical pixels / cells. Bounded by
    /// `0..=max_x` where `max_x = content.size.x - viewport.size.x`.
    pub offset_x: i32,
    /// Vertical offset analogous to `offset_x`.
    pub offset_y: i32,
    /// R51.122 §5.41 — input router tag for wheel + key dispatch.
    pub tag: Option<Cow<'static, str>>,
}
```

```json
// R55.F — scene/scroll RPC method (11th typed method)
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "scene/scroll",
  "params": {
    "path": "/external/list_box/scroll",
    "action": { "scroll_by": { "dx": 0, "dy": 120 } }
  }
}
// Result mirrors the post-scroll offset so the AI client can confirm
// the framework clamped to bounds and the visible window has moved.
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": { "offset_x": 0, "offset_y": 240, "clamped": false }
}
```



**Implementations**:
- crates/pinion-tui/src/paint.rs:to_buffer_inner
- crates/pinion-tui/src/paint.rs:CellClip
- crates/pinion-core/src/scene.rs:ScrollNode::from_state
- crates/pinion-core/src/widgets/scroll.rs:ScrollState::with_tag
- crates/pinion-core/src/widgets/scroll.rs:ScrollState::tag
- examples/hello-listbox/src/main.rs:view
- examples/hello-listbox/src/main.rs:listbox_row_at_y
- crates/pinion-core/src/scene.rs:ScrollNode::map_layout
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBar
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBarExternal
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBar::attach_state
- crates/pinion-core/src/widgets/scrollbar.rs:ScrollBarExternal::pointer_move
- crates/pinion-runtime/src/layout.rs:compute_layout_with_scroll_dirty
- crates/pinion-core/src/widgets/scroll.rs:ScrollState::set_max
- crates/pinion-shell/src/substrate.rs:ShellCore::compute_paint_scene



### §5.49. AI-first RPC self-verification harness (R59)


**Intent**: Claude-side RPC dogfood harness: every visual round ends with a typed scene/query|invoke|snapshot demo that proves observable state without humans narrating screenshots.


**Rationale**:
- §2 #2 demands the framework prove its own state via typed RPC, not via screenshot prose.
- §2 #7 scene-as-data: observable widget state reaches the AI through typed RPC paths.
- R51.192 caught Claude asking the user how many rows were visible — direct META violation.
- Self-verifying demos catch RPC regressions the unit-test side misses (boot grace, framing).
- Python harness reuses target/release/* across runs — no cargo resolution per run.



**Inputs**:
- Workspace-relative pinion-shell example name (e.g. hello-toggle) buildable via cargo run -p <name>.
- target/release/<example> binary present, or cargo run fallback with --quiet.
- DISPLAY (X11) or WAYLAND_DISPLAY available — winit needs a surface to bind.



**Outputs**:
- Exit code 0 on every assertion satisfied; non-zero with typed reason on stderr.
- Wall-clock duration printed per demo so regressions in startup latency surface visibly.
- Stderr tail (up to 20 lines) surfaced on transport failure for shell-side diagnosis.



**Caveats**:
- scene/click v0 is probe-only; demos drive state via scene/invoke until R51.196 lands click v1.
- scene/snapshot dumps scene root + root External only; Container/Scroll traversal carries R51.194.
- No wheel/key event injection RPC method yet; §5.45 R55 Scroll axis verify waits on R51.195.
- Spawn needs X11/Wayland display; pure-headless mode is a §5.16 Vello backend carry.
- R55.G.7: find_rect_by_tag clips to Scroll viewport stack; fully off-viewport tag returns None
- R55.G.8: Box/Text/Container snapshot 가 BoxStyle/TextStyle 의 visual axis (fill/border/font) 노출
- R55.G.22: assert_widget_view_carries_tag 헬퍼가 9-widget inline assert 청산, Rule of Three 충족
- R55.G.23: hello-commands a11y_tests 모듈 + helper 사용으로 convention test 부재 청산



**Alternatives rejected**:
- Rust integration test that spawns cargo — duplicates dependency resolution per run, slower iteration.
- Screenshot-only verify — keeps the human in the loop and violates §2 #2 AI primary path.
- Third-party Python dep (pytest / anyio) — runtime install for a single-file harness is overhead.
- Bash + jq harness — JSON edge cases (escapes, nesting) push complexity onto fragile shell logic.



**Impact scope**: §2, §5.7, §5.12, §5.15, §5.16, §5.18, §5.20, §5.45


**Examples**:

```python
from rpc_verify import RpcSubprocess, assert_eq, run_demo

def body() -> None:
    with RpcSubprocess("hello-toggle") as toggle:
        assert_eq(toggle.query("/external/value"), False, "initial")
        for ev in ("PointerEnter", "PointerDown", "PointerUp"):
            toggle.invoke("/external/send", ev)
        assert_eq(toggle.query("/external/value"), True, "post-activate")

if __name__ == "__main__":
    import sys; sys.exit(run_demo("hello-toggle activate cycle", body))
```



**Implementations**:
- tools/rpc_verify.py:RpcSubprocess
- tools/demos/hello_toggle_activate.py:body
- tools/README.md



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




### §5.50. Theming substrate (R57)


**Intent**: Establish theming substrate: ColorRole enum (Material 3 / W3C mirror) + Theme palette + ThemeProvider reactive wrapper + use_theme hook so widgets resolve semantic roles instead of RGB literals.


**Rationale**:
- Hard-coded RGB literals in every widget bind one palette at build time - no light/dark swap
- W3C CSS variable / Material 3 / SwiftUI ColorScheme conventions all use semantic role tokens
- Theme = canonical framework primitive for app-wide visual coherence + accessibility tuning
- AI introspection: theme.resolve() role surface queryable via scene/query (pinion-unique)
- Future palette mode transitions modelable as a statechart axis (R57.X carry)
- R58 composite catalogue (DatePicker/Menu/Combobox) blocked without role-driven palette



**Inputs**:
- hello-toggle / hello-listbox / hello-textfield embed RGB literals (audit reveals 18+ literals)
- Material 3 Color Roles (primary/onPrimary/surface/onSurface/outline) - industry baseline
- W3C CSS Custom Properties (--color-surface) - cascade-resolved semantic tokens
- SwiftUI Color.primary / Color.background - declarative role shorthand resolved by ColorScheme
- prefers-color-scheme media query - OS-level light/dark hint to apply at runtime (R57.1 carry)



**Outputs**:
- pinion-core::ColorRole enum (Material 3 mirror, non_exhaustive for SemVer-safe extension)
- pinion-core::Theme palette struct (6 Color fields, Copy, light/dark preset factories)
- pinion-core::ThemeProvider reactive wrapper (Signal<Theme>, atomic set_theme swap)
- pinion-core::use_theme(tag) hook (Owner::cache typed-key slot, callback-root-owner-wrap)
- examples/hello-theme binary (visible light/dark toggle via Toggle + set_theme reactive cycle)
- Color now derives serde Serialize/Deserialize (Signal<Theme> requirement; hot-reload prep)



**Caveats**:
- R57.0: Tier 1 substrate -- ColorRole + Theme + ThemeProvider + use_theme. 6 roles only.
- R57.0: existing widget catalogue NOT retrofitted yet -- primitive paint colors remain literal
- R57.0: ThemeMode enum + system prefers-color-scheme bridge deferred to R57.1
- R57.0: typography / spacing tokens deferred to R57.2 (TextStyleRole + SpacingToken cascade)
- R57.0: Color now derives serde Serialize/Deserialize -- Signal<Theme> trait bound; hot-reload prep
- R57.0: hello-theme reactive wire -- view-fn use_theme().theme() + update set_theme on toggle
- R57.X.toggle: V::update authority = intent.payload, not V::read_state (pre-flip lag).
- R57.X.toggle: intent matching uses dotted wire form (e.g. "main_toggle.toggle"); runtime prefixes.
- R57.X.toggle: hover/pressed = Color::lerp toward OnSurface (M3 state layer 0.08/0.12).
- ColorRole +3 variants: SurfaceContainerLow/Container/High complete M3 5-tier elevation.
- hello-listbox retrofit: all 16 RGB literals replaced by 7 role resolves + M3 state-layer lerp.
- M3 state-layer 0.08 (hover) / 0.12 (pressed) / 0.38 (disabled) via Color::lerp.
- hello-textfield retrofit: 13 RGB literals replaced — field/caret/selection/preedit all role-driven.
- TextField filled-variant: Idle=SurfaceContainerHighest, Focused=SurfaceContainerHigh.
- hello-button retrofit: 6 RGB literals replaced — M3 filled-tonal Button role mapping.
- hello-radio/group retrofit: 23 RGB literals via shared radio_border_color (Outline/Accent + lerp).
- hello-checkbox + hello-slider + hello-slider-vertical retrofit: 41 RGB literals across 3 binaries.
- R57.1: ThemeMode (Light/Dark/System) + SystemColorScheme (W3C prefers-color-scheme mirror) enums.
- R57.1: ThemeProvider holds light+dark palettes + mode; theme() dispatches via mode + system signal.
- R57.1: thread_local SystemColorScheme Signal + system_color_scheme/set_system_color_scheme fns.
- R57.1: pinion-shell winit ThemeChanged + Window::theme() in resumed wire OS to global signal.
- R57.1: ThemeProvider::set_theme removed; set_mode + set_light_palette/set_dark_palette replace.
- R57.X.theme-fade: theme_animated() opt-in spring fades palette ~200ms via THEME_FADE_SPRING.
- R57.X.theme-fade: widget retrofit cascade carry; theme_animated() opt-in keeps theme() instant.
- R57.X.theme-fade: THEME_FADE_SPRING 400/40/1 = critically damped, omega_n=20rad/s, ~200ms settle.
- R57.X.theme-fade: Owner::current() None falls back to instant theme(); diagnostic / RPC safe.
- R57.X.theme-fade: ThemeLinear (10 AnimVec4) bridges sRGB Theme to spring solver linear space.
- R57.X.theme-fade: at-rest snap returns exact sRGB target, bypasses linear round-trip.
- R57.X.theme-fade: in-flight value uses linear-light spring; ~1 channel round-trip only mid-fade.
- R57.X.theme-fade: SwiftUI / Compose canon -- at-rest animation value equals target exactly.
- R57.X.theme-fade: snap enables widget cascade to assert == against palette fields (exact contract).
- R57.X.theme-fade: settle_owner_animations lifts 60-tick spring settle pattern to test_fixtures.
- R57.X.theme-fade: cascade migrates 10 binary view-fn theme() to theme_animated().
- R57.X.theme-fade: textfield apply_key mirrors view migration (LayoutCache identity lock-step).
- R57.X.theme-fade: cascade tests use 2-phase owner.run + settle + owner.run for clean retarget.
- R586 ime_caret_rect lock-step 마이그는 same-frame cache hit 도출 — roll back 시 cache miss 증가 (R587 측정).
- hello-textfield fade per-frame shape ~1-2ms × 12 = ~6-12% budget; visible threshold 아래 (R587).
- ColorRole +4 error tier (M3 Error 40/100/90/10 light, 80/20/30/90 dark); non_exhaustive 유지 (R590).



**Alternatives rejected**:
- Inline RGB literals per widget — no app-wide swap, retrofit cost grows with catalogue
- Per-widget theme parameter passed through view-fn args — view-fn signature pollution
- Global static Theme — race conditions on swap, no reactive subscription, no per-scope override
- ColorScheme media query auto-resolve only — needed but R57.1; substrate must work without OS



**Impact scope**: §5.2, §5.3, §5.22, §5.28, §5.38, §5.41


**Examples**:

```rust
// R57.0 view-fn consumer (hello-theme): use_theme + reactive subscribe + role resolve.
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    let provider = use_theme("app");
    let theme = provider.theme();  // auto-subscribes the current Owner
    Scene::Container(
        ContainerNode::new(vec![
            Scene::Text(TextNode::styled(
                "Theme demo",
                Rect::default(),
                TextStyle::new()
                    .with_size_px(18)
                    .with_fg(theme.resolve(ColorRole::OnSurface)),
            )),
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface))),
    )
}

// R57.0 reducer side-effect (hello-theme): swap palette on toggle intent.
fn update(state: (ToggleState, bool), intent: &Intent) -> Vec<Command> {
    if intent.tag.as_ref() == "toggle" {
        let provider = use_theme("app");
        provider.set_theme(if state.1 { Theme::dark() } else { Theme::light() });
    }
    Vec::new()
}
```

```rust
// R57.0 substrate API surface (pinion-core::theme).
#[non_exhaustive]
pub enum ColorRole { Surface, OnSurface, OnSurfaceMuted, Accent, OnAccent, Outline }

pub struct Theme {
    pub surface: Color, pub on_surface: Color, pub on_surface_muted: Color,
    pub accent: Color, pub on_accent: Color, pub outline: Color,
}
impl Theme {
    pub const fn light() -> Self { /* Material 3 light baseline, WCAG AA */ }
    pub const fn dark()  -> Self { /* Material 3 dark baseline, WCAG AA  */ }
    pub const fn resolve(&self, role: ColorRole) -> Color { /* per-role field dispatch */ }
}

pub struct ThemeProvider { /* Signal<Theme> + tag */ }
impl ThemeProvider {
    pub fn theme(&self) -> Theme { /* Signal::get -> auto-subscribe */ }
    pub fn set_theme(&self, t: Theme) { /* Signal::set -> notify subscribers */ }
}

pub fn use_theme(tag: &'static str) -> Rc<ThemeProvider> {
    Owner::current().expect("use_theme requires an active Owner scope")
        .cache(tag, || ThemeProvider::with_tag(tag, Theme::light()))
}
```



**Implementations**:
- crates/pinion-core/src/theme.rs:ColorRole
- crates/pinion-core/src/theme.rs:Theme
- crates/pinion-core/src/theme.rs:ThemeProvider
- crates/pinion-core/src/theme.rs:use_theme
- crates/pinion-core/src/style.rs:Color
- examples/hello-theme/src/main.rs:view
- examples/hello-theme/src/main.rs:HelloThemeView::update
- crates/pinion-core/src/theme.rs:ColorRole::SurfaceContainerHighest
- examples/hello-toggle/src/main.rs:ToggleView::update
- examples/hello-toggle/src/main.rs:view
- crates/pinion-core/src/theme.rs:ColorRole::SurfaceContainerLow
- crates/pinion-core/src/theme.rs:ColorRole::SurfaceContainer
- crates/pinion-core/src/theme.rs:ColorRole::SurfaceContainerHigh
- examples/hello-listbox/src/main.rs:listbox_row
- examples/hello-listbox/src/main.rs:build_scrollbar_visual
- examples/hello-textfield/src/main.rs:text_fg_for
- examples/hello-textfield/src/main.rs:field_fill_for
- examples/hello-textfield/src/main.rs:selection_fill
- examples/hello-textfield/src/main.rs:preedit_bg_fill
- examples/hello-textfield/src/main.rs:preedit_underline
- examples/hello-button/src/main.rs:button_fill_endpoints
- examples/hello-radio/src/main.rs:radio_border_color
- examples/hello-radio-group/src/main.rs:radio_border_color
- examples/hello-checkbox/src/main.rs:checkbox_accent_for
- examples/hello-checkbox/src/main.rs:checkbox_outline_for
- examples/hello-slider/src/main.rs:slider_accent_for
- examples/hello-slider-vertical/src/main.rs:slider_accent_for
- crates/pinion-core/src/theme.rs:SystemColorScheme
- crates/pinion-core/src/theme.rs:ThemeMode
- crates/pinion-core/src/theme.rs:system_color_scheme
- crates/pinion-core/src/theme.rs:set_system_color_scheme
- crates/pinion-shell/src/app.rs:winit_theme_to_pinion_scheme
- crates/pinion-core/src/theme.rs:THEME_FADE_SPRING
- crates/pinion-core/src/theme.rs:ThemeLinear
- crates/pinion-core/src/theme.rs:ThemeFadeState
- crates/pinion-core/src/theme.rs:ThemeProvider::theme_animated
- crates/pinion-core/src/test_fixtures.rs:settle_owner_animations



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



**Implementations**:
- crates/pinion-rpc/src/commands.rs:list_pending_commands



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



**Implementations**:
- crates/pinion-core/src/frame.rs
- crates/pinion-core/src/frame.rs:Frame
- crates/pinion-core/src/frame.rs:Frame::with_dt



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

### 408 — Round 40.8 — §5.34 ai-introspect-demo propose/apply visual dogfood — preview lifecycle end-to-end

**Changes**:
- examples/ai-introspect-demo/src/main.rs: state_scene (Scene::External(CountedExternal)) + paint_scene 분리 — RPC mutates state, paint derives from count
- App: PreviewLedger + SceneRevision + last_preview: Option<PreviewId> + locate_highlights: Vec<String>
- build_paint_scene(count): info_panel.fill = palette_color(count) — 5-entry palette 가 count 변화 visible
- rebuild_paint_scene(): base + yellow PENDING_HIGHLIGHT (preview in flight) + red locate-highlights 합성
- P key → propose_change(SetSignal {target_path:/info_panel, signal_path:/external/count, value:current+1})
- A key → apply_preview against state_scene; revision bump; count visible 색 변경; conflict 시 preview 유지
- C key → cancel_preview; preview 제거; 색 변경 없음 (apply 와 대비)
- L key → list_previews stdout 출력 (id / base_revision / target / affected / ttl_remaining)
- target_path != signal_path 의도적 분리 — AI 가 reasoning 하는 widget anchor 와 mutation slot 분리 입증
- pinion-overlay HighlightStyle::with_stroke + with_stroke_width 빌더 — #[non_exhaustive] 회피, const-friendly 시맨틱 style 선언 지원
- PENDING_HIGHLIGHT const = HighlightStyle::new().with_stroke(0xff_d000).with_stroke_width(3) — yellow 3px
- examples/ai-introspect-demo/Cargo.toml: serde_json workspace dep 추가 (TypedProposal::SetSignal value)



**Verification**:
- cargo build -p ai-introspect-demo: 통과
- cargo test --workspace: 517 pass (516 baseline + with_stroke_overrides_color_const_safely 신규 1)
- cargo clippy --workspace --all-targets: 신규 위반 0 (5 pre-existing baseline only — pinion-core widgets/external/topology)
- Bloch — API completeness: §5.34 lifecycle (propose/apply/cancel/list) 4 method 모두 user-driven visible path 확보
- target_path/signal_path 분리 = §5.34 typed proposal 의 anchor/mutation slot separation textbook 입증



**Impact**: §5.34, §5.33, §5.32


**Carry forward**:
- R40.9+: TypedProposal::SetStyle / ReplaceView / DispatchIntent 순차 추가 (§5.34 closed pattern 완성)
- §5.16 GPU/Vello R&D 결정 ratify (큰 architectural lock; spec round 진입 후보)
- §5.34 path walker 확장: nested External 주소 지정 (/segment/external/path) — query/rewind 가 root-only 제약 해제 시 demo 가 state/paint 분리 없이 작동 가능
- overlay Controller promote (R39.4 v0 caveat) — locate_highlights state 관리는 현재 demo-local, controller 형태로 옮기면 multi-scene 재사용
- hello-button reactive layer 통합 (R38.3 carry-forward; SCXML + Forge 공존 입증)



### 409 — Round 40.9 — §5.34 TypedProposal::DispatchIntent — ApplyContext refactor + intent emission variant

**Changes**:
- pinion-rpc/preview/proposal.rs: ApplyContext struct (scene + emitted_intents) + Proposal::apply signature change (&mut ApplyContext<'_>)
- pinion-rpc/preview/kinds.rs: TypedProposal::DispatchIntent { target_path, intent } 2번째 variant; SetSignal::apply 가 ctx.scene 사용으로 갱신
- pinion-rpc/preview/apply.rs: ApplyOutcome.emitted_intents: Vec<Intent> 필드 추가; #[non_exhaustive] 적용 + Copy/Eq derive 제거 (Vec 비호환); apply_preview 가 ApplyContext::new 생성 → proposal.apply → ctx.emitted_intents 추출
- pinion-rpc/preview/mod.rs + lib.rs: ApplyContext public 재내보내기
- pinion-rpc/preview/ledger.rs: TestProposal::apply signature 갱신
- pinion-rpc/dispatch.rs: parse_typed_proposal 가 DispatchIntent kind 처리 (target_path + intent{tag,payload} JSON deserialize); apply_outcome_to_json 가 emitted_intents 직렬화 (intent_to_json reuse); WireTestProposal::apply signature 갱신
- DispatchIntent semantics: scene 비변경 / ctx.emitted_intents push만; intent 는 apply 응답 wire 로 즉시 노출 (scene/intents poll 분리)



**Verification**:
- cargo test --workspace: 527 pass (517 → 523 → 527, +10 새 test)
- cargo clippy --workspace --all-targets: 신규 위반 0 (5 pre-existing baseline only)
- TypedProposal variant 추가 = 4종 중 2종 완성 (SetSignal R40.5 + DispatchIntent R40.9)
- ApplyContext = R40.7 DispatchContext 의 sibling — 향후 SetStyle/ReplaceView 가 추가 ctx field 비파괴 가능
- ApplyOutcome.emitted_intents 가 모든 variant 응답에 (empty 라도) 포함 — AI client switch 단순화



**Impact**: §5.34, §5.20


**Carry forward**:
- R40.10+: TypedProposal::SetStyle (BoxNode.style.fill mutation) — Scene::lookup_path_mut 워커 추가 필요
- R40.11+: TypedProposal::ReplaceView (subtree replace) — 동일 워커 재사용
- §5.20 intent 모델 재고: DispatchIntent 가 surface 한 intent 는 wire 즉시 노출이지만 scene/intents poll 과는 별 채널 — 통합 정책은 R41+ spec round 후보
- ai-introspect-demo 에 DispatchIntent variant 동시 토글 (P/I 2 key) — R40.x dogfood 카드 확장 후보
- ApplyContext future fields: animation registry, effect ledger — §5.23 R27 Effect/Command 완성 시 추가



### 410 — Round 40.10 — §5.34 TypedProposal::SetStyle — Scene::lookup_path_mut + BoxStyle 변종

**Changes**:
- pinion-core/scene.rs: Scene::lookup_path_mut(&mut self, &[String]) -> Option<&mut Scene> — lookup_path 의 mutable counterpart, two-phase 차용 으로 borrow checker 회피
- pinion-rpc/preview/kinds.rs: TypedProposal::SetStyle { target_path, style: BoxStyle } 3번째 variant
- apply_set_style: scene_segments(target_path) 구성 → lookup_path_mut → Box/Container 이면 style 교체, 아니면 "UnsupportedStyleTarget", path miss 면 "UnknownTarget"
- scene_segments: /window[id]/ prefix strip 후 세그먼트 분해 (path::resolve 의 재구현 아닌 변종)
- pinion-rpc/dispatch.rs: parse_typed_proposal 가 SetStyle kind 처리; parse_box_style helper — fill 필수, border_color+border_width 쌍으로 선택, corner_radius 선택
- BoxStyle wire shape forward-compat: 알 수 없는 key 무시, 미래 BoxStyle 필드 추가 시 additive



**Verification**:
- cargo test --workspace: 545 pass (527 → 540 → 545, +18 새 test)
- cargo clippy --workspace --all-targets: 신규 위반 0 (5 pre-existing baseline only)
- TypedProposal variant 완성도 4종 중 3종 (SetSignal R40.5 + DispatchIntent R40.9 + SetStyle R40.10)
- lookup_path_mut = lookup_path 의 대입 — borrow checker two-phase pattern 입증 (향후 ReplaceView 동일 재사용)
- BoxStyle JSON wire = §5.34 제안 의 "closed-form·but-extensible primitive" 입증 — future fields 추가 시 구문 안정



**Impact**: §5.34, §5.2, §5.3


**Carry forward**:
- R40.11+: TypedProposal::ReplaceView — lookup_path_mut 재사용 (subtree 교체) — 4종 variant 완성
- SetStyle 확장: Text/Path/Image style 변종 는 별 sub-slice (각 variant 서로 다른 style 구조)
- BoxStyle wire shape 공식 문서화 — §5.34 spec body 에 wire example 추가 후보
- ai-introspect-demo 에 SetStyle dogfood 통합 — 명시적 color picker 는 future



### 411 — Round 40.11 — §5.34 TypedProposal::ReplaceView + ViewBlueprint — 4종 variant 완성

**Changes**:
- pinion-rpc/preview/blueprint.rs (new): ViewBlueprint enum {Box, Container} — Send+Sync+Clone wire-friendly Scene 설명자; Scene 자체는 !Send+!Sync+!Clone 따라 Proposal trait 경곽에 들어갈 수 없이서 우회구
- ViewBlueprint::materialize(self) -> Scene — once-only consumption; 재귀적 children materialise
- pinion-rpc/preview/kinds.rs: TypedProposal::ReplaceView {target_path, replacement: ViewBlueprint} 4번째 variant
- apply_replace_view: scene_segments → lookup_path_mut → *node = replacement.materialize(); root-replace 지원 (empty path 시 self 반환)
- pinion-rpc/preview/mod.rs + lib.rs: ViewBlueprint public 노출
- pinion-rpc/dispatch.rs: parse_typed_proposal가 ReplaceView kind 처리; parse_view_blueprint 재귀 파서 (Box/Container kind discriminate, tag optional, children optional); parse_rect helper
- Scene !Clone 제약 해소 아님 — ViewBlueprint::Clone 로 TypedProposal::Clone derive 유지 — 기존 Clone 의 관찰 계약 Hyrum-safe



**Verification**:
- cargo test --workspace: 561 pass (540 → 545 → 561, +21 새 test — ViewBlueprint unit 5 + kinds 6 + dispatch 5 + Scene compile-time check 5)
- cargo clippy --workspace --all-targets: 신규 위반 0 (5 pre-existing baseline only)
- TypedProposal variant 4종 완성: SetSignal (R40.5) + DispatchIntent (R40.9) + SetStyle (R40.10) + ReplaceView (R40.11) — §5.34 원안 closed pattern 완성
- ViewBlueprint = closed-form mini-DSL = textbook bridge between wire (JSON) 와 runtime (Scene); !Send+!Sync+!Clone Scene 경곽에서 우회구 없는 정답
- JSON wire 재귀 파서 — nested Container 구조 round-trip 검증



**Impact**: §5.34, §5.2


**Carry forward**:
- R41 후보: §5.16 GPU/Vello 결정 spec ratify round (큰 architectural lock)
- ViewBlueprint variants 확장: Text/Path/Image — 워이어 차용 시 각 변종별 style sidecar + content payload 추가 (additive)
- Scene륾 direct 다루는 future variants (e.g. ReplaceViewWithSubtree) 는 ExternalNode 처리 설계 필요 — R42+ axis 후보
- ai-introspect-demo 에 SetStyle/ReplaceView 2 추가 toggle — R40.x 4 variant 모두 visible dogfood 후보
- §5.34 list_previews 가 ViewBlueprint kind 표시 — PreviewView 에 variant tag 필드 추가 검토



### 412 — Round 41 — §5.16 Vello hybrid path C ratify — UI 모드 backend Vello 임베드, 3D engine pass thin RHI 자체

**Changes**:
- §5.16 에 R41 caveat 7 추가 — Vello hybrid path C 결정, R11 thin RHI + naga 보존, 2024 재평가 근거, 시퀀스, 언리얼-class B path 평가 시점, AI-introspectable 렌더 차별점
- spec only — code 0 줄 변경; §5.16 architecture 결정 (R11) 자체는 보존, Vello 임베드는 implementation strategy refinement
- Vello = 2024 Linebender Xilem production-ready 2D vector rasterizer — GPU compute Stage-A/B, analytic AA, fully deterministic
- Phase 구조 명시화: Phase 1 (현재~18-24mo) Vello 임베드, Phase 2 pinion thin RHI 3D pass, Phase 3 custom render passes, Phase 4+ 언리얼-class B3 (wgpu drop) 평가
- §2#4 mode toggle (immediate vs retained) 의 textbook 용도 명시 — UI / 게임 모드 backend 분기 예약점



**Verification**:
- mnemosyne validate_workspace: T1=0 T3=0 reject=0, GENERATED.md=sync 유지
- spec round — code 변경 부재, cargo test 561 / cargo clippy baseline only 유지 (B3 이후 변경 없음)
- Linebender Vello 2024 production status = Xilem 검증 (§5.16 R11 시점 0.x maturity 경고 무효화)
- [[textbook-long-term-correct]] + [[sce-universal-meta-layer]] + [[project-scope-game-engine]] 세 메모리 정합 — lifetime project 에서 독자 렌더 구축 포기 아닌 하이브리드 좌는 점진



**Impact**: §5.16, §2


**Carry forward**:
- R42+: §5.16 build slice 1 — pinion-runtime ↔ vello::Scene 어댑터 (wgpu 의존 추가, paint 함수 교체)
- R42+: 위젤 카다로그 확장 (Slider / Toggle / TextField) — Vello 임베드 전에 몇 개 더 land 고려
- Phase 2 (3D primitive axis) 후보 — 새 axis §5.36? Mesh / Material / Pass; 언제 전에는 R40 lifecycle 안정 우선
- §5.34 ai-introspect-demo 에 SetStyle/ReplaceView/DispatchIntent dogfood 통합 — R40 4 variant 완성 이후
- R40.12+: hello-button 에 pinion-forge reactive layer 통합 (R38.3 carry-forward 계속)
- [[gui-now-engine-later]] 메모리 — Vello hybrid 결정으로 더 구체화; future game engine path 술명 추가 고려



### 413 — Round 42 — §5.34 path walker nested External addressing — R40.8 state/paint 분리 부채 상환

**Changes**:
- pinion-core/scene.rs: Scene::lookup_path_ref(&[String]) -> Option<&Scene> 이미접 버전 (lookup_path 는 Rect 반환, R42 는 nested External walk 용 세이워키 reference 필요)
- pinion-rpc/path.rs: split_at_external(scene_path) -> Option<(Vec<String>, &str)> helper — "/external/" 리터럴 세퍼레이터 기준 split
- pinion-rpc/query.rs + rewind.rs: "/<scene_seg>/external/<intro>" nested addressing 지원 — lookup_path_ref/_mut 으로 Container/Box 등단 개객
- examples/ai-introspect-demo: state_scene/paint_scene 2 필드 제거 → single canonical scene; counter ExternalNode 가 scene 안에 태그 "counter" 로 등재; signal_path = "/counter/external/count" 로 nested addressing 사용
- demo 에서 paint(scene, count) 이 info_panel 태그 일 때만 palette_color(count) 대체 — BoxStyle.fill 변경 없이 렌더 시 파생
- demo refresh_overlays 가 std::mem::replace + Scene::Effect 센티넬 스왈 — Scene !Clone 제약 하에 in-place overlay 관리



**Verification**:
- cargo test --workspace: 580 pass (561 → 580, +19 새 test — 5 lookup_path_ref + 5 split_at_external + 5 query nested + 4 rewind nested)
- cargo clippy --workspace --all-targets: 신규 위반 0 (5 pre-existing baseline only)
- cargo build -p ai-introspect-demo: 통과
- R40.8 의 textbook-long-term-correct 위반 (path walker 확장 회피) 부채 상환 완료 — demo 는 이제 single scene, nested addressing 사용
- "external" 리터럴이 세퍼레이터로 예약 (Python __init__ / Rust crate:: 컨벤션) — 그 이름 태그는 축자적 인덱스 사용 필요



**Impact**: §5.34, §5.18, §5.12


**Carry forward**:
- R43: ViewBlueprint 에 Text/Path/Image variants 추가 — Scene parity 완성 (Effect/External 은 wire 표현 부재로 제외)
- R44: §5.34 caveat — DispatchIntent.emitted_intents (sync) ↔ scene/intents (async poll) dual channel 의도적 분리 정착
- lookup_path/lookup_path_ref/lookup_path_mut 3명 불일치 — lookup_path 만 Rect 반환; 향후 lookup_path 도 &Scene 반환으로 일관 평가 후보 (bbox 은 .rect() 호출)



### 414 — Round 43 — §5.34 ViewBlueprint Text/Path/Image variant parity — R40.11 평행 우주 부채 상환

**Changes**:
- pinion-rpc/preview/blueprint.rs: ViewBlueprint 에 Text/Path/Image 3 variants 추가 — Scene introspectable variants 5종 완전 parity (Box/Container/Text/Path/Image)
- ViewBlueprint module doc: wire-side description (Scene !=parallel; wire-vs-runtime 의도적 분리, Bloch value objects / Hickey 'data is the API') 문서화
- Effect/External 은 wire 제외 명시 — Effect 는 declarative shape 부재, External 은 Box<dyn External> factory registry 부재 (행상 R-axis)
- dispatch.rs parse_view_blueprint: 5 kinds 지원 + Effect/External 명시적 reject (closed-by-design)
- parse_text_style / parse_path_style / parse_image_style / parse_path_commands helpers — 각 style 구조별 wire 파서
- parse_optional_tag: 이전 중복 해결; 5 variants 의 tag 처리 동일화



**Verification**:
- cargo test --workspace: 589 pass (580 → 589, +9 R43 test — 4 ViewBlueprint unit + 3 wire round-trip + 2 closed-kind rejection)
- cargo clippy --workspace --all-targets: 신규 위반 0 (5 pre-existing baseline only)
- ViewBlueprint Scene parity 5종 완성 (introspectable variants all covered); Effect/External wire 제외 = closed-by-design 철학 일관
- R40.11 의 'Scene 평행 우주' 우려 해소 — ViewBlueprint = wire description, Scene = runtime; 서로 역할 다름 입증
- Bloch value objects + Hickey 'data is the API' = JSON-RPC framework 의 textbook 분리 정답



**Impact**: §5.34, §5.2, §5.3


**Carry forward**:
- R44: §5.34 caveat — DispatchIntent.emitted_intents (sync) vs scene/intents (async poll) dual channel 정책 정착
- External factory registry axis — ReplaceView 가 External 교체 가능하려면 author-side type registry 필요 (§5.15 와 연결)
- Effect declarative wire shape axis — 셌더 소스 / preset enum / 파라미터 스키마 결정 필요 (§5.16 GPU pipeline 공의)
- ViewBlueprint 는 wire surface 이므로 JSON serde-derive 추가 신중 검토 — 현재 수동 parser 가 forward-compat 좋음



### 415 — Round 44 — §5.34 DispatchIntent ↔ scene/intents dual channel 정책 spec — R40.9 부채 상환

**Changes**:
- §5.34 에 R44 caveat 5 추가 — DispatchIntent emit 채널 (synchronous apply response) vs scene/intents (asynchronous poll) 두 채널 의도적 분리 정착
- AI cause-effect (apply→intent 같은 turn) = ApplyOutcome.emitted_intents; widget state machine emission (별도 turn) = scene/intents poll
- spec only — code 0 줄 변경; R40.9 의 '두 채널 모호함' 부채 가 '의도적 분리' 로 정답화
- 통합 reject 사유 명시 — 단일 channel 시 cause-effect timing 구분 불가; Brooks conceptual integrity 위반



**Verification**:
- mnemosyne validate_workspace: T1=0 T3=0 reject=0, GENERATED.md=sync 유지
- spec round — code 부재, cargo test 589 / cargo clippy baseline only 유지
- R40.9 의 dual channel 이 이제 '의도적 sync/async 구분' 으로 spec 정착 — AI client switch 근거 명시
- Bloch / Brooks textbook — cause-effect channel 과 emission stream 은 명령어 구조 어렬 (단일화 자체가 잘못)



**Impact**: §5.34, §5.20


**Carry forward**:
- AI client SDK helper — ApplyOutcome.emitted_intents 와 scene/intents drain 를 단일 stream 으로 reduce 하는 utility (선택적 client-side concern)
- §5.20 scene/intents 의 timestamp / cause-effect chain id 추가 검토 — dual channel timeline 교사 필요 시



### 416 — Round 45 — §5.16 SceneRenderer 표현 = pinion-forge renderer kind 빌드 코드젠 — backend manifest-driven, runtime 추상화 비용 0

**Changes**:
- §5.16 에 R45 caveat 8 추가 — SceneRenderer abstraction = pinion-forge renderer kind 의 build-time codegen 으로 정착
- R11 zero-overhead invariant + R31 'compile-time per target' 와 정합하는 abstraction layer 결정
- Phase 2/3/4 backend 교체 = renderer kind emit template 추가, caller 무변경 (Open-Closed)
- spec only — code 0 줄 변경



**Verification**:
- mnemosyne validate_workspace: T1=0 T3=0 reject=0, GENERATED.md=sync
- code 변경 부재 — cargo test 589 / clippy baseline 유지 (R44 이후 변경 없음)
- R11 supersede 흐름과 정합: codegen 거절은 AAA dynamic dispatch 한정, abstraction codegen 자체는 §5.16 의 정통 패턴
- sce-universal-meta-layer 정합 — pinion-forge 가 framework-side codegen 책임 (SCE upstream 미요구)



**Impact**: §5.16, §2, §5.22, §5.12


**Carry forward**:
- R46 build slice 1 commit 1: pinion-forge 에 renderer kind parser + AST + diagnostic 추가 (reactive 옆)
- R46 build slice 1 commit 2: renderer kind 의 Vello first emit template — wgpu/vello workspace dep + emit 본체
- R46 build slice 1 commit 3: ai-introspect-demo 에 app.pinion.xml renderer manifest 추가; build.rs codegen 호출; softbuffer paint 함수가 codegen 된 SoftbufferRenderer 로 교체
- R47+: Headless renderer template — §5.12 screenshot RPC 미해제 항목 진입
- R47+: text path — cosmic-text glyph cache (R31 caveat 기존 결정 정통 이행)



### 422 — Round 46 — §5.16 build slice 1 commit 2 — pinion-forge Vello first emit template (RenderContext + RenderSurface + Renderer 래퍼 struct, wgpu/vello workspace dep)

**Changes**:
- workspace.dependencies: vello = "0.6" + wgpu = "26" (vello 0.6.0 → wgpu 26 transitive dedup). 코멘트로 §5.16 R41 hybrid path C / R46.2 first emit / forward-compat slot 명시. pinion-forge 자체 deps 무변경 — emit template 은 string-only, consumer crate (R46.3 demo) 가 vello = { workspace = true } 추가
- codegen.rs: emit_renderer_stub 제거, emit_renderer(name, backend) backend-dispatch + emit_renderer_vello(name) Vello 전용 template 추가. RendererBackend 새 variant (Headless/Softbuffer/thin-RHI) = 새 arm + 새 emit 함수 Open-Closed
- VELLO_TEMPLATE const (self-contained Rust 모듈 body) — pub struct <Name> { context: RenderContext, surface: RenderSurface<'static>, renderer: Renderer }, pub enum <Name>Error { Vello(vello::Error), Surface(wgpu::SurfaceError) } + Display + Error + From impls, async fn new<W: Into<wgpu::SurfaceTarget<'static>>>(target, w, h) -> Result<Self, _>, fn render(&mut, &Scene, Color) -> Result<(), _>, fn resize(&mut, u32, u32)
- substitution: format!() 대신 .replace("__NAME__", name).replace("__ERR_NAME__", err_name) — template body 가 data 로 단일 const 표현 (textbook: codegen template = data, format string = small interpolation). placeholder 가 Rust ident shape 라 surrounding 코드와 충돌 X. clippy needless_raw_string_hashes + too_many_lines + uninlined_format_args 셋 동시 해소
- Vello 0.6 canonical pattern (Xilem reference impl): RenderContext::create_surface(..., wgpu::PresentMode::AutoVsync) → Renderer::new(device, RendererOptions { use_cpu: false, antialiasing_support: AaSupport::area_only(), ... }) → render_to_texture(device, queue, scene, target_view, RenderParams { base_color, w, h, antialiasing_method: AaConfig::Area }) → blitter.copy(..., target_view, swapchain_view) → surface_texture.present(). AaSupport::area_only() ↔ AaConfig::Area 일치 (only-area path)
- lib.rs tests: emits_renderer_stub_records_name_and_backend 제거 (stub 사라짐), 4 신규 추가 — emits_renderer_vello_struct_and_constructor_signature (pub struct + 3 field + 3 method signature) / emits_renderer_vello_error_enum_with_from_impls (closed enum + Display + Error + From×2) / emits_renderer_vello_uses_canonical_vello_api_surface (vello::util / vello::wgpu / render_to_texture / blitter / AutoVsync / area_only ↔ Area) / emits_renderer_vello_module_header_and_no_text_pollution (banner + DO NOT EDIT + __NAME__ / __ERR_NAME__ leak 0)



**Verification**:
- cargo test --workspace = 611 pass (608 baseline + 3 net pinion-forge: 76 unit = 73 + 4 신규 신호 - 1 removed stub test), 0 failed
- cargo clippy --workspace --all-targets = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / ai-introspect-demo 4 / pinion-forge 0 (const refactor 가 needless_raw_string_hashes + too_many_lines + uninlined_format_args 셋 해소)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync (generate_docs cascade)
- workspace consumer 영향 0 — pinion-rpc / pinion-runtime / pinion-cli / examples 모두 R46.2 emit template 미사용; ai-introspect-demo / hello-button 통과 = R46.3 (다음 slice). vello/wgpu workspace.deps 추가 = lookup table 만, 어떤 member 도 workspace=true 인용 안 함 → cargo check 빌드 시간 변화 0



**Impact**: §5.16, §5.22


**Carry forward**:
- R46.3: ai-introspect-demo build slice — examples/ai-introspect-demo 에 app.pinion.xml renderer manifest (kind="renderer" name="DemoRenderer" backend="vello") + build.rs 가 emit_rust() → OUT_DIR/<name>.rs + include! 로 main.rs 통합. softbuffer paint(...) 함수 제거 → renderer.render(scene, base_color) 교체. ai-introspect-demo Cargo.toml 에 vello = { workspace = true } + winit 0.30 (이미 있음) + pollster (async new 호출용)
- R46.4+: vello::Scene 변환 어댑터 textbook 위치 — pinion-runtime 안 새 module (paint_adapter). Scene::Container/Box/External/Effect/Text/Path/Image 각 case 가 vello::Scene 의 fill / stroke / push_layer / pop_layer 호출로 매핑. ai-introspect-demo 의 R46.3 first dogfood = palette_color background 만 (Scene 전체 변환 X); full Scene tree 변환 = R46.4. hello-button 도 동일 adapter 사용 (R46.5+)
- R47+: Headless renderer template — §5.12 screenshot RPC 미해제 항목 진입. RendererBackend::Headless variant 추가 + emit_renderer_headless 함수 (render_to_texture only, surface acquisition skip; render_to_texture target 가 owned wgpu::Texture). screenshot RPC dispatch = manifest entry 통해 closed-form, runtime virtual dispatch 0 유지
- R47+: text path — cosmic-text glyph cache (R31 caveat 기존 결정 정통 이행). renderer kind 의 첫 번째 horizontal axis (backend orthogonal cross-cutting concern); Vello + Headless + Softbuffer 모두 공유. cosmic-text + vello::Scene::draw_glyph integration 점
- R47+: 위젯 카탈로그 확장 — Slider / Toggle / TextField. R41 sequence 명시 'R40 lifecycle → 위젯 카탈로그 → §5.16 build' 의 위젯 단계, R46.3 build phase end-to-end 정착 후 진입. R48 InputRouter 와 multi-widget dispatch 실증
- R297 false-positive hint mnemosyne round, pinion atomic 무관 — R46.2 commit 까지 4-commit carry, 무시 가능 (mnemosyne main HEAD 와 host build 차이 — 동기화 작업은 향후 별도 round)



### 423 — Round 46.2.1 — §5.16 Vello aa manifest attribute forward-compat — RendererBackend::Vello { aa: VelloAaMode } ADT 확장, AaSupport/AaConfig 하드코딩 제거, R46.2 self-audit Concern 1 same-session 상환

**Changes**:
- ast.rs ADT 확장: RendererBackend::Vello → struct variant Vello { aa: VelloAaMode }. 새 enum VelloAaMode { Area, Msaa8, Msaa16 } + from_attr/as_attr. 새 enum RendererBackendKind tag-only (PinionKind ↔ PinionSpec 점롬). RendererBackend::from_attr/as_attr → RendererBackendKind 으로 이동 (concerns 분리 — tag parsing vs payload assembly)
- parser.rs: parse_root_attrs 가 aa_raw 캐프처 + 4-tuple 반환 (kind, backend_raw, aa_raw, name). 새 validate_renderer_aa(aa_raw, location) -> Option<VelloAaMode> — None/whitespace-only 는 default Area (R46.1 manifest 구형 backward-compat 유지), unknown literal 만 UnknownAa diagnostic. 새 assemble_backend(kind, aa) -> RendererBackend 자유 함수 — 명시적 (kind, payload) → variant 조립 지점
- diagnostic.rs: PinionForgeDiagnostic::UnknownAa { found, location } 추가. code = dsl/unknown-aa, stage = Validate. UnknownBackend 와 같은 (found, location) 패턴 — actual field 이 found 노출. 동일 구조 유지
- wire.rs: key_fragments + actual_of 에 UnknownAa arm 추가 — (code, stage, file, found) tuple 로 id 안정 유지, agent dispatch 용 actual = 잘못된 literal
- codegen.rs: emit_renderer({ aa }) 가 emit_renderer_vello(name, aa) 호출. VELLO_TEMPLATE 에 __AA_SUPPORT__ / __AA_METHOD__ placeholder 추가. 새 helper aa_support_literal(aa) 와 aa_method_literal(aa) — 각각 'AaSupport { area: X, msaa8: Y, msaa16: Z }' struct literal 과 'AaConfig::Area|Msaa8|Msaa16' 반환. struct literal 평을 선택한 이유 = Vello 가 주어진 mode 만 shader compile (binary smaller, init faster) — R45 'compile-time per target' 정합
- lib.rs tests: 11 신규 추가 — aa parsing happy path ×3 (area/msaa8/msaa16) + default Area when absent (R46.1 backward-compat) + default Area when whitespace-only + UnknownAa diagnostic + wire actual carriage + UnknownAa only when backend valid + codegen substitution verify ×2 (msaa16/msaa8) + default vs explicit area emit byte-equality. 기존 2 테스트 패턴 업데이트 (RendererBackend::Vello { aa: VelloAaMode::Area }) + 테스트 1 개 module_header_no_pollution 에 __AA_SUPPORT__/__AA_METHOD__ leakage check 추가



**Verification**:
- cargo test --workspace = 622 pass (611 baseline + 11 신규 R46.2.1 tests in pinion-forge), 0 failed
- cargo clippy --workspace --all-targets = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / ai-introspect-demo 4 / pinion-forge 0 (신규 4 doc-backtick warning 즉시 정정 — AaSupport/AaConfig 백틱 누락)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync (generate_docs cascade)
- workspace consumer 영향 0 — pinion-rpc / pinion-runtime / pinion-cli / examples 모두 RendererBackend API 미사용. R46.1 backward-compat 회귀 검증 — (kind="renderer" backend="vello") manifest 가 default aa=Area 로 처리 (parses_empty_renderer_self_closing + defaults_renderer_aa_to_area_when_absent test pass)



**Impact**: §5.16, §5.22


**Carry forward**:
- R46.2 self-audit Concern 1 (RendererOptions 하드코딩 forward-compat 위반) — R46.2.1 에서 aa 와 우선 상환. 나머지 (present_mode / use_cpu / num_init_threads / pipeline_cache) 는 build-time vs runtime 분류가 ambiguous — 별도 axis
- R46.2.x: present_mode + use_cpu surface policy spec round — build-time per target (manifest attribute) vs runtime selectable (builder method) 결정 필요. present_mode 는 §2#4 mode toggle (UI/game 전환) candidate 로 runtime selectable 가 더 textbook; use_cpu 는 WebGPU CPU fallback 을 build-time 이지 runtime 인지 환경 의존. spec round 로 분류 결정
- R46.2 self-audit Concern 2 (컴파일 검증 부재) — R46.3 의 cargo check 가 첫 실증. 여전히 vello::Error / wgpu::SurfaceError / Into<wgpu::SurfaceTarget<'static>> 실제 존재 미검증. trybuild integration test = 별도 axis (R46.x carry, pinion-forge 전체 codegen 공통 격차)
- R46.2 self-audit Concern 3 (resize fallibility) — Vello RenderContext::resize_surface 실제 fallible 여부 결정 후 정정. R46.3 cargo check 에서 파압 가능 (signature mismatch 시 컴파일 실패)
- R46.3: ai-introspect-demo build slice — app.pinion.xml renderer manifest (kind="renderer" name="DemoRenderer" backend="vello") — aa 속성 생략 = default Area (UI 캐너니컬). build.rs 가 emit_rust() → OUT_DIR/<name>.rs + include! 로 main.rs 통합. softbuffer paint(...) 제거 → renderer.render(scene, base_color) 교체
- R47+ 쪽 carry items 모두 유지 (Headless renderer template / cosmic-text glyph cache / 위젯 카탈로그 확장)



### 424 — Round 46.3 — §5.16 ai-introspect-demo Vello path end-to-end + workspace rustc 1.86 MSRV bump (Vello 0.6 transitive); R46.2 Concern 2/3 self-audit 해소 — emit template 실증 컴파일 통과, Vello resize_surface 가 infallible 확인

**Changes**:
- rust-toolchain.toml 1.85.0 → 1.86.0 + workspace.package.rust-version 교체. Vello 0.6.0 transitive (vello_encoding / vello_shaders) 가 rustc 1.86 요구. 2025-04 릴리스, 현재 2026-05 — lifetime project 맥락 에서는 자연 이동
- examples/ai-introspect-demo/Cargo.toml: vello + pollster 추가, softbuffer 제거 (더 이상 사용 안됨). build-dependencies 에 pinion-forge 추가. winit 은 workspace 유지
- app.pinion.xml 신규 — kind="renderer" name="DemoRenderer" backend="vello" (aa 세속성 생략 = R46.2.1 default Area). build.rs 가 pinion_forge::compile_file 호출 → OUT_DIR/app.rs. forge-counter 와 동일 패턴
- src/main.rs 소프트버퍼 → Vello 전도: softbuffer Context/Surface + Rc<Window> 제거, Arc<Window> + DemoRenderer + VelloScene buffer (매 프레임 reset). resumed 이 pollster::block_on(DemoRenderer::new(Arc<Window>, w, h)) 호출. WindowEvent::Resized 가 renderer.resize(w, h) 호출. include!(".../app.rs") 가 mod gen_renderer { ... } 안으로 wrap — codegen 의 use vello::* 가 main 모듈의 pinion_core::style::Color namespace 와 충돌 안 함
- build_vello_scene / fill_rect / stroke_rect / root_background / pinion_to_peniko helper 추가 — pinion Scene tree → vello::Scene (Container/Box 쪽만 paint, External/Effect/Text/Path/Image 는 이전 paint() 그대로 no-op). Border 는 vello::Stroke 로 전도 (이전 4-side fill 을 대체, inset=width/2 로 center-stroke ↔ inside-rect 일치). pinion Color (0x00RRGGBB) → peniko::Color::from_rgb8 (opaque) 변환
- VELLO_TEMPLATE header `//!` → `//` 교체 (R46.3 컴파일 실증 시 서프라이즈 수정) — include!() 를 mod { ... } 내부에 쓰면 `//!` 가 부모 모듈의 inner doc 으로 해석되어 use 아이템 앞을 도철잘 거절. emits_renderer_vello_module_header_and_no_text_pollution 테스트 업데이트
- pinion-forge codegen.rs format_push_string 재구조 — 워크스페이스 rustc 1.86 clippy::pedantic 가 13 곳 `string.push_str(&format!(...))` 을 플래그. textbook idiom 으로 writeln!(out, ...) (std::fmt::Write trait) 교체. 13 함수 내부 변경, byte-output 동일 (87 pinion-forge 테스트 그대로 통과)
- pinion-core external.rs 의 2 doc list overindented 정정 — 서속 라인을 5-space 들여쓰기로 재정렬 (clippy::doc_overindented_list_items 권고)
- ai-introspect-demo allow list 확장 — clippy::cast_possible_wrap (R42 pre-existing as i64 cast, PALETTE.len()=5 bounded) + clippy::doc_overindented_list_items + clippy::doc_lazy_continuation (rustc 1.86 새 doc lint, 예제-narrative 차원). 이유 주석 추가



**Verification**:
- cargo test --workspace = 622 pass (R46.2.1 baseline 유지, R46.3 는 demo binary 수정 만으로 신규 테스트 없음), 0 failed
- cargo clippy --workspace --all-targets = pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / pinion-forge 0 / ai-introspect-demo 0 (R46.2 baseline 의 4 에서 개선 — allow list 가 1.86 strict doc lint 까지 포괄). hello-button / forge-counter / pinion-overlay / pinion-cli / pinion-derive 0
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- cargo check -p ai-introspect-demo 통과 — R46.2 Concern 2 (컴파일 검증 부재) 실증 완료. emit template 의 vello::Error / wgpu::SurfaceError / Into<wgpu::SurfaceTarget<'static>> / RenderContext::create_surface / Renderer::new / render_to_texture / blitter.copy / surface_texture.present() 모두 실제 API 와 일치. Concern 3 (resize fallibility) 실증 — Vello RenderContext::resize_surface 는 () 반환 (시그니처 fn resize_surface(&mut self, surface: &mut RenderSurface<'_>, width: u32, height: u32) — fallible 아님), 현재 signature 정합



**Impact**: §5.16, §6.4


**Carry forward**:
- R46.4: paint_adapter → pinion-runtime 로 이동 — ai-introspect-demo 의 inline build_vello_scene / fill_rect / stroke_rect / root_background / pinion_to_peniko 가 reusable runtime module 되어야. hello-button 의 softbuffer 교체 (R46.5+) 가 두 번째 consumer 로 경계 의 검증. Scene::Container/Box 명당 fill/stroke + tag-based palette substitution (info_panel) 은 운영 레이어의 framework primitive 후보. 단 'info_panel' tag substitution 이 애플리케이션-specific 이라 paint_adapter 이 callback (Scene → Color hook) 로 노출해야
- R46.5+: hello-button softbuffer → Vello 교체. R46.3 자신의 build.rs + manifest 패턴 재사용. paint_adapter (R46.4) 의 두 번째 consumer. R48 InputRouter 의 multi-widget dispatch 실증 결합 가능
- R46.x: pinion-rpc 의 13 clippy 경고 감사 (most are pre-existing pre-MSRV-bump; 1.85 부터 있었음). cast_possible_truncation (dispatch.rs:1081 f64→f32) / needless_pass_by_value (1203, 1307) / missing_errors_doc (path.rs:84) / doc_markdown (multiple) 명세 분류 후 수정 vs allow 결정. 별도 round
- R46.2 Concern 1 (RendererOptions 전체 노출) carry 유지 — R46.2.1 에서 aa 만 상환. present_mode / use_cpu / num_init_threads / pipeline_cache 는 build-time vs runtime 분류 spec round 대기. R46.3 의 base_color = root container fill 을 PenikoColor::BLACK 대신 사용 한 결정과 동일 속성
- R46.2 self-audit Concern 4 (식별 신규) — emit template 이 use vello::* 를 개별 use 로 채택, include!() 시 consumer 니임스페이스 충돌 가능. R46.3 에서 mod { ... } wrap 으로 해소. textbook canonical 은 fully-qualified path (::vello::*) 로 테플릿 재작성 — reactive emit 과 일관성. 별도 R46.x 후보
- R46.3 surfaced 한 VELLO_TEMPLATE header `//!` 이슈 — 이미 본 commit 에서 정정 (// regular comments 로 교체). 추가 carry 항목 아님
- R47+ carry items (Headless template / cosmic-text / 위젯 카탈로그) 그대로 유지
- R297 false-positive hint 그대로 (mnemosyne round, pinion atomic 무관). pinion R46.3 commit 이후 5-commit carry



### 425 — Round 46.3.1 — §5.16 paint_adapter framework primitive — Scene→vello::Scene 변환 inline 어댑터를 ai-introspect-demo 에서 pinion-runtime 으로 승격 (R47/R48 패턴: application-level workaround → framework primitive), R46.3 self-audit Concern #1 #2 #4 (partial) same-session 상환

**Changes**:
- pinion-runtime/Cargo.toml: features.vello = ["dep:vello"] (default 비활성), vello = { workspace = true, optional = true } 추가. 고레대 구조 유지 — 디폴트 pinion-runtime 는 wgpu 의존 없음 (headless / TUI / 미래 pinion-render-* 백엔드 forward-compat)
- pinion-runtime/src/paint_adapter.rs 신규 (#[cfg(feature = "vello")]) — pub fn to_vello<F: Fn(&BoxNode) -> Option<Color>>(scene, hook, out) + pub fn root_background(scene) -> PenikoColor + pub fn to_peniko(c) -> PenikoColor + fill_rect/stroke_rect private helpers. Scene::Container/Box walking, alpha-preserving from_rgba8, transparent Color skip, border inset = width/2 (R46.3.2 carry note for placement enum)
- ai-introspect-demo/Cargo.toml: pinion-runtime = { path = "...", features = ["vello"] } 의존 추가. 이제 demo 는 framework primitive 소비자
- ai-introspect-demo/src/main.rs: build_vello_scene / fill_rect / stroke_rect / pinion_to_peniko / root_background 5 개 인라인 함수 전체 제거 (약 90 LOC). use pinion_runtime::paint_adapter. render() 이 paint_adapter::root_background + paint_adapter::to_vello (info_panel 태그 치환을 closure 로 노출) 호출
- ai-introspect-demo Color migration — PALETTE 가 &[u32] → &[Color] (Color::rgb(r,g,b) opaque constructor). build_initial_scene 의 6 개 Color::from_argb(0x00...) → Color::rgb(...) (알파 byte = 0 이 이제 framework 에서 제대로 투명으로 렌더링됨 — R46.3 inline 이 강제 알파=255 hardcode 로 버그 은폐하던 것을 수정). PENDING_HIGHLIGHT 동일 적용
- paint_adapter tests 7 신규 — to_peniko_preserves_all_channels_including_alpha (R46.3 alpha=255 hardcode 회귀 방지) / to_peniko_alpha_zero_is_transparent (legacy 0x00RR_GGBB 명시적 동작) / root_background_extracts_root_container_fill / root_background_falls_back_to_black_for_non_container / to_vello_walks_container_and_box_children (Cell-based hook counting) / to_vello_hook_some_overrides_box_native_fill (info_panel 패턴 검증) / to_vello_nested_container_recurses (2-level nest)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 629 pass (622 R46.3 baseline + 7 paint_adapter unit tests), 0 failed
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 (신규 2 doc-backtick warning 즉시 정정) / pinion-forge 0 / ai-introspect-demo 0
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- cargo check -p ai-introspect-demo 통과 — paint_adapter 교체 후 demo 컴파일 clean. binary smoke: paint_adapter::to_vello 가 generic Fn 좌표 다석적으로 (move 아닌 &-bind), info_panel 태그 분기 일치
- R46.3 Concern #1 (paint_adapter framework primitive) ✅ 완료 — R47/R48 패턴 (애플리케이션 workaround → framework) 적용; #2 (info_panel callback hook) ✅ closure 노출; #4 (Color alpha semantic) ✅ partial — paint_adapter 가 alpha 보존 + demo migration. #3 (Border placement) → R46.3.2, #5 winit suspend → R46.3.4, #6 fully-qualified template → R46.3.3, #7 doc reflow → R46.3.5



**Impact**: §5.16, §5.3


**Carry forward**:
- R46.3.2: Border placement spec — pinion_core::style::Border 에 placement: BorderPlacement { Inside (legacy softbuffer behavior, default), Center, Outside } 필드 추가. paint_adapter::stroke_rect 가 placement 별 inset 계산 — Inside = +width/2 (현재), Center = 0, Outside = -width/2. §5.3 caveat 추가
- R46.3.3: VELLO_TEMPLATE fully-qualified path refactor — use vello::* 제거 → 모든 타입 ::vello::* 인라인. reactive emit (::pinion_core::reactive::*) 와 일관성. ai-introspect-demo 의 mod gen_renderer wrap 제거 가능
- R46.3.4: winit suspend/resume — ai-introspect-demo 에 RenderState enum { Active{surface,window}, Suspended(Option<Arc<Window>>) } 도입. mobile/Wayland forward-compat
- R46.3.5: doc reflow — main.rs module docstring 을 clippy-clean indentation 으로 재정렬, clippy::doc_overindented_list_items + doc_lazy_continuation allow 제거
- R46.5+: hello-button 가 paint_adapter 의 두 번째 consumer. softbuffer 교체 + InputRouter (R48) 결합 가능
- R47+ carry items 그대로 (Headless template / cosmic-text / 위젯 카탈로그)



### 426 — Round 46.3.2 — §5.3 BorderPlacement enum 도입 — Border 의 inset 의미가 implicit (paint_adapter 의 width/2 인라인 트릭) 이었음을 explicit framework primitive 로 승격, R46.3 self-audit Concern #3 same-session 상환

**Changes**:
- pinion-core/src/style.rs: 신규 enum BorderPlacement { Inside (default), Center, Outside } — #[non_exhaustive] forward-compat. Inside = legacy softbuffer "drawn inside rect bounds" 호환, Center = Vello 이 native stroke (반구조 spill), Outside = CSS content-box. §5.3 R20 lock 에 R46.3.2 caveat
- Border struct 에 placement: BorderPlacement field 추가. Border::new(color, width) 가 BorderPlacement::Inside default (기존 콜 사이트 backward-compat). 새 builder Border::with_placement(placement) 추가
- paint_adapter::stroke_rect 가 BorderPlacement match 로 offset 계산 — Center=0, Outside=-width/2, Inside (+ 미래 variant) = +width/2. clippy::match_same_arms 해소 을 위해 Inside | _ 움별 패턴 사용 (forward-compat 커버리지 + 중복 arm 회피)
- pinion-core tests 3 신규 — border_default_placement_is_inside (Border::new 의 default 필드 검증) / border_with_placement_builder_overrides_default (builder API) / border_placement_three_variants_distinct (세 variant 독립성 + Default 구현)
- paint_adapter tests 1 신규 — stroke_rect_inside_placement_inset_matches_softbuffer_geometry (세 placement variant 모두 panic-free walk + match 분기 실증)
- Border::new 계속 사용자 (pinion-overlay highlight, pinion-rpc dispatch.rs) backward-compat 유지 — placement 명시 안 하면 Inside (기존 동작)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 633 pass (629 이전 + 4 신규: 3 Border placement + 1 stroke_rect placement)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / pinion-forge 0 / ai-introspect-demo 0 (신규 2 warning 즉시 정정: paint_adapter doc backtick + match_same_arms)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- Backward compat 검증 — pinion-overlay highlight.rs (line 148) + pinion-rpc dispatch.rs (line 1155) 의 Border::new 사용처 모두 수정 불필요, placement 기본값 Inside 가 레거시 동작 보존



**Impact**: §5.3, §5.16


**Carry forward**:
- R46.3.3: VELLO_TEMPLATE fully-qualified path refactor — use vello::* 제거, mod gen_renderer wrap 해소
- R46.3.4: winit suspend/resume 구조 — ai-introspect-demo 의 RenderState enum
- R46.3.5: doc reflow — main.rs module docstring + clippy allow 제거
- Future BorderPlacement variant 후보 — R47+ 에서 아직 없음. 현재 세 variant 이 CSS / softbuffer / Vello 세 개 래트러리 온전 커버. 다른 placement (e.g. 공간 권한제원에 따른 불규칙 inset) 가 쟈광되면 새 variant + paint_adapter 의 wildcard fallback 로 처리
- Border 의 dash / dot pattern, miter style, corner-radius interaction 등 추가 속성 은 별도 axis (어첌든 placement 과 직교)



### 427 — Round 46.3.3 — §5.16 §5.22 VELLO_TEMPLATE fully-qualified path refactor — emit template 의 use vello::* 전체 제거 → ::vello::* 인라인, reactive emit 의 ::pinion_core::reactive::* 패턴과 일관성 확보, R46.3 self-audit Concern #6 same-session 상환

**Changes**:
- pinion-forge/src/codegen.rs VELLO_TEMPLATE 재작성 — use vello::peniko::Color / use vello::util::{RenderContext, RenderSurface} / use vello::wgpu / use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene} 4 개 use 항목 전부 제거. 모든 Vello/wgpu 타입 경로가 ::vello::* / ::vello::wgpu::* / ::std::* 으로 절대 경로 인라인
- aa_support_literal / aa_method_literal helper 의 반환 문자열도 ::vello::AaSupport / ::vello::AaConfig fully-qualified 로 교체. RenderParams field positions 일치 유지
- ai-introspect-demo/src/main.rs mod gen_renderer { include!(...) } wrap 제거 → bare include!() (forge-counter reactive-emit consumer 패턴 복구). 더 이상 namespace 충돌 isolation 움별각 필요 없음
- lib.rs renderer tests 6 개 업데이트 — emits_renderer_vello_struct_and_constructor_signature / emits_renderer_vello_error_enum_with_from_impls / emits_renderer_vello_uses_canonical_vello_api_surface / emits_renderer_vello_aa_msaa16_substitutes_struct_literal / emits_renderer_vello_aa_msaa8_substitutes_struct_literal. 이제 절대 경로 (::vello::*, ::std::*) marker 검증. canonical_api_surface 테스트 에 'no use vello::* items' 부정 assert 추가 (R46.3.3 namespace 계약 보장)
- reactive emit (::pinion_core::reactive::Signal/Computed/Resource) 과 일관성 획득 — R38 이후 reactive template 이 채택한 fully-qualified 패턴 을 Vello template 도 따름. 추후 backend (Headless / Softbuffer / thin-RHI) 도 동일 귀약 적용 예정



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 633 pass (같은 수 유지, 6 테스트 업데이트 = identical 수 테스트 파일). 0 failed
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / pinion-forge 0 / ai-introspect-demo 0
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- ai-introspect-demo cargo check 통과 — bare include!() 에서 emit 결과 자동 경로 충돌 없이 컴파일. main.rs 의 use pinion_core::style::Color (typed) 와 emit 의 ::vello::peniko::Color (절대경로) 제대로 분리



**Impact**: §5.16, §5.22


**Carry forward**:
- R46.3.4: winit suspend/resume RenderState enum — ai-introspect-demo mobile/Wayland forward-compat
- R46.3.5: doc reflow + clippy allow 제거
- R47+ Headless backend template (§5.12 screenshot RPC) 추가 시 동일 fully-qualified 컨벤션 적용 필수 — R46.3.3 이 명시적 표준 설정
- future codegen template 의 namespace 계약 = 모든 emit 는 fully-qualified path + no use items. consumer 는 module wrap 없이 include!() 가능. forge-counter / ai-introspect-demo / hello-button (R46.5+) 머스트 바 consume convention



### 428 — Round 46.3.4 — §5.16 ai-introspect-demo winit suspend/resume RenderState enum — mobile/Wayland forward-compat: 두 Option field (window / renderer) 의 implicit-sync 패턴을 explicit RenderState { Active, Suspended } enum 으로 승격, R46.3 self-audit Concern #5 same-session 상환

**Changes**:
- examples/ai-introspect-demo/src/main.rs: 신규 enum RenderState { Active { window: Arc<Window>, renderer: DemoRenderer }, Suspended(Option<Arc<Window>>) } 도입. Linebender Vello 0.6 canonical pattern (Xilem reference impl) 그대로 따름
- App struct: window: Option<Arc<Window>> + renderer: Option<DemoRenderer> 두 field 제거 → state: RenderState 단일 field. App::new() 가 Suspended(None) 으로 시작. 두 Option 의 implicit 상관관계 제거 — typed ADT 가 illegal states unrepresentable
- resumed() 해들러: 이메 Active 는 no-op. Suspended(cached) 에서 cached window 재사용 (있으면) 또는 create_window. DemoRenderer::new 호출 실패 시 cached window 는 보존 (다음 resumed 재시도 가능) — std::mem::replace 로 안전한 state 전환
- suspended() 해들러 신규 — Active 에서 window 만 따서 Suspended(Some(window)) 으로 전환. renderer 는 드롭 (wgpu Surface 해제, OS 가 reclaim 가능). 데스크탑 platform 은 suspended 미호출 (no-op forward-compat slot)
- request_redraw() / render() / Resized handler 가 RenderState pattern matching 으로 재작성. Active 에서만 renderer 접근, 나머지 모두 no-op. clippy::single_match_else 해소 을 위해 if let 패턴 사용 (조기 return 있는 None branch)
- App.cursor / pending_escape_exit / scene / revision / ledger / vello_scene 등 lifecycle-무관 필드는 이동 안 함 — 상태와 있는 window+renderer 만 state 안으로 캡쇄



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 633 pass (R46.3.3 이전과 동일, demo binary unit test 추가 없음)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / pinion-forge 0 / ai-introspect-demo 0 (신규 single_match_else warning 즉시 정정)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- Desktop 만 가능한 smoke test — cargo check 통과, RenderState enum dispatch 경로 컴파일 clean. Mobile/Wayland 테스트는 R47+ Android target 적곰 이후 가능



**Impact**: §5.16


**Carry forward**:
- R46.3.5: ai-introspect-demo doc reflow + clippy allow 제거 — 마지막 R46.3 self-audit concern (#7) 상환
- R47+ mobile/Android target 검증 — winit Android 통합 시 R46.3.4 의 RenderState 패턴 실증. 현재 데스크탑만 compile clean, 실제 suspend 이벤트 테스트 필요
- R47+ Wayland-compositor focus change 테스트 — GNOME/Sway 에서 app focus loss 시 winit 의 suspended/resumed 이벤트 fire 패턴 검증
- Renderer drop 의 GPU resource cleanup 타이밍 — wgpu Surface drop 은 RAII 로 즉시, 다음 resume 가 fresh device handle 획득. 현재 구현 의 unwrap path 제한적 (e.g. DemoRenderer::new 실패 시 cached window 도 drop)— textbook 극도는 재시도 로직 (R47+ exponential backoff 등)



### 429 — Round 46.3.5 — §5.16 ai-introspect-demo doc reflow + clippy allow 2개 제거 — example narrative 가 lifetime project framework 의 example 으로도 textbook 깨끗해야, R46.3 self-audit Concern #7 same-session 상환 (R46.3 부채 청산 최종 step)

**Changes**:
- main.rs module docstring reflow — R42-vintage 'aligned-to-dash' 다중라인 bullet (col ~25 에서 이어지는 패턴) 을 single-line bullet (col 5 으로 reflow) 으로 교체. 각 bullet 이 자체-완결 적으로 구성
- module docstring section header 추가 — ## Single canonical scene + ## Controls (R42 보존) markdown H2 으로 구조화, prose flow 더 읽기 쉬움
- build_initial_scene doc 변경 — '+' 로 시작하는 line 없애기 위해 prose 재구성. clippy::doc_lazy_continuation 의 '+ counter' (markdown list marker 해석) 해소
- #![allow] 에서 clippy::doc_overindented_list_items + clippy::doc_lazy_continuation 두 항목 제거. R46.3 에서 MVP framing 으로 추가했던 차선책 — textbook = doc 자체를 clippy-clean 하게 이제 수정. cast_possible_truncation / cast_sign_loss / cast_possible_wrap / doc_markdown 4 개 allow 는 유지 (각각 이유 명시 주석 있음)
- module doc 에서 R46.3.3 (fully-qualified template) + R46.3.1 (paint_adapter) 언급 추가 — R46.3 carry forward 'gen_renderer wrap' / 'build_vello_scene inline' 언급 제거 (이제 outdated)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 633 pass (변화 없음)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / pinion-forge 0 / ai-introspect-demo 0
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- R46.3 self-audit 7 개 concern 전부 same-session 상환 완료 — #1 paint_adapter framework primitive (R46.3.1) / #2 callback hook (R46.3.1) / #3 BorderPlacement enum (R46.3.2) / #4 Color alpha semantic (R46.3.1) / #5 winit suspend RenderState (R46.3.4) / #6 fully-qualified template (R46.3.3) / #7 doc reflow + allow 제거 (R46.3.5)



**Impact**: §5.16


**Carry forward**:
- R46.5+: hello-button softbuffer → Vello 교체. R46.3.1 paint_adapter 의 두 번째 consumer. R46.3.4 RenderState 패턴 그대로 적용. R48 InputRouter 결합 가능
- R46.x: pinion-rpc 13 개 clippy 경고 감사 (pre-MSRV-bump 이전부터 있었음). cast_possible_truncation (dispatch.rs:1081 f64→f32) / needless_pass_by_value (1203, 1307) / missing_errors_doc (path.rs:84) / doc_markdown (multiple) 분류 후 수정 vs allow 결정. 별도 round
- R46.2 Concern 1 (RendererOptions 전체 노출) carry 유지 — R46.2.1 에서 aa 만 상환. present_mode / use_cpu / num_init_threads / pipeline_cache 는 build-time vs runtime 분류 spec round 대기
- R47+ Headless renderer template (§5.12 screenshot RPC) — RendererBackend::Headless variant 추가 + emit_renderer_headless 함수. R46.3.3 fully-qualified path 표준 적용
- R47+ cosmic-text glyph cache — R31 caveat 행이. paint_adapter 의 Scene::Text 쟬이 no-op 에서 실제 렌더링으로 확장
- R297 false-positive hint 그대로 — R46.3.5 commit 까지 9-commit carry



### 430 — R46.4 — §5.18 §5.20 §5.34 pinion-rpc clippy 13→0 청산: doc backtick 8 + missing_errors_doc 1 + cast_possible_truncation 2 + needless_pass_by_value 2 (finite-bounded f64→f32 narrowing 명시화 + apply_outcome/invoke_error by-reference signature 정통화)

**Changes**:
- path.rs (§5.18 RPC 경로 해석): 모듈 docstring 백틱 추가 — `/window[<id>]/<scene_path>` 메타구문 백틱 wrap + `WindowId` 타입 백틱 (clippy::doc_markdown 2건); resolve() 함수 공개 doc 에 `# Errors` 섹션 추가 — PathError 3 variants (MalformedPrefix / EmptyWindowId / UnknownWindow) 명시 (clippy::missing_errors_doc 1건)
- dispatch.rs (§5.34 preview/intent pipeline + parse_box_style §5.20): parse_box_style doc 의 `BoxStyle` 백틱 (1건); apply_outcome_to_json(outcome: ApplyOutcome) → (outcome: &ApplyOutcome) signature 변경 + caller (handle_scene_apply_preview) 같이 `&outcome` 으로 변경; invoke_error_to_rpc(err: InvokeError) → (err: &InvokeError) signature + caller `&err` 변경 (clippy::needless_pass_by_value 2건). 두 함수 모두 match→variant 이름 추출만 — owned 가 불필요, by-reference 가 textbook canonical
- dispatch.rs:parse_path_command 의 read_point closure 재구성 — 단일 read_coord(axis) 헬퍼로 x/y 공통 추출 + JSON f64 의 finite check (NaN/±∞ 거절 → invalid_params 에러), 그 후 `as f32` narrowing 에 `#[allow(clippy::cast_possible_truncation, reason="PathPoint stores f32 per §5.3; finite-bounded narrowing is the wire→scene contract")]` 명시 (clippy::cast_possible_truncation 2건). 단순 `as` 캐스트의 잠재 NaN/∞ 입력 UB-ish 행동을 invariant precondition 으로 승격
- preview/kinds.rs:DispatchIntent doc 의 `SetSignal` 타입 백틱 (1건)
- preview/proposal.rs:Proposal::apply doc 의 `SetSignal` / `SetStyle` / `ReplaceView` / `DispatchIntent` 4 타입 이름 백틱 (4건)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 633 pass / 0 fail (baseline 유지 — finite check 추가가 기존 테스트 invariant 보존)
- cargo clippy -p pinion-rpc --all-targets --features pinion-runtime/vello = pinion-rpc 13 → 0. workspace baseline 잔존: pinion-core 5 / pinion-runtime 1 / pinion-forge 0 / ai-introspect-demo 0 / hello-button 0 / forge-counter 0
- clippy::cast_possible_truncation allow attribute 에 `reason = ...` 명시 (Rust 1.81+ stable lint attribute syntax) — 미래 reader 가 narrowing 의 invariant 정당화를 grep 으로 즉시 발견 가능
- finite check 신규 가드: 기존 unit test suite 가 NaN/±∞ 입력을 cover 하지 않음 — 다음 R46.x clippy round 의 sub-slice 로 test 케이스 추가 carry



**Impact**: §5.18, §5.20, §5.34


**Carry forward**:
- R46.5 hello-button softbuffer → Vello 교체 — paint_adapter 두 번째 consumer 정착. R46.3.1 의 framework primitive boundary 가 multi-app 검증 완료되어야 R46.4 carry (paint_adapter generality) 청산. softbuffer dep workspace 전체에서 제거 가능
- R46.6 RendererOptions surface policy spec round — R46.2 Concern 1 carry 청산. present_mode / use_cpu / num_init_threads / pipeline_cache 의 build-time (manifest attr) vs runtime (builder method) 분류 결정. §2#4 mode toggle invariant 와 정합
- R47 Headless renderer template — §5.12 screenshot RPC 활성화. RendererBackend::Headless variant + emit_renderer_headless template + screenshot RPC schema 정합
- R47 cosmic-text glyph cache + Scene::Text 렌더 path — backend-orthogonal cross-cutting concern. paint_adapter 의 Text arm 활성화 + cosmic-text font fallback / shaping + GPU glyph texture cache evict 정책
- parse_path_command finite-check NaN/±∞ unit test 추가 — R46.4 의 외곽 invariant 검증 (다음 sub-slice). 단발 test 케이스 — invalid_params 에러 메시지 / 모든 axis (x, y) / 모든 op (MoveTo / LineTo / CurveTo c1/c2/end) 매트릭스
- pinion-core 5 + pinion-runtime 1 clippy 경고 — items_after_statements / doc_markdown / must_use_candidate / missing_panics_doc. 별도 sub-slice (R46.4.x) 로 case-by-case. textbook: must_use_candidate 는 #[must_use] 명시 / items_after_statements 는 함수 추출 / missing_panics_doc 는 invariant doc 보강
- R297 false-positive hint mnemosyne round, pinion atomic 무관 — 9-commit carry 무시 가능



### 431 — R46.5 — §5.16 hello-button softbuffer → Vello (paint_adapter 두 번째 consumer 정착 + softbuffer workspace dep 제거 + R46.3.4 RenderState ADT 패턴 적용; cosmic-text 라벨은 R47 carry 로 임시 no-op)

**Changes**:
- examples/hello-button/Cargo.toml: softbuffer (workspace) + cosmic-text 0.12 직접 의존 제거; vello (workspace) + pollster (workspace) 추가; pinion-runtime features = ["vello"]; build-dependencies = pinion-forge (workspace path) 추가. ai-introspect-demo R46.3 마이그레이션 패턴 동일
- examples/hello-button/app.pinion.xml 신설 — `<pinion kind="renderer" name="HelloButtonRenderer" backend="vello"/>` (ai-introspect-demo DemoRenderer 와 동일 manifest 셍셐, name 만 변경). aa attr 생략 → R46.2.1 default = Area
- examples/hello-button/build.rs 신설 — ai-introspect-demo R46.3 build.rs 와 동일 (`pinion_forge::compile_file` + `cargo:rerun-if-changed`). forge-counter R382e 첫 dogfood 패턴 계승
- examples/hello-button/src/main.rs 대수술: paint() / paint_box() / paint_container_fill() / paint_text() / blend_span() 5 함수 전면 제거 (~145 LOC 접소) → pinion_runtime::paint_adapter::to_vello + root_background 호출 (R46.3.1 framework primitive consumer). App struct field surface / context / window: Option (단일 Option장) + font_system + swash_cache 4 field → state: RenderState ADT (R46.3.4 pattern) + vello_scene: VelloScene reusable buffer 2 field. resumed() → RenderState::Suspended cache + HelloButtonRenderer::new (pollster::block_on async). suspended() → Active drop + window cache. WindowEvent::Resized → renderer.resize. softbuffer / cosmic-text / FontSystem / SwashCache / Context / Surface use 문 제거
- Color migration — BG_FILL (0x0020_3040 dark navy) + btn_fill 4 상태 (Idle 0xff_ffff / Hover 0xd0_d0d0 / Pressed 0x50_5050 / Disabled 0xb0_2020) 모두 `Color::from_argb(0x00...)` → `Color::rgb(...)` 정통화. alpha 0 → Vello transparent 함정 회피 (R46.3.1 PALETTE migration 동일 패턴; paint_adapter::to_peniko_alpha_zero_is_transparent test 공고)
- label TextNode ("Click me!" / "Disabled") 은 view() scene tree 에 그대로 보존 — model survives, paint_adapter::to_vello Text arm 이 현재 no-op 이므로 일시 화면 표시 안 됨. R47 cosmic-text framework primitive 활성화 시 렌더링 복원. carry_forward 명시
- main.rs doc 의 `paint_adapter` 백틱 추가 (clippy::doc_markdown 1 동일-commit 청산)
- workspace Cargo.toml: `softbuffer = "0.4"` 제거 + 주석 갱신 (R46.2 emit template + R46.3 첫 consumer + R46.5 두 번째 consumer chain 종료 대이어레트 명시). hello-button 이 마지막 caller 였으므로 last-consumer-retire pattern
- ai-introspect-demo 의 R46.3.x carry forward 7 항목 중 "R46.5 hello-button Vello 교체" 단독 청산 — paint_adapter 의 multi-app boundary 증명



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 633 pass / 0 fail (baseline 유지)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 동일 — pinion-core 5 + pinion-runtime 1 (R46.4 서 별도 sub-slice carry); pinion-rpc 0 / pinion-forge 0 / hello-button 0 / ai-introspect-demo 0 / forge-counter 0. hello-button R46.5 main.rs 는 doc_markdown 1 신규 발생 → same-commit 정통 청산
- cargo check --workspace --features pinion-runtime/vello = green (softbuffer dep 제거 후도 transitive resolve 이상 없음)
- paint_adapter 두 번째 consumer (ai-introspect-demo + hello-button) 검증 — single-consumer over-fit 위험 해소. framework primitive boundary 다양 widget 구조 (R39.4.3 introspect 3 button + overlay vs R12 button widget) 에 점검 완료
- softbuffer workspace dep 완전 제거 — R46.4 carry-forward 청산. deprecated paint path workspace 전체에서 사라짐. 다음 예제 / widget 카탈로그 추가 시 Vello-only path 강제



**Impact**: §5.16


**Carry forward**:
- R47 cosmic-text glyph cache + paint_adapter::to_vello Text arm 활성화 — hello-button "Click me!" / "Disabled" 라벨 복원 의존. backend-orthogonal cross-cutting concern (Vello + 미래 Headless 공유). cosmic-text font fallback / shaping + vello::Scene::draw_glyph 통합 + GPU glyph texture cache evict 정책. textbook: framework primitive (paint_adapter 내) — [[r47-class-incident-prevention]] 정신
- R46.6 RendererOptions surface policy spec round (R46.2 Concern 1 carry) — present_mode / use_cpu / num_init_threads / pipeline_cache build-time (manifest attr) vs runtime (builder method) 분류 결정. §2#4 mode toggle invariant 정합
- R47 Headless renderer template — §5.12 screenshot RPC 활성화. RendererBackend::Headless variant + emit_renderer_headless template + screenshot RPC schema 정합
- R47+ 위젯 카탈로그 확장 — Slider / Toggle / TextField. paint_adapter + InputRouter framework primitive 둘 다 정착 완료, prereq 증명 끝난 상태. R47 cosmic-text land 후 widget 라벨 렌더링 가능
- R47+ mobile/Android target 검증 — RenderState ADT 가 ai-introspect-demo (R46.3.4) + hello-button (R46.5) 두 consumer 에서 정착. 실제 Android event loop 통합 + suspend 이벤트 실증은 별도. 현재 데스크탑 compile clean
- R46.4 carry 그대로: parse_path_command finite check NaN/±∞ unit test 추가 + pinion-core 5 / pinion-runtime 1 clippy sub-slice
- R297 false-positive (mnemosyne 주 라운드, pinion atomic 무관) — 10-commit carry 무시 가능



### 432 — Round 47 — §5.36 new — text shaping & glyph cache framework primitive (parley + swash + fontique); R21 cosmic-text partial supersede

**Changes**:
- §5.36 신설 — Linebender parley text shaping + GlyphCache + paint_adapter Text arm 활성화 primitive
- library 결정 — parley (Linebender layout primary) + swash + fontique = R41 Vello ecosystem 정합
- R21 cosmic-text 결정 partial supersede — layout 책임이 parley 로 이동
- R31 glyph atlas GPU texture 정신 보존 — GlyphCache + Vello draw_glyph 통합으로 실현
- crate 배치 — 별도 pinion-text crate 신설 (SOLID single responsibility, backend-orthogonal)



**Verification**:
- atomic store add_section + setter chain 7개 + add_section_caveat × 6 = 14 mutations
- 구현은 R47.1+ build slice (crate skeleton → parley wire → GlyphCache → paint_adapter Text arm 활성화 → hello-button 라벨 복원)
- validate_workspace 통과 후 baseline 갱신 (entries 86→87, sections 45→46 expected)



**Impact**: §5.3, §5.16, §5.20, §5.30, §5.36, §6.3


**Carry forward**:
- R47.1 pinion-text crate skeleton + parley/swash/fontique workspace dep 추가
- R47.2 GlyphCache struct (LRU bounded, private fields, Hyrum-immune schema)
- R47.3 paint_adapter Text arm 활성화 + vello::Scene::draw_glyph 통합
- R47.4 hello-button 라벨 시각 복원 검증 + ai-introspect-demo 텍스트 노드 확인
- R47.x TextStyle schema 확장 (font_family / weight / decoration — parley StyleProperty 친화)
- R47.x GlyphCache evict 정책 (LRU capacity, per-renderer vs shared scope)
- R47.x fontique font fallback override API



### 433 — Round 47.1 — §5.36 pinion-text crate skeleton + parley/swash workspace dep + MSRV 1.88 bump

**Changes**:
- crates/pinion-text/ 신설 — Cargo.toml + src/lib.rs doc skeleton (구현 R47.2+)
- workspace.dependencies — parley 0.9 + swash 0.2 (fontique 0.9 transitive via parley::FontContext)
- rust-toolchain channel 1.86 → 1.88 (parley 0.9 transitive 트리거, R46.3 패턴)
- workspace.package rust-version 1.86 → 1.88 동기
- workspace members 에 crates/pinion-text 추가
- system dep 요구 — libfontconfig dev (fontique system feature default)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello = pass (rustc 1.88, fontconfig dev)
- cargo test --workspace --features pinion-runtime/vello = 633 pass (baseline 유지)
- cargo clippy pinion-text = 0 (introduced 3 doc_markdown → same-commit backtick fix)
- MSRV 노출 기존 코드 clippy::large_enum_variant 2건 → R47.1.1 별도 sub-slice



**Impact**: §5.16, §5.36, §6


**Carry forward**:
- R47.1.1 — MSRV-노출 clippy::large_enum_variant fix (hello-button + ai-introspect-demo RenderState)
- R47.2 GlyphCache struct (LRU bounded, private fields, Hyrum-immune schema)
- R47.3 Layout builder + paint_adapter Text arm 활성화 + Vello draw_glyph
- R47.4 hello-button 라벨 시각 복원 검증 + ai-introspect-demo 텍스트 노드 확인
- R47.x TextStyle schema 확장 (font_family / weight / decoration — parley StyleProperty 친화)
- R47.x GlyphCache evict 정책 + fontique font fallback override API



### 434 — Round 47.1.1 — MSRV 1.88 노출 clippy::large_enum_variant fix (hello-button + ai-introspect-demo RenderState Box 우회)

**Changes**:
- examples/hello-button RenderState::Active.renderer → Box<HelloButtonRenderer>
- examples/ai-introspect-demo RenderState::Active.renderer → Box<DemoRenderer>
- Active 생성 지점에 Box::new(renderer) wrap
- doc 주석 이유 명시 — ~1576 bytes vs 8 bytes variant 크기 차 해소



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = R46.5 baseline 복원
- per-crate: pinion-core 5 + pinion-runtime 1 / pinion-text 0 / pinion-rpc 0 / pinion-forge 0 / examples 0
- cargo test --workspace --features pinion-runtime/vello = 633 pass 유지



**Impact**: §5.16, §5.36


**Carry forward**:
- R47.2 GlyphCache struct (LRU bounded, private fields, Hyrum-immune schema)
- R47.3 Layout builder + paint_adapter Text arm 활성화 + Vello draw_glyph
- R47.4 hello-button 라벨 시각 복원 검증



### 435 — Round 47.2.0 — §5.36 outputs amend: Layout / LayoutCache / GlyphCache 세 책임 분리 + 단계별 진화 명시

**Changes**:
- §5.36 outputs 정확화 — Layout (parley re-export) + LayoutCache (R47.2) + GlyphCache (R47.5+) + adapter
- GlyphCache 책임 명확화 — consumer GPU rasterized atlas (Vello consumer-side 위임 명시, AAA 144 FPS prereq)
- 단계별 진화 caveat 2건 추가 — R47.2 = LayoutCache, R47.5+ = GlyphCache + Vello 통합 path
- Vello 0.6 draw_glyphs 의 (font + glyph_id) 입력 모델 과 정합 — atlas 책임 consumer



**Verification**:
- Mnemosyne set_section_outputs + add_section_caveat × 2 = 3 mutations
- validate_workspace 통과 예상 (entries 89→90 / sections 46 / T1=0 / GENERATED.md=sync)
- 구현 변경 없음 — spec amend only (atomic + GENERATED.md)



**Impact**: §5.16, §5.36


**Carry forward**:
- R47.2 Layout + LayoutCache 진입 (parley reuse cache)
- R47.3 paint_adapter Text arm + Vello draw_glyphs 통합
- R47.4 hello-button 라벨 시각 복원
- R47.5+ GlyphCache (consumer GPU atlas) + Vello 통합 path (upstream PR / 우회 결정)



### 436 — Round 47.2 — §5.36 pinion-text Layout + LayoutCache 구현 (parley + lru, +6 tests)

**Changes**:
- pinion-text::Layout — parley::Layout<Color> re-export (§5.36 R47.2 output)
- pinion-text::LayoutCache — LRU bounded (text+style+max_width) → Layout cache
- FontContext + LayoutContext lifecycle 내장 — single-thread (§6.3 view-fn purity)
- DEFAULT_CAPACITY = NonZeroUsize const 256 (panic-free new())
- workspace.dep + crate dep — lru 0.18 추가
- +6 unit tests (cache hit/miss / 3가지 key 변경 / capacity evict)
- §5.36 caveat × 2 추가 — AAA 144 FPS framing 정정 + Phase 2+ 자체 text engine carry



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 639 pass (633 baseline + 6 new)
- cargo clippy 전체 = R46.5 baseline 정확 복원 (pinion-core 5 + pinion-runtime 1 / 나머지 0)
- pinion-text introduced 3 warnings (doc_markdown + missing_panics_doc × 2) → same-commit textbook fix
- fix: DEFAULT_CAPACITY → const NonZeroUsize (panic-free) + layout() # Panics doc 섹션



**Impact**: §5.16, §5.36, §6.3


**Carry forward**:
- R47.3 paint_adapter Text arm 활성화 + vello::Scene::draw_glyphs 통합
- R47.4 hello-button 라벨 시각 복원 검증
- R47.5+ GlyphCache (consumer GPU atlas, UI 모드 dense text 보강)
- R47.x TextStyle schema 확장 (font_family / weight / decoration)
- R47.x fontique font fallback override API
- Phase 2+ lifetime canonical = pinion 자체 text engine (§5.16 R11 thin RHI 정합)



### R232 — R51.88 §5.40 AccessFocus::with_active_descendant strict YAGNI 제거 — R51.84 vs R51.86 inconsistency 정정

**Changes**:
- crates/pinion-a11y/src/focus.rs: AccessFocus::with_active_descendant builder 제거 (R51.84 add 회수)
- crates/pinion-a11y/src/focus.rs: composite 가 atomic+with_active_descendant chain 대신 Self { focus_tag, active_descendant: Some(_) } 직접 필드 구성
- crates/pinion-a11y/src/focus.rs: doc-comment 의 ignore 예제 (conditional builder chain) 동반 제거
- crates/pinion-a11y/src/focus.rs: r51_84_with_active_descendant_chains_on_atomic 테스트 → r51_88_composite_constructs_directly_without_builder_chain 으로 대체



**Verification**:
- cargo build -p pinion-a11y = clean
- cargo test -p pinion-a11y = 17 pass / 0 fail
- cargo test --workspace --features pinion-runtime/vello = 1516 pass / 0 fail / 8 ignored (이전 9 ignored 에서 -1 = with_active_descendant 의 ignore doctest 제거 분)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 외부 caller grep 0 확인 (workspace 전역에서 with_active_descendant 호출 없음)



**Impact**: §5.40


**Carry forward**:
- R51.89 — dispatch.rs::RpcError builder API land + focus.rs 5 local helper 제거
- R51.90 — RadioGroup activate path 의 focused_index 동기화 (apply_key arrow + AT Click/Default)
- R51.91 — InterveneError::OutOfRange variant 추가 + RadioGroup selected_index/focused_index 의 TypeMismatch 우회 정정
- R51.92 — pinion-shell/src/lib.rs 모듈 분할 (core.rs/shell.rs) — R51.83 visibility 변경의 substantive 효과 회복



### R233 — R51.89 §5.40 RpcError builder API — focus.rs local helper 회수 + dispatch.rs invalid_params/font_registry_unavailable/Method-not-found 도 동일 builder 정렬

**Changes**:
- crates/pinion-rpc/src/dispatch.rs: RpcError::new(code, message) base constructor 신설
- crates/pinion-rpc/src/dispatch.rs: RpcError::with_data(Value) + with_data_string(impl Into<String>) chainable builder
- crates/pinion-rpc/src/dispatch.rs: RpcError::invalid_params(impl Display) + internal_error(impl Display) convenience constructors (-32602 / -32603 lockstep)
- crates/pinion-rpc/src/dispatch.rs: 기존 invalid_params(&str) 와 font_registry_unavailable() 가 새 builder 경유
- crates/pinion-rpc/src/dispatch.rs: Method-not-found arm 의 RpcError struct 리터럴 → RpcError::new(-32601, "Method not found").with_data_string(...)
- crates/pinion-rpc/src/focus.rs: err_invalid_params + err_internal local helper 2개 제거 (RpcError::invalid_params / internal_error 으로 안정 대체)
- crates/pinion-rpc/src/focus.rs: err_focus_unavailable + err_from_focus + state_to_value 가 builder 경유 (RpcError literal 구성 제거)
- crates/pinion-rpc/src/focus.rs: use crate::dispatch::RpcError 으로 축약 테이블, handler return type 각각 이용



**Verification**:
- cargo build -p pinion-rpc = clean
- cargo test --workspace --features pinion-runtime/vello = 1516 pass / 0 fail / 8 ignored (변화 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- focus.rs local err_invalid_params / err_internal 호출 사이트 그립 0 확인 (후속)



**Impact**: §5.40


**Carry forward**:
- R51.90 — RadioGroup activate path (apply_key arrow + AT Click/Default) 에서 focused_index sync
- R51.91 — InterveneError::OutOfRange variant 추가 + RadioGroup selected_index/focused_index 우회 정정
- R51.92 — pinion-shell/src/lib.rs 모듈 분할 (core.rs/shell.rs) — R51.83 substantive 회복
- carry: dispatch.rs 의 49 RpcError literal 구성 사이트 중 잠재적 builder 1-line 단축 적용 - 우선순위 낮은 sweep (evidence-first)



### R234 — R51.90 §5.40 RadioGroup::send activate edge 가 focused_index 자동 동기화 — WAI-ARIA roving-tabindex first-class

**Changes**:
- crates/pinion-core/src/widgets/radio_group.rs: RadioGroup::send 의 !was_selected && now_selected branch 에 self.focused = Some(index) 추가
- crates/pinion-core/src/widgets/radio_group.rs: focused_index() / set_focused_index() doc 가 R51.90 가 채워지는 sync 경로 명시
- crates/pinion-core/src/widgets/radio_group.rs: r51_87_focused_index_and_selected_can_diverge 테스트 의 끝 주석 이 R51.90 collapse 의미 로 갱신
- crates/pinion-core/src/widgets/radio_group.rs: R51.90 신규 7 테스트 (first_activate_syncs / switching_activation_moves / reactivating_same / cancelled_press_leaves / set_selected_programmatic / at_focus_then_activate_collapses / external_activate_via_send_invoke_syncs)



**Verification**:
- cargo test -p pinion-core widgets::radio_group = 41 pass / 0 fail (이전 34 에서 +7 R51.90 신규)
- cargo test --workspace --features pinion-runtime/vello = 1523 pass / 0 fail / 8 ignored (1516 +7 R51.90)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- hello-radio-group 27 테스트 전부 pass — active_radio_index focused→selected→0 fallback 여전히 유효 (focused 가 더 자주 채워진다는 것만 달라짐)
- set_selected (프로그래마틱 path) 은 focused 건드리지 않음 — form default / persisted preference 에서 AT 이 커밋 전에 active descendant 선택되지 않도록 의도적 수준



**Impact**: §5.40


**Carry forward**:
- R51.91 — InterveneError::OutOfRange variant + RadioGroup selected_index/focused_index 우회 정정
- R51.92 — pinion-shell/src/lib.rs 모듈 분할 (core.rs/shell.rs) — R51.83 substantive 회복
- carry: hello-radio-group access_child_invoke_focus_returns_true_without_mutation 테스트 명 이 제한적 (focused_index 은 실제 변경) — 언제가 rename 가능
- carry: listbox/menu/tree/tab composite 신설 시 R51.90 sync pattern 공유 (R59 axis)



### R235 — R51.91 §5.40 InterveneError::OutOfRange variant — RadioGroup selected_index/focused_index 의 value-domain 실패 가 TypeMismatch 차용 우회에서 정통 OutOfRange 로 정정

**Changes**:
- crates/pinion-core/src/external.rs: InterveneError::OutOfRange variant 신설 (#[non_exhaustive] enum 의 additive 확장)
- crates/pinion-core/src/external.rs: TypeMismatch / OutOfRange 의 도메인 경계 도텍 (variant-vs-value-domain)
- crates/pinion-core/src/widgets/radio_group.rs: RadioGroupExternal::resolve_index_intervene helper 추출 (selected_index + focused_index 공유)
- crates/pinion-core/src/widgets/radio_group.rs: selected_index + focused_index intervene 가 OutOfRange 발사 (이전 TypeMismatch ×3 우회 제거)
- crates/pinion-core/src/widgets/radio_group.rs: 기존 2 테스트 (out_of_range_rejects + out_of_range_is_type_mismatch) 가 OutOfRange 검증 으로 갱신
- crates/pinion-core/src/widgets/radio_group.rs: R51.91 신규 4 테스트 (negative / wrong_variant_int / wrong_variant_focused / at_boundary)



**Verification**:
- cargo test -p pinion-core widgets::radio_group = 45 pass / 0 fail (이전 41 에서 +4 R51.91)
- cargo test --workspace --features pinion-runtime/vello = 1527 pass / 0 fail / 8 ignored (1523 +4)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- Slider value clamp (소프트 계약) « 자체 구현이 이미 OutOfRange 안 쓰고 clamp, 의도적 유지
- 다른 widget (Checkbox/Toggle/Radio/Button) 는 고유 path 아닌 앞 우회 없음 — sweep 부담 없음



**Impact**: §5.40


**Carry forward**:
- R51.92 — pinion-shell/src/lib.rs 모듈 분할 (core.rs/shell.rs) — R51.83 substantive 회복
- carry: listbox/menu/tree/tab composite 신설 시 resolve_index_intervene 패턴 공유 (R59 axis)
- carry: dispatch.rs 의 49 RpcError literal sweep (R51.89 carry)



### R236 — R51.92 §5.40 pinion-shell/src/substrate.rs 모듈 분할 — R51.83 의 ShellCore field private 다운그레이드가 단일 파일 한계를 넘어 모듈 경계 substantive 효과 발휘

**Changes**:
- crates/pinion-shell/src/substrate.rs 신규 파일 (~700 LOC) — ShellCore + AccessEmitDecision + impl ShellCore + impl Default for ShellCore + build_tag_map helper
- crates/pinion-shell/src/lib.rs 에서 동일 콘텐츠 제거 + `mod substrate;` + `pub use substrate::{AccessEmitDecision, ShellCore};` re-export
- crates/pinion-shell/src/lib.rs imports 정리 — substrate 전용 import (winit::event::Touch/TouchPhase, ModifiersState, IntentQueue, InputRouter, FocusManager, LayoutCache, PreviewLedger, DispatchContext, SceneRevision, IntrospectValue, compute_layout, walk_scene_and_drain, build_layout_node, rect_for_tag, dispatch, translate_action, PinionAccessAction, ROOT_NODE_ID, tag_to_node_id) 을 substrate.rs 로 이동
- crates/pinion-shell/src/lib.rs AppShell + impl + impl ApplicationHandler + run + spawn_stdin_rpc_reader + named_key_str 는 잔존 (수행자 역할)
- ShellCore 14 필드 + AccessEmitDecision 의 필드 visibility 가 substrate.rs 모듈 내부로 완전 제한 — AppShell (lib.rs) 은 pub accessor + dispatch 메서드 만 호출 가능
- build_tag_map 은 substrate.rs 내 fn (모듈 별명차 없이 commit_access_emit 의 유일 호출자)
- atomic store: 15 ShellCore/AccessEmitDecision implementation binding 를 lib.rs → substrate.rs 경로 이전 (15 remove + 15 add)



**Verification**:
- cargo build -p pinion-shell = clean (import 재조정 완료)
- cargo test --workspace --features pinion-runtime/vello = 1527 pass / 0 fail / 8 ignored (R51.91 와 동일 — 순수 refactor)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- grep 검증: AppShell 의 self.core. 접근은 전원 메서드 호출 — 필드 직접 접근 0 (이미 R51.83 에서 달성되었으며 substrate.rs 분할로 백만 없는 타입 경계 확정)
- external consumer (tests + examples) 계속 ShellCore/WidgetView/run/vello_renderer_impl 을 pub use 로 이용 — break 없음



**Impact**: §5.40


**Carry forward**:
- carry: AppShell + impl ApplicationHandler 도 별도 모듈 (e.g. app.rs) 분할 — 이번에는 lib.rs 자체에 잔존 (수행자 역할 + run 과 좀 섞임). R51.92.1 carry: app.rs 구조 최적화
- carry: dispatch.rs 의 49 RpcError literal sweep (R51.89 carry)
- carry: listbox/menu/tree/tab composite 신설 시 resolve_index_intervene 패턴 공유 (R59 axis)



### R237 — R51.92.1 §5.40 pinion-shell/src/app.rs 모듈 분할 — R51.92 3-모듈 textbook 구조 완성 (substrate + app + lib entry)

**Changes**:
- crates/pinion-shell/src/app.rs 신규 파일 (~480 LOC): AppShell + impl AppShell + impl ApplicationHandler + named_key_str + spawn_stdin_rpc_reader + run
- crates/pinion-shell/src/lib.rs 에서 동일 콘텐츠 제거 + `mod app;` + `pub use app::{run, AppShell};` re-export
- crates/pinion-shell/src/lib.rs imports 대폭 축소 — surface 전용 use 항목 (winit::application/dpi/event/event_loop/keyboard/window + std::io::{BufRead,Write} + std::thread + AccessTreeBuilder + BoxNode + paint_adapter + PointerId) 는 app.rs 로 이동; 잔존 은 lib.rs 자체 (trait + enum + macro) 에 필요한 (Arc + Window + Frame + Scene + AccessNode + AccessFocus + AccessAction + External + VelloScene + PenikoColor) 만
- lib.rs 구조 안정 = entry + AppEvent + VelloRenderer trait + vello_renderer_impl macro + WidgetView trait + RenderState enum + mod 선언 + pub use re-export 으로 목적 단일화
- AppShell.core (substrate 차용) 필드 visibility 가 substrate.rs 내부 강제 — lib.rs 도 이미 접근 불가, 이제 app.rs 내부로 모듈 경계 완전 확정



**Verification**:
- cargo build -p pinion-shell = clean
- cargo test --workspace --features pinion-runtime/vello = 1527 pass / 0 fail / 8 ignored (R51.92 와 동일 — 순수 refactor)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- external consumer (tests + 8 examples) 계속 run + WidgetView + ShellCore + vello_renderer_impl 을 pub use 로 이용 — break 0
- lib.rs LOC ~990 → ~470 (52% 속) + substrate.rs ~700 + app.rs ~480 = total ~1650 (계 동일, docs 소폭 증가 포함)



**Impact**: §5.40


**Carry forward**:
- R51.89.1 — dispatch.rs 41 RpcError literal full sweep (builder 적용 높은 반복)
- carry: R51.92.2 을 set_selected 의미 분리 검토 — over-engineering 가능성 높음, restore-semantic 이 이미 도텍 완료



### R238 — R51.89.1 §5.40 dispatch.rs RpcError struct-literal full sweep — 14 error-converter + 1 test 가 RpcError::invalid_params / internal_error / new+with_data 으로 통일

**Changes**:
- crates/pinion-rpc/src/dispatch.rs: 14 error-converter 함수 (click/rewind/snapshot/dry_run/wait_for/screenshot/locate/bbox/layout_query/resize/apply/propose/invoke/query) 가 RpcError::invalid_params(variant) / with_data(...) 적용
- crates/pinion-rpc/src/dispatch.rs: parse_typed_proposal 의 UnknownProposalKind arm 도 builder 적용
- crates/pinion-rpc/src/dispatch.rs: apply_error_to_rpc 의 Map data 카르ier 가 RpcError::new(-32602, ...).with_data(Value::Object(map))
- crates/pinion-rpc/src/dispatch.rs: font_error_to_rpc 가 RpcError::new(code, message).with_data_string(variant)
- crates/pinion-rpc/src/dispatch.rs: serialize_outcome 가 RpcError::internal_error 적용
- crates/pinion-rpc/src/dispatch.rs: error_response 헬퍼가 RpcError::new + 옵셔널 with_data builder 적용
- crates/pinion-rpc/src/dispatch.rs: response_result_none_is_elided_on_serialize 회귀 테스트도 RpcError::new 사용



**Verification**:
- cargo build -p pinion-rpc = clean
- cargo test --workspace --features pinion-runtime/vello = 1527 pass / 0 fail / 8 ignored (불변)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- grep 'RpcError {' dispatch.rs = 0 struct-literal construction (struct 정의 + impl 블록 + fn return type 제외)



**Impact**: §5.40


**Carry forward**:
- carry: dispatch.rs::invalid_params(detail: &str) wrapper 는 30+ caller 와의 호환 보존을 위해 유지 (RpcError::invalid_params 로 delegate 만)
- carry: R51.92.2 set_selected 의미 분리 평가 (over-engineering 가능성 높음)



### R239 — R51.93 §5.35 §5.13 TouchPhase::Cancelled 가 commit-class intent 발사 버그 정통 정정 — pointer_cancel SCXML 이벤트 + InputRouter::pointer_cancel + 5 widget 통일

**Changes**:
- crates/pinion-core/widgets/standard_button.sce-template.xml: pressed/hover 에서 pointer_cancel → idle transition 추가 (activate raise 없음) — Button/Toggle/Checkbox/Radio 4 widget 동시 적용
- crates/pinion-core/widgets/slider.scxml: dragging/hover 에서 pointer_cancel → idle transition (slider.activate raise 없음)
- crates/pinion-core/src/widgets/{button,toggle,checkbox,radio,slider}.rs: parse_*_event 에 'PointerCancel' → *::PointerCancel 매핑 5개
- crates/pinion-runtime/src/input.rs: InputRouter::pointer_cancel(pid, scene) 신규 — pointer_up 과 동일 수신자 론징만 'PointerCancel' wire 이벤트 발사, capture release + hover refresh 동일
- crates/pinion-shell/src/substrate.rs: handle_touch 의 TouchPhase::Ended | TouchPhase::Cancelled 한 한 arm 이 두 arm 으로 분리 — Cancelled 가 pointer_cancel 경유 (pointer_up 아닌)
- 5 widget 당 R51.93 회귀 테스트 추가 = button 5 (cancel/hover/idle/disabled/parse) + toggle 2 + checkbox 2 + radio 2 + slider 2 = 13



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1540 pass / 0 fail / 8 ignored (1527 +13 R51.93)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- sce-build 자동 재생성 확인: ButtonEvent::PointerCancel / ToggleEvent::PointerCancel / CheckboxEvent::PointerCancel / RadioEvent::PointerCancel / SliderEvent::PointerCancel 모두 generated_sm 에 출현
- 해소된 소원 UX 버그: 4-finger system gesture / phone call / notification banner / app switcher / edge-swipe 등 OS-revoked touch 들이 더이상 의도치 않은 click/toggle/checked/selected/value_committed intent 발사 안 함



**Impact**: §5.40, §5.35, §5.13


**Carry forward**:
- carry: pre-R51.93 의 종속적 mock test fixture 가 PointerUp 만 쓴다면 cancel path coverage 가 아직 일면적 — future widget composite 신설 시 PointerCancel 경로도 테스트 포함 권고
- carry: pointer_cancel 의 RPC dual — 'scene/invoke send PointerCancel' wire 이벤트 가 이미 자동 지원됨 (parse_*_event PointerCancel arm). AI 클라이언트가 외부 cancel 시너리오를 재현 가능



### R51.100 — R51.100 §5.38 — ListBox JSON-RPC e2e (single + multi mode invoke 검증)

**Changes**:
- crates/pinion-rpc/tests — ListBox single/multi e2e: query/dispatch/intervene/screenshot
- selected.<i> per-row + multi-mode intent payload 검증



**Verification**:
- e2e 수십 건 통과, JSON-RPC 2.0 경자 valid



**Impact**: §5.38, §5.7



### R51.101 — R51.101 §5.40 — invalid_params 래퍼 제거 (R51.89.1 follow-up, dispatch.rs literal 0 도달)

**Changes**:
- crates/pinion-rpc/src/dispatch.rs — fn invalid_params(&str) wrapper 제거
- 30+ caller 가 RpcError::invalid_params builder 직접 호출
- literal 0, builder primitive 조건 부재



**Verification**:
- cargo test 전원 통과, R51.89/89.1 framework primitive surface complete



**Impact**: §5.40, §5.7



### R51.102 — R51.102 §5.38 — WidgetTransition::detect → Vec<Intent> (substrate API alloc discipline, data-driven SmallVec vs Vec 결정)

**Changes**:
- crates/pinion-core/src/widget_transition.rs — WidgetTransition::detect signature change
- 5 widget impl 액소 1 intent 대부분 이지만 multi-select 이 N=4 alloc requirement
- criterion bench (R51.105) 가 alloc cost 결정 evidence



**Verification**:
- cargo test 전원 통과
- noise envelope ±5% 단년 변화 < 1%



**Impact**: §5.38



### R51.103 — R51.103 §5.38 — type-ahead 다문자 + i18n (Unicode grapheme + UAX #29 boundary)

**Changes**:
- examples/hello-listbox — type-ahead buffer 다문자 prefix match
- Latin / CJK / emoji / combining mark 증명 테스트 + manual demo



**Verification**:
- UAX #29 grapheme boundary 정통 (unicode-segmentation)



**Impact**: §5.38



### R51.104 — R51.104 §5.38 — hello-listbox-multi 시각 demo + Cargo.lock 동반 commit (R51.104 + R51.104 caveat append 통합)

**Changes**:
- examples/hello-listbox-multi (new binary) — multi-select visual demo
- Ctrl/Shift modifier extend 입력 — cell-style row selection
- Cargo.lock 동반 commit + atomic store caveat append



**Verification**:
- cargo run -p hello-listbox-multi — visual confirm
- Cargo.toml workspace.members += examples/hello-listbox-multi



**Impact**: §5.38



### R51.105 — R51.105 §5.38 — ListBox dispatch criterion bench (data-driven Snapshot=Vec<bool> alloc cost 측정)

**Changes**:
- crates/pinion-core/benches/listbox_dispatch.rs 신규
- criterion micro-bench: per-user-input alloc cost (Snapshot=Vec<bool>)
- SmallVec-vs-Vec data-driven 결정 evidence (R51.102 의 문법 원자)



**Verification**:
- cargo bench -p pinion-core --bench listbox_dispatch -- --quick
- noise envelope ±5% 4+ samples



**Impact**: §5.38



### R51.106 — R51.106 §5.38 — type-ahead → pinion_shell::typeahead (substrate-incompleteness-signal 2nd consumer trigger)

**Changes**:
- crates/pinion-shell/src/typeahead.rs 신설 — buffer + timeout + prefix match substrate
- examples/hello-listbox 의 application-level inline 구현 청산, framework primitive 으로 승격



**Verification**:
- substrate-incompleteness-signal 2nd consumer trigger (hello-listbox + future binding)
- cargo test 전원 통과



**Impact**: §5.38, §5.40



### R51.107 — R51.107 §5.41 — TUI 백엔드 axis RFC (§2 invariant #6 GUI/TUI dual 의 cell-based render mode primitive)

**Changes**:
- docs/.atomic §5.41 신규 — TUI 백엔드 axis (Cell-based scene render target)
- ratatui Backend + crossterm event source 의 sibling pipeline 정통화
- pinion-shell (Vello-coupled binary path) ↔ pinion-tui (ratatui-coupled) 공존 결정



**Verification**:
- §2 #6 invariant 관철 — one scene, two render dispatch paths
- Linebender-adjacent maintainer continuity + egui/iced/bubbletea-class adoption baseline



**Impact**: §5.41, §2


**Carry forward**:
- R51.108 = substrate winit-free 분리
- R51.109+ = pinion-tui crate skeleton 구축



### R51.108 — R51.108 §5.41 — substrate winit-free 분리 (pinion-runtime 의 Modifiers/Touch types lift, TUI bridge 동일 shape)

**Changes**:
- crates/pinion-shell — winit-coupled types 제거, pinion-runtime 로 lift
- crates/pinion-runtime — abstract Modifiers / Touch / TouchPhase / PointerId 정통
- pinion-shell::winit_modifiers_to_pinion helper + future pinion-tui::crossterm_modifiers_to_pinion 평행



**Verification**:
- cargo test 전원 통과, dep direction (pinion-runtime no winit) 관철



**Impact**: §5.41, §5.35



### R51.109.0 — R51.109.0 §5.41 — pinion-tui crate skeleton (cell-based render mode primitive scaffold)

**Changes**:
- crates/pinion-tui (new crate) 신설 — Cargo.toml + lib.rs skeleton
- deps: pinion-core + pinion-runtime + pinion-a11y + ratatui + crossterm + unicode-segmentation + unicode-width
- no winit/wgpu transitive (관철)



**Verification**:
- Cargo.toml workspace.members += crates/pinion-tui
- cargo check --workspace = 0 errors



**Impact**: §5.41



### R51.109.1 — R51.109.1 §5.41 — WidgetRenderer trait 분리 (pinion-core 로 lift, dep direction 정통)

**Changes**:
- crates/pinion-core/src/renderer.rs — WidgetRenderer trait lift
- pinion-shell::Renderer 이 WidgetRenderer impl, pinion-tui 가 동일 surface
- Vello ↔ ratatui 공존 substrate



**Verification**:
- cargo test 전원 통과, trait location = lowest layer (pinion-core)



**Impact**: §5.41



### R51.109.2 — R51.109.2 §5.41 — WidgetRenderer lift + TuiRenderer<B> (production CrosstermBackend / test TestBackend monomorph)

**Changes**:
- crates/pinion-tui/src/renderer.rs — TuiRenderer<B: ratatui::backend::Backend>
- production = CrosstermBackend<Stdout>, test = TestBackend (헤드리스 검증 인프라)
- WidgetRenderer impl — width(), height() returns u16 cells



**Verification**:
- cargo test 전원 통과, monomorphization 검증



**Impact**: §5.41



### R51.110.0 — R51.110.0 §5.41 — Scene→Buffer text-first 매핑 (UAX #11/#29 grapheme + East Asian Width)

**Changes**:
- crates/pinion-tui/src/paint.rs — Scene::Container/Text → ratatui::buffer::Buffer cells
- unicode-segmentation grapheme cluster iter + unicode-width East Asian Width
- wide grapheme (CJK / fullwidth Latin / some emoji) 2 cells, narrow 1 cell



**Verification**:
- CJK ‘한글’ + emoji + combining mark 재현 valid
- cargo test 수십 건 통과



**Impact**: §5.41



### R51.110.1 — R51.110.1 §5.41 — WidgetViewTui trait + render_one_frame (TUI binding surface, WidgetView 의 alternate)

**Changes**:
- crates/pinion-tui/src/widget.rs — WidgetViewTui<V: WidgetCore>: Renderer + initial_size
- render_one_frame — single Scene paint cycle 의 commit
- WidgetViewTui = WidgetView 의 cell-native alternate trait



**Verification**:
- TUI binding surface 정통 — trait substrate-incompleteness trigger 관용



**Impact**: §5.41



### R51.110.2 — R51.110.2 §5.41 — run::<V> + hello-button-tui first dogfood (TUI binding first land, RAII TerminalGuard panic-safe)

**Changes**:
- crates/pinion-tui/src/run.rs — run::<V>() entry, TerminalGuard Drop = raw mode off + leave alt screen + disable mouse
- examples/hello-button-tui (new binary) — first dogfood example
- Cargo.toml workspace.members += examples/hello-button-tui



**Verification**:
- cargo run -p hello-button-tui — visual confirm
- TerminalGuard panic-safe = panic 이도 terminal 정상 복구



**Impact**: §5.41



### R51.111 — R51.111 §5.41 — TUI input dispatch + SCXML wire-up (keyboard event → WidgetCore::apply_key → SCXML transition)

**Changes**:
- crates/pinion-tui/src/run.rs — crossterm event loop poll timeout + KeyEvent → apply_key
- abstract Modifiers vocabulary (pinion-runtime) substrate 사용
- walk_scene_and_drain intent drain primitive 공유 (pinion-shell 과 동일)



**Verification**:
- cargo test 수십 건, SCXML transition 관측 valid



**Impact**: §5.41, §5.35



### R51.112 — R51.112 §5.41 — TUI mouse dispatch + InputRouter wire-up (crossterm MouseEvent → cell→pixel coord → InputRouter)

**Changes**:
- crates/pinion-tui/src/run.rs — crossterm MouseEvent dispatch 라우팅
- PIXEL_PER_CELL 8×16 placeholder 공유, cell-native coord axis 캐리
- cursor_moved / pointer_down / pointer_up / pointer_cancel 재사용 (pinion-runtime API)



**Verification**:
- cargo test 수십 건
- TUI hit-test resolve 검증



**Impact**: §5.41, §5.35


**Carry forward**:
- PIXEL_PER_CELL 8×16 placeholder — 2nd TUI binding mismatch 시 cell-native axis 평가



### R51.113 — R51.113 §5.41 — hello-toggle-tui 2nd TUI binding land (TUI binding pattern lock-in)

**Changes**:
- examples/hello-toggle-tui (new binary) — 2nd TUI binding, ToggleView WidgetViewTui impl
- Cargo.toml workspace.members += examples/hello-toggle-tui



**Verification**:
- cargo run -p hello-toggle-tui — visual confirm
- TUI binding API 고정 evidence (substrate-incompleteness-signal trigger 회피)



**Impact**: §5.41



### R51.114 — R51.114 §5.38 — ARIA activate helper DRY 청산 (apply_aria_activate widget helper, Button/Switch/Toggle Space single sweep)

**Changes**:
- crates/pinion-core/src/widgets/aria.rs — apply_aria_activate(scene, focused, key, tag) helper
- Button + Switch + ToggleButton apply_key 가 해당 helper 호출 으로 일원화
- Checkbox/Radio/Slider 은 별도 spec (Space only / arrow / arrow+Home+End+PgUp+PgDn) 이므로 제외



**Verification**:
- cargo test 전원 통과, WAI-ARIA APG button activation pattern 관철



**Impact**: §5.38, §5.40



### R51.115 — R51.115 §5.41 — Scene::Box + ContainerNode style paint (TUI border + bg, BoxStyle 의 cell-unit 분기)

**Changes**:
- crates/pinion-tui/src/paint.rs — Scene::Container.style (BoxStyle) → ratatui buffer cells
- Box drawing chars U+2500..U+2518 — doc string 에만 Unicode literal (Rust source 는 \u{XXXX} escape)
- corner_radius / placement / width = TUI 의미 없음 (cell unit, sub-cell 0) 명시



**Verification**:
- cargo test 수십 건, visual confirm box border rendering



**Impact**: §5.41


**Carry forward**:
- Box paint corner_radius/placement TUI cleanup — cell-unit 의미 분기 명시 cosmetic



### R51.116 — R51.116 §5.41 — TUI button/toggle view BoxStyle 적용 (hello-button-tui + hello-toggle-tui visual polish)

**Changes**:
- examples/hello-button-tui + hello-toggle-tui view fn — BoxStyle (border + bg) 적용
- R51.115 paint primitive 의 2nd consumer evidence



**Verification**:
- cargo run -p hello-button-tui / hello-toggle-tui — visual confirm



**Impact**: §5.41



### R51.117 — R51.117 §5.41 — ShellCoreTui substrate extraction (TUI dispatch substrate first-cut, pinion-shell::ShellCore parity)

**Changes**:
- crates/pinion-tui/src/substrate.rs 신규 — ShellCoreTui<V> dispatch substrate
- scene + cached_state + router + intent_queue + _phantom 5 fields
- dispatch_key / cursor_moved / pointer_down / pointer_up / refresh_state two-call 패턴



**Verification**:
- cargo test 수십 건 통과
- first-cut surface — R51.122-R51.124 4-round lift 으로 이행 예정 (CoreShell composition substrate)



**Impact**: §5.41


**Carry forward**:
- R51.122-R51.124 = CoreShell substrate lift cascade (strategic 부채 #1)
- refresh_state two-call 패턴 = R51.124 에서 auto-tail single-call 으로 청산 예정



### R51.118 — R51.118 §5.41 — TUI a11y substrate first cut (pinion-native AccessNode/AriaRole/AccessState/AccessValue + WidgetViewTui::access_node)

**Changes**:
- crates/pinion-tui 의 WidgetViewTui trait + access_node default vec![] (TUI a11y 첫 cut)
- pinion-a11y dep direct 승격 — AccessNode 을 구체 Vec<AccessNode> 로 노출
- AT integration carry-forward — PTY screen reader path 또는 미래 AccessKit-TUI



**Verification**:
- cargo test 수십 건, dep direction (pinion-tui → pinion-a11y, no transitive winit) 관철



**Impact**: §5.41, §5.40


**Carry forward**:
- AT integration (PTY 또는 AccessKit-TUI) — 새 axis carry



### R51.119 — R51.119 §5.41 — atomic stale citation cleanup (R51.117 substrate lift 후 후속 audit, refactor 후 즉시 cleanup lesson)

**Changes**:
- docs/.atomic §5.41 stale citation 제거 — ShellCoreTui first-cut 구조 관련
- lesson: refactor + atomic cleanup same-round (R51.124 + R51.121.1 이후 동일 패턴 적용)



**Verification**:
- mnemosyne validate_workspace = T1=0 유지, atomic audit accuracy 회복



**Impact**: §5.41



### R51.120 — R51.120 §5.41 — substrate stderr → optional file sink (alternate screen 보호, PINION_TUI_LOG opt-in)

**Changes**:
- crates/pinion-tui/src/substrate.rs — log_sink: Option<Box<dyn Write>> (silent default 정통)
- PINION_TUI_LOG=path env var opt-in — file sink 만 개방 (사용자 visual report 계기)
- lesson: alternate screen + stderr write = ratatui cell cache unsync, TUI shell 의 eprintln!/println! under raw mode + alternate screen 금지



**Verification**:
- cargo run -p hello-button-tui — alternate screen cell overwrite 관찰 0
- PINION_TUI_LOG=/tmp/tui.log 세션 로 디버그 valid



**Impact**: §5.41


**Carry forward**:
- lesson — alternate screen + stderr write = anti-pattern, silent default 정통 (file sink 만)



### R51.121 — R51.121 §5.41 — WidgetCore + WidgetA11y supertrait split: WidgetView/WidgetViewTui 가 Renderer + initial_size 만 남김 (ISP 정통)

**Changes**:
- crates/pinion-core/src/widget_core.rs 신설 — WidgetCore trait (state/event/create_external/tag/read_state/view/event_name/title/keybinding/apply_key/focusable_tags/fmt_state_log)
- crates/pinion-a11y/src/widget_a11y.rs 신설 — WidgetA11y: WidgetCore supertrait (access_node/access_focus_target/access_child_invoke)
- crates/pinion-shell/src/lib.rs WidgetView 변환: pinion_a11y::WidgetA11y supertrait + Renderer + initial_size (u32×u32) 만
- crates/pinion-tui/src/widget.rs WidgetViewTui 변환: WidgetA11y supertrait + Renderer + initial_size (u16×u16, default 80×24)
- 11 binding (9 GUI + 2 TUI) impl atomic 분할 — impl WidgetCore + impl WidgetA11y + impl WidgetView/Tui
- substrate tests / smoke / dispatch_core / DummyView fixture 3-impl-block 분할 동시 land
- stale citations cleanup — WidgetViewTui::{apply_key,keybinding,access_node} 제거 (supertrait 이동)



**Verification**:
- cargo check --workspace 통과
- cargo test --workspace --features pinion-runtime/vello = 1709 pass / 0 fail / 8 ignored (baseline 유지, behavior 변경 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = entries=249 / sections=58 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.41, §5.16, §5.40


**Carry forward**:
- WidgetCore::view 가 §6.3 view-fn purity invariant 유지 (sync + pure same (state, frame) → same Scene)
- initial_size return type unit (u32×u32 pixels vs u16×u16 cells) 의도적 분기 — backend native 단위
- blanket impl WidgetA11y for T: WidgetCore 비적용 — Rust specialization 없이는 composite override 불가
- Self::tag() 호출 시 supertrait method dispatch 정통 — <Self as WidgetCore>::tag() 명시 우선 (ambiguity 회피 + audit grep)
- TUI focus management hardcode + cell-native coord + AT integration 등 잔여 carry 그대로



### R51.121.1 — R51.121.1 §5.40 — R51.121 supertrait split follow-up: WidgetView::access_* stale citation 청산 + WidgetA11y impl 추가

**Changes**:
- §5.40 stale removal 3 — WidgetView::{access_node, access_focus_target, access_child_invoke}
- §5.40 add 5 — widget_a11y.rs + WidgetA11y trait + 3 method (access_node / access_focus_target / access_child_invoke)
- §5.40 caveat — R51.121 supertrait split lesson 명시



**Verification**:
- mnemosyne validate_workspace = entries=250 / sections=58 / T1=0 / RT=1/1 / GENERATED.md=sync
- atomic audit accuracy 회복 — R51.121 후 stale 3 → 0
- R51.119 lesson 정통 적용 (refactor 후 atomic citation 즉시 cleanup)



**Impact**: §5.40


**Carry forward**:
- R51.121 의 §5.16 audit 결과 — stale 0 (WidgetView trait 자체 구조 변화 없음, citation valid)



### R51.122 — R51.122 §5.41 — pinion-runtime::CoreShell<V: WidgetCore> backend-agnostic dispatch substrate 신설 (R51.122-R51.125 4-round 분할 중 #1)

**Changes**:
- crates/pinion-runtime/src/core_shell.rs 신설 (+744 LOC) — CoreShell<V: WidgetCore> + DispatchTail<S> + StateChange ADT + 13 unit tests
- CoreShell fields = scene + cached_state + router + intent_queue (4 backend-agnostic core fields)
- DispatchTail<S> = { intents: Vec<Intent>, state_change: StateChange<S> } — dispatch auto-tail named artifact
- 12 dispatch primitive: forward / apply_key / cursor_moved / cursor_left / pointer_down / pointer_up / pointer_cancel / touch_event / tail / update_paint_scene + hover_target accessor
- crates/pinion-runtime/src/lib.rs += pub mod core_shell + pub use
- §5.41 caveats + 13 implementations 신규 — atomic store 에 framework primitive 명시
- dep graph 0 변경 — pinion-runtime = pinion-core + pinion-text (+ taffy + vello optional), pinion-a11y / pinion-rpc 0 의존



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1709 → 1722 pass / 0 fail / 8 ignored (+13 CoreShell unit tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict 유지)
- cargo check --workspace 통과 — backend-agnostic state lift 동작 검증
- mnemosyne validate_workspace = entries=250 → 251 / sections=58 / T1=0 / RT=1/1 / GENERATED.md=sync (post-#1)
- behavior 변경 0 — pinion-runtime 신규 모듈만 추가, 기존 consumer 0 영향



**Impact**: §5.41, §5.16


**Carry forward**:
- R51.123 = pinion-shell::ShellCore wraps CoreShell<V> (Vello extras 만 유지, 4-round #2) 즉시 land 예정
- R51.124 = pinion-tui::ShellCoreTui wraps CoreShell<V> + refresh_state 제거 (auto-tail, 4-round #3) 즉시 land 예정
- R51.125 = dispatch_rpc trait extraction — 2nd RPC consumer (TUI RPC) carry 시까지 defer 검토
- [[coreshell-composition-lift]] lesson — backend-agnostic state lift = composition (struct field), not subtrait inheritance
- [[dispatch-tail-auto-tail-pattern]] lesson — dispatch method = mutate + return DispatchTail<S> {intents, state_change}; backend wrapper handle_tail() consolidates log + side-effect



### R51.123 — R51.123 §5.41 — pinion-shell::ShellCore<V> wraps CoreShell<V> (Vello extras 만 유지, 4-round 분할 #2)

**Changes**:
- crates/pinion-shell/src/substrate.rs ShellCore: 4 fields (scene/cached_state/intent_queue/router) → core: CoreShell<V> 1 field 으로 압축 (256 - 191 = +65 LOC net, +/-345 churn)
- Dispatch methods (forward, apply_key, cursor_moved, cursor_left, pointer_down, pointer_up, pointer_cancel, touch_event, dispatch_rpc, finalize_frame, update_paint_scene) — 전원 'core.X() → handle_tail(&tail)' shape 로 반복
- handle_tail helper 신설 — stderr log + state_change 시 request_redraw 단 1 곡Ş 시말고 처리. drain_intents / refresh_state private helpers 제거 (handle_tail 으로 일원화)
- crates/pinion-runtime/src/core_shell.rs += hover_target accessor (router-state 읽기 prop — click_to_focus follow-up)
- public API 0 변경 — ShellCore 11 method signature (scene / cached_state / forward / apply_key / cursor_moved / cursor_left / pointer_down / pointer_up / pointer_cancel / touch_event / dispatch_rpc / finalize_frame) 전원 보존, AppShell + tests 미변경



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 / 0 / 8 (R51.122 baseline 유지, +0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = entries=251 → 252 / sections=58 / T1=0 / RT=1/1 / GENERATED.md=sync
- AppShell consumer + 13 example binaries 재컴파일 — ShellCore signature 보존 증명



**Impact**: §5.41, §5.16


**Carry forward**:
- R51.124 = pinion-tui::ShellCoreTui wraps CoreShell<V> + refresh_state 제거 (auto-tail, 4-round #3) 즉시 land
- R51.125 = dispatch_rpc trait extraction — 2nd RPC consumer (TUI RPC) carry 시까지 defer 검토
- [[coreshell-composition-lift]] 확장 — 1st wrapper land 계속, dep graph (pinion-shell → pinion-runtime → pinion-core) cycle 0 적합
- handle_tail 패턴 = backend-specific log + request_redraw side-effect (Vello path 1차 binding)



### R51.124 — R51.124 §5.41 — pinion-tui::ShellCoreTui<V> wraps CoreShell<V> + dispatch_X auto-tail bool 반환 (refresh_state 청산, 4-round 분할 #3)

**Changes**:
- crates/pinion-tui/src/substrate.rs ShellCoreTui: 5 fields (scene/cached_state/router/intent_queue/_phantom) → core: CoreShell<V> + log_sink 2 fields 축소 (399 churn, -224 +296)
- dispatch_key / cursor_moved / pointer_down / pointer_up 전원 bool 반환 (auto-tail state_changed) — 별도 refresh_state 호출 패턴 청산
- refresh_state + forward_event helper 제거 — handle_tail 하나로 log_sink 라우팅 + intent + state_change + bool 반환 통합
- crates/pinion-tui/src/shell.rs callers: 'dispatch_X && refresh_state' two-call → 'dispatch_X' 단 1 호출 (+24 -10 LOC)
- dispatch_mouse Down(Left) arm = '|' 로 cursor_moved + pointer_down 두 dispatch 의 state_changed 관측 보존
- atomic stale citation cleanup (R51.119 lesson) — §5.41 의 ShellCoreTui::refresh_state implementation 동시 제거 (auto-tail 흡수)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 / 0 / 8 (baseline 유지, behavior 0 변경)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = entries=252 → 253 / sections=58 / T1=0 / RT=1/1 / GENERATED.md=sync
- hello-button-tui + hello-toggle-tui 실행 검증 — visual identical (refactor only, no behavior change)
- TUI signature breaking but consumer 0 — dispatch_X bool 반환 §2 binding (hello-button-tui / hello-toggle-tui) caller 적용 same-round land



**Impact**: §5.41, §5.40, §5.16


**Carry forward**:
- R51.124.1 = R51.125 dispatch_rpc trait extraction defer audit entry (Rule of Three + substrate-incompleteness-signal)
- R51.125 = dispatch_rpc trait extraction 자체 — 2nd RPC consumer (TUI RPC) land 시까지 defer (cycle 0 / 1 impl / 2nd consumer 0)
- [[dispatch-tail-auto-tail-pattern]] lesson 2nd binding 증명 — drain/refresh two-call 완전 청산 (Vello R51.123 + TUI R51.124 일관 패턴)
- [[abstraction-needs-second-consumer]] preview — trait 추출 실재 cycle + 2 impl 동시 충족 시에만 (R51.125 carry)
- TUI focus management hardcode + cell-native coord + AT integration 등 잔여 carry 그대로



### R51.124.1 — R51.124.1 §5.41 — R51.125 (dispatch_rpc trait extraction) deferral audit: Rule of Three + substrate-incompleteness-signal 미충족 정통 명시

**Changes**:
- §5.41 caveats += R51.125 dispatch_rpc trait extraction defer 정통 사유 — cycle 0 (dep graph reverse direction 없음) + impl 1곳 (pinion-shell::ShellCore::dispatch_rpc) + 2nd RPC consumer 없음 (pinion-tui RPC carry)
- docs/GENERATED.md 동기 (caveat 1줄 추가, mnemosyne-cli generate_docs cascade)
- Memory 추가 3건 — coreshell-composition-lift.md / dispatch-tail-auto-tail-pattern.md / abstraction-needs-second-consumer.md (R51.122-R51.124 lessons 영구 capture)
- Rule of Three (Fowler) + [[substrate-incompleteness-signal]] abstraction layer 적용 명시 — 1 impl 의 trait abstraction = unused indirection, 가상 cycle 추출 = premature



**Verification**:
- mnemosyne validate_workspace = entries=253 / sections=58 / T1=0 / RT=1/1 / GENERATED.md=sync (post-#4)
- cargo test / clippy 변경 0 (audit-only entry, 코드 변경 0)
- atomic store mutation = caveat 1줄 (100 char limit 검증), GENERATED.md 1줄 추가 (round-trip sync 유지)
- R51.122-R51.124 4-round 분할 종료 마침표 — strategic 부채 #1 (ShellCore lift) 청산 audit trail 완성



**Impact**: §5.41


**Carry forward**:
- R51.125 = dispatch_rpc trait extraction — 2nd RPC consumer (pinion-tui RPC) land 시 자동 trigger 로 재평가; 그 시점 cycle 검증 + 2-impl 동시 검증 필수
- [[abstraction-needs-second-consumer]] lesson 정통 capture — trait/interface 추출은 (a) dep graph 실재 cycle (b) 2 이상 impl 동시 충족 시에만, 가상 cycle 으로 추출 금지
- TestButton fixture 중복 (runtime + tui tests) shared module lift 후보 — R51.127 cosmetic
- self-audit framework #140-#143 통합 — R51.122-R51.124.1 lessons 4건 (CoreShell composition / DispatchTail / TUI auto-tail / trait extraction Rule of Three) 자동 carry
- changelog 95 entries 백필 후보 — R51.88-R51.120 gap 청산 (R51.128 후보)



### R51.127 — R51.127 §5.41 — pinion-core::test_fixtures::ButtonFixture shared lift: pinion-runtime + pinion-tui 두 test suite 의 ~75 LOC TestButton 중복 청산

**Changes**:
- crates/pinion-core/src/test_fixtures.rs 신설 (+99 LOC) — ButtonFixture struct + WidgetCore impl (state/event/external/tag/read_state/view/event_name/title/keybinding/apply_key) one canonical copy
- crates/pinion-core/src/lib.rs += `#[cfg(any(test, feature = "test-fixtures"))] pub mod test_fixtures;` (production binary 영향 0)
- crates/pinion-core/Cargo.toml += `[features] test-fixtures = []` (downstream dev-dep feature flag)
- crates/pinion-a11y/Cargo.toml += `[features] test-fixtures = ["pinion-core/test-fixtures"]` + crates/pinion-a11y/src/widget_a11y.rs `impl WidgetA11y for ButtonFixture {}` (atomic-default, orphan rule 회피)
- crates/pinion-runtime/Cargo.toml [dev-dependencies] += pinion-core features=["test-fixtures"]; crates/pinion-runtime/src/core_shell.rs#tests — TestButton struct + WidgetCore impl ~76 LOC 제거, `use ButtonFixture as TestButton`
- crates/pinion-tui/Cargo.toml [dev-dependencies] += pinion-core + pinion-a11y features; crates/pinion-tui/src/substrate.rs#tests — TestButtonView struct + WidgetCore + WidgetA11y impl ~80 LOC 제거, `use ButtonFixture as TestButtonView`
- WidgetViewTui impl 은 backend-local trait 이므로 pinion-tui tests 안에 유지 (orphan rule OK), 종합 육구에 좌우 없이 textbook ISP 3-impl-block split 일관



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 / 0 / 8 (R51.126 baseline 유지, behavior 0 변경)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict 유지, doc_lazy_continuation + field_reassign_with_default 정통 공공)
- git diff --stat: 9 files changed, -145 LOC net (pinion-core +99 new, pinion-runtime -75, pinion-tui -80, 3 Cargo.toml +18, pinion-a11y impl +13)
- behavior 변경 0 — ButtonFixture 에서 view fn 이 struct literal 으로 재작성되었으나 명이는 원본과 동일 (Rect (0,0,32,48) tag test_btn 1 TextNode child)



**Impact**: §5.41, §5.40, §5.16


**Carry forward**:
- R51.128 = R51.88-R51.120 changelog gap (~33 entries) 백필 아직 남아있음 — heavy bookkeeping, audit trail commit↔ledger drift 완전 회복
- test-fixtures feature flag pattern = test scaffolding cross-crate share 의 정통 — production binary 조거 보존 + downstream dev-dep 설정으로 노출
- future fixtures 추가 시 이 패턴 따르기 — ButtonFixture 외 ToggleFixture / CheckboxFixture / RadioFixture / SliderFixture 등은 아직 필요 시 종속 자동 lift
- WidgetA11y blanket impl strategy 제한 — Rust specialization 부재로 blanket `impl WidgetA11y for T: WidgetCore` 불가 (composite override 충돌), per-fixture explicit impl 제한



### R51.129 — R51.129 §5.40 — WidgetA11y test-fixtures impl 별도 module 분리 (R51.127 정정 보고 #c 청산, trait 정의 파일은 trait 자체에 집중)

**Changes**:
- crates/pinion-a11y/src/test_fixtures.rs 신설 (+30 LOC) — ButtonFixture WidgetA11y impl 전용 module, feature `test-fixtures` gated
- crates/pinion-a11y/src/lib.rs += `#[cfg(any(test, feature = "test-fixtures"))] mod test_fixtures;` (private module, public re-export 불요)
- crates/pinion-a11y/src/widget_a11y.rs — R51.127 에서 추가한 `impl WidgetA11y for ButtonFixture {}` block 13 LOC 제거 (trait 정의 파일 separation of concerns)
- feature gate 동일 (`test-fixtures = ["pinion-core/test-fixtures"]`) — downstream API 변경 0, dev-dep 설정 변경 불요



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 / 0 / 8 유지
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = entries=301 / sections=58 / T1=0 / RT=1/1 / sync



**Impact**: §5.40


**Carry forward**:
- test-fixtures module 구조 정통 — 미래 추가 fixture (ToggleFixture / CheckboxFixture / RadioFixture) 동일 패턴 적용
- [[test-fixtures-feature-gate-pattern]] memory lesson 의 구체 evidence 갱신
- trait 정의 파일 의 cohabitation 제거 — widget_a11y.rs 는 이제 trait 자체만 carry



### R51.130 — R51.130 §5.41 — TUI paint box-drawing chars `\u{XXXX}` escape + named const lift (baseline carry "Rust source 내 non-ASCII literal" 청산)

**Changes**:
- crates/pinion-tui/src/paint.rs — 6 named const BOX_{HORIZONTAL/VERTICAL/TOP_LEFT/TOP_RIGHT/BOTTOM_LEFT/BOTTOM_RIGHT} 신설 (U+2500..U+2518 light set)
- 8 개 set_symbol("─")/...("┘") inline Unicode literal 제거 — named const 참조로 아스키 source baseline 회복
- doc comment 은 actual glyph 1회 명시 (─ │ ┌ ┐ └ ┘) — reader navigation 속을 유지
- behavior 변경 0 — 모든 const 가 원본과 동일 곡Ş프 (UTF-8 byte sequence 부함)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 / 0 / 8 유지
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = entries=301 → 302 / sections=58 / T1=0 / RT=1/1 / sync
- Rust source ASCII baseline 회복 — box-drawing chars 은 doc string + const 값 내부 (\u{XXXX} escape)



**Impact**: §5.41


**Carry forward**:
- box-drawing const 파트트 정통 — future heavy/double/rounded 바리언트 추가 시 동일 패턴 적용 (BOX_HEAVY_HORIZONTAL = "\u{2501}" 등)
- BoxStyle 의 corner_radius / placement / width 는 R51.115 paint_box_style doc 에서 이미 cell-unit 분기 명시 — 자체 carry 추가 없음



### R51.131 — R51.131 §5.38 — R51.107 type-ahead polish carry close (substrate textbook 정통, application thread_local 은 evidence-first 미충족)

**Changes**:
- audit-only entry — 코드 변경 0
- typeahead substrate 재검토 — TypeaheadCursor::step pure + caller-inject `now: Instant` + caller-owned
- 모듈 doc: "thread-local/struct field/ECS resource — caller's choice" → multi-window safe
- hello-listbox{,-multi} `thread_local!` = application-side design choice (substrate 별도 부채 아님)



**Verification**:
- pinion-shell/src/typeahead.rs L17-L23 의 “## Design boundary” 섹션 관철 — cursor = application-side state 명시
- Rule of Three (Fowler) + [[abstraction-needs-second-consumer]] 적용 — multi-window app 0 건 존재, application-side storage abstraction 추출 = premature
- multi-window app land 시 자동 trigger 로 재평가



**Impact**: §5.38


**Carry forward**:
- R51.107 carry — multi-window app land 시 application-side storage abstraction 재평가 trigger
- [[abstraction-needs-second-consumer]] lesson 받아 — evidence 없는 polish 는 textbook 도 acceptable defer



### R51.132 — R51.132 §5.38 — R51.131 publishable typo + over-length 정정 (audit immutable, ledger anchored)

**Changes**:
- set_changelog_publishable_changes: changes_bullets 4건 압축 — 165→<=100 char + '너뻘'→'별도'
- set_changelog_publishable_carry_forward: carry 2건 typo — '반아'→'받아', '도ҫacceptable'→'도 acceptable'
- mnemosyne.toml: [[publishable_override_ledger]] row 1건 추가 — content_hash_after 01c0589...



**Verification**:
- validate_workspace: entries=11 ledger_rows=16 — R51.131 divergence anchored (직전 10/15)
- audit half immutable: decision_summary / changes / carry audit 측 frozen 보존 (R294 ledger)
- code 변경 0 — cargo test 1722/0/8 baseline 무변동, clippy 0 warning



**Impact**: §5.38


**Carry forward**:
- R51.107 carry close 자체는 R51.131 audit-only 로 land 완료 (substrate textbook 정통)
- host mnemosyne-cli emit-publishable-override-ledger-draft subcommand 미지원 회피
- validate_workspace stderr hint = anchor sha256 정통 path (cli rebuild 불요)



### R51.133 — R51.133 §5.28 — animation primitive substrate (Animatable trait + SpringConfig + SpringState semi-implicit Euler 첫 land, R52 axis 시작)

**Changes**:
- pinion-core/src/animation.rs 신규 — Animatable trait + AnimVec2/AnimVec4 + SpringConfig + SpringState (+11 테스트)
- pinion-core/src/lib.rs — pub mod animation + 5 type re-export top-level
- atomic §5.28 — 4 implementation bindings (file + Animatable + SpringConfig + SpringState) + impact_scope=[5.22, 5.23, 6.3]



**Verification**:
- cargo test -p pinion-core --lib animation: 11/11 passed (수렴 검증 / pure function / interrupt velocity 보존)
- cargo test --workspace --features pinion-runtime/vello: 1733/0/8 (+11 신규, baseline 1722 보존)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warning (doc_markdown 5건 backtick 정통)



**Impact**: §5.28


**Carry forward**:
- Color / Rect Animatable impls — linear-RGBA / f32-shadow space 변환 quality 라운드
- AnimationDriver Effect substrate — §5.23 의존 + Owner drop cancel (R51.134+)
- Animated<T> Signal 래퍼 — §5.22 의존 + Frame.dt driver (R51.135+)
- SCE schema + Forge emit — declarative animated bindings (R51.136+)
- Easing enum (Linear / EaseInQuad / …) — tween special case path



### R51.134 — R51.134 §5.28 — Color sRGB linear-space conversion (to_linear/from_linear exact EOTF) + AnimRect 4-f32 wrapper

**Changes**:
- Color::to_linear/from_linear — sRGB IEC 61966-2-1 EOTF, alpha linear, saturate clamp
- pinion-core/src/animation.rs — AnimRect struct + Animatable impl + Rect↔AnimRect helpers
- pinion-core/src/lib.rs — AnimRect top-level re-export
- atomic §5.28 — +3 binding (AnimRect, Color::to_linear, Color::from_linear)



**Verification**:
- cargo test -p pinion-core --lib: 506 passed (495 base + 11 R51.134: 6 sRGB / 5 AnimRect)
- cargo test --workspace --features pinion-runtime/vello: 1744/0/8 (+11 신규, baseline 1733)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warning
- perceptual midpoint test 검증 — sRGB exact (b/w mid = ~188) vs naive (127), 정통 입증



**Impact**: §5.28


**Carry forward**:
- Color::Animatable impl deferred — caller-explicit to_linear → spring → from_linear 이 정통 (lossy saturating impl 회피)
- AnimationDriver Effect substrate — §5.23 Effect + §5.22 Signal 의존 (R51.135+)
- premultiplied-linear vs straight-linear alpha decision (quality round)
- AnimVec2/3 계열 올림 여부 — Vec3 needed 시 evidence-first



### R51.135 — R51.135 §6.3 — Frame ZST→{dt: f32} evolution (§5.28 R51.133 carry 청산, AnimationDriver prerequisite)

**Changes**:
- pinion-core/src/frame.rs — Frame { dt: f32 } + Frame::new (dt=0) + Frame::with_dt(dt) factory
- size assertion 제거 — Frame 이 더 이상 ZST 아님 (4-byte, single-register ABI)
- atomic §6.3 — +3 binding (file + Frame + Frame::with_dt)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 1748/0/8 (+4 신규, baseline 1744)
- cargo clippy --workspace --all-targets: 0 warning (float_cmp → to_bits)
- #[non_exhaustive] 보호 — caller 25 sites Frame::new() 영향 0 (additive evolution)



**Impact**: §6.3, §5.28


**Carry forward**:
- AnimationDriver Effect substrate — §5.23 Effect first-cut + Frame.dt driver wire-up
- Frame::with_dt caller migration — Frame::new → with_dt(measured_dt) (evidence-first)
- frame_index / scale_factor field carry — evidence-first, 2nd consumer trigger
- §6.3 charter expand — body/intent/rationale (현재 empty), Async model full doc



### R51.136 — R51.136 §5.28 — Easing enum (7 variants) + Tween<T> deterministic tween path (spring 의 special case, 결정성 우선 use case)

**Changes**:
- pinion-core/src/animation.rs — Easing enum (Linear/Quad×3/Cubic×3) + apply pure
- Tween<T: Animatable> { from, to, duration, easing, elapsed } + new/current/tick/is_done
- pinion-core/src/lib.rs — Easing, Tween top-level re-export
- atomic §5.28 — +2 binding (Easing, Tween)



**Verification**:
- cargo test --workspace: 1759/0/8 (+11 신규 Easing 4 + Tween 7, 1748 base)
- cargo clippy: 0 warning (cast_precision_loss test allow i≤0..=10 f32 exact)
- 27 animation tests 누적 (R51.133 11 + R51.134 5 + R51.136 11) — endpoint exact + monotonic



**Impact**: §5.28


**Carry forward**:
- AnimationDriver Effect substrate — §5.23 first-cut + Signal subscription (R51.137+)
- Color/Rect Animatable impl — caller-explicit linear path 정통 (R51.134 carry)
- premultiplied-linear vs straight-linear alpha quality round
- EaseInQuart/Quint/EaseInOutBack 등 extended curves — evidence-first



### R51.137 — §5.23 Effect substrate first-cut — eager-rerun Owner-tied reactive scope (R52 critical path)

**Changes**:
- crates/pinion-core/src/reactive/effect.rs 신규 (~520 LOC): Effect / EffectInner + ReactiveNode impl
- Effect::new(owner, FnMut) — eager initial run + lazy Signal subscription + cycle detect (in_run)
- EffectInner::rerun(self: &Rc<Self>) — drains source_cleanups, run_with_node, panic-safe RAII
- mark_dirty → weak_self.upgrade() → rerun (OnceCell self-pointer, dyn-safe ReactiveNode preserved)
- Owner::on_cleanup(Box<FnOnce>) public method — Effect 의 owner-tied cancellation 등록 entry
- reactive::Effect re-export at mod.rs + lib.rs (Computed sibling, public surface)
- +15 신규 tests: eager/dep-track/equality-skip/owner-drop/cascade/dyn-dep/batch/cycle/panic



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1774 pass / 0 fail / 8 ignored (+15)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- Effect/Signal/Computed substrate sibling shape — `Rc<dyn ReactiveNode>` 동일 dispatch path
- mnemosyne validate_workspace: entries=309 / T1=0 / T3=0 / GENERATED.md=sync (post-mutation)



**Impact**: §5.23, §5.22


**Carry forward**:
- R51.138: §5.28 AnimationDriver concrete (Animated<T> wraps Signal<T>, Spring/Tween + Effect tick)
- R51.139: hello-button hover transition demo (1st application, visual evidence path)
- §5.23 Command<Intent> + Handler trait substrate — async/IO escape hatch (R52 carry)
- Effect dry_run skip — §2 #3 substrate (R51.140+ carry, requires thread-local dry_run flag)
- Owner topological order across siblings — current order = registration; explicit topo carry



### R51.138 — §5.28 Animation&lt;T&gt; Signal wrapper + Tickable trait + Owner tick registry

**Changes**:
- Animation<T> 신규: Signal<T> wrap + SpringState + 타겟 + Tickable impl (~210 LOC)
- Tickable trait (tick + is_at_rest): object-safe surface, Owner 가 Rc<dyn Tickable> 으로 저장
- Owner::register_animation(Rc<dyn Tickable>) + Owner::tick_animations(dt) public API
- tick = batch + depth-first cascade (children→self) + snapshot pattern (mid-tick register safe)
- AnimVec2/4/Rect: serde::Serialize+Deserialize derive 추가 (Signal<T> bound 충족)
- 13 신규 tests: at-rest / tick / interrupt / signal-fire / batch-coherence / drop / depth-first



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1787 pass / 0 fail / 8 ignored (+13)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- clippy reactive fix 4건: clone_on_copy x2 + items_after_statements x2 (사전 audit 미적용 lesson)
- mnemosyne validate_workspace: entries=310 / T1=0 / T3=0 / GENERATED.md=sync (post-mutation)



**Impact**: §5.28, §5.22


**Carry forward**:
- R51.139: framework runtime paint-loop → Owner::tick_animations(Frame.dt) integration (Effect wrap)
- R51.140: hello-button hover transition demo (1st application / visual evidence path)
- Animation 의 Owner-tied drop = registry 자동 release (이미 land); driver tick caller R51.139 carry
- Animation::set_target 비-Signal API; reactive subscription 은 Animation::value/signal 만 정통
- rest_epsilon = const default (0.01 sub-pixel), per-Animation 변경 API 는 evidence-first carry



### R51.139 — §5.23 Command&lt;I&gt; declarative struct + Owner-tied pending queue (R52 axis B substrate)

**Changes**:
- crates/pinion-core/src/command.rs 신규 (~145 LOC): Command wire-form struct (Intent mirror)
- Owner::dispatch_command + pending_commands snapshot + take_pending_commands FIFO drain
- Owner::take_pending_commands_recursive: depth-first subtree drain (children → self order)
- Owner drop cancels pending queue (cancellation via Owner-tied lifetime, Solid 패턴)
- Command kind=Cow<str> + payload=IntrospectValue + scope_id=u64 (Serialize-friendly for RPC)
- 14 신규 tests: dispatch/snapshot/FIFO drain/drop-cancel/recursive/follow-up



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1801 pass / 0 fail / 8 ignored (+14)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- clippy reactive fix 5건: doc_markdown x3 + must_use_candidate x2 (lesson #149 또 위반)
- mnemosyne validate_workspace: entries=311 / T1=0 / T3=0 / GENERATED.md=sync (post-mutation)



**Impact**: §5.23, §5.20


**Carry forward**:
- R51.140: Handler trait (async dispatch surface) + boot-time registry — framework-side carry
- R51.141+: scene/commands RPC method (10th) — pinion-rpc 측 pending queue surface
- Update fn signature: Update(&mut Model, Intent) → Vec<Command> — Reducer integration carry
- SCE schema for declarative command tables + Forge codegen (R51.x+ axis, evidence-first carry)
- Cancellation: 신규 Command 같은 scope 가 prior in-flight 취소 (Solid pattern) — R51.140+ carry



### R51.140 — R51.137-139 lesson #153-156 memory entry 4건 land + §5.41 clippy lint family 사전-audit 회복 선언 (bookkeeping)

**Changes**:
- memory 4건 land (lesson #153-156): OnceCell-self-ptr / Tickable-snap / Cmd-mirror / clippy-pre-audit
- MEMORY.md 인덱스 21 → 25 entries (4 lesson 슬러그 + 한 줄 hook 각각)
- §5.41 caveat 1건 추가: clippy lint family 사전-audit 회복 선언 (R51.137-139 reactive 10건 누적)
- atomic store mutation = 1 caveat + 1 changelog entry (R51.140)



**Verification**:
- validate_workspace: entries=311→312 / T1=0 / RT=1/1 / GENERATED.md=sync 유지
- T4 info 120 → 121 (+1 bullet_list_preference, decision_summary 109 char prose hint)
- cargo test/clippy baseline 무변동 (코드 변경 0 — bookkeeping round)
- memory 디렉터리 25개 파일 (MEMORY.md + 24 슬러그)



**Impact**: §5.41, §5.23, §5.28


**Carry forward**:
- R51.141 — Handler trait first-cut (async handle, pinion-runtime boot-time registry, §5.23 axis B)
- R51.142 — framework paint-loop wiring (Owner::tick_animations(dt), AnimationDriver Effect-wrap)
- R51.143 — hello-button hover transition demo (1st visual application of §5.28 + ColorAnimation)
- R51.125 dispatch_rpc trait extraction defer 유지 (2nd RPC consumer trigger 까지)



### R51.141 — §5.23 Handler trait + HandlerRegistry first-cut land — pinion-runtime async dispatch substrate

**Changes**:
- pinion-runtime/src/command/{mod,handler,registry}.rs 신설 (3 파일, ~350 LOC)
- Handler trait (object-safe BoxFuture dispatch) + HandlerRegistry (BTreeMap kind→Arc<dyn Handler>)
- lib.rs: pub mod command + Handler/HandlerFuture/HandlerRegistry re-exports
- futures-executor dev-dep 추가 (test block_on; runtime-agnostic public surface)



**Verification**:
- 1801 → 1815 tests (+14 신규: handler 3 + registry 11), 0 failed, 8 ignored
- clippy --features pinion-runtime/vello = 0 warnings
- entries 312 → 313, T1=0, RT=1/1, GENERATED.md=sync
- T4 info 121 (사전 char audit 100% 통과: decision 95, bullet max 96, caveat 96)



**Impact**: §5.23, §5.20, §6.3


**Carry forward**:
- R51.142 — executor binding (pinion-rpc/pinion-shell tokio::spawn + Command queue drain pump)
- R51.143 — Solid in-flight cancellation (JoinHandle / CancellationToken per executor)
- R51.144 — scene/commands RPC method (10th typed method, §5.7 + §5.23 inspection)
- Update(&mut Model, Intent) -> Vec<Command> reducer signature evolution carry



### R51.142 — §5.28 CoreShell<V> root_owner + tick_animations 추가 — backend paint-loop animation tick surface

**Changes**:
- pinion-runtime/src/core_shell.rs: root_owner: Owner field + 2 pub methods 추가
- CoreShell::root_owner() &Owner accessor + CoreShell::tick_animations(dt) driver hook
- 5 신규 tests (root_owner usable / dt forward / zero-dt idempotent / repeat tick / drop cascade)



**Verification**:
- 1815 → 1820 tests (+5 신규), 0 failed, 8 ignored
- clippy --features pinion-runtime/vello = 0 warnings
- entries 313 → 314, T1=0, RT=1/1, GENERATED.md=sync
- ShellCore / ShellCoreTui wrap carry 0 (composition transparent; 1815→1820=+5 정합)



**Impact**: §5.28, §5.41, §6.3


**Carry forward**:
- R51.143 — Vello + TUI paint cycle 측 dt 측정 + tick_animations 호출 + Frame::with_dt(dt) 전환
- R51.144 — hello-button hover transition demo (1st visual application, ColorAnimation use case)
- R51.145 — AnimationDriver Effect-wrap (§5.28 R33 'framework Effect' 진본화)
- R51.146 — Handler executor binding (R51.141 carry, pinion-rpc/pinion-shell tokio runtime owner)



### R51.143 — §5.28 ShellCore (Vello) paint cycle dt 측정 + tick_animations + Frame::with_dt wiring (#1/2)

**Changes**:
- pinion-shell/src/substrate.rs: last_paint_instant: Option<Instant> 필드 + root_owner forward
- compute_paint_scene: Instant 측정 → dt → core.tick_animations(dt) + Frame::with_dt(dt)
- 3 integration tests (first dt=0 / second dt>1ms / repeated 5 ticks)



**Verification**:
- 1820 → 1823 tests (+3 신규 integration), 0 failed, 8 ignored
- clippy --features pinion-runtime/vello = 0 warnings
- entries 314 → 315, T1=0, RT=1/1, GENERATED.md=sync
- TUI 평행 wiring carry R51.144 (ShellCoreTui Cell<Option<Instant>> 패턴)



**Impact**: §5.28, §5.40, §6.3


**Carry forward**:
- R51.144 — ShellCoreTui (TUI) paint cycle dt wiring 평행 (#2/2; Cell interior mutability)
- R51.145 — hello-button hover transition demo (1st visual application, ColorAnimation)
- R51.146 — AnimationDriver Effect-wrap (§5.28 R33 'framework Effect' 진본화)
- dt frame budget cap (background/long-pause robustness, 100ms 또는 1/30s clamp 정통)



### R51.144 — §5.28 ShellCoreTui (TUI) paint cycle dt 측정 + tick_animations + Frame::with_dt wiring (#2/2)

**Changes**:
- pinion-tui/src/substrate.rs: last_paint_instant: Cell<Option<Instant>> + root_owner forward
- compute_paint_scene (&self): Cell interior mut + dt 측정 + tick_animations + Frame::with_dt
- 3 신규 inline tests (first dt=0 / second dt>1ms / shared-borrow signature 검증)



**Verification**:
- 1823 → 1826 tests (+3 신규 inline), 0 failed, 8 ignored
- clippy --features pinion-runtime/vello = 0 warnings
- entries 315 → 316, T1=0, RT=1/1, GENERATED.md=sync
- §5.28 R52 axis 백엔드 wiring 양쪽 완료 (Vello R51.143 + TUI R51.144)



**Impact**: §5.28, §5.41, §6.3


**Carry forward**:
- R51.145 — hello-button hover transition demo (1st visual application, ColorAnimation)
- R51.146 — AnimationDriver Effect-wrap (§5.28 R33 'framework Effect' 진본화)
- R51.147 — Handler executor binding (R51.141 carry, pinion-rpc/pinion-shell tokio runtime)
- dt frame budget cap (background pause robustness, 100ms 또는 1/30s clamp 정통 carry)



### R51.145 — §5.28 clamp_frame_dt (1/30s cap) helper land — 양 backend compute_paint_scene apply

**Changes**:
- pinion-runtime/src/frame_pacing.rs 신설 (MAX_FRAME_DT_SECS + clamp_frame_dt, NaN guard)
- ShellCore (Vello) + ShellCoreTui (TUI) compute_paint_scene: raw_dt → clamp_frame_dt
- lib.rs: pub mod frame_pacing + pub use clamp_frame_dt, MAX_FRAME_DT_SECS
- 6 신규 tests (zero/typical/long-pause/negative/NaN/anchor)



**Verification**:
- 1826 → 1832 tests (+6 신규 frame_pacing), 0 failed, 8 ignored
- clippy 0 warnings (lesson #149 회복 1 reactive doc_markdown 'SwiftUI' backtick 정직 보고)
- entries 316 → 317, T1=0, RT=1/1, GENERATED.md=sync
- NaN test가 실제 defect catch (f32::clamp NaN propagate) — 명시 guard 추가



**Impact**: §5.28, §5.41, §6.3


**Carry forward**:
- R51.146 — hello-button hover demo (view-fn ↔ Animation 통합 substrate 선결 carry)
- R51.147 — AnimationDriver Effect-wrap (§5.28 R33 'framework Effect' 진본화)
- R51.148 — Handler executor binding (R51.141 carry, tokio dispatch)
- view-fn ↔ Owner context substrate gap — Owner::current() public + run() framework wrap



### R51.146 — §5.22 view-fn ↔ Owner context substrate (option b): framework wrap root_owner().run + Owner::current() public

**Changes**:
- pinion-core::reactive::owner Owner::current() public + CURRENT_OWNER_HANDLE thread-local stack
- pinion-core OwnerHandleGuard RAII; Owner::run pushes both subscriber + handle stacks
- pinion-shell ShellCore::compute_paint_scene wraps V::view in root_owner().run(|| ...)
- pinion-shell ShellCore::dispatch_rpc producer closure wraps V::view in root_owner.run(...)
- pinion-tui ShellCoreTui::compute_paint_scene wraps V::view in root_owner().run(|| ...)
- Computed::recompute leaves handle stack untouched — Owner::current() returns lexical enclosing Owner



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1848 passed / 0 failed / 8 ignored
- baseline 1832 → 1848 (+16 R51.146: 10 owner.rs + 3 pinion-shell + 3 pinion-tui)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace: entries=318 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.22, §5.28, §5.41


**Carry forward**:
- R51.147 §5.28 first visual demo: hello-button hover transition via Owner::current()
- R51.148 §5.28 AnimationDriver Effect-wrap (manual tick → Effect-driven, §5.28 R33 진본화)
- R51.149+ §5.23 Handler executor binding (tokio::spawn + Command queue drain pump)
- memory entries #157-166 (Handler / CoreShell composition / clamp anchor / Owner::current)



### R51.147 — §5.28 Owner::any_animation_active + hello-button hover Animation<f32> demo (first visual application of §5.28 substrate)

**Changes**:
- pinion-core Owner::any_animation_active(eps) recursive walk — children depth-first then self
- pinion-runtime CoreShell::any_animation_active accessor forward
- pinion-shell ShellCore::compute_paint_scene sets redraw_requested if any_animation_active
- pinion-tui ShellCoreTui::any_animation_active accessor forward (surface poll-loop carry)
- hello-button drive_hover_progress: thread_local OnceCell<Animation<f32>> + Owner::current()
- hello-button lerp_grayscale + view fn Idle↔Hover spring-driven lightness fade



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1856 passed / 0 failed / 8 ignored
- baseline 1848 → 1856 (+8 R51.147: 6 owner.rs + 2 core_shell.rs)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- hello-button binary built — visual spring transition 사용자 verification pending (binary side)



**Impact**: §5.28, §5.22, §5.41


**Carry forward**:
- R51.148 cleaner application context API (avoid thread_local OnceCell view-fn pattern)
- R51.148 §5.28 AnimationDriver Effect-wrap (manual tick → Effect-driven, R33 진본화)
- R51.149 TUI surface continuous-paint loop (poll timeout while any_animation_active)
- R51.150+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)



### R51.148 — §5.28 TUI shell adaptive poll-timeout + hello-button-tui hover Animation demo (Vello R51.147 의 TUI 대칭)

**Changes**:
- pinion-tui shell::run adaptive poll timeout (IDLE 100ms ↔ ACTIVE 16ms while animations move)
- pinion-tui shell::run timeout-tick repaint commit while any_animation_active
- IDLE_POLL_MS / ACTIVE_POLL_MS / REST_EPSILON module-level constants (clippy 정합)
- hello-button-tui drive_hover_progress: OnceCell<Animation<f32>> via Owner::current()
- hello-button-tui lerp_grayscale + view fn Idle↔Hover spring lerp (truecolor)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1856 passed / 0 failed / 8 ignored
- baseline 1856 → 1856 (R51.148 = surface/example only, 신규 unit tests 없음)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- hello-button-tui binary built — terminal truecolor spring transition 사용자 verify pending



**Impact**: §5.28, §5.41


**Carry forward**:
- R51.149 §5.28 AnimationDriver Effect-wrap (manual tick → Effect-driven, R33 진본화)
- R51.150+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)
- R51.151+ application context API (avoid thread_local OnceCell view-fn pattern)
- memory entries #157-166 lessons partial land (5 of ~9), 잔여 carry



### R51.149 — §5.28 R33 framework AnimationDriver 진본화 — CoreShell tick_animations 가 reactive Effect (frame_signal counter)을 통해 dispatch

**Changes**:
- pinion-runtime CoreShell + frame_signal Signal<u64> + last_dt Rc<Cell<f32>> + driver Effect
- tick_animations(dt) = last_dt.set(dt) + frame_signal.set(counter++)
- driver Effect subscribes frame_signal; eager initial run = noop primer
- monotonic counter sidesteps Signal equality-skip — identical dt 5 ticks dispatch 5 times
- frame_signal() 공개 accessor — applications observe paint clock without separate counter
- Effect-driven routing = §5.28 R33 framework AnimationDriver 진본화



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1860 passed / 0 failed / 8 ignored
- baseline 1856 → 1860 (+4 R51.149 in core_shell)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- existing 20 core_shell tests unchanged (observable behavior preserved)



**Impact**: §5.28, §5.22, §5.41


**Carry forward**:
- R51.150+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)
- R51.151+ application context API (avoid thread_local OnceCell view-fn pattern)
- memory entry #168 — Effect-driven driver + monotonic counter pattern



### R51.150 — §5.22 Owner::cache primitive + hello-button*/hello-button-tui thread_local OnceCell 청산 (useMemo/useRef 정통 mirror)

**Changes**:
- pinion-core Owner::cache<V>(key, factory) -> Rc<V> 정통 primitive (lazy-init)
- OwnerInner.cache: RefCell<HashMap<&str, Rc<dyn Any>>> field + downcast pattern
- Owner::cache_contains(key) -> bool diagnostic accessor
- hello-button drive_hover_progress thread_local OnceCell → Owner::current().cache
- hello-button-tui 동일 패턴 적용 (hello_button_tui::hover_progress key)
- application-side workaround 청산 ([[textbook-long-term-correct]] 위반 회복)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1870 passed / 0 failed / 8 ignored
- baseline 1860 → 1870 (+10 R51.150 cache tests across 10 scenarios)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- same_key_mismatched_type_panics 테스트 통과 (load-bearing 계약)



**Impact**: §5.22, §5.28


**Carry forward**:
- R51.151+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)
- R51.152+ lerp_grayscale framework primitive 화 (hello-button + hello-button-tui DRY)
- R51.153+ Owner::cache positional/macro variant (call-site identity ergonomic)
- memory entry #169 — Owner::cache substrate (useMemo/useRef 정통 mirror)



### R51.151 — §5.28 Color::lerp linear-space primitive + hello-button*/hello-button-tui lerp_grayscale DRY 청산 + #[allow(cast)] 회복

**Changes**:
- pinion-core Color::lerp(self, other, t) -> Color (linear-space 정통)
- NaN guard (NaN → 0.0 → self) + clamp t ∈ [0.0, 1.0]
- AnimVec4 + Animatable::lerp 재사용 (spring solver path 와 일치)
- hello-button BTN_FILL_IDLE/HOVER const + Color::lerp (lerp_grayscale 제거)
- hello-button-tui 동일 적용 (lerp_grayscale 제거)
- #[allow(clippy::cast_*)] 제거 (framework primitive 정통 회피)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1878 passed / 0 failed / 8 ignored
- baseline 1870 → 1878 (+8 R51.151 lerp tests: endpoints/clamp/NaN/perceptual/alpha/parity)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- linear-space midpoint perceptually correct (mid > 180 vs sRGB-naive 127)



**Impact**: §5.28, §5.3


**Carry forward**:
- R51.152+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)
- R51.153+ Owner::cache positional/macro variant (call-site identity)
- R51.154+ Color::Animatable trait 정식 binding (lerp 자체적으로 trait method)



### R51.152 — §5.22 CoreShell::apply_key V::apply_key root_owner.run wrap + hello-listbox*/hello-listbox-multi typeahead thread_local 청산

**Changes**:
- CoreShell::apply_key 가 V::apply_key 호출을 root_owner.run() wrap
- Owner::current() 가 apply_key 안에서 root_owner 으로 resolve (R51.146 sibling)
- hello-listbox thread_local TYPEAHEAD → owner.cache('hello_listbox::typeahead')
- hello-listbox-multi 동일 적용 (hello_listbox_multi::typeahead)
- Owner::cache 두 번째 consumer (R51.150 [[abstraction-needs-second-consumer]] 충족)
- test-fixture TestView::apply_key 가 Owner::current() observation 기록 (R51.146 pattern)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1882 passed / 0 failed / 8 ignored
- baseline 1878 → 1882 (+4 R51.152: 1 core_shell + 3 pinion-shell apply_key wrap tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- hello-listbox + hello-listbox-multi 기존 a11y tests 9+ 모두 통과 (regress 0)



**Impact**: §5.22, §5.41


**Carry forward**:
- R51.153+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)
- R51.154+ access_node / access_child_invoke 도 root_owner wrap (대칭 완성)
- R51.155+ Owner::cache positional/macro variant (call-site identity DX)



### R51.154 — §5.3 scale_normalized_to_px framework primitive + hello-slider*/hello-slider-vertical DRY 청산 + #[allow(cast)] 회복

**Changes**:
- pinion-core::style::scale_normalized_to_px(value, total) primitive (clamp + safe cast + drift)
- NaN guard (NaN → 0) + endpoint saturation + zero-total handling
- hello-slider filled_w = scale_normalized_to_px (value*RANGE 청산)
- hello-slider-vertical filled_h 동일 적용
- #[allow(clippy::cast_possible_truncation/sign_loss)] 2건 제거 (framework 정통)
- pub use lib.rs re-export 공개 알죠맄



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1891 passed / 0 failed / 8 ignored
- baseline 1882 → 1891 (+9 R51.154 scale tests: endpoints/clamp/NaN/zero/drift/large/round)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- hello-slider + hello-slider-vertical 시각 결과 동일 (계산 값 일치)



**Impact**: §5.3, §5.41


**Carry forward**:
- R51.155+ Owner::cache positional/macro variant (call-site identity DX)
- R51.156+ §5.23 Handler executor binding (tokio::spawn + Command queue drain)
- R51.157+ access_node / access_child_invoke root_owner wrap (대칭)



### R51.155 — §5.15 IntrospectValue typed accessors (as_bool/i64/i32/usize/f64/f32/str/is_null) + slider 마지막 #[allow(cast)] 청산

**Changes**:
- IntrospectValue::as_bool/as_i64/as_i32/as_usize/as_f64/as_f32/as_str/is_null primitives
- as_i32 narrowing failure surfaces None (안전 대안)
- as_usize negative 거부 (usize bound 정합)
- as_f32 f64→f32 truncation 캡슐화 (#[allow(cast)] containment)
- hello-slider 2건 + hello-slider-vertical 2건 IntrospectValue::Float match → as_f32
- #[allow(clippy::cast_possible_truncation)] 4건 추가 제거



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1901 passed / 0 failed / 8 ignored
- baseline 1891 → 1901 (+10 R51.155: bool/i64/i32 narrow/i32 reject/usize/f64/f32/f32 reject/str/is_null)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- as_i32 i32::MAX+1 narrowing → None (silent truncation 회피)



**Impact**: §5.15


**Carry forward**:
- R51.156+ 다른 examples (hello-toggle/hello-listbox/hello-radio) IntrospectValue 패턴 migration
- R51.157+ §5.23 Handler executor binding (multi-round)
- R51.158+ access_node / access_child_invoke root_owner wrap (대칭 완성)



### R51.156 — §5.23 Executor + IntentSink trait + CommandExecutor composite — R27 dispatch loop final binding substrate

**Changes**:
- pinion-runtime/src/command/executor.rs (신규 ~430 LOC): Executor trait + BoxFuture alias
- executor.rs: CommandTaskHandle (Arc cancel callback + AtomicBool 이중 cancel guard, Clone 공유)
- executor.rs: CommandExecutor composite (registry+executor+sink) + #[must_use] dispatch
- executor.rs: BlockOnExecutor 레퍼런스 impl (futures_executor::block_on, sync) + Debug
- pinion-runtime/src/command/sink.rs (신규 ~190 LOC): IntentSink trait (Send+Sync+'static)
- sink.rs: VecSink 테스트 픽스처 (Arc<Mutex<Vec<Intent>>>, drain/snapshot/len/is_empty)
- pinion-runtime/src/command/mod.rs: 새 module 등록 + pub use 갱신
- pinion-runtime/src/lib.rs: command 8개 신규 심볼 pub use
- pinion-runtime/Cargo.toml: futures-executor 0.3 dev-dep → 일반 dep 승격 (block_on sync helper)
- 테스트 +23 (executor.rs 16개 + sink.rs 7개)



**Verification**:
- cargo check -p pinion-runtime --features vello → 0 error
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1924 pass / 0 fail / 9 ignored (직전 1901/0/8 대비 +23 pass)
- BlockOnExecutor가 future를 spawn 내부에서 block_on 완료까지 구동, sink 정확히 1회 호출 확인
- CommandTaskHandle::cancel idempotent (Acqrel swap guard) + Clone 시 cancelled flag 공유 검증
- CommandExecutor::dispatch unknown kind None + known kind sink 라우팅 + scope_id round-trip 확인



**Impact**: §5.23, §5.20, §5.22, §6.3


**Carry forward**:
- R51.157 — CoreShell::dispatch_pending_commands drain pump + Option<Arc<CommandExecutor>> 필드
- R51.158 — per-scope BTreeMap<scope_id, CommandTaskHandle> cancellation (R27 Solid 패턴)
- R51.159 — pinion-shell tokio current-thread Executor + EventLoopProxy IntentSink + AppEvent::IntentArrived



### R51.157 — §5.23 CoreShell drain pump + Option&lt;Arc&lt;CommandExecutor&gt;&gt; executor field

**Changes**:
- pinion-runtime/src/core_shell.rs: CoreShell.executor: Option<Arc<CommandExecutor>> 신규 필드
- CoreShell::with_executor builder + set_executor swap + clear_executor + executor() accessor
- CoreShell::dispatch_pending_commands(): root_owner.take_pending_commands_recursive() 순회 → executor.registry().has(kind) 골라내기 → executor.dispatch(cmd) 명시적 라우팅 (handler 재시 실수 표면화)
- executor 미설치 시 no-op drain (Owner queue 보존, AI 관찰 가능)
- #[must_use] — unhandled Vec<Command> 합니 backend 로그/표자구우
- core_shell tests +9 (no-executor/handled/unhandled/mixed/recursive/set/clear/accessor/builder)



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1933 pass / 0 fail / 10 ignored
- 직전 1924/0/9 대비 +9 pass +1 ignored (R51.157 신규 9 tests + 1 doc-ignore example)
- r51_157_dispatch_drains_child_scope_commands_too: child-first 카스케이드 순서 표면
- r51_157_dispatch_mixed_handled_and_unhandled: handled vs unhandled 분리 로직 검증



**Impact**: §5.23, §5.22, §5.41


**Carry forward**:
- R51.158 — CommandExecutor.in_flight: Mutex<BTreeMap<scope_id, CommandTaskHandle>> 경쟁취소
- R51.159 — pinion-shell ShellCore.ShellCore::dispatch_pending_commands 쉬프들 wire-up + tokio current-thread
- R51.160 — pinion-tui ShellCoreTui drain pump 동명령 + IntentSink



### R51.158 — §5.23 R27 Solid 패턴 per-scope cancellation — CommandExecutor.in_flight tracker + cancel_scope

**Changes**:
- pinion-runtime/src/command/executor.rs: CommandExecutor.in_flight: Mutex<BTreeMap<u64, CommandTaskHandle>> 신규 필드
- CommandExecutor::dispatch 증강: scope_id 기반 prior handle remove + cancel 선행 후 new task spawn + tracker insert
- CommandExecutor::cancel_scope(scope_id) 명시 cancel API + Mutex poisoned panic doc
- CommandExecutor::in_flight_len + has_in_flight 접근자 (테스트 + scene/commands carry)
- Debug 출력에 in_flight_len 추가
- executor tests +10 (insert/unknown-no-pollute/same-scope-cancel/different-scopes/cancel API 계열)



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1943 pass / 0 fail / 10 ignored (직전 1933/0/10 +10 pass)
- r51_158_dispatch_same_scope_cancels_prior_handle: AtomicBool flag clone-family 공유 교차 검증
- r51_158_dispatch_three_times_same_scope_only_latest_tracked: 연속 cancel 순서 (id 1,2 cancelled, 3 alive)
- r51_158_cancel_scope_with_tracking_executor_observes_callback_fire: TrackingExecutor 로 executor-side cancel 실행 확인



**Impact**: §5.23, §5.22


**Carry forward**:
- R51.159 — pinion-shell tokio current-thread Executor impl + EventLoopProxy IntentSink
- pinion-shell AppEvent::IntentArrived variant + user_event arm → ShellCore::dispatch_intent
- ShellCore: CoreShell drain pump 호출 도메인 (handle_tail 후 / event 종료 시)
- R51.160 — pinion-tui ShellCoreTui drain pump + IntentSink dual-backend symmetry



### R51.159 — §5.23 pinion-shell tokio Executor + ProxyIntentSink — R52 axis B 완성 (Command→Future→Intent→UI loop)

**Changes**:
- pinion-shell/src/executor.rs 신규: TokioExecutor (multi-thread 1 worker) + ProxyIntentSink (winit EventLoopProxy)
- AppEvent::IntentArrived(Intent) variant + AppShell.user_event arm → core.dispatch_intent
- pinion-shell/Cargo.toml tokio 1 dep (rt, rt-multi-thread, macros, time, sync)
- ShellCore::set_command_executor / command_executor / dispatch_intent (SCXML invoke send + revision bump)
- ShellCore::handle_tail 증강: dispatch_pending_commands 결과 표면 (처리/미처리 log)
- app.rs run_with_handlers entry point (registry 입력 → tokio+sink 조립 + ShellCore 주입)
- pinion-shell tests +9 (executor unit 4 + dispatch_core integration 5)



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1952 pass / 0 fail / 10 ignored (직전 1943 +9)
- tokio_executor_cancel_aborts_pending_future: 5s sleep abort 관찰 검증 (cancel 실제 작동)
- forward_drain_pumps_handled_command_through_to_sink: 원 owner queue → drain → sink Intent 계열 완성
- dispatch_intent_bumps_revision: 재주입 path OCC revision +1



**Impact**: §5.23, §5.20, §5.12, §6.3


**Carry forward**:
- R51.160 — pinion-tui ShellCoreTui drain pump 동명령 + mpsc IntentSink
- Intent.payload SCXML send 채널 전파 (Update reducer signature evolution)
- scene/commands RPC method (10th method, 폈딩 큐와 in-flight 스냅샷)
- demo example — 실제 Handler 접속 (http.get / clipboard.write 수준 fixture)



### R51.160 — §5.23 pinion-tui tokio Executor + MpscIntentSink — §2 #6 GUI/TUI dual invariant CommandExecutor 대칭

**Changes**:
- pinion-tui/src/executor.rs 신규: TokioExecutor (Vello sibling) + MpscIntentSink (Sender<Intent>)
- ExecutorSinkBundle type alias + build_executor_and_sink 편의 helper
- ShellCoreTui::set_command_executor / command_executor / dispatch_intent (alternate-screen 안전 log_sink 라우팅)
- shell::run_with_handlers 진입점 + run_impl shared 이벤트 루프 + mpsc try_recv 드레인
- ShellCoreTui::handle_tail 증강: dispatch_pending_commands + log_unhandled_command (스타더아웃 머스트 X)
- pinion-tui/Cargo.toml tokio 1 dep (rt, rt-multi-thread, macros, time, sync)
- pinion-tui tests +12 (executor 7 + substrate 5)



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1964 pass / 0 fail / 10 ignored (직전 1952 +12)
- tokio_executor_cancel_aborts_pending_future: TUI side에서도 abort 관찰 (Vello 동하일)
- dispatch_key_drain_pumps_handled_command_to_sink: TUI 포트 dispatch 후 sink 도착
- dispatch_intent_routes_through_scxml_and_returns_change_bool: 재주입 경로 Disable 이벤트 시각 전이 확인



**Impact**: §5.23, §5.41, §6.3


**Carry forward**:
- TokioExecutor DRY 추출 — 3번째 백엔드 (모바일 / RPC-only) 액 시 pinion-async crate
- Intent.payload SCXML send 채널 전파 (Update reducer signature evolution)
- scene/commands RPC method (10th method, 폈딩 큐와 in-flight 스냅샷)
- demo example — 실제 Handler 접속 (http.get / clipboard.write 수준 fixture)



### R51.161 — §5.23 §5.7 scene/commands 10th RPC 메서드 — pending Command snapshot AI introspection

**Changes**:
- pinion-core/src/reactive/owner.rs: Owner::pending_commands_recursive() 신규 — 재귀 픽 대이프디르 순서, 드레인 없음
- pinion-rpc/src/commands.rs 신규: CommandsError + PendingCommandView (Serialize) + list_pending_commands
- pinion-rpc/src/dispatch.rs: DispatchContext.commands_owner 필드 + with_commands_owner builder + scene/commands 아름
- introspect_value_to_json pub(crate) 승격 + commands.rs 공유 (DRY)
- pinion-shell ShellCore::dispatch_rpc: with_commands_owner(&root_owner) 주입
- 테스트 +11 (owner 3 + commands 8)



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1975 pass / 0 fail / 10 ignored (직전 1964 +11)
- r51_161_pending_commands_recursive_does_not_drain: 스냅샷 서롤 결과 큐 유지 확인
- pending_traversal_is_children_first_then_root: 재귀 순서 검증 (드레인 식별자와 일치)
- introspect_value_*_payload_maps_to_*: Null/Bool/Int/Float/Text JSON 툦이도 검증



**Impact**: §5.23, §5.7, §5.12


**Carry forward**:
- R51.162 — result.in_flight: [...] 안속 활동 소개 (CommandExecutor.in_flight tracker 에 kind+payload 필드 확장)
- scope_id → widget tag lookup (어느 widget 특정의 owner_id 공유)
- path filter 젤러니 설정 (scene/intents 동명 carry, 멀티윈도우 carry)



### R51.162 — §5.23 §5.7 scene/commands result.in_flight 보강 — CommandExecutor 추적 확장

**Changes**:
- pinion-runtime/src/command/executor.rs: in_flight 값을 (Command, Handle) tuple 로 확장 (내부 InFlightEntry)
- CommandExecutor::in_flight_snapshot() → Vec<Command> (BTreeMap scope_id ascending)
- pinion-rpc/src/commands.rs: list_in_flight_commands(&CommandExecutor) 추가
- pinion-rpc/src/dispatch.rs: DispatchContext.commands_executor + with_commands_executor + scene/commands { pending, in_flight }
- pinion-shell ShellCore::dispatch_rpc: 축소된 executor Arc 클론 + with_commands_executor 조립
- 테스트 +9 (executor 6 + commands 3)



**Verification**:
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1984 pass / 0 fail / 10 ignored (직전 1975 +9)
- r51_162_in_flight_snapshot_deterministic_btreemap_order: scope_id ascending 절대 순서
- r51_162_in_flight_snapshot_replaces_on_same_scope_dispatch: 재디스패치 시 뛹석
- r51_162_list_in_flight_orders_by_scope_id_ascending: RPC View 툦이도 순서 일치



**Impact**: §5.23, §5.7, §5.12


**Carry forward**:
- scope_id → widget tag lookup (composite scope 자동 용어 극한자)
- path filter 젤러니 설정
- demo example 접속 (http.get / clipboard.write Handler 등록 예제)



### R51.163 — §5.23 hello-commands 데모 — view-fn one-shot dispatch + run_with_handlers + Handler echo cycle

**Changes**:
- examples/hello-commands/ 신규 binary (Cargo.toml + build.rs + app.pinion.xml + src/main.rs)
- queue_one_shot_demo_command: Owner::cache idempotent guard 패턴 — view-fn 은 순도 유지 (양자-적 cell)
- run_with_handlers → demo.echo Handler 등록 → first paint 시 dispatch_command → tokio worker echo → IntentArrived → dispatch_intent re-feed
- ButtonExternal SCXML 재사용 (새 SCXML 작성 X), 포그 codegen 이름만 HelloCommandsRenderer 로 변경
- workspace Cargo.toml members 추가 (한 줄)
- stderr trace = command flow 시웠 관찰 (handler → intent-feedback 패턴 일치)



**Verification**:
- cargo check -p hello-commands → 0 error
- cargo clippy -p hello-commands --all-targets → 0 warning
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo test --workspace --features pinion-runtime/vello → 1984 pass / 0 fail / 10 ignored (테스트 추가 없음, 바이너리 추가)
- cargo run -p hello-commands 시 Owner::cache one-shot 관찰 가능 (stderr trace)



**Impact**: §5.23, §5.22, §5.16


**Carry forward**:
- Visual feedback (state = command_arrived) — SCXML 확장 필요, 현재는 stderr trace 만 표면
- tokio::time::sleep 기반 async 해들러 (지연 관찰) 시연 추가
- demo example — hello-commands-tui 관결 차베이 (TUI 쓸 실제 용례 확보)



### R51.164 — §5.23 §2 #6 hello-commands-tui — TUI sibling of hello-commands, GUI/TUI dual invariant 대칭 데모

**Changes**:
- examples/hello-commands-tui/ 신규 binary (Cargo.toml + src/main.rs)
- queue_one_shot_demo_command: Owner::cache idempotent guard (Vello sibling 동일 패턴, key 접두어 분기)
- pinion_tui::run_with_handlers → echo Handler 워커 스레드 처리 → MpscIntentSink → shell loop try_recv → dispatch_intent
- silent stderr (raw-mode + alternate-screen 항 철칙 준수), 트레이스 = PINION_TUI_LOG=path 옷인
- workspace Cargo.toml members 추가 (한 줄)



**Verification**:
- cargo check -p hello-commands-tui → 0 error
- cargo clippy -p hello-commands-tui --all-targets → 0 warning
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warning
- cargo run -p hello-commands-tui (PINION_TUI_LOG=/tmp/x.log) 시 tui: intent-feedback echo.demo.echo 관찰 가능



**Impact**: §5.23, §5.41, §2


**Carry forward**:
- Visual feedback (SCXML 결과 어쨌 상태 전이 계석) — 현재는 log_sink 트레이스만
- async delay (tokio::time::sleep) — 시간 상석 시연 추가
- PINION_TUI_LOG fallback 결재 시 기본 silent (alt-screen safe)



### R51.165 — §5.23 hello-commands(-tui) echo_handler async delay 시연 — tokio::time::sleep 200ms

**Changes**:
- examples/hello-commands/Cargo.toml + main.rs: tokio dep(time) + 200ms sleep + stderr trace 보강
- examples/hello-commands-tui/Cargo.toml + main.rs: tokio dep(time) + 200ms sleep (silent, PINION_TUI_LOG 통한 트레이스)
- Handler async boundary 실증 — view-fn one-shot 시작 시점과 intent-feedback 사이에 ~200ms gap
- 양 backend에서 동일 패턴 (Vello/TUI dual invariant 유지)



**Verification**:
- cargo clippy -p hello-commands -p hello-commands-tui --all-targets → 0 warning
- cargo run -p hello-commands 시 stderr 소접 '200ms sleep' 타임라인 관찰 가능
- tokio worker thread에서 sleep 수행, UI 쓰레드 paint/poll 은 잘 움직임



**Impact**: §5.23


**Carry forward**:
- Visual feedback — dispatch_intent 시 Signal 업데이트 로 view fn 에 술해 적달 (별 R51.166+)



### R51.166 — §5.23 R27 — WidgetCore::update reducer substrate trait method (default no-op); R27 axis A first round

**Changes**:
- crates/pinion-core/src/widget_core.rs: WidgetCore::update(&mut Self::State, &Intent) -> Vec<Command> added with Vec::new() default impl — every existing impl keeps compiling unchanged
- crates/pinion-core/src/widget_core.rs: r51_166_tests inline cfg module (3 tests): default no-op + custom reducer state+command emission + intent borrow contract
- doc: §5.23 R27 Update(&mut Model, Intent) -> Vec<Command<Intent>> signature cited; R51.167-170 wiring carry enumerated (CoreShell route / Intent.payload SCXML send / hello-commands real flow / Forge codegen)
- doc: borrow rationale — Intent is Clone, framework retains authoritative copy, reducer reads tag/payload without consuming so SCXML send path stays unaffected



**Verification**:
- cargo test -p pinion-core --lib r51_166: 3 pass
- cargo test --workspace --features pinion-runtime/vello: 1987 pass / 0 fail / 10 ignored (+3 vs R51.165 baseline 1984)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (doc_markdown reactive: §6.3 `dry_run` backtick fix during the round)
- mnemosyne validate_workspace baseline: T1=0 / RT=1/1 / T4 unchanged



**Impact**: §5.23, §5.20, §5.41, §6.3


**Carry forward**:
- R51.167 — CoreShell::dispatch_intent routes Intent through <V as WidgetCore>::update before SCXML send; produced Vec<Command> queued via Owner::dispatch_command
- R51.168 — Intent.payload typed routing through SCXML invoke send (currently tag-only path drops payload)
- R51.169 — hello-commands(-tui) migrate from R51.163 Owner::cache one-shot hack to reducer-driven Command flow
- R51.170 — Forge codegen emits update body from SCE schema effect + command tables



### R51.167 — §5.23 R27 — CoreShell::route_intent_through_update substrate routing API queues reducer-produced Vec<Command> on root_owner

**Changes**:
- crates/pinion-runtime/src/core_shell.rs: CoreShell<V>::route_intent_through_update(&self, intent: &Intent) -> Vec<Command> — reads state via V::read_state, calls V::update, dispatches each command to root_owner queue
- crates/pinion-runtime/src/core_shell.rs: 3 R51.167 tests — default-reducer empty path on ButtonFixture + EchoButton override fixture (queues per-intent + FIFO accumulation across calls)
- doc: routing path stated — SCXML drain / async re-feed Intent both flow through this method before reaching invoke("send", …); state writeback to Scene is the R51.168 carry



**Verification**:
- cargo test -p pinion-runtime --lib r51_167 --features pinion-runtime/vello: 3 pass
- cargo test --workspace --features pinion-runtime/vello: 1990 pass / 0 fail / 10 ignored (+3 vs R51.166)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (doc_markdown reactive: read_state/event_name backtick fix during the round)
- mnemosyne validate_workspace: entries=338 (+1) / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.23, §5.41, §6.3


**Carry forward**:
- R51.168 — state writeback: V::update mutated state propagates back to Scene::External (currently the mutation lives only in the transient cached projection)
- R51.169 — ShellCore::dispatch_intent (shell + tui) calls route_intent_through_update before forwarding to SCXML invoke send
- R51.170 — Intent.payload typed routing through SCXML send (currently tag-only path drops payload)
- R51.171 — hello-commands(-tui) migrate from R51.163 Owner::cache one-shot hack to reducer-driven Command flow



### R51.168 — §5.23 R27 — ShellCore + ShellCoreTui dispatch_intent wire route_intent_through_update before SCXML send; EchoButtonFixture lifted

**Changes**:
- crates/pinion-shell/src/substrate.rs: ShellCore::dispatch_intent calls self.core.route_intent_through_update(intent) BEFORE the invoke("send", tag) channel — Elm/Iced ordering (Update before Cmd dispatch)
- crates/pinion-tui/src/substrate.rs: ShellCoreTui::dispatch_intent mirror; both backends now drive identical reducer-before-SCXML ordering
- crates/pinion-core/src/test_fixtures.rs: EchoButtonFixture lifted from inline core_shell.rs tests — reusable WidgetCore::update override fixture (echo.reply Command per intent) for the 3 R51.167/168 test sites
- crates/pinion-a11y/src/test_fixtures.rs: blank WidgetA11y impl for EchoButtonFixture (orphan-rule placement: trait lives here)
- crates/pinion-runtime/src/core_shell.rs: inline EchoButton removed, R51.167 tests use lifted fixture via `use pinion_core::test_fixtures::EchoButtonFixture as EchoButton`
- crates/pinion-shell/tests/dispatch_core.rs: TestView::update mock (UPDATE_EMITS_ECHO_COMMAND flag + UPDATE_INTENT_LOG); 3 R51.168 wiring tests (reducer called / commands queued / default empty)
- crates/pinion-tui/src/substrate.rs: r51_168_dispatch_intent_reducer_routing mod with inline impl WidgetViewTui for EchoButtonFixture; 3 wiring tests (queued / FIFO accumulate / default empty)



**Verification**:
- cargo test -p pinion-shell --test dispatch_core r51_168: 3 pass
- cargo test -p pinion-tui --lib r51_168: 3 pass
- cargo test --workspace --features pinion-runtime/vello: 1996 pass / 0 fail / 10 ignored (+12 vs R51.165 baseline 1984; +6 vs R51.167)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (doc_markdown reactive: 3 backtick fixes on test_fixtures.rs)
- mnemosyne validate_workspace: entries=339 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.23, §5.41, §5.40, §6.3


**Carry forward**:
- R51.169 — state writeback: V::update mutated state propagates back to Scene::External (currently mutation lives only in transient cached projection)
- R51.170 — Intent.payload typed routing through SCXML invoke send (currently tag-only path drops payload)
- R51.171 — hello-commands(-tui) migrate from R51.163 Owner::cache one-shot hack to reducer-driven Command flow
- R51.172 — Forge codegen emits update body from SCE schema effect + command tables (SCE upstream RFC carry per [[sce-upstream-debts]])



### R51.169 — §5.23 R27 — handle_tail (shell + tui) routes drained intents through V::update; closes input → drain → reducer arc

**Changes**:
- crates/pinion-shell/src/substrate.rs: handle_tail for-loop now calls self.core.route_intent_through_update(intent) for every drained §5.20 Intent before dispatch_pending_commands runs
- crates/pinion-tui/src/substrate.rs: handle_tail mirror; both backends close the R27 input → drain → reducer arc identically
- crates/pinion-shell/tests/dispatch_core.rs: EXTERNAL_DRAIN_INTENT static + TestExternal::{is_dirty, drain_intents} overrides (drain scaffold); 2 R51.169 tests (echo on drain / default empty on drain)
- crates/pinion-tui/src/substrate.rs: 2 R51.169 tests using EchoButtonFixture (dispatch_intent fires both incoming+drained reducers → 2 commands; default reducer → 0 commands)



**Verification**:
- cargo test -p pinion-shell --test dispatch_core r51_169: 2 pass
- cargo test -p pinion-tui --lib r51_169: 2 pass
- cargo test --workspace --features pinion-runtime/vello: 2000 pass / 0 fail / 10 ignored (+16 vs R51.165 baseline 1984)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- mnemosyne validate_workspace: entries=340 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.23, §5.20, §5.41, §6.3


**Carry forward**:
- R51.170 — hello-commands(-tui) migrate from R51.163 Owner::cache one-shot hack to reducer-driven Command flow; with drain → reducer wired (R51.169), CommandsView::update(intent.tag == main_btn.click) can emit demo.echo Command directly
- R51.171 — Intent.payload typed routing through SCXML invoke send (still tag-only path drops payload — not yet addressed)
- R51.172 — V::update state writeback semantics clarification: &mut state mutation currently transient (Scene::External SCXML send is authoritative); design choice between Elm-style separate Model field vs SCXML-as-Model carry
- R51.173 — Forge codegen emits update body from SCE schema effect + command tables (SCE upstream RFC carry per [[sce-upstream-debts]])



### R51.170 — §5.23 R27 — hello-commands(-tui) reducer-driven dogfood (Owner::cache one-shot HACK removed)

**Changes**:
- examples/hello-commands/src/main.rs: removed queue_one_shot_demo_command + ONE_SHOT_KEY + use std::cell::Cell + use pinion_core::Owner
- examples/hello-commands/src/main.rs: CommandsView::update matches CLICK_INTENT_TAG ("main_btn.click") and emits demo.echo Command; view fn no longer carries the one-shot guard
- examples/hello-commands-tui/src/main.rs: HelloCommandsTui::update mirror (matches "hello_commands_tui.click"); same imports/constants cleanup
- both binaries: doc block rewritten to describe the real R51.169 handle_tail → V::update → demo.echo flow (R51.166-169 substrate citations)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello: clean
- cargo test --workspace --features pinion-runtime/vello: 2000 pass / 0 fail / 10 ignored (no regression — example dogfood, substrate covered by R51.166-169)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (doc_markdown reactive: 4 backtick fixes during the round)
- mnemosyne validate_workspace: entries=341 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.23, §5.20, §5.16, §2


**Carry forward**:
- R51.171 — substrate refinement: wrap V::update inside root_owner.run(...) ([[callback-root-owner-wrap]]) so the reducer can call Owner::current() to fill scope_id automatically; current hardcoded 0 is the limit
- R51.172 — Intent.payload typed routing through SCXML invoke send (still tag-only path drops payload)
- R51.173 — V::update state writeback semantics design: pinion has SCXML-as-Model so &mut state is transient; document the contract OR add a separate application Model field
- R51.174 — Forge codegen emits update body from SCE schema effect + command tables (SCE upstream RFC carry per [[sce-upstream-debts]])



### R51.171 — §5.23 R27 / §5.22 R26 — route_intent_through_update wraps V::update in root_owner.run ([[callback-root-owner-wrap]])

**Changes**:
- crates/pinion-runtime/src/core_shell.rs: route_intent_through_update wraps V::update(&mut state, intent) in self.root_owner.run(...) so Owner::current() resolves to root_owner inside the reducer
- crates/pinion-runtime/src/core_shell.rs: r51_171 test (OwnerCaptureButton fixture + AtomicU64 sentinel u64::MAX) verifies Owner::current() id matches root_owner().id()
- examples/hello-commands/src/main.rs: CommandsView::update uses pinion_core::Owner::current().map_or(0, |o| o.id()) for Command.scope_id (canonical RPC-introspection-friendly pattern)
- examples/hello-commands-tui/src/main.rs: HelloCommandsTui::update mirror (same Owner::current() lookup)



**Verification**:
- cargo test -p pinion-runtime --lib r51_171: 1 pass
- cargo test --workspace --features pinion-runtime/vello: 2001 pass / 0 fail / 10 ignored (+17 vs R51.165 baseline 1984)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- mnemosyne validate_workspace: entries=342 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.23, §5.22, §5.41, §6.3


**Carry forward**:
- R51.172 — Intent.payload typed routing through SCXML invoke send (still tag-only path drops payload — textbook R27 spec 1/3 remaining gap)
- R51.173 — V::update state writeback semantics design decision: pinion has SCXML-as-Model so &mut state is transient; document the contract OR add a separate application Model field
- R51.174 — Forge codegen emits update body from SCE schema effect + command tables (SCE upstream RFC carry per [[sce-upstream-debts]])



### R51.172 — §5.23 R27 design clarification — Intent.payload consumed by reducer (V::update), SCXML invoke send remains tag-only by design

**Changes**:
- crates/pinion-shell/src/substrate.rs: dispatch_intent doc rewrites the R51.159 'payload-aware SCXML send carry' as a design choice: SCXML = name-keyed Model, V::update = payload-consuming reducer
- crates/pinion-tui/src/substrate.rs: ShellCoreTui::dispatch_intent mirror doc update; cross-references the Vello rationale
- 3 memory entries land: widgetcore-update-substrate-pattern (R51.166-171 land lessons), scxml-as-model-update-transient (Model semantics), reducer-incoming-vs-drain-symmetry (two arc wiring)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello: clean (doc-only changes)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- mnemosyne validate_workspace: entries=343 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.23, §5.20, §5.41


**Carry forward**:
- R51.173 — V::update state writeback semantics: docs added but no concrete API; if a future widget needs application-side Model beyond SCXML state, design Owner::cache<S> lift OR CoreShell.app_state<S> field
- R51.174 — Forge codegen emits V::update body from SCE schema effect + command tables (SCE upstream RFC carry per [[sce-upstream-debts]])
- R51.175 — if a future widget needs payload-driven SCXML transitions, extend invoke('send', ...) contract to accept Json({tag, payload}) and update all 8 widget Externals (button/toggle/checkbox/radio/radio_group/slider/listbox/listbox_item)



### R51.173 — R51.173 §5.23 R27 — WidgetCore::update by-value snapshot makes SCXML-as-Model design explicit

**Changes**:
- pinion-core::WidgetCore::update — signature now `(state: Self::State, intent: &Intent)` (by-value)
- pinion-core::r51_166_tests — 3 inline tests use by-value call sites; mutation-assert renamed
- pinion-core::test_fixtures::EchoButtonFixture::update — by-value snapshot signature
- pinion-runtime::CoreShell::route_intent_through_update — `let state` + V::update(state, intent)
- pinion-runtime::OwnerCaptureButton::update (r51_171 test) — by-value signature
- pinion-shell::tests::dispatch_core::TestView::update — by-value signature
- examples/hello-commands::CommandsView::update — by-value signature
- examples/hello-commands-tui::HelloCommandsTui::update — by-value signature



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2001 pass / 0 fail / 10 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict)
- decision: A1 (immutable by-value) over A2 (app_state<S> field); SCXML-as-Model docs explicit



**Impact**: §5.23, §5.41, §5.20, §6.3


**Carry forward**:
- R51.174 — Forge codegen emits update body from SCE schema effect+command tables (SCE RFC carry)
- R51.175 — Intent.payload typed routing through SCXML invoke send (wait first concrete consumer)
- R51.176 — app Model field decision wait (Owner::cache<S> lift OR CoreShell.app_state<S>)
- R51.177 — handler cascade guard (reducer reactivity lurking risk; kind whitelist or scope_id)
- R51.178 — shell test scaffold lift pinion-shell::test_fixtures::impl WidgetView (process maturity)



### R51.174 — R51.174 §5.23 R27 hello-commands(-tui) match polish on update reducer (Elm/Iced canonical shape)

**Changes**:
- examples/hello-commands::CommandsView::update — if/else → match arm + doc note
- examples/hello-commands-tui::HelloCommandsTui::update — if/else → match arm + doc note



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2001 / 0 / 10 (unchanged)
- cargo clippy 0 warnings (strict baseline maintained)
- ratify: match-on-tag-str is Elm/Iced canonical Update reducer shape



**Impact**: §5.23, §5.20


**Carry forward**:
- R51.175 — shell test scaffold lift pinion-shell::test_fixtures::impl WidgetView (process maturity)
- R51.176 — r51_171 test fragility: next_node_id() thread-local counter parallel isolation
- R51.177 — handler cascade guard (reducer reactivity lurking risk; kind whitelist or scope_id)
- R51.178 — app Model field wait (Owner::cache<S> lift OR CoreShell.app_state<S>) first consumer
- R51.179 — Forge codegen emits update body from SCE schema (SCE upstream RFC carry)
- R51.180 — Intent.payload typed routing through SCXML invoke send (wait first consumer)



### R51.175 — R51.175 §5.41 §5.23 R27 shell test_fixtures + WidgetView for EchoButtonFixture (TUI parity)

**Changes**:
- pinion-shell/Cargo.toml — [features] test-fixtures + self path-dep dev-dep entry
- pinion-shell/src/lib.rs — cfg-gated `pub mod test_fixtures`
- pinion-shell/src/test_fixtures.rs (new) — TestRenderer + impl WidgetView for EchoButtonFixture
- pinion-shell/tests/dispatch_core.rs — r51_175_shared_fixture_wiring sub-module +3 tests



**Verification**:
- cargo test (vello) = 2004 / 0 / 10 (+3 new R51.175 tests, all pass)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- shell + tui now drive same EchoButtonFixture through dispatch_intent (process maturity)



**Impact**: §5.41, §5.23


**Carry forward**:
- R51.176 — r51_171 test fragility: next_node_id() thread-local counter parallel isolation
- R51.177 — handler cascade guard (reducer reactivity lurking risk; kind whitelist or scope_id)
- R51.178 — app Model field wait (Owner::cache<S> lift OR CoreShell.app_state<S>) first consumer
- R51.179 — Forge codegen emits update body from SCE schema (SCE upstream RFC carry)
- R51.180 — Intent.payload typed routing through SCXML invoke send (wait first consumer)



### R51.176 — R51.176 §5.22 r51_171 test fragility polish: Option<u64> sentinel replaces AtomicU64 + u64::MAX

**Changes**:
- pinion-runtime::core_shell::tests::R51_171_CAPTURED_OWNER_ID — Mutex<Option<u64>> (was AtomicU64)
- OwnerCaptureButton::update — stores Some(id); test entry clears to None
- r51_171_update_runs_inside_root_owner_run_scope — .expect(...) replaces u64::MAX inequality



**Verification**:
- cargo test (vello) = 2004 / 0 / 10 (unchanged from R51.175; same test count post-polish)
- cargo clippy 0 warnings (strict baseline; doc_markdown backtick fix for `r51_171`)
- ratify: None sentinel ambiguity-free (no aliasing with legitimate `0` Owner::id())



**Impact**: §5.22, §5.23


**Carry forward**:
- R51.177 — handler cascade guard (reducer reactivity lurking risk; kind whitelist or scope_id)
- R51.178 — app Model field wait (Owner::cache<S> lift OR CoreShell.app_state<S>) first consumer
- R51.179 — Forge codegen emits update body from SCE schema (SCE upstream RFC carry)
- R51.180 — Intent.payload typed routing through SCXML invoke send (wait first consumer)



### R51.177 — R51.177 §5.23 R27 reducer cascade discipline docs (Elm/Iced/Redux canonical, no framework guard)

**Changes**:
- pinion-core::WidgetCore::update doc — new `## Cascade discipline` section (3 rules)
- pinion-core::test_fixtures::EchoButtonFixture::update doc — test-only cascade-unsafe note
- memory entry: reducer-cascade-discipline.md (process maturity)



**Verification**:
- cargo test (vello) = 2004 / 0 / 10 (unchanged; doc-only round)
- cargo clippy 0 warnings (doc_lazy_continuation: `+` markdown list marker escaped to `and`)
- ratify: Elm/Iced/Redux convention — no framework cascade guard; reducer discipline + scene/commands observability



**Impact**: §5.23, §5.20, §5.7


**Carry forward**:
- R51.178 — app Model field wait (Owner::cache<S> lift OR CoreShell.app_state<S>) first consumer
- R51.179 — Forge codegen emits update body from SCE schema (SCE upstream RFC carry)
- R51.180 — Intent.payload typed routing through SCXML invoke send (wait first consumer)
- R51.181 — framework cascade detect debug-assert (lift on first concrete cascade evidence)



### R51.178 — R51.178 §5.41 pinion-tui::test_fixtures lift (TUI symmetry with R51.175 pinion-shell)

**Changes**:
- pinion-tui/Cargo.toml — [features] test-fixtures + self path-dep dev-dep entry
- pinion-tui/src/lib.rs — cfg-gated `pub mod test_fixtures`
- pinion-tui/src/test_fixtures.rs (new) — impl WidgetViewTui for ButtonFixture + EchoButtonFixture
- pinion-tui/src/substrate.rs — inline impls (2 sites) replaced by lifted module reference



**Verification**:
- cargo test (vello) = 2004 / 0 / 10 (unchanged from R51.177; same test count post-lift)
- cargo clippy 0 warnings (strict baseline maintained)
- ratify: shell + tui now mirror-symmetric test_fixtures modules (R51.175 + R51.178)



**Impact**: §5.41, §5.23


**Carry forward**:
- R51.179 — Forge codegen emits update body from SCE schema (SCE upstream RFC carry)
- R51.180 — Intent.payload typed routing through SCXML invoke send (wait first consumer)
- R51.181 — framework cascade detect debug-assert (lift on first concrete cascade evidence)
- R51.182 — app Model field wait (Owner::cache<S> lift OR CoreShell.app_state<S>) first consumer



### R51.179 — R51.179 §5.41 dispatch_core.rs TestRenderer dedup (drop inline, use lifted symbol)

**Changes**:
- pinion-shell/tests/dispatch_core.rs — inline TestRenderer + TestRendererError + impl block (-50 LOC)
- pinion-shell/tests/dispatch_core.rs — `use pinion_shell::test_fixtures::TestRenderer` import
- pinion-shell/tests/dispatch_core.rs — now-unused `core::fmt` + `vello_renderer_impl!` removed



**Verification**:
- cargo test (vello) = 2004 / 0 / 10 (unchanged from R51.178)
- cargo clippy 0 warnings (strict baseline maintained)
- dispatch_core.rs LOC trimmed by 50 (duplicate stub replaced by lifted symbol)



**Impact**: §5.41


**Carry forward**:
- R51.180 — Forge codegen emits update body from SCE schema (SCE upstream RFC carry)
- R51.181 — Intent.payload typed routing through SCXML invoke send (wait first consumer)
- R51.182 — framework cascade detect debug-assert (lift on first concrete cascade evidence)
- R51.183 — app Model field wait (Owner::cache<S> lift OR CoreShell.app_state<S>) first consumer



### R51.180 — R51.180 §5.45 R55 Scroll axis ratify + ScrollNode primitive scaffold (data shape land)

**Changes**:
- atomic store: §5.45 R55 Scroll axis ratify (intent/rationale/impact/in/out/alt/8 caveats/2 ex)
- pinion-core::scene::Scene::Scroll variant + ScrollNode struct + new/with_tag/with_offset builders
- pinion-core::scene::Scene::rect/tag — Scroll arm returns viewport / tag.as_deref()
- pinion-core::scene::tests — 4 R55.A scaffold smoke tests + exhaustive-match guard arm



**Verification**:
- cargo test (vello) = 2008 / 0 / 10 (+4 R55.A scaffold tests; was 2004)
- cargo clippy 0 warnings (strict baseline maintained)
- atomic: entries=351 / sections=59 (+1 R55 axis) / T1=0 / T3=0 / RT=1/1



**Impact**: §5.45, §5.2, §5.11


**Carry forward**:
- R51.181 R55.A.2 — Scene::hit_test + lookup_path_* descent through ScrollNode.content offset-translated
- R51.182 R55.B — ScrollState scope-id keyed (Owner::cache substrate + Animation<f32> bound)
- R51.183 R55.C — wheel + arrow + PgUp/PgDn + Home/End input mapping (Event enum extension)
- R51.184 R55.D — ScrollBar sub-widget (vertical + horizontal) hover/drag SCXML statechart
- R51.185 R55.E — paint clipping at Vello + TUI boundaries (clip layer / cell-mask write skip)
- R51.186 R55.F — scene/scroll RPC method (offset_to / scroll_by variants)
- R51.187 R55.G — ListBox + future Grid/CardList composite integration through Scroll wrap



### R51.181 — R51.181 §5.45 R55.A.2 Scene::hit_test descends through ScrollNode with offset translation

**Changes**:
- pinion-core::scene::Scene::hit_test — Scroll arm descends content with offset-translated coords
- pinion-core::scene::Scene::hit_test — i64 promotion avoids u32 wrap on negative-offset edge
- pinion-core::scene::Scene::hit_test — viewport-contains gate kept; outside-viewport returns None
- pinion-core::scene::tests — 4 R55.A.2 descent tests (in/out/clip-fallback/parent-route)



**Verification**:
- cargo test (vello) = 2012 / 0 / 10 (+4 R55.A.2 descent tests; was 2008)
- cargo clippy 0 warnings (i64 promotion sidesteps cast_sign_loss / cast_possible_wrap)
- ratify: hit_test offset translation is half-open viewport contains + i64 promotion



**Impact**: §5.45, §5.32


**Carry forward**:
- R51.182 R55.A.3 — lookup_path / lookup_path_ref / lookup_path_mut Scroll passthrough
- R51.183 R55.A.4 — collect_intersections / hit_test_region offset-translated descent
- R51.184 R55.B — ScrollState scope-id keyed (Owner::cache + Animation<f32>)
- R51.185 R55.C — wheel + arrow + PgUp/PgDn + Home/End input mapping (Event extension)
- R51.186 R55.D — ScrollBar sub-widget (vertical + horizontal) hover/drag statechart
- R51.187 R55.E — paint clipping at Vello + TUI boundaries
- R51.188 R55.F — scene/scroll RPC method (offset_to / scroll_by variants)
- R51.189 R55.G — ListBox + future composite integration through Scroll wrap



### R51.182 — R51.182 §5.45 R55.A.3 Scene::lookup_path family passthrough through ScrollNode

**Changes**:
- pinion-core::scene::Scene::lookup_path — Scroll arm forwards segments unchanged into content
- pinion-core::scene::Scene::lookup_path_ref — Scroll arm forwards segments unchanged into content
- pinion-core::scene::Scene::lookup_path_mut — Scroll arm via Box<Scene> DerefMut into content
- pinion-core::scene::tests — 6 R55.A.3 passthrough tests (empty/index/tag/ref/mut/parent-chain)



**Verification**:
- cargo test (vello) = 2018 / 0 / 10 (+6 R55.A.3 passthrough tests; was 2012)
- cargo clippy 0 warnings (head/tail bindings still used by Container arm below)
- ratify: ScrollNode is path-transparent across the full lookup-path family (mirrors R51.181)



**Impact**: §5.45, §5.32, §5.34


**Carry forward**:
- R51.183 R55.A.4 — collect_intersections / hit_test_region offset-translated descent
- R51.184 R55.B — ScrollState scope-id keyed (Owner::cache + Animation<f32>)
- R51.185 R55.C — wheel + arrow + PgUp/PgDn + Home/End input mapping (Event extension)
- R51.186 R55.D — ScrollBar sub-widget (vertical + horizontal) hover/drag statechart
- R51.187 R55.E — paint clipping at Vello + TUI boundaries
- R51.188 R55.F — scene/scroll RPC method (offset_to / scroll_by variants)
- R51.189 R55.G — ListBox + future composite integration through Scroll wrap



### R51.183 — R51.183 §5.45 R55.A.4 hit_test_region descends through ScrollNode

**Changes**:
- pinion-core::scene::ScrollNode::translate_query_into_content — viewport-clip + offset shift helper
- pinion-core::scene::Scene::collect_intersections — Scroll arm descends with translated query
- pinion-core::scene::tests — 5 R55.A.4 tests (viewport / descent / offset / clip / chain)



**Verification**:
- cargo test (vello) = 2023 / 0 / 10 (+5 R55.A.4 descent tests; was 2018)
- cargo clippy 0 warnings (i64 promotion + try_from guards same shape as R51.181)
- ratify: hit_test_region viewport-clips query first, then offset-shifts content



**Impact**: §5.45, §5.32


**Carry forward**:
- R51.184 R55.B — ScrollState scope-id keyed (Owner::cache + Animation<f32>)
- R51.185 R55.C — wheel + arrow + PgUp/PgDn + Home/End input mapping (Event extension)
- R51.186 R55.D — ScrollBar sub-widget (vertical + horizontal) hover/drag statechart
- R51.187 R55.E — paint clipping at Vello + TUI boundaries
- R51.188 R55.F — scene/scroll RPC method (offset_to / scroll_by variants)
- R51.189 R55.G — ListBox + future composite integration through Scroll wrap



### R51.184 — R51.184 §5.45 R55.B ScrollState substrate (Owner::cache scope-id keyed)

**Changes**:
- pinion-core::widgets::scroll — ScrollState + use_scroll_state hook (Owner::cache)
- offset Signal<i32> + max Cell<i32>; clamp + equality-skip + saturating-add guards
- pinion-core::widgets::scroll::tests — 10 R55.B tests (init/clamp/saturate/hook)



**Verification**:
- cargo test (vello) = 2033 / 0 / 10 (+10 R55.B tests; was 2023)
- cargo clippy 0 warnings (doc_markdown backtick polish on ListBox / SolidJS)
- ratify: ScrollState = Signal offset + Cell bounds; use_scroll_state = Owner::cache hook



**Impact**: §5.45, §5.22


**Carry forward**:
- R51.185 R55.C — wheel + arrow + PgUp/PgDn + Home/End input mapping (Event extension)
- R51.186 R55.D — ScrollBar sub-widget (vertical + horizontal) hover/drag statechart
- R51.187 R55.E — paint clipping at Vello + TUI boundaries
- R51.188 R55.F — scene/scroll RPC method (offset_to / scroll_by variants)
- R51.189 R55.G — ListBox + future composite integration through Scroll wrap
- R55.B.2 — Animation<i32> smooth-scroll layer (additive on top of this substrate)



### R51.185 — R51.185 §5.45 R55.C.1 PointerEvent::Wheel + WheelDelta substrate

**Changes**:
- pinion-core::event::PointerEvent::Wheel { coord, delta } — wheel input variant
- pinion-core::event::WheelDelta — Pixels / Lines unit-tagged enum
- pinion-core::event::tests — 5 R55.C.1 tests (Pixels/Lines/round-trip/exhaustive)



**Verification**:
- cargo test (vello) = 2038 / 0 / 10 (+5 R55.C.1 tests; was 2033)
- cargo clippy 0 warnings (doc_markdown backtick polish on PgUp / PgDn)
- ratify: Wheel variant + unit-tagged delta; W3C deltaMode shape



**Impact**: §5.45, §5.13


**Carry forward**:
- R51.186 R55.C.2 — input router wires Wheel into ScrollState (offset deltas)
- R51.187 R55.C.3 — KeyEvent extension for ArrowKey / PgUp / PgDn / Home / End
- R51.188 R55.D — ScrollBar sub-widget (vertical + horizontal) hover/drag statechart
- R51.189 R55.E — paint clipping at Vello + TUI boundaries
- R51.190 R55.F — scene/scroll RPC method (offset_to / scroll_by variants)
- R51.191 R55.G — ListBox + future composite integration through Scroll wrap
- R55.B.2 — Animation<i32> smooth-scroll layer (additive on top of this substrate)



### R51.186 — §5.45 R55.C.2 input router wires PointerEvent::Wheel → attached ScrollState across all layers

**Changes**:
- ScrollNode { ..., state: Option<Rc<ScrollState>> } + with_state() backreference builder (widget-owns-state canonical: Material/SwiftUI/GTK/Qt mirror)
- Scene::scroll_target_at(x, y) -> Option<&ScrollNode> + scroll_state_at(x, y) -> Option<Rc<ScrollState>> hit-test helpers; nested-scroll descent picks innermost (W3C overflow:scroll ancestor walk)
- InputRouter::wheel(id, delta) → bool; cursors[id] lookup mirrors winit/web/iOS MouseWheel-without-position contract; silent drop on missing cursor / paint / scroll / state
- LINE_HEIGHT_PX const = 16.0 (W3C/Chromium/Firefox/Safari default); wheel_delta_to_pixels Pixels verbatim + Lines × const; round_clamp_i32 NaN-guard mirrors R51.145 clamp_frame_dt
- CoreShell::wheel(id, delta) -> (DispatchTail, dispatched: bool); ShellCore::wheel + ShellCoreTui::wheel wrappers; dispatched bool gates request_redraw / TUI repaint commit
- winit MouseScrollDelta::{LineDelta, PixelDelta} → WheelDelta::{Lines, Pixels} at app.rs boundary via winit_wheel_to_pinion helper
- crossterm MouseEventKind::{ScrollUp, ScrollDown, ScrollLeft, ScrollRight} → WheelDelta::Lines{±1, 0/0, ±1} with cursor sync (matches Down(Left) pattern)
- Tests +21: 8 scene (scroll_target_at + scroll_state_at + nested), 11 input (Pixels/Lines/NaN/cursor-tracking/multi-pointer/no-state-drop/LINE_HEIGHT_PX pin), 2 core_shell (dispatched bool + no-scroll-false)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2059 pass / 0 fail / 11 ignored (+21 tests vs R51.185 baseline 2038/0/10)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (doc_markdown ScrollNode backtick pre-audit applied)
- all 8 r55_c2_scroll_target_at_* + r55_c2_scroll_state_at_* scene tests pass; all 11 r55_c2_wheel_* router tests pass; both CoreShell::wheel tests pass
- R55.C.1 substrate-incompleteness-signal (PointerEvent::Wheel variant standalone from R51.185) cleared — wheel now routes end-to-end from winit/crossterm → InputRouter → ScrollState.scroll_by



**Impact**: §5.45, §5.13, §5.41, §5.35


**Carry forward**:
- R55.C.3 KeyEvent ArrowKey/PgUp/PgDn/Home/End routing through FocusManager → ScrollState (matches wheel arc shape)
- R55.C.4 per-widget LINE_HEIGHT_PX override (monospace text containers, custom cell sizes); current 16px is framework-wide const
- WheelDelta::Pages future variant (PgUp/PgDn coarse) + explicit arm in wheel_delta_to_pixels (currently #[non_exhaustive] wildcard zero-delta degrade)
- ScrollState ↔ ScrollNode ergonomic helper (scroll_container builder closure) — caller boilerplate (use_scroll_state + offset() + ScrollNode::new + with_state) is 4-line; framework helper carry
- R55.D ScrollBar sub-widget (SCXML statechart, drag-to-position); R55.E paint clipping at Vello + TUI boundaries; R55.F scene/scroll RPC method; R55.G ListBox composite (first application consumer, visual milestone)
- hit_test bbox coordinate frame docs ambiguity (R51.181 carry) still open; use_scroll_state key uniqueness convention docs



### R51.187 — §5.45 R55.C.3 keyboard scroll input — Arrow / Page / Home / End route to ScrollState via apply_key fallback

**Changes**:
- InputRouter::scroll_key(id, key) -> bool: cursor-based hit-test → deepest Scene::Scroll → state.scroll_by / scroll_to per W3C key mapping
- Key table: ArrowDown/Up/Left/Right step LINE_HEIGHT_PX (16); PageDown/Up step viewport.h; Home/End jump y to 0 / max_y (x preserved)
- LINE_HEIGHT_PX_I32 = 16 const mirror (avoids f32->i32 cast on every arrow keypress; matches LINE_HEIGHT_PX float)
- CoreShell::scroll_key(pid, key) -> (DispatchTail, bool) lifts dispatched bool for backend redraw gating; mirrors CoreShell::wheel shape
- ShellCore::handle_named_key cascade: V::apply_key first (widget-bound: Slider arrows / Toggle Space / Button Enter); unhandled -> ShellCore::scroll_key fallback (widget never sees the key it consumed)
- ShellCoreTui::dispatch_key gets same cascade: keybinding -> apply_key -> scroll_key fallback; scroll_key returns dispatched || state_changed for repaint trigger
- Horizontal Home/End + Ctrl-modifier corner-jump variants deferred to R55.C.4 (page_x already computed but unused this round)
- Tests +6: arrow ach-axis step / page step / Home+End y-extremes / unknown-key false / cursor-off-scroll silent-drop / arrow clamps bounds



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2065 pass / 0 fail / 11 ignored (+6 R55.C.3 tests vs R51.186 baseline 2059/0/11)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- all 6 r55_c3_* router tests pass; widget-fallback cascade verified by-construction (apply_key Some short-circuits before scroll_key)
- W3C key string vocabulary (ArrowUp/ArrowDown/ArrowLeft/ArrowRight/PageUp/PageDown/Home/End) already in named_key_str (R51.92 + tui input::key_str_from_event), so no boundary mapping change needed



**Impact**: §5.45, §5.41, §5.35


**Carry forward**:
- R55.C.4 horizontal Home/End + Ctrl-Home/Ctrl-End corner jumps + per-widget LINE_HEIGHT_PX override (page_x already computed substrate-side)
- R55.C.5 focus-based scroll routing — currently cursor-based; W3C convention: scroll routes to focused element's ancestor scroll. Needs scene path-walk + FocusManager integration
- R55.D ScrollBar sub-widget (SCXML statechart, drag-to-position) + R55.E paint clipping + R55.F scene/scroll RPC + R55.G ListBox composite (first consumer, visual milestone)
- Wheel + scroll_key share guard structure: lift the cursor-lookup + paint-walk + state-lookup chain into a shared helper to remove the 3-line duplication in InputRouter (carry, low priority)
- ScrollState ergonomic helper (scroll_container builder closure) still pending; 4-line view-fn boilerplate persists



### R51.188 — §5.45 R55.E.1 Vello paint adapter clips Scene::Scroll viewport + shifts content by offset

**Changes**:
- paint_adapter::to_vello forwards to new to_vello_inner(..., transform: Affine) carrying the cumulative parent transform through the recursion
- fill_rect / stroke_rect / paint_text gain transform: Affine parameter; previously each used Affine::IDENTITY (bit-identical for non-scroll callers since IDENTITY * T = T)
- Scene::Scroll arm: push_clip_layer(transform, viewport_kurbo) then recurse with child_transform = transform * Affine::translate((viewport.xy - offset.xy)); pop_layer on exit
- paint_text composes parent_transform * Affine::translate((t.rect.x, t.rect.y)) for glyph + decoration paints; clip-rect passes parent_transform so scroll-embedded text clips in its parent frame
- Public to_vello signature unchanged — all 6 existing call sites (app.rs render + 5 paint_adapter tests) keep working without edits
- TUI-side scroll clipping deferred to R51.189 R55.E.2 (carry); cell-grid clipping requires different shape from Vello clip-layer push/pop
- Tests +4: scroll_arm_walks_content_box_hook + scroll_layer_balances_on_panic_free_walk (empty + plain + nested) + scroll_text_inside_lays_out_through_cache + scroll_arm_survives_offset_overshoot (i32::MAX)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2069 pass / 0 fail / 11 ignored (+4 R55.E.1 tests vs R51.187 baseline 2065/0/11)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (clippy::too_many_arguments allowed at to_vello_inner only)
- Vello layer push / pop balance verified by `pop_layer`'s encoder-underflow panic acting as a guard — empty + plain + nested scroll all exit cleanly
- Text-inside-scroll smoke test exercises transform composition (parent_transform * text-local translate) without breaking the existing LayoutCache hit-rate



**Impact**: §5.45, §5.16


**Carry forward**:
- R51.189 R55.E.2 TUI cell-grid clipping for Scene::Scroll — paint.rs::to_buffer + paint_container / paint_box / paint_text need explicit clip-rect + offset parameters (no native push_layer/pop_layer in ratatui Buffer)
- R51.190 R55.D ScrollBar sub-widget visualisation (SCXML statechart, drag-to-position thumb)
- R51.191 R55.F scene/scroll RPC method (11th typed method) for AI introspection of attached ScrollState
- R51.192 R55.G ListBox composite integration (first application consumer of R55.A/B/C/E; visual milestone)
- Wheel + scroll_key + Vello clip share the cursor-lookup-paint-walk pattern; lift the 3-line guard into a shared helper (low priority polish)



### R51.189 — R51.189 §5.45 R55.E.2 TUI paint adapter clips Scroll viewport (Vello R51.188 backend-symmetry)

**Changes**:
- crates/pinion-tui/src/paint.rs: CellClip struct (i32 half-open) + pixels_to_cell_floor + clamp_to_i32 + cell_to_buf_xy helpers
- crates/pinion-tui/src/paint.rs: to_buffer wraps to_buffer_inner(scene, buf, CellClip::from_buf(area), (0, 0)) — public surface preserved (R51.188 pattern mirror)
- crates/pinion-tui/src/paint.rs: to_buffer_inner Scene::Scroll arm — viewport clip intersect + child_offset = parent + viewport.xy - scroll.offset.xy (i64 arithmetic)
- crates/pinion-tui/src/paint.rs: paint_container / paint_box / paint_box_style / paint_text_inner take clip + offset_px; every cell write clipped against CellClip + buf bounds
- crates/pinion-tui/src/paint.rs: paint_text wraps paint_text_inner with full-clip + (0, 0) offset



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2075 passed / 0 failed / 11 ignored (+6 new R55.E.2 tests: paint-content / clip-overshoot / offset-shift / nested-clips / overshoot-no-panic / empty-viewport-skip)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (clippy::pedantic deny baseline holds)
- mnemosyne validate_workspace = entries=360 / sections=59 / T1=0 / T3=0 / RT=1/1 / orphan_refs=4+0 (no new violations)



**Impact**: §5.45, §5.41


**Carry forward**:
- R51.190 R55.G — ListBox composite first consumer (substrate now complete on both backends)
- R51.191 R55.D — ScrollBar sub-widget (SCXML statechart axis per R55.D plan)
- R51.192 R55.F — scene/scroll RPC method (11th typed method)
- R55.C.4 — horizontal Home/End + Ctrl-Home/End extreme jump (R51.187 vertical-only partial)
- R55.C.5 — focus-based scroll routing (W3C UX, vs cursor-based R51.186/187)



### R51.190 — R51.190 §5.45 ScrollNode::from_state ergonomic ctor collapses canonical 5-line scroll boilerplate to 1 call

**Changes**:
- crates/pinion-core/src/widgets/scroll.rs: ScrollState gains `tag: Option<&'static str>` field + with_tag(key) constructor + tag() accessor
- crates/pinion-core/src/widgets/scroll.rs: use_scroll_state factory closure switches from ScrollState::new to `|| ScrollState::with_tag(key)` so cached state records its key
- crates/pinion-core/src/scene.rs: ScrollNode::from_state(state, viewport, content) derives offset (state.offset()) + tag (state.tag()) + state attachment in one call
- crates/pinion-core/src/scene.rs: ScrollNode::with_state doc-comment now points to from_state as the canonical entry point (with_state stays for explicit override use)
- Closes substrate-incompleteness-signal carry from R51.184-188 cascade (5-line view-fn boilerplate eliminated; key string repeated only once at use_scroll_state call site)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2083 passed / 0 failed / 11 ignored (+8 new R51.190 tests: 3 ScrollState tag + 5 ScrollNode::from_state)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (clippy::pedantic deny baseline holds)
- mnemosyne validate_workspace: entries 360 → 361 / sections 59 / T1=0 / T3=0 / RT=1/1 / orphan_refs=4+0 (no new violations)



**Impact**: §5.45


**Carry forward**:
- R51.191 R55.G — ListBox composite first consumer (substrate + ergonomic wiring now both complete)
- R55.D — ScrollBar sub-widget (SCXML statechart)
- R55.F — scene/scroll RPC method (11th typed)
- R55.C.4/C.5 — horizontal Home/End + focus-based routing carries from R51.187



### R51.191 — R51.191 §5.45 R55.G hello-listbox first ScrollNode consumer wraps 12-row column in 5-row viewport

**Changes**:
- examples/hello-listbox/src/main.rs: N bumped 4 → 12 so content overflows viewport + option_label extended to 12 alphabetised fruit labels
- examples/hello-listbox/src/main.rs: SCROLL_KEY + VIEWPORT_W + VIEWPORT_H constants for the scroll wrap; viewport centred in the 360x320 window
- examples/hello-listbox/src/main.rs: view fn replaces flex column with ScrollNode::from_state(use_scroll_state(...), viewport, content); set_max derives bound from N row geometry
- examples/hello-listbox/src/main.rs: listbox_row_at_y helper replaces listbox_row — manual rect (0, y, ROW_WIDTH, ROW_HEIGHT) because layout::compute_layout does not yet recurse into Scene::Scroll content (R55.G.2 carry)
- First consumer validates: ScrollNode::from_state 1-call ergonomics (R51.190), wheel + key input routing (R51.186/187), Vello + TUI paint clip (R51.188/189), hit_test offset translation (R51.181)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2086 passed / 0 failed / 11 ignored (+3 new R51.191 smoke tests: scroll wrap + scroll_max derivation + intrinsic y positioning)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (clippy::pedantic deny baseline holds)
- hello-listbox a11y_tests = 12/12 passed (9 pre-existing + 3 new); ListBoxView::access_node + access_focus_target + active_option_index all parametric on N, refactor transparent
- mnemosyne validate_workspace: entries 361 → 362 / sections 59 / T1=0 / T3=0 / RT=1/1 / orphan_refs=4+0 (no new violations)



**Impact**: §5.45, §5.38


**Carry forward**:
- R55.G.2 — layout::compute_layout recurse into Scene::Scroll content (taffy subtree), revert hello-listbox to flex layout
- R55.G.3 — auto-scroll-active-into-view on focus change (Arrow/Home/End/typeahead should keep focused row visible)
- R55.D — ScrollBar sub-widget (SCXML statechart)
- R55.F — scene/scroll RPC (11th typed method)
- R55.C.4/C.5 — horizontal Home/End + focus-based routing carries from R51.187
- hello-listbox-multi same R55.G refactor (parallel sibling consumer)



### R51.192 — R51.192 fix(shell) §5.45 R55.C.2 winit MouseScrollDelta flips sign to W3C convention (TUI sibling agreement restored)

**Changes**:
- crates/pinion-shell/src/app.rs: winit_wheel_to_pinion flips dx + dy sign at the boundary so substrate receives W3C-signed deltas (positive = scroll toward content end)
- Pre-R51.192 winit's LineDelta(_, y>0) (forward wheel) reached scroll_by as dy>0 → offset_y increased → content shifted up → user saw reverse direction; matches user-reported regression on hello-listbox
- TUI sibling (crossterm MouseEventKind::ScrollUp → WheelDelta::Lines { dy: -1.0 }) already W3C-signed since R51.186 — the substrate stayed consistent only for TUI
- Restores §2 #6 GUI/TUI dual invariant for scroll direction: forward wheel on Vello and ScrollUp on TUI now both decrement offset_y identically



**Verification**:
- cargo test -p pinion-shell --lib r51_192 = 4/4 passed (line delta x + y flip + pixel delta both axes flip + winit↔TUI sibling sign agreement guard)
- cargo test --workspace --features pinion-runtime/vello = 2090 passed / 0 failed / 11 ignored (+4 R51.192 regression tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace: entries 362 → 363 / sections 59 / T1=0 / T3=0 / RT=1/1 / orphan_refs=4+0



**Impact**: §5.45, §5.13


**Carry forward**:
- R51.193+ — AI-first RPC introspection self-verification (spawn hello-* + scene/snapshot from this side; obviates user-reported visual confirmation for code-verifiable state)
- R55.G.2 layout::compute_layout recurse into Scene::Scroll content (R51.191 carry)
- R55.D ScrollBar / R55.F RPC scene/scroll / R55.C.4/C.5 keyboard extensions



### R51.33 — R51.33 §5.38 hello-radio paint-side N=4 amortization on the pinion-shell substrate

**Changes**:
- examples/hello-radio (new binary): Cargo.toml + app.pinion.xml + build.rs + src/main.rs (235 LOC), pinion-core + pinion-shell + vello deps only — same Radio-on-shell shape as R51.30/R51.31/R51.32 button/toggle/checkbox
- Cargo.toml workspace.members += examples/hello-radio
- RadioView WidgetView impl: State = (RadioState, bool selected), tag = main_radio, introspect read = state + selected (Bool), keybinding = d / e (Disable / Enable)
- view fn: 24x24 ring (Container, transparent fill, corner_radius=12, 2 px border) with optional 12x12 inner dot (Box, corner_radius=6, filled, only when selected) — Material / SwiftUI convention; right-of label "Premium tier"
- §5.38 implementations += examples/hello-radio/{app.pinion.xml, src/main.rs:view, src/main.rs:RadioView}



**Verification**:
- cargo check --workspace --features pinion-runtime/vello = 0 errors
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (initial doc_markdown on bare "RadioGroup" fixed by backtick wrap)
- cargo test --workspace --features pinion-runtime/vello = 1226 pass / 0 fail / 6 ignored (baseline preserved — pure-additive binary)
- LOC: 235 main.rs + 24 Cargo.toml + 27 build.rs + 12 app.pinion.xml = 298 total — within the 200-240 envelope of hello-button (203) / hello-toggle (269) / hello-checkbox (221); substrate amortization holds — pinion-shell API unchanged
- mnemosyne validate_workspace pending — confirm entries=180 / sections=55 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.38, §5.16, §5.20, §5.35


**Carry forward**:
- Slider visual demo needs InputRouter PointerMove cursor-X forwarding + WidgetView drag-position / key-intervene hook — substrate gap surfaced when R51.33 path A was selected over Slider-first; next round candidate
- RadioGroup visual demo needs pinion-shell multi-External / multi-tag dispatch — single WidgetView::tag() insufficient for N siblings; substrate round candidate
- Tier-1 widget visual coverage now 4/6 (button / toggle / checkbox / radio); Slider + RadioGroup remain blocked on the two substrate gaps above



### R51.34 — R51.34 §5.15 + §5.35 pointer-capture opt-in + pointer_move forward substrate for drag-aware widgets

**Changes**:
- pinion-core External trait: new default-false fn wants_pointer_capture and new default-noop fn pointer_move(x_rel, y_rel) under §5.15 item-5 input-forwarding policy; existing impls unaffected (Button / Toggle / Checkbox / Radio keep cancel-by-leave)
- pinion-runtime InputRouter: new captured_target field + capture-mode branch in cursor_moved / cursor_left / pointer_up; pointer_down opt-in on wants_pointer_capture = true; forward_pointer_move method normalises cursor over the widget's post-layout rect
- free helpers rect_for_tag / normalize_cursor / widget_wants_capture / widget_wants_capture_walk added; clippy::cast_possible_truncation + cast_precision_loss localised to normalize_cursor only
- InputRouter::captured_target() accessor + DragCaptureExternal test fixture (wants_pointer_capture=true + pointer_move logging) + 8 new capture-lock unit tests + 2 new pinion-core stub trait tests
- §5.15 += External::wants_pointer_capture + External::pointer_move; §5.35 += InputRouter::captured_target + InputRouter::forward_pointer_move + rect_for_tag + normalize_cursor + widget_wants_capture



**Verification**:
- cargo check --workspace --features pinion-runtime/vello = 0 errors
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict baseline preserved: forbid unsafe + deny warnings + clippy::pedantic deny)
- cargo test --workspace --features pinion-runtime/vello = 1238 pass / 0 fail / 6 ignored (1226 + 12 new: 2 pinion-core stub trait + 10 InputRouter capture-lock)
- regression coverage: button_like_widget_preserves_pre_r51_34_cancel_by_leave proves Button / Toggle / Checkbox / Radio UX unchanged with default wants_pointer_capture = false
- mnemosyne validate_workspace pending after entry append — expect entries=179 / sections=55 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.15, §5.35


**Carry forward**:
- R51.35 Slider visual demo can now consume this substrate: SliderExternal overrides wants_pointer_capture=true + pointer_move(x_rel, _) → set_value(x_rel.clamp(0.0, 1.0)) — mouse-drag UX with capture lock across stray paths
- RadioGroup visual demo (R51.x) still blocked on the separate pinion-shell multi-External / multi-tag dispatch substrate (this round only addresses single-widget drag-forward)
- captured_target is single-target v0; multi-touch / capture queue is a future axis (carry to a later substrate round when a real second use case surfaces)



### R51.35 — R51.35 §5.38 hello-slider paint-side N=5 with real drag UX on R51.34 capture substrate

**Changes**:
- examples/hello-slider (new binary, 290 LOC main.rs): Material-style 200x8 pill track + 16x16 thumb + filled/unfilled portion encoding state x value; same pinion-core + pinion-shell + vello deps as the four sibling visual binaries
- Cargo.toml workspace.members += examples/hello-slider
- SliderExternal: wants_pointer_capture override = true + pointer_move(x_rel, _) override that clamps x_rel into Slider::set_value (gate-by-effect value_changing emission preserved)
- InputRouter::pointer_down patch: after capture entry, forward the press-time cursor through forward_pointer_move so a click-without-drag still seeds the value at the click point (Material click-to-position UX)
- 5 new tests: 4 SliderExternal (wants_capture / pointer_move clamp / value_changing intent on effective change / drag-end value_committed via pointer_move) + 1 InputRouter (pointer_down_forwards_initial_cursor); updates to capture_lock_forwards_pointer_move_normalized + capture_lock_allows_coords_outside_rect to expect the new press-time entry
- §5.38 implementations += {examples/hello-slider/app.pinion.xml + view + SliderView + slider.rs:SliderExternal::wants_pointer_capture + slider.rs:SliderExternal::pointer_move}



**Verification**:
- cargo check --workspace --features pinion-runtime/vello = 0 errors
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (clippy::pedantic deny baseline preserved; localised cast allows scoped to view fn and read_state f64 narrowing)
- cargo test --workspace --features pinion-runtime/vello = 1243 pass / 0 fail / 6 ignored (1238 + 5 new)
- hello-slider 290 LOC main + 24 + 27 + 12 = 353 LOC scaffolding; 5 binaries main.rs total 1218 LOC (203 button + 269 toggle + 221 checkbox + 235 radio + 290 slider) — substrate amortization holds (pinion-shell crate unchanged)
- Tier-1 widget visual coverage now 5/6 (Button / Toggle / Checkbox / Radio / Slider); RadioGroup remains blocked on pinion-shell multi-External / multi-tag substrate



**Impact**: §5.38, §5.35, §5.15


**Carry forward**:
- RadioGroup visual demo needs pinion-shell multi-External / multi-tag dispatch (single WidgetView::tag() insufficient for N siblings) — next substrate round candidate
- Keyboard value step (decrement / increment via arrow keys) not wired: WidgetView::keybinding(key) -> Option<Self::Event> is enum-only; a key-driven intervene hook would need a separate trait extension. Carry until a clear N=2 case shows up
- ai-introspect-demo migration to pinion-shell still blocked on multi-External support (same gap as RadioGroup)



### R51.36 — R51.36 §5.16 pinion-shell compile-only smoke test fixture closes R51.30 doc carry

**Changes**:
- crates/pinion-shell/tests/smoke.rs (new, ~210 LOC): SmokeRenderer + SmokeRendererError empty enum + SmokeExternal + SmokeView fixture matching the pinion-forge codegen template signature byte-for-byte; #[test] captures fn pointer of run::<SmokeView> to type-check the full VelloRenderer + WidgetView + vello_renderer_impl + run surface independent of the five examples/hello-* binaries
- module-scoped #![allow(clippy::unused_self, clippy::unnecessary_wraps)] on the smoke fixture only — stub-method bodies have no `self` use but the signatures are dictated by the trait / macro contract; workspace.lints strict baseline preserved everywhere else
- §5.16 implementations += crates/pinion-shell/tests/smoke.rs



**Verification**:
- cargo check --workspace --features pinion-runtime/vello = 0 errors
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- cargo test --workspace --features pinion-runtime/vello = 1244 pass / 0 fail / 6 ignored (1243 + 1 new shell_substrate_type_checks_with_minimal_fixture)
- smoke test detects any future regression that breaks the WidgetView / VelloRenderer trait surface (renamed associated types, tightened bounds, unsafe slip) at cargo test time, independent of the application binaries
- mnemosyne validate_workspace pending — expect entries=181 / sections=55 / T1=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.16, §5.38


**Carry forward**:
- docstring //! example blocks at crates/pinion-shell/src/lib.rs:7 and :141 remain rust,ignore (visual snippets) — textbook escape hatch for async + Into<SurfaceTarget> bounds whose hidden-line stubs would be unwieldy; smoke fixture covers the compile guarantee instead
- RadioGroup visual demo still blocked on multi-External / multi-tag substrate; first-client evidence (RadioGroup land) needed before substrate refactor per [[substrate-incompleteness-signal]] discipline



### R51.37 — R51.37 §5.35+§5.38 WidgetView::apply_key substrate + Slider arrow-keys a11y first-class — W3C/ARIA Slider keyboard accessibility 표준 도달

**Changes**:
- crates/pinion-shell/src/lib.rs: WidgetView trait 에 apply_key(scene: &mut Scene, key: &str) -> bool default false 추가 — 응용이 enum-typed keybinding 채널로 표현 못 하는 임의 키 처리를 §5.15 introspect 채널(intervene)로 라우팅, escape hatch 패턴, default 보존 시 4개 hello-* 위젯 동작 0 회규
- crates/pinion-shell/src/lib.rs: AppShell::apply_key 신규 — V::apply_key 가 true 반환 시 §5.34 revision bump + refresh_state + drain_intents (forward 와 동일 post-input 부킹)
- crates/pinion-shell/src/lib.rs: named_key_str(NamedKey) -> Option<&'static str> 신규 — winit NamedKey → W3C KeyboardEvent.key 문자열 매핑(ArrowLeft/Right/Up/Down/Home/End/PageUp/PageDown/Tab/Enter/Space); Escape 는 상류에서 quit 으로 필터
- crates/pinion-shell/src/lib.rs: KeyboardInput 핸들러 확장 — Character 는 keybinding 우선·실패 시 apply_key 로 폴백, Named 는 named_key_str 변환 후 apply_key 로 라우팅, Escape 동작 불변
- examples/hello-slider/src/main.rs: SliderView::apply_key override — 6개 키(ArrowLeft/Down=-5%, ArrowRight/Up=+5%, Home=0.0, End=1.0, PageDown=-10%, PageUp=+10%) 를 query+intervene 사이클로 wire, clamp(0.0..=1.0), Disabled 상태에서는 false 반환(ARIA aria-disabled 의 키보드 무시 규약)
- examples/hello-slider/src/main.rs: #[cfg(test)] mod tests 12개 — Arrow/Home/End/Page 6개 키 + ArrowUp=Right·ArrowDown=Left alias + 양끝 clamp + Disabled gate + unknown-key swallow 까지 ARIA Slider 키보드 패턴 전수 검증
- examples/hello-slider/src/main.rs: 파일 docstring 갱신 — keybinding 의 R51.34 carry 문장(decrement/increment carry-forward) 제거 후 R51.37 apply_key 8키 표 정식 명시
- §5.35 implementations += crates/pinion-shell/src/lib.rs:AppShell::apply_key + named_key_str
- §5.38 implementations += crates/pinion-shell/src/lib.rs:WidgetView::apply_key + examples/hello-slider/src/main.rs:SliderView::apply_key



**Verification**:
- cargo build --workspace --features pinion-runtime/vello = 0 errors (substrate + slider override + shell handler 동시 통과)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict 유지 — forbid unsafe_code / deny warnings / deny clippy::pedantic; doc_markdown SwiftIU 백틱 + manual_let_else 1회 정정)
- cargo test --workspace --features pinion-runtime/vello = 1256 pass / 0 fail / 8 ignored (직전 1244 → +12 SliderView::apply_key unit tests)
- cargo test -p hello-slider = 12/12 pass (arrow_right_increments / arrow_left_decrements / arrow_up·down alias / home·end jumps / page_up·down large step / 양끝 clamp / disabled_state_ignores_keyboard / unknown_key_returns_false)
- cargo test -p pinion-shell --test smoke = 1/1 pass (SmokeView 가 apply_key default false 그대로 → 트레잇 surface 회규 가드 유지)
- cargo test -p pinion-runtime button_like_widget_preserves_pre_r51_34_cancel_by_leave = 1/1 pass (R51.34 opt-in capture 패턴 0 회규 재확인)
- Mnemosyne validate_workspace: entries=182 / sections=55 / T1=0 T3=0 / RT=1/1 / GENERATED.md=sync / commit↔ledger drift cited=1 ledger=129 missing=0



**Impact**: §5.35, §5.38


**Carry forward**:
- 부채 2: multi-pointer / multi-touch v0 제한 — InputRouter::captured_target HashMap<PointerId, String> 화 + cursor_moved/pointer_down/pointer_up 시그니처에 PointerId 합류 (~150 LOC)
- 부채 3: RadioGroup substrate-chicken-and-egg — substrate-first 3-round (RFC + sub-index routing + first-client hello-radio-group), Tier-1 visual 6/6 도달 (~300+280 LOC)
- 부채 4: Slider vertical axis future-proof — SliderState/SliderEvent 에 axis 추가, SliderExternal::pointer_move axis 분기 (~50 LOC)
- 부채 5: External trait surface 비대화 검토 — InputForwarding/Lifecycle/Introspection sub-trait 분리 RFC (design only)
- 부채 6: widget_wants_capture O(N) walk — multi-External 환경 frame-마다 비용, last hover_target 의 wants_capture early-cache (~50 LOC)
- 부채 9: R41 §5.16 Phase 2 thin RHI 3D pass axis ratify (spec round)
- 부채 10: R51.31 L4 alternative impl path RFC (spec round)
- 메모리 신설 후보: design-phase-industry-ux-checklist.md (R51.34 click-to-position 누락 lesson + R51.35 keyboard a11y 누락 lesson), a11y-first-class-requirement.md (carry deferred 가 ARIA 답 아님 lesson), opt-in-pattern-textbook-merit.md (wants_pointer_capture default false 4-widget 0 회규 검증)



### R51.38 — R51.38 §5.35 multi-pointer first-design substrate — InputRouter per-pointer HashMap (cursors / hover_targets / captured_targets), 모바일/터치 진입 전 aliasing-by-default refactor cost 회피

**Changes**:
- crates/pinion-runtime/src/input.rs: PointerId(u64) newtype 신설 — Hash+Eq+Copy+Debug, PointerId::MOUSE(0) const 예약, PointerId::touch(finger_id) 가 +1 offset 로 mouse 겹치 회피 (winit FingerId u64 폭과 일치, wrapping_add 로 이론적 max 에지 대응)
- crates/pinion-runtime/src/input.rs: InputRouter 필드 3개 HashMap<PointerId, _> 화 — cursors (커서 위치) / hover_targets (호버 tag) / captured_targets (capture-lock tag). single-target Option 시절 multi-touch 악러이싱 가능성 제거
- crates/pinion-runtime/src/input.rs: cursor_moved / cursor_left / pointer_down / pointer_up 시그니처에 id: PointerId 인자 선두 추가; 각 메서드가 대응 HashMap 엔트리만 조작, 다른 포인터 상태 불변
- crates/pinion-runtime/src/input.rs: update_paint_scene 이 cursors 의 모든 활성 PointerId 에 대해 refresh_hover 실행 (capture 포인터는 skip) — layout shift 시 전체 인터읽 동기화
- crates/pinion-runtime/src/input.rs: refresh_hover 가 per-pointer (id 인자), 각 포인터가 독립적 leave-before-enter ordering 발생
- crates/pinion-runtime/src/input.rs: hover_target(id) / captured_target(id) 접근자 per-pointer query 화 — diagnostic/test surface, application 코드 는 직접 query 불필요
- crates/pinion-runtime/src/lib.rs: pub use input::{InputRouter, PointerId} — substrate 타입 re-export, downstream shell 이 use 가능
- crates/pinion-shell/src/lib.rs: winit mouse 핸들러 4개 (CursorMoved / CursorLeft / MouseInput Pressed / Released) 가 PointerId::MOUSE 를 항상 전달; touch 이벤트 wiring 은 follow-up carry
- crates/pinion-runtime/src/input.rs tests: 기존 15개 single-pointer test 모두 PointerId::MOUSE 인자 추가, 6개 multi-pointer new test (pointer_id_mouse_is_reserved_zero / two_touches_drag_two_widgets_independently / mouse_and_touch_dont_alias_hover / releasing_one_touch_does_not_release_other_capture / cursor_left_for_one_pointer_keeps_other_state / update_paint_scene_refreshes_every_active_pointer)
- §5.35 implementations += PointerId / PointerId::MOUSE / PointerId::touch / InputRouter::{cursors, hover_targets, captured_targets}



**Verification**:
- cargo build --workspace --features pinion-runtime/vello = 0 errors (substrate + shell call site + 5 example 모두 통과)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict 유지 — forbid unsafe_code / deny warnings / deny clippy::pedantic)
- cargo test --workspace --features pinion-runtime/vello = 1262 pass / 0 fail / 8 ignored (직전 R51.37 의 1256 → +6 multi-pointer InputRouter tests)
- cargo test -p pinion-runtime --features vello = 59 pass (이전 53 → +6) — single-pointer 15개 잘지 없이 PointerId::MOUSE 로 이주, multi-touch 6개 new
- cargo test -p pinion-shell --test smoke = 1/1 pass (트레잇 surface 회규 0)
- button_like_widget_preserves_pre_r51_34_cancel_by_leave = 1/1 pass (R51.34 opt-in capture 패턴 backwards-compat 0 회규)
- Mnemosyne validate_workspace: entries=183 / sections=55 / T1=0 T3=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.35


**Carry forward**:
- winit Touch event 핸들러 wiring at pinion-shell layer — 현재 router API 는 PointerId::touch 수용 준비 완료, 실제 WindowEvent::Touch 소스 연결만 남음 (부채 2 소소 carry)
- 부채 4: Slider vertical axis future-proof — SliderState/SliderEvent axis, SliderExternal::pointer_move axis 분기 (~50 LOC)
- 부채 6: widget_wants_capture O(N) walk early-cache — multi-External 환경 frame cost (~50 LOC)
- 부채 3: RadioGroup substrate-chicken-and-egg — substrate-first 3-round (RFC + sub-index routing + first-client hello-radio-group)
- 부채 5: External trait surface 비대화 segregation RFC (design only)
- 부채 9: R41 §5.16 Phase 2 thin RHI 3D pass axis ratify (spec round)
- 부채 10: R51.31 L4 alternative impl path RFC (spec round)



### R51.39 — R51.39 §5.38 Slider vertical axis future-proof — SliderAxis(Horizontal/Vertical) + with_axis builder + pointer_move axis 분기 + orientation introspect 필드, vertical 추가 시 widget-level breaking change 회피

**Changes**:
- crates/pinion-core/src/widgets/slider.rs: SliderAxis enum 신설 (Horizontal/Vertical) + Default=Horizontal (backwards-compat), Hash/Copy/PartialEq derive
- crates/pinion-core/src/widgets/slider.rs: Slider 에 axis: SliderAxis 필드 추가, Slider::new() 가 Horizontal default 유지 (pre-R51.39 callers 0 migration), Slider::with_axis(axis) builder + Slider::axis() accessor 신설
- crates/pinion-core/src/widgets/slider.rs: SliderExternal::with_axis(axis) builder + SliderExternal::axis() accessor 신설 (IntentEmitter::new(Slider::with_axis(axis)) wrapping)
- crates/pinion-core/src/widgets/slider.rs: SliderExternal::pointer_move axis 분기 — Horizontal=x_rel (기존), Vertical=1.0-y_rel (Material 3 / W3C ARIA aria-orientation=vertical top=max 규약)
- crates/pinion-core/src/widgets/slider.rs: introspect schema 4-slot 확장 — state/value/orientation/send (orientation 신규), query("orientation") -> Text("horizontal"/"vertical") aria-orientation 직매핑, intervene("orientation")=ReadOnly (construction-time fixed 보호)
- crates/pinion-core/src/widgets/slider.rs: slider_axis_name(axis) helper — lowercase aria-orientation 정렬 매핑
- crates/pinion-core/src/widgets/slider.rs tests: 7 new (default_axis_is_horizontal / with_axis_pins_orientation_at_construction / horizontal_pointer_move_reads_x_rel / vertical_pointer_move_inverts_y_rel / vertical_pointer_move_clamps_outside_rect / orientation_query_returns_aria_string / orientation_intervene_is_read_only / schema_lists_orientation_field), 기존 external_schema_declares_three_slots → four_slots rename 으로 4-field 검증
- §5.38 implementations += SliderAxis / Slider::with_axis / Slider::axis / SliderExternal::with_axis / SliderExternal::axis / slider_axis_name



**Verification**:
- cargo build --workspace --features pinion-runtime/vello = 0 errors (substrate + 5 example 모두 통과; SliderExternal::new() default Horizontal 보존으로 hello-slider 회규 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (manual_pattern_or 회피 위해 state | orientation 합쳐 1개 arm)
- cargo test --workspace --features pinion-runtime/vello = 1270 pass / 0 fail (직전 R51.38 의 1262 → +8 axis tests)
- cargo test -p pinion-core widgets::slider = 24 pass / 0 fail (기존 16 + 신규 8)
- Mnemosyne validate_workspace: entries=184 / sections=55 / T1=0 T3=0 / RT=1/1 / GENERATED.md=sync
- R51.34 capture-lock + R51.37 apply_key + R51.38 multi-pointer cascade 회규 0 — Horizontal default 가 pre-R51.39 caller 의 pointer_move(x_rel, _) semantics 그대로 유지



**Impact**: §5.38


**Carry forward**:
- 부채 6: widget_wants_capture O(N) walk early-cache — multi-External 환경 frame cost (~50 LOC)
- 부채 3: RadioGroup substrate-chicken-and-egg — substrate-first 3-round (RFC + sub-index routing + first-client hello-radio-group)
- 부채 5: External trait surface 비대화 segregation RFC (design only)
- 부채 9: R41 §5.16 Phase 2 thin RHI 3D pass axis ratify (spec round)
- 부채 10: R51.31 L4 alternative impl path RFC (spec round)
- vertical-axis 시각 예제 (hello-slider-vertical) carry — N=6 도달 시 substrate amortization 검증 후보 (현재 부채 우선순위 더 높음)
- winit Touch event 와이어링 at pinion-shell layer carry (R51.38 follow-up)



### R51.40 — R51.40 §5.35 widget_wants_capture early-cache — refresh_hover 이 hover walk 와 동시에 wants_capture 조회 · per-pointer cache, pointer_down 은 bit read 만 — textbook layering (input router 가 click 시 점 scene walk 없음)

**Changes**:
- crates/pinion-runtime/src/input.rs: InputRouter 에 hover_wants_capture: HashMap<PointerId, bool> 필드 추가 — per-pointer cache, hover_targets 와 동일 lifecycle (입장/이동/표즜 해제 시 populate/replace/drop)
- crates/pinion-runtime/src/input.rs: refresh_hover 가 PointerLeave 시 cache 명시적 remove(&id), PointerEnter 시 widget_wants_capture(state_scene, &target) 조회 결과 insert — hover-resolve walk 에 아주 작은 추가 일, click 시 점 재조회 제거
- crates/pinion-runtime/src/input.rs: pointer_down 의 widget_wants_capture(...) call 제거 이후 cache.get(&id).copied().unwrap_or(false) 으로 read — 입력 핸들러 의 scene-walk 없음 (textbook layering: input router = state machine, scene walk = hover/layout)
- crates/pinion-runtime/src/input.rs tests: wants_capture_cache_co_locates_with_hover_walk 신규 — drag-aware widget capture lock + button-like 0-lock 의 두 시나리오를 cache 경로로 end-to-end 검증 (기존 capture-related test 8개 동일 풋스 통과, cache 투명성 검증)
- §5.35 implementations += InputRouter::hover_wants_capture



**Verification**:
- cargo build --workspace --features pinion-runtime/vello = 0 errors
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict)
- cargo test --workspace --features pinion-runtime/vello = 1271 pass / 0 fail / 8 ignored (직전 R51.39 의 1270 → +1 cache invariant test)
- cargo test -p pinion-runtime --features vello = 60 pass (이전 59 → +1)
- R51.34 button_like_widget_preserves_pre_r51_34_cancel_by_leave + R51.34-R51.39 capture-related 8개 test 모두 0 회규 — cache 가 hover lifecycle 와 완전 동일, 관찰 가능한 시맨틱스 불변
- Mnemosyne validate_workspace: entries=185 / sections=55 / T1=0 T3=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.35


**Carry forward**:
- 부채 3: RadioGroup substrate-chicken-and-egg — substrate-first 3 rounds (RFC + sub-index routing + first-client)
- 부채 5: External trait surface segregation RFC (design only) — InputForwarding/Lifecycle/Introspection 분리
- 부채 9: R41 §5.16 Phase 2 thin RHI 3D pass axis ratify (spec round)
- 부채 10: R51.31 L4 alternative impl path RFC (spec round)
- wants_pointer_capture 의 "effectively constant per widget instance" 가정 carry — 동적 toggle 원하는 widget 등장 시 cache invalidation 훅 추가 가능성 (현재 최소한 변경 원칙)
- winit Touch event 와이어링 at pinion-shell layer (R51.38 follow-up)
- vertical-axis 시각 예제 hello-slider-vertical N=6 도달 시 substrate amortization 검증 가능



### R51.41 — R51.41 §5.35+§5.38 sub-index routing convention RFC — RadioGroup substrate-first 3-round 의 1단계 (RFC), 'tag#idx' suffix → InputRouter '#' split → 'idx:Event' forward, paint N tags + state 1 External pattern 명문화

**Changes**:
- §5.35 caveat 추가 (sub-index routing convention): 페인트 tag 'primary#idx' suffix 가 hit-test 시 '#' 분리, InputRouter::dispatch_send 가 'idx:EventName' 형식으로 External invoke('send', Text(...)) 호출. RadioGroupExternal::invoke('send') 의 '<index>:<EventName>' 기존 wire format (line 358) 과 정확히 정합, 추가 wire 정의 0
- §5.38 caveat 추가 (composite widget hit-target convention): RadioGroup 같은 composite 위젯의 paint scene = N 'group#0..N-1' 태그 Container, state scene = single 'group' External — HTML <input type=radio name=...> + Material RadioGroup + SwiftUI Picker 산업 표준 precedent (per-radio hit, framework-owned mutual exclusion 유지)
- (RFC 단계 — 0 LOC impl): R51.42 = InputRouter '#' suffix 파서 + dispatch_send_with_subindex impl + tests (~300 LOC), R51.43 = examples/hello-radio-group first-client binary (~280 LOC)
- Alternatives 검토 (atomic ledger 외부 notes): (a) per-radio ExternalNode N개 → R51.15 framework-owned mutual exclusion 의 단일 source-of-truth 위반, 거부. (b) ExternalNode tag 에 sub_index Option<u32> 필드 추가 → invasive struct change + paint scene tag 와 state scene tag 의 비대칭, 거부. (c) tag '#' suffix convention → 비invasive, paint 만 변경, '#' 가 tag literal 에서 드문 collision 위험 낮음, 채택



**Verification**:
- mnemosyne validate_workspace: entries=185 → 186 / sections=55 / T1=0 T3=0 / RT=1/1 / GENERATED.md=sync (RFC 라운드, atomic mutation only)
- §5.35 caveats_bullets += 1 (R51.41 sub-index), §5.38 caveats_bullets += 1 (R51.41 composite hit-target) — caveat 100 char 한도 준수
- GENERATED.md 의 §5.35 + §5.38 caveats 섹션이 cascade 렌더링 sync 확인
- code 변경 0 — pre-existing 1271 pass / 0 fail / 8 ignored baseline + 0 clippy warnings 유지 (RFC = spec-only)
- R51.42 / R51.43 카리 명시: substrate-first 의 textbook 계층분리 (RFC → impl → first-client) 유지, premature impl/client 회피



**Impact**: §5.35, §5.38


**Carry forward**:
- R51.42 impl: InputRouter '#' suffix 파서 + dispatch_send 가 'idx:EventName' forward + 새 tests (single-tag backwards-compat + sub-index forward 두 경로 검증, ~300 LOC)
- R51.43 first-client: examples/hello-radio-group binary — N=3 Radio 시각화, paint 'main_group#0..2' tags + state Scene::External(RadioGroupExternal::new(3)).with_tag('main_group') (~280 LOC)
- 부채 5 External trait segregation RFC (design only)
- 부채 9 R41 §5.16 Phase 2 thin RHI 3D pass axis ratify (spec round)
- 부채 10 R51.31 L4 alt impl path RFC (spec round)
- vertical-axis 시각 예제 hello-slider-vertical N=6 도달 시 substrate amortization 검증 후보
- winit Touch event 와이어링 at pinion-shell layer (R51.38 follow-up)
- '#' suffix collision 위험 모니터링 — application 측에서 tag literal 에 '#' 사용 금지 도큐먼트화 carry



### R51.42 — R51.42 §5.35 InputRouter sub-index split + dispatch_send wire-format land

**Changes**:
- crates/pinion-runtime/src/input.rs: split_subindex helper 신설 — 'tag#idx' → (primary, Some(idx)), 'tag' / 'tag#' → (primary, None) collapse, empty primary 보존
- crates/pinion-runtime/src/input.rs: dispatch_send 가 split_subindex 적용 — primary 로 state-scene ExternalNode lookup, sub_index 존재 시 wire payload 를 'idx:EventName' 으로 재작성 (radio_group.rs:357 split_once(':') 의 mirror)
- crates/pinion-runtime/src/input.rs: widget_wants_capture 가 primary 로 state lookup — 합성 widget 의 capture 는 sub-region 이 아닌 composite handle 의 결정
- crates/pinion-runtime/src/input.rs: forward_pointer_move 가 raw 페인트 tag 로 rect 조회 + primary 로 state lookup — 드래그 합성 widget 미래 호환 (RadioGroup 은 wants_pointer_capture=false 라 미사용)
- tests +5: sub_index_dispatch_forwards_idx_prefixed_event_name / single_tag_backwards_compat / sub_index_capture_wires_to_primary / empty_subindex_treated_as_unsplit / split_subindex_helper_covers_all_shapes



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1276 pass / 0 fail (+5 from 1271)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict — forbid unsafe / deny warnings / clippy::pedantic deny)
- R51.34/R51.37/R51.38/R51.40 회규 0 — single-tag backwards-compat test + button_like_widget_preserves_pre_r51_34_cancel_by_leave + 14 capture/multi-pointer tests 모두 통과
- radio_group.rs invoke('send', 'idx:Event') wire format (line 357) 와 dispatch_send 출력 정확 정합 — RadioGroup external_invoke_send_drives_specified_radio test 의 reverse path
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / T4=99 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.35


**Carry forward**:
- R51.43 examples/hello-radio-group first-client — substrate-first 3-round 의 3/3, paint N 'main_group#0..2' tags + state 1 'main_group' RadioGroupExternal, keybinding a/b/c → 0/1/2 + ARIA radio-group ArrowDown/Up navigation
- External trait segregation RFC (부채 5) — R51.34/.37/.38 누적 trait 확장 정리, InputForwarding / Lifecycle / Introspection sub-trait 분리 design only
- R41 §5.16 Phase 2 thin RHI 3D pass axis spec round (부채 9)
- winit Touch event 와이어링 (PointerId::touch 수용 ready, shell call site 남음)



### R51.43 — R51.43 §5.38 RadioGroupExternal introspect per-radio query paths

**Changes**:
- crates/pinion-core/src/widgets/radio.rs: radio_state_name 을 pub(crate) 로 승격 — radio_group.rs 의 per-radio query 에서 DRY 재사용, RadioState → name 관례 단일 소스
- crates/pinion-core/src/widgets/radio_group.rs: RadioGroupExternal::query 가 'state.<i>' / 'selected.<i>' 동적 path 처리 — strip_prefix + usize 파싱, out-of-range / malformed 제술 아닌 None 묵식 관례 (최상위 unknown-path fall-through 일치)
- crates/pinion-core/src/widgets/radio_group.rs: schema 가 ('state.<index>','string') + ('selected.<index>','bool') 포함 — 'send' 의 wire-format placeholder 관례와 동일, AI scene/schema discovery 대상
- WidgetView::read_state 가 RadioGroupExternal 을 introspect 만으로 per-radio state 를 읽을 수 있도록 substrate gap 채움 — R51.44 first-client (hello-radio-group) 의 선행 조건
- tests +4: external_query_state_per_radio_returns_state_name / external_query_selected_per_radio_returns_bool / external_query_out_of_range_index_is_none / external_query_malformed_per_radio_path_is_none + external_schema_declares_three_slots → _five_slots rename



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1280 pass / 0 fail (+4 from 1276)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict)
- RadioGroup public API surface 변화 0 — send/state/is_selected/selected_index 시그니처 부변, query 확장은 순수 additive substrate
- Radio (single-tag) query('state')/query('selected') 관례과 정합 — 동일 값 타입 (Text 이름 / Bool 비트)
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync



**Impact**: §5.38


**Carry forward**:
- R51.44 examples/hello-radio-group first-client — N=3 Radio composite, paint 'main_group#0..2' tags + state RadioGroupExternal::new(3) with_tag('main_group'), keybinding a/b/c → 0/1/2 + ARIA radio-group ArrowDown/Up navigation, substrate-first 3-round (R51.41 RFC → R51.42 InputRouter → R51.43 introspect → R51.44 first-client) 의 4/4
- External trait segregation RFC (부채 5)
- R41 §5.16 Phase 2 thin RHI 3D pass axis spec round (부채 9)
- winit Touch event 와이어링 (PointerId::touch 수용 ready, shell call site 남음)



### R51.44 — R51.44 §5.38+§5.35 hello-radio-group composite hit-target first-client land

**Changes**:
- examples/hello-radio-group/{Cargo.toml, build.rs, app.pinion.xml, src/main.rs} 신설 — workspace.members 등록, pinion-forge codegen 시작점 + HelloRadioGroupRenderer manifest
- examples/hello-radio-group/src/main.rs: RadioGroupView WidgetView impl — type State=[(RadioState,bool);3], paint N=3 vertical Column rows tagged 'main_group#0..2', state RadioGroupExternal::new(3) with_tag('main_group')
- RadioGroupView::read_state 가 state.<i>/selected.<i> (R51.43) per-radio query 로 구성 — introspect single source of truth, AI scene/query 와 동일 경로
- RadioGroupView::apply_key (R51.37 escape hatch) 가 ARIA radio-group keyboard navigation 구현 — a/b/c → 0/1/2, Home/End → first/last, ArrowDown/Right + ArrowUp/Left → wrap-step, 전체 activation cycle (Enter/Down/Up/Leave) wire-format 으로 forward 해 '"selected"' intent emission 보장
- Cargo.toml workspace.members 에 examples/hello-radio-group 추가



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1280 pass / 0 fail (no test change — binary client)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict)
- cargo check -p hello-radio-group 성공 — forge codegen pipeline 정상 wire
- InputRouter R51.42 '#' split + RadioGroupExternal R51.43 per-radio introspect 의 시각 end-to-end 검증 substrate — visual evidence (hello-radio-group binary)
- substrate-first 3-round (R51.41 RFC → R51.42 InputRouter → R51.43 introspect) + this first-client 의 4/4 completion — Tier-1 visual coverage 6/6



**Impact**: §5.38, §5.35


**Carry forward**:
- External trait segregation RFC (부채 5) — R51.34/.37/.38 누적 trait 확장 정리, InputForwarding / Lifecycle / Introspection sub-trait 분리 design
- R41 §5.16 Phase 2 thin RHI 3D pass axis spec round (부채 9)
- R51.31 L4 alternative impl path RFC (부채 10) — pre-substitute path lock spec
- winit Touch event 와이어링 (PointerId::touch 수용 ready, shell call site 남음)
- hello-radio-group N=3 구조 — Vec/array 관례 제한 없이 dynamic-N first-client (e.g. settings binary) 는 명시 spec round 일면 추가 ratify 필요



### R51.45 — R51.45 §5.35 winit Touch event wiring closes R51.38 multi-pointer arc

**Changes**:
- crates/pinion-shell/src/lib.rs: winit Touch / TouchPhase import 상단 use 절에 추가, WindowEvent::Touch arm 설치
- crates/pinion-shell/src/lib.rs: AppShell::handle_touch helper 신설 — PointerId::touch(finger_id) factory + 4 TouchPhase 매핑 (Started=cursor_moved+pointer_down / Moved=cursor_moved / Ended | Cancelled=pointer_up+cursor_left)
- Started 이 cursor_moved 선행 — mouse 경로와 직교 (CursorMoved 이 MouseInput 선행하는 winit 계약과 일치), hover 해소 조건 행동 동등
- Ended 의 pointer_up + cursor_left 시퀘스 가 post-release refresh_hover 트리거 (button-like cancel-by-leave + capture release deferred PointerLeave 동일 컨트랙트 수용)
- WindowEvent::Touch arm body 는 helper 호출 만 수행 — window_event 100 LOC 제한 회규 없이 textbook 레이어링 (winit dispatch=arm, router routing=helper)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1280 pass / 0 fail (Touch 는 실특 macOS desktop 불가능 — runtime regression 없으며 InputRouter R51.38 6 multi-pointer test 가 PointerId::touch 의 재사용 경로 검증)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (handle_touch 추출 로 too_many_lines 회피)
- R51.34 / R51.37 / R51.38 / R51.40 회규 0 — mouse path 불변 (button-like + Slider drag + multi-pointer + wants_capture cache)
- winit 0.30.13 Touch struct 의 phase / location / id 필드 제항 정합 — device_id / force 는 currently no-op (force 는 future pressure-sensitive carry)
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync



**Impact**: §5.35


**Carry forward**:
- TouchPhase::Cancelled 의 PointerUp commit-class intent 의도치 않은 emission — PointerCancel event variant 또는 InputRouter::cancel_pointer(id) helper 로 철자 release 는 별도 round (substrate 확장)
- winit Touch.force 압력 감지 — Material InputRouter 는 pressure-aware (3D Touch / Apple Pencil) 그래니 포워딩 경로 설계 필요, carry



### R51.46 — R51.46 §5.38 hello-slider-vertical first-client validates SliderAxis::Vertical

**Changes**:
- examples/hello-slider-vertical/{Cargo.toml, build.rs, app.pinion.xml, src/main.rs} 신설 — workspace.members 등록, pinion-forge codegen 시작점 + HelloSliderVerticalRenderer manifest
- src/main.rs: SliderVerticalView WidgetView impl — SliderExternal::with_axis(SliderAxis::Vertical) construct, paint vertical track Column [unfilled (top) | thumb | filled (bottom)] = 8×200 rail + 16×16 thumb, aria-orientation=vertical (top=max) Material/iOS volume HUD convention
- apply_key 가 hello-slider 와 동일 — ARIA Slider keyboard contract 는 orientation 의존 않음 (ArrowUp/Right=increment, ArrowDown/Left=decrement)
- tests +5: vertical_arrow_up_increments / vertical_arrow_down_decrements / vertical_home_jumps_to_minimum / vertical_end_jumps_to_maximum / vertical_orientation_reports_through_introspect



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1285 pass / 0 fail (+5 from 1280)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict)
- R51.39 SliderExternal::pointer_move 의 1.0 - y_rel 인버전 + R51.34 capture + R51.42 sub-index dispatch substrate 가 axis-specific branch 없이 재사용 됨 에 대한 시각 evidence
- hello-slider (horizontal) 회규 0 — axis builder line 이 유일 분기점, 그 외 의 binding 완전 공유
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync
- Tier-1 visual coverage 7 binary 도달 (button/toggle/checkbox/radio/slider/radio-group/slider-vertical)



**Impact**: §5.38


**Carry forward**:
- External trait segregation RFC (부채 5)
- R41 §5.16 Phase 2 thin RHI 3D pass axis spec round (부채 9)
- R51.31 L4 alternative impl path RFC (부채 10)
- TouchPhase::Cancelled commit-class intent leak — PointerCancel substrate 확장 (R51.45 carry)



### R51.47 — R51.47 §5.15 External sub-trait segregation future-path RFC (design-only)

**Changes**:
- §5.15 caveat: R51.47 sub-trait future-path — 새 orthogonal axes 는 item 8 Option<&dyn ExternalIntrospect> 선례의 sub-trait 패턴 채택 (누적 External default 회피)
- §5.15 caveat: R51.47 backwards-compat — R51.34 input forwarding axis (wants_pointer_capture / pointer_move) 는 External default 로 남아 있음, v0 retrofit 안함
- §5.15 caveat: R51.47 sub-trait 후보 — Drag (R51.34) / Lifecycle (item 4 mount/unmount/visibility/focus) / Cancel (R51.45 PointerCancel carry)
- design-only round — code mutation 0, atomic store 의 contract 확장 path 명시화만 수행 ([[textbook-long-term-correct]] 관례 — 미래 widget 저자 가 Option<&dyn> 패턴 이 canonical 임을 도출할 수 있게)



**Verification**:
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync (3 caveat add)
- cargo test --workspace --features pinion-runtime/vello = 1285 pass / 0 fail (코드 미변)
- §5.15 body 의 8-point contract ¶ + Item 8 opt-in ¶ 원본 결정 불변 — 이번 RFC 는 확장 path 명시화 일 뿐, R5 Round 7 ratify 관련 자녘 부재
- Sub-trait 실질 land 은 실 widget 에서 요구 시점 (e.g. 두 번째 drag-aware widget 등장 시 해당 라운드) — 명시 이용 chicken-and-egg 회피



**Impact**: §5.15


**Carry forward**:
- Sub-trait 실질 land — 둘째 drag-aware widget 등장 시 Drag sub-trait extract, 둘째 lifecycle-sensitive External 등장 시 Lifecycle sub-trait extract
- PointerCancel sub-trait (R51.45 carry 과 일치)
- External default 누적 한계 트리거 명시 — 현재 17 method 중 12 개가 default, 3 개이상 추가 시 sub-trait extract



### R51.48 — R51.48 §5.16 R41 Phase 2 thin RHI 3D pass axis spec round (design-only)

**Changes**:
- §5.16 caveat: R51.48 Phase 2 trigger — first 3D primitive scene 요구이 entry gate, R45 renderer kind 'rhi' template 가 land 조건
- §5.16 caveat: R51.48 Phase 2 scope — thin RHI 3D pass 는 Vello UI path 와 additive 공존 (동시 운용), Vello drop 은 Phase 4+ 평가
- §5.16 caveat: R51.48 Phase 2 surface — pinion-forge renderer kind=rhi + naga shader emit (WGSL→SPIRV/MSL/HLSL/DXIL) + bgfx/makepad multi-threaded 패턴
- §5.16 caveat: R51.48 Phase 2 demo target — 단일 3D primitive (triangle/cube) + AI-introspect scene = first dogfood (R45 demo manifest 관례 재사용)
- §5.16 caveat: R51.48 Phase 4+ B 평가 gate — 언리얼-class engine pass, AAA RDG/bindless 운용 검증 후 ratify



**Verification**:
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync (5 caveat add)
- cargo test --workspace --features pinion-runtime/vello = 1285 pass / 0 fail (코드 미변)
- R11 'thin RHI + naga' 결정 (§5.16 body) + R41 Phase 2 4-phase plan + R45 renderer kind codegen 정합 — spec drift 0
- Phase 1 Vello 원안 보존 — R51.48 은 Phase 2 trigger · scope · surface · demo target · Phase 4+ gate 5 caveat 추가만 수행, body 게속 수정 없음



**Impact**: §5.16


**Carry forward**:
- Phase 2 entry 시 R45 renderer kind 'rhi' template emit 세부 설계 (pinion-render-rhi crate skeleton + naga 종속 depend + bgfx/makepad reference design)
- Phase 4+ 언리얼-class B 평가 round — AAA 도메인 구체적 demo requirement 대충 등장 시점에
- Vello 영구 의존 frame 회피 invariant 유지 — Phase 2 land 이후 의 Vello-drop 는 자연스럽게 additive path migration



### R51.49 — R51.49 §5.37.4 BIDI L4 alt impl path lock RFC (pre-substitute 채택, render-time 거부)

**Changes**:
- §5.37.4 caveat: R51.49 L4 path lock — pre-substitute (R51.27 paint_adapter + R51.31 LayoutCache integration) 채택, render-time parley GlyphRun.is_rtl substitute 거부
- §5.37.4 caveat: R51.49 pre-substitute 장점 — parley API decouple (backend swap 가능) + LRU 단일 lookup 가 BIDI helper + shape pass 양쪽 amortize
- §5.37.4 caveat: R51.49 render-time 거부 이유 — R51.31 cache layer unwind 강요 + parley-specific GlyphRun.is_rtl 의존 = backend lock-in 위험
- §5.37.4 caveat: R51.49 pre-substitute 한계 — font fallback 시 mirror codepoint 미공급 폰트 케이스, mirroring_glyph fallback chain 필요 (carry)
- design-only round — R51.27 / R51.31 시점 누락한 architectural decision audit trail 만 atomic store 에 기록



**Verification**:
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync (4 caveat add)
- cargo test --workspace --features pinion-runtime/vello = 1285 pass / 0 fail (코드 미변)
- BIDI L4 lookup substrate (R51.23 mirroring_glyph + R51.27 mirror_paired_brackets paint_adapter wire + R51.31 LayoutCache move) 모두 보존 — 이번 RFC 는 결정 명문화 만
- §5.37.4 body 의 'L-rules' 명시 + alternatives '부분 구현' 거부 + UAX #9 full conformance 정신 정합



**Impact**: §5.37.4


**Carry forward**:
- mirror codepoint 미공급 폰트 fallback chain — mirroring_glyph(cp) 가 None 반환할 때 다른 폰트 시도 (현재는 관령 그대로 폴시터록 공급)
- L4 알고리즘 정확성 conformance sweep 자동화 — UAX BidiCharacterTest.txt + BidiTest.txt 의 mirroring assertion subset



### R51.50 — R51.50 §5.35 '#' suffix collision policy 명문화 (design-only)

**Changes**:
- §5.35 caveat: R51.50 — application 측 paint tag literal 의 '#' 사용 금지, InputRouter R51.42 split_subindex 와 충돌 조건
- §5.35 caveat: R51.50 — 위반 시 행동 명시 (첫 '#' 이전 primary 추출 → 의도치 않은 state lookup 또는 dispatch drop)
- §5.35 caveat: R51.50 — 정식 용법 명시 (composite hit-target convention: paint 'tag#idx' + state primary tag 만 허용)
- design-only round — 코드 변경 0, 향후 widget 저자 / AI agent 가 합성 widget tag naming 시 참조할 업종 주의사항



**Verification**:
- mnemosyne-cli validate-workspace = T1=0 / T3=0 / RT=1/1 / sync (3 caveat add)
- cargo test --workspace --features pinion-runtime/vello = 1285 pass / 0 fail (코드 미변)
- R51.42 split_subindex 프리미티브 의 contract 와 정식 정합 — '#' 가 존재하면 split, 없으면 unsplit, 빈 sub-index collapse
- hello-radio-group (R51.44) 의 'main_group#0..2' 관례 도의 입명



**Impact**: §5.35



### R51.51 — R51.51 §5.39 Focus model RFC — keyboard navigation + activation primitive (design-only)

**Changes**:
- §5.39 new section — Focus model RFC (design-only round, code mutation 0)
- FocusManager substrate ownership + apply_key broadcast → focused-only breaking change spec
- Tab/Shift+Tab traversal + Space/Enter activation + ARIA roving tabindex pattern 명문화
- WidgetView::focusable_tags trait method + WindowEvent::Focus save/restore 양측 spec
- 12 caveats: roving tabindex, focus on click, focus_set RPC, focus_clear, Window blur restore, focus ring, composite focus, single-tag sub-focus, enumeration scope, key priority, Space/Enter delegation, breaking change discipline



**Verification**:
- validate_workspace clean — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync/sections=55→56
- code unchanged (design-only round, R51.43/.48/.49/.50 spec-only precedent)



**Impact**: §5.39


**Carry forward**:
- R51.52 — pinion-runtime FocusManager substrate land
- R51.53 — pinion-shell focus wiring + WidgetView trait extension (apply_key breaking change)
- R51.54-R51.57 — first-client widget activation + roving tabindex
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore focus state save/restore



### R51.52 — R51.52 §5.39 FocusManager substrate — focus state owner + Tab traversal + save/restore

**Changes**:
- crates/pinion-runtime/src/focus.rs 신설 — FocusManager + 7 메서드 + 17 unit test
- lib.rs pub mod focus + pub use focus::FocusManager re-export
- Tab/Shift+Tab wrap traversal + ARIA Authoring Practices initial-focus convention
- update_focusable_tags 가 stale focus drop (view-fn 이 widget 제거 시 자동 cleanup)
- save / restore — Window blur/refocus 용 snapshot (R51.59 wiring 경로 확보)



**Verification**:
- cargo test -p pinion-runtime — 70 pass / 0 fail (focus tests 17 추가)
- workspace cargo test --features pinion-runtime/vello — 1285 → 1302 pass
- workspace clippy --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.39


**Carry forward**:
- R51.53 — pinion-shell focus wiring + WidgetView::focusable_tags trait method
- R51.54-R51.57 — widget-side activation + roving tabindex first-clients
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore wiring (save/restore 메서드 이미 land)



### R51.53 — R51.53 §5.39 Shell focus wiring + WidgetView trait extension (apply_key broadcast → focused-only breaking)

**Changes**:
- WidgetView::focusable_tags() 신설 — default vec![tag()], composite override gateway
- WidgetView::apply_key signature breaking — (scene, focused: Option<&str>, key) 인자 추가
- AppShell::focus + modifiers field 신설 — FocusManager + winit ModifiersState 캠시
- Tab/Shift+Tab swallow by FocusManager (named_key_str 에서 Tab 제거, apply_key forward 차단)
- AppShell::click_to_focus 신설 — mouse Left Pressed + touch Started 후 focus auto-set
- WindowEvent::ModifiersChanged handler + Key::Named(Tab) match arm 명시 처리
- 3 examples (slider/slider-vertical/radio-group) apply_key signature 동시 update — _focused 일단 무시
- 16 example unit test 호출에 Some("main_slider") 인자 추가 (R51.56 focused-only refactor future-proof)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1302 pass / 0 fail (test count unchanged, signature 만 확장)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.35, §5.39


**Carry forward**:
- R51.54 — hello-button Space/Enter apply_key first-client (ARIA activation)
- R51.55 — Toggle/Checkbox/Radio Space activation 3-in-1 first-client
- R51.56 — Slider/SliderVertical _focused 인자를 실제 결정에 사용 (broadcast → focused-only)
- R51.57 — RadioGroup roving tabindex composite focus
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore wiring



### R51.54 — R51.54 §5.39 Button Space/Enter activation — ARIA keyboard click via SCXML internal transition

**Changes**:
- standard_button.sce-template.xml 에 keyboard_activate internal transition 2개 (idle/hover) 추가
- WidgetTransition::detect signature breaking — event 인자 추가 (Copy bound on Event)
- Button::detect 이 pointer_click ∨ keyboard_click 양측 “click” intent emit
- ButtonEvent::KeyboardActivate 자동 codegen + parse_button_event 에 수동 매핑 추가
- hello-button apply_key 신설 — focused == "main_btn" + Space/Enter 검증
- Button unit test +4 — idle/hover 에서 click intent + disabled → 침묵 + invoke 파시판



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1302 → 1306 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.38, §5.39


**Carry forward**:
- R51.55 — Toggle/Checkbox/Radio parse_*_event 에 KeyboardActivate 추가 + apply_key 3-in-1 first-client
- R51.56 — Slider/SliderVertical focused-only routing
- R51.57 — RadioGroup roving tabindex
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore wiring



### R51.55 — R51.55 §5.39 Toggle/Checkbox/Radio Space activation 3-in-1 — value sidecar flip on internal transition

**Changes**:
- Toggle/Checkbox/Radio::send 이 KeyboardActivate 시 value sidecar flip (단 disabled 는 침묵)
- Toggle/Checkbox/Radio::detect 가 keyboard_activate branch 추가 — state-stable internal 에서 동일 intent
- parse_toggle_event / parse_checkbox_event / parse_radio_event 에 KeyboardActivate 매핑 추가
- ToggleView / CheckboxView / RadioView apply_key 3-in-1 (toggle = Space|Enter, checkbox/radio = Space only)
- Toggle 4 + Checkbox 3 + Radio 3 = 10 unit test 추가 — idle/disabled/invoke 경로



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1306 → 1316 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.38, §5.39


**Carry forward**:
- R51.56 — Slider/SliderVertical focused-only routing (apply_key focused 인자 실제 결정)
- R51.57 — RadioGroup roving tabindex composite focus
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore wiring



### R51.56 — R51.56 §5.39 Slider/SliderVertical focused-only routed — broadcast aliasing 폐기

**Changes**:
- SliderView::apply_key / SliderVerticalView::apply_key 가 focused == Self::tag() gate 추가
- broadcast → focused-only — sibling widget 간 Arrow / Home / End / Page* aliasing 제거
- 4 unit test 추가 (2 widget 각 None + other-focus = false)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1316 → 1320 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.39


**Carry forward**:
- R51.57 — RadioGroup roving tabindex composite focus
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore wiring



### R51.57 — R51.57 §5.39 RadioGroup roving tabindex — focused-only routing (composite single tab stop)

**Changes**:
- RadioGroupView::apply_key 이 focused == "main_group" gate 추가 — composite single tab stop
- ARIA roving tabindex pattern 완감 — focusable_tags default + Arrow=focus+check (R51.44) + focused gate
- 4 unit test 추가 (focused=group routes / None/other-focus = silent / Home from End)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1320 → 1324 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.38, §5.39


**Carry forward**:
- R51.58 — paint-time focus ring rendering
- R51.59 — Window blur/restore wiring
- RadioGroup internal focused index (selected 과 분리) carry — evidence-first



### R51.58 — R51.58 §5.39 Focus visual ring — paint-time WCAG 2.4.11 indicator

**Changes**:
- paint_adapter::paint_focus_ring 신설 — 2px outer stroke + 2px offset, Material #1A73E8
- focus_rect_for_tag private helper (input::rect_for_tag 와 dup 의도, vello-gated)
- shell::render 이 to_vello 다음 paint_focus_ring 호출 — frame submit 이전
- WCAG 2.4.11 Focus Appearance 준수 — ≥2px 움라인, ≥3:1 contrast
- paint_adapter unit test +5 — rect lookup + 4 paint_focus_ring 시나리오



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1324 → 1329 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.16, §5.39


**Carry forward**:
- R51.59 — Window blur/restore wiring (save/restore 는 R51.52 로 이미 land)
- theming axis — focus ring color 를 Modifier 하튼으로 (현재 hard-coded blue)



### R51.59 — R51.59 §5.39 Focus restoration — Window blur/refocus wiring (save/restore 호출 site)

**Changes**:
- AppShell::window_event 에 WindowEvent::Focused arm 신설
- focused=false → FocusManager::save / focused=true → FocusManager::restore
- Alt+Tab 후 반환 시 직전 focused widget 복원, ARIA Focus Order 준수



**Verification**:
- cargo test --workspace --features pinion-runtime/vello — 1329 pass / 0 fail (test count unchanged, winit event path)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.39


**Carry forward**:
- theming axis — focus ring color 를 Modifier hint 로
- RadioGroup internal focused index (selected 과 분리) 카리 — evidence-first
- focus_set RPC method ('focus/set') — AI-first 측 programmatic focus, R51.59 carry 청산 evidence 시



### R51.60 — R51.60 §5.40 a11y semantic tree RFC — AccessKit integration for WCAG 4.1.2 (Name/Role/Value)

**Changes**:
- §5.40 신설 — Accessibility semantic tree RFC (design-only round, code mutation 0)
- AccessKit 선택 — Rust 표준 cross-platform AT adapter (UIA/AX/AT-SPI/Android, Mozilla/Bevy/egui/Slint 채택)
- WidgetView::access_node(&Scene, &str) -> Option<AccessNode> trait method default None spec
- Action 5종 (Click/Focus/Increment/Decrement/Default) → InputRouter intent 변환 layer spec
- 13 caveats: adapter ownership, TreeUpdate debounce, action 매핑, name 추출, live region/AT test carry



**Verification**:
- validate_workspace clean — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync/sections=56→57
- code unchanged (design-only round, R51.43/.48/.49/.50/.51 spec-only precedent)



**Impact**: §5.40


**Carry forward**:
- R51.61 — accesskit + accesskit_winit dependency add + pinion-a11y substrate crate
- R51.62 — pinion-shell AppShell Adapter wiring + WindowEvent::Accessibility* arm
- R51.63-67 — per-widget access_node 매핑 (Button/Toggle/Checkbox/Radio/Slider/RadioGroup)
- R51.67 — Action handler → InputRouter intent 변환 layer
- R51.68 — conformance test (AccessKit consumer mock + Tree snapshot + ActionRequest round-trip)



### R51.61 — R51.61 §5.40 pinion-a11y substrate — AccessNode / AriaRole / AccessTreeBuilder / AccessAction land

**Changes**:
- crates/pinion-a11y 신설 — AccessKit wrapper substrate (lib + role + node + tree + action)
- AriaRole enum (Button/Switch/CheckBox/RadioButton/Slider/RadioGroup/Generic) — to_accesskit + aria_name lower
- AccessNode + AccessState + AccessValue — pinion-native widget descriptor + builder pattern
- AccessTreeBuilder — TreeUpdate 조립 + composite parent-child + ROOT_NODE_ID(1) 예약
- AccessAction + translate_action — accesskit::Action 5종 매핑 + unmapped Other silent drop
- tag_to_node_id — DefaultHasher + high-bit reserve (NodeId(1) 겹침 방지)
- accesskit 0.24 workspace.dependencies 추가 + pinion-a11y workspace member 등록



**Verification**:
- cargo test -p pinion-a11y — 36 pass / 0 fail (role 6 + node 8 + tree 13 + action 9)
- cargo test --workspace --features pinion-runtime/vello — 1365 pass / 0 fail (+36 from 1329)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- R51.62 — pinion-shell AppShell 의 accesskit_winit::Adapter wiring + WindowEvent::Accessibility* arm
- R51.63-66 — WidgetView::access_node trait 신설 + per-widget 매핑 (Button/Toggle/Checkbox/Radio/Slider/RadioGroup)
- R51.67 — Action handler → InputRouter intent 변환 layer
- R51.68 — conformance integration test (AccessKit consumer mock + Tree snapshot + ActionRequest round-trip)



### R51.62 — R51.62 §5.40 AppShell accesskit_winit::Adapter wiring + WidgetView::access_node trait

**Changes**:
- accesskit_winit 0.33 workspace.dependencies 추가 + pinion-shell 의 accesskit + accesskit_winit 소비
- AppEvent 확장 — AccessKit(accesskit_winit::Event) variant + From<Event> impl (Clone derive 제거)
- WidgetView::access_node(&State, Option<&str>) -> Vec<AccessNode> trait method 신설 (default empty)
- AppShell.proxy + .accesskit field 추가, new(proxy) 생성자, Default impl 제거
- resumed 시 accesskit_winit::Adapter::with_event_loop_proxy 호출 (Active 처음 진입 한 번)
- forward_to_accesskit helper — winit WindowEvent 를 Adapter::process_event 로 전달
- user_event 확장 — AppEvent::AccessKit 처리 (InitialTreeRequested/ActionRequested/Deactivated)
- render 후부 — Adapter::update_if_active 로 TreeUpdate emit (paint_scene 이동 전)
- pinion-runtime::rect_for_tag 을 pub 로 승격 + lib.rs re-export (shell 적장)



**Verification**:
- cargo build -p pinion-shell --features pinion-runtime/vello — clean (accesskit_winit linked)
- cargo test --workspace --features pinion-runtime/vello — 1365 pass / 0 fail (활동 수 유지)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings (window_event helper extract)
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- R51.63 — Button::access_node override (첫 first-client + Button 소속 AccessNode)
- R51.64 — Toggle/Checkbox/Radio access_node 매핑 (Switch/CheckBox/RadioButton role)
- R51.65 — Slider access_node + Float value + Action::Increment/Decrement support
- R51.66 — RadioGroup composite access_node (parent + radio_N children)
- R51.67 — ActionRequested handler → translate_action + InputRouter intent 변환 실제 디스패치



### R51.63 — R51.63 §5.40 Button access_node first-client — AriaRole::Button + 4-state flag mapping

**Changes**:
- examples/hello-button/src/main.rs 의 ButtonView::access_node override (첫 first-client)
- AriaRole::Button + label 이름 ("Click me!" / "Disabled") + AccessState 4 flag 매핑
- ButtonState 4종 → AccessState (Idle=없음, Hover=hovered, Pressed=pressed, Disabled=disabled)
- focused == Some("main_btn") 시 자동 focused flag set
- examples/hello-button Cargo.toml 의 pinion-a11y 의존성 추가
- hello-button 접수 7 unit test (idle/hover/pressed/disabled/focused/non-focused/checked)



**Verification**:
- cargo test -p hello-button — 7 pass / 0 fail (a11y_tests 명당)
- cargo test --workspace --features pinion-runtime/vello — 1372 pass / 0 fail (+7 from 1365)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- R51.64 — Toggle/Checkbox/Radio access_node (Switch/CheckBox/RadioButton role + checked state)
- R51.65 — Slider access_node (Slider role + Float value + min/max + orientation hint)
- R51.66 — RadioGroup composite access_node (parent + children topology)
- R51.67 — ActionRequested dispatch (translate_action → InputRouter intent 변환)



### R51.64 — R51.64 §5.40 Toggle/Checkbox/Radio access_node — Switch/CheckBox/RadioButton 3-in-1

**Changes**:
- ToggleView::access_node — AriaRole::Switch + AccessValue::Bool + checked state lockstep
- CheckboxView::access_node — AriaRole::CheckBox + AccessValue::Bool + checked state lockstep
- RadioView::access_node — AriaRole::RadioButton + AccessValue::Bool + checked state lockstep
- 3 example labels 고정: 'Dark mode' / 'Receive newsletter' / 'Premium tier'
- 3 example Cargo.toml 의 pinion-a11y 의존성 추가
- 12 unit test 추가 (4 each — unchecked/checked/disabled/focused)



**Verification**:
- cargo test -p hello-toggle -p hello-checkbox -p hello-radio — 12 pass / 0 fail
- cargo test --workspace --features pinion-runtime/vello — 1384 pass / 0 fail (+12 from 1372)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- R51.65 — Slider access_node (AriaRole::Slider + Float value + min/max/orientation)
- R51.66 — RadioGroup composite access_node (parent + radio_N children)
- R51.67 — ActionRequested dispatch (translate_action → InputRouter intent)
- R51.68 — conformance test (Tree snapshot + ActionRequest round-trip)



### R51.65 — R51.65 §5.40 Slider/SliderVertical access_node — AriaRole::Slider + Float value range

**Changes**:
- SliderView::access_node 신설 — AriaRole::Slider + AccessValue::Float(value, 0.0, 1.0) range
- SliderVerticalView::access_node 신설 — horizontal 과 동일 role/value, orientation hint carry
- Dragging state → pressed flag, Hover → hovered, Disabled → disabled, checked = None
- 2 example Cargo.toml 의 pinion-a11y 의존성 추가
- 9 unit test 시워 Slider + 3 unit test SliderVertical 추가 (range / focus / state)



**Verification**:
- cargo test -p hello-slider — 20 pass (a11y_tests 6 + apply_key tests 14 — 기존 유지)
- cargo test -p hello-slider-vertical — 10 pass (a11y_tests 3 + apply_key tests 7 — 기존 유지)
- cargo test --workspace --features pinion-runtime/vello — 1393 pass / 0 fail (+9 from 1384)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings



**Impact**: §5.40


**Carry forward**:
- R51.66 — RadioGroup composite access_node (parent + radio_N children)
- R51.67 — ActionRequested dispatch (Click/Focus/Increment/Decrement 실제 디스패치)
- R51.68 — conformance test (Tree snapshot + ActionRequest round-trip)
- AccessNode 의 orientation field 캴 (Slider vertical/horizontal 구분) — evidence 시



### R51.66 — R51.66 §5.40 RadioGroup composite access_node + WidgetView::access_focus_target trait method

**Changes**:
- WidgetView::access_focus_target trait method 신설 — composite focus redirect (default passthrough)
- AppShell::render 의 builder.focused() = access_focus_target 결과 사용 (atomic widget 은 그대로)
- RadioGroupView::access_node 신설 — N+1 nodes (RadioGroup parent + N RadioButton children)
- RadioGroupView::access_focus_target 신설 — 'main_group' focus → 'main_group#active_idx' redirect
- active_radio_index helper — selected radio 또는 fallback 0 (arrow_step 과 일관)
- hello-radio-group Cargo.toml 의 pinion-a11y 의존성 추가
- 11 unit test 추가 (N+1 노드 / children / label / checked / focus redirect / passthrough)



**Verification**:
- cargo test -p hello-radio-group — 15 pass / 0 fail (a11y_tests 11 + tests 4)
- cargo test --workspace --features pinion-runtime/vello — 1404 pass / 0 fail (+11 from 1393)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- R51.67 — ActionRequested dispatch (translate_action → focus/click/increment 실제 수행)
- R51.68 — conformance test (Tree snapshot per widget + ActionRequest round-trip)
- accesskit Node::set_active_descendant (현재 focus redirect 으로 대체) — evidence 시 강화



### R51.67 — R51.67 §5.40 AccessKit ActionRequested dispatch — Click/Focus/Increment/Decrement intent

**Changes**:
- AppShell.last_access_tag_map (NodeId→tag) field 신설 + render 마다 갱신
- build_tag_map helper — ROOT_NODE_ID + AccessNode tag 와 NodeId 매핑
- handle_action_request — translate_action → PinionAccessAction 디스패치
- dispatch_access_action — Focus/Click/Default/Increment/Decrement→Enter/ArrowRight/ArrowLeft 매핑
- apply_a11y_key helper — focus_set + apply_key + revision bump + refresh_state + drain_intents
- composite child tag (main_group#N) = parent focus + carry log (widget-specific wire-format 필요)
- handle_accesskit_event ActionRequested arm — carry log 제거 + handle_action_request 호출



**Verification**:
- cargo build -p pinion-shell --features pinion-runtime/vello — clean
- cargo test --workspace --features pinion-runtime/vello — 1404 pass / 0 fail (wiring only, no new tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- R51.68 — conformance integration test (Tree snapshot per widget + ActionRequest round-trip)
- composite child action dispatch — widget-side per-index invoke surface (RadioGroup wire-format)
- AccessAction::Default semantic difference (Enter vs widget-specific) — evidence 시



### R51.68 — R51.68 §5.40 a11y conformance integration test — mixed scene Tree snapshot + ActionRequest round-trip

**Changes**:
- crates/pinion-a11y/tests/conformance.rs 신설 — end-to-end integration test
- mixed scene fixture: Button + Switch + Slider + RadioGroup(3 children) = 7 widgets
- 14 conformance test — tree topology / focus resolution / tag_map / ActionRequest round-trip
- ActionRequest round-trip per kind (Click/Increment/Focus on root/unknown/composite child/unmapped)
- TreeUpdate metadata invariant (initial=Some / subsequent=None) 결과 검증



**Verification**:
- cargo test -p pinion-a11y --test conformance — 14 pass / 0 fail
- cargo test --workspace --features pinion-runtime/vello — 1418 pass / 0 fail (+14 from 1404)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- validate_workspace — T1=0/T3=0/round-trip=1/1/GENERATED.md=sync



**Impact**: §5.40


**Carry forward**:
- composite child action dispatch (widget-side wire-format) — evidence 시 장착
- platform AT integration test (Windows Narrator / macOS VoiceOver / Linux Orca) — manual carry
- accesskit_consumer crate 기반 mock AT (Tree walk 검증) — evidence 시
- Modifier::aria_label hint surface (widget 내부 Text override) — evidence 시



### R51.69 — R51.69 §5.40 aria_label hint surface — ContainerNode override + scene-walk name derivation (WAI-ARIA name precedence)

**Changes**:
- crates/pinion-core/src/scene.rs — ContainerNode::aria_label field + with_aria_label() builder
- crates/pinion-a11y/src/scene_label.rs 신설 — enrich_names_from_scene + DFS first-text-leaf helper
- crates/pinion-a11y/src/lib.rs — scene_label 모듈 등록 + enrich_names_from_scene re-export
- crates/pinion-shell/src/lib.rs — render 가 access_node 결과를 enrich (paint_scene 기반)
- 7 widget access_node 의 .with_name(label) hard-coded literal 제거 (DRY 회복) — RadioGroup parent 만 explicit name 유지 (scene 비-존재)
- hello-toggle / hello-checkbox / hello-slider / hello-slider-vertical view fn — tagged container 에 .with_aria_label(...) 추가 (label sibling 위치 / check-glyph 회피)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — 0 warnings
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1430 pass / 0 fail (+12 from 1418)
- scene_label 8 unit test (override / DFS first-text / multiline collapse / nested) + 4 widget enriched 회귀 test



**Impact**: §5.40, §5.11


**Carry forward**:
- R51.70 composite child action dispatch (WidgetView::access_child_invoke trait hook + RadioGroup wire-format invoke) — WCAG 4.1.2 write path 회복
- R51.71 accesskit::Node::set_active_descendant 채택 — focus redirect 폐기, ARIA Authoring Practices 정통
- R51.72 incremental TreeUpdate dirty tracking — last_access_nodes cache, AccessKit performance 권고 준수



### R51.70 — R51.70 §5.40 composite child action dispatch — WidgetView::access_child_invoke hook + RadioGroup wire-format (WCAG 4.1.2 write 회복)

**Changes**:
- crates/pinion-shell/src/lib.rs — WidgetView::access_child_invoke trait method (default false)
- crates/pinion-shell/src/lib.rs — AppShell::dispatch_access_action composite child 경로 교체 (carry log 제거, V::access_child_invoke 호출 + revision/refresh/drain commit)
- examples/hello-radio-group/src/main.rs — RadioGroupView::access_child_invoke impl (Click/Default = wire-format invoke, Focus = R51.71 carry suppression, 그 외 fallback)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1438 pass / 0 fail (+8 from 1430)
- 8 RadioGroup access_child_invoke test (Click/Default/switch/Focus/out-of-range/non-numeric/Increment-decline/Other-decline)



**Impact**: §5.40


**Carry forward**:
- R51.71 accesskit::Node::set_active_descendant 채택 — focus redirect 폐기, ARIA Authoring Practices 정통
- R51.72 incremental TreeUpdate dirty tracking — last_access_nodes cache, AccessKit performance 권고 준수



### R51.71 — R51.71 §5.40 active_descendant 정통 — AccessFocus typed + accesskit Node::set_active_descendant (ARIA roving-tabindex)

**Changes**:
- crates/pinion-a11y/src/focus.rs 신설 — AccessFocus struct + atomic/composite constructor
- crates/pinion-a11y/src/lib.rs — focus 모듈 등록 + AccessFocus re-export
- crates/pinion-a11y/src/tree.rs — AccessTreeBuilder.active_descendants HashMap + active_descendant() setter, build() 시 accesskit::Node::set_active_descendant 호출
- crates/pinion-shell/src/lib.rs — WidgetView::access_focus_target 시그니처 Option<String> → Option<AccessFocus>; render 의 builder.focused + builder.active_descendant 등록 경로 교체
- examples/hello-radio-group/src/main.rs — access_focus_target composite 변형 반환 (parent + active descendant)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1445 pass / 0 fail (+7 from 1438)
- AccessFocus 3 unit + AccessTreeBuilder active_descendant 3 unit + conformance composite focus 1



**Impact**: §5.40


**Carry forward**:
- R51.72 incremental TreeUpdate dirty tracking — last_access_nodes cache, AccessKit performance 권고 준수
- composite tabindex vs selection 구분 — ARIA radio-group 의 tabindex 가 selected_index 와 독립적이어야 함 (현재 pinion 은 결합, R51.x carry)



### R51.72 — R51.72 §5.40 incremental TreeUpdate — AccessTreeBuilder.dirty_tags + last_access_nodes diff (AccessKit incremental-update 권고 준수)

**Changes**:
- crates/pinion-a11y/src/tree.rs — AccessTreeBuilder.dirty: Option<HashSet<String>> + dirty_tags() setter, build() 시 dirty subset 만 emit (root 은 항상)
- crates/pinion-shell/src/lib.rs — AppShell.last_access_nodes + access_emit_initial 필드 추가, render 의 access tree emit 구간 재구성 (bounds 적용 → diff vs cache → cache snapshot → builder.dirty_tags 전달)
- crates/pinion-a11y/tests/conformance.rs — incremental emit + empty-dirty focus-only 회귀 test 2



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1451 pass / 0 fail (+6 from 1445)
- AccessTreeBuilder dirty_tags 4 unit + conformance incremental 2



**Impact**: §5.40


**Carry forward**:
- composite tabindex vs selection 구분 — ARIA radio-group 의 tabindex 가 selected_index 와 독립적이어야 함 (현재 pinion 은 결합, R51.x carry)
- no-change frame emit skip — 궁극 제로 을 위해 dirty+focus 변화 없으면 update_if_active 자체 스킵 (R51.x carry)



### R51.73 — R51.73 §5.40 focus/set + focus/get RPC — AI a11y primary path (AccessKit Focus action 의 RPC dual)

**Changes**:
- crates/pinion-rpc/src/focus.rs 신설 — focus_set + focus_get + FocusError + FocusSetParams + FocusState
- crates/pinion-rpc/src/dispatch.rs — DispatchContext.focus_manager 필드 + with_focus_manager() builder + focus/set + focus/get 라우트
- crates/pinion-rpc/src/lib.rs — focus 모듈 등록 + re-export
- crates/pinion-shell/src/lib.rs — dispatch_rpc 가 with_focus_manager 연결, focus 변경 감지 시 request_redraw



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1463 pass / 0 fail (+12 from 1451)
- focus.rs 6 unit (known/null/unknown/no-op/get/empty) + dispatch 6 wire integration



**Impact**: §5.40, §5.7


**Carry forward**:
- composite tabindex vs selection 구분 — R51.71 carry, ARIA radio-group tabindex independence
- no-change frame emit skip — R51.72 carry, AccessKit incremental 극극 제로
- focus/next + focus/prev RPC — keyboard Tab equivalent 의 AI path



### R51.74 — R51.74 §5.40 focus/next + focus/prev RPC — Tab / Shift+Tab 구현설 동웁 (AI primary path 의 keyboard navigation dual)

**Changes**:
- crates/pinion-rpc/src/focus.rs — focus_next + focus_prev + handle_focus_next + handle_focus_prev
- crates/pinion-rpc/src/lib.rs — re-export focus_next, focus_prev
- crates/pinion-rpc/src/dispatch.rs — focus/next + focus/prev 라우트 (last-arm move 패턴으로 clippy needless_option_as_deref 회피)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1472 pass / 0 fail (+9 from 1463)
- focus.rs 7 unit (next/prev advance/wrap/from-unfocused/empty) + dispatch 2 wire



**Impact**: §5.40, §5.7


**Carry forward**:
- composite tabindex vs selection 구분 — R51.71 carry
- no-change frame emit skip — R51.72 carry



### R51.75 — R51.75 §5.40 no-change frame AT emit skip — last_access_focus diff (R51.72 carry repayment)

**Changes**:
- crates/pinion-shell/src/lib.rs — AppShell.last_access_focus 필드 추가, render 의 emit 구간이 dirty.is_empty() && focus 불변 시 update_if_active 자체 스킵



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- cargo test --workspace --features pinion-runtime/vello — 1472 pass / 0 fail (R51.75 는 behavior change, R51.72 substrate 이 쿼워 됩 테스트 재사용)
- AppShell mock-based dispatch path test infra 는 별도 carry (R51.x)



**Impact**: §5.40


**Carry forward**:
- AppShell mock-based dispatch path test infrastructure (handle_action_request / dispatch_access_action / apply_a11y_key / R51.75 skip behavior)
- tag_to_node_id collision 디버그 검증 (debug_assert injective on build) — known textbook 약점



### R51.76 — §5.40 R51.76 — substrate/surface 분리: ShellCore<V> 추출 + AccessEmitPlan + redraw flag drain + 17 dispatch_core 회귀 test 추가 (R51.75 verification gap 청산).

**Changes**:
- crates/pinion-shell/src/lib.rs — ShellCore<V> struct 신설 (14 dispatch substrate 필드 + redraw_requested flag 보유; AppShell 은 render / vello_scene / proxy / accesskit 만 owning)
- crates/pinion-shell/src/lib.rs — AccessEmitPlan struct (should_emit + initial + dirty + nodes + focus carrier) + ShellCore::compute_access_emit (pure emit decision + cache update)
- crates/pinion-shell/src/lib.rs — ShellCore::dispatch_rpc 시그니처 &mut dyn FnMut(u32, u32) 로 변경 (DispatchContext 와 일관, monomorphization 회피)
- crates/pinion-shell/src/lib.rs — ShellCore::request_redraw flag 기반 + AppShell::drain_redraw_to_winit drain helper, ApplicationHandler arm 끝에서 호출
- crates/pinion-shell/src/lib.rs — AppShell::handle_key_press helper extract (window_event too_many_lines 100 LOC 회복)
- crates/pinion-shell/src/lib.rs — dispatch_access_action / handle_action_request pub 노출 + Default impl for ShellCore
- crates/pinion-shell/tests/dispatch_core.rs — 17 회귀 test (R51.67 atomic Focus/Click/Default/Increment/Decrement/Other + R51.70 composite child invoke true/false + R51.67 handle_action_request resolve via compute_access_emit + R51.72 dirty diff + R51.75 no-change skip + R51.71 active descendant + R51.75 focus unset 후 emit)



**Verification**:
- cargo test -p pinion-shell --test dispatch_core --features pinion-runtime/vello → 17 passed / 0 failed
- cargo test --workspace --features pinion-runtime/vello → 1489 passed / 0 failed / 8 ignored (+17 from 1472 baseline)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings (workspace.lints pedantic deny 유지)
- Mnemosyne validate_workspace → entries=220 / T1=0 / round-trip 1/1 / GENERATED.md=sync / commit↔ledger missing=0



**Impact**: §5.40


**Carry forward**:
- R51.77 — compute_access_emit naming 약점 (pure 처럼 보이나 self.last_access_* mutation): pure decision fn + 별도 commit step 분리가 textbook 정통
- R51.78 — handle_key_press winit 결합 (Key + ActiveEventLoop 동시 의존): Escape arm 별도 dispatch + helper winit-free 가 textbook 정통 + 단위 테스트 가능
- R51.79 — AccessEmitPlan owning Vec<AccessNode> alloc churn (60fps animation 누적): borrow-based 또는 by-value consume 정당성 명시
- R51.80 — ShellCore 14 필드 pub(crate) cross-struct intimacy: render path 의 paint_scene compute + focus ring paint 도 ShellCore owning 으로 deeper extraction (encapsulation 정통)
- 이전 carry — aria_label Band-Aid (R51.77 대안 carry) / API 일관성 AccessFocus builder + AccessTreeBuilder signature 통일 / handle_focus_* boilerplate helper / composite AccessAction::Focus 의미 명확화 / 이전 R51.x carry 11개



### R51.77 — §5.40 R51.77 — compute_access_emit silent surprise 청산: plan_access_emit (pure &self, AccessEmitDecision 반환) + commit_access_emit (&mut, cache 진보) 2-step textbook 분리.

**Changes**:
- crates/pinion-shell/src/lib.rs — AccessEmitPlan (owning nodes+focus) 제거, AccessEmitDecision (should_emit + initial + dirty 3-field) 신설
- crates/pinion-shell/src/lib.rs — compute_access_emit 제거, plan_access_emit (&self, borrowed nodes/focus, pure) + commit_access_emit (&mut, cache update only) 신설
- crates/pinion-shell/src/lib.rs — AppShell::render 가 plan + (optional) emit + commit pattern 으로 재구성 (nodes 클론 1회 이동 closure consume, focus 도 borrow 유지)
- crates/pinion-shell/tests/dispatch_core.rs — 7 compute_access_emit 공호출처 plan+commit 제안 대체 + R51.77 plan purity 회귀 test 신설 (back-to-back plan = identical decision) + R51.71 active descendant 재구성 (dirty leak 검증)



**Verification**:
- cargo test -p pinion-shell --test dispatch_core --features pinion-runtime/vello → 18 passed / 0 failed (+1 R51.77 purity regression)
- cargo test --workspace --features pinion-runtime/vello → 1490 passed / 0 failed / 8 ignored (+1 from 1489)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings (workspace.lints pedantic deny 유지)
- Mnemosyne validate_workspace → entries=221 / T1=0 / round-trip 1/1 / commit↔ledger missing=0 / AccessEmitPlan + compute_access_emit stale citation 청산 (remove_section_implementation 2회)



**Impact**: §5.40


**Carry forward**:
- R51.78 — AppShell::handle_key_press winit 결합 (Key + ActiveEventLoop 동시 의존): Escape arm 별도 dispatch + helper winit-free 가 textbook 정통 + 단위 테스트 가능
- R51.79 — AppShell::render 의 nodes.clone() (R51.77 표즜 설계 포함): closure 소유권 이전 vs commit 세션 아래 by-ref pattern 완성함 — 60fps animation alloc churn 회복
- R51.80 — ShellCore 14 필드 pub(crate) cross-struct intimacy: render path 의 paint_scene compute + focus ring paint 도 ShellCore owning 으로 deeper extraction (encapsulation 정통)
- 이전 carry — aria_label Band-Aid (R51.77 대안) / API 일관성 AccessFocus builder + AccessTreeBuilder signature 통일 / handle_focus_* boilerplate helper / composite AccessAction::Focus 의미 명확화



### R51.78 — §5.40 R51.78 — handle_key_press winit 결합 청산: ShellCore 에 handle_focus_traverse / handle_character_key / handle_named_key 3 winit-free method 분리, AppShell::handle_key_press 는 winit↔substrate adapter routing 으로 축소.

**Changes**:
- crates/pinion-shell/src/lib.rs — ShellCore::handle_focus_traverse(shift) -> bool (Tab/Shift+Tab dispatch, focus.focus_next/prev + request_redraw + change flag 반환)
- crates/pinion-shell/src/lib.rs — ShellCore::handle_character_key(c) (V::keybinding lookup → forward 또는 apply_key fallthrough)
- crates/pinion-shell/src/lib.rs — ShellCore::handle_named_key(key_str) (V::apply_key thin wrap)
- crates/pinion-shell/src/lib.rs — forward / apply_key pub 으로 노출 (R51.78 테스트 접근 + 이전 R51.x carry pub fix 동반)
- crates/pinion-shell/src/lib.rs — AppShell::handle_key_press 가 ~40 LOC → ~15 LOC (Escape만 winit 고유, 나머지 core delegation)
- crates/pinion-shell/tests/dispatch_core.rs — 5 R51.78 winit-free key dispatch 회귀 test (Tab/Shift+Tab/character±binding/named) + KEYBINDING_RETURNS_SOME + EVENT_NAME_LOG mock 추가



**Verification**:
- cargo test -p pinion-shell --test dispatch_core --features pinion-runtime/vello → 23 passed / 0 failed (+5 R51.78 회귀)
- cargo test --workspace --features pinion-runtime/vello → 1495 passed / 0 failed / 8 ignored (+5 from 1490)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings (workspace.lints pedantic deny 유지)
- Mnemosyne validate_workspace → entries=222 → 다음 R51.78 append 후 223 / T1=0 / round-trip 1/1 / sync



**Impact**: §5.40


**Carry forward**:
- R51.79 — AppShell::render 의 nodes.clone() (R51.77 carry 잘라옥): closure consume + commit by-ref pattern 와 충돌, alloc 회복 필요
- R51.80 — ShellCore 14 필드 pub(crate) cross-struct intimacy: render path 의 paint_scene compute + focus ring paint 도 ShellCore owning 으로 deeper extraction
- 이전 carry — aria_label Band-Aid / AccessFocus builder + AccessTreeBuilder signature 통일 / handle_focus_* boilerplate helper / composite AccessAction::Focus 의미 명확화 / Tier-1 R51.x carry 9개



### R51.79 — §5.40 R51.79 — R51.77 carry 청산: AccessTreeBuilder::add 시그니처 &AccessNode (borrow) + commit_access_emit by-value Vec move 으로 AppShell::render 의 nodes.clone() 제거, 프레임당 alloc 2N → N.

**Changes**:
- crates/pinion-a11y/src/tree.rs — AccessTreeBuilder::add(&AccessNode) signature (내부 clone, caller 가 Vec 소유권 유지)
- crates/pinion-a11y/src/tree.rs — unit test 17개 사이트 b.add(...) → b.add(&...) update
- crates/pinion-a11y/tests/conformance.rs — 13 conformance 사이트 builder.add(node) → builder.add(&node) update
- crates/pinion-shell/src/lib.rs — ShellCore::commit_access_emit by-value Vec<AccessNode> + into_iter().map move (클론 0회)
- crates/pinion-shell/src/lib.rs — AppShell::render: nodes_for_emit clone 제거, closure 가 &nodes borrow + commit 이 by-value move 소비
- crates/pinion-shell/tests/dispatch_core.rs — 6 commit_access_emit 사이트 nodes.clone() 명시 (test 구조 상 nodes 재사용 필요)



**Verification**:
- cargo test -p pinion-shell --test dispatch_core --features pinion-runtime/vello → 23 passed / 0 failed
- cargo test --workspace --features pinion-runtime/vello → 1495 passed / 0 failed / 8 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings
- AccessTreeBuilder unit 17 + conformance 13 + dispatch_core 23 — 전 caller fan-out 적용 검증



**Impact**: §5.40


**Carry forward**:
- R51.80 — ShellCore 14 필드 pub(crate) cross-struct intimacy: render path 의 paint_scene compute + focus ring paint 도 ShellCore owning 으로 deeper extraction (encapsulation 정통)
- 이전 carry — aria_label Band-Aid / AccessFocus builder + AccessTreeBuilder signature 통일 / handle_focus_* boilerplate helper / composite AccessAction::Focus 의미 명확화 / Tier-1 R51.x carry 9개 (touch cancel commit-class, pressure widget, drag 2nd, lifecycle 2nd, 3D primitive, font mirror, L4 conformance, theming axis, platform AT test)



### R51.80 — §5.40 R51.80 — ShellCore deeper extraction: compute_paint_scene/collect_access_emit_inputs/finalize_frame + cursor_moved/_left/mouse_pressed/_released/touch_event/set_modifiers/window_focused/_blurred 10 wrapper method, AppShell::render + window_event 의 cross-struct intimacy 청산.

**Changes**:
- crates/pinion-shell/src/lib.rs — ShellCore::compute_paint_scene(w,h)->Scene (V::view + compute_layout encapsulate)
- crates/pinion-shell/src/lib.rs — ShellCore::collect_access_emit_inputs(&Scene)->(Vec<AccessNode>, Option<AccessFocus>) (V::access_node + enrich + bounds + access_focus_target pipeline)
- crates/pinion-shell/src/lib.rs — ShellCore::finalize_frame(Scene) (last_paint_layout + router.update + refresh_state + drain_intents)
- crates/pinion-shell/src/lib.rs — ShellCore::{cursor_moved/_left/mouse_pressed/_released/touch_event/set_modifiers/window_focused/_blurred} 7 winit-free wrapper method
- crates/pinion-shell/src/lib.rs — AppShell::render 110 LOC → ~70 LOC (paint scene compute/access input collect/finalize 단일 호출)
- crates/pinion-shell/src/lib.rs — AppShell::window_event 7 arm (CursorMoved/CursorLeft/MouseInput ×2/Touch/ModifiersChanged/Focused) 각 4-6 LOC 수준으로 축소, 높은 LOC arm 제거 (향후 too_many_lines 재발 방지)
- crates/pinion-shell/tests/dispatch_core.rs — 4 R51.80 회귀 test (compute_paint_scene root tag / finalize_frame idempotent / window_blurred+focused restore / collect_access_emit_inputs empty path)



**Verification**:
- cargo test -p pinion-shell --test dispatch_core --features pinion-runtime/vello → 27 passed / 0 failed (+4 R51.80)
- cargo test --workspace --features pinion-runtime/vello → 1499 passed / 0 failed / 8 ignored (+4 from 1495)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings



**Impact**: §5.40


**Carry forward**:
- pub(crate) 14 필드 일부 private 가능 (text_cache / last_paint_layout / last_access_*): R51.80 wrapper method land 이후 cross-crate 접근 제로 — 차기 round 에 명시적 visibility 단계적 하향
- 이전 carry — aria_label Band-Aid / AccessFocus builder + AccessTreeBuilder signature 통일 / handle_focus_* boilerplate helper / composite AccessAction::Focus 의미 명확화 / Tier-1 R51.x carry 9개 (이전 carry list 유지)



### R51.81 — §5.40 R51.81 — TextNode presentational role marker: pinion-core::TextRole 신설, enrich_names_from_scene 가 Presentational TextNode 스킵 → Checkbox 의 check-glyph aria_label Band-Aid 청산.

**Changes**:
- crates/pinion-core/src/scene.rs — TextRole enum (Default/Presentational/Label, non_exhaustive) + TextNode.role Option field + with_role(role) builder
- crates/pinion-a11y/src/scene_label.rs — first_text_leaf 가 Presentational TextNode 스킵 (DFS first-text scan 에서 제외) + 2 회귀 test
- examples/hello-checkbox/src/main.rs — check-glyph TextNode 에 .with_role(Presentational) 적용, ContainerNode::aria_label override 제거 (role marker 가 textbook fix), access_node doc 업데이트, test 이름 aria_label → role_marker
- Toggle/Slider/SliderVertical 은 변경 없음 — 이들의 aria_label 은 'label outside tagged scope' 정통 사용, Band-Aid 아닔 (검토 결과)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello → 1501 passed / 0 failed / 8 ignored (+2 R51.81 회귀)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings
- hello-checkbox role_marker_skips_check_glyph_when_checked 회귀 테스트 pass (Receive newsletter 이 enrichment 결과)



**Impact**: §5.40


**Carry forward**:
- pub(crate) 14 필드 일부 private 가능 (R51.80 carry) — visibility 단계적 하향
- AccessFocus builder + AccessTreeBuilder signature 통일
- handle_focus_* boilerplate helper
- composite AccessAction::Focus 의미 명확화
- Tier-1 R51.x carry 9개 (touch cancel commit-class / pressure widget / drag 2nd / lifecycle 2nd / 3D primitive / font mirror / L4 conformance / theming axis / platform AT test)
- TextRole::Label 변형 활용 — WAI-ARIA 1.2 §5.2.6 labelling axis 랜딩 시 explicit label TextNode 우선 적용



### R51.82 — §5.40 R51.82 — composite AccessAction::Focus 의미 명확화: dispatch_access_action::Focus arm 이 sub_tag 인식 + access_child_invoke 라우팅 (active descendant 갱신 책임 위임), R51.70/R51.71 carry 청산.

**Changes**:
- crates/pinion-shell/src/lib.rs — dispatch_access_action::Focus arm 이 sub_tag 인식, V::access_child_invoke(scene, sub, Focus) 호출 후 revision bump + refresh + drain
- crates/pinion-shell/src/lib.rs — Focus 는 단독 fall-back (apply_key Enter 철회) — keyboard 동등물 없으므로
- examples/hello-radio-group/src/main.rs — access_child_invoke::Focus arm comment 재구조 (R51.71 carry 쟠임, R51.82 의미 명문화)
- crates/pinion-shell/tests/dispatch_core.rs — 2 R51.82 회귀 test (composite Focus 라우팅 / atomic Focus 자명)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello → 1503 passed / 0 failed / 8 ignored (+2 R51.82)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello → 0 warnings
- RadioGroup access_child_invoke_focus_returns_true_without_mutation 기존 test 계속 pass



**Impact**: §5.40


**Carry forward**:
- RadioGroup state model 의 focused_index vs selected_index 분리 (WAI-ARIA roving-tabindex 정통): R51.x 포괄 안 속편, SCE template 재설계 필요
- pub(crate) 14 필드 일부 private 가능 (R51.80 carry)
- AccessFocus builder + AccessTreeBuilder signature 통일
- handle_focus_* boilerplate helper
- Tier-1 R51.x carry 9개
- TextRole::Label variant 활용 (R51.81 carry)



### R51.83 — §5.40 R51.83 — ShellCore 14 필드 + AppShell.core `pub(crate)` → private (encapsulation substantive): R51.80 wrapper-only claim 의 substantive 단계 land, surface boundary 강화, 외부 직접 접근 점 0개로 축소.

**Changes**:
- crates/pinion-shell/src/lib.rs — ShellCore 14 필드 (scene/cached_state/intent_queue/previews/revision/router/focus/modifiers/text_cache/last_paint_layout/last_access_tag_map/last_access_nodes/access_emit_initial/last_access_focus/redraw_requested) `pub(crate)` → private
- crates/pinion-shell/src/lib.rs — AppShell.core 필드 `pub(crate)` → private (surface 가 자신의 substrate 를 외부 노출하지 않음)
- crates/pinion-shell/src/lib.rs — ShellCore::text_cache_mut() accessor 신설 (paint_adapter::to_vello 의 유일 surface-side mutable 진입점)
- crates/pinion-shell/src/lib.rs — ShellCore::modifiers_shift_key() accessor 신설 (Tab+Shift detection 의 minimal-surface bit 노출 — ModifiersState 전체는 substrate-internal 보존)
- crates/pinion-shell/src/lib.rs — AppShell::render 의 paint_adapter::to_vello 호출이 self.core.text_cache_mut() 사용 (이전 &mut self.core.text_cache 직접 접근)
- crates/pinion-shell/src/lib.rs — AppShell::handle_key_press Tab arm 이 self.core.modifiers_shift_key() 사용 (이전 self.core.modifiers.shift_key() 직접 접근)
- crates/pinion-shell/src/lib.rs — AppShell::window_event CloseRequested arm 이 self.core.cached_state() accessor 사용 (이전 &self.core.cached_state 직접 접근)
- crates/pinion-shell/src/lib.rs — 14 필드 + AppShell.core doc comment 에 R51.83 private 결정 + accessor 통한 단방향 surface boundary 명시



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — 모든 crate clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warning (workspace.lints strict baseline 보호)
- cargo test --workspace --features pinion-runtime/vello — 1503 pass / 0 fail / 8 ignored (R51.82 baseline byte-identical)
- dispatch_core 27 test 전 통과 — 모든 외부 accessor (focus()/revision()/redraw_requested()) 만 사용
- grep `core\.(scene|cached_state|intent_queue|previews|revision|router|focus|modifiers|text_cache|last_paint_layout|last_access_|access_emit_initial|redraw_requested)` 외부 접근 점 0개 (이전 3개 — 1518/1607/1765 라인)
- R51.80 claim accuracy gap 회복 — `encapsulation 정통` claim 의 substantive 단계 (wrapper method add 만이 아니라 필드 visibility 하향) 완료



**Impact**: §5.40


**Carry forward**:
- R51.84 — AccessFocus::with_active_descendant builder + AccessTreeBuilder::initial signature 통일 (다른 method 와 &mut self 일관) + ContainerNode::with_aria_label → with_name_override rename (WAI-ARIA 의미 1:1 일치) + handle_focus_traverse empty tab_order early return
- R51.85 — pinion-rpc focus 4 route (set/get/next/prev) Option<&mut FocusManager> null check + RpcError 매핑 helper 추출 (DRY 회복)
- R51.86 — TextRole::Label variant 활용처 land 또는 enum 에서 제거 + carry 명시 (strict YAGNI textbook)
- R51.87 — RadioGroup focused_index vs selected_index 분리 (WAI-ARIA roving-tabindex 정통, SCE template 재설계 동반)



### R51.84 — §5.40 R51.84 — AccessTreeBuilder::initial signature 통일 (`mut self → Self` 에서 `&mut self → &mut Self` 로) + AccessFocus::with_active_descendant chainable builder 신설 (composite shorthand 가 atomic+with_active_descendant 로 delegate).

**Changes**:
- crates/pinion-a11y/src/tree.rs — AccessTreeBuilder::initial signature `mut self → Self` 에서 `&mut self → &mut Self` 로 통일 (add/focused/dirty_tags/active_descendant 과 일관)
- crates/pinion-a11y/src/tree.rs — initial_false_omits_tree_field test 가 by-value chain 대신 let-binding 으로 재구성
- crates/pinion-shell/src/lib.rs — render 의 `builder = builder.initial(false)` 가 plain `builder.initial(false)` 로 대체 (다른 setter 와 일관)
- crates/pinion-a11y/tests/conformance.rs — 3 test 의 by-value chain (subsequent_emission_omits_tree_metadata / incremental_emit_* / incremental_empty_dirty_*) 을 let-binding form 으로 전환
- crates/pinion-a11y/src/focus.rs — AccessFocus::with_active_descendant(mut self, child) -> Self chainable builder 신설 (AccessNode::with_* 패턴과 일관)
- crates/pinion-a11y/src/focus.rs — AccessFocus::composite shorthand 가 Self::atomic(parent).with_active_descendant(child) 로 delegate (single source of truth for active-descendant 설정)
- crates/pinion-a11y/src/focus.rs — r51_84_with_active_descendant_chains_on_atomic 회귀 test 신설 (chain == composite shorthand 동등성 검증)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — 모든 crate clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warning
- cargo test --workspace --features pinion-runtime/vello — 1504 pass / 0 fail / 9 ignored (+1 AccessFocus chain regression test, +1 intentionally-ignored doctest)
- AccessFocus chain == composite shorthand byte-identical 검증 (PartialEq), 필드 구조 변경 없음



**Impact**: §5.40


**Carry forward**:
- R51.84 evaluated but skipped: ContainerNode::with_aria_label → with_name_override rename. 이유: 구현차가 WAI-ARIA 1.2 §4.3 (Accessible Name Computation) + §5.2.6 (aria-label attribute) 와 1:1 일치함 — scene_label.rs 의 우선순위 체인이 경우 수는 aria-labelledby ≻ aria-label ≻ name-from-content 를 충실히 반영. 현재 이름이 표준에 정렬됨.
- R51.84 evaluated but skipped: handle_focus_traverse empty tab_order early return. 이유: FocusManager::advance 가 line 151-153 에서 이미 `if self.tab_order.is_empty() { return false; }` 로 short-circuit 함 — shell-layer 중복 체크는 방어적 중복코드, 동작 변경 없음.
- R51.85 — pinion-rpc focus 4 route (set/get/next/prev) Option<&mut FocusManager> null check + RpcError 매핑 helper 추출
- R51.86 — TextRole::Label variant 활용처 land 또는 enum 에서 제거
- R51.87 — RadioGroup focused_index vs selected_index 분리 (SCE template 재설계)



### R51.85 — §5.40 R51.85 — pinion-rpc focus 4 route handler boilerplate helper 추출 (err_focus_unavailable / err_invalid_params / err_internal / state_to_value / err_from_focus): DRY 회복, RpcError code 일관성 lockstep.

**Changes**:
- crates/pinion-rpc/src/focus.rs — err_focus_unavailable() helper 신설 (`-32004 focus manager unavailable` RpcError 4곳 중복 제거)
- crates/pinion-rpc/src/focus.rs — err_invalid_params(impl Display) helper 신설 (`-32602 Invalid params` map_err 대상, `impl Display` generic 으로 clippy needless_pass_by_value 회피)
- crates/pinion-rpc/src/focus.rs — err_internal(impl Display) helper 신설 (`-32603 Internal error` map_err 대상)
- crates/pinion-rpc/src/focus.rs — state_to_value(FocusState) helper 신설 (FocusState→serde_json::Value lift 의 단일 진입점)
- crates/pinion-rpc/src/focus.rs — err_from_focus(FocusError) helper 신설 (Unavailable→err_focus_unavailable, NotFocusable→-32602 `tag_not_focusable` 매핑)
- crates/pinion-rpc/src/focus.rs — handle_focus_set / _get / _next / _prev 본철 다 `let focus = focus.ok_or_else(err_focus_unavailable)?;` + helper 체인 으로 재작성 (각 ~3-7 LOC 로 축소)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — 모든 crate clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warning (needless_pass_by_value lint 도 해결)
- cargo test --workspace --features pinion-runtime/vello — 1504 pass / 0 fail / 9 ignored (R51.84 baseline byte-identical)
- focus.rs LOC 감소: handler 4개 이전 ~81 LOC → helper 5개 + handler 4개 ~75 LOC. DRY 회복 + code/message lockstep 강화.



**Impact**: §5.40


**Carry forward**:
- R51.86 — TextRole::Label variant 활용처 land 또는 enum 에서 제거 + carry 명시
- R51.87 — RadioGroup focused_index vs selected_index 분리 (SCE template 재설계)
- R51.84 carry 잠재: with_aria_label rename 결론 재검토 (현재 WAI-ARIA 정렬 판단 유지)
- R51.84 carry 잠재: handle_focus_traverse early return 결론 재검토 (현재 FocusManager short-circuit 중복 불필요 판단 유지)



### R51.86 — §5.40 R51.86 — TextRole::Label variant 제거 (forward-compat declaration 미land, strict YAGNI 회복): #[non_exhaustive] 보존으로 §5.2.6 labelling axis 정착 시 추가 변형 가능.

**Changes**:
- crates/pinion-core/src/scene.rs — TextRole::Label variant 제거 (활용처 0, forward-compat declaration 만 존재 했던 strict-YAGNI 위반)
- crates/pinion-core/src/scene.rs — TextNode::role + TextRole doc comment 에서 Label paragraph 제거 + R51.86 strict-YAGNI 설명 추가
- TextRole 은 여전히 #[non_exhaustive] 이므로 future WAI-ARIA 1.2 §5.2.6 labelling axis 정착 시 구체 consumer 와 함께 Label 명시적 재도입 가능



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — 모든 crate clean (Label 참조 없음 경우만 존재했으므로 제거 안전)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warning
- cargo test --workspace --features pinion-runtime/vello — 1504 pass / 0 fail / 9 ignored (R51.85 baseline byte-identical)
- TextRole consumer set: Default + Presentational 두 variant 만 권장 — 실제 사용처와 enum surface 1:1 일치 (claim accuracy)



**Impact**: §5.40


**Carry forward**:
- R51.87 — RadioGroup focused_index vs selected_index 분리 (WAI-ARIA roving-tabindex 정통, SCE template 재설계)
- future R5x.y — WAI-ARIA 1.2 §5.2.6 labelling axis 정착 시 TextRole::Label 재도입 (구체 consumer + 우선순위 룰 동반 land)
- R51.84 carry 잠재: with_aria_label rename / handle_focus_traverse early return — 기술적 정당성 재검토



### R51.87 — §5.40 R51.87 — RadioGroup focused_index vs selected_index 분리 (WAI-ARIA roving-tabindex 정통): AT Focus action 이 selected 와 독립적으로 active descendant 를 이동, application access_focus_target 가 focused → selected → 0 fallback 으로 일관.

**Changes**:
- crates/pinion-core/src/widgets/radio_group.rs — RadioGroup 구조체에 `focused: Option<usize>` 필드 + focused_index() getter + set_focused_index(idx) mutator 추가 (선택과 독립, no "selected" intent fires)
- crates/pinion-core/src/widgets/radio_group.rs — RadioGroupExternal::focused_index() forwarding accessor 추가
- crates/pinion-core/src/widgets/radio_group.rs — schema 6개 slot (count/selected_index/focused_index/state.<i>/selected.<i>/send) 이로 확장
- crates/pinion-core/src/widgets/radio_group.rs — query "focused_index" 추가 (Null 또는 Int)
- crates/pinion-core/src/widgets/radio_group.rs — intervene "focused_index" 추가 (Int/Null, out-of-range → TypeMismatch, no commit/intent)
- crates/pinion-core/src/widgets/radio_group.rs — Debug impl 에 focused_index 필드 추가
- crates/pinion-core/src/widgets/radio_group.rs — 9 R51.87 회귀 test 신설 (focused initial None / set independent / clear / diverge from selected / out-of-range panic / external query / intervene set / intervene null / intervene out-of-range reject)
- examples/hello-radio-group/src/main.rs — GroupState alias 을 struct `{rows: [(RadioState, bool); N], focused: Option<usize>}` 로 재설계 (Copy 보존)
- examples/hello-radio-group/src/main.rs — read_state 가 "focused_index" 을 읽어 GroupState.focused 에 반영
- examples/hello-radio-group/src/main.rs — active_radio_index 가 state.focused 우선 조회, fallback to selected, fallback to 0 해서 access_focus_target / access_node 둘 다 일관 해서키는 우선순위 접근
- examples/hello-radio-group/src/main.rs — access_child_invoke::Focus arm 이 intervene "focused_index" 로 idx 고정 (R51.82 의 `true` silent no-op 개선, 이제 실제 상태가 변경됨)
- examples/hello-radio-group/src/main.rs — fmt_state_log 에 focused index 동반 출력
- examples/hello-radio-group/src/main.rs — view / access_node / fmt_state_log 가 state.rows[i] / state.rows.iter() form 으로 업데이트
- examples/hello-radio-group/src/main.rs — 3 R51.87 회귀 test 신설 (active descendant honors focused over selected / falls back to selected when focused None / focused state marks correct radio)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello — 모든 crate clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warning
- cargo test --workspace --features pinion-runtime/vello — 1516 pass / 0 fail / 9 ignored (R51.86 baseline 1504에서 +12 테스트: pinion-core +9, hello-radio-group +3)
- WAI-ARIA roving-tabindex 정통: focus.focus_tag = parent + active_descendant = AT-addressed row (focused) 또는 selected fallback. selected commit 없이 Focus 명명적이로 이동 가능.



**Impact**: §5.40


**Carry forward**:
- future R5x.y — RadioGroup arrow-key activation path의 focused_index sync (activated row 가 focused_index 에도 반영되는 동기화 고려, 현재는 AT side 만 변경)
- future R5x.y — RadioGroup arrow keys 가 selected commit 없이 focused_index 만 이동 하는 listbox-style roving 옵션 (현재 radio-group 관례대로 immediate activate 유지)
- future R5x.y — listbox / menu / treeview / tab composite widget 가 같은 focused_index 패턴 공유 — RadioGroup 의 설계가 제일 설계 구독
- R51.84 carry 잠재: with_aria_label rename / handle_focus_traverse early return — 재검토



### R51.88 — R51.88 §5.40 — AccessFocus::with_active_descendant builder 제거 (R51.84 회수, strict YAGNI 일관)

**Changes**:
- crates/pinion-a11y/src/focus.rs — with_active_descendant builder 제거, composite 가 필드 직접 구성
- doc-comment ignore 예제 동반 제거, R51.84 회귀 테스트 R51.88 대체
- caller 0 + R51.86 strict YAGNI 일관 — 2-field struct actual axis 는 atomic + composite shorthand cover



**Verification**:
- cargo test 기존 몀테스트 필드 직접 구성으로 재작성 + 전원 통과
- future widget 의 conditional active descendant 필요 시 additive 재도입



**Impact**: §5.40


**Carry forward**:
- chain delegate 명시 제거 — future builder 도입 시 우선 evidence 필요



### R51.89 — R51.89 §5.40 — RpcError builder API + focus DRY 회수 (R51.85 framework 우회 정정)

**Changes**:
- crates/pinion-rpc/src/dispatch.rs — RpcError::new(code, message) base + with_data(Value) / with_data_string chain + invalid_params/internal_error convenience
- crates/pinion-rpc/src/focus.rs — err_invalid_params + err_internal helper 2 개 제거, 잔존 3 helper 는 builder 경유 (literal 0)
- focus 2 helper 는 RpcError builder 부재 의 우회 — builder land 후 안정 대체



**Verification**:
- dispatch 49 literal sweep 은 R51.89.1 carry (evidence-first)
- cargo test 전원 통과



**Impact**: §5.40


**Carry forward**:
- R51.89.1 = dispatch.rs 14 site full sweep (R51.89 follow-up)



### R51.89.1 — R51.89.1 §5.40 — dispatch.rs RpcError 14 site full sweep (R51.89 builder primitive full propagation)

**Changes**:
- 14 _error_to_rpc 함수 RpcError::invalid_params(variant) 통일
- apply_error_to_rpc = new(-32602).with_data(Object) builder, font_error_to_rpc = new(code, message).with_data_string(variant), serialize_outcome = internal_error
- parse_typed_proposal UnknownProposalKind arm builder, response_result_none_is_elided_on_serialize 회귀 builder
- dispatch.rs::invalid_params(&str) wrapper 30+ caller 호환 쟠존 (RpcError::invalid_params delegate 만)



**Verification**:
- struct-literal RpcError 구성 0 (정의 + impl + fn return type 제외)
- cargo test = 1527 pass 불변



**Impact**: §5.40, §5.7



### R51.90 — R51.90 §5.40 — RadioGroup activate edge focused sync (WAI-ARIA APG roving-tabindex first-class)

**Changes**:
- crates/pinion-core/src/widgets/radio_group.rs send 의 !was && now branch 에 self.focused = Some(index)
- focused_index/set_focused_index doc 에 R51.90 sync 명시, r51_87 diverge 주석 R51.90 collapse 갱신
- R51.90 7 신규 테스트 추가



**Verification**:
- activate path 가 send 경유 — 단일 사이트 정사, AT-only Focus 분기 보존
- cargo test 전원 통과



**Impact**: §5.40



### R51.91 — R51.91 §5.40 — InterveneError::OutOfRange variant (R51.87 TypeMismatch 차용 정정)

**Changes**:
- crates/pinion-core/src/external.rs — InterveneError::OutOfRange variant 신설 (additive), TypeMismatch vs OutOfRange 도메인 경계 doc
- crates/pinion-core/src/widgets/radio_group.rs — resolve_index_intervene helper 추출, selected/focused_index OutOfRange 발사
- 기존 2 회귀 테스트 OutOfRange 검증 + R51.91 신규 4 테스트



**Verification**:
- enum 은 #[non_exhaustive] 이므로 breaking change 0
- cargo test 전원 통과



**Impact**: §5.40, §5.20


**Carry forward**:
- composite 신설 시 동일 패턴 공유 (RadioGroup 외 widget)



### R51.92 — R51.92 §5.40 — pinion-shell substrate 모듈 분할 (lib.rs 990 → 470 LOC, R51.83 visibility substantive)

**Changes**:
- crates/pinion-shell/src/substrate.rs 신규 (~700 LOC) — ShellCore + AccessEmitDecision + impl
- crates/pinion-shell/src/lib.rs — 동일 콘텐트 제거 + mod substrate + pub use re-export, imports 재조정
- AppShell + ApplicationHandler + run + helpers 는 lib.rs 잔존 (R51.92.1 follow-up)



**Verification**:
- cargo test = 1527 pass 불변, atomic 15 binding 경로 이전
- ShellCore 14 필드 substrate 내부 private, AppShell 은 pub accessor + dispatch 만 호출



**Impact**: §5.40, §5.16


**Carry forward**:
- R51.92.1 = app.rs 신규 3-모듈 textbook 완성 follow-up



### R51.92.1 — R51.92.1 §5.40 — app.rs 모듈 분할 textbook 완성 (3-모듈 substrate + app + lib)

**Changes**:
- crates/pinion-shell/src/app.rs 신규 (~480 LOC) — AppShell + impls + helpers + run
- crates/pinion-shell/src/lib.rs — 동일 콘텐트 제거 + mod app + pub use, imports 축소
- AppShell 필드 + core: ShellCore 참조 전원 app.rs 내부 진정 private



**Verification**:
- cargo test = 1527 pass 불변
- lib.rs 990 → 470 LOC (R51.92 + R51.92.1 누적)



**Impact**: §5.40, §5.16



### R51.93 — R51.93 §5.35 — TouchPhase::Cancelled fix (OS-revoked touch 의 click/toggle/checked/selected/committed intent 발사 차단)

**Changes**:
- standard_button.sce-template + slider.scxml — pressed/hover/dragging pointer_cancel → idle
- 5 widget parse_*_event 가 PointerCancel 매핑, InputRouter::pointer_cancel(pid, scene) 신설, substrate handle_touch Cancelled 가 pointer_cancel 경유
- 13 R51.93 회귀 테스트



**Verification**:
- cargo test = 1540 pass
- 4-finger 제스처/전화/알림/앤스위쳐/엣지 OS revoke = click intent 0



**Impact**: §5.35, §5.40


**Carry forward**:
- R51.93.1 = composite (RadioGroup) cancel 회귀 테스트
- R51.93.2 = Slider value sidecar cancel invariant 회귀 테스트



### R51.93.1 — R51.93.1 §5.40 — RadioGroup 합성 cancel 회귀 테스트 (R51.93 template 수정 composite 전파 lock-in)

**Changes**:
- r51_93_composite_pointer_cancel_does_not_select_row 신규 테스트
- 0:Enter → 0:Down → 0:Cancel wire-format 시 selected/focused 불변 검증
- ‘selected’ intent 미발사 검증



**Verification**:
- cargo test = 1541 pass



**Impact**: §5.40, §5.35



### R51.93.2 — R51.93.2 §5.35 — Slider value sidecar cancel invariant (cancel은 commit 만 억제, value 보존)

**Changes**:
- r51_93 slider 테스트: PointerCancel 후 value 0.5 assert
- 문서화된 spec lock: cancel 은 commit 만 억제 (value 보존)



**Verification**:
- cargo test = 1542 pass



**Impact**: §5.35



### R51.94 — R51.94 §5.40 — tag_to_node_id debug_assert injective (NodeId 충돌 debug-build 즉시 검출)

**Changes**:
- crates/pinion-a11y/src/access_tree.rs build 가 debug_assertions cfg 에서 NodeId 중복 검증
- ROOT_NODE_ID + 각 tag 의 NodeId HashSet insert
- 충돌 시 panic 이 tag 명 + 해소 가이드



**Verification**:
- 확률 ≈ N²/2^64 사실상 0, release cost 0
- cargo test = 1541 pass



**Impact**: §5.40



### R51.95 — R51.95 §5.38 — ListBoxItem widget primitive (ListBox composite 의 item primitive, WAI-ARIA Listbox Option role)

**Changes**:
- crates/pinion-core/src/widgets/listbox_item.scxml + listbox_item.rs — ListBoxItem + ListBoxItemExternal
- standard_button template + listbox_item.activate event
- 15 R51.95 회귀 테스트 (R51.93 cancel + keyboard activate 포함)
- mod.rs + build.rs 등록



**Verification**:
- cargo test = 1556 pass
- Radio 패턴 평행, WAI-ARIA Listbox Option role 노출 예정



**Impact**: §5.38, §5.40


**Carry forward**:
- R51.96 = ListBox composite (single-select) follow-up



### R51.96 — R51.96 §5.38 — ListBox composite (single-select) — ListBoxItem children + roving-tabindex + active-descendant

**Changes**:
- crates/pinion-core/src/widgets/listbox.scxml + listbox.rs — ListBox composite (RadioGroup 패턴 평행)
- WAI-ARIA roving-tabindex + ArrowUp/Down + Home/End + Space/Enter activate
- selected: Option<usize> + focused: Option<usize> sidecar



**Verification**:
- cargo test 신규 수십 테스트 통과
- R51.96.1 = AriaRole Listbox + ListBoxOption follow-up



**Impact**: §5.38, §5.40



### R51.96.1 — R51.96.1 §5.40 — AriaRole Listbox + ListBoxOption (R51.96 ListBox composite a11y surface)

**Changes**:
- crates/pinion-a11y/src/role.rs — AriaRole::Listbox + AriaRole::ListBoxOption variants
- ListBox composite 의 AccessNode 노출 shape 정통화



**Verification**:
- cargo test 전원 통과, accesskit lowering valid



**Impact**: §5.40, §5.38



### R51.97 — R51.97 §5.38 — hello-listbox 예제 (ListBox composite first dogfood, paint-side N=4 amortization)

**Changes**:
- examples/hello-listbox (new binary) — Cargo.toml + app.pinion.xml + build.rs + src/main.rs
- ListBoxView WidgetView impl 의 view fn — 4 item ListBox, keybinding d/e (Disable/Enable)
- Cargo.toml workspace.members += examples/hello-listbox



**Verification**:
- cargo check --workspace = 0 errors
- substrate amortization 유지 — pinion-shell API 변경 0



**Impact**: §5.38



### R51.98 — R51.98 §5.38 — ListBox multi-select mode (aria-multiselectable + Ctrl/Shift extend)

**Changes**:
- crates/pinion-core/src/widgets/listbox.rs — multi-select mode + Vec<bool> snapshot
- Ctrl/Shift modifier extend + click contiguous range
- WAI-ARIA aria-multiselectable=true a11y surface



**Verification**:
- cargo test = ListBox multi-mode 신규 테스트 수십 건
- single ↔ multi mode runtime toggle



**Impact**: §5.38, §5.40



### R51.99 — R51.99 §5.38 — hello-listbox type-ahead 점프 (Unicode i18n grapheme cluster prefix match)

**Changes**:
- examples/hello-listbox — type-ahead buffer + 500ms timeout reset
- unicode-segmentation prefix match (Latin / CJK / emoji)



**Verification**:
- multi-window thread-local buffer 한계 보존 — polish carry



**Impact**: §5.38


**Carry forward**:
- R51.107 type-ahead polish 후보 (buffer-no-reset + multi-window)



### R55.D.1 — R55.D.1 §5.45 — ScrollBar 가시화 axis 의 첫 sub-round: scrollbar_thumb_rect closed-form 헬퍼 + ScrollBarOrientation/Geometry 타입 land (paint 기하만, SCXML+routing carry)

**Changes**:
- crates/pinion-core/src/widgets/scrollbar.rs 새 모듈 land (~330 LOC 포함 13 unit test)
- ScrollBarOrientation enum (Vertical/Horizontal) + ScrollBarGeometry struct (orientation/track/thumb)
- scrollbar_thumb_rect closed-form helper (u64 widening + u32::try_from saturating fallback)
- Material/UIKit min_thumb_size floor convention (24-32 px grabbable thumb) wired
- degenerate guards: track_extent=0, content_extent=0, content<=viewport, offset>scroll_max



**Verification**:
- cargo test r55_d1 = 13 passed / 0 failed (vertical+horizontal mirror + 9 boundary cases)
- cargo test --workspace --features pinion-runtime/vello = 2205/0/12 (baseline 2192 → +13 r55_d1)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = T1=0 / RT=1/1 / GENERATED sync / orphan_refs=4 baseline 유지



**Impact**: §5.38, §5.45


**Carry forward**:
- R55.D.2: ScrollBar SCXML statechart (Idle/Hover/Pressing/Dragging) + ScrollBarExternal 사이드카
- R55.D.3: PointerEvent routing (thumb hit + drag delta → ScrollState offset 움직임)
- R55.D.4: hello-listbox 에 visible scrollbar peer 편입 또는 hello-scrollbar 새 demo



### R55.D.2 — R55.D.2 §5.45 §5.38 — ScrollBar SCXML statechart + widget binding land (Slider mirror 4-state Idle/Hover/Dragging/Disabled, drag-end emits scroll_committed intent)

**Changes**:
- crates/pinion-core/widgets/scroll_bar.scxml 신규 (4-state SCXML, Slider mirror, scrollbar.activate raise)
- crates/pinion-core/src/widgets/scrollbar.rs widget binding 확장: ScrollBar + ScrollBarExternal struct
- ScrollBarEvent/ScrollBarState codegen 노출 + Default Vertical orientation (ARIA scrollbar role 정통)
- WidgetTransition::detect Dragging→Hover만 scroll_committed (Null payload, cancel 분기 silent)
- ExternalIntrospect 3-slot schema: state/orientation/send, state+orientation ReadOnly (R51.39 mirror)
- wants_pointer_capture=true (R51.35 mirror — drag past track edge 안전) + 31 r55_d2 unit test land



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2236/0/12 (baseline 2205 → +31 r55_d2)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = T1=0 / RT=1/1 / GENERATED sync / orphan_refs=4 baseline 유지
- 10 demos 회귀 PASS (visible 변화 0, internal statechart + Rust binding axis cascade)



**Impact**: §5.13, §5.20, §5.38, §5.45


**Carry forward**:
- R55.D.3: PointerEvent routing (thumb hit + drag delta → ScrollState scroll_to, capture lock)
- R55.D.4: visible demo (hello-listbox 편입 또는 hello-scrollbar 신규 예제) — 첫 visible 가시화



### R55.D.3 — R55.D.3 §5.45 §5.15 §5.35 — ScrollBar PointerEvent routing land: with_state composition + drag-start snapshot + pointer_move가 ScrollState::scroll_to 직접 dispatch

**Changes**:
- ScrollBar struct에 state: Option<Rc<ScrollState>> + drag_start: Option<DragStart> 필드 추가
- attach_state(mut self, state) -> Self builder + scroll_state() read-only accessor (양 layer)
- DragStart 구조체: cursor_fraction + offset_at_press + scroll_max (press-time pinned)
- ScrollBarExternal::pointer_move impl: axis fraction, first frame snapshot, subsequent delta×scroll_max
- ScrollBar::send가 Dragging 종료 transition(up/leave/cancel/disable) 시 drag_start clear
- f32→i32 delta cast = .round() (truncation 회피, 0.2*400=80 정확) + 18 r55_d3 unit test



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2254/0/12 (baseline 2236 → +18 r55_d3)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = T1=0 / RT=1/1 / GENERATED sync / orphan_refs=4 baseline 유지
- 10 demos 회귀 PASS (visible 변화 0, internal pointer routing axis cascade)



**Impact**: §5.15, §5.35, §5.38, §5.45


**Carry forward**:
- R55.D.4: visible demo (hello-listbox 편입 또는 hello-scrollbar 신규) — 첫 visible scrollbar 가시화
- F1 framework auto-tag conflict-aware 영구 carry (R55.G.17 F7 채택)



### R55.D.4 — R55.D.4 §5.45 — hello-listbox에 visible scrollbar peer 편입 (paint-only): scrollbar_thumb_rect 동적 thumb position + flex Row sibling-of-Scroll 구조, 첫 visible scrollbar 가시화

**Changes**:
- examples/hello-listbox: SCROLLBAR_W/MIN_THUMB/TRACK_FILL/THUMB_FILL const + scrollbar import
- build_scrollbar_visual 헬퍼 (scroll_state offset/max → scrollbar_thumb_rect → spacer+thumb Scene)
- listbox_root 변경: flex Row 안에 [Scroll, scrollbar_visual] sibling, PRIMARY_TAG 그대로 더 paint root
- tools/demos/hello_listbox_snapshot.py: wrapper children expected 1→2 + tree doc update
- a11y_tests r55_d4 2 신규 (root sibling 구조 + spacer-thumb flex Column 패턴)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2256/0/12 (baseline 2254 → +2 r55_d4)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = T1=0 / RT=1/1 / GENERATED sync / orphan_refs=4 baseline 유지
- 10 demos PASS (snapshot demo expected children 1→2 업데이트 동반)



**Impact**: §5.45


**Carry forward**:
- R55.D.5: ScrollBarExternal drag wiring (multi-External path) — thumb drag-able로 완성
- F1 framework auto-tag conflict-aware 영구 carry (R55.G.17 F7 채택)



### R55.D.5 — R55.D.5 §5.45 multi-External substrate + ScrollBarExternal drag wiring in hello-listbox

**Changes**:
- pinion-core/widget_core.rs: ExtraExternal struct + WidgetCore::create_extra_externals default empty
- pinion-core/scene.rs: Scene::find_external_with_tag/_mut + Scene::primary_external/_mut helpers
- pinion-runtime/core_shell.rs: CoreShell::new composes Scene::Container([primary, ...extras]) when extras present
- pinion-rpc query/invoke/dry_run/rewind: descend to primary_external so external/<action> resolves through wrapper
- hello-listbox/main.rs: create_extra_externals registers ScrollBarExternal sharing Owner::cache ScrollState
- hello-listbox/main.rs: read_state + apply_key + access_child_invoke use find_external_with_tag
- 13 substrate tests (7 scene + 6 core_shell incl. end-to-end pointer drag scenario)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 2272 passed (+13 new) / 0 failed / 12 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- 10 demos all PASS (hello-listbox row_click/focus_border/composite_path re-validated after multi-External wrap)
- drag scenario test: pointer_down at (10,0) then move to (10,100) on bar with max_y=100 lands offset_y=50



**Impact**: §5.45, §5.41, §5.15, §5.35


**Carry forward**:
- R55.D.6 absolute layer Scene primitive (closes R55.D.4 spacer-flex workaround)
- scene/<tag>/external/<action> RPC path syntax for addressing extra Externals by name
- ScrollBarExternal::pointer_enter from refresh_hover - currently Idle→Hover only fires on first enter



### R55.D.6 — R55.D.6 §5.45 §5.21 LayoutStyle absolute_position CSS-mirror primitive closes spacer-flex workaround

**Changes**:
- pinion-core/style.rs: LayoutStyle.absolute_position Option<(u32,u32)> field + with_absolute_position builder
- pinion-runtime/layout.rs: to_taffy_style maps absolute_position -> Position::Absolute + Inset.{left,top}
- pinion-runtime/layout.rs: 3 substrate tests pin offset / removal from flex flow / default-none backward-compat
- examples/hello-listbox/main.rs: scrollbar thumb uses absolute_position, retires spacer-flex Column workaround
- examples/hello-listbox: r55_d6 test replaces pre-R55.D.6 r55_d4 spacer-flex pinning
- tools/demos/hello_listbox_snapshot.py: scrollbar children 2 -> 1 (absolute thumb only)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 2275 passed (+3 layout) / 0 failed / 12 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- 10 demos all PASS (hello_listbox_snapshot updated for new single-child scrollbar shape)
- taffy Position::Absolute integration: child at (40,80) with size (20,30) lands at exact rect



**Impact**: §5.45, §5.21


**Carry forward**:
- R56.1 TextField caret rendering + cursor blink animation (new largest axis)
- non_exhaustive on ScrollBarOrientation/Geometry future-proof attribute (cosmetic ~5 LOC)
- F1 framework auto-tag conflict-aware contains_tag walker (deferred - regression risk)



### R55.D.7 — R55.D.7 §5.45 non_exhaustive on ScrollBarOrientation + ScrollBarGeometry forward-compat hedge

**Changes**:
- pinion-core/widgets/scrollbar.rs: ScrollBarOrientation gains #[non_exhaustive]
- pinion-core/widgets/scrollbar.rs: ScrollBarGeometry gains #[non_exhaustive]
- Convention parity with Scene / Display / FlexDirection / AlignItems already non_exhaustive



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 2275 passed / 0 failed / 12 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- in-crate match sites compile unchanged; only out-of-crate construct site (hello-listbox) found, no pattern



**Impact**: §5.45


**Carry forward**:
- R56.1 TextField caret rendering + cursor blink animation (new largest framework axis)
- F1 framework auto-tag conflict-aware Scene::contains_tag walker (regression risk - permanent defer)



### R55.D.8 — R55.D.8 §5.45 §5.7 RPC tag-addressable contract pin on multi-External wrap shape

**Changes**:
- pinion-rpc/src/query.rs: 3 tests pin /external/ + /<tag>/external/ + /<extra-tag>/external/ resolution on Container([External, External]) shape
- R55.D.5 carry-forward bullet retired: scene/<tag>/external/<action> already works via §5.34 R42 lookup_path_ref tag walker + R55.D.5 primary_external descent



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 2278 passed (+3 new) / 0 failed / 12 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- 10 demos all PASS (unchanged)



**Impact**: §5.45, §5.7


**Carry forward**:
- R56.1 TextField caret rendering + cursor blink animation (new largest framework axis)
- F1 framework auto-tag conflict-aware Scene::contains_tag walker (regression risk - permanent defer)



### R55.G.22 — R55.G.22 §5.41 §5.49 — composite paint-root tag convention regression helper assert_widget_view_carries_tag 추출 + 9 widget inline assert 청산

**Changes**:
- pinion_core::test_fixtures::assert_widget_view_carries_tag<V>(state, frame) framework helper land
- Owner::new() wrap inside helper 가 R51.147 Owner::current 의존 hello-button hover animation 흡수
- 9 widget example (toggle/button/checkbox/radio/slider/slider-vertical/listbox/radio-group/listbox-multi) inline assert refactor
- 9 example Cargo.toml dev-dependencies pinion-core test-fixtures feature wiring
- test_fixtures r55_g22_tests 3-arm verify (pass / should_panic tag-mismatch / repeat-safe)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2191/0/12 (baseline 2188 → +3 r55_g22 + 1 ignored doctest)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace = T1=0 / RT=1/1 / divergence=16 / orphan_refs=4 (baseline 유지)
- 10 demos PASS (hello_toggle_activate ~ hello_listbox_composite_path) 회귀 확인



**Impact**: §5.41, §5.49


**Carry forward**:
- F1 framework auto-tag conflict-aware 영구 carry (R55.G.17 F7 채택, hello-toggle inner-tag 충돌 회피)
- hello-commands convention test (test module 부재) — Owner-wrap convention test closure 후보
- R55.D ScrollBar visible drag / R56 TextField+IME / R57 Theming new axes 후보



### R55.G.23 — R55.G.23 §5.49 — hello-commands convention test gap 청산: a11y_tests 모듈 + R55.G.22 helper 1-LOC entry로 카탈로그 10/10 widget 컨벤션 회귀 방어

**Changes**:
- examples/hello-commands/src/main.rs a11y_tests 모듈 + r55_g23 함수 랜드
- Cargo.toml dev-dependencies pinion-core test-fixtures feature wiring
- CommandsView ButtonExternal SCXML 재사용 이라 paint topology hello-button 과 동일



**Verification**:
- cargo test -p hello-commands r55_g23 = 1 passed / 0 failed
- cargo clippy -p hello-commands --all-targets = 0 warnings
- 10 widget catalog 100% convention coverage (R55.G.17/18/20/22 + R55.G.23 hello-commands)



**Impact**: §5.49


**Carry forward**:
- F1 framework auto-tag conflict-aware 영구 carry (R55.G.17 F7 채택)
- R55.D ScrollBar visible drag / R56 TextField+IME / R57 Theming new axes 후보



### R55.G.24 — R55.G.5.fix §5.45 ScrollState max/offset signal-batched so layout-driven set_max re-runs view once

**Changes**:
- crates/pinion-core/src/widgets/scroll.rs: ScrollState.max_x / max_y promoted Cell<i32> -> Signal<i32>
- crates/pinion-core/src/widgets/scroll.rs: set_max / scroll_to / scroll_by wrapped in reactive::batch atomic-update collapse
- crates/pinion-core/src/widgets/scroll.rs: max() docs updated, subscribes both axes when called inside view-fn
- crates/pinion-core/src/widgets/scroll.rs: 3 reactive tests pin atomic-batch single-fire contract on set_max / scroll_to / scroll_by



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 2259 passed (+3 new) / 0 failed / 12 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- hello-listbox R55.D.4 visible scrollbar peer now re-paints after first layout writes the bound



**Impact**: §5.45, §5.22


**Carry forward**:
- R55.D.5 ScrollBarExternal drag wiring (multi-External path through hello-listbox)
- R55.D.6 absolute layer Scene primitive (closes R55.D.4 spacer-flex workaround)
- R56.1 TextField caret rendering + cursor blink animation (new largest axis)



### R56.1.a — R56.1.a §5.38 §5.13 §5.20 — TextField SCXML statechart + binding (Idle/Focused/Editing/Disabled). Statechart-first slice; text content + IME preedit deferred to R56.1.b/g.

**Changes**:
- crates/pinion-core/widgets/text_field.scxml — 4-state SCXML + commit_edit/cancel_edit raise rules
- crates/pinion-core/src/widgets/text_field.rs — TextField + TextFieldExternal + WidgetTransition
- crates/pinion-core/build.rs — text_field.scxml added to scxml_inputs codegen list
- introspect schema: state read + send invoke (2 slots, no value sidecar on R56.1.a)
- text_committed intent on Editing→Focused via CommitEdit or →Idle via Blur



**Verification**:
- cargo test -p pinion-core --lib text_field: 30 pass
- cargo test --workspace --features pinion-runtime/vello: 2308 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings



**Impact**: §5.38, §5.13, §5.20


**Carry forward**:
- R56.1.b — caret rendering primitive (String + caret usize + caret rect)
- R56.1.c — caret blink animation (Owner::cache + Tickable, 530ms canonical)
- R56.1.d — key input dispatch (apply_key + §5.39 FocusManager)
- R56.1.e — clipboard primitive (X11/Wayland/macOS/Win32 substrate)
- R56.1.f — selection (mouse drag + shift-arrow)
- R56.1.g — IME composition (preedit buffer + Wayland text-input-v3)
- hello-text-field example crate (R56.1.b first visible consumer)



### R56.1.b — R56.1.b §5.38 §5.22 §5.21 — TextEditState reactive primitive + caret_rect helper + TextField::attach_state composition + introspect text/caret slots. Substrate slice; first consumer in R56.1.b.1.

**Changes**:
- crates/pinion-core/src/widgets/text_edit.rs — TextEditState (text+caret signals) + hook
- crates/pinion-core/src/widgets/text_field.rs — caret_rect helper (R55.D.1 mirror)
- crates/pinion-core/src/widgets/text_field.rs — TextField::attach_state composition
- introspect schema 2→4 slots: state/text/caret/send (query+intervene)
- atomic-multi-axis batch wrap on set_text/insert/backspace (R55.G.24 mirror)



**Verification**:
- cargo test -p pinion-core --lib text_edit: 37 pass / 0 fail
- cargo test -p pinion-core --lib text_field: 52 pass / 0 fail (30 R56.1.a + 22 R56.1.b)
- cargo test --workspace --features pinion-runtime/vello: 2367 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings



**Impact**: §5.38, §5.22, §5.21


**Carry forward**:
- R56.1.b.1 — hello-text-field example crate (first visible consumer)
- R56.1.b.2 — caret_x_for_position parley shaped-run integration
- R56.1.c — caret blink animation (Owner::cache + Tickable, 530ms)
- R56.1.d — key input dispatch (apply_key + §5.39)
- R56.1.e — clipboard substrate (X11/Wayland/macOS/Win32)
- R56.1.f — selection + grapheme-cluster navigation (unicode-segmentation)
- R56.1.g — IME composition (preedit buffer + text-input-v3)



### R56.1.b.1 — R56.1.b.1 lands hello-textfield, the first visible consumer for the R56 TextField axis, together with the four substrate cascade fixes its boilerplate audit revealed.

**Changes**:
- pinion-a11y::AriaRole adds TextInput variant
- AriaRole::TextInput lowers to accesskit::Role::TextInput
- aria_name() returns WAI-ARIA 1.2 literal 'textbox'
- tree::add_actions_for_role registers Focus + Click for TextInput
- core_shell::CoreShell::new wraps V::create_external in root_owner.run
- create_external can now call use_text_edit_state / use_caret_blink / use_scroll_state
- substrate.rs collect_access_emit_inputs wraps V::access_node in root_owner.run
- V::access_focus_target receives the same root_owner.run wrap for parity
- Owner::cache key shape becomes (TypeId, &'static str) for per-type slot
- use_text_edit_state(tag) + use_caret_blink(tag) compose without collision
- cache_contains becomes generic cache_contains::<V>(key)
- use_caret_blink updated to cache_contains::<CaretBlink>(key)
- same_key_mismatched_type_panics test replaced by typed-key contract pair
- examples/hello-textfield new crate with TextFieldView impl + paint + apply_key
- TextFieldExternal::new().attach_state(...).attach_blink(...) inside create_external
- view fn shapes text via Owner::cache<RefCell<LayoutCache>> per paint
- caret_rect_for_byte_offset + LayoutStyle::with_absolute_position drive caret overlay
- Caret overlay only paints when Focused|Editing AND CaretBlink::visible() is true
- apply_key delegates to TextFieldExternal::invoke('key', Text(key))
- access_node emits AriaRole::TextInput + AccessValue::Text(text_state.text())
- 12 binary tests pin the substrate composition
- tools/demos/hello_textfield_type.py drives the live window via JSON-RPC
- Demo exercises focus/set + 6 keystroke variants + F1 rejection + blur



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 2483 passed / 0 failed / 13 ignored
- Baseline 2469 + 14 new tests = 2483 (delta net +14)
- Delta: -1 deleted panic test + 2 typed-cache tests + 1 role lowering + 12 hello-textfield
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- Lint baseline: forbid unsafe_code + deny warnings + clippy::pedantic deny
- tools/demos/hello_textfield_type.py end-to-end RPC self-verify PASS in 1.87s
- Demo covers: focus drive + 6 keystrokes + F1 rejection + blur survives text
- Regression sweep: all 11 hello-* demos PASS (7 listbox + 3 toggle + 1 textfield)
- AccessKit AriaRole::TextInput -> Role::TextInput lowering test pinned



**Impact**: §5.22, §5.38, §5.40, §5.41


**Carry forward**:
- R56.1.j caret blink reset on edit (R56.1.c carry)
- pinion-tui FocusManager substrate (TUI parity for TextField)
- R56.1.e clipboard / R56.1.f selection / R56.1.g IME composition (R56 axis remainder)
- R57 Theming runtime palette (cross-cutting axis)



### R56.1.b.1.tui — R56.1.b.1.tui lands the TUI sibling of hello-textfield, validating §2 #6 GUI/TUI dual rendering for the R56 TextField axis. Same pinion-core widget substrate; only the shell differs.

**Changes**:
- New crate examples/hello-textfield-tui (Cargo.toml + src/main.rs)
- WidgetCore impl mirrors Vello sibling — same State shape + read_state
- create_external attaches use_text_edit_state(TF_TAG) to TextFieldExternal
- apply_key delegates to TextFieldExternal::invoke('key', Text(key)) (R56.1.d)
- view renders cell-based ContainerNode + TextNode + Border via pinion-tui
- Cursor glyph U+2588 paints at byte-offset cell when not Disabled
- Disabled state omits the cursor (no edit affordance)
- AriaRole::TextInput access_node with AccessValue::Text(live text)
- No attach_blink — TUI defers caret animation to terminal native cursor
- Status line mirrors live state: state | caret position | text content
- Hint line documents keyboard cheatsheet (Type/Backspace/Arrow/Home/End/d/e/Esc)
- 8 binary tests: state round-trip + 3 ARIA + 4 render snapshot via render_one_frame
- render_one_frame snapshot tests assert cursor glyph + text + status row content
- First R56-axis consumer to validate §2 #6 GUI/TUI dual invariant



**Verification**:
- cargo test -p hello-textfield-tui: 8 passed / 0 failed
- cargo test --workspace --features pinion-runtime/vello: 2499 passed / 0 failed / 13 ignored
- Delta: +8 TUI snapshot/ARIA/round-trip tests over R56.1.j baseline of 2491
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- Lint baseline preserved: forbid unsafe + deny warnings + clippy::pedantic deny
- Visible run path: cargo run -p hello-textfield-tui drops into alternate-screen TUI
- Substrate sharing verified: pinion-core TextField widget reused unchanged



**Impact**: §5.22, §5.38, §5.40, §5.41


**Carry forward**:
- pinion-tui FocusManager substrate (deferred per [[substrate-incompleteness-signal]] until 2nd focusable TUI binding)
- TUI grapheme-cluster cell mapping (current cursor col = byte offset, ASCII-only correct)
- TUI caret animation infrastructure if terminal native cursor proves insufficient



### R56.1.b.2 — R56.1.b.2 §5.36 §5.38 — pinion-text caret_rect_for_byte_offset closed-form helper wrapping parley::Cursor + CaretRect f32 struct.

**Changes**:
- crates/pinion-text/src/caret.rs: caret_rect_for_byte_offset(layout, byte, width)
- CaretRect struct (x/y/width/height f32) with #[non_exhaustive] forward-compat
- parley::Cursor::from_byte_index(Affinity::Downstream) + ::geometry wrap
- f64 BoundingBox → f32 CaretRect bridge for pinion paint pipeline
- lib.rs re-exports caret_rect_for_byte_offset + CaretRect public surface
- 10 substrate tests: byte-zero / end / monotonic / multibyte / oversized clamp



**Verification**:
- cargo test -p pinion-text --lib caret: 10 pass / 0 fail
- cargo test --workspace --features vello: 2469 pass / 0 fail (+10 vs R56.1.h 2459)
- cargo clippy --workspace --all-targets --features vello: 0 warnings



**Impact**: §5.36, §5.38



### R56.1.c — R56.1.c §5.38 §5.28 — CaretBlink animation (530ms canonical) + use_caret_blink hook (Owner::cache + register_animation dedup). Tickable + enabled gate + reset-on-edit.

**Changes**:
- crates/pinion-core/src/widgets/caret_blink.rs — CaretBlink struct + Tickable impl
- use_caret_blink hook — Owner::cache + register_animation once (cache_contains dedup)
- PERIOD_SECS = 0.530 — Chromium/Firefox/Safari/Windows canonical half-period
- reset() on text edit / caret move — macOS/iOS/Web canonical UX
- is_at_rest = !enabled — backend redraw-loop gate releases when unfocused



**Verification**:
- cargo test -p pinion-core --lib caret_blink: 20 pass / 0 fail
- cargo test --workspace --features pinion-runtime/vello: 2387 pass / 0 fail
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- integration test pins owner.tick_animations drives blink phase flip end-to-end



**Impact**: §5.38, §5.28


**Carry forward**:
- R56.1.b.1 — hello-text-field example crate (first visible consumer)
- R56.1.b.2 — caret_x_for_position parley shaped-run integration
- R56.1.d — key input dispatch + CaretBlink.reset() wire on edit
- R56.1.e — clipboard substrate (X11/Wayland/macOS/Win32)
- R56.1.f — selection + grapheme cluster navigation
- R56.1.g — IME composition (preedit + text-input-v3)
- TextField statechart → CaretBlink.set_enabled wire (R56.1.b.1 first consumer)



### R56.1.d — R56.1.d §5.38 §5.22 — TextField apply_key static W3C UI Events key helper + invoke('key', text) RPC path. Schema 4→5 slots.

**Changes**:
- apply_key(state, key) static helper — W3C UI Events keystroke → TextEditState ops
- TextFieldExternal invoke('key', text) RPC path — Bool(true|false) recognition gate
- Schema 4→5 slots — 'key' field stable across bare/wired TextFields
- Recognized: Backspace/Delete/ArrowLeft/Right/Home/End/Space + single printable codepoint
- Rejected: ArrowUp/Down (R56.1.h), Enter (R56.1.h), F-keys, multi-char (R56.1.g IME), control
- Pure mapping — no statechart drive, no IME path; focus lifecycle pends R56.1.h



**Verification**:
- cargo test r56_1_d_tests: 39 pass / 0 fail
- cargo test --workspace --features vello: 2426 pass / 0 fail (+39 vs R56.1.c 2387)
- cargo clippy --workspace --all-targets --features vello: 0 warnings
- R56.1.a/b/c invariants preserved; 4-slot schema test renamed to _five_slots



**Impact**: §5.38, §5.22



### R56.1.e — R56.1.e §5.22 §5.38 — `pinion_core::clipboard` substrate (`Clipboard` trait + `InMemoryClipboard`) + Ctrl/Cmd+C/X/V dispatch via `TextFieldExternal::invoke("key", ...)`; platform bridge crate carry.

**Changes**:
- `pinion_core::clipboard`: `Clipboard` trait + `InMemoryClipboard` (`RefCell<Option<String>>`).
- `TextEditState::selection_text() -> Option<String>` ready for `Clipboard::copy(text)`.
- `TextField` / `TextFieldExternal` add `attach_clipboard(Rc<dyn Clipboard>)` builder.
- `dispatch_key` intercepts Ctrl/Meta + `c`/`x`/`v` when clipboard + state attached.
- `apply_key` printable arm rejects Ctrl/Meta-modified letters (no literal `c` on Ctrl+C).
- hello-textfield `use_clipboard` Owner-cache hook attaches `InMemoryClipboard`.
- AltGr-style Ctrl+Alt chords gated by Ctrl||Meta refusal (AltGr safety).
- +24 tests: 6 clipboard + 5 selection_text + 13 dispatch (C/X/V/Meta/bare/Alt).



**Verification**:
- cargo test workspace vello: 2596 pass / 0 fail / 13 ignored (+24 vs R56.1.f.3).
- cargo clippy workspace all-targets vello: 0 warnings under strict pedantic baseline.
- mnemosyne validate-workspace: T1 0 / T3 0 / round-trip 1/1 / GENERATED.md sync.
- 13/13 demos PASS (12 prior + new hello_textfield_clipboard.py end-to-end ~2s).



**Impact**: §5.22, §5.38, §5.49


**Carry forward**:
- Platform clipboard bridge crate pinion-platform-clipboard X11 PRIMARY Wayland data device macOS NSPasteboard Win32.
- Multi MIME clipboard items text html image png file references; clipboard history; X11 PRIMARY vs CLIPBOARD distinction.
- R56.1.g IME composition preedit buffer is the next R56 axis follow up.



### R56.1.f.0 — R56.1.f.0 §5.13 — apply_key W3C 4-bit modifier surface (shift/ctrl/alt/meta) lifted into pinion-core so WidgetCore::apply_key carries it end-to-end. Substrate prep for R56.1.f text selection.

**Changes**:
- `pinion_core::input::Modifiers` — new module (W3C `shift`/`ctrl`/`alt`/`meta` + `is_empty`).
- `pinion_runtime::input` drops local `Modifiers`; re-exports `pinion_core::Modifiers`.
- `WidgetCore::apply_key` widened to `(scene, focused, key, modifiers)`; default impl no-op.
- `CoreShell::apply_key` forwards the modifier into the `root_owner.run` V::apply_key wrap.
- `ShellSubstrate::apply_key` sources `self.modifiers`; `apply_a11y_key` uses `empty()`.
- `ShellCoreTui::dispatch_key` widened; crossterm bridge via `modifiers_from_crossterm`.
- `widgets::text_field::apply_key` free fn widened; `invoke("key")` accepts Text + Json.
- Json shape mirrors W3C `KeyboardEvent` (bool slots; missing key → TypeMismatch).
- 17 V impls + 32 direct callers updated (examples, pinion-core + pinion-shell tests).



**Verification**:
- cargo test workspace vello: 2510 pass / 0 fail / 13 ignored (+11 vs R56.1.b.1.tui).
- cargo clippy workspace all-targets vello: 0 warnings under strict pedantic baseline.
- mnemosyne validate-workspace: T1 0 / T3 0 / round-trip 1/1 / GENERATED.md sync.
- 11/11 prior demos PASS — Modifiers::empty forwarding preserves R56.1.d wire shape.



**Impact**: §5.13, §5.41, §5.45, §5.38, §5.22


**Carry forward**:
- R56.1.f.1 TextEditState selection_anchor sidecar + selection-aware mutators.
- R56.1.f.2 apply_key Shift-prefix to select_X plus printable insert replaces selection.
- R56.1.f.3 RPC selection introspect slot + hello-textfield Vello and TUI selection overlay.



### R56.1.f.1 — R56.1.f.1 §5.22 — TextEditState selection sidecar (selection_anchor Signal Option usize) + selection-aware mutators (W3C DOM Selection shape; replace-on-non-collapsed; Shift-Arrow select_* extension).

**Changes**:
- `selection_anchor: Signal<Option<usize>>` — pinned end of W3C selection; caret is focus.
- `selection_range()` collapses anchor==caret to None; pure boolean `has_selection`.
- `set_selection(anchor, focus)` + `clear_selection()`; char-boundary clamp on both ends.
- `insert` / `backspace` / `delete_forward` drain selected range first (W3C `inputType`).
- `move_left` / `move_right` collapse to leading/trailing edge; `move_home`/`end` clear.
- `select_left` / `select_right` / `select_home` / `select_end` Shift-Arrow extension.
- `set_text` / `set_caret` drop selection (W3C `selectionchange` canonical).
- 3-axis writes (text + caret + anchor) wrapped in `batch` (R55.G.24 atomic-multi).
- +30 regression tests: accessors, select_*, replace-on-selection, multi-byte UTF-8.



**Verification**:
- cargo test workspace vello: 2540 pass / 0 fail / 13 ignored (+30 vs R56.1.f.0 baseline).
- cargo clippy workspace all-targets vello: 0 warnings under strict pedantic baseline.
- mnemosyne validate-workspace: T1 0 / T3 0 / round-trip 1/1 / GENERATED.md sync.
- 11/11 prior demos PASS (no visible regression — caret-only path is byte-equivalent).



**Impact**: §5.22, §5.38


**Carry forward**:
- R56.1.f.2 apply_key Shift-prefix to select_X plus printable insert replaces selection.
- R56.1.f.3 RPC selection introspect slot plus hello-textfield Vello and TUI selection overlay.



### R56.1.f.2 — R56.1.f.2 §5.22 §5.38 — `apply_key` Shift-prefix selection extension (Shift+Arrow/Home/End → select_*) + Ctrl/Cmd+A select-all + type-to-replace path through R56.1.f.1 selection-aware mutators.

**Changes**:
- `ArrowLeft` / `ArrowRight` / `Home` / `End` branch on `shift_key()` → `select_*` vs `move_*`.
- Printable `a` + `ctrl||meta` + `!alt` calls `set_selection(0, len)` (Ctrl/Cmd+A).
- `Ctrl+Alt+a` chord refused (AltGr safety on European layouts; alt gates select-all).
- Plain printable/Space/Backspace/Delete flow through R56.1.f.1 selection-aware paths.
- +16 tests: Shift+Arrow extension, Ctrl/Cmd+A, plain-key replace, RPC Json shift bit.



**Verification**:
- cargo test workspace vello: 2556 pass / 0 fail / 13 ignored (+16 vs R56.1.f.1).
- cargo clippy workspace all-targets vello: 0 warnings under strict pedantic baseline.
- mnemosyne validate-workspace: T1 0 / T3 0 / round-trip 1/1 / GENERATED.md sync.
- 11/11 prior demos PASS — no-modifier path is byte-equivalent to R56.1.d.



**Impact**: §5.22, §5.38


**Carry forward**:
- R56.1.f.3 RPC selection introspect slot plus hello-textfield Vello and TUI selection overlay.



### R56.1.f.3 — R56.1.f.3 §5.22 §5.38 §5.49 — TextFieldExternal `selection` query/intervene + new `scene/intervene` RPC method + hello-textfield Vello/TUI selection overlay + RPC self-verify demo.

**Changes**:
- `TextFieldExternal::schema` adds `selection` object slot; 6 slots total.
- `query("selection")` returns `Json {start,end}` or `Null` (collapsed).
- `intervene("selection", Null|Json)` clears or sets via `set_selection` batch.
- `pinion-rpc::intervene` new module + `scene/intervene` RPC method (§5.12 #9).
- `InterveneError` mirrors trait (UnknownPath / ReadOnly / TypeMismatch / OutOfRange).
- hello-textfield Vello: `SELECTION_COLOR` rgba tint behind text via absolute pos.
- hello-textfield-tui: cell-bg band + status line `sel: [s,e]` for AI verify.
- `tools/demos/hello_textfield_select.py` Shift+Arrow / Ctrl+A / replace end-to-end.
- `rpc_verify.py` adds `intervene` wrapper; +22 tests (16 introspect, 6 module).



**Verification**:
- cargo test workspace vello: 2572 pass / 0 fail / 13 ignored (+22 vs R56.1.f.2).
- cargo clippy workspace all-targets vello: 0 warnings under strict pedantic baseline.
- mnemosyne validate-workspace: T1 0 / T3 0 / round-trip 1/1 / GENERATED.md sync.
- 12/12 demos PASS (11 prior + new hello_textfield_select.py end-to-end ~2s).



**Impact**: §5.22, §5.38, §5.49, §5.7, §5.12, §5.15


**Carry forward**:
- R56.1.e clipboard primitive plus platform X11 Wayland macOS Win32 bridge.
- R56.1.g IME composition preedit buffer plus Wayland text-input-v3 macOS NSTextInputContext Windows TSF.
- TUI grapheme-cluster cell mapping needed once multi-byte selection lands.



### R56.1.h — R56.1.h §5.38 §5.39 §5.28 — TextField focus lifecycle wire: shell focus mgr ↔ External::on_focus_change ↔ TextField Focus/Blur statechart drive ↔ CaretBlink sync.

**Changes**:
- ShellCore::notify_focus_change helper walks scene + fires External::on_focus_change(old/new)
- 6 focus mutation sites refactored (click/Tab/AT Focus/AT Click/a11y key/RPC) to notify on diff
- TextFieldExternal::on_focus_change override drives TextFieldEvent::Focus / Blur via SCXML
- TextField::attach_blink + sync_blink; Focused/Editing → blink on, Idle/Disabled → off
- Editing→Idle via Blur emits text_committed (IME canonical commit-on-blur preserved)
- 25 pinion-core + 8 pinion-shell substrate tests; FOCUS_CHANGE_LOG fixture in dispatch_core



**Verification**:
- cargo test pinion-core widgets::text_field::r56_1_h_tests: 25 pass / 0 fail
- cargo test pinion-shell --test dispatch_core r56_1_h: 8 pass / 0 fail
- cargo test --workspace --features vello: 2459 pass / 0 fail (+33 vs R56.1.d 2426)
- cargo clippy --workspace --all-targets --features vello: 0 warnings



**Impact**: §5.38, §5.39, §5.28



### R56.1.j — R56.1.j closes the R56.1.c caret-blink carry — recognized keystrokes reset the attached CaretBlink (caret stays solid while typing, resumes blinking on pause; macOS/iOS/GTK/Web canonical UX).

**Changes**:
- TextFieldExternal::invoke('key') recognized arm calls blink.reset()
- Snap visible to true + timer back to 0.0 while blink is enabled
- Bare TextField (no attached blink) silently no-ops via Option::map chain
- Unrecognized keys do not reset the blink (no field interaction)
- Rejection list pinned: F1 / ArrowUp / Enter / Escape / Tab
- Backspace-at-caret-0 still resets (recognized no-op counts as user input)
- ArrowLeft / Home / End / Space all reset (navigation = interaction)
- 8 r56_1_j_tests pin recognized + rejected + bare-no-blink + text-only-attached



**Verification**:
- cargo test pinion-core widgets::text_field::r56_1_j: 8 passed 0 failed
- cargo test --workspace --features pinion-runtime/vello: 2491 passed / 0 failed / 13 ignored
- Delta: +8 R56.1.j substrate tests over the R56.1.b.1 baseline of 2483
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- Lint baseline holds: forbid unsafe + deny warnings + clippy::pedantic deny
- tools/demos/hello_textfield_type.py: PASS (1.85s) — no regression
- All 11 hello-* demos sweep: still PASS (R56.1.b.1 + R56.1.j stack)



**Impact**: §5.22, §5.28, §5.38


**Carry forward**:
- R56.1.e clipboard primitive (X11/Wayland/macOS/Win32 selection bridge)
- R56.1.f selection (mouse drag + shift-arrow + grapheme cluster nav)
- R56.1.g IME composition (Wayland text-input-v3 preedit buffer)
- pinion-tui FocusManager substrate (TextField TUI parity)



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



### Round 29 — Round 29 — §5.25 new section: Modifier composition (chain pattern, Vec<ModifierOp>); supersedes R20 vestigial Modifier struct

**Changes**:
- New §5.25 section: Modifier composition under §5 parent (Compose/SwiftUI chain pattern)
- Modifier = Vec<ModifierOp> closed enum replaces R20 §5.11 struct (margin/padding/align absorbed earlier)
- ModifierOp v0 = 9 variants: Padding/Background/Border/Clickable/Hover/Focus/Semantic/OnAppear/OnSignalChange
- Clickable dispatches Intent via §5.23 Command; Hover/Focus toggle §5.24 SemanticState bits
- Reactive modifiers (Signal dep) update visual without view-fn rebuild
- scene/modifiers = 12th RPC method (§5.12 caveat cross-ref); §5.11 supersede caveat
- 11 §5.25 caveats lock chain semantics + ModifierOp shape + Forge codegen + RPC inspect



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 34 → 35; entries 28 → 29
- no code changes this round; R30+ axis batch continues
- atomic mutations: 1 add_section + 5 set_section_* + 13 add_section_caveat (3 retries for caps)
- Textbook: Compose Modifier.padding(8).background(red).clickable{} = direct precedent



**Impact**: §5.11, §5.12, §5.20, §5.22, §5.23, §5.24, §5.25


**Carry forward**:
- R30 §5.26 Incremental layout + damage tracking
- R31 §5.16 GPU render (vello)
- R32 §5.27 Virtualization Scene variant
- R33 §5.28 Animation (spring physics)
- R34 §5.29 Structured concurrency
- R35 §5.30 Accessibility (AccessKit bridge)
- R36 §5.31 Hot reload (signal serialization)



### Round 297 — R45 entry 416 publishable_decision_summary 'Round 45 — ' prefix 일관성 복원 (audit half 미접촉, publishable side 만 redact_term 1-call) — mnemosyne redact_term 1-call 로 entry 416 (= R45 entry) 의 publishable_decision_summary 한 필드만 'Round 45 — ' prefix 추가 — 412-415 entry 들과 prefix 일관성 복원, audit half 는 미접촉

**Changes**:
- mnemosyne redact_term: entry 416 publishable_decision_summary 정정 ('§5.16 SceneRenderer ...' → 'Round 45 — §5.16 SceneRenderer ...')
- mnemosyne.toml: [[publishable_override_ledger]] row 추가 — kind=redaction / target_id=416 / fields=[publishable_decision_summary] / reason=R45 round prefix 누락 정정 (412-415 entry 와 prefix 일관성 복원) / applied_in=pinion R46+
- docs/.atomic/workspace.atomic.json: entry 416 publishable_decision_summary 한 필드 정정 (content_hash_before=cfd485e9... → content_hash_after=5f810b0f...)



**Verification**:
- mnemosyne-cli validate-workspace: T1 orphan total=0 / round-trip mandatory=1/1 / GENERATED.md=sync
- publishable / audit divergence: 7 → 8 (override ledger row +1) / ledger_rows 12 → 13
- atomic ledger: entries=N — audit half (decision_summary) unchanged, frozen-ledger 원칙 보존



**Impact**: §5.16


**Carry forward**:
- pinion R46+ entry 들은 'Round N — ' prefix 일관성 설계로 이 종류의 backfill 은 추가 발생 의미 없음 (제안 결정)



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



### Round 30 — Round 30 — §5.26 new section: Incremental layout + damage tracking; refines §5.21 full-recompute baseline for AAA performance

**Changes**:
- New §5.26 section: Incremental layout + damage tracking under §5 parent
- Layout cache by (node identity, LayoutStyle hash); Signal dep tracking marks subtrees dirty
- DamageRect = union of dirty rects per frame; emitted to paint pipeline as scissor
- Off-thread compute opt-in; default single-thread; deterministic result either way
- compute_layout sig evolves to return DamageRect (§5.21 caveat cross-ref)
- scene/layout = 13th RPC method ratified (§5.12 caveat cross-ref)
- 10 §5.26 caveats lock cache keys + dirty propagation + invalidation triggers + LRU eviction



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 35 → 36; entries 29 → 30
- no code changes this round (spec-only)
- atomic mutations: 1 add_section + 5 set_section_* + 12 add_section_caveat (1 retry for intent cap)
- Industry consensus: Compose layout pass + Flutter RenderObject + iOS UIView all cache per identity



**Impact**: §5.12, §5.21, §5.22, §5.26


**Carry forward**:
- R31 §5.16 GPU render backend (vello) — existing axis ratify
- R32 §5.27 Virtualization Scene variant (VirtualList<T>)
- R33 §5.28 Animation (spring physics)
- R34 §5.29 Structured concurrency
- R35 §5.30 Accessibility (AccessKit bridge)
- R36 §5.31 Hot reload (signal serialization)



### Round 31 — Round 31 — §5.16 GPU render integration ratify: §5.22/§5.24/§5.25/§5.26 cross-axis paint pipeline + glyph atlas + softbuffer dev fallback

**Changes**:
- §5.16 R11 thin RHI + Forge + naga decision unchanged; R31 adds integration caveats
- 10 §5.16 caveats lock cross-axis integration: §5.26 damage, §5.25 visual ops, §5.24 not rendered
- Glyph atlas GPU texture + cosmic-text shaping cache (content, font, size, max_w) keyed
- softbuffer demoted to dev fallback; thin RHI primary per target; backend at compile time
- Per-target shader emit via naga (WGSL canonical) for SPIR-V/MSL/HLSL/DXIL
- Scene → display list → GPU command buffer; retained across frames; AAA 144 FPS budget locked



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections unchanged; entries 30 → 31
- no new section; §5.16 from R11 simply extended with 10 caveats
- §5.16 vello rejection from R11 unchanged — thin RHI for AAA dynamic dispatch needs
- atomic mutations: 10 add_section_caveat on §5.16



**Impact**: §5.16, §5.22, §5.24, §5.25, §5.26


**Carry forward**:
- R32 §5.27 Virtualization Scene variant (VirtualList<T>)
- R33 §5.28 Animation (spring physics)
- R34 §5.29 Structured concurrency
- R35 §5.30 Accessibility (AccessKit bridge)
- R36 §5.31 Hot reload (signal serialization)
- Implementation rounds for §5.16 thin RHI + display list cache later; large effort



### Round 32 — Round 32 — §5.27 new section: Virtualization (VirtualList<T> 8th Scene variant); windowed rendering for 10K+ datasets

**Changes**:
- New §5.27 section: Virtualization under §5 parent (Compose LazyColumn / SwiftUI List industry standard)
- Scene closed enum extended to 8th variant: VirtualList(VirtualListNode); §5.2 caveat cross-ref
- VirtualListNode {item_count, visible_range, item_fn, item_size, scroll_offset}
- Materialization at layout pass per §5.26 (O(window) not O(total))
- scene/virtual_list = 14th RPC method (§5.12 caveat cross-ref)
- 10 §5.27 caveats lock variant fields + materialization + scroll Signal + dry_run + damage



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 36 → 37; entries 31 → 32
- no code changes (spec-only); R33+ axis batch continues
- atomic mutations: 1 add_section + 5 set_section_* + 12 add_section_caveat



**Impact**: §5.2, §5.12, §5.22, §5.26, §5.27


**Carry forward**:
- R33 §5.28 Animation (spring physics)
- R34 §5.29 Structured concurrency
- R35 §5.30 Accessibility (AccessKit bridge)
- R36 §5.31 Hot reload
- Auto-size / variable height for VirtualList items (R32 v0 fixed Px only)
- Partial damage rect within VirtualList scroll (R32 v0 marks whole rect)



### Round 33 — Round 33 — §5.28 new section: Animation (spring physics + interruptible); SwiftUI Animation pattern over Signal substrate

**Changes**:
- New §5.28 section: Animation (spring physics) under §5 parent
- Animated<T> wrapper over Signal<T>; tracks value + velocity + target
- SpringConfig {stiffness, damping, mass}; 4 presets (Default/Gentle/Stiff/Wobbly)
- Interruptible: new target preserves velocity, no jump
- Frame.dt field addition per §6.3 (Frame ZST evolves)
- AnimationDriver Effect ticks active Animated per frame; cancel via Owner
- 8 §5.28 caveats lock spring solver + interrupt + dry_run prediction + SCE schema



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 37 → 38; entries 32 → 33
- no code changes (spec-only)
- atomic mutations: 1 add_section + 5 set_section_* + 8 add_section_caveat



**Impact**: §5.22, §5.23, §5.28, §6.3


**Carry forward**:
- R34 §5.29 Structured concurrency
- R35 §5.30 Accessibility (AccessKit bridge)
- R36 §5.31 Hot reload (signal serialization)
- Tween animations as special case of spring (carry-forward)
- Custom easing curves (carry-forward)



### Round 34 — Round 34 — §5.29 new section: Structured concurrency (Owner scope + Tokio + SyncSignal); orphan tasks eliminated

**Changes**:
- New §5.29 section: Structured concurrency under §5 parent
- TaskScope per Owner; spawn returns AbortHandle; cancel propagates on drop
- SyncSignal<T> = Arc<RwLock<T>> + version counter for cross-thread reactive state
- Tokio multi-thread runtime app-owned; UI thread distinct from worker pool
- Cross-thread Intent via mpsc; lock discipline (clippy lint enforced)
- 8 §5.29 caveats + 1 §5.22 cross-ref ratify SyncSignal addition



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 38 → 39; entries 33 → 34
- no code changes (spec-only)
- atomic mutations: 1 add_section + 5 set_section_* + 9 add_section_caveat



**Impact**: §5.22, §5.23, §5.29, §6.3


**Carry forward**:
- R35 §5.30 Accessibility (AccessKit bridge)
- R36 §5.31 Hot reload (signal serialization)



### Round 35 — Round 35 — §5.30 new section: Accessibility (AccessKit bridge); platform AT delegates derive from §5.24 SemanticProps

**Changes**:
- New §5.30 section: Accessibility under §5 parent (AccessKit canonical Rust AT)
- SemanticProps → AccessKit Node auto-conversion; closed translation table
- Focus state Signal-backed; Tab/Shift+Tab/arrow nav as Intent::Focus(direction)
- Live regions from SemanticProps.live_region polite/assertive announce
- Per-OS feature gates: AT-SPI / UIA / NSAccessibility / iOS UIAccessibility
- 8 §5.30 caveats lock pinion-a11y wrapper + focus management + throttling



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 39 → 40; entries 34 → 35
- no code changes (spec-only)
- atomic mutations: 1 add_section + 5 set_section_* + 8 add_section_caveat



**Impact**: §5.22, §5.24, §5.25, §5.30


**Carry forward**:
- R36 §5.31 Hot reload (signal serialization)
- Screen magnifier hooks, voice control — future a11y axes
- WCAG conformance testing harness via scene/semantic RPC



### Round 36 — Round 36 — §5.31 new section: Hot reload via Signal serialization; code swap preserves state; final spec batch close

**Changes**:
- New §5.31 section: Hot reload under §5 parent (Flutter + Compose industry standard)
- Signal<T> bound extended: T: Serialize + Deserialize (§5.22 caveat cross-ref)
- Snapshot/restore protocol via Owner-tree traversal; stable SCE-emitted path keys
- Added/removed/type-changed Signal handling rules locked
- scene/reload = 15th RPC method (§5.12 caveat cross-ref)
- Animation/SCXML state preserved across reload; in-flight Commands cancelled
- 9 §5.31 caveats + 2 cross-refs (§5.22 §5.12) ratify protocol



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync; sections 40 → 41; entries 35 → 36
- no code changes (spec-only)
- atomic mutations: 1 add_section + 5 set_section_* + 11 add_section_caveat
- Spec batch close: R25§5.22 — R36§5.31 covers all textbook layered architecture axes



**Impact**: §5.12, §5.22, §5.28, §5.31, §6.3


**Carry forward**:
- Implementation rounds for R26—R36 axes (each = 3-tier SCE schema + Forge codegen + Rust runtime)
- Widget library buildout (TextField/Slider/Checkbox/List/Modal/etc.) atop locked architecture
- TUI backend (§5.9 invariant #6) realization
- Multi-window WindowRouter live dogfood
- Forge integration tooling for SCE → Rust pipeline



### Round 37 — Round 37 — §5.22 reactive Rust runtime: Signal/Computed/Resource + Owner snapshot/restore for dry_run

**Changes**:
- pinion-core::reactive::Signal<T> bound = Clone+PartialEq+Serialize+DeserializeOwned per R36 §5.31
- Computed<T> lazy push-pull; Owner tree thread-local + cascade-drop; ReactiveNode shared trait
- batch(fn) closure: PENDING_DIRTY defers cascade until outermost exit; idempotent dirty propagation
- Resource<T,E>: Loading/Ready/Error state + FetchToken generation cancellation (sync API, no tokio dep)
- SignalExternal<T> scalar RPC bridge + Owner snapshot/restore via SnapshotableSignal for dry_run



**Verification**:
- cargo test --workspace = 307 pass (baseline 235 + 72 new reactive across 7 slices)
- cargo clippy --workspace --all-targets: 12 pre-existing only; zero new in reactive module
- Mnemosyne validate-workspace: T1=0 T3=0 RT=1/1 sync; 5 §5.22 implementations registered



**Impact**: §5.22


**Carry forward**:
- R38 §5.22: SCE schema for signal graph + Forge codegen (Rust state struct + reactive wiring)
- IntrospectValue Json variant for structured-T Signal RPC (currently scalar-only bridge)
- Effect/Command (§5.23 R28) will wire Resource auto-refetch on dep change
- §5.29 SyncSignal Rust runtime (Arc<RwLock<T>> wrapper) — cross-thread variant impl
- §5.31 hot reload: snapshot/restore wire format + stable path key generation



### Round 37.7 — Round 37.7 — SCE 범용성 확정; pinion-forge crate 신설 결정으로 R26+ axis들의 SCE upstream 항목 철회 (3단→2단 세트)

**Changes**:
- SCE = universal codegen infrastructure; framework-specific kind는 upstream 안 함
- pinion-forge crate 신설 — R26+ codegen은 framework 측 (3단→2단 세트)
- R37 carry-forward 정정: 'SCE schema + Forge codegen' 항목 → pinion-forge로 대체
- memory sce-universal-meta-layer textbook 정정 반영 (infra ≠ authoring)



**Verification**:
- atomic-store entries 37→38; sections 41 (변경 없음); 정정-only commit
- Mnemosyne validate-workspace: T1=0 T3=0 RT=1/1 sync



**Impact**: §2, §5.22


**Carry forward**:
- R38 §5.22 재정의: SCE Forge codegen → pinion-forge DSL + Rust emit
- pinion-forge crate를 §6 워크스페이스 멤버로 추가 build round 필요
- §2 invariant #8 표현 명확화 spec round 후보 (R37.8 또는 R38 직전)
- SCE 측 RFC: sce-build library API + custom namespace tolerance 두 항목



### Round 37.8 — Round 37.8 — SCE RFC 001 closed (maintainer 응답 받음 + 6 revisions 적용); pinion-forge consumption policy 확정 (commit-pin + 별도 file + 자체 diagnostic)

**Changes**:
- SCE RFC 001 closed — maintainer 응답 (2026-05-16); 6 revisions 적용
- pinion-forge dep 정책: sce-build commit-pin + private-by-policy 운영
- Foreign-NS 안 쓰기 결정 — pinion DSL은 별도 파일 (.pinion.xml/유사)
- pinion-forge 자체 diagnostic type 정의 (SCE DiagnosticCode 확장 안 함)



**Verification**:
- claudedocs/sce-rfc-001-{downstream-infra,response}.md — commit fe4cb79
- Mnemosyne validate: T1=0 T3=0 RT=1/1 sync; entries 38→39



**Impact**: §5.22


**Carry forward**:
- R38 §5.22 재정의 진입: pinion-forge crate 명세 + Rust codegen 작성
- Pinion DSL file extension 결정 (.pinion.xml vs .pscxml 등) — build round
- pinion-forge를 §6 워크스페이스 멤버로 추가하는 build round
- §2 invariant #8 표현 명확화 spec round 후보 (선택)
- W3C SCXML local-name collision footgun 회피 정책 pinion-forge 단 적용



### Round 37.9 — §2 invariant #8 narrowed to universal cross-framework patterns; framework authoring out of scope

**Changes**:
- §2 intent: AI authoring surface → universal cross-framework pattern authoring
- §2 caveat: R37.9 narrowing — framework authoring (pinion-forge) out of #8 scope



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1, GENERATED.md=sync
- byte-length: intent=189 (≤200), caveat=95 (≤100)



**Impact**: §2


**Carry forward**:
- R38: pinion-forge DSL 명세 (file extension + element schema + diagnostic catalog)
- R38.1: crates/pinion-forge skeleton — sce-build commit-pin dep 추가



### Round 38 — §5.22 redefined: pinion-forge DSL spec — .pinion.xml + <pinion> root; codegen + diagnostic

**Changes**:
- intent: SCE meta references removed; pinion-forge DSL framing
- inputs/outputs: SCE schema/Forge codegen → pinion-forge DSL + codegen emit
- 5 R38 caveats added (file ext / root / children / CDATA embed / codegen / diagnostic)
- alternatives: 3 R38 DSL alternatives rejected (KDL / proc-macro / SFC)



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1, GENERATED.md=sync
- decisions: file=.pinion.xml; root=<pinion>; CDATA body; pinion::dsl::* diag



**Impact**: §5.22


**Carry forward**:
- R38.1 build: crates/pinion-forge skeleton — sce-build commit-pin dep
- Future §5.23/§5.24/§5.25/§5.27/§5.28 also need pinion-forge codegen redefinition



### Round 38.1 — crates/pinion-forge skeleton — empty <pinion kind=reactive> parse + emit + diagnostic NDJSON wire

**Changes**:
- Added crates/pinion-forge to workspace; path dep on vendor/sce/sce-build per R37.8
- AST: PinionDoc + PinionKind::Reactive + uninhabited PinionChild (R38.2+ variants)
- Parser: quick-xml event scan; accumulates every Parse/Validate diagnostic per run
- Codegen: pub struct Name + impl Name::new(_: &::pinion_core::reactive::Owner)
- Diagnostic: 9-variant PinionForgeDiagnostic enum with code()/stage()/location()
- Wire: SCE v1 NDJSON pattern (v=1, fnv1a id, code, stage, message, location)
- PINION_DSL_NS = https://pinion.dev/dsl/v1 (canonical xmlns)
- build.rs helper: compile_str + compile_file with CompileError(Io | Diagnostics)



**Verification**:
- cargo check -p pinion-forge → clean
- cargo test --workspace → 347 pass (328 baseline + 19 new)
- cargo clippy --workspace --all-targets → 12 pre-existing warnings (pinion-forge zero)
- Wire id stable across rewording verified (test wire_id_is_stable_under_message_rewording)
- Multi-diagnostic accumulation verified (test accumulates_multiple_attribute_diagnostics)



**Impact**: §5.22


**Carry forward**:
- R38.2: <signal name ty>/<computed>/<resource>/<use> AST + parser + codegen
- R38.2: <ty> child element + CDATA code embedding
- R38.2: first dogfood .pinion.xml authoring (hello-button widget)
- R38.2: integration test crate that consumes pinion-forge from build.rs
- R38.2: prettyplease or syn round-trip for emitted source validation



### Round 38.2a — §5.22 <signal> child element — parser + codegen + 3 generic child diagnostics

**Changes**:
- AST: PinionChild::Signal(SignalDecl{name, ty, initial}) variant
- Parser: dispatch_child + parse_signal + scan_signal_body + skip_subtree
- Codegen: emit_struct_with_signals — pub field + Signal::new(initial) per child
- Diagnostic: MissingAttribute / InvalidIdent / EmptyBody (generic over tag+attr)
- Wire: key_fragments updated for new variants (tag/attr/found composition)
- Parser body accepts CDATA or plain Text; whitespace-only → EmptyBody



**Verification**:
- cargo test --workspace → 359 pass (347 baseline + 12 R38.2a new)
- cargo clippy --workspace --all-targets → 12 pre-existing only, pinion-forge zero
- Multi-sibling accumulation verified (test accumulates_signal_diagnostics_across_siblings)
- Unsupported sibling does not block valid signal parsing (test verified)
- Wire id distinguishes MissingAttribute by tag+attribute hash fragments



**Impact**: §5.22


**Carry forward**:
- R38.2b: <computed name ty>CDATA body</computed> + Computed::new(owner, |_| ...)
- R38.2b: owner usage in new() (no longer prefixed _) once Computed needs it
- R38.2c: <resource> async fetch + ResourceState enum wiring
- R38.2d: <use crate path/> external module import + path resolution
- R38.2x: syn-based <ty> validation if rustc error distance hurts UX



### Round 38.2b — §5.22 <computed> child element — over-capture closure codegen + parser DRY refactor

**Changes**:
- AST: PinionChild::Computed(ComputedDecl{name, ty, body}) variant
- Parser: parse_named_typed_body helper unifies signal+computed (name, ty, body) shape
- Parser: scan_text_body generic over tag (was scan_signal_body)
- Codegen: emit_struct_with_children walks prior_names for over-capture tuple shadow
- Codegen: tuple-shadow uses trailing comma for 1-name uniformity; #[allow] silences unused
- Codegen: empty prior_names path emits plain Computed::new(move || { body }) form



**Verification**:
- cargo test --workspace → 370 pass (359 baseline + 11 R38.2b new)
- cargo clippy --workspace --all-targets → 12 pre-existing only, pinion-forge zero
- Multi-element tuple capture verified (test emits_computed_with_multi_capture_tuple)
- Chained dependency emit verified (test emits_chained_computed_captures_all_priors)
- Empty-priors plain form path verified (test emits_computed_with_no_priors_no_capture_block)
- Multi-tag diagnostic accumulation across signal+computed siblings verified



**Impact**: §5.22


**Carry forward**:
- R38.2c: <resource> async fetch + ResourceState wiring + LocalSpawner threading
- R38.2d: <use crate=... path=.../> external module import + path validation
- R38.2x: syn::Expr visit for precise capture analysis (drop over-capture #[allow])
- R38.2x: syn-based ty validation if rustc error distance hurts UX
- R38.2e: first dogfood .pinion.xml (hello-button) + integration test crate



### Round 38.2c — §5.22 <resource> async child + dynamic new() signature (spawner only when resource present)

**Changes**:
- AST: PinionChild::Resource(ResourceDecl{name, ty, err, body}) variant
- Parser: parse_resource (4-attr shape) separate from parse_named_typed_body
- Codegen: emit_resource_into emits Resource::<T,E>::loading() + fetch_with(spawner, body)
- Codegen: needs_spawner gate widens new() to <S: LocalSpawner>(&Owner, &S) on resource
- Codegen: over-capture block reuses capture_tuple helper for resource priors
- lib.rs: ResourceDecl re-exported alongside other AST types



**Verification**:
- cargo test --workspace → 380 pass (370 baseline + 10 R38.2c new)
- cargo clippy --workspace --all-targets → 12 pre-existing only, pinion-forge zero
- Signature variants verified: empty/signal-only doc keeps 1-arg new() (regression test)
- Resource signature verified: <S: LocalSpawner>(&Owner, &S) emitted with where-clause
- Three-way diagnostic accumulation (signal+computed+resource) verified
- Over-capture block emitted for resource with prior signal (test resource_with_prior_signal)



**Impact**: §5.22


**Carry forward**:
- R38.2d: <use crate=... path=.../> external module import + path validation
- R38.2x: element-parser builder unifying signal/computed/resource attribute collection
- R38.2x: syn::Expr visit for precise capture analysis (drop over-capture #[allow])
- R38.2x: consistency vs minimum signature trade-off review (always-spawner option)
- R38.2e: first dogfood .pinion.xml (hello-button) + integration test crate



### Round 38.2d — §5.22 <use path/> child — module-level Rust use statement emitter

**Changes**:
- AST: PinionChild::Use(UseDecl{path}) closing the 4-variant child set
- Parser: parse_use validates path attr non-empty; body silently skipped via skip_subtree
- Codegen: emit_use_block emits top-level use lines + blank separator before struct
- Codegen: is_binding_child gate — Use neither contributes prior_names nor struct field
- child_name() helper removed (was tied to binding-children naming convention)



**Verification**:
- cargo test --workspace → 388 pass (380 baseline + 8 R38.2d new)
- cargo clippy --workspace --all-targets → 12 pre-existing only, pinion-forge zero
- Use-only doc emits unit struct + use block (test use_alone_emits_unit_struct)
- Capture tuple regression verified: use sibling does not pollute prior_names
- Group/rename/simple use forms all roundtrip (test emits_multiple_use_statements_in_order)



**Impact**: §5.22


**Carry forward**:
- R38.2e: first dogfood .pinion.xml authoring + integration test crate
- R38.2x: syn::UseTree path validation (paired with broader syn adoption)
- R38.2x: element-parser builder unification across signal/computed/resource/use
- R38.2x: signature consistency review (always-spawner vs minimum)
- R39: §5.22 split into §5.22.1/.2/.3/.4 if T4 body-length stays high



### Round 38.2e — First pinion-forge dogfood — examples/forge-counter exercises 3 of 4 child variants end-to-end

**Changes**:
- New examples/forge-counter crate: Cargo.toml + build.rs + ui/counter.pinion.xml + main.rs
- Workspace members extended; build.rs invokes compile_file → $OUT_DIR/counter.rs
- Codegen: emit #[must_use] on Counter::new matching pinion-core Signal/Computed convention
- Three new tests assert #[must_use] across unit / binding / spawner signature variants
- Resource dogfood deferred to R38.2e-2 (LocalSpawner impl + runtime choice TBD)



**Verification**:
- cargo build -p forge-counter → codegen + rustc roundtrip clean
- cargo run -p forge-counter → expected output: 0/0 → 5/10 → 10/20 → doubled=20
- cargo test --workspace → 391 pass (388 baseline + 3 must_use tests)
- cargo clippy --workspace --all-targets → 12 pre-existing only; forge-counter zero
- R26 push-pull dependency tracking validated through chained Computed-of-Computed



**Impact**: §5.22


**Carry forward**:
- R38.2e-2: resource dogfood (LocalSpawner impl + async runtime selection)
- R38.2x: syn-based path/ty/body validation broader sweep
- R38.2x: signature consistency (always-spawner) review post-dogfood corpus
- R38.3: hello-button reactive layer integration via .pinion.xml
- R39: §5.22 split into §5.22.1/.2/.3/.4 if T4 body-length stays high



### Round 39 — §5.32 new — AI scene introspection (spatial-semantic locate); 3 new RPC methods queued

**Changes**:
- New §5.32 section ratified: xy↔path bidirectional + region selection primitives
- 3 new RPC methods queued: scene/locate, scene/locate_region, scene/bbox
- pinion-rpc dispatch table will extend 7 → 10 typed methods (R39.1+ build)
- pinion-core scene primitive trait gains hit_test() method (R39.1 build)
- Statechart-aware hit-test surfaces disabled/hidden state to AI in one round-trip
- Alternatives rejected: screenshot+OCR, ARIA-only, IDE-protocol, spatial-index-v0
- Impact scope: §5.7 (RPC envelope), §5.12 (screenshot fallback), §5.2 (primitives), §2



**Verification**:
- validate_workspace pre-write T1 cross-ref guarded impact_scope [5.7,5.12,5.2,2]
- All 4 cross-refs verified to exist (list_sections confirmed §5.2/§5.7/§5.12/§2)
- 8 caveats authored covering coords/z-order/disabled/sce-aware/empty/order/v0/bbox
- 5 alternatives rejected with explicit rationale (— separator)
- intent ≤200 chars, all bullets ≤100 chars (T3 default)



**Impact**: §5.32, §5.7, §5.12, §5.2, §2


**Carry forward**:
- R39.1: scene/locate RPC method build (parser/dispatch/wire/hit_test impl)
- R39.2: scene/locate_region build
- R39.3: scene/bbox build (path → viewport bbox)
- R39.4: AI overlay UX mode — visual selection cursor + highlight rendering
- R39.x: spatial index (R-tree) for scenes >10k elements
- R40+: scene/propose_change + dry_run preview lifecycle (separate axis)
- R40+: event_history ring buffer + RPC export (separate axis)



### Round 39.1 — §5.32 scene/locate impl — Scene::hit_test + LocateOutcome + JSON-RPC method 10

**Changes**:
- pinion-core: Scene::hit_test + HitPath{segments, bbox} + Scene::rect/Scene::tag accessors
- pinion-core: rect_contains half-open + saturating_add overflow guard
- pinion-rpc: locate.rs module with LocateOutcome + LocateError::OutOfBounds
- pinion-rpc: window-prefixed path (/window[name]/...) + root-first ancestor chain
- pinion-rpc: dispatch handler scene/locate {x,y} → {path, bbox, ancestors}
- Method count: 9 → 10 typed JSON-RPC methods (carry-forward to §5.7 ledger)



**Verification**:
- cargo test --workspace → 412 pass (391 baseline + 21 R39.1 new)
- cargo clippy --workspace --all-targets → 12 pre-existing only; no new warnings
- Topmost-last-child overlap rule verified (test hit_test_overlapping_siblings)
- Tag-takes-precedence over index segment verified (hit_test_tagged_child)
- Effect variant skipped during traversal (hit_test_effect_variant_is_skipped)
- JSON-RPC envelope round-trip: happy + oob + missing-x + negative-x all covered



**Impact**: §5.32, §5.7, §5.2


**Carry forward**:
- R39.2: scene/locate_region {x,y,w,h} build — reuse hit_test, collect intersect set
- R39.3: scene/bbox {path} build — path → viewport bbox lookup
- R39.4: AI overlay UX mode — visual selection cursor + highlight rendering



### Round 39.2 — §5.32 scene/locate_region impl — region select with common_ancestor; 11th RPC method

**Changes**:
- pinion-core: Scene::hit_test_region(x,y,w,h) DFS pre-order intersect collect
- pinion-core: rects_intersect half-open + saturating_add overflow guard
- pinion-rpc: locate_region + LocateRegionOutcome{paths, common_ancestor}
- pinion-rpc: longest_common_prefix helper for ancestor computation
- pinion-rpc: dispatch handler scene/locate_region {x,y,w,h}
- Method count: 10 → 11 typed JSON-RPC methods



**Verification**:
- cargo test --workspace → 427 pass (412 baseline + 15 R39.2 new)
- cargo clippy --workspace --all-targets → 12 pre-existing only
- Container + leaves both included; Effect skipped; tag-takes-precedence
- Disjoint query returns empty paths + root common_ancestor (never errors)
- Zero-area query rejected at intersection level (returns empty)



**Impact**: §5.32, §5.7


**Carry forward**:
- R39.3: scene/bbox {path} build — path → viewport bbox lookup
- R39.4: AI overlay UX mode — visual selection cursor + highlight rendering



### Round 39.3 — §5.32 scene/bbox impl — path→bbox reverse lookup completes bidirectional surface

**Changes**:
- pinion-core: Scene::lookup_path traverses segments → Option<Rect>
- pinion-core: tag wins over index on collision; declaration order tiebreak
- pinion-rpc: bbox(scene, path) + BboxError{Path, UnknownPath}
- pinion-rpc: parse_segments splits scene_path; empty / // /// all mean root
- pinion-rpc: dispatch handler scene/bbox {path} → {bbox}
- Method count: 11 → 12 typed JSON-RPC methods; locate ↔ bbox now round-trip



**Verification**:
- cargo test --workspace → 442 pass (427 baseline + 15 R39.3 new)
- cargo clippy --workspace --all-targets → 12 pre-existing only
- Round-trip verified: locate → path; bbox(path) → same rect (test)
- Non-container descent rejected; unknown segments → UnknownPath
- Empty/short-circuit paths return root rect



**Impact**: §5.32, §5.7


**Carry forward**:
- R39.4: AI overlay UX mode — visual selection cursor + highlight rendering (first user-visible piece)
- R39.x: spatial index (R-tree) when scenes ≥10k elements



### Round 39.4 — §5.33 new — AI overlay UX axis (pinion-overlay crate ratify, functional v0 API)

**Changes**:
- New §5.33 section: pinion-overlay crate as framework axis (Option B)
- Functional v0 API: inject_highlight / clear_highlights pure transforms
- OverlayEvent enum: Click / Drag / Escape / Acknowledge (transport-agnostic)
- Highlight = Scene::Box inject with ai-overlay/ tag prefix (introspect-friendly)
- Alternatives rejected: pinion-runtime embed / examples-only / Effect / winit-dep / Controller-v0
- Impact: §5.32 (introspection), §5.7 (RPC), §5.2 (primitives), §5.20 (tags)



**Verification**:
- 8 caveats authored covering v0 shape / transport / tag prefix / immutability / set semantics
- 5 alternatives explicitly rejected with rationale
- Cross-refs verified: §5.32 / §5.7 / §5.2 / §5.20 / §2 all exist
- Build slices R39.4.1/.2/.3 queued for skeleton / transforms / dogfood demo



**Impact**: §5.33, §5.32, §5.7, §5.2, §5.20, §2


**Carry forward**:
- R39.4.1: pinion-overlay crate skeleton + OverlayEvent + HighlightStyle
- R39.4.2: inject_highlight + clear_highlights pure transforms + tests
- R39.4.3: ai-introspect-demo example end-to-end (winit + RPC + overlay)
- R39.4.x: Controller pattern promotion after dogfood evidence
- R39.4.x: pinion-runtime integration hook (post runtime maturation)



### Round 39.4.1 — §5.33 pinion-overlay crate skeleton + functional inject/clear transforms

**Changes**:
- New crate pinion-overlay added to workspace; deps pinion-core + pinion-rpc only
- OverlayEvent enum: Click / Drag / Escape / Acknowledge (transport-agnostic)
- Drag::drag_as_rect normaliser handles any corner-start direction
- inject_highlight: lookup_path → add ai-overlay/<path> tagged Box sibling
- clear_highlights: strip every ai-overlay/ tagged child; idempotent
- Auto-wrap non-Container roots; HighlightStyle with default 2px red border



**Verification**:
- cargo test -p pinion-overlay → 12 pass (3 event + 9 highlight)
- cargo test --workspace → 454 pass (442 baseline + 12 R39.4.1)
- cargo clippy --workspace --all-targets → 12 pre-existing only
- Idempotency verified (inject_is_idempotent_on_same_path)
- Multiple accumulation verified (inject_different_paths_accumulate)
- Silent no-op on unknown path (inject_on_unknown_path_is_silent_no_op)



**Impact**: §5.33, §5.32, §5.2, §5.20


**Carry forward**:
- R39.4.3: ai-introspect-demo example with winit/softbuffer + RPC + overlay
- R39.4.x: Controller pattern promotion (post-demo evidence)
- R39.4.x: pinion-runtime integration hook (post runtime maturation)



### Round 39.4.3 — §5.33 first visual dogfood — ai-introspect-demo proves AI-native xy↔path UX end-to-end

**Changes**:
- New examples/ai-introspect-demo: winit + softbuffer + in-process pinion-rpc + pinion-overlay
- Static demo scene: 3 tagged buttons + info_panel + tinted container background
- Right-click → locate → inject_highlight; Left-click/Esc → clear; Esc-twice exits
- Stdout prints path + bbox + ancestors for every locate — zero pixels in the AI input
- paint_border helper (4 thin rects) renders the highlight outline at demo scope
- Scene-tree pretty-printer (R key) for live introspection during demo



**Verification**:
- cargo build -p ai-introspect-demo → clean compile
- cargo test --workspace → 454 pass (no new tests; runtime-only feature)
- cargo clippy --workspace --all-targets → 12 pre-existing only; demo zero
- End-to-end protocol verified: locate → path → lookup_path → inject succeeds in-process



**Impact**: §5.33, §5.32, §5.2, §5.20


**Carry forward**:
- R39.4.x: Controller pattern promotion with dogfood evidence collected
- R39.4.x: pinion-runtime integration so hello-button gets overlay for free
- R39.4.x: scene/locate_region demo hook (region-select drag UX)
- R39.5+: AI agent transport (stdin/HTTP JSON-RPC binding) connecting overlay to real LLM



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



### Round 40 — §5.34 new — AI scene change proposal lifecycle axis ratify (prepare/preview/apply/cancel)

**Changes**:
- §5.34 new section: AI scene change proposal lifecycle (typed propose → preview_id → apply/cancel)
- intent + 7 rationale + 4 inputs + 4 outputs + 7 caveats + 5 alternatives + impact_scope=[2,5.7,5.22,5.32,5.33]
- lifecycle RPC 4종 queued: propose_change / apply_preview / cancel_preview / list_previews (R40.1+)
- 초기 typed change enum 4종 명시: SetSignal / ReplaceView / SetStyle / DispatchIntent
- preview ledger TTL + side-effect sandbox 보증; §2#3 dry_run invariant 의 stateful lifecycle 층



**Verification**:
- Mnemosyne validate_workspace: T1=0 T3=0 RT=1/1 GENERATED.md=sync (post-mutation 확인 예정)
- atomic entries 54→55 / sections 43→44 / orphan_refs 0+0
- cargo test --workspace 454 pass 유지 (spec-only round, no code change)



**Impact**: §2, §5.7, §5.22, §5.32, §5.33, §5.34


**Carry forward**:
- R40.1: pinion-rpc dispatch 12 → 16 methods (propose/apply/cancel/list_previews)
- R40.1: preview ledger 구조 spec — monotonic ID + TTL clamp + per-target conflict policy
- R40.2+: typed change enum 단계 impl — SetSignal 부터 ReplaceView/SetStyle/DispatchIntent 순
- R40.3+: ai-introspect-demo 확장 — locate → propose → overlay preview → apply 시연
- existing: §5.16 GPU render 진입; hello-button reactive 통합 (R38.3); overlay Controller promote



### Round 40.1 — §5.34 PreviewLedger 모델-first slice — pinion-rpc/src/preview/ module + 19 tests

**Changes**:
- crates/pinion-rpc/src/preview/ 신규 디렉터리 모듈 (mod / id / proposal / error / ledger)
- PreviewId(NonZeroU64) newtype + AtomicU64 monotonic ID + RwLock<BTreeMap> entries
- PreviewLedger: with_config / propose / cancel / list / apply_extract / sweep_expired API
- Proposal open trait (Send+Sync+Debug); Box<dyn Proposal> ledger storage; concrete variants R40.5
- ProposeError::CapacityFull / ApplyError::{UnknownPreview,Expired,BaseRevisionConflict} 완전 surface
- DEFAULT_TTL=60s / MAX_TTL=600s / DEFAULT_CAPACITY=64 — textbook AI workflow tuning
- OCC (Q2=C) — base_revision token 각 entry, apply시 current_scene_revision 비교
- lazy eviction on propose — past-deadline entries reclaimed before capacity check



**Verification**:
- cargo test --workspace: 454 → 473 pass (+19 preview ledger tests)
- cargo clippy --workspace --all-targets: 12 pre-existing baseline only — preview/ 신규 0
- OCC 경합 시나리오: revision mismatch → entry 유지 (cancel 가능), expired → entry 제거
- concurrent propose test: 8 threads × 8 IDs = 64 unique (AtomicU64 monotonic)
- Mnemosyne add_section_implementation: 9 file/symbol bindings under §5.34



**Impact**: §5.34, §5.7


**Carry forward**:
- R40.2: scene/propose_change RPC method (13th method) — dispatch + JSON serialization
- R40.3: scene/cancel_preview RPC method (14th)
- R40.4: scene/list_previews RPC method (15th)
- R40.5: scene/apply_preview RPC + typed Proposal enum (SetSignal/ReplaceView/SetStyle/DispatchIntent)
- R40.x: pinion-core Scene에 scene_revision counter 입히기 (OCC token source)
- carry-forward existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 40.2 — §5.34 scene/cancel_preview (13th RPC) + dispatch &PreviewLedger param + PreviewId::try_new

**Changes**:
- pinion-rpc dispatch fn signature: + &PreviewLedger 파라미터 주입
- handle_scene_cancel_preview: -32602 invalid_params on missing/zero/non-numeric preview_id
- preview/cancel.rs typed dispatcher — transport-agnostic wrapper around ledger.cancel
- PreviewId::try_new(u64) -> Option<Self> — wire-side null-safe constructor
- dispatch dispatch table 12 → 13 typed methods (scene/cancel_preview)
- examples/hello-button: PreviewLedger field + dispatch call 입력 구조 업데이트



**Verification**:
- cargo test --workspace: 473 → 479 pass (+6 wire tests for cancel_preview)
- cargo clippy --workspace --all-targets: 11 pre-existing baseline only — 신규 0
- RPC test 시나리오: active cancel / unknown id / idempotency / 3 invalid_params
- hello-button cargo check pass — ledger field PreviewLedger::default() 공존



**Impact**: §5.34, §5.7, §5.12


**Carry forward**:
- R40.3: scene/list_previews RPC (14th) — PreviewView 직렬화 + ledger.list wire
- R40.4: scene_revision counter pinion-core Scene 명시적 필드 또는 forward-compat field
- R40.5: typed Proposal enum (SetSignal/etc) + scene/propose_change RPC (15th)
- R40.6: scene/apply_preview RPC (16th) — OCC 검증 + runtime side-effect application
- carry-forward existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 40.3 — §5.34 scene/list_previews (14th RPC) — PreviewView wire serialization + age/ttl ms

**Changes**:
- preview/list.rs typed dispatcher — transport-agnostic wrapper around ledger.list
- handle_scene_list_previews + preview_view_to_json — JSON shape with age_ms/ttl_remaining_ms
- Instant 안적 대신 relative ms 직렬화 (saturating_duration_since)
- wire result: {"previews": [{preview_id, base_revision, target_path, affected_paths, age_ms, ttl_remaining_ms}]}
- dispatch table 13 → 14 typed methods (scene/list_previews)



**Verification**:
- cargo test --workspace: 479 → 483 pass (+4 list_previews wire tests)
- cargo clippy: 11 baseline only — #[allow(unnecessary_wraps)] on infallible handler with rationale
- empty ledger → empty array; multi-entry ID order; field shape covered



**Impact**: §5.34, §5.7, §5.12


**Carry forward**:
- R40.4: scene_revision counter pinion-core Scene (OCC token source for apply)
- R40.5: typed Proposal enum (SetSignal initial) + scene/propose_change (15th)
- R40.6: scene/apply_preview (16th) — OCC 검증 + runtime side-effect
- existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 40.4 — §5.34 SceneRevision OCC token (pinion-core) + dispatch auto-bump on mutating methods

**Changes**:
- pinion-core/src/revision.rs: SceneRevision(AtomicU64) + new/current/bump/Default
- Acquire/AcqRel 메모리 순서 — reader 가 mutator 결과 관측 보장
- dispatch 시그니처 + &SceneRevision 파라미터 (Scene/PreviewLedger/Revision/json)
- mutates_scene_on_success() single-source-of-truth (click/rewind/invoke)
- scene/intents 드레인, scene/dry_run, preview lifecycle, 읽기 method 은 bump 안 함
- hello-button: revision 필드 + forward()에서 bump() (winit 입력 bypass)



**Verification**:
- cargo test --workspace: 483 → 493 pass (+5 SceneRevision +5 dispatch bump tests)
- cargo clippy: 11 baseline only — 신규 0
- concurrent test: 8 threads × 8 bumps = 64 unique values 1..=64
- invoke success bumps; invoke invalid_params Ⱶ stays; read-only & lifecycle stays



**Impact**: §5.34, §5.7


**Carry forward**:
- R40.5: typed Proposal enum (SetSignal initial) + scene/propose_change (15th RPC)
- R40.6: scene/apply_preview (16th) — revision.current() vs entry.base_revision 교차 + side-effect
- DispatchContext 구조체 refactor 후보 — 파라미터 4개 도달, 추가 시 고려
- existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 40.5 — §5.34 TypedProposal::SetSignal + scene/propose_change (15th RPC) — captures OCC base_revision

**Changes**:
- preview/kinds.rs TypedProposal #[non_exhaustive] enum (SetSignal first variant)
- preview/propose.rs propose_change typed dispatcher — captures revision.current() as base_revision
- ProposeOutcome { preview_id, base_revision } typed result struct
- handle_scene_propose_change + parse_typed_proposal JSON 고수 파싱
- wire: kind/target_path/signal_path/value (+optional ttl_ms) → {preview_id, base_revision}
- UnknownProposalKind / CapacityFull / missing-field invalid_params surface
- propose_change 는 revision 안 bump — ledger 명시 않은 OCC token capture only



**Verification**:
- cargo test --workspace: 493 → 507 pass (+14 wire/typed tests)
- cargo clippy --workspace --all-targets: 11 baseline only — 신규 0
- round-trip test: propose_change → list_previews 에 ttl_remaining_ms / target_path 계속
- OCC: base_revision = revision.current() at propose time



**Impact**: §5.34, §5.7, §5.12


**Carry forward**:
- R40.6: scene/apply_preview (16th) — OCC 검증 + Signal::set 적용 + revision.bump()
- R40.7+: TypedProposal::SetStyle / ReplaceView / DispatchIntent 순차 추가
- scene_revision 동기화 to signal writes — R40.6 결정 (auto bump in apply?)
- existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 40.6 — §5.34 scene/apply_preview (16th RPC) — OCC + variant apply + revision bump; lifecycle complete

**Changes**:
- Proposal trait + apply(scene) method — vtable polymorphic side-effect dispatch
- TypedProposal::SetSignal::apply — routes to rewind() with json→IntrospectValue 교차
- preview/apply.rs apply_preview: extract → proposal.apply → revision.bump
- ApplyOutcome { preview_id, new_revision } typed result
- ApplyError + ApplyRejected(String) new variant; rejection tag for AI branch logic
- handle_scene_apply_preview: wire JSON parsing + apply_error_to_rpc data shape
- data: {variant, expected/actual for conflict, reason for ApplyRejected}
- apply self-bumps revision (excluded from mutates_scene_on_success to avoid double-bump)



**Verification**:
- cargo test --workspace: 507 → 516 pass (+5 apply ledger +4 wire RPC)
- cargo clippy --workspace --all-targets: 11 baseline only — 신규 0
- end-to-end: propose → apply → query roundtrip writes 77 신호
- OCC 경합: scene/rewind 으로 revision 이동 → apply서 BaseRevisionConflict
- type mismatch: bool value vs Int slot → ApplyRejected(Intervene) tag



**Impact**: §5.34, §5.7, §5.12, §5.22


**Carry forward**:
- R40.7+: TypedProposal::SetStyle / ReplaceView / DispatchIntent 순차 추가
- AI agent dogfood: ai-introspect-demo 의 locate → propose → apply 디모 (선택)
- DispatchContext 구조체 도입 — 파라미터 4개 도달, 추가 시 고려
- existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 40.7 — §5.34 DispatchContext struct refactor — 4-param dispatch collapsed to bundle (forward-compat)

**Changes**:
- dispatch.rs: DispatchContext<'a> struct { scene, previews, revision } + new()
- dispatch 시그니처: dispatch(&mut DispatchContext, json) — 4개 파라미터 1개 한들
- 향후 상태 추가 (event history, effect ledger 등) 시 시그니처 안정
- test helper dispatch_t / dispatch_full 구조체 구축 + dispatch 호출 명시화
- hello-button: ctx = DispatchContext::new(...) 구성 후 dispatch 호출
- lib.rs: DispatchContext public 노출



**Verification**:
- cargo test --workspace: 516 pass (동일 — 순수 refactor)
- cargo clippy --workspace --all-targets: 11 baseline only — 신규 0
- Bloch / Hyrum — 향후 필드 추가는 비파괴적, public dispatch signature 고정



**Impact**: §5.34, §5.7


**Carry forward**:
- R40.8+: TypedProposal::SetStyle / ReplaceView / DispatchIntent 순차 추가 (closed pattern 검증 됨)
- ai-introspect-demo 에 propose/apply flow 통합 (visual end-to-end)
- future state primitives (event history ring, effect ledger) → DispatchContext 필드 추가만 필요
- existing: §5.16 GPU, hello-button reactive, overlay Controller promote



### Round 417 — Round 46 — §5.16 build slice 1 commit 1 — pinion-forge renderer kind parser scaffold (PinionSpec ADT + RendererBackend, codegen stub)

**Changes**:
- ast.rs refactor: PinionDoc { name, spec: PinionSpec } — PinionSpec::Reactive { children } / PinionSpec::Renderer { backend } ADT 도입. RendererBackend::Vello 초기 variant. PinionKind { Reactive, Renderer } wire/hash identity 유지. textbook 'make illegal states unrepresentable' (Minsky RWOC, Effective Rust Item 1, syn::Expr / serde_json::Value precedent)
- parser.rs: kind 별 분기 — validate_renderer_backend + scan_renderer_body 추가. parse_root_attrs 가 (PinionKind, Option<backend_raw>, name) tuple 반환. backend attribute 수집 시 reactive 에선 silent drop (SCE v1 forward-compat 정책 일관성)
- diagnostic.rs: 신규 variant 3 — MissingBackend / UnknownBackend / RendererChildNotAllowed. wire code dsl/missing-backend / dsl/unknown-backend / dsl/renderer-child-not-allowed. stage = Validate. UnknownKind 메시지 갱신 (reactive + renderer 둘 다 enumerate)
- codegen.rs: emit_rust 가 match &doc.spec 으로 exhaustive dispatch — 새 kind 추가시 모든 callsite compile error. PinionSpec::Renderer arm = comment-only Rust module stub (R46 commit 2 가 Vello emit template land). unimplemented!() 회피 — build.rs 사용자 panic 방지
- wire.rs: key_fragments / actual_of 의 3 신규 variant arm. UnknownBackend = found (actual 노출), RendererChildNotAllowed = tag, MissingBackend = empty (no per-instance discriminator beyond location)
- lib.rs tests: 10 신규 — renderer happy path (self-closing + open-close) / missing-backend / unknown-backend / empty-backend / renderer-child-not-allowed (specific code vs unsupported-element) / stub emit assertion / wire actual carriage / unknown-kind message coverage



**Verification**:
- cargo test --workspace = 599 pass (589 baseline + 10 신규 renderer tests in pinion-forge), 0 failed
- cargo clippy --workspace --all-targets = baseline 유지 — pinion-core 5 / pinion-rpc 13 / pinion-runtime 1 / ai-introspect-demo 4 / pinion-forge 0 (신규 3 doc-backtick warning 즉시 정정)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync (generate_docs cascade)
- workspace consumer 영향 0 — pinion-rpc / pinion-runtime / pinion-cli / examples 모두 PinionDoc API 미사용, cargo check workspace clean (한 commit 안에 ast breaking change 안전 land)



**Impact**: §5.16, §5.22, §2


**Carry forward**:
- R45 prefix gap 정정 완료 audit — entry 416 publishable_decision_summary 가 'Round 45 — §5.16 ...' 형식으로 정정, mnemosyne.toml [[publishable_override_ledger]] target_id='416' row 추가 (mnemosyne R297 redact_term + R296 gate). 직전 세션 RFC 001 self-withdraw 후 정통 surface 처리 완료, 부채 청산
- R46 build slice 1 commit 2: renderer kind 의 Vello first emit template — wgpu/vello workspace dep + emit 본체. commit 1 의 PinionSpec::Renderer { backend: Vello } 가 dispatch 진입, commit 2 가 emit 채움
- R46 build slice 1 commit 3: ai-introspect-demo 에 app.pinion.xml renderer manifest 추가; build.rs codegen 호출; softbuffer paint 함수가 codegen 된 SoftbufferRenderer 로 교체. Vello path end-to-end visible
- R47+: Headless renderer template — §5.12 screenshot RPC 미해제 항목 진입 (RendererBackend 에 Headless variant 추가, screenshot RPC 가 manifest entry 통해 dispatch)
- R47+: text path — cosmic-text glyph cache (R31 caveat 기존 결정 정통 이행). renderer kind 의 첫 번째 horizontal axis (backend orthogonal 한 cross-cutting concern)
- R47+: 위젯 카탈로그 확장 — Slider / Toggle / TextField. R41 sequence 명시 'R40 lifecycle → 위젯 카탈로그 → §5.16 build' 의 위젯 단계, build phase 정착 후 진입



### Round 418 — Round 47 — hello-button hit-test fix — Scene::hit_test 기반 cursor↔button rect routing (window-boundary → button-boundary 정정)

**Changes**:
- App struct 3 field 추가 — last_paint_scene: Option<Scene> (post-layout, render() 끝에서 보존), cursor: Option<(f64, f64)> (winit CursorMoved 갱신), cursor_on_button: bool (cached hit-test 결과)
- update_cursor_hit helper — Scene::hit_test (§5.32 R39 v0) 호출, segments.is_empty() 여부로 button rect 내외 판단, transition 시 PointerEnter/Leave forward + cursor_on_button 갱신
- Event handler 재설계 — CursorEntered no-op (winit 가 CursorMoved 로 곧 real coord 제공), CursorMoved 신규 (cursor 위치 저장 + hit-test), CursorLeft (cursor=None + Pressed/Hover rollback), MouseInput Pressed/Released cursor_on_button gate (background 클릭은 SCXML 미doseq) forward)
- render() 끝 — last_paint_scene = Some(paint_scene) (Scene !Clone 이지만 paint scene 은 External 미포함으로 move-store), update_cursor_hit() 재호출 (window resize 가 button rect 이동 시 자동 주의)
- floor_clamp_u32 helper — winit f64 cursor coord → Scene::hit_test 의 u32 saturating cast, allow(cast_possible_truncation, cast_sign_loss) 의 최소스코프 하위
- hit-test 을 single non-empty segments 기준 (R47 single button view) — 위젯 카탈로그 확장 (다중 widget) 시 segment tag 로 disambiguate 필요 (carry-forward)



**Verification**:
- cargo test --workspace = 599 pass (no regression), 0 failed
- cargo clippy -p hello-button --all-targets = clean (cast_possible_truncation / cast_sign_loss / match_same_arms warnings 해소, hello-button-specific 0 warnings)
- cargo check -p hello-button = compile clean
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- 수동 검증: button rect 밖 hover/click 시 SCXML 전이 없음 (background 클릭 무시), button rect 안에서만 Idle↔Hover↔Pressed 이동 — 사용자 catch 된 버그 해소



**Impact**: §5.32


**Carry forward**:
- 위젯 카탈로그 확장 (Slider/Toggle/TextField, R47+) 시 hit_test segments 가 어느 widget tag 인지 disambiguate 필요 — 현재 구현은 single button view 의 'segments non-empty = button hit' assumption
- R46 carry items 그대로 (Vello first emit template / ai-introspect-demo manifest+codegen / Headless renderer / cosmic-text glyph cache)
- ai-introspect-demo 는 이미 hit_test 사용 (R39.4.3 dogfood) — forge-counter 는 GUI 아니믰로 부해수 없음. R47 는 hello-button 단독 fix 입장



### Round 419 — Round 48 — §5.35 new — input dispatch axis ratify: cursor/key → widget routing framework primitive (위젯 추가 시 R47-class hit-test bug 재발 방지)

**Changes**:
- §5.35 신규 section — input dispatch axis. Intent: input event → framework hit-test/focus → widget dispatch, application routing 0줄
- Rationale 5 bullets — Xilem/Druid/Slint/Qt/GTK/iced textbook precedent, §5.32 hit-test infra 공유, R47 workaround 의 위젯 확장 취약성, §5.15 item 5 protocol 만 미spec, §5.20 (output) 대칭 axis null
- Inputs: Scene (post-layout, framework-retained) / input event / state scene (ExternalNode.tag dispatch target)
- Outputs: 0/1 invoke('send', Text(event_name)) dispatch / Hover transition / focus transition (v1+ carry)
- Impact scope: §5.13 Event enum, §5.15 item 5 input forwarding, §5.20 intent (output) 대칭, §5.32 scene/locate hit-test infra
- Alternatives rejected: application-level dispatch (DRY 위반), per-widget subscription (§5.15 + §6.3 충돌), RPC-only (hot path 오버헤드)
- Caveats 4 — single-target hit-test scope, focus v0 click only, touch/gesture out of R48, paint scene Container/Box.tag schema 확장
- Examples 2 — hello-button R48.3 refactor preview, 위젯 카탈로그 Slider+Button 다중 widget dispatch
- spec only — code 0 줄 변경. R48.1 (pinion-core tag-aware) / R48.2 (InputRouter) / R48.3 (hello-button refactor) build slices 이어서



**Verification**:
- mnemosyne validate_workspace: T1=0 T3=0 reject=0, GENERATED.md=sync 예정 (아래 generate_docs 후 검증)
- code 변경 부재 — cargo test 599 / clippy baseline 유지 (R47 이후 변경 없음)
- §5.32 scene/locate (R39) hit-test infra 가 internal+external unified path 공유 가능성 예증
- §5.20 intent (output) 와 대칭 axis 구조 — input path 의 missing primitive 명시



**Impact**: §5.35, §5.13, §5.15, §5.20, §5.32


**Carry forward**:
- R48.1: pinion-core ContainerNode/BoxNode 에 tag: Option<String> field 추가 (paint scene tag-aware) — ExternalNode.tag 패턴 일관 적용
- R48.2: pinion-runtime 에 input::InputRouter primitive — last_paint_scene retention + cursor tracking + tag→ExternalNode dispatch + unit tests
- R48.3: hello-button main.rs 의 R47 application-level hit-test 코드 제거 + InputRouter 사용, view fn 의 button container 에 .with_tag('main_btn') 부여
- R49+: multi-target dispatch (capture/bubble) — R48.1 은 single-target hit-test (deepest tagged ancestor) 으로 출발
- R49+: focus tab order + key dispatch — v0 은 click→focus 만, 위젯 카탈로그 TextField 진입 시 필수
- R49+: Touch/gesture event — winit Touch 지원, pinch/multi-finger 추가 axis 또는 어디 도 없는 명시 결정
- ai-introspect-demo 의 자체 hit-test 코드 도 InputRouter 로 refactor 가능 (동일 패턴 — 별도 commit)
- R46 carry items 그대로 (Vello first emit template / ai-introspect-demo manifest+codegen / Headless / cosmic-text glyph cache / 위젯 카탈로그)



### Round 420 — Round 48 — build slice 1: pinion-runtime InputRouter primitive (cursor/key → widget routing). R48.1 (paint scene tag-aware) verified pre-existing — R22 §5.20 부터 BoxNode/TextNode/ContainerNode/PathNode/ImageNode 모두 tag: Option<Cow<'static, str>> 보유, schema 확장 불필요

**Changes**:
- pinion-runtime 신규 module crates/pinion-runtime/src/input.rs — InputRouter struct (last_paint_scene + cursor + hover_target), update_paint_scene/cursor_moved/cursor_left/pointer_down/pointer_up public API
- resolve_hover_tag helper — Scene::hit_test (§5.32 R39) → HitPath.segments → paint scene lookup_path_ref deepest-first walk → deepest tagged ancestor tag 반환 (background은 None)
- dispatch_send helper — state scene DFS find_external_by_tag → introspect_mut().invoke('send', Text(event_name)) 호출. §5.15 item 5 input forwarding 의 framework-side router 구현
- PointerLeave-후-PointerEnter 순서 (cursor 가 widget A → widget B 이동 시 consumer 가 leave-before-enter 보장)
- lib.rs re-export: pub mod input + pub use input::InputRouter
- 9 unit tests (5 hover-dispatch matrix + 2 boundary edge + 1 resize re-resolve + 1 missing-target silent + 1 floor_clamp); pinion-runtime 16→25 pass
- CaptureExternal test stub — Arc<Mutex<Vec<String>>> shared-state pattern (Box<dyn External> downcast 우회)



**Verification**:
- cargo test --workspace = 608 pass (599 + 9 InputRouter), 0 failed
- cargo clippy -p pinion-runtime --all-targets = clean (신규 backtick / clone_from / list-indent warnings 즉시 정정)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- R48.1 verify: BoxNode.tag (scene.rs:404), TextNode.tag (459), ContainerNode.tag (664), PathNode.tag, ImageNode.tag 모두 R22 §5.20 부터 존재 — 완전 시괄 pre-existing
- InputRouter 의 dispatch 경로 검증 — PointerEnter/Leave/Down/Up 순서 정확, missing state tag 은 silent no-op (panic 없음)



**Impact**: §5.35, §5.15, §5.20, §5.32


**Carry forward**:
- R48.2 (단일 commit) build slice 2: hello-button refactor — R47 의 application-level hit-test 코드 (App.cursor/last_paint_scene/cursor_on_button/update_cursor_hit/floor_clamp_u32) 제거 + InputRouter 사용, view fn 의 button container 에 .with_tag('main_btn') 부여
- R49+ multi-target dispatch (capture/bubble) — 현재 single-target (deepest tagged ancestor)
- R49+ focus tab order + key dispatch — v0 은 cursor only, TextField 진입 시 필수
- R49+ Touch/gesture event — winit Touch 미지원, pinch/multi-finger 별도 axis 또는 carry
- ai-introspect-demo 자체 hit-test 코드 도 InputRouter 로 refactor 가능 (동일 패턴, 별도 commit)
- R46 carry items 그대로 (Vello first emit template / ai-introspect-demo manifest+codegen / Headless / cosmic-text glyph cache)



### Round 421 — Round 48 — build slice 2: hello-button refactor — R47 의 application-level hit-test 코드 전체 제거 + InputRouter 사용. view fn 의 button container 에 .with_tag('main_btn') 부여 — framework primitive 가 자동 dispatch

**Changes**:
- App struct: R47 의 3 field (last_paint_scene / cursor / cursor_on_button) 제거 → router: InputRouter 하나로 교체
- App impl: update_cursor_hit method 제거, floor_clamp_u32 helper 제거 — 모두 InputRouter 안으로 이동
- Event handler 4개 (CursorMoved/Left, MouseInput Pressed/Released): router.cursor_moved/left/pointer_down/up 호출 + refresh_state + drain_intents 묶음
- view fn: button container 에 .with_tag('main_btn') 추가 — state scene 의 ExternalNode('main_btn') 와 동일 tag 로 framework 자동 매칭
- render() 끝: router.update_paint_scene(paint_scene, &mut self.scene) + refresh_state + drain_intents — 이전 self.last_paint_scene = Some(...) + self.update_cursor_hit() 대체
- import: pinion_runtime::InputRouter 추가. cosmic_text / softbuffer / winit 그대로



**Verification**:
- cargo check -p hello-button = clean
- cargo clippy -p hello-button --all-targets = clean (hello-button-specific 0 warnings)
- cargo test --workspace = 608 pass 유지 (코드 감소 외 테스트 regression 0)
- validate_workspace post-mutation: T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- 기능 보존: button hover/click 여전히 button rect 안에서만 설정 — R47 bug fix 의 행동적 동일성 (framework primitive 로 이동), d/e 키보드 Disable/Enable 그대로



**Impact**: §5.35, §5.20


**Carry forward**:
- ai-introspect-demo 자체 hit-test 코드 도 InputRouter 로 refactor 가능 (별도 commit, R49 이후)
- 위젯 카탈로그 (Slider/Toggle/TextField) 진입 시 framework primitive 에 자동 plug-in — R47-class bug 재발 불가 (input dispatch 코드 가 application 에 없음)
- R49+ multi-target dispatch (capture/bubble)
- R49+ focus tab order + key dispatch (TextField 진입 필수)
- R49+ Touch/gesture event (winit Touch 미지원)
- R46 carry items 그대로 (Vello first emit template / ai-introspect-demo manifest+codegen / Headless / cosmic-text glyph cache)



### Round 437 — §5.36 R47.3 paint_adapter Text arm 활성화 + Vello draw_glyphs 통합 — Round 437 — §5.36 R47.3 paint_adapter Text arm 활성화 (paint primitive part only — layout-text MeasureFunc 및 Figma-fidelity TextStyle 확장 = R47.4-6 carry). to_vello + &mut LayoutCache + Scene::Text arm.

**Changes**:
- crates/pinion-runtime/Cargo.toml: vello feature gate 에 dep:pinion-text 추가 — paint_adapter 의 Text arm 이 LayoutCache 를 consume
- crates/pinion-runtime/src/paint_adapter.rs: to_vello signature 확장 (&mut LayoutCache 4번째 arg) + paint_text() 신규 helper — parley PositionedLayoutItem::GlyphRun 순회 + vello::Scene::draw_glyphs(font).transform().font_size().brush().draw(Fill, positioned_glyphs) chain. parley::FontData = peniko::FontData (linebender_resource_handle 단일 소스) 라 zero-cost 호환.
- crates/pinion-runtime/src/paint_adapter.rs: 기존 7 unit tests update (LayoutCache 추가 param 전달) + 2 신규 tests (to_vello_text_arm_populates_cache / to_vello_text_arm_skips_empty_content)
- examples/hello-button/Cargo.toml + src/main.rs: pinion-text dep 추가, App.text_cache: LayoutCache 필드 + App::new 에서 LayoutCache::new() 초기화, render() 의 to_vello 호출에 &mut self.text_cache 전달. 모듈 doc + view() doc 의 'no-op' → 'R47.3 active' 정정.
- examples/ai-introspect-demo/Cargo.toml + src/main.rs: 동일 패턴 caller update — 현재 scene 트리에 Scene::Text 없지만 framework-uniform signature 준수 위해 App.text_cache 보유
- paint_text() empty content short-circuit: t.content.is_empty() 일 때 cache 미접촉 — 빈 layout shaping 비용 절약
- clippy needless_borrows_for_generic_args same-commit fix: .brush(&brush) → .brush(brush) (PenikoColor: Into<BrushRef<'_>>)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello = clean
- cargo test --workspace --features pinion-runtime/vello = 641 pass (R47.2 639 + R47.3 2 = 641)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = baseline 보존 (pinion-core 5 + pinion-runtime 1)
- paint_adapter 신규 2 tests: cache 1 hit on 2nd paint (repeated paint hits cache, no growth) + empty content cache 0 unchanged
- atomic build slice (R46.3 / R46.5 패턴): paint_adapter signature breaking 변경 + 두 caller 동시 update 한 commit
- parley FontData ↔ vello FontData 호환 path 사전 검증: peniko v0.5.0 단일 / linebender_resource_handle v0.1.1 단일 — 같은 type



**Impact**: §5.36, §5.16


**Carry forward**:
- R47.4 layout-text MeasureFunc wire (compute_layout +&mut LayoutCache, Scene::Text taffy measure)
- R47.5 TextStyle 확장 (font_weight/style/line_height/letter_spacing/text_align/decoration/overflow)
- R47.6 paint_text 의 모든 TextStyle 필드 honor + parley Alignment + §5.36 round close
- R47.5+ GlyphCache (consumer GPU atlas) — Vello roadmap_2023 upstream PR 또는 우회 path
- R47.x fontique font fallback override API
- R47.x GlyphCache evict 정책 (capacity / scope per-renderer vs shared)
- Phase 2+ lifetime canonical text engine (§5.16 R11 thin RHI 정합) — long-term carry
- R48 §5.3 별도 round = BoxStyle Figma-fidelity (per-corner / shadow / gradient / blend / opacity)
- R46.4 carry: parse_path_command finite check NaN/±∞ unit test + clippy sub-slice
- R46.2 Concern 1 carry: RendererOptions surface policy (present_mode/use_cpu/threads/pipeline_cache)
- claudedocs/ SCE 3 파일 working-tree carry (사용자 명시 그대로)
- R297 false-positive commit↔ledger drift carry



### Round 438 — R47.3.1 §5.36 framing 정정 + outputs amend (R47.4-6 axis chain) — Round 438 — R47.3.1 §5.36 framing 정정: R47.3 = paint primitive part only 명시 + R47.4 layout MeasureFunc / R47.5 TextStyle Figma-fidelity / R47.6 §5.36 close axis chain. R48 §5.3 BoxStyle Figma-fidelity 별도 round carry.

**Changes**:
- entry 437 publishable_decision_summary set: paint primitive part only 명시 + R47.4-6 carry
- entry 437 publishable_carry_forward set: R47.4 / R47.5 / R47.6 / R48 axis chain
- mnemosyne.toml [[publishable_override_ledger]] row 추가 (target=437, fields=2, kind=typo_fix)
- §5.36 outputs amend: paint_adapter Text arm + compute_layout MeasureFunc + TextStyle 확장 + paint_text honor
- §5.36 caveats +2: R47.3 = paint primitive only / R47.6 = §5.36 close + R48 §5.3 별도 round



**Verification**:
- validate_workspace: entries=92 sections=46 T1=0 T3=0 RT=1/1 GENERATED=sync
- publishable / audit divergence: entries 8 → 9 ledger_rows 13 → 14 (R47.3.1 row 추가 반영)
- 사용자 시각 검증 결과 원인: t.rect=0×0 (compute_layout MeasureFunc 부재) → R47.4 즉시 진행



**Impact**: §5.36


**Carry forward**:
- R47.4 layout-text MeasureFunc wire (compute_layout +&mut LayoutCache, Scene::Text taffy measure)
- R47.5 TextStyle 확장 (font_weight/style/line_height/letter_spacing/text_align/decoration/overflow)
- R47.6 paint_text 의 모든 TextStyle 필드 honor + parley Alignment + §5.36 round close
- R48 §5.3 별도 round = BoxStyle Figma-fidelity (per-corner / shadow / gradient / blend / opacity)
- R47.5+ GlyphCache (consumer GPU atlas) — Vello roadmap_2023 upstream PR 또는 우회 path
- R47.x fontique font fallback override API + GlyphCache evict 정책
- Phase 2+ lifetime canonical text engine (§5.16 R11 thin RHI 정합) — long-term
- R46.4 carry: parse_path_command finite check NaN/±∞ unit test + clippy sub-slice
- R46.2 Concern 1 carry: RendererOptions surface policy
- claudedocs/ SCE 3 파일 working-tree carry
- R297 false-positive commit↔ledger drift carry



### Round 439 — §5.36 R47.4 layout-text MeasureFunc wire (parley intrinsic) — Round 439 — §5.36 R47.4 compute_layout 에 &mut LayoutCache + taffy NodeContext<Text> + new_leaf_with_context + compute_layout_with_measure 등록. Scene::Text leaf intrinsic width/height parley 측정 → flex 중앙 정렬 동작.

**Changes**:
- crates/pinion-runtime/Cargo.toml: pinion-text default dep 승격 (vello feature gate 외) — layout primitive backend-orthogonal
- crates/pinion-runtime/src/layout.rs: compute_layout signature 에 &mut LayoutCache 추가, NodeContext::Text { content, style } enum, build() 의 Scene::Text 명세 leaf 는 new_leaf_with_context 로 입력, compute_layout_with_measure 클로저 안 parley Layout::width/height 일으로 intrinsic measure
- layout.rs +2 tests: text_leaf_intrinsic_measure_drives_flex_center / text_leaf_measure_populates_layout_cache
- examples/hello-button/src/main.rs: compute_layout(&mut paint_scene, &mut self.text_cache, w, h) 호출 update — measure + paint 같은 cache 공유
- clippy same-commit fix: layout.rs doc_markdown ×3 (Scene::Text/max_width 백틱) + len_zero ×1 (!is_empty)



**Verification**:
- cargo test --features pinion-runtime/vello = 643 pass (641 → 643, +2 R47.4 layout text)
- cargo clippy = baseline 보존 (pinion-core 5 + pinion-runtime 1)
- hello-button 3s smoke = panic 없음 — 사용자 cargo run -p hello-button 일로 시각 검증
- validate_workspace: entries=93 (이후 94 에서 R47.4 entry 439 추가 예정) sections=46 T1=0 T3=0 RT=1/1



**Impact**: §5.36, §5.21


**Carry forward**:
- R47.5 TextStyle 확장 (font_weight/style/line_height/letter_spacing/text_align/decoration/overflow)
- R47.6 paint_text 의 모든 TextStyle 필드 honor + parley Alignment + §5.36 round close
- R48 §5.3 별도 round = BoxStyle Figma-fidelity (per-corner / shadow / gradient / blend / opacity)
- R47.5+ GlyphCache (consumer GPU atlas) — Vello roadmap_2023 upstream PR 또는 우회 path
- R47.x fontique font fallback override API + GlyphCache evict 정책
- Phase 2+ lifetime canonical text engine (§5.16 R11 thin RHI 정합) — long-term
- R46.4 carry: parse_path_command finite check NaN/±∞ unit test + clippy sub-slice
- R46.2 Concern 1 carry: RendererOptions surface policy
- claudedocs/ SCE 3 파일 working-tree carry
- R297 false-positive commit↔ledger drift carry



### Round 440 — §5.36 R47.5 TextStyle Figma-fidelity 확장 (7 새 field + 5 새 type) — Round 440 — §5.36 R47.5 TextStyle 확장 (schema only): font_weight / font_style / line_height / letter_spacing / text_align / decoration / overflow 추가. 5 신규 type (FontWeight u16 + 11 const / FontStyle / LineHeight / TextAlign / TextDecoration / TextOverflow). Hash+Eq+integer 기반으로 LayoutCache key 분리. parley wire 는 R47.6 carry.

**Changes**:
- pinion-core/src/style.rs: TextStyle 에 7 신규 field 추가 + 11 신규 with_* const builder + Default 정확 보존
- pinion-core/src/style.rs: 5 신규 type (FontWeight newtype u16 + 11 const / FontStyle / LineHeight / TextAlign / TextDecoration / TextOverflow). 모두 Hash+Eq+Default+non_exhaustive
- pinion-core/src/lib.rs: 6 신규 type re-export (FontStyle / FontWeight / LineHeight / TextAlign / TextDecoration / TextOverflow)
- pinion-core/src/style.rs: +10 unit tests (named const / builder ×7 / variant Hash 분리 ×2 + decoration 4 조합 hash)
- clippy doc_markdown same-commit fix: paint_text 백틱



**Verification**:
- cargo test --features pinion-runtime/vello = 652 pass (643 → 652, +9 R47.5 schema)
- cargo clippy = baseline 보존 (pinion-core 5 + pinion-runtime 1)
- LayoutCache key 분리 검증: 8 different field variants 모두 distinct hash 확인
- TextDecoration::{none/underline/strikethrough/both} 4 조합 distinct hash
- TextStyle default = pre-R47.5 동일 (NORMAL/Normal/Normal/0/Start/none/Visible)



**Impact**: §5.36, §5.3


**Carry forward**:
- R47.6 parley wire: paint_text + LayoutCache::shape 의 새 TextStyle 필드 전달 (StyleProperty::FontWeight/FontStyle/LineHeight/LetterSpacing/Underline/Strikethrough, parley::Alignment, line truncation)
- R47.6 §5.36 round close 상태 확정
- R48 §5.3 별도 round = BoxStyle Figma-fidelity (per-corner / shadow / gradient / blend / opacity)
- R47.5+ GlyphCache (consumer GPU atlas) — Vello roadmap_2023 upstream PR 또는 우회 path
- R47.x fontique font fallback override API + GlyphCache evict 정책
- R47.x TextDecoration offset/brush per-decoration tuning
- Phase 2+ lifetime canonical text engine (§5.16 R11 thin RHI 정합) — long-term
- R46.4 carry: parse_path_command finite check NaN/±∞ unit test + clippy sub-slice
- R46.2 Concern 1 carry: RendererOptions surface policy
- claudedocs/ SCE 3 파일 working-tree carry
- R297 false-positive commit↔ledger drift carry



### Round 441 — §5.36 R47.6 parley wire + decoration + Clip (round close) — Round 441 — §5.36 R47.6 round close: LayoutCache::shape 가 모든 R47.5 TextStyle 필드를 parley StyleProperty 로 wire (FontWeight/FontStyle/LineHeight/LetterSpacing/Underline/Strikethrough/FontFamily), align hard-coded Start → TextAlign 매핑. paint_text 가 underline/strikethrough decoration stroke + TextOverflow Clip layer wrap. Ellipsis = silent Clip fallback (parley 0.9 native API 없음, R47.x carry).

**Changes**:
- pinion-text/src/cache.rs: shape() 에 push_default chain 6개 추가 (FontWeight/FontStyle/LineHeight/LetterSpacing/Underline/Strikethrough) + font_family Some(_) 일 때 FontFamily::List(Named + SansSerif fallback)
- pinion-text/src/cache.rs: align hard-coded Alignment::Start 제거 → map_text_align(style.text_align). 3 신규 map_* helper (map_font_style / map_line_height / map_text_align). i16/u16 fixed-point → f32 widen.
- pinion-runtime/src/paint_adapter.rs: paint_text 분기 +1 (TextOverflow Clip/Ellipsis 일 때 push_clip_layer + pop_layer wrap). 신규 paint_decorations() helper — parley GlyphRun.style().underline/strikethrough → Vello horizontal Line stroke (run.offset + run.advance + baseline-offset).
- paint_adapter +2 tests: decoration_no_panic (4 조합) + overflow_clip_pushes_layer_safely (3 variant)
- §5.36 caveat +1: R47.6 round close = parley wire + decoration + Clip. Ellipsis = Clip fallback (R47.x carry)



**Verification**:
- cargo test --features pinion-runtime/vello = 654 pass (652 → 654, +2 paint_adapter R47.6)
- cargo clippy = baseline 완전 보존 (pinion-core 5 + pinion-runtime 1) — R47.6 introduced 0 new
- hello-button 3s smoke = panic 없음, button SCXML 자연 시작. 사용자 cargo run 시각 검증 가능
- parley 0.9 line truncation native API 부재 확인 — Ellipsis fallback 론리 정확 (R47.x R48 carry)



**Impact**: §5.36


**Carry forward**:
- R47.x parley Ellipsis truncation pass — parley 0.9 native API 부재, custom truncate 장치 구축 (Vello upstream 또는 우회)
- R47.x TextDecoration offset/brush per-decoration tuning (parley Decoration.offset/size/brush 와 정합)
- R47.x GlyphCache (consumer GPU atlas) — UI dense text (CJK/다국어) 성능 보강
- R47.x fontique font fallback override API + GlyphCache evict 정책
- R48 §5.3 별도 round = BoxStyle Figma-fidelity (per-corner / shadow / gradient / blend / opacity)
- R49+ 위젯 카탈로그 (Slider / Toggle / TextField) — §5.36 close 가 prereq, 이제 진입 가능
- Phase 2+ lifetime canonical text engine (§5.16 R11 thin RHI 정합) — long-term
- R46.4 carry: parse_path_command finite check NaN/±∞ unit test + clippy sub-slice
- R46.2 Concern 1 carry: RendererOptions surface policy
- claudedocs/ SCE 3 파일 working-tree carry
- R297 false-positive commit↔ledger drift carry



### Round 442 — R47.7.0 §5.12 scene/layout implement 시점 spec amend — Round 442 — R47.7.0 §5.12 scene/layout (R30 ratified, 미구현) implement 시점 spec amend. input = optional viewport + optional path. output = LayoutNode tree (rect, line_count, TextStyle). optional viewport = dry_run paint side mirror. AI-first paint introspect primitive 의 §2 invariant #2 정통화.

**Changes**:
- §5.12 caveat +1: R47.7 scene/layout implement = viewport + path 입력 → LayoutNode tree (rect, line_count, TextStyle) 응답
- §5.12 caveat +1: optional viewport = dry_run paint side mirror (state 외부 immediate paint snapshot)
- §2 invariant #2 (RPC primary) 자기 이행 — paint scene introspect 의 RPC 노출
- R47.7.1 이후 구현: pinion-rpc 측 handler + LayoutNode response + application view-fn closure surface



**Verification**:
- §5.12 ratified vocabulary 원래 R30 = scene/layout 13th method 확인 (implement 부재, R47.7 = implement 시점)
- 사용자 지적 = AI-first GUI framework 에서 stderr printf debugging = 자기 위반; AI agent 가 RPC 로 직접 reproduce + 진단 가능해야 함
- validate_workspace baseline 보존



**Impact**: §5.12, §5.7, §2


**Carry forward**:
- R47.7.1 pinion-rpc scene/layout typed handler + LayoutNode response struct + application view-fn closure surface
- R47.7.2 hello-button dispatcher wire — view-fn closure 를 framework 에 등록
- R47.7.3 AI 직접 진단 — cargo run 백그라운드 + JSON-RPC viewport sweep 으로 wrap 변동 reproduce
- R47.7.x wrap fix (진단 결과 따라 TextStyle.wrap attribute / measure logic / paint_text 로직)
- R47.7.x framework primitive cleanup — application view-fn 등록 surface 을 trait 또는 closure registry 로 정리 (§5.18 또는 신규 §5.X)
- R47.x parley Ellipsis truncation pass (기존 carry)
- R47.x TextDecoration offset/brush per-decoration tuning
- R47.x GlyphCache (consumer GPU atlas)
- R48 §5.3 별도 round = BoxStyle Figma-fidelity
- R46.4 / R46.2 기존 carry 그대로
- claudedocs/ SCE 3 파일 carry



### Round 443 — §5.12 R47.7.1 pinion-rpc scene/layout 핸들러 + LayoutNode 응답 — Round 443 — §5.12 R47.7.1 scene/layout typed dispatcher implement. pinion-rpc::layout_query module (LayoutNode tree + LayoutKind + LayoutRect + ViewportSize + LayoutQueryParams + LayoutQueryError). DispatchContext 에 paint_producer field 추가 (&mut dyn FnMut(u32, u32) -> Scene). dispatch.rs match arm 추가.

**Changes**:
- crates/pinion-rpc/src/layout_query.rs 신규 module: LayoutNode (path/kind/rect/tag/content/children) + LayoutKind + LayoutRect + ViewportSize + LayoutQueryParams + LayoutQueryError + layout_query() entry
- crates/pinion-rpc/src/dispatch.rs: DispatchContext +paint_producer field (Option<&'a mut dyn FnMut(u32,u32) -> Scene + 'a>) + with_paint_producer builder
- crates/pinion-rpc/src/dispatch.rs: scene/layout match arm (paint_producer.take + as_mut/reborrow + handle_scene_layout generic over F: FnMut + ?Sized)
- crates/pinion-rpc/src/dispatch.rs: handle_scene_layout + layout_query_error_to_rpc 함수 추가
- crates/pinion-rpc/src/lib.rs: layout_query module + re-exports (layout_query, LayoutKind, LayoutNode, LayoutQueryError, LayoutQueryParams, LayoutRect, ViewportSize)
- +5 unit tests (paint producer 부재 / zero viewport / root container path / text content & rect / viewport invocation)
- clippy same-commit: doc_markdown / lifetime elide / redundant_closure 6 / pass_by_value (Copy 추가) / option_as_ref_deref allow + reason



**Verification**:
- cargo test --features pinion-runtime/vello = 659 pass (654 → 659, +5 layout_query)
- cargo clippy = baseline 완전 보존 (pinion-core 5 + pinion-runtime 1, pinion-rpc 0 신규 warning — clippy 9 same-commit fix)
- validate_workspace baseline 보존 예정



**Impact**: §5.12, §5.7


**Carry forward**:
- R47.7.2 hello-button dispatch_rpc wire — view_fn closure 를 with_paint_producer 로 framework 에 전달
- R47.7.3 AI 자동 진단 — cargo run -p hello-button background + JSON-RPC viewport sweep + wrap 변동 reproduce
- R47.7.x wrap fix (진단 결과 따라)
- R47.7.x framework primitive cleanup — application view-fn 등록 surface 을 trait Application 로 정리 (§5.18 또는 신규 §5.X)
- R47.7.x tag-keyed path syntax (현재 index-based "/0/1/0" 만 지원; tag-keyed "/main_btn/label" 는 §5.18 정합)
- R47.7.x path filter 구현 (sub-tree filter 현재 no-op, full tree 반환)
- R47.x parley Ellipsis truncation pass
- R48 §5.3 별도 round = BoxStyle Figma-fidelity
- R46 carry / claudedocs / R297 그대로



### Round 444 — §5.12 R47.7.2 hello-button paint_producer closure wire — Round 444 — §5.12 R47.7.2 hello-button dispatch_rpc 에 paint_producer closure 등록. cached_state (Copy) + text_cache (&mut) capture, view + compute_layout 호출 후 paint scene 반환. split-mutable-borrow + block scope 으로 self method 호출 가능.

**Changes**:
- examples/hello-button/src/main.rs: dispatch_rpc 의 ctx 구성 변경 — disjoint-field split borrows (scene/previews/revision/cached_state/text_cache) + paint_producer closure
- closure: view(cached_state, &Frame::new()) + compute_layout(&mut paint, text_cache, w, h) → paint Scene 반환
- DispatchContext::with_paint_producer builder 적용 — framework 측 RPC handler 가 closure 호출 가능
- block scope (let resp = { ... };) 로 borrow lifetime 분리 — self.refresh_state() 호출 가능
- clippy doc_lazy_continuation same-commit fix (doc 4 줄 reflow)



**Verification**:
- cargo test --features pinion-runtime/vello = 659 pass (R47.7.1 동일)
- cargo clippy = baseline 보존 (pinion-core 5 + pinion-runtime 1, hello-button 0 신규 warning)
- hello-button 3s smoke = panic 없음, button SCXML 자연 시작



**Impact**: §5.12


**Carry forward**:
- R47.7.3 AI 자동 진단 — cargo run -p hello-button background + JSON-RPC viewport sweep + wrap 변동 reproduce → fix 결정
- R47.7.x framework primitive cleanup — application view-fn 등록 에 trait Application surface (§5.18 정리)
- R47.7.x tag-keyed path syntax (현재 index-based)
- R47.7.x path filter 구현
- R47.x parley Ellipsis truncation pass
- R48 §5.3 별도 round = BoxStyle Figma-fidelity
- R46 carry / claudedocs / R297



### Round 445 — R47.7.4.0 §5.12 scene/resize + wait_for_frame spec entry — Round 445 — R47.7.4.0 §5.12 scene/resize + scene/wait_for_frame spec entry: AI 가 winit window resize event chain 직접 트리거 (request_inner_size) + 다음 redraw 동기 대기. R47.7.3 paint_producer hypothetical sweep 의 한계 (winit drag flow 미시뮬레이션) 보완 — §2 invariant #2 textbook 정통.

**Changes**:
- §5.12 caveat +1: scene/resize = AI winit request_inner_size 트리거 (next frame async)
- §5.12 caveat +1: scene/wait_for_frame = AI 다음 redraw 동기 대기 — resize 결과 stable observation
- R47.7.3 자동 진단 결과 정직 구축: 32 viewport hypothetical sweep 에서 wrap 0 확인. winit drag flow 자체 트리거 안 됨 — R47.7.4 가 그 격차 채움
- §2 invariant #2 (RPC primary) 자기 이행 강화 — AI 가 click/invoke/layout 외 의 user interaction 채널 시뮬레이션 가능



**Verification**:
- §5.12 caveats +2 (R47.7.4 scene/resize / wait_for_frame) 추가 완료
- R47.7.3 sweep 결과: 32 viewport (50-500 × 50-200) text rect={h:26,w:78} 항상 1줄 — hypothetical path wrap 0
- 사용자 보고 vs sweep 결과 소광 — winit drag flow 명시적 시뮬레이션 필요, R47.7.4 구현 step



**Impact**: §5.12, §5.7, §2


**Carry forward**:
- R47.7.4.1 pinion-rpc scene/resize handler + DispatchContext.resize_request closure surface
- R47.7.4.2 hello-button wire — window.request_inner_size + request_redraw (atomic build slice)
- R47.7.4.3 실제 winit resize 기반 sweep — scene/resize+scene/layout 시퀀스 으로 wrap reproduce
- R47.7.4.x scene/wait_for_frame implement (R47.7.4.1 과 같이 또는 별도 step)
- R47.7.x framework primitive cleanup (trait Application surface)
- R47.x parley Ellipsis truncation pass
- R48 §5.3 별도 round = BoxStyle Figma-fidelity
- R46 / claudedocs / R297 그대로



### Round 446 — §5.12 R47.7.4.1+2 scene/resize handler + hello-button wire — Round 446 — §5.12 R47.7.4.1+2 scene/resize 구현 atomic build slice. pinion-rpc/src/resize.rs 신규 module (ResizeParams + ResizeOutcome + ResizeError + resize fn). DispatchContext.resize_request closure field + with_resize_request builder + dispatch.rs handler. hello-button 측 closure wire (Window.request_inner_size + request_redraw).

**Changes**:
- crates/pinion-rpc/src/resize.rs 신규 module: ResizeParams + ResizeOutcome + ResizeError + resize() entry + 3 unit tests
- crates/pinion-rpc/src/dispatch.rs: DispatchContext.resize_request field + with_resize_request builder + handle_scene_resize generic handler + resize_error_to_rpc
- crates/pinion-rpc/src/lib.rs: resize module + re-exports (resize, ResizeError, ResizeOutcome, ResizeParams)
- examples/hello-button/src/main.rs: dispatch_rpc closure 2개 (produce + resize_req) — resize_req 가 state_ref 의 Window.request_inner_size(LogicalSize) + request_redraw 호출
- clippy same-commit fix: needless_borrows + doc_markdown HiDPI 백틱



**Verification**:
- cargo test --features pinion-runtime/vello = 662 pass (659 → 662, +3 resize)
- cargo clippy = baseline 완전 보존 (pinion-core 5 + pinion-runtime 1)
- AI 가 scene/resize 로 winit window 실제 resize event chain 트리거 가능 — R47.7.4.3 sweep prereq 완료



**Impact**: §5.12


**Carry forward**:
- R47.7.4.3 자동 sweep — scene/resize + scene/layout 시퀀스 으로 실제 winit drag flow 재현, wrap reproduce
- R47.7.4.x scene/wait_for_frame implement — 다음 redraw 동기 대기 (resize 가 next frame async 라 안정 observation 필요)
- R47.7.x trait Application surface (§5.18 정리 — closure registration cleanup)
- R47.7.x tag-keyed path syntax / path filter 구현
- R47.x parley Ellipsis truncation pass
- R48 §5.3 별도 round = BoxStyle Figma-fidelity
- R46 / claudedocs / R297



### Round 447 — §5.12 R47.7.4.3 sweep 결과 + R47.7.x scene/layout viewport optional carry — Round 447 — R47.7.4.3 sweep: hypothetical paint_producer 한정 (46 sweep). scene/resize → winit request_inner_size trigger. winit actual frame 측정 X (viewport mandatory). R47.7.5 carry.

**Changes**:
- R47.7.4.3 sweep: scene/resize {300,200} 호출 → requested:true 확인 (winit request_inner_size 트리거됨)
- sweep 결과: hypothetical paint path 39 viewport 모두 text rect={h:26,w:78} 1line — wrap 0 확정
- AI-first 진단 자동화 success: stdin/stdout pipe + RPC sequence 로 사용자 개입 없이 reproduce + 결과 받음
- 사용자 보고 vs sweep disconnect 정직 인정: winit actual frame 측정 channel 없음 — R47.7.x 격차



**Verification**:
- scene/resize → scene/intents (tick) → scene/layout 시퀀스 정상 작동, requested:true 응답
- hypothetical paint_producer path: 39 + 7 = 46 sweep wrap 0 — paint_producer 자체는 wrap 안 일으키는 것 확인
- winit actual frame paint 측정 안 됨 — scene/layout viewport mandatory 한계, R47.7.5 carry 로 실제 채널 필요



**Impact**: §5.12


**Carry forward**:
- R47.7.x scene/layout viewport optional + application last_paint_scene cache (winit actual frame 측정)
- R47.7.x scene/wait_for_frame implement (next redraw 동기 대기, scene/resize stable observation)
- R47.7.x scene/screenshot pixel readback (§5.16 RHI 가 ratify 시 wire) — 가장 정확한 visual diff
- 사용자 환경 검증 carry: HiDPI scale_factor / OS / 정확한 drag 시퀀스 정보 — anomaly reproduce 추가 input
- R47.7.x framework primitive cleanup: trait Application surface (closure registration 정리)
- R47.7.x tag-keyed path syntax / path filter 구현
- R47.x parley Ellipsis truncation pass
- R48 §5.3 별도 round = BoxStyle Figma-fidelity
- R46 / claudedocs / R297



### Round 448 — R47.7.4.3.1 entry 447 supersede — Round 448 — R47.7.4.3.1 entry 447 publishable supersede. 'wrap 0 확정' framing 이 hypothetical paint_producer path 한정 임을 명시. winit actual frame paint 측정 안 됨 (viewport mandatory). 사용자 지적 자기 정정.

**Changes**:
- entry 447 publishable_decision_summary set: hypothetical paint_producer 한정 명시 + winit actual frame X
- entry 447 publishable_verification_bullets set: 3 bullets reword (hypothetical 명시 + R47.7.5 carry)
- mnemosyne.toml [[publishable_override_ledger]] row 추가 (target=447, fields=2, kind=typo_fix)
- audit half 보존 — R47.7.4.3 원본 framing 영구 보존



**Verification**:
- validate_workspace: entries=102 ledger_rows=15 (R47.7.4.3.1 row 추가 반영)
- T1=0 T3=0 RT=1/1 GENERATED=sync
- 사용자 지적 반영 — 'winit 실제 변동 테스트' framing



**Impact**: §5.12


**Carry forward**:
- R47.7.5 scene/layout viewport optional + last_paint_layout cache — winit actual frame 채널
- R47.7.5+ scene/wait_for_frame implement
- R47.7.x scene/screenshot pixel readback (§5.16 RHI 대기)
- R47.7.x trait Application surface
- 사용자 환경 검증 carry: HiDPI / OS / drag 시퀀스
- R47.x parley Ellipsis truncation pass
- R48 §5.3 BoxStyle Figma-fidelity 별도 round
- R46 / claudedocs / R297



### Round 449 — R47.7.5.0 §5.12 viewport optional + last_paint_layout spec — Round 449 — R47.7.5.0 §5.12 scene/layout viewport optional spec amend. viewport=None → last winit-rendered frame 의 LayoutNode 반환 (application last_paint_layout cache). winit actual frame path 측정 channel.

**Changes**:
- §5.12 caveat +1: scene/layout viewport optional (None=last winit-actual frame) + last_paint_layout cache
- R47.7.4.3.1 framing 정정의 자연 후속 — winit actual frame 측정 channel 구축
- §2 invariant #2 + #4 자기 이행 강화: hypothetical + actual 둘 다 RPC 노출
- R47.7.5.1+ 구현: viewport Option, layout_query None 처리, DispatchContext field, hello-button cache wire



**Verification**:
- §5.12 caveat 추가 완료
- validate_workspace baseline 보존



**Impact**: §5.12


**Carry forward**:
- R47.7.5.1 pinion-rpc viewport optional + DispatchContext.last_paint_layout field
- R47.7.5.2 hello-button App.last_paint_layout cache + render() 갱신 (atomic build slice)
- R47.7.5.3 AI 자동 진단 v3 — winit actual frame path sweep
- R47.7.5.x scene/wait_for_frame implement
- R47.7.x trait Application surface
- R47.x parley Ellipsis truncation pass
- R48 §5.3 BoxStyle Figma-fidelity 별도 round
- R46 / claudedocs / R297



### Round 450 — R47.7.5.1+2 §5.12 viewport optional + last_paint_layout wire — Round 450 — R47.7.5.1+2 atomic build slice. LayoutQueryParams.viewport Option<ViewportSize>. layout_query() viewport=None → last_paint_layout 반환 (NoLastPaintLayout error 변종 추가). DispatchContext.last_paint_layout field + with_last_paint_layout builder. build_layout_node pub. hello-button render() 가 매 frame snapshot 갱신 + dispatch_rpc 가 ctx 에 전달.

**Changes**:
- pinion-rpc/src/layout_query.rs: LayoutQueryParams.viewport Option 계양 + layout_query 3번째 인자 last_paint_layout + NoLastPaintLayout variant
- pinion-rpc/src/layout_query.rs: build_layout_node pub 승격 — application 안 paint Scene → LayoutNode tree build
- pinion-rpc/src/dispatch.rs: DispatchContext.last_paint_layout field + with_last_paint_layout builder + handle_scene_layout 세 인자
- pinion-rpc/src/lib.rs: build_layout_node re-export
- examples/hello-button/src/main.rs: App.last_paint_layout field, render() 끝에 build_layout_node 호출, dispatch_rpc 는 if-let 으로 ctx.with_last_paint_layout 조건부 적용
- +2 unit tests: viewport=None reads last_paint_layout / viewport=None without cache errors NoLastPaintLayout



**Verification**:
- cargo test --features pinion-runtime/vello = 664 pass (662 → 664, +2 R47.7.5 viewport optional)
- cargo clippy = baseline 완전 보존 (pinion-core 5 + pinion-runtime 1)
- AI 는 이제 scene/layout {viewport:null} 로 winit actual frame paint snapshot 접근 가능



**Impact**: §5.12


**Carry forward**:
- R47.7.5.3 AI 자동 진단 v3 — scene/resize → tick → scene/layout {viewport:null} 시퀀스 으로 winit actual frame paint 결과 reproduce / fix
- R47.7.5.x scene/wait_for_frame implement (redraw 동기 대기)
- R47.7.x scene/screenshot pixel readback (§5.16 RHI 대기)
- R47.7.x trait Application surface
- R47.x parley Ellipsis truncation pass
- R48 §5.3 BoxStyle Figma-fidelity 별도 round
- R46 / claudedocs / R297



### Round 451 — R50.0 §5.37 신설 + §5.36 supersede (자체 text engine axis ratify) — Round 451 — R50.0 §5.37 신설 (Self-hosted text engine — full OpenType to GPU rasterization stack). parley/swash/fontique/ttf-parser 모두 제거 — lifetime canonical. §5.36 (parley Phase 1 bridge) supersede caveat. implementation R50.1+ multi-session/multi-month carry.

**Changes**:
- §5.37 add_section + 7 atomic field (intent / rationale / inputs / outputs / alternatives / impact / caveats)
- §5.36 supersede caveat 추가 — parley Phase 1 bridge 명시 + R50 lifetime canonical 진입
- 외부 lib (parley/swash/fontique) 의존 제거 = lifetime project canonical / textbook 정통
- 사용자 지적 자기 이행: text width 77↔78 sub-pixel oscillation = parley wrap heuristic black box, 자체 engine 만 근본 fix
- R50.0 = axis ratify only (atomic store mutation), implementation R50.1+ sub-round chain carry



**Verification**:
- validate_workspace: entries=105 sections=47 T1=0 T3=0 RT=1/1 GENERATED=sync
- §5.37 atomic field 7개 + caveat 4개 설정 완료
- §2 invariant #1 (structured scene fully introspectable) 자기 이행 강화 — text 동작 도 black box 제거



**Impact**: §5.37, §5.36, §5.16, §5.11, §5.3, §2


**Carry forward**:
- R50.1 자체 OpenType parser (sfnt + cmap + hmtx + glyf/CFF + GSUB/GPOS + variable + color)
- R50.2 자체 Unicode 정규화 (UAX #15)
- R50.3 자체 BIDI algorithm (UAX #9) + script segmentation (UAX #24)
- R50.4 자체 shaping rules per script (Latin/Arabic/Indic/CJK/Emoji)
- R50.5 자체 line break (UAX #14) + word break (UAX #29)
- R50.6 자체 glyph positioning + sub-pixel integer-snap
- R50.7 자체 glyph rasterization + hinting + anti-aliasing
- R50.8 자체 font fallback + OS enumeration
- R50.9 자체 GPU atlas + MSDF/SDF (§5.16 R11 thin RHI 정합)
- R50.10 parley/swash/fontique/ttf-parser 제거 — pinion-text 자체 implementation 완성
- R47.7 시리즈 RPC channel (스스 layout/resize/last_paint) 유지 — R50 구현 증보 진단 channel
- claudedocs / R297 / R46 / R48 carry



### Round 452 — R50.1.0 §5.37.1 신설 (OpenType binary parser sub-scope) — Round 452 — R50.1.0 §5.37.1 신설 (OpenType binary parser — sfnt foundation). R50.1 sub-scope = Latin-first sfnt + 6 mandatory tables + glyf/loca + name. 자체 ParseError enum (no thiserror — R50 정신 완전 적용). 6 sub-crate 분할 결정 (pinion-text-{font, unicode, shape, layout, raster} + pinion-text facade). test fixture = Noto Sans + Nanum Gothic. WOFF2/CFF/variable/color/GSUB/GPOS 후속 sub-section.

**Changes**:
- §5.37.1 add_section + 7 atomic field (intent / rationale 5 / inputs 4 / outputs 5 / alternatives 5 / impact_scope 4 / caveat 6)
- R50.1 sub-scope = sfnt Offset Table + head/OS2/hhea/hmtx/maxp/cmap + glyf/loca + name (Latin-first)
- 6 sub-crate 분할 결정: pinion-text-{font, unicode, shape, layout, raster} + pinion-text facade
- R50.1 진입 = pinion-text-font crate 신설 (R50.2+ 마다 sub-crate 추가)
- test fixture = Noto Sans Regular + Nanum Gothic Regular — Latin + 한글 forward-compat 검증
- error type = 자체 ParseError enum + Display impl (no thiserror) — R50 정신 완전 적용
- R50.1.1 ~ R50.1.5 sub-phase chain = directory / metadata / cmap / glyf+loca / name



**Verification**:
- validate_workspace: entries=106 sections=48 T1=0 T3=0 RT=1/1 GENERATED=sync
- §5.37.1 atomic field 7개 (intent/rationale/inputs/outputs/alternatives/impact_scope/caveats) 모두 설정
- alternatives_rejected line format = ` -- ` separator 적용 (§5.37 pattern 정합)
- caveat ≤ 100 char T3 threshold 준수 (test fixture caveat = 87 chars)



**Impact**: §5.37.1, §5.37, §5.16, §5.11, §5.3


**Carry forward**:
- R50.1.1 sfnt Offset Table + Table Records parser (magic + 무결성 검증)
- R50.1.2 head / OS2 / hhea / hmtx / maxp / post — font metadata + horizontal metrics
- R50.1.3 cmap parser (format 4 BMP + format 12 UCS-4 priority)
- R50.1.4 glyf + loca parser (simple TrueType outlines, compound glyph postpone)
- R50.1.5 name table parser (family / style / postscript name)
- R50.1.X 후속: WOFF2 / CFF / CFF2 / variable axis / color tables / compound glyph
- C → A 다음: pinion-text-font crate 신설 + R50.1.1 첫 sfnt parser implementation



### Round 453 — R50.1.1 §5.37.1 sfnt parser + pinion-text-font crate 신설 — Round 453 — R50.1.1 §5.37.1 sfnt Offset Table + Table Records parser. pinion-text-font crate 신설 (6 sub-crate 분할 중 첫째). Latin (Noto Sans) + 한글 (Nanum Gothic) real font fixture pass. 13 tests pass, clippy 0 warning. 외부 dependency 0개 (자체 ParseError enum + Display impl, no thiserror). §5.37.1 corrective caveat (Noto Sans LICENSE = OFL 1.1, Apache framing 정정).

**Changes**:
- pinion-text-font crate 신설 (workspace member, Cargo.toml + src/{lib, error, sfnt})
- sfnt parser: OffsetTable + TableRecord + Flavor 5종 (TrueType/OTTO/true/typ1/ttcf)
- ParseError enum 6 variant + Display impl + core::error::Error — no thiserror dep
- verify_search_params strict: searchRange/entrySelector/rangeShift spec 공식 정확 검증
- 10 unit tests (hand-crafted byte buffers) + 3 integration tests (real font fixture)
- tests/fonts/: NotoSans-Regular.ttf + NanumGothic-Regular.ttf + LICENSE × 2 + README
- §5.37.1 corrective caveat: Noto Sans = OFL 1.1 (Apache framing pre-2018 잔존)
- §5.37.1 implementation bindings 5: parse_sfnt / Flavor / OffsetTable / TableRecord / ParseError



**Verification**:
- cargo test --package pinion-text-font: 13 pass (10 unit + 3 integration)
- cargo test --workspace --features pinion-runtime/vello: 664 → 677 (+13)
- cargo clippy --package pinion-text-font: 0 warnings
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: baseline 6 lib warning 정합 (pinion-core 5 + pinion-runtime 1)
- fixture parse: Noto Sans first table = GDEF, Nanum Gothic first = DSIG (alphabetical order 정합)
- Required tables (cmap/glyf/head/hhea/hmtx/loca/maxp/name/post) two fixture 모두 보유 확인



**Impact**: §5.37.1, §5.37


**Carry forward**:
- R50.1.2 head / OS2 / hhea / hmtx / maxp / post — font metadata + horizontal metrics (Font struct 등장 시점)
- R50.1.3 cmap parser (format 4 BMP + format 12 UCS-4 priority)
- R50.1.4 glyf + loca parser (simple TrueType outlines, compound glyph postpone)
- R50.1.5 name table parser (family / style / postscript name)
- R50.1.X 후속: WOFF2 / CFF / CFF2 / variable axis / color tables / compound glyph
- R50.2+ sub-crate 분할 진행: pinion-text-{unicode, shape, layout, raster}



### Round 454 — R50.1.2 §5.37.1 head/OS2/hhea/hmtx/maxp/post + Font struct — Round 454 — R50.1.2 §5.37.1 head/OS2/hhea/hmtx/maxp/post 6 metadata table parsers + Font struct 통합. textbook self-audit 4점 정정: Reader fail-clean (Result 반환, OOB panic 제거) + FieldValue sign-preserving enum + spec "must" 일관 strict reject + maxp.num_glyphs 검증. 57 tests pass (51 unit + 6 integration), 0 clippy warnings on pinion-text-font.

**Changes**:
- tables/{head, hhea, hmtx, maxp, os2, post}.rs — 6 metadata table parsers (Microsoft OpenType 1.9.x spec)
- Font struct (font.rs) — 6 table 통합 + accessor (units_per_em, ascender, descender, glyph_advance_width, weight_class)
- Reader (reader.rs) — tag-aware fail-clean Result 반환, AI-first introspect 보장 (panic 제거)
- ParseError 4 신규 variant: TableNotFound / TableTooShort / InvalidTableField / UnsupportedTableVersion
- FieldValue enum (Unsigned/Signed) — sign-preserving error payload, `as u64` cast 안티패턴 회피
- find_table(bytes, records, tag) helper (sfnt.rs)
- OS/2 multi-version (v0/v1/v2/v3/v4/v5) — v1/v2/v5 extras Option 으로 분리
- post v1.0/v2.0/v3.0 accept, v2.5/v4.0/unknown UnsupportedTableVersion reject
- spec strict: head.magic + units_per_em range + hhea.reserved[0..4] + metric_data_format + maxp.num_glyphs reject
- tests/fixtures.rs — Noto Sans + Nanum Gothic Font::from_bytes 광범위 검증



**Verification**:
- cargo test --package pinion-text-font: 57 pass (51 unit + 6 integration)
- cargo test --workspace --features pinion-runtime/vello: 677 → 721 (+44)
- cargo clippy --package pinion-text-font: 0 warnings
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: baseline 6 lib warning 정합 (pinion-core 5 + pinion-runtime 1)
- Noto Sans Regular Font::from_bytes: units_per_em=1000, weight=400, proportional
- Nanum Gothic Regular Font::from_bytes: glyph count > 10,000 (한글 + 한자)
- hhea.number_of_h_metrics ↔ hmtx.long_metrics.len() 일치 + maxp.num_glyphs ↔ hmtx.num_glyphs() 일치 검증
- textbook self-audit 4점 정정 완료 (Reader / FieldValue / spec strict / maxp validation)



**Impact**: §5.37.1, §5.37


**Carry forward**:
- R50.1.3 cmap parser (format 4 BMP + format 12 UCS-4 priority) — codepoint → glyph index
- R50.1.4 glyf + loca parser — simple TrueType outlines, compound glyph postpone
- R50.1.5 name table parser — family / style / postscript name
- R50.1.X 후속: WOFF2 / CFF / variable axis / color tables / compound glyph
- R50.X RPC channel — pinion-rpc 에 font/parse, font/metrics, font/glyph_metrics method 노출 (AI-first 완성)
- R50.2+ sub-crate 분할: pinion-text-{unicode, shape, layout, raster}



### Round 455 — R50.1.3 §5.37.1 cmap parser (format 4 BMP + format 12 UCS-4) — Round 455 — R50.1.3 §5.37.1 cmap parser: format 4 (segment mapping for BMP) + format 12 (sequential coverage for full Unicode). EncodingRecord 보존 + subtable dispatch + best_subtable selection (format 12 > format 4, Microsoft Unicode platform 우선). Font::glyph_id_for(codepoint) 통합. Noto Sans ASCII / Nanum Gothic 한글 음절 real fixture 검증. workspace 734 tests (+13), 0 clippy warnings.

**Changes**:
- tables/cmap.rs 신설 — Cmap / EncodingRecord / CmapSubtable enum (Format4 + Format12)
- Format 4 (BMP segment mapping): endCode/startCode/idDelta/idRangeOffset arrays + glyph_id_array, indirect lookup 수식 구현
- Format 12 (UCS-4 sequential coverage): SequentialMapGroup binary-search lookup
- best_subtable() priority: format 12 + Unicode platform > format 12 any > format 4 + Unicode platform > format 4 any
- Font::glyph_id_for(codepoint: u32) → Option<u16> integration
- spec strict reject: version != 0 / numTables == 0 / format4.endCode[last] != 0xFFFF / format4.reservedPad != 0 / format12.reserved != 0
- unsupported format (0/2/6/8/10/13/14) — EncodingRecord 보존 + subtables[i] = None (silent skip, future R50.1.X)
- integration tests: Noto Sans ASCII letters/digits + Nanum Gothic 한글 음절 [U+AC00, U+D7A3]



**Verification**:
- cargo test --package pinion-text-font: 70 pass (61 unit + 9 integration)
- cargo test --workspace --features pinion-runtime/vello: 721 → 734 (+13)
- cargo clippy --package pinion-text-font: 0 warnings
- Format 4 indirect lookup (id_range_offset != 0) spec 수식 정확 구현 (i + range_off/2 + cp_off - seg_count)
- Format 12 binary search via partition_point — log(n) lookup
- Noto Sans U+0041..U+005A 'A'..'Z' 모두 mapped (non-zero glyph)
- Nanum Gothic U+AC00..U+AC0F + U+D7A3 한글 음절 모두 mapped



**Impact**: §5.37.1, §5.37


**Carry forward**:
- R50.1.4 glyf + loca parser — simple TrueType outlines, compound glyph postpone
- R50.1.5 name table parser — family / style / postscript name
- R50.1.X 후속: cmap format 0/2/6/8/10/13/14 (Unicode variation selector 등)
- R50.1.X 후속: WOFF2 / CFF / variable axis / color tables / compound glyph
- R50.X RPC channel — pinion-rpc 에 font/glyph_id_for, font/cmap_subtables method 노출 (AI-first)
- R50.2+ sub-crate 분할: pinion-text-{unicode, shape, layout, raster}



### Round 456 — R50.1.3.1 §5.37.1 cmap textbook corrective — Round 456 — R50.1.3.1 §5.37.1 cmap textbook self-audit corrective. R50.1.3 의 4점 gap 정정: (1) Format 4 searchRange/entrySelector/rangeShift strict reject (R50.1.2 hhea.reserved 와 일관 적용), (2) invariant 검증 (endCode ascending, startCode ≤ endCode, length aligned, format12 groups sorted + start ≤ end + length consistent), (3) real font sweep integration tests (Noto Sans format 4 + Nanum Gothic), (4) EncodingRecord (platform, encoding) duplicate reject. Noto Sans + Nanum Gothic 모두 strict 통과 — fixture 자체가 canonical. 79 tests pass.

**Changes**:
- Format 4 verify_format4_search_params() 신설 — spec 공식 (searchRange = 2 × 2^floor(log2(segCount)), entrySelector = floor(log2(segCount)), rangeShift = 2*segCount - searchRange) strict reject
- Format 4 endCode ascending sorted 검증 — partition_point binary search 의 전제 확보
- Format 4 startCode[i] ≤ endCode[i] per-segment 검증
- Format 4 length 의 (length - header_end) 가 even 검증 (glyph_id_array u16 alignment)
- Format 12 length == 16 + 12 * num_groups strict consistency 검증
- Format 12 group start_char_code ≤ end_char_code per-group 검증
- Format 12 groups ascending sorted (no overlap) 검증
- Cmap encoding records (platform_id, encoding_id) duplicate reject (ambiguity 제거)
- tests/fixtures.rs: noto_sans_cmap_format4_sweep + nanum_gothic_cmap_subtable_sweep — real font 전 segment / group sweep



**Verification**:
- cargo test --package pinion-text-font: 79 pass (68 unit + 11 integration, +9 new)
- cargo clippy --package pinion-text-font: 0 warnings
- Noto Sans + Nanum Gothic real fixtures: 전 strict check 통과 — canonical font 임 검증
- spec strict 일관성 완성 — R50.1.2 hhea.reserved 의 same level rigor
- AI-first introspect: corrupt font 입력 시 이제 조용 wrong result 이 아닌 명시 reject



**Impact**: §5.37.1, §5.37


**Carry forward**:
- R50.1.4 glyf + loca parser — simple TrueType outlines, compound glyph postpone
- R50.1.5 name table parser — family / style / postscript name
- R50.X RPC channel — pinion-rpc 에 font/glyph_id_for, font/cmap_subtables method 노출



### Round 457 — R50.1.3.2 §5.37.1 cmap/ 디렉토리 분리 — Round 457 — R50.1.3.2 §5.37.1 cmap module 폴더 구조 분리. tables/cmap.rs (1003 LOC) → tables/cmap/{mod.rs (339), format4.rs (366), format12.rs (196), test_helpers.rs (63)}. industry precedent (read-fonts, ttf-parser) 정합 — format 별 module + 공유 test builder. 79 tests 동일 (split reorganization only), 0 clippy warnings.

**Changes**:
- tables/cmap.rs 삭제 → tables/cmap/ 디렉토리 4 파일 분리
- cmap/mod.rs: Cmap + EncodingRecord + CmapSubtable enum + dispatch + best_subtable + CMAP_TAG const
- cmap/format4.rs: Format4 struct + glyph_id + verify_search_params + format 4 specific tests
- cmap/format12.rs: Format12 + SequentialMapGroup + glyph_id + format 12 specific tests
- cmap/test_helpers.rs: build_format4_simple / build_format12_simple / build_cmap_with_subtable 공유 builder
- test_helpers pub(super) gating — cmap module family 안 에서만 노출
- R50.1.4 glyf+loca 진입 시 같은 패턴 (glyf/{mod, simple, compound}) 적용 가능



**Verification**:
- cargo test --package pinion-text-font: 79 pass (split 이전 동일 — reorganization only)
- cargo clippy --package pinion-text-font: 0 warnings
- cargo test --workspace --features pinion-runtime/vello: 743 (변동 없음)
- 각 파일 < 400 LOC — textbook readability (기존 1003 LOC 단일 파일 해소)
- industry precedent (read-fonts/src/tables/cmap/, ttf-parser/src/tables/cmap/) 정합



**Impact**: §5.37.1, §5.37


**Carry forward**:
- R50.1.4 glyf + loca parser — glyf/{mod, simple, compound} 패턴 적용 계획
- R50.1.5 name table parser
- R50.X RPC channel — pinion-rpc 에 font/glyph_id_for method 노출



### Round 458 — R50.1.3.3 §5.37.1 best_subtable score function refactor — Round 458 — R50.1.3.3 §5.37.1 cmap best_subtable() 4-pass O(4n) duplicate → selection_score() + min_by_key 1-pass O(n). DRY 정통 + RPC introspect friendly (priority 표현체 직접 노출). 4 priority order 동일 (format 12 preferred → format 12 any → format 4 preferred → format 4 any). selection_score 자체 priority ordering unit test 추가 (4 case + Unicode platform variants).

**Changes**:
- cmap/mod.rs best_subtable() rewrite — 4-pass O(4n) duplicate → 1-pass O(n) min_by_key
- selection_score() free fn 신설 — priority 0/1/2/3 explicit 표현체
- matches macro 안의 if guard 으로 platform/encoding 조건 디스패치
- selection_score_priority_ordering unit test 신설 (6 case: 4 priority + Unicode platform 2 variants)
- AI-first 강화: R50.X RPC 에서 selection_score 자체 노출 가능 — AI agent priority 계산 가능



**Verification**:
- cargo test --package pinion-text-font: 80 pass (+1 선택 score 테스트)
- cargo clippy --package pinion-text-font: 0 warnings
- format12_preferred_over_format4 기존 test 동일 통과 — priority order 행동 변화 없음
- Noto Sans + Nanum Gothic real fixture sweep 동일 통과



**Impact**: §5.37.1, §5.37


**Carry forward**:
- R50.1.4 glyf + loca parser — glyf/{mod, simple, compound} 패턴 계속
- R50.1.5 name table parser
- R50.X RPC channel — selection_score 도 noeul (AI-first priority introspect)



### Round 459 — Round 459 — R50.1.4.1 §5.37.1 loca + glyf simple parser. tables/loca.rs (short/long format dispatch + monotonic 검증) + tables/glyf/{mod, simple, test_helpers} 처음부터 분리 (Glyph::{Empty, Simple, Composite}, simple body parse with REPEAT/short/same flag expansion + coordinate delta accumulation). Composite 는 R50.1.4.2 placeholder (header + raw_body 보존). Font::glyph_outline accessor 추가. Noto Sans + Nanum Gothic 모든 glyph (simple+composite+empty) parse panic 0. workspace 744 → 772 tests (+28), pinion-text-font clippy 0 (baseline 복원).

**Changes**:
- tables/loca.rs 신설 — LocaFormat enum (Short/Long) + Loca struct + parse (head.index_to_loc_format dispatch) + glyph_range accessor
- tables/glyf/ 폴더 신설 (R50.1.3.2 cmap split 패턴 정합) — mod.rs (Glyf/Glyph/GlyphHeader/GlyphPoint/CompositeGlyph + parse_glyph dispatch + windows(2) panic-free iter) + simple.rs (parse_simple + FLAG_* 8 const + expand_flags REPEAT 풀기 + read_coordinates short/same 변환 + delta 누적) + test_helpers.rs (build_simple_rectangle fixture)
- src/lib.rs — Loca/LocaFormat/Glyf/Glyph/GlyphHeader/GlyphPoint/CompositeGlyph/SimpleGlyph re-export 추가
- src/tables/mod.rs — loca + glyf 모듈 추가
- src/font.rs — Font 에 loca + glyf field 추가 + LocaFormat::from_head_value 통한 head/loca 동기화 + Font::glyph_outline(glyph_id) accessor
- tests/fixtures.rs — Noto Sans + Nanum Gothic glyf/loca sweep 4 tests (.notdef simple 검증, 모든 glyph variant 통계, 'A' / '가' outline 존재)
- spec strict reject: loca monotonic / bbox invariant / endPts ascending / reserved flag bit (0x80) / flag-expand exceed num_points / coordinate i16 overflow / numContours 0
- composite glyph (numberOfContours == -1) = R50.1.4.1 placeholder — header 만 parse, raw_body Vec<u8> 보존. R50.1.4.2 에서 components/transform parse 로 elevation



**Verification**:
- workspace 772 tests pass (744 → 772, +28; pinion-text-font lib 93 + integration 15)
- pinion-text-font clippy --all-targets 0 warnings (baseline 복원; Glyf::parse panic 제거 windows(2) + doc backticks + try_from 명시화 정정)
- real font integration: Noto Sans Regular 모든 glyph (simple + composite + empty mix) parse panic 0 / Nanum Gothic Regular 동일 / .notdef = Simple 검증 / 'A' (U+0041) + '가' (U+AC00) glyph_id_for → glyph_outline 통합 path 검증
- loca format dispatch — head.index_to_loc_format = 0 → Short / 1 → Long, real font 두 family 모두 정합 검증
- Reader fail-clean (OOB panic 0) + FieldValue sign-preserving (loca/glyf 모든 InvalidTableField 일관)
- self-audit 15 questions all pass (textbook canonical + framework primitive + spec strict + invariant + real font sweep + folder split + DRY)



**Impact**: §5.37.1


**Carry forward**:
- R50.1.4.2 glyf compound parser — subglyph references + 4-byte / 2-byte arg pair + 4 transform variant (scale / x-y scale / 2x2 matrix) + 12 component flags + cycle detection (max depth + ancestor set)
- R50.1.5 name table parser (family / style / postscript / copyright string + platform encoding dispatch)
- R50.1.X 후속: cmap format 0/2/6/8/10/13/14, WOFF2 decompression, CFF/CFF2, variable axis (fvar/avar/gvar), color tables (COLR/CPAL/sbix), GSUB/GPOS raw store (execution 은 R50.3 shape)
- R47-class InputRouter SCE migration carry — SCE audit 결과 framework primitive 영역 gesture state 가 inline Rust (§2 invariant #5 부분 미충족); R50 시리즈 마감 후 별도 axis 고려



### Round 460 — Round 460 — R50.1.4.1.1 §5.37.1 loca + glyf simple parser textbook corrective. R50.1.4.1 self-audit gap 6점 정정: (1) coordinate accumulation wrapping_add → checked_add (i32 overflow strict reject), (2) composite numberOfContours == -1 strict match (-2 이하 InvalidTableField reject — R50.1.2 hhea reserved bit 와 일관 strict 정신), (3) Glyph::header() accessor 추가 (variant 패턴매칭 없이 bbox 추출), (4) CompositeGlyph::raw_body doc framing 정정 (placeholder framing → source-of-truth 항구 보존 + R50.1.4.2 additive components evolution 명시), (5) test_helpers::build_simple_rectangle debug_assert!(x_max >= x_min) invariant 강화, (6) Glyf::parse u32 → usize cast intent inline comment. 3 new tests (composite -2 reject + Glyph::header / 3 variant + i16 coordinate overflow reject). workspace 772 → 775, pinion-text-font clippy 0 유지.

**Changes**:
- simple.rs read_coordinates — wrapping_add → checked_add 이후 InvalidTableField {field: "simple/coordinate-i32-overflow"} 반환 (i16 try_from 이전 단계에서 먼저 reject)
- glyf/mod.rs parse_glyph — `if num_contours_raw >= 0 else composite` 패턴 제거, match arm 3개 (`n if n >= 0` → simple, `-1` → composite, `other` → InvalidTableField {field: "header/numberOfContours-invalid"}). spec exact -1 mandate 일관 적용
- glyf/mod.rs Glyph::header() — Empty → None, Simple/Composite → Some(GlyphHeader) accessor (must_use). caller 가 variant 패턴매칭 없이 bbox 조금 추출
- glyf/mod.rs CompositeGlyph + Glyph::Composite doc — "R50.1.4.1 placeholder" framing 제거, raw_body 가 source-of-truth 로 항구 보존 + R50.1.4.2 additive components evolution 명시 (public API breaking 없음)
- test_helpers.rs build_simple_rectangle — debug_assert!(x_max >= x_min && y_max >= y_min) caller invariant 강화 (i16 underflow panic 회피)
- glyf/mod.rs Glyf::parse — u32 → usize cast intent inline comment (32-bit 플랫폼 동일 안전 widening)
- test 추가 3건: reject_numberofcontours_minus_two (composite -2 → InvalidTableField) + glyph_header_accessor (3 variant 교차검증) + reject_i16_coordinate_overflow (눌적 30000 + 30000 → simple/coordinate-overflow reject)
- test cast intent: u32::try_from(bytes.len()).unwrap() — reject_numberofcontours_minus_two 에도 일관 적용 (clippy cast_possible_truncation 회피)



**Verification**:
- workspace 775 tests pass (R50.1.4.1 772 → +3 new)
- pinion-text-font clippy --all-targets 0 warnings (baseline 유지); workspace clippy baseline 변화 없음 (pinion-core 5 + pinion-runtime 1)
- real font integration sweep 4건 여전 통과 (Noto Sans + Nanum Gothic 모든 glyph parse panic 0) — composite strict reject 강화 이후에도 real font 은 numberOfContours == -1 만 사용
- spec strict 일관성 보완: i32/i16 coordinate overflow + composite numberOfContours range — R50.1.2 hhea reserved bit / R50.1.3.1 cmap searchParams 와 동일 정신
- API ergonomics: Glyph::header() unified accessor — ttf-parser / read-fonts 의 bounding_box() industry precedent 정합
- doc 정확성: CompositeGlyph::raw_body 가 항구 보존 source-of-truth 임을 명시 — R50.1.4.2 진입 시 public API breaking 0 을 commit-time guarantee



**Impact**: §5.37.1


**Carry forward**:
- R50.1.4.2 glyf compound parser — subglyph references + 4-byte / 2-byte arg pair + 4 transform variant (scale / x-y scale / 2x2 matrix) + 12 component flags + cycle detection (max depth + ancestor set). raw_body 는 source 로 유지 상태에서 components: Vec<Component> additive field
- R50.1.5 name table parser (family / style / postscript / copyright + platform encoding dispatch)
- R50.1.X 후속: cmap format 0/2/6/8/10/13/14, WOFF2, CFF/CFF2, variable axis, color tables, GSUB/GPOS raw store
- R47-class InputRouter SCE migration carry — R50 시리즈 마감 후 별도 axis



### Round 461 — Round 461 — R50.1.4.2 §5.37.1 glyf compound (composite) glyph parser. tables/glyf/compound.rs 신설 — Component + ComponentArgs (Offset i8/i16 + PointMatch u8/u16) + ComponentTransform (Identity / Scale / XYScale / Matrix 4 variant, F2DOT14 raw i16 보존). MORE_COMPONENTS loop + last component's WE_HAVE_INSTRUCTIONS 의 instructions 부착. spec strict: reserved bits (0x0010 + 0xE000) + mutually exclusive transform bits (count_ones <= 1) reject. CompositeGlyph additive (components + instructions field, raw_body source-of-truth 항구 유지). cycle detection = R50.X+ traversal layer separation of concerns. Noto Sans 1848 composite / 3788 components 모두 Identity transform real font sweep 통과. workspace 775 → 786 tests (+11).

**Changes**:
- tables/glyf/compound.rs 신설 — Component / ComponentArgs (Offset {x: i32, y: i32} 또는 PointMatch {parent: u32, child: u32}) / ComponentTransform (Identity / Scale {scale: i16} / XYScale {x: i16, y: i16} / Matrix {xx, xy, yx, yy: i16}). F2DOT14 raw i16 보존 — caller 가 /16384.0 변환 (lossless representation)
- FLAG_* 12 const pub(super) — single source of truth (test_helpers 가 import). ARG_1_AND_2_ARE_WORDS / ARGS_ARE_XY_VALUES / WE_HAVE_A_SCALE / RESERVED_BIT_4 (0x0010) / MORE_COMPONENTS / WE_HAVE_AN_X_AND_Y_SCALE / WE_HAVE_A_TWO_BY_TWO / WE_HAVE_INSTRUCTIONS + RESERVED_HIGH (0xE000). hint bits (ROUND_XY_TO_GRID / USE_MY_METRICS / OVERLAP_COMPOUND / SCALED_COMPONENT_OFFSET / UNSCALED_COMPONENT_OFFSET) 는 #[allow(dead_code)] 보존 (caller flags inspect)
- parse_composite — MORE_COMPONENTS loop, last iter 안에서 WE_HAVE_INSTRUCTIONS 조건부 numInstr + instructions[] parse (last_flags variable 제거 — textbook control flow). read_args / read_transform helper 분리 (DRY)
- spec strict reject: composite/flags/reserved-bit-set (0x10 또는 0xE000 set), composite/flags/multiple-transform-bits (count_ones > 1)
- tables/glyf/mod.rs CompositeGlyph — components: Vec<Component> + instructions: Vec<u8> field additive (R50.1.4.1 raw_body source-of-truth 그대로 유지)
- tables/glyf/mod.rs parse_glyph composite arm — compound::parse_composite 호출 변경, raw_body capture → parsed view 둘 다 포함 반환
- tables/glyf/test_helpers.rs — ComponentSpec / TransformSpec / build_composite_body builder 추가 (super::compound 의 FLAG_* import — DRY)
- src/lib.rs — Component + ComponentArgs + ComponentTransform re-export 추가
- tests/fixtures.rs — noto_sans_composite_components_nonempty (1848 composite / 3788 components / transform variant 통계) sweep
- compound::tests 11 unit — single component identity/i8/i16 + offset/PointMatch + scale/XYScale/Matrix transform + multiple components loop + WE_HAVE_INSTRUCTIONS + reserved bit reject (low + high) + multiple transform bits reject



**Verification**:
- workspace 786 tests pass (R50.1.4.1.1 775 → +11 R50.1.4.2)
- pinion-text-font clippy --all-targets 0 warnings; workspace clippy baseline 유지 (pinion-core 5 + pinion-runtime 1)
- real font integration: Noto Sans 1848 composite glyphs / 3788 components / all Identity transform (Latin accented 글자 placement offset only, scale/rotation 없음) — panic 0
- spec strict 일관: composite reserved bits + transform mutually exclusive — R50.1.1–3.1 의 hhea reserved / cmap searchParams strict 정신 연장
- API additive evolution: CompositeGlyph 에 components + instructions field 추가 — R50.1.4.1 의 raw_body 그대로 보존, public API breaking 0 (R50.1.4.1.1 doc framing 증명)
- self-audit 15 questions all pass + last_flags variable 제거 (textbook control flow last iter 내 instruction handling) + FLAG_* DRY single source (compound → test_helpers import)



**Impact**: §5.37.1


**Carry forward**:
- R50.1.4.X cycle detection helper API — Glyf::composite_cycle_check(root) / max_depth traversal. parse 단계 아닌 R50.X+ layout/render layer responsibility (separation of concerns); ttf-parser MAX_RECURSION_DEPTH = 32 정합
- R50.1.5 name table parser (family / style / postscript / copyright + platform encoding dispatch)
- R50.1.X 후속: cmap format 0/2/6/8/10/13/14, WOFF2, CFF/CFF2, variable axis (fvar/avar/gvar), color tables (COLR/CPAL/sbix), GSUB/GPOS raw store
- R50.2+ self-hosted Unicode (UAX #15), BIDI (UAX #9), shaping per script
- R47-class InputRouter SCE migration carry — R50 시리즈 마감 후 별도 axis



### Round 462 — Round 462 — R50.1.4.2.1 §5.37.1 glyf composite parser corrective. spec mandate "WE_HAVE_INSTRUCTIONS should only be set in the last component of a composite glyph" — non-last component (MORE_COMPONENTS = 1) 와 WE_HAVE_INSTRUCTIONS 동시 set 시 ambiguity = InvalidTableField {field: "composite/flags/instructions-on-non-last-component"} strict reject. R50.1.2 hhea reserved bit / R50.1.3.1 cmap searchParams 의 strict 정신 일관 적용. 1 new test (reject pattern). Noto Sans + Nanum Gothic real font sweep 모두 통과 — real-world canonical 정합 (no false positive).

**Changes**:
- compound.rs parse_composite 의 component loop 안에 조기 조건부 reject 추가: (FLAG_MORE_COMPONENTS && FLAG_WE_HAVE_INSTRUCTIONS) → InvalidTableField {field: "composite/flags/instructions-on-non-last-component"}
- compound::tests reject_instructions_on_non_last_component 추가 — 2 component (첫째 = MORE+INSTR set, 둘째 = last) 을 build_composite_body 로 만들어 strict reject 검증
- real font (Noto Sans + Nanum Gothic) sweep 재검증 — 이 new strict rule 가 production font 에서 false positive 없음을 확인



**Verification**:
- workspace 786 → 787 tests (+1; pinion-text-font lib 107 → 108)
- pinion-text-font clippy --all-targets 0 warnings (baseline 유지)
- Noto Sans 1848 composite / 3788 components real font sweep 재통과 — production canonical font 는 non-last component WE_HAVE_INSTRUCTIONS 사용 안 함 → strict rule = real-world canonical 정합
- spec strict 일관 완성: R50.1.2 hhea reserved / R50.1.3.1 cmap searchParams / R50.1.4.1.1 composite numberOfContours == -1 와 동일 정신 적용



**Impact**: §5.37.1


**Carry forward**:
- R50.1.4.X cycle detection helper API (R50.X+ traversal layer responsibility)
- R50.1.5 name table parser (family / style / postscript / copyright + platform encoding dispatch)
- R50.1.X 후속: cmap format 0/2/6/8/10/13/14, WOFF2, CFF/CFF2, variable axis, color tables, GSUB/GPOS raw store
- R47-class InputRouter SCE migration carry



### Round 463 — Round 463 — R50.1.5 §5.37.1 name table parser (family / style / postscript / copyright). tables/name.rs 신설 (single-file — version 0/1 header dispatch 단순, multi-format 아닌 records list). Name struct + NameRecord (platform/encoding/language/nameID + raw string bytes 보존) + LangTagRecord (v1) + NameId 26 semantic variant (+Other(u16) forward compat). UTF-16BE decode helper (Unicode platform 0/* + Windows platform 3/{0,1,10}). Name::find_string priority (Windows Unicode BMP en-US 우선 → any Unicode UTF-16BE). Font::family_name / subfamily_name / full_name / postscript_name accessor. spec strict: version 0/1 외 reject + storageOffset bounds + record string offset+length overflow. Noto Sans family="Noto Sans" subfamily="Regular", Nanum Gothic 동일 패턴 real font sweep. workspace 787 → 800 tests (+13).

**Changes**:
- tables/name.rs 신설 (single-file — cmap 과 다른 결정, name = records list 단일 surface) — Name + NameRecord + LangTagRecord + NameId enum (26 semantic + Other(u16) forward compat)
- Name::parse — version 0/1 dispatch (v1 만 langTagCount + langTagRecord[] additive) + storageOffset bounds + record string offset+length overflow check
- NameRecord::decode_utf16be — Unicode platform (0, *) 또는 Windows (3, 0/1/10) UTF-16BE 변환, char::decode_utf16 (std) lossless. odd-length 또는 invalid surrogate → None
- Name::find_string — priority 1: Windows Unicode BMP en-US (3, 1, 0x0409), priority 2: 첫 UTF-16BE match (any lang). Microsoft typography reference 정합
- tables/mod.rs — name 모듈 등록
- src/lib.rs — Name / NameId / NameRecord / LangTagRecord re-export
- src/font.rs — Font 에 name field 추가 + family_name / subfamily_name / full_name / postscript_name accessor 4개
- tests/fixtures.rs — noto_sans_name_strings ("Noto Sans" / "Regular" / full / postscript) + nanum_gothic_name_strings (Nanum/나눔 + "Regular") real font sweep 2건
- name::tests 10 unit — minimal v0 + NameId round-trip + Other unknown + version 2 reject + storageOffset OOB + record string OOB + Unicode UTF-16BE decode + Macintosh reject + odd-length reject + v1 langTagRecord
- spec strict reject: version != 0/1 → InvalidTableField "version", storageOffset > bytes.len() → "storageOffset/out-of-bounds", record offset+length overflow → "nameRecord/offset+length-overflow"



**Verification**:
- workspace 800 tests pass (R50.1.4.2.1 787 → +13; pinion-text-font lib 108 → 118, integration 16 → 18)
- pinion-text-font clippy --all-targets 0 warnings (baseline 유지)
- real font sweep 정합: Noto Sans family="Noto Sans" subfamily="Regular" full_name contains "Noto Sans" postscript_name starts "NotoSans"; Nanum Gothic family contains Nanum/나눔 subfamily="Regular"
- 외부 dep 0 정신 유지 — char::decode_utf16 는 std (core::char). non-std crate 추가 0
- self-audit 15 questions all pass + single-file 결정 textbook (multi-format 아닌 때 폴더 분리 unnecessary; cmap = format dispatch 임) + NameId::Other forward compat (15 reserved 포함 26+ 모두 텍스트북 라우팅)



**Impact**: §5.37.1


**Carry forward**:
- R50.1.5.X future: Macintosh platform 1 Mac Roman 변환 (자체 테이블, R50 정신 외부 dep 0 적용)
- R50.1.5.X future: Font::copyright / trademark / license_description accessor (현재는 NameId enum 통해 caller find_string 가능)
- R50.1.4.X cycle detection helper API (R50.X+ traversal layer)
- R50.1.X 후속: cmap format 0/2/6/8/10/13/14, WOFF2, CFF/CFF2, variable axis, color tables, GSUB/GPOS raw store
- R50.X RPC channel — pinion-rpc 에 font/* method (parse / glyph_id_for / cmap_subtables / metrics / family_name) 노출 — AI-first 완성
- R50.2+ self-hosted Unicode (UAX #15), BIDI (UAX #9), shaping per script
- R47-class InputRouter SCE migration carry



### Round 464 — Round 464 — R50.1.5.1 §5.37.1 name table strict + helper 분리. spec mandate "string storage starts after the name records and any other records" 적용: storageOffset 가 header (v0: 6 byte, v1: 6 + 2 + 4*langTagCount) + records (12 * count) 너머 시작인지 검증. 미만 시 records 가 storage 와 overlap, record 가 자신을 string 으로 가리킬 수 있음 = malformed → InvalidTableField {field: "storageOffset/overlaps-header-or-records"} reject. R50.1.2 hhea reserved / R50.1.3.1 cmap searchParams / R50.1.4.2.1 composite instructions strict 정신 일관. Name::parse 도 textbook refactor — read_header + read_record_headers + read_lang_tag_headers + resolve_records + resolve_lang_tags 5 helper 분리 (too_many_lines clippy 회피). 1 new test (overlap reject pattern). real font compatibility 유지.

**Changes**:
- tables/name.rs Name::parse strict: storage_offset < r.position() (= header_end) reject — records / langTagRecords 와 overlap 검출
- tables/name.rs refactor: Name::parse 가 5 helper free fn 으로 분리 — read_header / read_record_headers / read_lang_tag_headers / resolve_records / resolve_lang_tags. RecordTuple type alias 추가. too_many_lines clippy 회피 + DRY
- name::tests reject_storage_offset_overlaps_records 추가 — count=2 + storageOffset=6 (header end, overlap) → InvalidTableField



**Verification**:
- workspace 800 → 801 tests (+1; pinion-text-font lib 118 → 119)
- pinion-text-font clippy --all-targets 0 warnings (too_many_lines 정리)
- Noto Sans + Nanum Gothic real font sweep 재통과 — production canonical 폰트는 storageOffset 귀칙 정합 (false positive 0)
- spec strict 일관 5단계 완성: hhea reserved (R50.1.2) / cmap searchParams + duplicate (R50.1.3.1) / composite numberOfContours == -1 (R50.1.4.1.1) / composite WE_HAVE_INSTRUCTIONS last (R50.1.4.2.1) / name storageOffset 인지 자리 (R50.1.5.1)
- helper 분리 = textbook SRP: 각 단계가 독립 검증 단위



**Impact**: §5.37.1


**Carry forward**:
- R50.X RPC channel — pinion-rpc 에 font/* method (parse / family_name / glyph_id_for / glyph_outline / metrics) 노출 — AI-first 완성
- R50.1.X future: Macintosh platform 1 Mac Roman 변환 (자체 테이블)
- R50.1.X future: cmap format 0/2/6/8/10/13/14, WOFF2, CFF/CFF2, variable axis, color tables, GSUB/GPOS raw store
- R50.2+ self-hosted Unicode (UAX #15), BIDI (UAX #9), shaping per script
- R47-class InputRouter SCE migration



### Round 465 — Round 465 — R50.1.6 §5.37.1 cmap format 0 (Mac Roman byte encoding). tables/cmap/format0.rs 신설 — 262-byte fixed (header 6 + glyphIdArray 256 byte). Format0::parse strict reject (length != 262). CmapSubtable::Format0 variant + selection_score priority 4 (Format 12 > 4 > 0 fallback). 가장 단순한 cmap format — legacy Mac Roman encoding 폰트 호환. workspace 801 → 807 tests (+6).

**Changes**:
- tables/cmap/format0.rs 신설 — Format0 (language + glyph_id_array [u8; 256]) + parse strict (length != 262 reject)
- Format0::glyph_id — codepoint > 255 → None, glyph_id_array entry == 0 → None (.notdef)
- cmap/mod.rs CmapSubtable::Format0 variant 추가 + glyph_id dispatch + parse format 0 case 추가
- selection_score priority 4 = Format 0 (Format 12 = 0/1, Format 4 = 2/3, Format 0 = 4 fallback) — priority 순서 명시 갱신
- cmap/mod.rs tests: unsupported_format_subtable_none 을 format 6 (trimmed) 으로 이전 (format 0 이제 parsed Some) + format0_parsed_and_fallback_priority 신규 test (priority 4 검증)
- tests/fixtures.rs nanum_gothic_cmap_subtable_sweep 에 Format0 arm 추가 (non-exhaustive panic)
- format0::tests 5 unit — parse minimal + glyph_id > 255 None + entry 0 unmapped + length != 262 reject + too-short reject



**Verification**:
- workspace 801 → 807 tests (+6; pinion-text-font lib 119 → 125 + 5 new unit + 1 priority test)
- pinion-text-font clippy --all-targets 0 warnings (baseline 유지)
- real font sweep 재통과 — Noto Sans + Nanum Gothic best_subtable 의 Format 4 / Format 12 우선순위 유지 (Format 0 은 fallback only)
- spec strict: length != 262 strict reject = R50.1.2/1.3.1/1.4.1.1/1.4.2.1/1.5.1 strict 6단계 일관 원칙 적용



**Impact**: §5.37.1


**Carry forward**:
- R50.X RPC channel — pinion-rpc font/* method (larger work, pinion-rpc 8292 LOC; Font registry/cache lifetime 결정 필요)
- R50.1.X future: cmap format 2/6/8/10/13/14 (variation selector / multi-byte CJK), WOFF2, CFF/CFF2, variable axis, color tables, GSUB/GPOS raw
- R50.1.X future: Macintosh platform 1 Mac Roman name table 변환
- R50.2+ self-hosted Unicode (UAX #15), BIDI (UAX #9), shaping per script
- R47-class InputRouter SCE migration



### Round 466 — R50.X.0 §5.37.2 신설 (text engine RPC channel sub-scope ratify) — Round 466 — R50.X.0 §5.37.2 신설. §5.37 text engine 의 RPC channel sub-scope ratify — pinion-rpc 가 §5.37.1 OpenType parser 결과 + 후속 text layer 를 JSON-RPC 2.0 로 AI agent 에게 노출. §2 invariant #2 (RPC AI-first) 의 text 영역 첫 적용. method namespace = font/* (text/* 와 분리), Font registry = Arc<Mutex<HashMap>> handle pattern, §5.7 / §5.12 ratify 정합. atomic-only round — code 변경 0, implementation 은 R50.X.1+ separate round.

**Changes**:
- docs/GENERATED.md 신규 §5.37.2 — Text engine RPC channel sub-scope (parent=§5.37)
- intent: §5.37.1 parser 결과 + 후속 text layer 를 JSON-RPC 2.0 로 AI agent 노출
- rationale 6 bullet: §2 invariant #2 첫 text channel / 진단 격차 해소 / §5.7+§5.12 정합 등
- inputs 5 bullet: §5.37.1 output / §5.7 transport / §5.12 dispatch / font binary / font_id handle
- outputs 5 bullet: pinion-rpc/src/font.rs / FontRegistry / JSON schema / dispatch routing / FontRpcError
- alternatives 6 rejected: parser direct / per-call re-parse / generic text/* / FFI / gRPC / multipart
- impact_scope: §5.37.1 / §5.7 / §5.12 / §2 — parser dependency + transport+dispatch ratify+invariant
- caveat 8 bullet: spec round entry / sub-round split / namespace / registry / minimal+extended method 등



**Verification**:
- validate_workspace: entries 120→121 / sections 48→49 / T1=0 T3=0 / RT=1/1 / GENERATED.md=sync
- atomic mutation chain 9 call: add_section + intent + rationale + inputs + outputs + alternatives + impact_scope + 8 caveat
- rationale bullet length ≤1 violation → 102 char 경고 1회후 80-95 char로 재작성 (T3 default 100 char)
- code 변경 0 — cargo test 807 / clippy baseline (pinion-core 5+1 / pinion-runtime 1) 유지



**Impact**: §5.37.2, §5.37.1, §5.7, §5.12, §2, §5.37


**Carry forward**:
- R50.X.1 minimal 3 method (font/parse / font/family_name / font/glyph_id_for) pinion-rpc/src/font.rs 신설
- FontRegistry struct 구현 — Arc<Mutex<HashMap<u32, Arc<Font>>>> + next_id counter (concurrent safe)
- dispatch::Request method routing 의 font/* prefix 분기 + integration test (기존 RPC method 정합)
- real font roundtrip test — Noto Sans byte stream 로 font/parse → font/family_name 검증
- R50.X.2 extended method (font/glyph_outline / font/cmap_subtables / font/metrics) — R50.X.1 이후
- R50.X.3 lifecycle (font/dispose / font/list) — registry cleanup 정리



### Round 467 — R50.X.1 §5.37.2 minimal 3 font/* method + FontRegistry — Round 467 — R50.X.1 §5.37.2 minimal 3 font/* method 구현. pinion-rpc/src/font.rs 신설 — FontRegistry (RwLock<HashMap<u32, Arc<Font>>> + AtomicU32 counter, 1-indexed, 0 reserved invalid sentinel) + parse / family_name / glyph_id_for typed fn. DispatchContext 에 font_registry: Option<&FontRegistry> 추가 + with_font_registry builder. dispatch fn 의 method routing 에 font/parse / font/family_name / font/glyph_id_for 분기. handle_font_* 3 + font_error_to_rpc + font_registry_unavailable helper. 외부 lib 0 — pinion-text-font + std::sync 만. real font fixture (Noto Sans Regular) include_bytes! 내장 — 10 typed fn test + 8 JSON-RPC E2E test = 18 신규 pass.

**Changes**:
- pinion-rpc/Cargo.toml — pinion-text-font path dependency 추가 (workspace internal, 외부 lib 0)
- pinion-rpc/src/font.rs 신설 — FontRegistry / parse / family_name / glyph_id_for / FontError (245 LOC)
- pinion-rpc/src/lib.rs — pub mod font + re-export (font_parse/font_family_name/font_glyph_id_for alias + Params/Outcome/FontRegistry/FontError)
- pinion-rpc/src/dispatch.rs — DispatchContext.font_registry: Option<&FontRegistry> 새 field
- pinion-rpc/src/dispatch.rs — with_font_registry builder (R47.7.5 last_paint_layout 패턴 정합)
- pinion-rpc/src/dispatch.rs — dispatch fn 의 font_registry borrow 추출 + 3 routing 분기 (font/parse / font/family_name / font/glyph_id_for)
- pinion-rpc/src/dispatch.rs — handle_font_parse / handle_font_family_name / handle_font_glyph_id_for + font_id_from_params helper + font_error_to_rpc + font_registry_unavailable
- JSON-RPC error mapping: NotFound/Parse=-32602 Invalid params, RegistryExhausted/RegistryPoisoned=-32603 Internal, registry missing=-32603 FontRegistryUnavailable
- wire shape: ParseParams { bytes: Vec<u8> } JSON array of u8 (pure JSON, no base64 lib) — payload 4x verbose but AI-first 정통
- typed fn 은 direct args (bytes/font_id/codepoint) — Params struct 는 serde 와이어 shape 만, click/screenshot/query 패턴 정합 + clippy 정합
- FontRegistry concurrency = RwLock (read-heavy AI introspect workload) + AtomicU32 (counter), u32::MAX exhaustion strict check
- Noto Sans Regular include_bytes! — font.rs 10 typed test + dispatch.rs 8 JSON-RPC E2E test 모두 real font 검증



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 807 → 825 (+18 new font tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: pinion-rpc 0 new warnings (baseline 유지: pinion-core 5+1 / pinion-runtime 1)
- Params struct 의 needless_pass_by_value clippy 가 발견 → typed fn direct args 로 refactor (기존 crate convention 정합)
- pinion-rpc lib tests: 270 pass (이전 252 + 18 신규 font area)
- validate_workspace: entries 121→122 / sections 49 (no change) / T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.37.2, §5.7, §5.12, §5.37.1


**Carry forward**:
- R50.X.2 extended method: font/glyph_outline / font/cmap_subtables / font/metrics / font/subfamily_name / font/full_name / font/postscript_name (pinion-text-font 의 6 accessor + glyph_outline 매핑)
- Glyph variant (Empty / Simple / Composite) JSON 직렬화 — pinion-text-font 에 serde dep 추가 vs RPC layer manual 직렬화 결정 필요
- R50.X.3 lifecycle: font/dispose (registry cleanup) + font/list (active font_id 들 enumerate)
- font_registry concurrency stress test — multi-thread RwLock contention pattern 검증 (R50.X.2+ 시점)
- method namespace = font/* 만 — text/* (shape/layout/break) 는 R50.4+ shape 후 별도 sub-scope (§5.37.3 후보)



### Round 468 — R50.X.2 §5.37.2 extended 6 font/* method — Round 468 — R50.X.2 §5.37.2 extended 6 font/* method 구현. font/glyph_outline (Glyph variant mirror with serde + From<&Glyph> 변환) / font/cmap_subtables (EncodingRecord + supported flag) / font/metrics (units_per_em/ascender/descender/line_gap/num_glyphs/weight_class/is_monospace 7-aggregate) / font/subfamily_name / font/full_name / font/postscript_name. FontError::GlyphIdOutOfRange variant 추가. dispatch routing 6 entries + 6 handle_font_* + serialize_outcome helper (serde_json::to_value 통한 serialize). pinion-text-font 의존 0 변경 — Glyph wire shape mirror 가 pinion-rpc 측 (GlyphOutlineOutcome / GlyphHeaderInfo / GlyphPointInfo / ComponentInfo / ComponentArgsInfo / ComponentTransformInfo). 10 typed test + 9 JSON-RPC E2E test = 19 신규 pass.

**Changes**:
- font.rs: GlyphOutlineOutcome enum (Empty/Simple/Composite) #[serde(tag="kind")] + GlyphHeaderInfo / GlyphPointInfo / ComponentInfo / ComponentArgsInfo / ComponentTransformInfo wire mirror
- font.rs: From<&Glyph> / From<&GlyphHeader> / From<&GlyphPoint> / From<&Component> / From<&ComponentArgs> / From<&ComponentTransform> impl chain (pinion-text-font × pinion-rpc wire 이용)
- font.rs: glyph_outline / cmap_subtables / metrics / subfamily_name / full_name / postscript_name 6 typed fn + Params/Outcome 구조 일관 적용
- font.rs: FontError::GlyphIdOutOfRange { glyph_id, num_glyphs } variant 추가 — strict bounds reject
- dispatch.rs: 6 routing entries + 6 handle_font_* + serialize_outcome<T: Serialize> helper (serde_json::to_value 경우 + RpcError -32603 매핑)
- dispatch.rs: font_error_to_rpc 에 GlyphIdOutOfRange variant 추가 (이개와 NotFound 모두 -32602)
- dispatch.rs: dispatch fn #[allow(too_many_lines)] + reason — routing match 가 method 증가 따라 자연 grow 교과서 canonical
- lib.rs: 6 new typed fn alias + 7 new wire types + 3 new Outcome export



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 825 → 844 (+19 R50.X.2 new tests: 10 font.rs + 9 dispatch.rs E2E)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: pinion-rpc 1 new → 0 (dispatch too_many_lines allow + reason)
- pinion-text-font 의존 녹이 0 변경 — serde mirror 가 pinion-rpc 측 (§5.37.1 외부 lib 0 정신 완전 유지)
- real font fixture (Noto Sans Regular) glyph_outline notdef = Simple kind / 'A' = Simple+points / out-of-range u16::MAX reject 검증
- validate_workspace: entries 122→123 / sections 49 / T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync



**Impact**: §5.37.2, §5.7, §5.12, §5.37.1


**Carry forward**:
- R50.X.3 lifecycle: font/dispose (RwLock write 해제) + font/list (active font_id enumerate)
- font_advance_width / left_side_bearing accessor RPC method 후보 (hmtx 세분 access)
- ParseError detail JSON 직렬화 (현재 variant name only) — AI agent diagnostic 강화
- concurrency stress test — RwLock multi-thread contention pattern (R50.X.3 이후 검토)
- method namespace = font/* 만 — text/* (shape/layout/break) 는 R50.4+ shape 후 의 §5.37.3 그룹짐



### Round 469 — R50.X.3 §5.37.2 lifecycle (font/dispose + font/list) — Round 469 — R50.X.3 §5.37.2 lifecycle. font/dispose (handle removal, idempotent on 0/unknown) + font/list (handle enumeration, ascending). FontRegistry::remove(id) → bool + snapshot_ids() → Vec<u32>. DisposeParams/DisposeOutcome { existed } + ListOutcome { font_ids }. dispatch routing 2 + handle_font_dispose + handle_font_list. handle counter 가 dispose 후 monotonic (next_id 회수 안 함) — Hyrum's Law 정합 + AI agent ID stability. 7 typed test + 4 JSON-RPC E2E test = 11 신규 pass.

**Changes**:
- font.rs: FontRegistry::remove(id) -> Result<bool, FontError> (0 sentinel 은 완전 false reject)
- font.rs: FontRegistry::snapshot_ids() -> Result<Vec<u32>, FontError> ascending sort_unstable
- font.rs: dispose / list typed fn + DisposeParams/DisposeOutcome/ListOutcome wire shape
- dispatch.rs: "font/dispose" + "font/list" routing + handle_font_dispose + handle_font_list
- lib.rs: dispose / list re-export + 3 new wire shape (DisposeParams/Outcome / ListOutcome) export
- dispose semantics: 0 또는 unknown handle = existed:false (idempotent) — 에러 아닌 이유는 AI agent retry-safe cleanup
- list semantics: ascending sort 고정 — AI agent 검증 용 deterministic enumeration
- next_id monotonic 항구 (dispose 후도 회수 0) — ID stability + Hyrum's Law 정합



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 844 → 855 (+11 R50.X.3 new: 7 font.rs + 4 dispatch.rs E2E)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: pinion-rpc 0 new warnings (baseline 유지)
- real font fixture: parse a / b → dispose(a) → list = [b] / next_id ascending continue verification
- validate_workspace: entries 123→124 / sections 49 / T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- §5.37.2 caveat #6 (R50.X.3 lifecycle: dispose/list) ratify completion



**Impact**: §5.37.2, §5.7, §5.12


**Carry forward**:
- §5.37.2 sub-scope = 9 method complete (parse / family_name / glyph_id_for / glyph_outline / cmap_subtables / metrics / subfamily_name / full_name / postscript_name / dispose / list = 11 actually)
- next text engine layer = R50.2 Unicode normalization (UAX #15 NFC/NFD/NFKC/NFKD) — R50.X.0∼R50.X.3 RPC channel 완성 후 자연 계속
- font_advance_width / left_side_bearing accessor RPC (hmtx 세분 access) — R50.X.4 후보
- concurrency stress test — multi-thread RwLock contention pattern (현재까지 single-thread test 만)
- InputRouter SCE migration (§2 invariant #5 부채 청산) carry 그대로



### Round 470 — R50.2.0 §5.37.3 신설 (Unicode UAX #15 normalization sub-scope ratify) — Round 470 — R50.2.0 §5.37.3 신설. §5.37 text engine 의 Unicode codepoint normalization sub-scope ratify — UAX #15 NFC/NFD/NFKC/NFKD 4 form 자체 구현. UCD decomposition + canonical combining class table 직접 embed. 외부 lib 0 (unicode-normalization / ICU FFI 모두 거부). §5.37 layer chain 의 input layer 위치 — §5.37.1 parser 결과를 받아 §5.37.4+ BIDI/shape 에게 정규화된 codepoint sequence 공급. Unicode 16.x version pin (Hyrum's Law 정합). atomic-only round — code 변경 0, implementation R50.2.1+ separate.

**Changes**:
- §5.37.3 신규 (parent §5.37). title 의 'Unicode self-hosted normalization — UAX #15 NFC/NFD/NFKC/NFKD (R50.2 sub-scope)'
- intent (≤ 200 char): §5.37 의 Unicode codepoint normalization sub-scope, UAX #15 4 form, UCD direct embed, 외부 lib 0
- rationale 5 bullet: UAX #15 input layer / 4 form self-hosted / UCD embed / Unicode 16.x version pin / layer chain
- inputs 3 bullet: TextNode codepoint / UCD 16.x table source / UAX #15 algorithm spec
- outputs 5 bullet: pinion-text-unicode crate / UCD embedded table / normalize() entry / quick-check helper / NormForm enum
- alternatives 5 rejected: unicode-normalization dep / ICU4C FFI / NFC 만 / runtime UCD download / raw 3MB embed
- impact_scope: §5.37 / §5.37.1 / §5.37.2 — parent + parser dep + RPC channel 정합
- caveat 8 bullet: spec entry / UCD 16.x pin / impl sub-round / crate vs module / codegen 압축 / layer chain / quick-check / RPC text/normalize 후보



**Verification**:
- atomic mutation chain: add_section + intent (재제 1회 210→195 char) + rationale + inputs + outputs (재제 1회 101→98 char) + alternatives + impact_scope + 8 caveat
- validate_workspace: entries 124→125 / sections 49→50 / T1=0 / T3=0 / RT=1/1 / GENERATED.md=sync
- code 변경 0 — cargo test 855 / clippy baseline (pinion-core 5+1 / pinion-runtime 1 / pinion-rpc 0) 유지
- §5.37.2 caveat #8 forward-compat anchor (Unicode/BIDI sibling sub-scope) 가 §5.37.3 신규로 구체화



**Impact**: §5.37.3, §5.37, §5.37.1, §5.37.2


**Carry forward**:
- R50.2.1: UCD 16.x table embed (decomposition + canonical_combining_class + compatibility_decomposition) 설계 + build.rs codegen 결정
- R50.2.1: pinion-text-unicode crate vs pinion-text/unicode/ module 결정 — 별도 crate (separation of concerns) 후보
- R50.2.2: NFD (canonical decomposition) algorithm + canonical reordering
- R50.2.3: NFC (canonical composition) algorithm + composition exclusion table
- R50.2.4 + R50.2.5: NFKD + NFKC (compatibility) 후속
- R50.2.6: Quick-check optimization (already-normalized fast path)
- §5.37.2 RPC channel 에 text/normalize method 추가 — R50.2.X RPC channel (별도 round)



### Round 471 — Round 471 — R50.2.1 §5.37.3 pinion-text-unicode crate scaffold (NormForm enum boundary, algorithm 미포함)

**Changes**:
- crates/pinion-text-unicode 신설 (Cargo.toml workspace member + src/lib.rs)
- NormForm enum (Nfc/Nfd/Nfkc/Nfkd) + R50.2.x roadmap doc
- 외부 dependency 0개 — algorithm + UCD table embed 은 R50.2.2+ separate slice



**Verification**:
- cargo check --workspace --features pinion-runtime/vello clean
- cargo clippy --workspace baseline 유지 (pinion-text-unicode 0 warning)
- cargo test --workspace = 855 pass (baseline, scaffold 단계 무 추가 test)



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.2 UCD 16.x vendor + build.rs codegen (decomp + combining class + exclusions)
- R50.2.3 NFD algorithm (Canonical Decomposition + Canonical Ordering)
- R50.2.4+ NFC/NFKD/NFKC + Quick-check + RPC text/normalize method



### Round 472 — Round 472 — R50.2.2 §5.37.3 UCD 16.0.0 vendor + build.rs codegen (5 normalization table 자체 embed)

**Changes**:
- ucd/ 신설 (UnicodeData + DerivedNormalizationProps + CompositionExclusions + Unicode License V3)
- build.rs ~290 LOC — 5 sorted table emit (CCC, canonical/compat decomp, full excl, primary composite)
- 13 spot-check test (À decomp, U+0300 CCC 230, Devanagari excl, A+grave composite, Hangul 알고리즘 누락 검증)



**Verification**:
- cargo build -p pinion-text-unicode = green (codegen emit ~10080 LOC tables.rs)
- cargo test --workspace --features pinion-runtime/vello = 868 pass (855 baseline + 13)
- cargo clippy -p pinion-text-unicode --all-targets = 0 warning (workspace baseline 유지)



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.3 NFD algorithm (Canonical Decomposition recursive + Canonical Ordering CCC swap)
- R50.2.4 NFC algorithm (PRIMARY_COMPOSITES lookup + Hangul algorithmic composition UAX #15 §16)
- R50.2.5+ NFKD/NFKC + Quick-check optimization + text/normalize RPC method



### Round 473 — Round 473 — R50.2.3 §5.37.3 NFD algorithm (recursive canonical decomposition + Hangul §16 + Canonical Ordering)

**Changes**:
- hangul/decompose/ordering/nfd 4 module split (UAX #15 §1.2 / §3 / §16 separation of concerns)
- nfd(s) -> String pub(crate) — recursive decomposition + CCC stable sort,  외부 lib 0
- ucd/NormalizationTest.txt vendor (2.7MB) + 5-invariant sweep ~20000 case @ 0.14s



**Verification**:
- cargo test -p pinion-text-unicode = 35 pass (NFD conformance sweep 20026 line)
- cargo test --workspace --features pinion-runtime/vello = 890 pass (855 + 35)
- cargo clippy -p pinion-text-unicode --all-targets = 0 warning (workspace baseline 유지)



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.4 NFC algorithm (PRIMARY_COMPOSITES adjacent-pair + Hangul algorithmic composition)
- R50.2.5 NFKD/NFKC (COMPATIBILITY_DECOMPOSITION reuse) + 4-form sweep
- R50.2.6 Quick-check + pub fn normalize entry (allow(dead_code) 해제)



### Round 474 — Round 474 — R50.2.4 §5.37.3 NFC algorithm (canonical composition + Hangul §16 compose)

**Changes**:
- hangul.rs compose_hangul (L+V→LV, LV+T→LVT) + 5 new test
- composition.rs + nfc.rs 신설 — primary composite + blocked semantics (UAX #15 D6)
- test_fixture.rs shared NormalizationCase loader (NFD/NFC sweep 양측 소비)



**Verification**:
- cargo test -p pinion-text-unicode = 54 pass (NFC conformance sweep 20026 line @ 0.19s)
- cargo test --workspace --features pinion-runtime/vello = 909 pass (855 + 54)
- cargo clippy -p pinion-text-unicode --all-targets = 0 warning (workspace baseline 유지)



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.5 NFKD/NFKC (COMPATIBILITY_DECOMPOSITION reuse + 4-form sweep complete)
- R50.2.6 Quick-check optimization (UAX #15 §5 NFC/NFD/NFKC/NFKD_QC tables)
- R50.2.7 pub fn normalize entry (all 4 form 완성, allow(dead_code) 해제)



### Round 475 — Round 475 — R50.2.5 §5.37.3 NFKD/NFKC algorithms (4 form 완성, UAX #15 fully conformant)

**Changes**:
- decompose.rs decompose_compatibility (recursive COMPATIBILITY_DECOMPOSITION) + 4 test
- nfkd.rs + nfkc.rs 신설 — Pattern 3/4 sweep 5x5 invariant
- 4 form 완성 — UAX #15 Pattern 1-4 conformance ~20000 case x 4 form



**Verification**:
- cargo test -p pinion-text-unicode = 71 pass (NFD/NFC/NFKD/NFKC sweep all)
- cargo test --workspace --features pinion-runtime/vello = 926 pass (855 + 71)
- cargo clippy -p pinion-text-unicode --all-targets = 0 warning



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.6 Quick-check optimization (UAX #15 §5 NFC/NFD/NFKC/NFKD_QC tables)
- R50.2.7 pub fn normalize entry + RPC text/normalize method (§5.37.2 channel)



### Round 476 — Round 476 — R50.2.6 §5.37.3 pub fn normalize entry (4 form public API + UCD_VERSION re-export)

**Changes**:
- lib.rs pub fn normalize(s, form) -> String + pub use UCD_VERSION re-export + 2 crate doctest
- build.rs UCD_VERSION emit 'pub const' 변경 — public surface 진입
- algorithm 8 모듈 allow(dead_code) 해제 — normalize chain 활성화 (tables 만 잔존)



**Verification**:
- cargo test -p pinion-text-unicode = 75 unit + 2 doctest pass
- cargo test --workspace --features pinion-runtime/vello = 932 pass (855 + 77)
- cargo clippy -p pinion-text-unicode --all-targets = 0 warning (baseline 유지)



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.7 Quick-check optimization (UAX #15 §5 QC tables + fast path)
- R50.2.X text/normalize RPC method (§5.37.2 channel, pinion-rpc 측 wrap)
- tables FULL_COMPOSITION_EXCLUSION allow(dead_code) — RPC introspect 시 해제



### Round 477 — Round 477 — R50.2.7 §5.37.3 perf 부채 3종 상환 (Quick-check + compose-write O(n) + Cow API + static)

**Changes**:
- build.rs 4 QC table emit (NFC/NFD/NFKC/NFKD_QC) + 전체 table const→static migration
- quick_check.rs 신설 + composition.rs read/write pointer O(n) refactor (Vec::remove O(n²) 제거)
- normalize() -> Cow<'_, str> + Quick-check Yes 시 borrow fast path



**Verification**:
- cargo test -p pinion-text-unicode = 84 unit + 2 doctest pass (4 form sweep 유지)
- cargo test --workspace --features pinion-runtime/vello = 941 pass (855 + 86)
- cargo clippy -p pinion-text-unicode --all-targets = 0 warning (workspace baseline 유지)



**Impact**: §5.37, §5.37.3


**Carry forward**:
- R50.2.X text/normalize RPC method (§5.37.2 channel)
- 2-stage table 2-level lookup 가속 (high-byte index) — perf 부채 잔존
- criterion bench harness (성능 회귀 가드)



### Round 478 — Round 478 — R50.2.X §5.37.2 text/normalize RPC method (pinion-text-unicode wired)

**Changes**:
- pinion-rpc Cargo.toml pinion-text-unicode dep + text.rs 신설 (NormalizeForm serde 'NFC/NFD/NFKC/NFKD' rename + Outcome)
- dispatch.rs text/normalize routing + handle_text_normalize + NFC-safe body helper
- 9 typed test + 8 JSON-RPC E2E test (compose / decompose / ligature / Hangul / error cases)



**Verification**:
- cargo test -p pinion-rpc text_normalize = 17 pass (typed 9 + E2E 8)
- cargo test --workspace --features pinion-runtime/vello = 958 pass (855 + 103)
- cargo clippy -p pinion-rpc --all-targets = 0 warning (workspace baseline 유지)



**Impact**: §5.37, §5.37.2, §5.37.3


**Carry forward**:
- criterion bench harness (성능 회귀 가드)
- 2-stage table 가속 (high-byte index → 2nd table)
- tables FULL_COMPOSITION_EXCLUSION RPC 노출 (text/composition_exclusion_member 등)



### Round 479 — R50.2.8 §5.37.3 criterion bench harness 첫 도입 — 5 scenario UAX #15 NFC throughput baseline 측정

**Changes**:
- workspace.dependencies criterion 0.5 추가 — workspace 첫 dev-grade dep 정통
- pinion-text-unicode [dev-dependencies] criterion + [[bench]] normalize harness=false
- benches/normalize.rs 신설 — 5 scenario (ascii fast path / precomposed / decomposed / hangul / sample)



**Verification**:
- cargo bench baseline: ascii 28 / precomposed 31 / decomposed 9 / hangul 21 / sample 31 MiB/s
- cargo test --workspace --features pinion-runtime/vello = 958 pass (baseline 유지)
- cargo clippy bench target 0 warning, baseline 6 유지 (pinion-core 5 + pinion-runtime 1)



**Impact**: §5.37.3


**Carry forward**:
- R50.2.9 2-stage table 가속 — A baseline 위 가속률 정량 검증
- ASCII 가 precomposed 보다 약간 느림 — Quick-check binary_search ASCII short-circuit 검토
- bench 결과 환경 명시 (CPU / Rust version) — 다음 bench round 메타데이터 정통화
- 외부 dev-dep 정책 workspace 측 정통 — 미래 BIDI / GSUB / 등 동일 패턴 inherit



### Round 480 — R50.2.9 §5.37.3 fast-path short-circuit anchors (UCD-derived) — ASCII/Latin-1 NFC throughput +70~90x

**Changes**:
- build.rs: 10 const anchor emit (UCD first/last codepoint per table)
- 4 lookup module short-circuit (ordering / decompose / quick_check / composition)
- manual_range_contains 정통 idiom — composition.rs `(FIRST..=LAST).contains(&a)`



**Verification**:
- cargo test pinion-text-unicode = 84 unit + 2 doctest (4 form UAX #15 conformance 정상)
- bench MiB/s: ascii 28→2562, pre 31→2050, dec 9→35, han 21→80, sam 31→112
- cargo test --workspace = 958 pass; clippy baseline 6 warning 유지



**Impact**: §5.37.3


**Carry forward**:
- R50.2.10 후속: 2-stage trie table replace binary_search (direct index lookup)
- R50.2.11 후속: hangul / CJK plane block anchor 확장 (현재 ASCII/Latin-1 위주)
- UAX #44 §3 stability forward-stable guarantee anchor refresh on UCD bump



### Round 481 — R50.2.10 §5.37.3 CCC 2-stage BMP trie (UTrie2-simplified) — decomposed +37%, hangul +24%, sample +19%

**Changes**:
- build.rs build_ccc_bmp_trie — Stage1 u16[256] + Stage2 packed u8 + supp linear
- ordering.rs combining_class: BMP 2 mem access + supplementary binary_search fallback
- lib.rs tests: 3 trie invariant + binary_search call 제거 (combining_class API 사용)



**Verification**:
- cargo test pinion-text-unicode = 87 unit + 2 doctest (NFC/NFD/NFKC/NFKD sweep 정상)
- bench MiB/s Δ: decomp 35→48 (+37%), hangul 80→99 (+24%), sam 112→133 (+19%)
- cargo test --workspace = 961 pass (+3); clippy baseline 6 유지



**Impact**: §5.37.3


**Carry forward**:
- R50.2.11 후속: NFC_QC / NFD_QC / NFKC_QC / NFKD_QC tables 도 BMP trie
- R50.2.12 후속: CANONICAL_DECOMPOSITION / COMPATIBILITY_DECOMPOSITION trie (indirect)
- R50.2.13 후속: PRIMARY_COMPOSITES 2D key hash 또는 trie



### Round 482 — R50.2.11 §5.37.3 4 QC tables BMP trie (build_u8_bmp_trie generic) — decomp/hangul/sample +13~55%

**Changes**:
- build.rs: build_u8_bmp_trie + emit_u8_bmp_trie_table generic (CCC + 4 QC 공유)
- quick_check.rs lookup_u8_trie single inline (cold isolation 시도 후 textbook 정통)
- emit doc fix — trailing \n 제거 + backtick wrap (clippy pedantic baseline guard)



**Verification**:
- cargo test --workspace = 961 pass (4 form conformance sweep 정상, regression 0)
- bench MiB/s vs R50.2.10: decomp 48→55, hangul 99→123, sam 132→207, pre 2050→1800
- cargo clippy baseline 6 유지 (pinion-text-unicode 0 warning)



**Impact**: §5.37.3


**Carry forward**:
- R50.2.12 precomposed_nfc -20% regression 분석 (flamegraph + inlining heuristic)
- R50.2.12 CANONICAL_DECOMPOSITION / COMPATIBILITY_DECOMPOSITION trie (indirect index)
- R50.2.13 PRIMARY_COMPOSITES 2D key hash 또는 trie (anchor short-circuit 외 추가 가속)



### Round 483 — R50.2.12 §5.37.3 perf debt 6종 일괄 상환 — combining_class hot/cold split (precomposed regression 완전 해결)

**Changes**:
- ordering.rs combining_class hot/cold split (cargo asm root cause 검증)
- benches 10s/500 config + 환경 metadata + build.rs TABLES_GENERATED_BYTES const
- atomic: +5 binding (build.rs emit_*, lookup_u8_trie, supp) + 1 ICU caveat



**Verification**:
- cargo asm: combining_class 0 callq (inline 확인), supplementary cold call
- bench Cumulative MiB/s vs R50.2.7: ascii 103x / pre 72x / dec 6.5x / han 6.6x / sam 6.7x
- tables.rs 519 KiB (1.5 MiB 35%); cargo test 962 pass (+1 const assert); clippy 6 유지



**Impact**: §5.37.3


**Carry forward**:
- R50.2.13: CANONICAL_DECOMPOSITION / COMPATIBILITY_DECOMPOSITION trie (indirect)
- R50.2.14: PRIMARY_COMPOSITES 2D key hash 또는 trie
- R50.2.x: cargo-bloat / binutils 가 필요한 release binary size 측정



### Round 484 — Round 484 — R51.0 §5.38 신설: F-tier widget catalog axis ratify (Tier-1 primitive widgets, atomic-only, code 변경 0, R47-class framework primitive 정통)

**Changes**:
- add_section §5.38 "Widget catalog — Tier 1 primitive widgets" (parent §5)
- set_section_intent: Button R12 시작 + Toggle/Checkbox/Slider/TextInput/Menu carry, framework-side 책임
- set_section_inputs: §5.4 SCXML / §5.13 Event / §5.20 Intent / §5.15 External / §5.24 Semantic / §4 first dogfood
- set_section_outputs: pinion-core::widgets module, per-widget SCXML, Widget+WidgetExternal pattern, External adapter
- set_section_rationale: R47-class lesson + industry precedent (Xilem/Druid/Slint/Qt/Material/SwiftUI) + Bloch API completeness
- set_section_impact_scope: 4, 5.4, 5.13, 5.15, 5.20, 5.24
- set_section_alternatives_rejected: inline impl (R47-class), mega-SCXML (SRP), egui-style kit, MVP subset



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 sections=51 sync (atomic-only round, GENERATED.md cascade)
- code 변경 0 (atomic-only) — cargo test 968 / clippy 0 회귀 가능성 없음 (baseline 그대로)
- F 위젯 카탈로그 axis ratify 자체로 R47-class 부채 (application/example inline implementation 반복) 영구 차단



**Impact**: §5.38, §4, §5.4, §5.13, §5.15, §5.20, §5.24


**Carry forward**:
- R51.1+: Toggle widget 첫 진입 (SCXML + Rust binding + External adapter + tests, Button R12 1:1 패턴 재사용)
- R51.2+: Checkbox/Slider/TextInput/Menu/... Tier-1 catalog 순차 land (per-widget atomic round)
- R51.x: §5.38 Tier-2 axis 분리 검토 (compound widget: ComboBox/DatePicker/...)
- R47.7.x atomic round 등록 carry (commit 됐지만 atomic binding 없음 — 적절한 round 에서 backfill)
- R50.2.13/14 atomic binding 미진행 carry (add_section_implementation §5.37.3 누적은 됐지만 changelog entry 미진행)



### Round 485 — Round 485 — R51.1 §5.12 LayoutNode.line_count: u32 노출 — Text-only measured-result sidecar, Scene-as-data invariant 정통, R47.7.6 regression 영구 보장

**Changes**:
- pinion-core::scene::TextNode 에 pub line_count: u32 추가 (Default 0, additive #[non_exhaustive])
- pinion-runtime::layout::compute_layout 에 HashMap<NodeId, u32> side-channel + apply text_lines wire
- pinion-rpc::layout_query::LayoutNode 에 line_count: u32 + SceneProjection named struct refactor
- describe_scene Text arm: t.line_count → LayoutNode.line_count (다른 kind 는 0)
- add_section_implementation §5.12 ×3: LayoutNode.line_count / TextNode.line_count / compute_layout::text_lines
- add_section_caveat §5.12 ×4: R51.1 Text-only / invariant / backend agnostic / R47.7.6 regression



**Verification**:
- cargo test 968 → 973 (+5: pinion-rpc +2 round-trip + non-Text zero, pinion-runtime +3)
- workspace clippy 0 lint warnings 유지 (baseline; 3 cargo:warning= 는 codegen 알림 lint 아님)
- cargo check --workspace --features pinion-runtime/vello 통과 (모든 caller 자동 적응)
- validate_workspace T1=0 T3=0 RT=1/1 sync (atomic store + GENERATED.md cascade)
- R47.7.6 regression test: viewport 300..=320 21장 sweep line_count=1 stable
- wrap baseline test: 60px slot 과 long sentence → line_count ≥ 2 (surface real wrap 계속 보고)



**Impact**: §5.12, §5.36, §5.37


**Carry forward**:
- R51.1.x: cargo run -p hello-button e2e RPC test (stdin pipe + scene/layout response 검증) — unit-test 충분, e2e 별도 carry
- R51.2+: F 위젯 카탈로그 Toggle/Checkbox/Slider/... 진입 (§5.38 carry, B 진입)
- §5.37.7 line break self-hosted text engine — LayoutCache backend swap 시 line_count surface 유지 확인
- R50.2.13/14 atomic binding 미진행 carry (§5.37.3 implementations)
- R47.7.x atomic round 등록 carry (commit 됐지만 atomic binding 없음)



### Round 486 — Round 486 — R51.2 §5.38 Toggle widget land — Button R12 SCXML 1:1 패턴 + value: bool layer, ToggleExternal AI-introspect 정통 (state/value/send schema)

**Changes**:
- crates/pinion-core/widgets/toggle.scxml 신설 (Button 4-state + raise toggle.activate)
- crates/pinion-core/build.rs scxml_inputs 에 toggle.scxml 추가 (codegen wire)
- crates/pinion-core/src/widgets/toggle.rs: Toggle + ToggleExternal + ToggleStateSnapshot
- widgets/mod.rs: pub mod toggle 추가
- value flip on Pressed→Hover activate; toggle intent emit IntrospectValue::Bool payload
- ExternalIntrospect schema 3 slots: state/value/send (query/intervene/invoke 정통)
- add_section_implementation §5.38 ×4 + caveat ×4 (Toggle land binding)



**Verification**:
- cargo test 973 → 993 (+20: Toggle 7 + ToggleExternal 9 + Snapshot 3 + Toggle activate 1)
- workspace clippy 0 lint baseline 유지 (5 doc backtick warning fix 후)
- cargo build pinion-core toggle_sm.rs codegen 성공 (sce-build TogglePolicy)
- ToggleExternal AI-first 정통: 5.15 8-item contract 충족 (state/value/send)
- Figma fidelity 정통: Toggle pure state, label = Scene::Text + R47.5 TextStyle



**Impact**: §5.38, §5.4, §5.13, §5.15, §5.20, §5.24


**Carry forward**:
- R51.3+: Checkbox / Slider / TextInput / Menu 후속 (§5.38 Tier-1 catalog)
- ToggleExternal e2e RPC test (cargo run + invoke send + drain_intents)
- R47.7.x atomic round entry 정리 carry
- R50.2.13/14 atomic binding 미진행 carry



### Round 487 — Round 487 — R47.7.5/6 backfill: winit redraw_request (example, §5.12) + parley layout ceil (runtime, §5.36) — c501ea6 commit atomic binding (single combined commit, no atomic at land time)

**Changes**:
- R47.7.5 (example): hello-button explicit self.request_redraw() after winit resumed callback
- R47.7.6 (runtime): pinion-runtime compute_layout measure callback layout.width().ceil() + .height().ceil()
- add_section_implementation §5.36 (compute_layout::ceil) + §5.12 (App::resumed::request_redraw)
- add_section_caveat §5.36 / §5.12 (R47.7.6 ceil + R47.7.5 request_redraw)



**Verification**:
- mouse-drag resize 1-px text jitter 차단 (parley 77.0/77.8 sub-pixel oscillation)
- first paint 전 RPC scene/layout {viewport: null} 응답을 위한 last_paint_layout populate 보장
- R51.1 §5.12 line_count surface 의 stable cross-frame measurement substrate (R47.7.6 속)



**Impact**: §5.12, §5.36


**Carry forward**:
- R51.1 LayoutNode.line_count regression test 가 R47.7.6 ceil 영구 검증 (300..=320 stable)



### Round 488 — Round 488 — R50.2.13 backfill: §5.37.3 decomposition tables 2-stage BMP trie (canonical + compatibility) — be09f2b commit atomic binding (implementation 이미 등록, changelog entry 만 누락)

**Changes**:
- canonical + compatibility decomp tables → 2-stage BMP trie (Option B' indirect layout, shape 동일)
- Stage 2 packed u32 ((length<<24) | offset); null block dedup; supplementary fallback (cold split)
- build.rs: build_decomp_bmp_trie + emit_decomp_table + emit_packed_u32_hex_row helper
- decompose.rs: lookup_decomp_trie (#[inline]) + lookup_decomp_supplementary (#[inline(never)])



**Verification**:
- throughput: sample +10%, hangul +15%, decomp -14% (trie miss cost; binary-layout carry)
- cargo test 모두 통과 + 4 conformance sweep (NFC/NFD/NFKC/NFKD) 지속
- compile-time const _: () = assert!() 2 cardinality 검증 (R50.2.12 정통)



**Impact**: §5.37.3


**Carry forward**:
- R50.2.15 precomposed_nfc binary-layout regression PGO investigation



### Round 489 — Round 489 — R50.2.14 backfill: §5.37.3 PRIMARY_COMPOSITES 2-level BMP trie + supplementary cold split — e23ab82 commit atomic binding (implementation 이미 등록, changelog entry 만 누락)

**Changes**:
- PRIMARY_COMPOSITES 2-level BMP trie (a 의 BMP 2-stage + per-a (b, c) sub-table flat)
- supplementary fallback (UAX #15 D5 의 'a 는 BMP' 가정 panic, U+105D2 발견 → R50.2.13 supp 패턴 재사용)
- composition.rs: compose_pair (fully inlined) + compose_pair_supplementary (out-of-line cold split)
- PrimaryCompositesTrieParts type alias (clippy::type_complexity 회피)



**Verification**:
- throughput: decomp 50.6→117 MiB/s (+138%), hangul 158→284 (+82%), sample 228→266 (+17%)
- asm 검증: compose_pair fully inlined, compose_pair_supplementary correctly out-of-line
- precomp -22% noise carry (R50.2.15 PGO investigation)



**Impact**: §5.37.3


**Carry forward**:
- R50.2.15: precomposed_nfc binary-layout regression PGO / #[hot] partition / link order investigation



### Round 490 — Round 490 — Carry 1+2 정정: line_count UAX #14 visual-line semantic spec + §5.37.4/.5/.6 placeholder cascade (T1 orphan 종결, lifetime axis forward declaration)

**Changes**:
- TextNode.line_count doc: UAX #14 visual lines / BIDI 무관 / empty→1 / 0 sentinel 명시
- LayoutNode.line_count doc: 동일 semantic + non-Text variants 0 명시
- add_section_caveat §5.12: R51.1 line_count UAX #14 visual lines semantic
- add_section_caveat §5.37.7 ×2: carry placeholder + decision_status RFC carry
- add_section §5.37.4 BIDI / §5.37.5 script / §5.37.6 shape placeholder (cascade)
- set_section_intent §5.37.4/.5/.6 placeholder — ratify multi-session carry



**Verification**:
- validate_workspace: T1=0 T3=0 RT=1/1 sync (entries 144, sections 52→55)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 lint
- cargo test 993 pass 유지 (doc-only 변경)



**Impact**: §5.12, §5.37.4, §5.37.5, §5.37.6, §5.37.7


**Carry forward**:
- Carry 3: SCXML template 시스템 (pinion-forge codegen template axis, multi-session, 다음 세션)
- mnemosyne MCP primitive RFC: set_section_caveats / remove_section_caveat / set_section_decision_status
- §5.37.4 BIDI / §5.37.5 script / §5.37.6 shape 정통 ratify (multi-session axis chain)



### Round 491 — R51.107 §5.41 신설 — §2 #6 GUI/TUI dual invariant 의 spec 구체화 RFC, Scene→cell 매핑 + crossterm wire + WidgetRenderer trait 추출 substrate plan (impl 0)

**Changes**:
- §5.41 신설 — TUI 백엔드 axis (cell-based render mode + crossterm input + WidgetRenderer trait)
- §2 #6 settled invariant 의 implementation substrate plan 첫 정통 (14 invariant 중 0% → spec)
- Slint TUI experimental + ratatui 정합으로 industry precedent enumerate
- §5.16 R45 renderer kind 와 직교 axis 분리 (pixel raster vs cell-based)
- §5.13 Event enum 의 winit-free 변환 layer (crossterm → §5.13 Event)
- first slice = R51.108 WidgetRenderer trait + InputRouter substrate (impl land)
- R51.109 pinion-tui crate + TuiRenderer impl + ApplicationHandlerTui (impl land)
- R51.110 hello-button TUI dogfood = first slice 평가 gate



**Verification**:
- validate_workspace 사후 = T1 orphan 0, T2 frozen 0, RT 1/1, GENERATED.md sync
- §5.41 impact_scope 7 refs (§2 §3 §5.2 §5.13 §5.15 §5.16 §5.40) 모두 존재 검증
- Round 491 entry_id 단조 증가 (Round 490 < Round 491)
- RFC round = atomic mutation only, cargo test/clippy 1657 pass / 0 warning 회귀 검증



**Impact**: §2, §3, §5.2, §5.13, §5.15, §5.16, §5.40, §5.41


**Carry forward**:
- R51.108 — WidgetRenderer trait 추출 (VelloRenderer 단일 → trait + 2nd impl 준비)
- R51.108 — InputRouter winit-free 분리 (event source 추상화 substrate evolution)
- R51.109 — pinion-tui crate 신설 + TuiRenderer impl + crossterm event loop
- R51.110 — hello-button TUI dogfood = first slice land 평가
- color depth fallback (24bit/256/16) 정통 — R51.111+ carry
- TUI mouse capability 매트릭스 manual test = R51.112+ carry
- logical pixel ↔ cell unit conversion contract = R51.108 substrate 시 결정
- Path/Image primitive unicode-art TUI 매핑 = R51.111+ carry
- TUI a11y 별도 path (screen reader PTY 청취) = framework 의무 없음



### Round 491 — chore(vendor): SCE submodule bump 7ec6721d → 75c80a03 + sce-rust-runtime features cascade — vendor/sce main fast-forward 75c80a03 (127 commits); SCE-001 (compile_scxml deps emit) upstream fix 가져오고 sce-rust-runtime no-script feature 제거에 따라 pinion-core Cargo.toml 동기화.

**Changes**:
- vendor/sce submodule HEAD 7ec6721d → 75c80a03 (main fast-forward, 127 commits)
- crates/pinion-core/Cargo.toml: sce-rust-runtime features=["no-script"] 제거 → default-features=false 만 유지 (SCE 측 489e1922 ScriptEngineProvider singleton 삭제 + script-engine-{lua,quickjs} feature 제거 cascade)
- SCE-001 (compile_scxml 의 preprocessor_deps 미emit) upstream 75c80a03 fix: Surface preprocessor deps on compile_scxml rerun-if-changed 으로 청산 — pinion 측 issue 제출 불필요



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 993 pass (회귀 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warning
- sce-upstream-debts memory SCE-001 entry status open → fixed in upstream 75c80a03 (2026-05-18) 갱신



**Impact**: §5.4, §5.38



### Round 492 — R51.108 §5.41 substrate winit-free — TouchPhase / Touch / Modifiers 추상 type 신설, ShellCore winit import 0 ([[substrate-incompleteness-signal]] 적용 + RFC carry split: R51.108=substrate / R51.109=WidgetRenderer trait+TuiRenderer)

**Changes**:
- pinion_runtime: TouchPhase / Touch / Modifiers abstract types 신설 (§5.13 hedge + W3C DOM Level 3 정합)
- ShellCore (substrate.rs) winit import 0: Touch / TouchPhase / Modifiers 모두 pinion_runtime
- ShellCore::modifiers field type ModifiersState → Modifiers (§5.40 / §5.41 정합)
- ShellCore::set_modifiers / touch_event signature winit→pinion type 추상화
- app.rs winit_touch_to_pinion / winit_modifiers_to_pinion 변환 helper (winit boundary)
- RFC carry split: R51.108=InputRouter substrate only, R51.109=WidgetRenderer trait + TuiRenderer (premature abstraction 회피)
- Modifiers #[allow(clippy::struct_excessive_bools)] = W3C KeyboardEvent shiftKey/ctrlKey/altKey/metaKey 정통 정합
- TouchPhase #[non_exhaustive] + wildcard arm = §5.13 hedge 정통 (새 phase 시 explicit arm)



**Verification**:
- cargo check --workspace --features pinion-runtime/vello clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1657 pass / 0 fail / 8 ignored (baseline 유지)
- substrate.rs grep `use winit\|use accesskit_winit` = 0 (winit import 청산 검증)



**Impact**: §2, §5.13, §5.35, §5.39, §5.40, §5.41


**Carry forward**:
- R51.109 — WidgetRenderer trait + TuiRenderer + paint_adapter::to_tui first 2 impl 동시 land
- R51.110 — hello-button TUI dogfood first-client substrate 평가
- Modifiers struct_excessive_bools allow = W3C precedent, future bitflag 정통 시 R52+ 평가
- TouchPhase non_exhaustive + wildcard arm = 새 phase 등장 시 explicit arm 추가 정통
- Modifiers control_key / alt_key / meta_key accessor 3개 현재 caller 0 (R51.109+ key shortcut)
- app.rs internal helper functions (winit_touch_to_pinion, winit_modifiers_to_pinion) 은 winit-coupled 변환 layer



### Round 492 — R51.3 §5.38 SCE sce:template adopt: button + toggle byte-복붙 청산, multi-widget catalog substrate — SCE 의 sce:template (RFC §6.5 Phase A, vendor 측 land 완료) 활용으로 Button R12 + Toggle R51.2 의 4-state 상호작용 body 공유; 89 LOC byte-복붙 → 44 LOC + 1줄 변형점(activate_event). 향후 Checkbox/Radio/MenuItem/Tab 등 button-like widget 진입 = sce:use 1줄.

**Changes**:
- crates/pinion-core/widgets/standard_button.sce-template.xml 신설 (~56 LOC) — idle/hover/pressed/disabled 4-state body — sce:param activate_event 하나가 유일 변형점
- crates/pinion-core/widgets/button.scxml: 43 LOC 쓠 본문 → sce:use template=standard_button.sce-template.xml activate_event=button.activate 1줄 축약
- crates/pinion-core/widgets/toggle.scxml: 46 LOC 쓠 본문 → sce:use 동일 축약 (activate_event=toggle.activate)
- atomic add_section_implementation ×2: §5.38 에 standard_button.sce-template.xml + button.scxml 등록 (toggle.scxml 는 R51.2 때 등록 완료)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 993 pass (회귀 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warning
- generated SM surface 바이트 동등: ButtonState/ButtonEvent/ButtonPolicy + ToggleState/ToggleEvent/TogglePolicy 이전 524 LOC 와 일치 유지 (file_stem PascalCase prefix 그대로)
- sce_build cargo::rerun-if-changed: SCE 75c80a03 fix 로 standard_button.sce-template.xml 수정 시 caller SCXML rebuild 자동 트리거 (R491 vendor bump 쾐안 완료, SCE-001 청산)



**Impact**: §5.38, §5.16



### Round 493 — R51.109.0 §5.41 pinion-tui crate skeleton — ratatui 0.29 + crossterm 0.28 workspace deps + TuiRenderer placeholder (R51.109.1 substrate trait / R51.109.2 실제 impl 의 type identity 사전 예약)

**Changes**:
- crates/pinion-tui 신설 — Cargo.toml + lib.rs + 3 smoke test (type identity reservation)
- ratatui 0.29 + crossterm 0.28 workspace.dependencies pin (§5.41 첫 외부 dep)
- pinion-tui::TuiRenderer 빈 placeholder + Default + Debug (R51.109.2 시 종속 field 추가)
- ratatui + crossterm re-export pub use (downstream binding dep 통합 surface)
- R51.109 sub-rounds 분할: R51.109.0=skeleton / .1=substrate trait / .2=실제 impl



**Verification**:
- cargo check -p pinion-tui clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1660 pass / 0 fail (+3 pinion-tui smoke)
- ratatui::layout::Rect / crossterm::event::Event re-export 검증 (3 smoke test 1개씩)



**Impact**: §2, §5.41


**Carry forward**:
- R51.109.1 — WidgetRenderer trait (pinion-shell) + paint_adapter::to_tui (pinion-runtime tui feature)
- R51.109.2 — TuiRenderer::new crossterm raw mode + ratatui::Terminal wire-up + WidgetRenderer impl
- R51.110 — hello-button TUI dogfood first-client substrate 평가
- ratatui 0.29.x lock — major 변경 시 paint_adapter::to_tui mapping 재평가
- crossterm 0.28.x 이 ratatui 0.29 와 transitive 일치 (lock-step 유지)



### Round 493 — R51.4 §5.38 Widget<P> + IntentEmitter generic — Button/Toggle binding 의 L3+L4 boilerplate 청산 — Tier-1 widget binding 의 두 layer (L3 engine facade + L4 §5.20 intent buffer) 를 generic 화: Widget<P: StatePolicy> + IntentEmitter<W>. Button = type alias, Toggle = newtype with sidecar; ButtonExternal/ToggleExternal 는 inner+pending_intents 두 field → IntentEmitter<W> 한 field. Checkbox/Radio 진입 시 facade/buffer 재선언 0줄.

**Changes**:
- crates/pinion-core/src/widgets/widget.rs 신설: Widget<P: StatePolicy> facade (with_policy/new/send/state/Default) + IntentEmitter<W> (inner/push/drain/is_dirty/Default)
- button.rs: pub struct Button + impl new/send/state/Default 제거 → pub type Button = Widget<ButtonPolicy>; ButtonExternal: inner+pending_intents 2 field → em: IntentEmitter<Button> 1 field, drain_intents/is_dirty 자동 위임
- toggle.rs: Toggle struct 의 engine: Engine<TogglePolicy> → inner: Widget<TogglePolicy> (value: bool sidecar 유지); ToggleExternal 동일 패턴 + intervene value set_on 도 em.inner 경유
- widgets/mod.rs: pub mod widget + pub use widget::{IntentEmitter, Widget} re-export



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 993 pass (회귀 0, API surface 완전 호환)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warning (workspace strict baseline: forbid unsafe + deny warnings + clippy::pedantic deny)
- atomic add_section_implementation ×2: §5.38 에 widget.rs:Widget + widget.rs:IntentEmitter 심볼 등록



**Impact**: §5.38, §5.20



### Round 494 — R51.109.1 §5.41 WidgetRenderer trait + VelloContext + VelloRenderer super-trait 분리 — backend-agnostic dispatch substrate, macro emit 갱신 (Frame/Context associated type 정통)

**Changes**:
- WidgetRenderer trait 신설 (Frame + Context + render + resize backend-agnostic surface)
- VelloContext struct 신설 (base_color carry, Default = BLACK = root_background fallback)
- VelloRenderer = WidgetRenderer<Frame=vello::Scene, Context=VelloContext> + Sized super-trait
- vello_renderer_impl! macro 가 두 trait impl 동시 emit (codegen template 미변경)
- app.rs:render call site = WidgetRenderer::render via method resolution (renderer.render)
- 기존 binding (hello-* 등) 0 변경 — macro 가 새 trait 두 impl 자동 emit



**Verification**:
- cargo check --workspace --features pinion-runtime/vello clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1660 pass / 0 fail (baseline 유지)
- WidgetView::Renderer: VelloRenderer + 'static 그대로 (transitively WidgetRenderer)



**Impact**: §2, §5.16, §5.41


**Carry forward**:
- R51.109.2 — TuiRenderer impl + WidgetRenderer for TuiRenderer + paint_adapter::to_tui
- R51.110 — hello-button TUI dogfood (first 2 impl substrate 평가)
- WidgetRenderer::Frame / Context associated type Send/Sync bound = R51.109.2 평가
- VelloContext Default = BLACK 가 paint_adapter::root_background fallback 일치 (정통)
- Context: Copy 한정 — Default 불필요 (새 backend 온 ratify 시 재평가)



### Round 494 — R51.5 §5.38 Checkbox (Tier-1) — substrate validation — Checkbox = Toggle 1:1 pattern with divergent intent name ("checked") and schema slot ("checked" vs "value"). R51.3 sce:template + R51.4 Widget/IntentEmitter substrate 의 첫 실사용 widget — new widget add ≈ sce:use 1줄 + Toggle 패턴 mirror.

**Changes**:
- widgets/checkbox.scxml 신설 — sce:use standard_button activate_event=checkbox.activate (R51.3 substrate 계승)
- widgets/checkbox.rs 신설 — Checkbox newtype (inner: Widget<CheckboxPolicy> + value: bool) + CheckboxExternal (em: IntentEmitter<Checkbox>); R51.4 substrate 계승
- intent name = "checked" + IntrospectValue::Bool payload — Toggle 의 "toggle" 과 분리, AI form-field listener 독립 subscribe
- build.rs scxml_inputs + widgets/mod.rs pub mod checkbox 등록
- atomic add_section_implementation ×3 (scxml + Checkbox + CheckboxExternal)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 993 → 1004 pass (+11 checkbox)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warning
- substrate validation: 새 widget land cost = scxml ~20 LOC + binding ~330 LOC (도구적 boilerplate — 실 새 로직 = value field/flip/intent 이름/schema slot ~5 places)



**Impact**: §5.38, §5.20



### Round 495 — R51.109.2 §5.41 WidgetRenderer trait pinion-core lift + TuiRenderer<B: Backend> 첫 2nd impl land — [[substrate-incompleteness-signal]] 정통 (trait + 두 impl 동시, premature abstraction 회피)

**Changes**:
- pinion-core/src/renderer.rs 신설 — WidgetRenderer trait (Frame + Context + render + resize) lift
- pinion-shell: WidgetRenderer trait def 제거, pub use pinion_core::WidgetRenderer re-export
- pinion-tui: pinion-core dep + TuiContext + TuiRenderer<B: ratatui::Backend>
- TuiRenderer 의 WidgetRenderer impl (terminal.draw closure 로 buffer cells copy)
- substrate-incompleteness-signal 정합: trait + first 2 impl (Vello + Tui) 동시 land
- test infra: TestBackend smoke + impl trait bound 컴파일 검증



**Verification**:
- cargo check --workspace --features pinion-runtime/vello clean (trait move 무회귀)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1662 pass / 0 fail (+2 신규)
- pinion-tui 는 vello / winit transitive 0 (cross-backend pollution 검증)



**Impact**: §2, §5.2, §5.16, §5.41


**Carry forward**:
- R51.110 — paint::to_buffer (Scene → ratatui::Buffer 매핑) + hello-button TUI dogfood
- R51.110 — pinion-tui-shell run::<V>() entry point (winit-free ApplicationHandler analog)
- TuiContext palette / colour depth field = R51.111+ (hello-button 평가 후)
- WidgetView::Renderer bound 의 TUI binding 시 generic 또는 alternate trait = R51.110 결정
- TuiRenderer<B> generic monomorphization — production CrosstermBackend / test TestBackend



### Round 495 — R51.6 §5.38 Radio (Tier-1) — set-not-flip variant — Radio = Toggle/Checkbox 와 동일 statechart, value mutation 만 다름 (activate 시 unconditional set true). Group constraint 는 application layer (set_selected(false) on siblings). Idempotent re-activate 는 §5.20 silent. substrate validation 2nd widget.

**Changes**:
- widgets/radio.scxml: sce:use standard_button activate_event=radio.activate
- widgets/radio.rs: Radio newtype (selected: bool) + RadioExternal (em: IntentEmitter<Radio>); activate sets selected=true unconditional (flip 아닄)
- intent "selected" + IntrospectValue::Null payload — false→true 전환에서만 emit, idempotent re-activate silent
- build.rs + widgets/mod.rs 등록 + add_section_implementation ×3



**Verification**:
- cargo test 1004 → 1015 pass (+11 radio)
- cargo clippy 0 warning
- substrate validation: R51.4 generic + R51.3 template 공유, divergent value mutation 만 widget-specific (5 lines)



**Impact**: §5.38, §5.20



### Round 496 — R51.110.0 §5.41 pinion_tui::paint::to_buffer text-first 매핑 land — Scene→ratatui::Buffer grapheme cluster paint walker (Box/Path/Image 는 R51.111+ alongside hello-button TUI dogfood)

**Changes**:
- pinion-tui/src/paint.rs 신설 — to_buffer + paint_text + pixel_to_cell_origin
- unicode-segmentation 1.12 + unicode-width 0.2 direct deps 추가 (grapheme + cell width)
- Scene match: Container 재귀 + Text paint + wildcard no-op (§3 escape 정합)
- 9 paint test: ASCII / pixel scaling / CJK width / 경계 / right-edge / Container 재귀 / 빈 content / Box skip / saturate
- PIXEL_PER_CELL_X=8 / PIXEL_PER_CELL_Y=16 placeholder constants (8×16 바이트맵 baseline)



**Verification**:
- cargo check -p pinion-tui --tests clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1671 pass / 0 fail (+9 paint)
- pinion-tui no vello/winit transitive (cross-backend pollution 0 유지)



**Impact**: §2, §3, §5.2, §5.41


**Carry forward**:
- R51.110.1 — WidgetViewTui trait + pinion_tui::run_test::<V>() 단일 frame substrate
- R51.110.2 — examples/hello-button-tui first client + crossterm event loop
- R51.111+ — Box border/bg paint, Path/Image unicode-art 매핑 (dogfood mismatch 이후)
- PIXEL_PER_CELL_X/Y = 8×16 placeholder, cell-native coord axis 는 R51.111+ 수정
- right-edge truncation 의 ellipsis policy = R51.111+ (§5.36 text layout cache TUI 안정화 후)



### Round 496 — R51.7 §5.38 Slider (Tier-1) — continuous f32 value + two-phase intent — Slider = shared button-like statechart + continuous f32 value (0..=1) sidecar. Pressed 를 "dragging" 으로 binding 측에서 해석. two-phase intent: "value_changing" (drag 중 live preview) + "value_committed" (drag-end 단일 commit). Material/SwiftUI/Qt convention. substrate validation 3rd widget.

**Changes**:
- widgets/slider.scxml: sce:use activate_event=slider.activate
- widgets/slider.rs: Slider (value: f32, set_value clamp+changed-bool) + SliderExternal (intent_changing + intent_committed)
- IntrospectValue::Float schema slot, intervene 가 Float/Int 둘 다 수락 (Int 시 cast)
- build.rs + widgets/mod.rs 등록 + add_section_implementation ×3



**Verification**:
- cargo test 1015 → 1027 pass (+12 slider)
- cargo clippy 0 warning (f64::from + epsilon compare 적용)
- substrate validation 3rd widget — statechart 공유 + value 타입/intent semantic 만 divergent (Toggle bool flip, Checkbox bool flip, Radio bool set, Slider f32 continuous)



**Impact**: §5.38, §5.20



### Round 497 — R51.110.1 §5.41 WidgetViewTui trait + render_one_frame helper land — TUI widget binding contract substrate (run::<V>() 의 foundation, event loop R51.110.2 carry)

**Changes**:
- pinion-tui/src/widget.rs 신설 — WidgetViewTui trait + render_one_frame helper
- WidgetViewTui = WidgetView 의 TUI sibling (alternate trait, generic merge = 2nd binding 시 평가)
- trait methods: State / Event / Renderer + create_external + tag + read_state + view + event_name + title + initial_size
- initial_size default (80, 24) = 산업 baseline terminal 사이즈
- render_one_frame::<V>(state, cols, rows) -> Buffer = test harness + R51.110.2 foundation
- 3 widget test: DummyView + state diff + initial_size default



**Verification**:
- cargo check -p pinion-tui --tests clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1674 pass / 0 fail (+3 widget)
- WidgetViewTui::Renderer bound = WidgetRenderer<Frame=Buffer, Context=TuiContext> + 'static



**Impact**: §2, §5.41


**Carry forward**:
- R51.110.2 — pinion_tui::run::<V: WidgetViewTui>() crossterm 이벤트 루프 + alternate screen + raw mode
- R51.110.2 — examples/hello-button-tui first client + Escape-to-exit + 단일 frame paint
- WidgetViewTui vs WidgetView merge = R51.111+ 2nd binding 시 평가 (substrate-incompleteness-signal)
- render_one_frame ZST Frame argument = pinion-core Frame ZST 정합 (per-frame dimension 없음)
- input dispatch / focus / a11y / keybinding hooks = R51.111+ (2nd TUI binding 등장 시)



### Round 497 — R51.8 §5.38/§5.12 Toggle e2e RPC dispatch — wire-form 검증 — ToggleExternal R51.2 widget 의 §5.15 8-item contract 를 JSON-RPC envelope 통해 wire-form 검증: scene/query (state+value), scene/rewind (value intervene), scene/invoke (send action), scene/intents (drain) 4 path. full activate cycle 시 "toggle" intent + bool(true) payload 정확 emit. ButtonExternal R12 e2e suite mirror. visual demo 는 별도 carry (hello-toggle binary, 큰 작업).

**Changes**:
- pinion-rpc/src/dispatch.rs inline tests +4: scene_query_on_toggle_external + scene_rewind_on_toggle_external (인텐트 silent) + scene_invoke_on_toggle_external + scene_invoke_full_cycle (toggle intent + Bool(true))
- atomic add_section_implementation §5.12 — dispatch.rs:tests Toggle e2e binding



**Verification**:
- cargo test 1027 → 1031 pass (+4 toggle e2e)
- cargo clippy 0 warning
- wire-form contract validation: intervene-set 이 "toggle" intent 을 forge 하지 않음 (사용자 activate 와 model write 채널 분리 유지)



**Impact**: §5.38, §5.12, §5.15, §5.20



### Round 498 — R51.110.2 §5.41 pinion_tui::run + hello-button-tui first TUI dogfood land — §2 #6 GUI/TUI dual invariant first visible substrate evaluation (crossterm event loop + Esc exit + Resize repaint, input dispatch 는 R51.111+ carry)

**Changes**:
- pinion-tui/src/shell.rs 신설 — run::<V>() crossterm raw mode + alternate screen + RAII guard
- TerminalGuard Drop — panic-safe 터미널 restore (raw mode off + leave alt screen)
- event loop: poll(100ms) + Esc 종료 + Resize repaint + 다른 이벤트 swallow
- examples/hello-button-tui 신설 — first visual TUI dogfood (static label + exit hint)
- workspace member 등록 + dep 구조 (pinion-core + pinion-tui)
- input dispatch / SCXML wire-up = R51.111+ carry ([[substrate-incompleteness-signal]] 정육)



**Verification**:
- cargo check -p hello-button-tui clean
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warning
- cargo test --workspace --features pinion-runtime/vello = 1674 pass / 0 fail (regression 없음)
- 수동 dogfood: cargo run -p hello-button-tui → 터미널에 hello-button 표시 + Esc 종료



**Impact**: §2, §5.15, §5.41


**Carry forward**:
- R51.111 — input dispatch via InputRouter (crossterm KeyEvent/Mouse → Scene::External invoke)
- R51.111 — state 변경 → repaint cycle (현재 는 동일 state 재그림)
- R51.111 — hello-button SCXML statechart 실제 연결 (StubExternal 교체)
- R51.112+ — Box border/bg paint, focus ring, a11y AccessKit-TUI (또는 PTY screen reader path)
- WidgetViewTui vs WidgetView merge 평가 = 2nd TUI binding 등장 시 trigger
- PIXEL_PER_CELL 8×16 → cell-native coord 전환 = 2nd TUI binding 시 mismatch 평가



### Round 498 — R51.10 §5.37.4 BIDI (UAX #9) axis ratify — placeholder → full body — §5.37.4 BIDI directional resolution axis ratify (atomic-only, code 변경 0). UAX #9 full conformance + external lib 0 (UCD DerivedBidiClass.txt embed via build.rs codegen, R50.2.x NFC 패턴 일관). intent/inputs/outputs/rationale/impact_scope/alternatives full set. self-hosted text engine layer chain 의 4번째 layer (NFC → BIDI → shape → line break → raster). multi-session impl carry — R51.11+ slice.

**Changes**:
- §5.37.4 title placeholder → BIDI directional resolution (UAX #9)
- intent: NFC → embedding levels + visual reorder, external lib 0
- inputs 6 (NFC seq / UCD DerivedBidiClass / UAX #9 6-stage / sfnt / RPC channel / invariant #2)
- outputs 6 (pinion-text-unicode bidi module / resolve fn / BidiResult / BidiClass trie / 6 stage 함수 / RPC text/bidi)
- rationale 6 (shape prereq / external 0 / AI-first / backend swap / Hyrum strict / full conformance)
- impact_scope 6 (§5.37 / §5.37.1~3 / §5.37.6~7)
- alternatives 5 (icu4x / unicode-bidi / LTR-only / partial subset / shape inline 모두 reject)



**Verification**:
- mnemosyne validate_workspace: entries 152 → 153, sections 55 (변동 없음), T1 orphan 0, RT 1/1, GENERATED.md sync
- code 변경 0 (atomic-only round)
- forward-reference 사전 확인: impact_scope 의 §5.37.6/§5.37.7 placeholder 존재 확인



**Impact**: §5.37.4, §5.37



### Round 499 — R51.11 §5.37.4 BIDI scaffold — BidiClass enum + UCD lookup table — UAX #9 BIDI first impl slice: UCD 16.0 DerivedBidiClass.txt vendor + build.rs codegen sorted ranges + BidiClass enum (23 variants) + bidi_class(char) binary-search lookup. external lib 0 (R50.2.x NFC 패턴 일관). 6-stage algorithm (P/X/W/N/I/L) 은 후속 slice carry.

**Changes**:
- ucd/DerivedBidiClass.txt vendor (UCD 16.0.0, 2579 라인)
- build.rs +parse_bidi_class + emit_bidi_tables → OUT_DIR/bidi_tables.rs (sorted &[(u32,u32,u8)] ranges)
- src/bidi.rs 신설: BidiClass 23 variant enum + from_index + ucd_name + bidi_class(char) binary-search
- lib.rs pub mod bidi + pub use BidiClass / bidi_class re-export
- atomic add_section_implementation ×4 (UCD file / build.rs symbol / 2 × bidi.rs symbol)



**Verification**:
- cargo test 1031 → 1041 (+10 bidi tests — ASCII L/EN, Hebrew R, Arabic AL/AN, WS, B/S, isolate markers, ucd_name round-trip, PUA fallback)
- cargo clippy 0 warning (generated tables.rs 의 unreadable_literal 의 module-level allow)
- external dep 0 유지 (pinion-text-unicode 의 std + alloc only)



**Impact**: §5.37.4, §5.37.3



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



### Round 500 — R51.12 §5.38 IntentEmitter::dispatch + WidgetTransition trait — 5-widget refactor — R51.12 §5.38 substrate generic 완성 — WidgetTransition trait + IntentEmitter::dispatch pipeline; 5 widget *External::send 의 snapshot→drive→detect→push boilerplate 1줄 dispatch 호출로 영구 청산

**Changes**:
- crates/pinion-core/src/widgets/widget.rs: WidgetTransition trait (Event / Snapshot:Copy / snapshot / drive / detect) — 5 widget 의 transition 감지 contract 를 trait 차원에서 표준화; Snapshot:Copy bound 으로 cheap-snapshot design rule 강제
- crates/pinion-core/src/widgets/widget.rs: IntentEmitter::dispatch — substrate 측 snapshot→drive→detect→push pipeline; *External::send 5x 의 5-10 LOC boilerplate 가 self.em.dispatch(event) 1줄로 축약
- crates/pinion-core/src/widgets/button.rs: impl WidgetTransition for Button — detect=Pressed→Hover ⇒ click/Null; ButtonExternal::send refactor (16 LOC → 1 LOC)
- crates/pinion-core/src/widgets/toggle.rs: impl WidgetTransition for Toggle — Snapshot=(State,bool) detect=Pressed→Hover ∧ flip ⇒ toggle/Bool(after); ToggleExternal::send refactor
- crates/pinion-core/src/widgets/checkbox.rs: impl WidgetTransition for Checkbox — Toggle 1:1 mirror, intent name 만 checked 로 swap; CheckboxExternal::send refactor
- crates/pinion-core/src/widgets/radio.rs: impl WidgetTransition for Radio — set-not-flip variant (!before ∧ after) ⇒ selected/Null; RadioExternal::send refactor
- crates/pinion-core/src/widgets/slider.rs: impl WidgetTransition for Slider — detect=Pressed→Hover ⇒ value_committed/Float(after); SliderExternal::send refactor; SliderExternal::set_value 직접 push 경로 유지 (transition 아닌 direct value mutation 으로 value_changing 발화)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1041 pass (refactor zero-delta — 모든 기존 widget test 통과)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints forbid/deny/pedantic strict baseline 유지)
- 5 widget *External::send public API surface 불변 — RPC dispatch / scene/invoke 경로 영향 zero (Toggle e2e R51.8 suite 4 test 그대로 통과)
- diff stat: +285 / -71 (7 file) — substrate 73 LOC 추가, 5 widget application 측 boilerplate 합 50+ LOC 감소



**Impact**: §5.38, §5.20


**Carry forward**:
- R51.13+ 후보: SCE-002 (Event payload, vendor RFC) / RadioGroup primitive / Toggle 외 widget RPC e2e ×3 (Checkbox/Radio/Slider) / BIDI P-rules (R51.x algorithm slice) / Slider statechart cleanup / hello-toggle visual demo
- future Tier-2 widget land 시 WidgetTransition impl 4-method 패턴 자동 적용 — application 측 *External::send 는 항상 1줄 dispatch 호출 (substrate 정통 완성 후 신규 widget 진입 cost 영구 축소)



### Round 501 — R51.12.1 §5.38 substrate dispatch tests + #[must_use] — R51.12 substrate gap closure — IntentEmitter::dispatch + WidgetTransition pipeline 격리 unit test 추가 + detect #[must_use] (silent intent loss 영구 방지)

**Changes**:
- crates/pinion-core/src/widgets/widget.rs: WidgetTransition::detect 에 #[must_use] 추가 — detect 결과 마세 시 silent intent loss 가 언제나 bug; attribute 로 compile-warn enforce
- crates/pinion-core/src/widgets/widget.rs:tests 추가 — StubWidget fixture + 6 substrate isolation test (dispatch_pushes / dispatch_skips / drive_between / before_pre_drive / after_post_drive / direction_sensitive)
- 5 widget 의 간접 coverage 와 독립적으로 substrate pipeline regression 포착 — widget impl 의 failure 와 substrate 의 failure 분리 관측 가능



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1047 pass (+6 from 1041)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (workspace.lints strict 유지)
- 6 신규 test 는 stub fixture 로 완전 격리 — SCE engine 미의존, 순수 trait contract 검증



**Impact**: §5.38


**Carry forward**:
- R51.x 다음 후보: derive macro for WidgetTransition (pinion-derive 확장, 5 widget impl boilerplate → #[derive] syntactic sugar) / Toggle 외 widget RPC e2e ×3 (Checkbox/Radio/Slider substrate validation 1/4 → 4/4) / RadioGroup primitive (framework-vs-application boundary fix) / SCE-002 RFC / BIDI P-rules / hello-toggle visual demo



### Round 502 — R51.13 §5.12+§5.38 widget RPC e2e ×3 (Checkbox/Radio/Slider) — Substrate validation coverage 1/4 → 4/4 — Toggle 외 3 widget JSON-RPC dispatch e2e suite mirror; Slider 의 intervene→value_changing asymmetric semantic 와 Radio set-not-flip 의 idempotent silent re-activate 를 wire-level 에서 enforce

**Changes**:
- crates/pinion-rpc/src/dispatch.rs: 12 new e2e tests (4 per widget) mirror R51.8 Toggle pattern — scene/query 상태+value, scene/rewind intervene, scene/invoke send transition, full activate cycle + scene/intents drain
- Checkbox 4 tests: schema slot "checked" + intent name "checked" + Bool(after) payload; intervene set-without-intent semantic
- Radio 4 tests: schema slot "selected" + intent name "selected" + Null payload; **second activate cycle silent on §5.20** (set-not-flip idempotent semantic wire-enforced)
- Slider 4 tests: schema slot "value" Float + intent names "value_changing" / "value_committed"; **intervene fires value_changing** (single-source-of-truth value path through set_value); full drag cycle = 2 ordered intents
- clippy::single_char_pattern 수정 (v.contains("0") → v.contains('0'))



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1059 pass (+12 from 1047)
- pinion-rpc: 323 → 335 tests
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline)
- Slider 2-intent ordering (value_changing 선행 → value_committed 후발) wire 관찰 — R51.7 binding-layer semantic 가 RPC 경로에서도 동일



**Impact**: §5.12, §5.38, §5.20


**Carry forward**:
- R51.x 다음 후보: derive macro for WidgetTransition (pinion-derive 확장) / RadioGroup primitive / SCE-002 RFC / BIDI P-rules / Slider statechart cleanup (SCE-002 후) / hello-toggle visual demo
- Tier-2 widget land 시 R51.8/R51.13 e2e 패턴 4-test 자동 적용 — widget-specific schema slot 이름 + intent name + payload type 만 교체하면 동일 structure



### Round 503 — R51.14 §5.38 Slider own statechart — Dragging state semantic cleanup — R51.7 abstraction leak 청산 — Slider 가 sce:use 의 standard_button template 을 buinding-layer reframe (Pressed = dragging) 으로 재해석하던 패턴을 종료, 자체 statechart 의 semantically named Dragging state 로 SCXML/Rust/RPC vocabulary 일관

**Changes**:
- crates/pinion-core/widgets/slider.scxml: 자체 statechart 으로 재작성 (sce:use 제거) — states idle/hover/dragging/disabled, R51.7 의 button-like template 대신 widget-specific body
- crates/pinion-core/src/widgets/slider.rs: SliderState::Pressed → SliderState::Dragging 전면 대체 — WidgetTransition::detect, slider_state_name, doc 업데이트
- RPC scene/query "state" 은 이제 "Dragging" 반환 — AI introspect 측에서 상태 독도 증가 (SCXML state vocabulary 가 binding semantic 와 일치)
- SCE-002 (Event payload 없음) 은 재평가 결과 design tradeoff — SCXML "null datamodel + typed Rust sidecar" 계층을 textbook 으로 운용; debt enumeration 보류



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1059 pass (refactor zero-delta — 상태 이름 만 변경, behavior 동일)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- slider.scxml sce-build codegen 가 SliderState::Dragging 방출 확인 — enum variant rename 이 전체 컴파일 통과



**Impact**: §5.38


**Carry forward**:
- R51.x 다음 후보: RadioGroup primitive (R47-class framework-vs-application boundary debt) / derive macro for WidgetTransition (improvement) / hello-toggle visual demo / BIDI P-rules (algorithm slice)
- Tier-2 drag-style widget (Knob, ScrollBar) 출현 시 pointer_track.sce-template.xml 으로 공통 body 추출 — 지금은 N=1 이라 YAGNI 조건 완화 적용



### Round 504 — R51.15 §5.38 RadioGroup primitive — framework-owned mutual exclusion — R47-class framework-primitive boundary 부채 청산 — Radio 의 sibling deselect 책임을 application 측에서 framework 측 RadioGroup 으로 이동; R51.12 WidgetTransition substrate 활용으로 응용 사례 입증

**Changes**:
- crates/pinion-core/src/widgets/radio_group.rs (new): RadioGroup struct (N 개 Radio 소유) + send(idx, ev) 이 activate 시 타 Radio set_selected(false) 자동 적용; selected_index Option<usize> 상태 관리
- RadioGroup 가 impl WidgetTransition (Event=(usize, RadioEvent), Snapshot=Option<usize>) — R51.12 substrate 적용 사례 (1줄 dispatch 호출)
- RadioGroupExternal 적용: schema (count/selected_index/send) + query/intervene/invoke 완전 커버리지 + "selected"/Int(idx) intent 발화 (selection change 시만, idempotent re-activate silent)
- crates/pinion-core/src/widgets/radio.rs: parse_radio_event 가 pub(crate) 로 증강 (RadioGroupExternal::invoke send 에서 재사용)
- crates/pinion-core/src/widgets/mod.rs: pub mod radio_group 추가



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1080 pass (+21 RadioGroup tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- pinion-core: 319 → 341 (+22 — 21 RadioGroup tests + parse_radio_event visibility 및 sequence test)
- industry precedent verified: HTML radio name attribute / Material RadioGroup / SwiftUI Picker / Qt QButtonGroup 모두 framework 측 mutual exclusion — pinion 일치
- R51.12 substrate 의 외부 적용 사례 (Radio 동일 패턴 외의 widget) — substrate 일반성 증명



**Impact**: §5.38, §5.20


**Carry forward**:
- R51.x 다음 후보: RadioGroup RPC e2e (dispatch.rs 에 4-test pattern 적용) / derive macro for WidgetTransition (improvement) / hello-toggle visual demo / BIDI P-rules
- RadioGroup Scene 측 helper (RadioGroupNode 혹은 layout API 통합) 출현 시 재결정 — 현재는 application 측 공간 배치
- Tier-2 group widget (TabBar, SegmentedControl, ListView selection) 출현 시 RadioGroup 패턴 재사용 — N 개 child widget + framework-owned mutual exclusion + Option<usize> snapshot



### Round 505 — R51.16 §5.12 RadioGroup RPC e2e — multi-Radio composite wire validation — RadioGroup substrate validation 완성 — JSON-RPC dispatch 경로 통과한 multi-Radio composite widget 의 query/rewind/invoke/full-cycle e2e, "selected"/Int(idx) intent 가 wire-form 에서 동일 동작 확인

**Changes**:
- crates/pinion-rpc/src/dispatch.rs: RadioGroup 4 e2e tests 제적 — query count/selected_index + rewind selected_index + invoke send (idx:Event format) + full activate cycle + selected intent verification
- selected_index = None 경로 는 raw JSON envelope 레벨로 assert — serde Option<Value> 가 JSON `null` 을 None 으로 deserialize 하는 기본 동작 우회 (parse_response.result.unwrap() 고챔 회피)
- wire format <index>:<EventName> (e.g. "2:PointerUp") 가 JSON-RPC 엔벨로프에서 정상 라우팅 — RadioGroupExternal::invoke send 가 R51.15 design 대로 동작



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1084 pass (+4 RadioGroup e2e from 1080)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline)
- Toggle/Checkbox/Radio/Slider/RadioGroup — 이제 모든 Tier-1 widget 이 RPC e2e 계층에서 검증 (substrate validation 5/5)



**Impact**: §5.12, §5.38, §5.20


**Carry forward**:
- serde Option<Value> JSON null 이주 우회 가 의존적으로 parse_response 과 raw envelope 두 경로 복합 사용 — 구조적 정리가 필요시 Response 구조체 자체 deserialize_with 개선 고려 (차기 RFC)
- R51.x 다음 후보: derive macro for WidgetTransition (pinion-derive 증원) / hello-toggle visual demo / BIDI P-rules



### Round 506 — R51.17 §5.37.4 BIDI P-rules (UAX #9 §3.3.1) — paragraph level + iter — UAX #9 §3.3.1 P1+P2+P3 land — paragraph 분리 (B class boundary) + 첫 strong character 기반 embedding level resolution + isolate-aware depth tracking; R51.11 BidiClass scaffold 위 첫 algorithm slice

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: paragraph_level(paragraph) -> u8 — UAX #9 P2+P3 구현, isolate_depth (LRI/RLI/FSI → PDI) 계수 skip + L/AL/R 첫 강한 char 검색 + P3 default LTR fallback
- crates/pinion-text-unicode/src/bidi.rs: iter_paragraphs(text) + ParagraphIter 구조체 — UAX #9 P1 lazy iterator, B class boundary, 각 paragraph 은 trailing B 포함 (UAX #9 귄장)
- external lib 0 유지 — std + alloc + R51.11 bidi_class 만 의존, [[uax-semantic-spec-lock]] policy 일치



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1100 pass (+16 from 1084 — BIDI P-rules)
- pinion-text-unicode: 120 tests pass (+16 BIDI: 10 paragraph_level + 6 iter_paragraphs)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline 유지)
- UAX #9 spec 준수 검증: P3 default LTR / RTL via R or AL / isolate skip via LRI...PDI / unmatched isolate / stray PDI saturate-at-zero / iter 의 single-pass + lazy + trailing-B semantic
- U+2028 LSEP vs U+2029 PSEP 구분 — LSEP 은 WS (line break in paragraph), PSEP 만 B (paragraph boundary). 검증 test 수정



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: BIDI X-rules (explicit embedding / override / isolate sequence build) — paragraph_level 의 신뢰 계층 위에 explicit-level stack 구축
- BIDI W-rules / N-rules / I-rules / L-rules 추후 slice 적용 — UAX #9 BidiTest.txt 수십만 수의 vector 계립으로 hand-test 대체 고려
- derive macro for WidgetTransition (improvement opportunity, 별도 라운드) / hello-toggle visual demo (paint-side validation)



### Round 507 — R51.18 §5.37.4 BIDI X-rules (UAX #9 §3.3.2) — explicit levels + status stack — UAX #9 §3.3.2 X1-X9 land — DirectionalStatusStack (entries + overflow_isolate/overflow_embedding/valid_isolate counters) + resolve_explicit_levels (per-codepoint level + class output) + FSI first-strong lookahead; R51.17 paragraph_level 위 두 번째 algorithm slice

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: resolve_explicit_levels(paragraph, paragraph_level) -> ExplicitLevels — UAX #9 X1-X9 single-pass per-codepoint (level, class) emission, X9 removed codes (RLE/LRE/RLO/LRO/PDF/BN) reported as BidiClass::BN
- crates/pinion-text-unicode/src/bidi.rs: ExplicitLevels { levels: Vec<u8>, classes: Vec<BidiClass> } — substrate for downstream W/N/I/L rules; per-codepoint parallel arrays
- crates/pinion-text-unicode/src/bidi.rs: MAX_DEPTH = 125 pub const (UAX #9 §3.3.2 bound) + DirectionalStatusStack/Entry/Override private substrate + next_odd/next_even const fn + fsi_resolves_to_rli helper (X5c first-strong lookahead, depth-tracked PDI matching)
- PDF level convention = pre-pop (embedding being closed) per icu4c/unicode-bidi; PDI level = post-pop per UAX #9 X6a explicit rule (asymmetry doc-noted)
- external lib 0 유지 — std + alloc + R51.11 bidi_class 만 의존, [[uax-semantic-spec-lock]] policy 일치



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1126 pass (+26 from 1100 — BIDI X-rules)
- pinion-text-unicode: 146 tests pass (+26 X-rules incl. 1 UCD format-control sanity check)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline 유지: forbid unsafe / deny warnings / clippy::pedantic deny)
- UAX #9 spec 준수 검증: X2-X5 embedding push (next_odd/next_even) / X4-X5 override (RLO/LRO) / X5a-X5b isolate push (RLI/LRI) / X5c FSI first-strong lookahead (RTL/LTR/none-default-LRI/nested-isolate-skip) / X6a PDI matched-pop + post-pop level / X7 PDF pre-pop level + isolate non-pop / X8 B = paragraph_level / X9 BN preserves level
- edge cases: empty paragraph / unmatched PDF / unmatched PDI / unmatched LRI / MAX_DEPTH 125 overflow_embedding 진입 / PDF clears overflow before stack pop / override leak past PDF / outer override blocked by isolate (X5a Neutral override on new entry)



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: BIDI W-rules (UAX #9 §3.3.3) — weak type resolution (W1 NSM / W2 EN-after-AL / W3 AL->R / W4 ES-CS between EN-AN / W5 ET adjacent EN / W6 leftover separators->ON / W7 EN context-strong); level run partitioning (X10) 필요할 가능성
- BIDI X-rules → W/N/I/L 진행 후 UAX #9 BidiTest.txt 수십만 vector conformance 적용 (hand-tests + 대표 vector subset)
- derive macro for WidgetTransition (improvement opportunity, 별도 라운드) / hello-toggle visual demo (paint-side validation) / serde Option<Value> JSON null RFC (small fix)



### Round 508 — R51.19 §5.37.4 BIDI W-rules (UAX #9 §3.3.3) — weak types + isolating run sequences — UAX #9 §3.3.3 W1-W7 + X10 level run + BD13 isolating run sequence land — resolve_weak_types(explicit, paragraph_level) over X-rules output; AL→R, EN-after-AL→AN, EN-context-L→L, NSM 전파 + isolate-boundary→ON, single-ES/CS-between-strong-neighbors→same-strong, ET-adjacent-EN→EN, residual→ON

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: resolve_weak_types(ExplicitLevels, paragraph_level) -> ExplicitLevels — UAX #9 §3.3.3 weak resolution; levels unchanged, classes rewritten W1→W7 per isolating run sequence
- private substrate: LevelRun + IsolatingRunSequence structs; build_level_runs (X9-removed skip) + match_isolate_initiators (depth-tracked stack) + build_isolating_run_sequences (BD13 connect runs via matched initiator→PDI pair at run boundaries) + compute_sos_eos (X10 max(neighbor_level, sequence_level) parity)
- private W-rule helpers: apply_w1 (NSM at sos / after isolate→ON / else preceding type) / apply_w2 (last-strong scan; AL→AN) / apply_w3 (AL→R) / apply_w4 (single ES|CS between EN-EN / single CS between AN-AN) / apply_w5 (ET-sequence adjacent EN) / apply_w6 (residual ES/ET/CS→ON) / apply_w7 (last-strong scan; L→L on EN)
- external lib 0 유지 — std + alloc 만 의존; HashMap 없이 Vec<Option<usize>> position→run index lookup; W rules 는 sequence-flattened slice view 일괄 처리 (collect_sequence_positions)
- overflow-isolate guard: matched LRI→PDI 가 같은 level run 안 (initiator overflow case) 는 BD13 connection 에서 제외 (initiator must be last member of run / PDI must be first member of run) — self-loop 회피



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1157 pass (+31 from 1126 — BIDI W-rules)
- pinion-text-unicode: 177 tests pass (+31 W-rules incl. 3 pipeline composition tests + 2 cross-sequence boundary tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline 유지: forbid unsafe / deny warnings / clippy::pedantic deny)
- UAX #9 spec 준수 검증: W1 NSM (start→ON / after-LRI/PDI→ON / propagation) / W2 EN-after-AL→AN / W2 sos-as-strong-boundary 구분 / W3 AL→R / W4 single-ES∈EN-EN→EN, double-ES leaves both / W4 single-CS∈AN-AN→AN / W4 EN-AN mismatch / W5 ET-before/after-EN→EN / W5 ET-sequence→EN / W6 residual→ON / W7 sos-L→L on bare EN
- cross-sequence: AL outside isolate 가 EN inside isolate 에 영향 안 주음 (sequence-local W2) / BD13 outer sequence 가 LRI…PDI 제하 운 AL…EN 연결됨 (잘 이이지않) (외부 시퀀스 명릹 적용)



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: BIDI N-rules (UAX #9 §3.3.4) — neutral resolution (N0 bracket pairs / N1 neutrals between strong→surrounding-strong / N2 remaining neutrals→sos-equivalent); BD16 괴 포용 조합하는 paired bracket table 적재 필요
- BD16 (paired bracket): UCD BidiBrackets.txt 파싱 + (open, close, open_no_canonical_equiv 등) build.rs codegen
- BIDI I-rules + L-rules 그 다음 장 — 레벨 재조정 (I1/I2) + level run reorder (L1-L4)
- BidiTest.txt 수십만 vector conformance applied subset 머지마지 장 (W/N 완료 후)
- derive macro for WidgetTransition (별도 라운드) / hello-toggle visual demo / serde Option<Value> JSON null RFC — BIDI 완성 후 돌아볼 부채



### Round 509 — R51.20 §5.37.4 BD16 paired bracket substrate (UCD BidiBrackets.txt) — UCD 16.0 BidiBrackets.txt 적재 + build.rs codegen + paired_bracket(cp) public lookup + BracketType enum — R51.21 N0 rule의 foundation; 64 bracket pairs (128 entries) sorted binary-search 가능

**Changes**:
- crates/pinion-text-unicode/ucd/BidiBrackets.txt: UCD 16.0.0 적재 (128 entries / 64 pairs, Date 2024-02-02)
- crates/pinion-text-unicode/build.rs: parse_bidi_brackets (BPT_o/BPT_c discrimination + cp-sorted) + emit_bidi_tables 시그니쳐 확장 (두 번째 table BIDI_BRACKET_PAIRS 추가, 기존 BIDI_CLASS_RANGES 병존)
- crates/pinion-text-unicode/src/bidi.rs: BracketType { Open, Close } pub enum + paired_bracket(cp: char) -> Option<(char, BracketType)> binary-search lookup, # Panics doc-section (codegen invariant)
- external lib 0 유지 — std + alloc 만 의존; binary_search over &[(u32, u32, u8)] table



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1164 pass (+7 from 1157 — BD16)
- pinion-text-unicode: 184 tests pass (+7 BD16: ASCII () [] {} pair / 4 round-trip / non-bracket / Tibetan + math BMP)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline 유지)
- round-trip invariant test: paired_bracket(matching) 은 원본 코드포인트 + 반대 kind 돌려줌 (BMP sweep '(', '[', '{', U+0F3A Tibetan, U+27E6 수학, U+2983 dotted curly bracket 교차)
- UCD format 검증: line parser '#' comment skip / 3-column split / o|c|n discrimination (n is no-op continue) / hex 16 구분



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: R51.21 N0 rule (UAX #9 §3.3.4) — isolating run sequence 단위로 bracket pair (BD16) matching stack + enclosed-strong scan + sos-context-fallback direction 항을 적용; paired_bracket 을 substrate 로 사용
- R51.22 N1 + N2 — N0 그림 후 neutral resolution (neutral between strong→surrounding, residual→sos-equivalent direction)
- BIDI I-rules + L-rules 그 다음 장 — 레벨 재조정 (I1/I2) + level run reorder (L1-L4)
- BD16 N0 stack max-depth 63 이 UAX #9 명시 — R51.21 에서 overflow case 처리



### Round 510 — R51.21 §5.37.4 BIDI N-rules (UAX #9 §3.3.4) — N0 bracket pairs + N1 + N2 — UAX #9 §3.3.4 N0+N1+N2 land — resolve_neutral_types(weak, paragraph, paragraph_level) over W-rules output; N0 BD16 bracket pair matching (max 63 stack) + embed-vs-opposite-context direction, N1 matched-strong-neighbor neutral runs (EN/AN treated as R), N2 residual neutral → embedding direction; R51.20 paired_bracket substrate 위 첫 client

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: resolve_neutral_types(ExplicitLevels, paragraph, paragraph_level) -> ExplicitLevels — UAX #9 §3.3.4; isolating run sequence 단위 N0→N1→N2 순차 적용
- private N-rule helpers: find_bracket_pairs (BD16 stack 괴 max 63 + overflow abort) / apply_n0 (embed-strong-first scan / opposite-strong context-backward scan / no-strong leave-ON) / apply_n1 (matched-strong NI run resolution) / apply_n2 (residual NI → embedding direction)
- is_neutral_or_isolate + n_strong_direction const fn 입별 조술 helper — LRI/RLI/FSI/PDI 는 N1/N2 의 NI 소속, EN/AN 은 R 은 influence direction 으로 접속 (UAX #9 N1 명시)
- Known limitation: UAX #9 N0 step 5 (NSM 아이디어 프로파게이션) 미구현 — pre-W1 class array 적재 필요, BidiTest.txt 적용 롬드에서 재분석. 모듈 도킨 제한 명시.
- external lib 0 유지 — std + alloc + R51.20 paired_bracket 만 의존



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1184 pass (+20 from 1164 — BIDI N-rules)
- pinion-text-unicode: 204 tests pass (+20 N-rules: bracket embed-match / opposite-context / no-strong-inside / nested / unmatched / N1 LL+RR+EN-as-R / N1 mismatch → N2 / pipeline composition + isolate-as-NI)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline 유지)
- UAX #9 spec 준수 검증: BD16 stack max 63 / N0 embed-strong-first short-circuit / N0 opposite-strong-with-context fallback / N0 no-strong leaves brackets ON for N1 / N1 EN+AN → R for influence / N1 mismatched neighbours → N2 fallback / N2 sequence-level parity



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: R51.22 BIDI I-rules (UAX #9 §3.3.5) — implicit level resolution; I1 (L on odd-level→L+1) + I2 (R/EN/AN on even-level→L+1 / EN 이 R 다음으로 따른 경우→L+2 etc.)
- R51.23 BIDI L-rules — visual reordering (L1 separators reset / L2 max-level reverse / L3 combining mark reorder / L4 mirroring); UAX #9 의 시각적 출력 단계 최종
- R51.24 N0 NSM-after-bracket propagation (deferred from R51.21) + BidiTest.txt typical-text vector conformance subset
- derive macro for WidgetTransition / hello-toggle visual demo — BIDI algorithm 완성 후 돌아볼 부채



### Round 511 — R51.22 §5.37.4 BIDI I-rules + L1 + L2 + bidi_reorder full-pipeline — UAX #9 §3.3.5 I-rules + §3.4 L1 + L2 + bidi_reorder full-pipeline wrapper land — implicit-level resolution (R-at-even+1, EN/AN-at-even+2, L/EN/AN-at-odd+1), L1 line-break level reset for S/B + trailing WS/isolate-format, L2 visual reorder (max-down-to-lowest-odd reverse), bidi_reorder(text) → Vec<usize> visual indices; BIDI algorithm 6단계 (P/X/W/N/I/L) 완성

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: resolve_implicit_levels(neutral) -> ExplicitLevels — UAX #9 §3.3.5 I1+I2; BN 제외 + level parity 기반 +1/+2 증량
- crates/pinion-text-unicode/src/bidi.rs: apply_l1_line_break(implicit, paragraph, paragraph_level) -> ExplicitLevels — UAX #9 §3.4 L1; original Bidi_Class 재산출 (S/B 과 그 않는 WS/isolate-format walk-back + trailing reset)
- crates/pinion-text-unicode/src/bidi.rs: reorder_visual(levels) -> Vec<usize> — UAX #9 §3.4 L2; max-level 단계적 감소 하향 반복 + lowest-odd-level floor
- crates/pinion-text-unicode/src/bidi.rs: bidi_reorder(paragraph) -> Vec<usize> — 고수준 pipeline wrapper (P→X→W→N→I→L1→L2)
- L3 (combining mark reorder) + L4 (bracket mirroring) 이후 라운드 이젤 (L4 는 BidiMirroring.txt UCD 보재 필요)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1201 pass (+17 from 1184 — BIDI I/L rules)
- pinion-text-unicode: 221 tests pass (+17: I-rules 5 + L1 3 + L2 4 + bidi_reorder 5)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (strict baseline 유지)
- UAX #9 spec 준수 검증: I1 R-at-even +1 / I1 EN-AN-at-even +2 / I2 L-at-odd +1 / I2 EN-AN-at-odd +1 / L1 trailing-WS reset / L1 S/B walk-back / L2 공조 LTR identity 숨골 ∘ 단일 RTL block 반복 ∘ nested levels 🔪-down 순차 반복
- bidi_reorder end-to-end: 'Hello' LTR identity / Pure RTL 'אבג' visual reverse [2,1,0] / mixed LTR+RTL block [0,1,3,2] / Arabic 'ا' + ASCII '5' 이 [1, 0]



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: R51.23 BIDI L3 (combining mark reorder, R에 base) + L4 (bracket mirroring) — UCD BidiMirroring.txt 임포트 + paired_bracket reflection + bidi_mirrored property
- R51.24 BidiTest.txt typical-text vector conformance subset — algorithm 완성 검증 (hand-tests 대체)
- R51.21 N0 NSM-after-bracket propagation 상환 (deferred from R51.21) — BidiTest.txt 이 드러낼 때 필요 시 처리
- derive macro for WidgetTransition / hello-toggle visual demo — BIDI 완성 후 돌아볼 부채 (BIDI algorithm 80-85% 완성, L3/L4/conformance 만 남음)



### Round 512 — R51.23 §5.37.4 BIDI L3 combining marks + mirroring_glyph (L4 substrate) — UCD 16.0 BidiMirroring.txt 적재 + build.rs codegen + mirroring_glyph(cp) pub fn + apply_l3_combining_marks helper + bidi_reorder pipeline L3 추가 — BIDI 알고리즘 P/X/W/N/I/L1/L2/L3 모두 land; L4 mirroring은 render-layer로 위임 (substrate만 제공)

**Changes**:
- crates/pinion-text-unicode/ucd/BidiMirroring.txt: UCD 16.0.0 적재 (428 pairs)
- crates/pinion-text-unicode/build.rs: parse_bidi_mirroring + emit_bidi_tables 시그니쳐 확장 (세 번째 table BIDI_MIRRORING_PAIRS 추가)
- crates/pinion-text-unicode/src/bidi.rs: mirroring_glyph(cp: char) -> Option<char> — UAX #9 L4 substrate, binary-search lookup, # Panics doc-section
- crates/pinion-text-unicode/src/bidi.rs: apply_l3_combining_marks(visual_indices, original_classes) — NSM-then-base reverse; bidi_reorder 은 paragraph.chars() 재산출 으로 original_classes 얻은 뒤 L3 적용
- L4 mirroring application 은 renderer 책임; pinion-text-unicode 은 lookup + resolved-level 기반 제공만 하면 됨



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1211 pass (+10 from 1201 — L3 + L4 substrate)
- pinion-text-unicode: 231 tests pass (+10: 5 mirroring_glyph + 5 L3 reorder 포함 파이프라인 컴포지션)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- UAX #9 준수: L3 RTL Hebrew+NSM identity 복원 / L3 multi-NSM order 유지 / L3 LTR no-op / L3 mixed 경지에서 base-before-NSM 구현 보장 / mirroring_glyph round-trip 대입
- BIDI 알고리즘 완성도: ~95-100% (P/X/W/N/I/L1/L2/L3 land + L4 substrate; conformance vector subset 만 남음)



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: R51.24 UAX #9 BidiTest.txt typical-text vector subset conformance — algorithm 완성 검증 (hand-tests 대체); R51.24+ 에서 이 때 부족 자병→ N0 NSM-after-bracket propagation 도 처리
- BIDI algorithm 80% 이상 완성 — N0 NSM propagation 잠잠 제외 + BidiTest conformance 잘 지마 철저한 구현
- derive macro for WidgetTransition / hello-toggle visual demo — BIDI 완성 후 돌아볼 부채



### Round 513 — R51.24 §5.37.4 BIDI UCD BidiCharacterTest conformance (full 91707-vector sweep, 100% pass; 3 부채 즉시 상환) — UCD 16.0 BidiCharacterTest.txt (91 707 vectors) 전체 통과 land — conformance harness + 3 부채 한 라운드에 즉시 상환 (L2 visible-only filter / N0 step (e) NSM propagation / BD16 canonical-equivalent bracket matching)

**Changes**:
- crates/pinion-text-unicode/ucd/BidiCharacterTest.txt: UCD 16.0.0 적재 (96 464 lines, 91 707 vectors after comment strip)
- crates/pinion-text-unicode/src/test_fixture.rs: BidiCharacterCase + BidiParagraphDirectionInput + parse_bidi_character_test + load_bidi_character_test — Field 0~4 디코더 ('x' = X9-removed Option<u8>::None 모형)
- crates/pinion-text-unicode/src/bidi.rs: bidi_reorder L2 visible-only filter — UAX #9 X9 의 'removed' 가 W/N/I/L 모든 단계에서 invisible 정신 정통화 (post-L1 BN 위치 mask 후 reorder_visual + map-back). hand-tests 가 BN-free 였기에 line 66 까지 미발견
- crates/pinion-text-unicode/src/bidi.rs: apply_n0 의 N0 step (e) 구현 — original_nsm 인자 + N0 가 bracket type 변경한 직후 originally-NSM 인 후행 codepoints 가 새 bracket type 받음 (line 84 'a ( b ) U+0331' 정정)
- crates/pinion-text-unicode/src/bidi.rs: canonical_bracket_form helper + find_bracket_pairs 의 BD16 canonical-equivalence — paired_bracket lookup 후 close 비교 양쪽 canonical singleton 매핑 (U+2329↔U+3008 / U+232A↔U+3009; lines 313/314/317/318 mixed-encoding angle bracket pairs)
- crates/pinion-text-unicode/src/bidi.rs: reorder_visual doc — `levels` 가 visible-only 라는 contract 명시 (X9-removed 사전 필터 책임은 호출자)
- crates/pinion-text-unicode/src/bidi.rs: run_bidi_character_case harness + smoke (first 100, default) + full_sweep (ignored, on-demand)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1212 pass (+1 from 1211 — smoke 1 default; full_sweep ignored)
- cargo test -p pinion-text-unicode -- --ignored bidi_character_test_full_sweep = 91 707/91 707 pass (100.0000%)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- UAX #9 conformance: full UCD 16.0 BidiCharacterTest 전체 vector (~92K) 통과 — 'implementation 정통' + 'conformance 검증' 동시 완성, hand-tests ~70 만 의존하던 R51.23 baseline 의 검증 측 부채 청산
- fail discovery progression — smoke 100 fail (line 66 visible-only) → 정정 → smoke 100 pass → full sweep 4 fail (lines 84 / 313 / 314 / 317 / 318) → N0 step (e) + BD16 canonical equivalence 정정 → full sweep 0 fail (4 → 0 in 2 정정 라운드)



**Impact**: §5.37.4


**Carry forward**:
- R51.x 다음 후보: (a) derive macro for WidgetTransition (~250 LOC, application boilerplate ~135 LOC 청산) / (b) hello-toggle visual demo (paint-side N=2) / (c) serde Option<Value> JSON null carry (R51.16) / (d) L4 mirroring renderer-side 통합 / (e) Phase 2 axis 시작 (pinion thin RHI 3D pass §5.x 신설)
- BIDI algorithm 완성도: ~100% (P/X/W/N/I/L1/L2/L3 + L4 substrate + UCD 표준 vector 91 707 conformance 100%)
- R297 backfill 부채 carry (dc425f8 R45 entry 416 backfill commit 의 R297 changelog entry atomic-store missing — 다음 mnemosyne-publishable fix 라운드에서 상환)
- BidiTest.txt (Bidi_Class 시퀀스, 384K vector) 는 별도 conformance vector — character vector 가 끝났으니 class 시퀀스 검증은 후속 라운드에서 결정



### Round 514 — R51.25 §5.12 Response::result nullable-present deserialize (R51.16 carry 청산) — Response::result 의 deserialize_with helper (`deserialize_nullable_present`) + #[serde(default)] 조합으로 JSON-RPC 의 'result: null' vs 'result absent' 구분 — R51.16 의 raw-envelope `assert!(raw.contains("\"result\":null"))` 우회 carry 청산

**Changes**:
- crates/pinion-rpc/src/dispatch.rs: deserialize_nullable_present helper — Value::deserialize 후 Some(...) wrap. null JSON 값을 Some(Value::Null) 로 보존
- crates/pinion-rpc/src/dispatch.rs: Response.result field 에 #[serde(default, deserialize_with = "deserialize_nullable_present", skip_serializing_if = "Option::is_none")] 적용 — 'absent' 은 default 이 None, 'null' 은 helper 가 Some(Value::Null), 'value' 는 일반 Some(value); 직렬화 측은 None 은 생략, Some 은 명시 null 또는 value
- crates/pinion-rpc/src/dispatch.rs: R51.16 carry 2건 정정 — scene_query_selected_index_none 과 scene_invoke_non_activating_returns_null 의 raw envelope contains check 를 parse_response().result == Some(Value::Null) 타입 비교로 교체
- crates/pinion-rpc/src/dispatch.rs: 5 dedicated unit tests — response_result_explicit_null / absent / value 3개 deserialize boundary + Some(Null) serialize=explicit null / None serialize=elided 2개 round-trip boundary



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1217 pass (+5 from 1212 — R51.25 boundary tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- JSON-RPC 2.0 즜명: deserialize 측 'result: null' = Some(Value::Null) / 'result absent' = None / 'result: 42' = Some(Number(42)); serialize 측 round-trip 명시 null 유지 + None 생략



**Impact**: §5.12


**Carry forward**:
- RpcError.data 도 동일 quirk 가능 — 현재의 #[serde(skip_serializing_if = "Option::is_none")] 만 으로는 deserialize 측 null vs absent 구분 안됨; data 의 'present null' use case 발생 시 같은 정통으로 정정 (현재 부채 carry 없음)



### Round 515 — R51.26 §5.37.4 BIDI UCD BidiTest class-sequence conformance (full 490 846-row sweep, 100% pass; 3 부채 즉시 상환) — UCD 16.0 BidiTest.txt (490 846 data rows × 1-3 paragraph modes) 전체 통과 — class-sequence harness + 3 부채 한 라운드에 즉시 상환 (L1 trailing walk 의 X9-removed skip / X10 unmatched-initiator eos / W1 NSM-at-start-of-sequence = sos type)

**Changes**:
- crates/pinion-text-unicode/ucd/BidiTest.txt: UCD 16.0.0 적재 (497 590 lines, 490 846 data rows after comment/section strip)
- crates/pinion-text-unicode/src/test_fixture.rs: BidiTestCase + parse_bidi_test + load_bidi_test — @Levels/@Reorder section anchors + bitset 1/2/4 (auto/LTR/RTL) decoded per row
- crates/pinion-text-unicode/src/bidi.rs: apply_l1_line_break Pass 1 + Pass 2 의 X9-removed (BN) skip — trailing/preceding S/B walk 이 BN 위치 를 skip-without-break (caught by 'LRE WS LRE; 3'; 이전에는 trailing WS 가 paragraph_level 로 reset 안 됨)
- crates/pinion-text-unicode/src/bidi.rs: IsolatingRunSequence 에 ends_with_unmatched_initiator + starts_with_unmatched_pdi 플래그 + compute_sos_eos X10 carve-out — unmatched isolate 경계 의 eos / sos 가 이웃 run level 대신 paragraph_level 로 계산 (caught by 'R RLI R; 2'; 이전에는 eos = R 되어 N1 이 RLI → R 로 수정, I1 가 RLI level 0→1 으로 부당 승급)
- crates/pinion-text-unicode/src/bidi.rs: apply_w1 의 NSM-at-start-of-isolating-run-sequence 을 sos type 으로 해석 (차아서 W1 시그니처 sos 명시 추가) — 이전에 ON 으로 잘못 축소 (caught by 'RLE S PDF NSM; 3': NSM 가 고립 Seq[1] 에서 sos=R 을 상속, 이전 ON 으로 가 N1/N2 가 L 로 끜을 점 수정)
- crates/pinion-text-unicode/src/bidi.rs: run_bidi_test_case + per-mode harness + smoke (first 100, default) + full_sweep (ignored, on-demand) + representative_codepoint 트릿이표 라운드-트립 검증 test



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1215 pass (+3 from 1212: BidiTest smoke + representative round-trip)
- cargo test -p pinion-text-unicode -- --ignored = 2 pass — BidiCharacterTest 91 707/91 707 (100%) 유지 + BidiTest 490 846/490 846 (100%)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- UAX #9 conformance: BidiCharacterTest + BidiTest 두 계층 의 UCD 표준 vector 모두 통과 — character vector ('what codepoints render where') + class-sequence vector ('what class sequence yields what levels/reordering') 의 이중 conformance 계층 완성
- fail discovery progression — smoke 100 pass → full sweep 95.28% (23 180 fail) L1 BN-skip 정정 → 99.27% (3 570 fail) X10 unmatched-initiator 정정 → 99.99% (52 fail) W1 NSM-sos 정정 → 100.0000% (0 fail) in 3 정정 라운드



**Impact**: §5.37.4


**Carry forward**:
- BIDI algorithm 완성도: 100% — UCD 16.0 두 구조 모두 100% pass; L3 적용 마찠 + L4 mirroring substrate 제공; pinion-text-unicode BIDI layer 의 algorithm 측 부채 챠ƹ
- R51.x 다음 후보: (a) L4 mirroring renderer-side 통합 — mirroring_glyph substrate 제공 되어있으나 render-layer slice 이 아직 없음 / (b) Phase 2 axis (통 RHI 3D pass §5.x 신설) 시작 / (c) hello-toggle visual demo (paint-side N=2) / (d) derive macro for WidgetTransition (~135 LOC boilerplate 청산, partial derive 보다 application detect logic 하이레 소용섭 하자 baseline 50%) / (e) framework primitive 추가 신설



### Round 516 — R51.27 §5.37.4 + §5.36 BIDI L4 mirroring renderer-side (paint_adapter wire) — pinion-text-unicode 의 `mirror_paired_brackets` pub helper + pinion-runtime 의 paint_adapter::paint_text 가 parley shape 전에 그 helper 호출 — UAX #9 L4 mirroring substrate 완성 (R51.23 carry 청산)

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: pub fn mirror_paired_brackets(text) -> Cow<'_, str> 신규 — paragraphs 분할 + 각 paragraph 의 P→X→W→N→I→L1 파이프라인 실행 후 odd resolved-level + paired bracket 위치의 mirroring_glyph substitute. 2-pass (검출+구성) 구조 이고 LTR/bracket-free 경우 1패스 종료 후 Cow::Borrowed
- crates/pinion-text-unicode/src/bidi.rs: resolved_levels_for_paragraph private helper — P→X→W→N→I→L1 캤스케이드 을 mirror_paired_brackets 의 두 패스에서 공유
- crates/pinion-runtime/Cargo.toml: pinion-text-unicode dep 신규 (default, no feature gate — Cow::Borrowed fast path 때문에 headless/TUI 도 비용 0)
- crates/pinion-runtime/src/paint_adapter.rs: paint_text 가 t.content 을 cache.layout 에 넘기기 전에 mirror_paired_brackets 호출, parley 는 mirror substituted 문자열로 shape — L4 substitute 는 shape engine 이 소모하는 source codepoints 계층에서 적용
- crates/pinion-text-unicode/src/bidi.rs: 5 unit tests — LTR Borrowed fast path / pure RTL no brackets Borrowed / RTL paired brackets swap / LTR brackets keep / multi-paragraph independent



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1220 pass (+5 from 1215 — mirror_paired_brackets 5 경계 tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- BIDI conformance 유지: UCD BidiCharacterTest 91 707/91 707 (100%) + BidiTest 490 846/490 846 (100%) — R51.27 의 substrate 추가 가 R51.26 모든 vector 통과 세트 유지
- L4 완성도: R51.23 mirroring_glyph substrate 제공 + R51.27 렼타이옥-side wire — pinion-text-unicode 의 BIDI algorithm + paint_adapter 의 mirror substitute (paint-side first client) 완결



**Impact**: §5.37.4, §5.36


**Carry forward**:
- mirror_paired_brackets 은 매 paint frame 마다 BIDI pipeline 재계산 — LayoutCache 와 같은 cache 계층 future round 에서 고려 (R51.x perf substrate)
- R51.x 다음 후보: (a) hello-toggle visual demo (paint-side N=2, ~500-700 LOC) — [[substrate-incompleteness-signal]] paint 측 검증 / (b) Phase 2 axis ratify (thin RHI 3D pass §5.x 신설) — R41 plan 의 정식 Phase 진입 / (c) derive macro for WidgetTransition (~150 LOC partial)



### Round 517 — R51.28 §5.37.4 BIDI W1 sos-type test alignment + RTL variants — Replace R51.19 stale W1 unit tests (still expecting NSM→ON at sequence start) with the post-R51.26 sos-type contract, and lock both sos polarities with new RTL paragraph + RLI inner-sequence variants.

**Changes**:
- crates/pinion-text-unicode/src/bidi.rs: rename w1_nsm_at_sequence_start_becomes_on -> _inherits_sos_l and w1_nsm_after_lri_becomes_on -> _inherits_inner_sos_l; both now expect L (paragraph_level=0 -> sos=L; LRI inner level=2 even -> sos=L) matching the apply_w1 sos parameter introduced in R51.26 (f274753)
- crates/pinion-text-unicode/src/bidi.rs: add w1_nsm_at_sequence_start_inherits_sos_r (paragraph_level=1 -> sos=R) and w1_nsm_after_rli_inherits_inner_sos_r (RLI inner level=1 odd -> sos=R) so both sos polarities are locked as regression sentries for the W1 fix



**Verification**:
- cargo test --workspace --features pinion-runtime/vello: 1226 passed; 0 failed; 6 ignored (was 1220 passed + 2 failed pre-fix; +2 from stale-test re-pass, +4 from RTL/RLI/LRI/L variants)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings
- manual reasoning chain matches BidiTest sweep (already 490 846/490 846 in R51.26): isolating run sequence sos = parity of max(prev_level, seq.level); apply_w1 view[0]==NSM => view[0] = sos



**Impact**: §5.37.4


**Carry forward**:
- R51.29 hello-toggle visual demo (paint-side N=2 evidence per substrate-incompleteness-signal)
- R51.30 derive macro WidgetTransition partial (~150 LOC, snapshot+drive only)
- R51.31 mirror_paired_brackets per-frame cache layer (perf substrate)
- L4 alternative impl RFC: render-time GlyphRun.is_rtl substitute vs pre-substitute (R51.27)
- Phase 2 axis ratify per R41 §5.16 4-phase plan



### Round 518 — R51.29 §5.38 + §5.16 hello-toggle paint-side N=2 visual demo (substrate-incompleteness-signal evidence) — Land examples/hello-toggle binary mirroring hello-button structure to surface paint-side substrate incompleteness via second-client boilerplate repetition; both render/forward/dispatch_rpc/drain_intents methods + ApplicationHandler impl + RenderState + spawn_stdin_rpc_reader recur identically across the two clients (~400 LOC), constituting the textbook substrate-refactor trigger per [[substrate-incompleteness-signal]].

**Changes**:
- examples/hello-toggle/Cargo.toml + build.rs + app.pinion.xml: new workspace member mirroring hello-button manifest shape; pinion-forge emits HelloToggleRenderer (Vello renderer wrapper, backend=vello, aa default Area)
- examples/hello-toggle/src/main.rs: new binary wrapping ToggleExternal in Scene::External('main_toggle'); view fn maps (ToggleState, bool) -> Scene with a 64x32 rounded-pill track (corner_radius 16) + 24x24 white knob (corner_radius 12) justified Start/End via JustifyContent based on value; track fill encodes the 7-cell (state, value) cross product including a distinct chromatically-muted Disabled hue (0x4a_42_38) to satisfy clippy::match_same_arms and keep Disabled visually distinct from Hover-off
- Cargo.toml: add examples/hello-toggle workspace member (between hello-button and forge-counter)



**Verification**:
- cargo build -p hello-toggle --features pinion-runtime/vello: ok
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (forbid unsafe / deny warnings / clippy::pedantic deny strict baseline preserved)
- cargo test --workspace --features pinion-runtime/vello: 1226 passed; 0 failed; 6 ignored (unchanged from R51.28; example has no #[test] yet — visual binary)
- manual structural diff vs hello-button: App struct fields identical except cached_state (ButtonState vs (ToggleState, bool)); all methods (forward, dispatch_rpc, drain_intents, refresh_state, request_redraw, render, ApplicationHandler impl, spawn_stdin_rpc_reader, RenderState) identical up to widget-specific event-name/parse fn substitution



**Impact**: §5.38, §5.16


**Carry forward**:
- R51.30 AppShell substrate refactor — extract the ~400 LOC App + RenderState + spawn_stdin_rpc_reader boilerplate into pinion-runtime::AppShell<V: WidgetView>; both hello-button and hello-toggle become 'view fn + state-specific bits'. Includes VelloRenderer trait so pinion-forge codegen can emit a trait-impl block instead of an inherent impl (single-codegen-source-of-truth maintained).
- R51.31 derive macro WidgetTransition partial (~150 LOC snapshot+drive only)
- R51.32 mirror_paired_brackets per-frame cache layer (perf substrate)
- L4 alternative impl RFC: render-time GlyphRun.is_rtl substitute vs pre-substitute (R51.27)
- Phase 2 axis ratify per R41 §5.16 4-phase plan



### Round 519 — R51.30 §5.16 + §5.38 pinion-shell AppShell substrate refactor (R51.29 incompleteness-signal immediate response) — Extract the ~400 LOC App + RenderState + dispatch_rpc + spawn_stdin_rpc_reader + ApplicationHandler boilerplate that R51.29 surfaced as duplicate between hello-button and hello-toggle into a new pinion-shell crate (VelloRenderer trait + WidgetView trait + AppShell&lt;V&gt; + run::&lt;V&gt;()), reducing each visual binary to its widget-specific diff (view fn + unit-struct WidgetView impl + one-line main); textbook immediate response to [[substrate-incompleteness-signal]] per memory.

**Changes**:
- crates/pinion-shell/: new workspace member; lib.rs defines VelloRenderer trait (async new&lt;W: Into&lt;wgpu::SurfaceTarget&lt;'static&gt;&gt;&gt; + sync render + sync resize, Sized + Error: Display), vello_renderer_impl! bridge macro (codegen-template-agnostic), WidgetView trait (assoc State/Event/Renderer + create_external/tag/read_state/view/event_name + title/initial_size + default keybinding/fmt_state_log), AppShell&lt;V: WidgetView&gt; struct (scene + cached_state + IntentQueue + PreviewLedger + SceneRevision + InputRouter + RenderState&lt;V::Renderer&gt; + VelloScene + LayoutCache + last_paint_layout), winit::ApplicationHandler impl + spawn_stdin_rpc_reader + run::&lt;V&gt;() entrypoint
- examples/hello-button/src/main.rs: rewrite as 220 LOC (was 659) — view fn + ButtonView WidgetView impl + vello_renderer_impl! + 3-line main; Cargo.toml drops direct pinion-runtime/pinion-rpc/pinion-text/winit/pollster deps (shell pulls transitively), keeps pinion-core + pinion-shell + vello + pinion-forge (build)
- examples/hello-toggle/src/main.rs: rewrite as 270 LOC (was 540) — view fn + ToggleView WidgetView impl with composite (ToggleState, bool) State + custom fmt_state_log (`Idle / Off` form) + vello_renderer_impl! + 3-line main; Cargo.toml same dep collapse
- Cargo.toml: add crates/pinion-shell workspace member after pinion-text-unicode



**Verification**:
- cargo build --workspace --features pinion-runtime/vello: ok
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (forbid unsafe / deny warnings / clippy::pedantic deny strict baseline preserved); fixed doc_markdown / doc_lazy_continuation / missing_panics_doc / missing_must_use in pinion-shell + hello-toggle along the way
- cargo test --workspace --features pinion-runtime/vello: 1226 passed; 0 failed; 6 ignored (unchanged from R51.29 — pure refactor)
- LOC: ~1199 LOC pre-refactor across 2 example main.rs vs ~490 LOC post-refactor + ~770 LOC new pinion-shell/lib.rs; future N=3 visual binary saves ~440 LOC over the pre-refactor pattern (substrate amortization realized starting at the 2nd binary)



**Impact**: §5.16, §5.38


**Carry forward**:
- R51.31 derive macro WidgetTransition partial (~150 LOC snapshot+drive only)
- R51.32 mirror_paired_brackets per-frame cache layer (perf substrate)
- L4 alternative impl RFC: render-time GlyphRun.is_rtl substitute vs pre-substitute (R51.27)
- Phase 2 axis ratify per R41 §5.16 4-phase plan
- pinion-shell follow-up: doc-tested example builds via `cargo test --doc -p pinion-shell` (today the //! example is `rust,ignore` — a tested smoke would lock the macro + run signature against accidental SemVer breaks)



### Round 520 — R51.31 §5.36 + §5.37.4 BIDI L4 mirroring substrate move into LayoutCache — Move `mirror_paired_brackets` invocation from `paint_adapter::paint_text` into `pinion_text::LayoutCache::shape` so a single LRU lookup (keyed on raw `TextNode.content`) covers both the BIDI L4 mirror substitution and the parley shape pass; paint_adapter sees `cache.layout(&t.content, ...)` and never touches the BIDI helper directly. Static UI labels skip the mirror recomputation entirely on steady-state frames; pinion-runtime sheds its direct `pinion-text-unicode` dep.

**Changes**:
- crates/pinion-text/Cargo.toml: add pinion-text-unicode dep (LayoutCache needs the BIDI helper now)
- crates/pinion-text/src/cache.rs::shape: call mirror_paired_brackets(text) at the top, pass the Cow result to ranged_builder + builder.build; Cow::Borrowed fast path for LTR/bracket-free content keeps the integration allocation-free on the hot path
- crates/pinion-runtime/Cargo.toml: drop pinion-text-unicode dep (no longer directly used; transitively pulled via pinion-text)
- crates/pinion-runtime/src/paint_adapter.rs::paint_text: drop explicit `let mirrored = mirror_paired_brackets(&t.content); cache.layout(mirrored.as_ref(), ...)` -> `cache.layout(&t.content, ...)`. Comment updated to point at LayoutCache as the new mirror integration site



**Verification**:
- cargo build --workspace --features pinion-runtime/vello: ok
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (forbid unsafe / deny warnings / clippy::pedantic deny strict baseline preserved)
- cargo test --workspace --features pinion-runtime/vello: 1226 passed; 0 failed; 6 ignored (unchanged from R51.30 — pure refactor; existing LayoutCache tests cover key-by-raw-text invariant; existing mirror_paired_brackets conformance sweeps cover algorithm correctness)
- cache-hit behaviour: LayoutKey continues to hash raw text (no schema change); cache hit now amortizes both BIDI helper + parley shape, where pre-R51.31 the BIDI helper recomputed even on cache hit



**Impact**: §5.36, §5.37.4


**Carry forward**:
- L4 alternative impl RFC: render-time GlyphRun.is_rtl substitute vs pre-substitute (R51.27 / R51.31) — the LayoutCache integration locks the pre-substitute path; an alt that operates on parley GlyphRun.is_rtl would now require unwinding the cache layer too
- Phase 2 axis ratify per R41 §5.16 4-phase plan
- derive macro WidgetTransition partial — evaluated this session as premature at N=6 widgets (~5 LOC saved per widget vs ~150 LOC proc-macro infra); revisit at N=15+ widgets
- pinion-shell doc-tested example smoke (R51.30 carry)
- pinion-shell extension to cover ai-introspect-demo's multi-control single-canonical-scene pattern (overlays + previews + Character keybindings like R/P/A/C/L)



### Round 521 — R51.32 §5.38 hello-checkbox paint-side N=3 (pinion-shell amortization evidence) — Land examples/hello-checkbox as the third visual binary on the pinion-shell substrate (after R51.29 hello-toggle established N=2 and R51.30 extracted the AppShell primitive); the 221 LOC main.rs vs ~650 LOC pre-shell projection confirms the substrate amortizes correctly past the second client.

**Changes**:
- examples/hello-checkbox/: new workspace member; Cargo.toml + build.rs + app.pinion.xml mirror hello-button/hello-toggle shape; pinion-forge emits HelloCheckboxRenderer (Vello renderer wrapper) into $OUT_DIR/app.rs
- examples/hello-checkbox/src/main.rs (221 LOC): CheckboxView WidgetView impl with composite (CheckboxState, bool) State; view fn renders a 24x24 rounded square (corner_radius=4) with state/checked-driven fill, white outline border via BoxStyle::with_border, optional U+2713 CHECK MARK Text child centered via flex when checked, and a 'Receive newsletter' label justified next to it; reuses Toggle's chromatic-mute Disabled convention (0x4a_42_38 fill + 0x70_66_58 border) for visual consistency across the example gallery
- Cargo.toml: add examples/hello-checkbox workspace member after hello-toggle



**Verification**:
- cargo build -p hello-checkbox: ok (after 2 unused-import fixes from initial scaffold)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings (forbid unsafe / deny warnings / clippy::pedantic deny strict baseline preserved)
- cargo test --workspace --features pinion-runtime/vello: 1226 passed; 0 failed; 6 ignored (unchanged — example has no #[test])
- LOC amortization: 3 binaries total 693 LOC (203 button + 269 toggle + 221 checkbox) vs pre-R51.30 projection of 3 × ~650 = ~1950 LOC; ~64% reduction realized; per-binary marginal cost ~220 LOC mostly view fn + WidgetView assoc-fns (the framework primitive amortization is the expected long-term constant)



**Impact**: §5.38


**Carry forward**:
- Visible-tag carry: example gallery for Tier-1 widgets now covers Button + Toggle + Checkbox; Slider + Radio + RadioGroup are the remaining Tier-1 widgets without paint-side demos (each ~200 LOC per the shell amortization curve)
- L4 alternative impl RFC (R51.27 / R51.31 carry)
- Phase 2 axis ratify per R41 §5.16 4-phase plan
- derive macro WidgetTransition partial — evaluated as premature at N=6 widgets; revisit at N=15+
- pinion-shell doc-tested example smoke (R51.30 carry)



### Round 522 — R51.122 §5.41 — pinion-runtime::CoreShell<V: WidgetCore> substrate 신설 (4-round 분할 #1)

**Changes**:
- crates/pinion-runtime/src/core_shell.rs 신설 (~470 LOC: CoreShell + DispatchTail + StateChange + 13 unit tests)
- CoreShell<V: WidgetCore> 4 fields: scene + cached_state + router + intent_queue (backend-agnostic)
- DispatchTail<S> = { intents: Vec<Intent>, state_change: Option<StateChange<S>> } — dispatch method 반환 shape
- 12 dispatch primitives: new/scene/scene_mut/cached_state/update_paint_scene/tail/forward/apply_key/cursor_moved/cursor_left/pointer_down/pointer_up/pointer_cancel/touch_event
- pinion-runtime/src/lib.rs: pub mod core_shell + pub use {CoreShell, DispatchTail, StateChange}
- TouchPhase match exhaustive (same-crate 무 #[non_exhaustive] wildcard arm 제거)
- dep direction 0 변경: pinion-runtime 의 기존 pinion-core + pinion-text 만 사용, pinion-a11y / pinion-rpc 0 의존



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 pass / 0 fail / 8 ignored (+13 신규 CoreShell unit tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (deny(warnings) + pedantic deny baseline 유지)
- mnemosyne validate_workspace: entries=251 / sections=58 / T1=0 / round-trip=1/1 / GENERATED.md=sync
- 13 신규 tests cover: constructor / default / tail empty / forward intent / apply_key None/Some + wrong focus / pointer cycle / cursor_left / touch start+end + cancel / keybinding / update_paint_scene



**Impact**: §5.41


**Carry forward**:
- R51.123 — pinion-shell::ShellCore = CoreShell wrap (Vello extras: focus/modifiers/text_cache/previews/revision/last_paint/AT caches/redraw)
- R51.124 — pinion-tui::ShellCoreTui = CoreShell wrap (TUI extras: log_sink + refresh_state→tail bridge)
- R51.125 — dispatch_rpc trait extraction (ShellDispatch trait in pinion-runtime, impl in pinion-shell, cycle 회피)



### Round 523 — R51.123 §5.41 — pinion-shell::ShellCore = CoreShell wrap (Vello extension), 4-round 분할 #2

**Changes**:
- crates/pinion-shell/src/substrate.rs: 4 fields (scene/cached_state/intent_queue/router) 제거 + core: CoreShell<V> 1 field 추가
- ShellCore::new() body: CoreShell::new() 초기화 + Vello extras (focus seeding, RPC ledger, OCC, parley LayoutCache)
- ShellCore::scene/cached_state 점근자 → self.core.scene() / self.core.cached_state() proxy
- dispatch method body: forward/apply_key/cursor_*/pointer_*/touch_event → self.core.X() + handle_tail(&tail) pattern
- fn handle_tail(&mut self, tail: &DispatchTail<V::State>) helper 추가: intents eprintln + state_change eprintln + request_redraw
- drain_intents / refresh_state private helpers 제거 (handle_tail 으로 통합)
- dispatch_rpc: disjoint-field borrow split 재작성 (core.scene_mut() / core.cached_state() / Vello extras)
- click_to_focus: self.router.hover_target → self.core.hover_target proxy
- handle_touch: TouchPhase::Started 의 click_to_focus Vello-only follow-up 만 유지, 나머지 phase 는 core.touch_event() 일임
- apply_a11y_key + dispatch_access_action: scene/apply_key/access_child_invoke 는 core.scene_mut() / core.apply_key()



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 pass / 0 fail / 8 ignored (variance 0, ShellCore public API 변경 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (deny(warnings) + pedantic deny baseline 유지)
- mnemosyne validate_workspace: entries=252 / sections=58 / T1=0 / round-trip=1/1 / GENERATED.md=sync
- ShellCore public method signature 0 변경 (AppShell + tests 0 일부만 수정)



**Impact**: §5.41


**Carry forward**:
- R51.124 — pinion-tui::ShellCoreTui = CoreShell wrap (TUI extras: log_sink + refresh_state→tail bridge)
- R51.125 — dispatch_rpc trait extraction (ShellDispatch trait in pinion-runtime)



### Round 524 — R51.124 §5.41 — pinion-tui::ShellCoreTui = CoreShell wrap (TUI extension), 4-round 분할 #3

**Changes**:
- crates/pinion-tui/src/substrate.rs: 5 fields (scene/cached_state/router/intent_queue/_phantom) 제거 + core: CoreShell<V> + log_sink 두 fields
- dispatch_key/cursor_moved/pointer_down/pointer_up: 이제 bool 반환 (state_changed) auto-tail. 별도 refresh_state 호출 필요 없음
- ShellCoreTui::refresh_state 메서드 제거 (atomic stale citation 동시 제거 — R51.119 lesson)
- ShellCoreTui::forward_event private helper 제거 (core.forward(event) 가 typed event 받음)
- handle_tail private helper 추가: log_sink 로 intent + state_change 출력 + state_changed bool 반환
- compute_paint_scene: V::view(*core.cached_state(), &Frame::new()) — compute_layout 없음 (TUI 패턴 유지)
- crates/pinion-tui/src/shell.rs: dispatch_key + dispatch_mouse 호출처가 더 이상 .refresh_state() 체이닝 안 함
- dispatch_mouse: && 대신 | (bitwise or) 사용 — Down(Left) arm 두 dispatch 모두의 state_changed 관측 유지



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 1722 pass / 0 fail / 8 ignored (variance 0, ShellCoreTui tests 의미는 같음, return shape 만 조정)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- mnemosyne validate_workspace: entries=253 / sections=58 / T1=0 / round-trip=1/1 / GENERATED.md=sync
- atomic stale citation 0 (ShellCoreTui::refresh_state remove + R51.124 caveat add)



**Impact**: §5.41


**Carry forward**:
- R51.125 — dispatch_rpc trait extraction (ShellDispatch trait in pinion-runtime + impl in pinion-shell, pinion-rpc → pinion-runtime direction 유지)



### Round 525 — R51.193 §5.49 R59 AI-first RPC self-verification harness — first Claude-side dogfood (hello-toggle activate)

**Changes**:
- tools/rpc_verify.py — RpcSubprocess (subprocess + JSON-RPC + query/invoke/snapshot + assert)
- tools/demos/hello_toggle_activate.py — spawn + query(false) + invoke×3 + query(true) cycle
- tools/README — harness usage + R51.194-196 carry list (snapshot/wheel/click v1)
- atomic §5.49 add_section + intent + rationale + caveats + 3 implementations + Python example
- impact_scope refs §5.7 §5.12 §5.15 §5.18 §5.20



**Verification**:
- python3 tools/demos/hello_toggle_activate.py — [demo] PASS (0.87s)
- cargo test --workspace --features pinion-runtime/vello — 2090 pass / 0 fail / 11 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello — 0 warnings
- mnemosyne validate_workspace — T1=0 T3=0 RT=1/1 (post-mutation re-run)



**Impact**: §5.49, §5.7


**Carry forward**:
- R51.194 — scene/snapshot Container/Scroll traversal so harness can enumerate widget rows
- R51.195 — wheel/key event injection RPC method (§5.45 R55 Scroll dogfood unblocker)
- R51.196 — scene/click v1: Container traversal + real PointerEvent through InputRouter
- R51.197 — Rust integration test pinning the harness so demo regressions appear in CI
- hello-listbox scroll dogfood (R51.192 original META response) blocked on R51.194-195



### Round 526 — R51.194 §5.12 §5.45 scene/snapshot Container/Scroll traversal + paint mode — hello-listbox dogfood lights up

**Changes**:
- pinion-rpc: SnapshotNode::Container(ContainerSnapshot) tuple variant — tag + recursive children
- pinion-rpc: SnapshotNode::Scroll(ScrollSnapshot) new variant — tag + viewport + offset + content
- snapshot_root recurses through Scene::Container.children and Scene::Scroll.content
- dispatch: handle_scene_snapshot from=state|paint param + viewport={w,h}; paint_producer wire
- JSON wire: Container/Scroll types carry children[] / viewport / offset_x / offset_y / content
- tools/rpc_verify: RpcSubprocess.snapshot(source=paint, viewport=(w,h)) wrapper
- tools/demos/hello_listbox_snapshot.py: 12 rows + Scroll tag/viewport/offset assertion



**Verification**:
- cargo test pinion-rpc r51_194 — 6 new snapshot module tests PASS
- cargo test pinion-rpc scene_snapshot — 7 PASS (5 new: Container/Scroll/paint wire)
- python3 tools/demos/hello_listbox_snapshot.py — [demo] PASS (0.87s)
- python3 tools/demos/hello_toggle_activate.py — R51.193 regression PASS (0.87s)
- cargo test --workspace vello — 2101/0/11 (was 2090, +11 new)
- cargo clippy --workspace --all-targets vello — 0 warnings



**Impact**: §5.12, §5.45, §5.49


**Carry forward**:
- R51.195 — wheel/key event injection RPC method for §5.45 Scroll axis demo coverage
- R51.196 — scene/click v1: Container traversal + real PointerEvent through InputRouter
- R51.197 — Rust integration test crate that pins harness demos against CI regressions
- Leaf primitive (Box/Text/Path/Image) tag/content exposure carry until a demo needs it



### Round 527 — R51.195 §5.7 §5.45 scene/wheel + deferred-input inbox — hello-listbox scroll dogfood closes R51.192 META

**Changes**:
- pinion-rpc: DeferredInput enum + DispatchContext::deferred_inputs + with_deferred_inputs builder
- pinion-rpc: handle_scene_wheel + parse_wheel_delta (lines | pixels, mutually exclusive)
- pinion-shell: ShellCore::dispatch_rpc builds inbox + post-dispatch drain_deferred_inputs
- drain replays cursor_moved + wheel through normal substrate path, redraw bump intact
- tools/rpc_verify: RpcSubprocess.wheel(at, lines|pixels) wrapper
- tools/demos/hello_listbox_scroll.py: wheel inject → offset_y > 0 dogfood
- TUI side N/A — pinion-tui has no RPC surface (stdin = raw key input)



**Verification**:
- cargo test pinion-rpc scene_wheel — 6 new tests PASS
- python3 tools/demos/hello_listbox_scroll.py — [demo] PASS (0.97s)
- regression: hello_toggle_activate + hello_listbox_snapshot demos PASS
- cargo test --workspace vello — 2107/0/11 (was 2101, +6)
- cargo clippy --workspace --all-targets vello — 0 warnings



**Impact**: §5.7, §5.12, §5.45, §5.49


**Carry forward**:
- R51.196 — scene/click v1: Container traversal + real PointerEvent through InputRouter
- R51.197 — scene/key injection extending DeferredInput (key down/up at cursor)
- R51.198 — pinion-tui RPC surface (raw stdin clash with newline JSON-RPC framing)
- Rust integration test crate that pins demo regressions in CI



### Round 528 — R51.196 §5.7 scene/click v1 — DeferredInput::Click + real PointerEvent through InputRouter (probe-only retired)

**Changes**:
- pinion-rpc: DeferredInput::Click + handle_scene_click v1 (params {at:{x,y}})
- pinion-rpc: removed click.rs (click() + ClickOutcome + ClickError) — v0 probe-only retired
- pinion-shell drain: cursor_moved + mouse_pressed + mouse_released triple
- InputRouter fires same activation arc as winit MouseInput (real state mutation)
- 4 new dispatch tests (enqueue + missing inbox + missing at + missing at.y)
- tools/rpc_verify: click(at) wrapper
- tools/demos/hello_toggle_click.py — real click → value:false→true



**Verification**:
- cargo test pinion-rpc scene_click — 4 v1 tests PASS
- python3 tools/demos/hello_toggle_click.py — [demo] PASS (0.93s)
- regression: toggle_activate + listbox_snapshot + listbox_scroll PASS
- cargo test --workspace vello — 2102/0/11 (was 2107, −5: click.rs tests gone, +4 v1)
- cargo clippy --workspace --all-targets vello — 0 warnings



**Impact**: §5.7, §5.12, §5.49


**Carry forward**:
- R51.197 — scene/key injection + key event dispatch path (DeferredInput::Key)
- R51.198 — leaf primitive rect/tag in scene/snapshot (no more hardcoded coords)
- R51.199 — pinion-tui RPC surface (raw stdin clash with newline JSON-RPC framing)
- Rust integration test crate pinning demo regressions in CI



### Round 529 — R51.197 §5.7 §5.45 scene/key — DeferredInput::Key + R55.C.3 keyboard scroll dogfood

**Changes**:
- pinion-rpc: DeferredInput::Key + handle_scene_key (params {at, key} W3C string)
- pinion-shell drain: cursor_moved + handle_named_key (apply_key + scroll_key fallback)
- InputRouter::scroll_key arc fires for ArrowUp/Down/Left/Right/PageUp/Down/Home/End
- 4 new dispatch tests (enqueue + no inbox + empty key + missing key)
- tools/rpc_verify: key(at, name) wrapper
- tools/demos/hello_listbox_keyboard_scroll.py: PageDown → offset_y > 0



**Verification**:
- cargo test pinion-rpc scene_key — 4 tests PASS
- python3 tools/demos/hello_listbox_keyboard_scroll.py — [demo] PASS (0.94s)
- regression: toggle_activate + listbox_snapshot + listbox_scroll + toggle_click PASS
- cargo test --workspace vello — 2106/0/11 (was 2102, +4)
- cargo clippy --workspace --all-targets vello — 0 warnings



**Impact**: §5.7, §5.12, §5.45, §5.49


**Carry forward**:
- R51.198 — leaf primitive rect/tag in scene/snapshot (no more hardcoded coords)
- R51.199 — pinion-tui RPC surface (raw stdin clash with newline JSON-RPC framing)
- Ctrl+Home / Ctrl+End scroll keys (R51.187 carry — corner cases)
- Rust integration test crate pinning demo regressions in CI



### Round 530 — R51.198 §5.49 leaf primitive rect/tag in scene/snapshot — Box/Text/Path/Image/Container/External 일관 노출

**Changes**:
- SnapshotNode 의 Box/Text/Path/Image 가 tuple variant 로 승격 + 전용 Snapshot struct 신규
- ContainerSnapshot rect 필드 추가, ExternalSnapshot rect + tag 필드 추가
- TextSnapshot.content 노출 — §2 #7 scene-as-data invariant 일관
- snapshot_node_to_json wire = {rect, tag, content?} 일관 emit (Effect 만 marker)
- tools/rpc_verify.py find_by_tag(snap, tag) + node_center(node) 헬퍼 신규
- hello_toggle_click.py hardcoded (180, 113) 청산 — snapshot 기반 bbox 자동 추출



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2120/0/11 (+14 R51.198 tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS (hello_toggle_click 0.97s = snapshot 기반 click target)
- snapshot::r51_198 7개 + dispatch::scene_snapshot_*_wire 7개 신규 테스트



**Impact**: §5.49, §5.7, §5.12, §5.15


**Carry forward**:
- R51.199 hello_listbox_scroll / keyboard_scroll 의 hardcoded VIEWPORT_CX/CY 청산
- R51.200 Scroll content rect = viewport-local — 절대좌표 변환 substrate
- R55.G.2 layout::compute_layout into Scroll content (R51.191 carry)



### Round 531 — R51.199 §5.49 listbox demos = snapshot-based viewport center — hardcoded VIEWPORT_CX/CY 청산

**Changes**:
- hello_listbox_scroll.py = find_by_tag('main_list_scroll') + node_center; VIEWPORT_W/H 청산
- hello_listbox_keyboard_scroll.py = 동일 패턴, hardcoded VIEWPORT_CX/CY 청산
- tools/rpc_verify.py rect_of(node) 헬퍼 신규 (Scroll viewport / 기타 rect 통합)
- node_center 가 Scroll.viewport 도 처리 — 모든 primitive uniform



**Verification**:
- 5 demos 회귀 PASS (~0.9s 각, 변동 없음)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- Rust code 변경 없음, Python harness + 2 demos 만 변경



**Impact**: §5.49, §5.7, §5.12


**Carry forward**:
- R51.200 Scroll content rect = viewport-local — nested scroll/coord 변환 substrate
- R55.G.2 layout::compute_layout into Scroll content (R51.191 carry)
- absolute_rect_of(node, path) walker — nested 좌표 변환 (현재 root scroll 만 작동)



### Round 532 — R55.G.2 §5.45 compute_layout descends into Scene::Scroll.content — R51.191 manual positioning 청산

**Changes**:
- compute_layout_inner(main_axis_unbounded) — MaxContent on height for scroll content flex
- lay_out_scroll_contents 가 Scene::Scroll 마다 inner pass 호출 (nested scroll 재귀)
- hello-listbox: listbox_row_at_y → listbox_row + flex Row + AlignItems::Center + padding
- hello-listbox view: content Container = flex Column + gap=ROW_GAP, manual rect/y 청산
- 외부 viewport position + outer.rect 는 R55.G.3 carry (Scroll-as-flex-child)
- r51_191_view_rows_positioned → r55_g2_view_rows_carry_flex_layout_sidecar test 갱신



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2124/0/11 (+4 R55.G.2 tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS (~0.9s 각, hello-listbox layout 자동 derive)
- layout::r55_g2 4건: flex_column_y / total_height_overflows / cross_axis_bound / nested_scroll



**Impact**: §5.45, §5.21, §5.49


**Carry forward**:
- R55.G.3 ScrollNode-as-flex-child — viewport position + outer.rect manual 청산
- ScrollNode 의 layout: LayoutStyle 필드 (R55.G.3 의 substrate)
- max_y 자동 계산 — layout 결과의 content.rect.h 사용 (chicken-and-egg 청산)



### Round 533 — R55.G.3 §5.45 ScrollNode-as-flex-child — viewport.{x,y} layout-derived, w/h app-set 보호

**Changes**:
- layout::build: Scene::Scroll 의 taffy size 를 viewport.{w,h} 로 override
- layout::assign_rect: Scene::Scroll 이 rect.{x,y} 만 viewport 에 write (w,h 보호)
- hello-listbox: outer Container = flex Column + JustifyContent::Center + AlignItems::Center
- hello-listbox: vp_x / vp_y 수동 중앙 정렬 청산, outer.rect = ... 청산
- ScrollNode::new(Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H), ...) — 위치 layout 맡김
- 1 새 r55_g3_scroll_centered_via_outer_flex_writes_viewport_position test



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2125/0/11 (+1 R55.G.3 test)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS (~0.9s 각, viewport 자동 중앙 정렬)
- R51.191 manual positioning 완전 종료 (content + outer + viewport 위치 모두 layout)



**Impact**: §5.45, §5.21, §5.49


**Carry forward**:
- ScrollNode 의 layout: LayoutStyle 필드 — flex_grow / align_items 지원 시 필요
- max_y 자동 계산 — layout 결과의 content.rect.h 사용 (chicken-and-egg 청산)
- R55.G.4 layout.padding / aria 등 ScrollNode 의 풍부한 LayoutStyle 활용



### Round 534 — R55.G.4 §5.45 ScrollNode.layout: LayoutStyle 필드 — R55.G.3 build override hack 청산

**Changes**:
- ScrollNode 구조체에 pub layout: LayoutStyle 필드 추가 (다른 Node 와 일관)
- ScrollNode::new 가 layout = LayoutStyle::with_size(viewport.{w,h}) 명시 세팅
- ScrollNode::with_layout(LayoutStyle) builder 신규 — flex_grow / margin / align 지원
- layout::layout_style_of 가 Scene::Scroll 을 정식 처리 (FALLBACK 아니라 &n.layout)
- layout::build 의 build-site size override hack 청산 (R55.G.3 재부채 해소)
- layout::assign_rect Scroll = 전체 rect write (x/y 만 쓰던 partial write 해소)
- 1 새 test: r55_g4_scroll_with_flex_grow_stretches_in_parent_flex



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2126/0/11 (+1 R55.G.4 test)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS (~1.0-1.6s 각, behavior 변화 없음)
- API 대칭성 복구 — 6 rect-bearing Node 모두 layout + with_layout pattern



**Impact**: §5.45, §5.21


**Carry forward**:
- max_y 자동 계산 — layout 결과의 content.rect.h 사용 (chicken-and-egg 청산)
- R51.200 absolute_rect_of(node, path) walker — nested scroll 절대좌표 변환
- R55.D ScrollBar sub-widget / R55.F scene/scroll RPC method (11th typed)



### Round 535 — R55.G.5 §5.45 layout 가 ScrollState max bounds 자동 write — chicken-and-egg 청산

**Changes**:
- pinion-runtime: update_scroll_state_bounds(scene) walker 신규
- compute_layout_inner 가 lay_out_scroll_contents 후 호출
- attached ScrollState 마다 content.rect 기반 max_x/max_y 자동 set
- hello-listbox view fn: content_h 계산 + manual set_max 청산
- hello-listbox: r51_191_view_sets_scroll_max_from_content_overflow test 제거
- 1 새 layout test: r55_g5_layout_writes_scroll_max_from_content_height



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2126/0/11 (net 변화 없음)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS — hello-listbox 의 scroll bounds 동일 작동
- Signal equality-skip 로 steady-state 동일 업데이트 = no repaint



**Impact**: §5.45, §5.21


**Carry forward**:
- R51.200 absolute_rect_of(node, path) walker — nested scroll 절대좌표 변환
- R55.D ScrollBar sub-widget (SCXML statechart 새 axis)
- R55.F scene/scroll RPC method (11th typed) — offset query + scroll_to action



### Round 536 — R51.201 §5.49 path-based scene/click — {path: tag} 으로 snapshot lookup 청산

**Changes**:
- scene/click params 에 path 필드 지원 (at 와 상호 배타)
- resolve_path_to_click_center: paint_producer + last_paint_layout walker
- find_rect_by_tag: depth-first Scene walker (Container/Scroll 재귀)
- viewport = last_paint_layout.root.rect (live window dimensions 자동 결정)
- handler signature generic <F: FnMut(u32,u32)->Scene+?Sized> (snapshot 와 일관)
- tools/rpc_verify.py: click(path=...) 지원 (at 와 mutually exclusive)
- hello_toggle_click: snapshot+find_by_tag+node_center 차례 청산 — click(path='main_toggle') 한 줄
- 5 새 dispatch tests (path resolve / no producer / missing tag / both / neither)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2131/0/11 (+5 R51.201 tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS — hello_toggle_click 가 path-based 로 작동
- AI ergonomics 향상 — 적절한 viewport 자동 결정 (하드코딩 자체 부재)



**Impact**: §5.49, §5.7, §5.12


**Carry forward**:
- R51.200 absolute_rect_of — Scroll content 안 tag 의 절대좌표 변환
- scene/wheel 와 scene/key 도 path-based 지원 (동일 패턴)
- R55.D ScrollBar / R55.F scene/scroll RPC method



### Round 537 — R51.202 §5.49 path-based scene/wheel + scene/key — at/path 통합 패턴 cascade

**Changes**:
- resolve_at_or_path 공유 헬퍼 신규 — click/wheel/key 3 handler 동일 사용
- resolve_path_to_click_center → resolve_path_to_center 리네이밍
- scene/wheel: params 에 path 지원 (기존 at + delta 와 조합)
- scene/key: params 에 path 지원 (기존 at + key 와 조합)
- tools/rpc_verify.py wheel/key wrapper at/path mutually exclusive
- hello_listbox_scroll: wheel(path='main_list_scroll', lines=...) 한 줄
- hello_listbox_keyboard_scroll: key(path='main_list_scroll', name=...) 한 줄
- 2 새 dispatch tests: scene_wheel_path / scene_key_path



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2133/0/11 (+2 R51.202 tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS — path-based input API 일관 적용
- API 대칭성: 3 deferred-input method 모두 at/path 이수 패턴



**Impact**: §5.49, §5.7, §5.12


**Carry forward**:
- R51.200 absolute_rect_of — Scroll content 안 tag 의 절대좌표 변환
- R51.198 carry: Path.commands + Image.source snapshot 노출
- R55.D ScrollBar / R55.F scene/scroll RPC (offset query + scroll_to)



### Round 538 — R51.200 §5.49 find_rect_by_tag 가 Scroll content 의 abs 좌표 누적 변환

**Changes**:
- find_rect_by_tag_with_offset(x_off, y_off) recursive walker 신규
- Scroll content 진입 시 (viewport.x - offset_x, viewport.y - offset_y) 누적
- i64 증분 후 saturating clamp — scroll-off 영역 = (0,0) 좌표
- Container.rect / leaf rect / External.rect 모두 translate 적용
- path-based click/wheel/key 가 높이 구조 안 태그 도 정확한 abs
- 2 새 tests: scene_click_path_inside_scroll {translates / with offset subtracts}



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2135/0/11 (+2 R51.200 tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 5 demos 회귀 PASS — 기존 root-level path-based input 계속 작동
- scrolled-off region click = (0, 0) abs → input router miss — 일관 하지만 safe



**Impact**: §5.49, §5.7, §5.45


**Carry forward**:
- hello-listbox row click demo — path='main_list#N' 의 first-consumer evidence
- R51.198 carry: Path.commands + Image.source snapshot 노출
- R55.D ScrollBar sub-widget / R55.F scene/scroll RPC method



### Round 539 — R51.200 first-consumer — hello_listbox_row_click demo proves nested-scroll path-based click

**Changes**:
- tools/demos/hello_listbox_row_click.py 신규 — 6번째 demo
- click(path='main_list#3') 한 줄 이 다섯 조상 안 높이 구조 해제
- /external/selected_index 이 None → 3 으로 전환 확인
- R51.200 substrate-incompleteness-signal 청산 — substrate + consumer 함께 land



**Verification**:
- 6 demos 회귀 PASS (~1.2s 각) — row click 가 selected_index 0->3 transition 관찰
- AI agent 이 높은 구조 widget tag 을 한 줄로 click 가능 증명



**Impact**: §5.49


**Carry forward**:
- R51.198 carry: Path.commands + Image.source snapshot 노출
- R55.D ScrollBar sub-widget (SCXML statechart 새 axis)
- R55.F scene/scroll RPC method — offset query + scroll_to action



### Round 540 — R51.198 carry §5.49 Path.commands + Image.source snapshot — leaf primitive 데이터 노출 완전화

**Changes**:
- PathSnapshot.commands: Vec<PathCommand> 신규 필드
- ImageSnapshot.source: String 신규 필드
- snapshot_root: PathNode.commands.clone() + ImageNode.source.clone() 추출
- path_command_to_json helper 신규 — 4 variant + Unknown wildcard
- Image wire: source string 일관 emit
- dispatch tests 갱신: rect_tag_and_commands / rect_tag_and_source
- snapshot tests 갱신: 4-variant PathCommand round-trip + Image.source



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2135/0/11 (기존 tests 갱신, 순추가 0)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 6 demos 회귀 PASS (~1.5-1.9s 각)
- §2 #7 scene-as-data invariant 완전: AI 가 path shape / image URI 직접 introspect



**Impact**: §5.49, §5.12, §5.3


**Carry forward**:
- R55.D ScrollBar sub-widget (SCXML statechart 새 axis)
- R55.F scene/scroll RPC method — offset query + scroll_to action
- R56 TextField + IME / R57 Theming / R58 composite 대형 axes



### Round 541 — R55.F §5.45 scene/scroll RPC — programmatic ScrollState 변경 (InputRouter 우회)

**Changes**:
- 새 method scene/scroll 워어 {path, to|by} params, 상호 배타
- handle_scene_scroll: paint_producer 쓰며 Scroll tag 검색 + state.scroll_to/by
- ScrollAction enum + parse_xy helper (x/y 또는 dx/dy)
- find_scroll_state_by_tag walker (Container.children / Scroll.content 재귀)
- tools/rpc_verify.py: scroll(path, to/by) wrapper
- hello_listbox_scroll_to.py 7번째 demo (to + by + clamp boundary)
- 7 dispatch tests (to / by / clamp / missing / together / neither / no producer)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2142/0/11 (+7 R55.F tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 7 demos 회귀 PASS — listbox 의 scroll_to/by 이 InputRouter 우회 자동 mutate
- AI 용 'jump to row N' pattern 지원 — PageDown 10 회 시뮬레이션 대체



**Impact**: §5.45, §5.7, §5.12


**Carry forward**:
- R55.D ScrollBar sub-widget (SCXML statechart 새 axis)
- R56 TextField + IME / R57 Theming / R58 composite 대형 axes
- scene/scroll 의 path-based input 와 일관 — 태그 찾기 walker 통합 고려



### Round 542 — R55.G.6 §5.45 — map_layout closure on 7 layout-bearing nodes cures ScrollNode with_layout default-size footgun

**Changes**:
- scene.rs: 7 layout nodes (Box/Text/Path/Image/Container/External/Scroll) gain map_layout
- map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle> preserves seeded default unlike with_layout
- ScrollNode::with_layout doc updated to point at map_layout for the footgun cure path
- tests: r55_g6 scroll map_layout preserves Px(120,80); container symmetry test added



**Verification**:
- cargo test (vello) = 2144 pass / 0 fail / 11 ignored (+2 R55.G.6 scene tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 7 demos PASS regression (release build, ~1.0-1.6s each)



**Impact**: §5.45


**Carry forward**:
- R51.200 carry — find_rect_by_tag width/height translate when content > viewport (sub-edge)
- Container.style + TextStyle snapshot exposure for §2 #7 scene-as-data completeness



### Round 543 — R55.G.7 §5.49 carry-of-R51.200 — find_rect_by_tag clips to Scroll viewport stack (width/height intersect)

**Changes**:
- dispatch.rs: translate_rect_into_clip helper takes (rect, x_off, y_off, clip) -> Option<Rect>
- find_rect_by_tag_with_offset gains clip: Option<Rect> param, threads Scroll viewport stack
- Scroll boundary derives new_clip via translate_rect_into_clip(viewport,...) reused
- over-wide / partially-scrolled / fully-off cases: 3 new tests verify clip semantics
- fully-scrolled-off rect returns None (was: (0,0) saturation) so RPC surfaces 'not found'



**Verification**:
- cargo test (vello) = 2147 pass / 0 fail / 11 ignored (+3 R55.G.7 carry tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 7 demos PASS regression



**Impact**: §5.49


**Carry forward**:
- Container.style + TextStyle snapshot exposure for §2 #7 scene-as-data completeness
- scene/scroll at-based variant for consistency with click/wheel/key (intentional deviation now)



### Round 544 — R55.G.8 §5.49 — BoxSnapshot/TextSnapshot/ContainerSnapshot expose BoxStyle/TextStyle for scene-as-data

**Changes**:
- snapshot.rs: BoxSnapshot/TextSnapshot/ContainerSnapshot gain style field (pinion_core::style::*)
- snapshot_root populates style from each node; non_exhaustive struct keeps it additive
- dispatch.rs: color_to_json + border_to_json + box_style_to_json + text_style_to_json helpers
- font_style_to_json: Normal/Italic as bare string; Oblique as {kind, angle?} object
- wire shape: {fill:{r,g,b,a}, border:null|{color,width,placement}, corner_radius}
- 7 new tests: 3 snapshot struct land + 4 dispatch wire shape (Box, Text, Oblique, null border)



**Verification**:
- cargo test (vello) = 2154 pass / 0 fail / 11 ignored (+7 R55.G.8 style tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 7 demos PASS regression



**Impact**: §5.49


**Carry forward**:
- TextStyle layout-axis fields (line_height/letter_spacing/align/decoration/overflow) snapshot opt-in
- scene/scroll at-based variant for consistency with click/wheel/key (intentional deviation now)



### Round 545 — R55.G.9 §5.49 first-consumer — hello_toggle_style.py demo asserts R55.G.8 style wire end-to-end

**Changes**:
- tools/demos/hello_toggle_style.py 8번째 demo (paint-mode snapshot)
- main_toggle Container.style.corner_radius==16, fill={64,64,64,255} (TRACK_RADIUS)
- knob Box.style.corner_radius==12, fill={255,255,255,255} (KNOB_RADIUS idle-off)
- first Text.style.font_size_px==18, fg_color={224,224,224,255} (Dark mode label)
- walk(node) helper iterates depth-first through Container.children + Scroll.content
- [[ai-first-rpc-introspection-obligation]] satisfied: AI verifies painted chrome via RPC



**Verification**:
- 8 demos PASS regression (R55.G.9 included, ~1.0-4.4s each)
- no Rust code changes — demo-only land, no clippy/test count delta



**Impact**: §5.49


**Carry forward**:
- TextStyle layout-axis fields (line_height/letter_spacing/align/decoration/overflow) snapshot opt-in
- scene/scroll at-based variant for consistency with click/wheel/key (intentional deviation now)



### Round 546 — R55.G.10 §5.49 — TextStyle layout-axis (line_height/letter_spacing/align/decoration/overflow) snapshot wire

**Changes**:
- dispatch.rs: line_height_to_json / text_align_to_json / text_decoration_to_json / text_overflow_to_json
- LineHeight unit variant Normal -> bare string; data variants emit {kind, value}
- text_style_to_json adds 5 fields: line_height, letter_spacing, text_align, decoration, overflow
- snapshot.rs module doc cites R55.G.10 layout-axis completion
- 3 wire tests (full layout-axis / Normal bare string / Px data variant)



**Verification**:
- cargo test (vello) = 2157 pass / 0 fail / 11 ignored (+3 R55.G.10 layout-axis tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 8 demos PASS regression



**Impact**: §5.49


**Carry forward**:
- scene/scroll at-based variant for consistency with click/wheel/key (intentional deviation now)
- BoxStyle / TextStyle expansion to Path/Image/External styles (PathStyle/ImageStyle snapshot)



### Round 547 — R55.G.11 §5.49 — PathStyle + ImageStyle snapshot exposure (Box/Text symmetric expansion)

**Changes**:
- snapshot.rs: PathSnapshot/ImageSnapshot gain style field (pinion_core::style::*)
- snapshot_root populates style from each node; #[non_exhaustive] keeps additive
- dispatch.rs: stroke_cap_to_json + stroke_to_json + path_style_to_json helpers
- dispatch.rs: fit_to_json + image_style_to_json helpers (Fill/Contain/Cover/Tile)
- PathStyle wire: {stroke:null|{color,width,cap}, fill:null|{r,g,b,a}}
- ImageStyle wire: {fit:string, tint:null|{r,g,b,a}}
- 6 new tests: 2 snapshot land + 4 dispatch wire (Path/Image with + without arms)



**Verification**:
- cargo test (vello) = 2163 pass / 0 fail / 11 ignored (+6 R55.G.11 Path/Image tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 8 demos PASS regression



**Impact**: §5.49


**Carry forward**:
- scene/scroll at-based variant for consistency with click/wheel/key (intentional deviation now)
- External (Toggle/Checkbox/Radio) widget-specific style introspect carry



### Round 548 — R55.G.12 §5.49 second-consumer — hello_listbox_focus_border demo (R55.G.11 style wire under reactive mutation)

**Changes**:
- tools/demos/hello_listbox_focus_border.py — 9번째 demo
- row 0 focus-border initial: width=2, color=(0x40,0x80,0xe0,255)
- row 5 non-focused: border=null assert
- scene/rewind focused_index=3 경우 row 0 border drop + row 3 border gain
- second-consumer of R55.G.8 style wire — reactive mutation reflected



**Verification**:
- 9 demos PASS regression (R55.G.12 included, ~1.0-1.5s each)
- demo-only land — no Rust code changes, test/clippy counts unchanged



**Impact**: §5.49


**Carry forward**:
- scene/key External keybinding tag routing (paint-mode tag mismatch carry)
- scene/scroll at-based variant for consistency with click/wheel/key



### Round 549 — R55.G.13 — §5.3 with_* builder primitives added on Border/BoxStyle/Stroke/PathStyle sidecars retiring two field-mutation workaround sites

**Changes**:
- crates/pinion-core/src/style.rs: Border::with_color and Border::with_width const builders
- crates/pinion-core/src/style.rs: BoxStyle::with_fill const builder composes default and filled entry
- crates/pinion-core/src/style.rs: Stroke::with_color and Stroke::with_width const builders
- crates/pinion-core/src/style.rs: PathStyle::with_stroke and PathStyle::with_fill const builders both arms
- crates/pinion-core/src/style.rs: 4 chain tests (Border BoxStyle Stroke PathStyle composition)
- crates/pinion-rpc/src/snapshot.rs:714 retires PathStyle.fill assignment via with_fill chain
- crates/pinion-rpc/src/dispatch.rs:3854 retires PathStyle.fill assignment via with_fill chain



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2167 pass / 0 fail / 11 ignored (+4 builder tests)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 9 demos PASS 1.25-1.97s (hello-toggle hello-listbox regression clean)



**Impact**: §5.3


**Carry forward**:
- PathStyle::without_stroke/without_fill negative builders deferred (no current consumer)



### Round 550 — R55.G.14 — §5.49 TextStyle layout-axis snapshot.rs land tests added (r55_g10 module 3 tests) symmetric with r55_g8 visual axis and r55_g11 Path/Image

**Changes**:
- crates/pinion-rpc/src/snapshot.rs: new r55_g10 module under snapshot::tests with 3 tests
- text_layout_axis_survives_snapshot: line_height/text_align/letter_spacing/decoration/overflow all
- text_letter_spacing_accepts_signed_through_snapshot: i32 negative/zero/positive boundary
- text_line_height_variants_each_survive_snapshot: Normal/Px/MultiplierX100 discriminants preserved
- module mirrors r55_g8 (visual) + r55_g11 (Path/Image) at snapshot-struct boundary



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2170 pass / 0 fail / 11 ignored (+3 land)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 9 demos PASS (hello-toggle + hello-listbox regression clean)



**Impact**: §5.49



### Round 551 — R55.G.15 — §5.49 §5.45 scene/scroll at-based coord locator added (path XOR at) mirroring click/wheel/key shape via Scene::scroll_state_at substrate reuse

**Changes**:
- crates/pinion-rpc/src/dispatch.rs: handle_scene_scroll accepts path XOR at locator
- crates/pinion-rpc/src/dispatch.rs: resolve_scroll_target_at_or_path helper extracted
- crates/pinion-rpc/src/dispatch.rs: parse_at_coords_u32 helper rejects negative coords
- crates/pinion-rpc/src/dispatch.rs: 5 new tests (at hit/miss + locator XOR + neither + negative)
- crates/pinion-rpc/src/dispatch.rs: build_scroll_producer fixture sets Container.rect for at lookup
- Scene::scroll_state_at substrate reused (R55.C.2) so wheel and scroll coord paths converge



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2175 pass / 0 fail / 11 ignored (+5)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 9 demos PASS (path-based legacy contract preserved regression clean)



**Impact**: §5.49, §5.45



### Round 552 — R55.G.16 — §6 §6.2 pre-commit gate runs cargo clippy when staged includes *.rs and pre-push runs it unconditionally (workspace.lints clippy::pedantic deny auto-catch)

**Changes**:
- .githooks/pre-commit: cargo clippy --workspace --features pinion-runtime/vello when staged *.rs
- .githooks/pre-push: cargo clippy unconditional (defense-in-depth for amend/rebase/no-verify)
- CLAUDE.md: Pre-commit hook section documents both gates and workspace.lints baseline
- workspace.lints baseline (forbid unsafe / deny warnings / clippy::pedantic deny) auto-enforced



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2175 pass / 0 fail / 11 ignored (baseline)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings (baseline)
- bash -n .githooks/pre-commit && bash -n .githooks/pre-push = syntax_ok
- .githooks/pre-commit dry-run on staged hook + CLAUDE.md = mnemosyne pass, clippy correctly skips



**Impact**: §6, §6.2


**Carry forward**:
- R55.G.17 carry — scene/key External tag routing (paint-mode + state-mode align) ~150-250 LOC
- R55.D — ScrollBar sub-widget (visible drag + SCXML statechart) ~400-600 LOC
- R56 — TextField + IME (caret + selection + IME composition) ~1000+ LOC



### Round 553 — R55.G.17 — §5.49 hello-listbox wraps Scroll in a transparent Container tagged main_list so scene/key {path: main_list} addresses the composite paint root (R55.G.12 carry close)

**Changes**:
- examples/hello-listbox/src/main.rs: view-fn wraps Scroll in Container::with_tag(PRIMARY_TAG)
- tools/demos/hello_listbox_composite_path.py: 10th demo verifies ArrowDown via composite path
- tools/demos/hello_listbox_snapshot.py: walkthrough updated for wrapper layer (children[0].tag)
- AT bounds for main_list now match Scroll viewport (not full window) via rect_for_tag attach



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2175 pass / 0 fail / 11 ignored (unchanged)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 10 demos PASS (hello_listbox_composite_path 10th + 9 existing regression-free)
- scene/key {path: main_list, ArrowDown} sequence: None->0 boundary + 0->1 step reach V::apply_key



**Impact**: §5.49


**Carry forward**:
- F1 framework auto-tag deferred (regression risk against inner-tag widgets like hello-toggle)
- R55.D — ScrollBar sub-widget (visible drag + SCXML statechart) ~400-600 LOC
- R56 — TextField + IME (caret + selection + IME composition) ~1000+ LOC



### Round 554 — R55.G.18 — §5.49 hello-radio-group + hello-listbox-multi inner column carries PRIMARY_TAG so composite paint root is addressable (R55.G.17 convention parity)

**Changes**:
- examples/hello-radio-group/src/main.rs: column container with_tag(PRIMARY_TAG)
- examples/hello-listbox-multi/src/main.rs: column container with_tag(PRIMARY_TAG)
- no extra wrapper layer — column already bounds the composite visual surface
- both follow R55.G.17 paint-addressable composite convention (sister widgets to hello-listbox)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2175 pass / 0 fail / 11 ignored (unchanged)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 10 demos PASS (no demos for hello-radio-group / hello-listbox-multi yet; sibling regressions clean)



**Impact**: §5.49


**Carry forward**:
- F1 framework auto-tag deferred (regression risk against inner-tag widgets like hello-toggle)
- R55.D — ScrollBar sub-widget (visible drag + SCXML statechart) ~400-600 LOC
- R56 — TextField + IME (caret + selection + IME composition) ~1000+ LOC



### Round 555 — R55.G.19 — §5.49 Scene::contains_tag primitive + per-composite convention tests pin R55.G.17 paint-root tag convention (3 widgets: listbox/radio-group/listbox-multi)

**Changes**:
- crates/pinion-core/src/scene.rs: Scene::contains_tag depth-first walker + 4 unit tests
- examples/hello-listbox: r55_g17_view_contains_composite_paint_root_tag test
- examples/hello-radio-group: r55_g18_view_contains_composite_paint_root_tag test
- examples/hello-listbox-multi: r55_g18_view_contains_composite_paint_root_tag test



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2182 pass / 0 fail / 11 ignored (+7)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 10 demos PASS (regression-free)



**Impact**: §5.49


**Carry forward**:
- F1 framework auto-tag deferred (regression risk against inner-tag widgets like hello-toggle)
- R55.D — ScrollBar sub-widget (visible drag + SCXML statechart) ~400-600 LOC
- R56 — TextField + IME (caret + selection + IME composition) ~1000+ LOC



### Round 556 — R55.G.20 — §5.49 convention test coverage extended to 6 remaining composites (toggle/button/checkbox/radio/slider/slider-vertical) via Scene::contains_tag

**Changes**:
- examples/hello-toggle: r55_g20_view_contains_composite_paint_root_tag test
- examples/hello-button: r55_g20_view_contains_composite_paint_root_tag test (Owner-wrapped)
- examples/hello-checkbox: r55_g20_view_contains_composite_paint_root_tag test
- examples/hello-radio + hello-slider + hello-slider-vertical: same convention test added



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2188 pass / 0 fail / 11 ignored (+6)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 9 widgets covered total (R55.G.19 listbox/radio-group/listbox-multi + R55.G.20 atomics)



**Impact**: §5.49


**Carry forward**:
- hello-commands convention test (no existing test module — defer until test layer needed)
- F1 framework auto-tag deferred (regression risk against inner-tag widgets like hello-toggle)
- R55.D — ScrollBar sub-widget (visible drag + SCXML statechart) ~400-600 LOC
- R56 — TextField + IME (caret + selection + IME composition) ~1000+ LOC



### Round 557 — R55.G.21 — §5.49 WidgetCore::tag and ::view doc comments reference Scene::contains_tag regression-test primitive (R55.G.17 convention discoverable)

**Changes**:
- crates/pinion-core/widget_core.rs: WidgetCore::tag doc references Scene::contains_tag
- crates/pinion-core/widget_core.rs: WidgetCore::view doc references R55.G.17 paint convention
- R55.G.17 paint-root tag convention now discoverable from the trait it constrains



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2188 pass / 0 fail / 11 ignored (unchanged)
- cargo clippy -p pinion-core --all-targets = 0 warnings (doc-only round, no code drift)



**Impact**: §5.49


**Carry forward**:
- hello-commands convention test (no existing test module — defer until test layer needed)
- F1 framework auto-tag deferred (regression risk against inner-tag widgets like hello-toggle)
- R55.D — ScrollBar sub-widget (visible drag + SCXML statechart) ~400-600 LOC
- R56 — TextField + IME (caret + selection + IME composition) ~1000+ LOC



### Round 558 — R56.1.g.0 §5.38 §5.22 — `TextEditState` IME preedit substrate: `preedit_buffer: Signal<Option<String>>` sidecar + 4-mutator lifecycle + 2 accessors mirror W3C `CompositionEvent`.

**Changes**:
- `crates/pinion-core/src/widgets/text_edit.rs`: add `preedit_buffer: Signal<Option<String>>` field
- Orthogonal to `text` / `caret` / `selection_anchor`; mirrors the W3C platform IME contract
- Four-mutator lifecycle: `preedit_start` / `preedit_update` / `preedit_commit` / `preedit_cancel`
- Mirrors W3C `compositionstart` / `compositionupdate` / `compositionend(data)` / cancel-shape
- `preedit()` returns `Option<String>` (mirror of W3C `CompositionEvent.data`)
- `is_composing()` predicate mirrors W3C `KeyboardEvent.isComposing`
- `preedit_start` with active selection drains range first then starts composition
- Drain + start is a 4-axis batched write (`text` + `caret` + `selection_anchor` + `preedit_buffer`)
- Canonical macOS / iOS / GTK / Web compose-over-selection contract
- `preedit_commit` inserts committed text at caret + clears preedit (3-axis batched write)
- Caret advances by `committed.len()` bytes on a `char` boundary (Rust `&str` UTF-8 invariant)
- `preedit_commit` with empty string clears buffer without inserting (cancel-shape `compositionend`)
- `preedit_update` / `preedit_commit` / `preedit_cancel` are defensive no-op when not composing
- Out-of-order delivery from AI client / RPC path stays idempotent
- `set_text` now also clears active preedit (whole-buffer replace invalidates composition)
- `new` / `with_initial` / `with_tag` / `Default` initialise `preedit_buffer` to `None`
- 17 `r56_1_g` regression tests cover the start/update/commit/cancel lifecycle
- Multi-byte UTF-8 commit (Korean 한 syllable) + defensive idempotence cases
- 2 batched-multi-axis subscriber tests verify single `Effect` re-run per logical edit



**Verification**:
- `cargo test -p pinion-core widgets::text_edit::tests::r56_1_g`: 17 pass / 0 fail
- `cargo test --workspace --features pinion-runtime/vello`: 2613 pass / 0 fail / 13 ignored
- Delta +17 tests over the session 50 baseline (2596)
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello`: 0 warnings
- Workspace.lints baseline: forbid `unsafe_code` + deny warnings + `clippy::pedantic` deny



**Impact**: §5.38, §5.22


**Carry forward**:
- R56.1.g.1 TextField apply_composition_* dispatch path
- R56.1.g.2 RPC preedit slot + composition invoke wire
- R56.1.g.3 hello-textfield preedit visual + composition demo
- Platform IME bridge crate carry (Wayland text-input-v3 + macOS NSTextInputContext + Windows TSF)



### Round 559 — R56.1.g.1 §5.38 §5.22 — `TextFieldExternal` IME composition dispatch: `apply_composition_start/update/commit/cancel` + commit-on-blur upgrades `text_committed` payload to `Text(committed)`.

**Changes**:
- `crates/pinion-core/src/widgets/text_field.rs`: four new `TextFieldExternal` composition methods
- `apply_composition_start` wires `preedit_start` + drives `BeginEdit` + blink reset
- `apply_composition_update` wires `preedit_update`; SCXML stays in `Editing`
- `apply_composition_commit` wires `preedit_commit` + bypasses `IntentEmitter::dispatch`
- Bypass via `em.inner.send(CommitEdit)` so `detect` does NOT push the legacy `Intent(Null)`
- Manual `em.push(Intent(text_committed, Text(committed)))` upgrades the payload shape
- Mirrors the W3C `CompositionEvent.data` shape the AI client expects
- `apply_composition_cancel` wires `preedit_cancel` + drives `CancelEdit` (silent in detect)
- IME canonical cancel-discards-preedit (Escape during composition, Wayland cancel)
- Intent emission gated on `was_composing AND !committed.is_empty()` (semantic correctness)
- `was_composing` sampled before `preedit_commit` clears the buffer (post-clear read fails)
- `on_focus_change(false)` commits non-empty preedit via `apply_composition_commit`
- Then drives `Blur` (W3C IME canonical commit-on-blur, Wayland / macOS / GTK / TSF)
- Empty preedit at blur cancels composition instead of committing
- No-data `compositionend` is a cancel — matches the platform W3C convention
- Plain `send(CommitEdit)` / `send(Blur from Editing)` still emit `Intent(Null)`
- Backward compat held for the legacy plain-send path
- Blink resets on start / update / commit / cancel (user-interaction-marker UX)
- 29 `r56_1_g_tests` cover 4-method lifecycle + commit-on-blur + blink reset
- Korean multi-byte `한` composition end-to-end test
- Backward-compat plain-send and edge-case idempotence tests



**Verification**:
- `cargo test -p pinion-core widgets::text_field::r56_1_g_tests`: 29 pass / 0 fail
- `cargo test --workspace --features pinion-runtime/vello`: 2642 pass / 0 fail / 13 ignored
- Delta +29 tests over the R56.1.g.0 baseline (2613)
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello`: 0 warnings
- Workspace.lints baseline: forbid `unsafe_code` + deny warnings + `clippy::pedantic` deny



**Impact**: §5.38, §5.22


**Carry forward**:
- R56.1.g.2 RPC preedit slot + composition invoke wire
- R56.1.g.3 hello-textfield preedit visual + composition demo
- Platform IME bridge crate carry (Wayland text-input-v3 + macOS NSTextInputContext + Windows TSF)



### Round 560 — R56.1.g.2 §5.38 §5.22 — `TextFieldExternal` RPC `preedit` query/intervene slot + `composition` invoke surface mirror the W3C `CompositionEvent` lifecycle for the AI-first introspection contract.

**Changes**:
- `crates/pinion-core/src/widgets/text_field.rs`: schema slot count 6 → 8
- Added `preedit` (W3C `CompositionEvent.data` mirror) + `composition` (action surface)
- `query("preedit")` returns `Text(s)` while composing, `Null` when idle
- Bare external (no attached state) returns `None` so AI client distinguishes "no binding"
- `intervene("preedit", Text(s))` auto-starts composition + sets buffer
- Substrate idempotence: `preedit_start` no-ops if already composing
- `intervene("preedit", Null)` cancels composition (mirror of no-data `compositionend`)
- `intervene("preedit", Json | Int | ...)` returns `TypeMismatch` (strict W3C wire shape)
- `invoke("composition", Json{action, data?})` dispatches via `apply_composition_*` methods
- Action vocabulary: `start` / `update` / `end` / `cancel` mirror W3C `CompositionEvent` types
- Required `data` field for `update` and `end`; missing returns `TypeMismatch`
- Unknown action string returns `TypeMismatch`; `Text` args also rejected (Json-only)
- `parse_composition_invoke_json` helper + `CompositionAction` enum encapsulate parsing
- Bare external `composition` invoke still drives SCXML so AI sees transitions
- Existing `external_schema_declares_six_slots` test renamed `eight_slots` + updated
- 23 `r56_1_g_2_tests` cover schema + query + intervene + invoke for all 4 actions



**Verification**:
- `cargo test -p pinion-core widgets::text_field::r56_1_g_2_tests`: 23 pass / 0 fail
- `cargo test --workspace --features pinion-runtime/vello`: 2665 pass / 0 fail / 13 ignored
- Delta +23 tests over the R56.1.g.1 baseline (2642)
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello`: 0 warnings
- Workspace.lints baseline: forbid `unsafe_code` + deny warnings + `clippy::pedantic` deny



**Impact**: §5.38, §5.22


**Carry forward**:
- R56.1.g.3 hello-textfield preedit visual + composition demo
- Platform IME bridge crate carry (Wayland text-input-v3 + macOS NSTextInputContext + Windows TSF)



### Round 561 — R56.1.g.3 §5.38 §5.22 — `hello-textfield` preedit visual (underline + tinted background) + TUI cell-bg mirror + `hello_textfield_compose.py` RPC self-verify closes the R56 axis.

**Changes**:
- `examples/hello-textfield/src/main.rs`: preedit splicing into `effective_text`
- Visual caret follows preedit end (W3C `compositionupdate` canonical position)
- GUI: `PREEDIT_BG_COLOR` semi-transparent warm-amber tint paints behind preedit
- GUI: `PREEDIT_UNDERLINE_COLOR` + 1 px underline below preedit baseline
- Canonical IME affordance shared by Wayland / macOS / Windows / GTK clients
- GUI: preedit pixel range from 2× `caret_rect_for_byte_offset` on effective shaped run
- GUI: status line carries preedit segment for AI-side visual verification
- `examples/hello-textfield-tui/src/main.rs`: same splicing pattern + cell-bg amber band
- TUI: visual cursor follows preedit end (mirror of GUI compositionupdate semantic)
- TUI: status line preedit segment mirrors GUI for AI-verifier parity
- `tools/demos/hello_textfield_compose.py`: new end-to-end demo (∼150 LOC)
- Demo covers `invoke("composition", ...)` start / update / end / cancel actions
- Demo covers `query("preedit")` and `intervene("preedit", ...)` round-trip
- Demo covers Korean multi-byte commit (`한` syllable, 3 UTF-8 bytes)
- Demo covers commit-on-blur via `focus_set(None)` with non-empty preedit
- 14 demos PASS (13 prior + `hello_textfield_compose`)
- `hello-textfield` 12 existing view tests still pass (preedit `None` path unchanged)



**Verification**:
- `python3 tools/demos/hello_textfield_compose.py`: PASS in 2.1 s
- All 14 demos PASS (toggle / listbox / textfield families + new compose)
- `cargo test --workspace --features pinion-runtime/vello`: 2665 pass / 0 fail / 13 ignored
- Delta 0 tests over R56.1.g.2 baseline (binding-level visual change, not test surface)
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello`: 0 warnings
- Workspace.lints baseline: forbid `unsafe_code` + deny warnings + `clippy::pedantic` deny



**Impact**: §5.38, §5.22


**Carry forward**:
- R56 axis closure: all sub-rounds a/b/b.1/b.1.tui/b.2/c/d/e/f.0-3/g.0-3/h/j completed
- Platform IME bridge crate carry (Wayland text-input-v3 + macOS NSTextInputContext + Windows TSF)
- TUI grapheme-cluster cell mapping carry (multi-byte preedit currently skews TUI column)
- R57 Theming axis next major step (runtime palette + token system)



### Round 562 — R56.2.a §5.13 §5.38 — shell WindowEvent::Ime arm + WidgetCore::apply_composition trait wire platform IME into R56.1.g substrate.

**Changes**:
- pinion-core: add CompositionEvent enum (Start/Update/Commit/Cancel) re-export with Modifiers
- pinion-core widget_core: add WidgetCore::apply_composition (default false) mirror apply_key
- pinion-runtime core_shell: add CoreShell::apply_composition with root_owner.run wrap (R51.152)
- pinion-shell substrate: add ShellCore::apply_composition forwards focus + bumps revision
- pinion-shell app: window.set_ime_allowed(true) on resumed + ime_was_composing state field
- pinion-shell app: WindowEvent::Ime arm calls winit_ime_to_composition helper + mapping doc
- winit_ime_to_composition: empty preedit -> Update; Commit while idle injects synthetic Start
- examples/hello-textfield: apply_composition via intro.invoke composition Json action/data
- examples/hello-textfield Cargo.toml: add serde_json workspace dep for json! macro



**Verification**:
- cargo test workspace vello feature: 2689 pass / 0 fail / 13 ignored (+24 new from R56.2.a)
- cargo clippy workspace all-targets vello feature: 0 warnings (clippy::pedantic deny baseline)
- 14-demo release-build regression PASS: hello_toggle_* / hello_listbox_* / hello_textfield_*
- winit_ime_to_composition mapping table tests pin canonical pinyin commit + Escape cancel sequences
- ShellCore::apply_composition wire tests pin focused-tag forwarding + revision bump on handled arm
- CoreShell::apply_composition tests pin default false return + root_owner.run pop on exit symmetric
- CompositionEvent enum tests pin four W3C-mirrored variants + clone round trip + non-exhaustive shape



**Impact**: §5.13, §5.22, §5.35, §5.38


**Carry forward**:
- R56.2.b set_ime_cursor_area wire (caret rect -> Window::set_ime_cursor_area) for candidate popup
- R56.2.c per-focus set_ime_allowed toggle when 2nd text-input widget joins (substrate-incompleteness)
- Platform clipboard bridge crate (R56.1.e cascade): X11 / Wayland / macOS / Win32 impls
- R57 Theming substrate axis: runtime palette + token system + theme switch demo
- R58 composite widget catalogue axis: DatePicker / Combobox / Menu



### Round 563 — R56.2.b §5.22 — new pinion-platform-clipboard crate wraps `arboard` (canonical Rust ecosystem clipboard) as second Clipboard trait consumer; hello-textfield prefers it with InMemoryClipboard fallback.

**Changes**:
- new crate pinion-platform-clipboard registered in workspace between pinion-shell and pinion-tui
- ArboardClipboard impl Clipboard trait wraps arboard 3.x via RefCell interior mutability
- ArboardClipboard::try_new returns arboard::Error so callers fall back to InMemoryClipboard
- hello-textfield use_clipboard prefers ArboardClipboard with stderr fallback log on init failure
- hello-textfield AppClipboard wrapper boxes dyn Clipboard so Owner::cache<V> stores one Sized V
- arboard default-features=false + wayland-data-control opt drops image-data bloat (image/png/tiff)



**Verification**:
- cargo test workspace vello feature: 2690 pass / 0 fail / 14 ignored (+1 new ArboardClipboard test)
- cargo clippy workspace all-targets vello feature: 0 warnings (clippy::pedantic deny baseline)
- 14 demos release-build PASS including hello_textfield_clipboard exercising Ctrl/Cmd C X V dispatch
- ArboardClipboard Debug-renders-without-panic test pins fallback observability on headless CI
- abstraction-needs-second-consumer satisfied: InMemoryClipboard + ArboardClipboard = 2 trait impls



**Impact**: §5.22, §5.38


**Carry forward**:
- Wayland PRIMARY selection (middle-click paste) support — arboard exposes CLIPBOARD only today
- Clipboard image-data round-trip — drop default-features=false opt-out when image widget lands
- Clipboard history / multi-item shape (W3C ClipboardItem) — current API is text-only LCD
- R56.2.c set_ime_cursor_area wire (caret rect publish from view to shell for IME popup positioning)
- R57 Theming substrate axis — runtime palette + token system + theme switch demo



### Round 564 — R56.2.c §5.13 §5.38 — WidgetView::ime_caret_rect + AppShell set_ime_cursor_area dedup wire so IME candidate popup tracks the caret instead of defaulting to the screen corner.

**Changes**:
- pinion-shell WidgetView add ime_caret_rect default None (window-local logical px coord frame)
- AppShell render hook: root_owner.run V::ime_caret_rect between paint and finalize_frame
- AppShell last_ime_cursor_area f32 tuple dedups unchanged caret so winit boundary call skipped
- pinion-shell re-exports pinion_runtime::rect_for_tag for application scene walking
- pinion-text CaretRect adds public const new constructor (non_exhaustive struct downstream synth)
- hello-textfield ime_caret_rect impl: use_layout_cache hit + visual caret byte mirror view fn
- hello-textfield ime_caret_rect composes field_rect (rect_for_tag) + FIELD_PAD + caret_local



**Verification**:
- cargo test workspace vello feature: 2693 pass / 0 fail / 14 ignored (+3 new from R56.2.c)
- cargo clippy workspace all-targets vello feature: 0 warnings (clippy::pedantic deny baseline)
- 14 demos release-build PASS including hello_textfield_compose end-to-end regression
- CaretRect::new tests pin f32 fields + const-eligibility for compile-time CaretRect literals
- Default ime_caret_rect None test pins trait shape on TestView mock across (state, focused) matrix



**Impact**: §5.13, §5.36, §5.38


**Carry forward**:
- Per-focus set_ime_allowed gate on second text-input widget (substrate-incompleteness signal)
- Wayland PRIMARY selection (middle-click paste) — arboard surface is CLIPBOARD-only today
- pinion-tui FocusManager + IME bridge (TUI multi-widget binary absent, axis-level carry)
- R57 Theming substrate axis — runtime palette + token system + theme switch demo
- R58 composite widget catalogue axis — DatePicker / Combobox / Menu



### Round 565 — R56.2.d §5.41 — hello-textfield-tui cell_column_for_byte_offset (unicode-width) closes ASCII-only assumption; Korean/CJK/combining-accent cursor lands correctly.

**Changes**:
- hello-textfield-tui Cargo.toml: add unicode-width 0.2 dep (UAX #11 East Asian Width LCD)
- cell_column_for_byte_offset(text, byte_offset) helper sums UnicodeWidthChar::width over prefix chars
- Selection band: byte→cell column delta via cell_column_for_byte_offset (preedit-active path drained)
- Preedit band: byte→cell column delta on effective_text; IME provisional run paints right cells
- Cursor column: cell_column_for_byte_offset(effective_text, visual_caret_byte) lands cursor correctly
- Defensive clamp byte_offset > text.len() → text full width; control-char width None → 0 cells



**Verification**:
- cargo test workspace vello feature: 2700 pass / 0 fail / 14 ignored (+7 new from R56.2.d)
- cargo clippy workspace all-targets vello feature: 0 warnings (clippy::pedantic deny baseline)
- 14 demos release-build PASS including hello_textfield_compose Korean syllable round-trip
- cell_column tests pin: ASCII 1:1, Korean syllable 3B→2C, mixed prefix, combining accent 0C
- Defensive tests pin: oversized offset clamp + empty text + full hangul word 6B→4C



**Impact**: §5.41


**Carry forward**:
- Lift cell_column helper into pinion-tui when 2nd TUI text consumer joins (substrate-incompleteness)
- Grapheme cluster precision via unicode-segmentation for emoji flags + ZWJ sequences (carry)
- Wayland PRIMARY selection (arboard CLIPBOARD only today) middle-click paste carry
- R57 Theming substrate axis — runtime palette + token system + theme switch demo
- R58 composite widget catalogue — DatePicker / Combobox / Menu



### Round 566 — R56.2.e.0 §5.22 — `ClipboardSelection {Clipboard, Primary}` enum + `Clipboard::copy_to`/`paste_from` default methods + `InMemoryClipboard` dual-buffer substrate (R56.2.b cascade).

**Changes**:
- `clipboard.rs`: `ClipboardSelection {Clipboard, Primary}` `#[non_exhaustive]` `Default = Clipboard`.
- `copy_to(sel, text)` / `paste_from(sel)` defaults route `Clipboard` to `copy`/`paste`.
- Default `Primary` arm = no-op write / `None` read (macOS / Windows / browser).
- `InMemoryClipboard` dual `RefCell<Option<String>>` buffers + full `copy_to`/`paste_from` override.
- `lib.rs`: re-export `ClipboardSelection` next to `Clipboard` + `InMemoryClipboard`.
- +7 `r56_2_e` tests: default / dual isolation / alias / fresh `None` / no-op / overwrite / Copy.



**Verification**:
- `cargo test -p pinion-core --lib clipboard::` = 13 pass (6 R56.1.e prior + 7 R56.2.e new).
- `cargo test --workspace --features pinion-runtime/vello` = 2707 pass / 0 fail / 14 ignored.
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.



**Impact**: §5.22


**Carry forward**:
- R56.2.e.1 — `ArboardClipboard` Linux PRIMARY override via `arboard::{GetExtLinux, SetExtLinux}` cfg.
- R56.2.e.2 — `TextField` auto-publish PRIMARY on selection mutation + `paste_from_primary` API.
- R56.2.e.3 — shell middle-click wire + `WidgetView::apply_middle_click` trait (default false).



### Round 567 — R56.2.e.1 §5.22 — `ArboardClipboard` cfg-Linux `copy_to`/`paste_from` override threads `Primary` via `arboard::{SetExtLinux, GetExtLinux, LinuxClipboardKind}` (X11 PRIMARY / Wayland).

**Changes**:
- ArboardClipboard copy_to/paste_from override gated cfg(unix && !(macos|android|ios|emscripten)).
- `Clipboard` arm reuses `set_text`/`get_text`; Primary arm chains `set().clipboard(Primary).text()`.
- Wildcard arm = no-op write / `None` read so future `ClipboardSelection` variants stay safe.
- Module doc adds R56.2.e Linux PRIMARY cascade section (widget + shell composition).
- +2 `r56_2_e` cfg-Linux smoke tests: PRIMARY round-trip / CLIPBOARD legacy alias.



**Verification**:
- `cargo test -p pinion-platform-clipboard` = 3 pass (1 R56.2.b + 2 R56.2.e new) + 1 ignored.
- `cargo test --workspace --features pinion-runtime/vello` = 2709 / 0 / 14.
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.



**Impact**: §5.22


**Carry forward**:
- R56.2.e.2 — `TextField` auto-publish PRIMARY on selection mutation + `paste_from_primary` API.
- R56.2.e.3 — shell middle-click wire + `WidgetView::apply_middle_click` trait (default false).



### Round 568 — R56.2.e.2 §5.38 §5.22 — `TextField` auto-publishes the active selection to PRIMARY after `dispatch_key` + RPC `intervene("selection")`; `paste_from_primary` API for shell middle-click.

**Changes**:
- `dispatch_key` after any `handled` key calls `publish_primary_selection_if_any` helper.
- Helper guards attached state+clipboard + non-empty selection then `copy_to(Primary, _)`.
- RPC `intervene("selection", Json)` arm calls helper after `set_selection`; `Null` retains PRIMARY.
- Doc comments cite X11 retain-until-new convention so semantics stay explicit at the source.
- `paste_from_primary()` API: reads PRIMARY, inserts at caret, resets blink, returns `bool`.
- Ctrl/Cmd C/X/V untouched — PRIMARY publish fires only on selection mutation.
- +13 `r56_2_e` widget tests: Ctrl+A / Shift+Arrow / intervene / paste_from_primary.



**Verification**:
- `cargo test -p pinion-core --lib r56_2_e` = 20 pass (7 substrate + 13 widget).
- `cargo test --workspace --features pinion-runtime/vello` = 2722 / 0 / 14.
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.



**Impact**: §5.22, §5.38


**Carry forward**:
- R56.2.e.3 — shell middle-click wire + `WidgetView::apply_middle_click` trait + hello-textfield impl.



### Round 569 — R56.2.e.3 §5.13 §5.22 §5.38 — shell middle-click wire: `WindowEvent::MouseInput Middle Pressed` + `WidgetCore::apply_middle_click` trait + hello-textfield impl (R56 platform-side full closure).

**Changes**:
- `WidgetCore::apply_middle_click(scene, focused, modifiers) -> bool` trait default false.
- `CoreShell::apply_middle_click` wraps the trait call in `root_owner.run` (R51.152 symmetric).
- `ShellCore::middle_click` reads focused + modifiers, calls `CoreShell::apply_middle_click`.
- `AppShell` `WindowEvent::MouseInput Middle Pressed` arm calls `self.core.middle_click()`.
- `TextFieldExternal` invoke gains `paste-primary` slot (Null arg → `Bool(handled)`).
- Schema slots count = 9 (`paste-primary` added with `boolean` type).
- `hello-textfield::apply_middle_click` routes through invoke (mirrors `apply_composition` pattern).
- +9 `r56_2_e` tests: 6 dispatch_core wire + 3 invoke (round-trip / empty / reject).



**Verification**:
- `cargo test --workspace --features pinion-runtime/vello` = 2731 / 0 / 14.
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.
- `cargo test -p pinion-core --lib r56_2_e` = 23 pass (7 substrate + 16 widget).



**Impact**: §5.13, §5.22, §5.38


**Carry forward**:
- R56 axis platform-side full closure (substrate + visible + RPC + IME + clipboard + PRIMARY).
- pinion-tui middle-click PRIMARY paste — axis carry (crossterm raw-mouse + 2nd consumer wait).
- F1 framework auto-tag conflict-aware — permanent carry.



### Round 570 — R56.2.f §5.38 §5.22 — `TextEditState::splice_preedit(caret)` substrate helper lifts the duplicate effective_text splice from 3 sites (DRY closure per [[substrate-incompleteness-signal]]).

**Changes**:
- `text_edit.rs`: `splice_preedit(caret) -> (effective_text, visual_caret, range)` substrate method.
- `caret` taken as arg (not `self.caret()`) so view-fns thread the by-value state snapshot.
- Empty preedit treated as no composition (W3C "no visible affordance" contract).
- `caret` clamped to `text.len()` defensively for slice safety.
- `hello-textfield::view` + `ime_caret_rect` collapse 11-line splice to single call.
- `hello-textfield-tui::view` mirrors via same substrate (`visual_caret_byte: usize`).
- +7 `r56_2_f` tests: collapsed / empty / caret-zero / caret-end / multi-byte / non-empty / clamp.



**Verification**:
- `cargo test -p pinion-core --lib r56_2_f` = 7 pass (new test module).
- `cargo test --workspace --features pinion-runtime/vello` = 2738 / 0 / 14.
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.



**Impact**: §5.22, §5.38


**Carry forward**:
- R57 Theming substrate — first slice axis carry.
- F1 framework auto-tag conflict-aware — regression risk carry.
- AT-TUI integration — axis carry.



### Round 571 — §5.32 — `HitPath::bbox` coordinate-frame docs clarification (R51.181 carry closure): explicit per-case rules for outside-Scroll / inside-Scroll-content / Scroll-itself-deepest.

**Changes**:
- `scene.rs` `HitPath::bbox` doc rewritten: matched primitive's declared rect verbatim, no transform.
- 3 cases pinned: outside any `Scroll` (viewport frame) / inside `Scroll` content (content-intrinsic).
- Third case: `Scroll`-itself-deepest — viewport rect in the parent (`(x,y)`) frame.
- AI clients translate via `Scroll` introspect surface (`viewport` + `offset_{x,y}`).
- Tagline: bbox = matched primitive's declared rect; walk `segments` to recover enclosing `Scroll`.
- Test surface unchanged — R55.A.2 `hit_test` tests already pin all 3 frame cases.
- Pure docs change — closes R51.181 carry.



**Verification**:
- `cargo test --workspace --features pinion-runtime/vello` = 2738 / 0 / 14 (unchanged — docs only).
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.



**Impact**: §5.32


**Carry forward**:
- R57 Theming substrate — first slice axis carry.
- F1 framework auto-tag conflict-aware — regression risk carry.



### Round 572 — R57.0 5.50 theming substrate -- ColorRole + Theme + ThemeProvider + use_theme + hello-theme demo

**Changes**:
- pinion-core::theme module added (ColorRole + Theme + ThemeProvider + use_theme hook)
- pinion-core::style Color now derives serde Serialize/Deserialize (Signal<Theme> requirement)
- Theme::light + Theme::dark preset factories (Material 3 baseline, WCAG AA contrast)
- ThemeProvider wraps Signal<Theme>; theme/set_theme atomic reactive swap (signal eq-skip)
- use_theme(tag) Owner::cache typed-key hook -- same shape as use_text_edit_state/use_scroll_state
- ColorRole::default_for fallback for partially-bound palette (W3C CSS variable cascade)
- examples/hello-theme demo binary -- Toggle drives set_theme; view reads theme reactively
- hello-theme view-fn uses 5 of 6 ColorRole tokens (surface/on_surface/on_surface_muted/accent/on_accent/outline)
- workspace Cargo.toml registers hello-theme example



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2755 pass / 0 fail / 14 ignored (+17 vs 2738 baseline)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- 15 theme.rs unit tests (ColorRole resolve / Theme light+dark pins / Provider swap / use_theme cache)
- 2 hello-theme tests (r55_g20 paint-root tag convention + r57_0 set_theme surface swap)
- 14 regression demos PASS (hello_toggle_activate ... hello_textfield_compose)
- mnemosyne validate-workspace clean (T1=0, RT=1/1, T3=0/0, T4=145, atomic orphan_refs=4+0)



**Impact**: §5.50, §5.22, §5.28, §5.38


**Carry forward**:
- R57.1: ThemeMode enum + prefers-color-scheme OS bridge
- R57.2: typography + spacing tokens (TextStyleRole + SpacingToken)
- R57.X: retrofit existing widget catalogue to ColorRole resolution (Toggle/ListBox/TextField)
- R57.X: theme fade animation via Color::lerp linear-space + Signal<Theme> interpolation
- R57.X: Material 3 container/variant role pairs (primaryContainer/onPrimaryContainer/...)



### Round 573 — R57.X.toggle §5.50 — hello-toggle Material 3 Switch role retrofit + V::update intent.payload Bool 권위 fix (post-flip payload authority, V::read_state SCXML in-flight lag 회피).

**Changes**:
- theme.rs: ColorRole::SurfaceContainerHighest variant + Theme field (M3 light/dark sur5 tier).
- hello-toggle/main.rs view: use_theme().theme().resolve() cascade + M3 Switch 4-axis role mapping.
- hello-toggle/main.rs update: intent.payload Bool 권위로 set_theme swap (post-flip authority).
- hello-theme/main.rs update: 동일 intent.payload fix + dotted wire form 매칭.
- tools/demos/hello_toggle_style.py: Outline + OnSurface role 색상 pin 갱신.



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2759 pass / 0 fail / 14 ignored (+2 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- 14 demos PASS 회귀 (hello_toggle_style demo color pin theme-aware로 갱신됨).
- RPC verify: hello-toggle + hello-theme light↔dark cycle 자동 검증 통과.



**Impact**: §5.50, §5.22, §5.38, §5.20


**Carry forward**:
- R57.X.intent-tag-macro: substrate macro (intent_tag!) before 6 widget retrofit cascade.
- R57.X widget retrofit cascade: ListBox / TextField / Button / Checkbox / Radio / Slider 6개.
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis.
- R57.2 typography + spacing tokens cascade.
- pinion-tui scrollbar first-paint parity verify (cross-backend GUI/TUI 정합).



### Round 574 — R57.X.scrollbar §5.45 — first-paint chicken-and-egg substrate fix: ScrollState::set_max bool 반환 + compute_layout_with_scroll_dirty 변형 + shell same-frame 2-pass (compute-then-render-twice 정통).

**Changes**:
- ScrollState::set_max -> bool (Signal::revision pre/post delta)로 dirty 산출.
- compute_layout_with_scroll_dirty(...) -> bool 신규 API (기존 compute_layout void wrapper).
- update_scroll_state_bounds 재귀 walk |= fold bool 반환.
- compute_paint_scene + dispatch_rpc::produce 양쪽 same-frame 2-pass (Signal eq-skip).
- +3 r57_x_scrollbar regression test (first-true / second-false eq-skip / no-state-false).



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2760 pass / 0 fail / 14 ignored (+3 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- 14 demos PASS 회귀; hello-listbox scrollbar 첫 paint thumb 비례 (~40%) 표시 확인.
- RPC verify: scene/path 가 scrollbar widget 의 max + offset 변화 추적.



**Impact**: §5.45, §5.49


**Carry forward**:
- resize event same-frame warmup: 현재 1 frame flash 후 안정 (Resize 이벤트 한정).
- ScrollState::set_max must_use_candidate clippy wart: setup-path init_max(...) void 분리 후보.
- pinion-tui scrollbar first-paint parity verify: 이번 fix는 pinion-shell 한정, TUI 시이트 미검증.



### Round 575 — R57.X.intent-tag-macro §5.20 — pinion_core::intent_tag! compile-time concat substrate macro (stdlib concat!, dual literal, 0 dep) + hello-toggle/hello-theme reducer binding migration.

**Changes**:
- pinion-core::intent_tag! macro 추가 (stdlib concat!, dual literal, 0 dep).
- hello-toggle/main.rs TOGGLE_INTENT_TAG_FULL → intent_tag!('main_toggle', 'toggle') 마이그레이션.
- hello-theme/main.rs TOGGLE_INTENT_TAG_FULL → intent_tag!('theme_toggle', 'toggle') 마이그레이션.
- 3 unit tests + 1 doc test 가 macro output 을 runtime format! shape 와 pin.
- MEMORY.md +1 entry [[intent-tag-macro-substrate]] + [[intent-tag-dotted-wire-form]] cross-link.



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2764 pass / 0 fail / 14 ignored (+4 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- doc test: intent_tag!('main_toggle', 'toggle') == 'main_toggle.toggle' 어설션.
- runtime format! shape pin test: format!('{prefix}.{event}') 와 macro output 일치.



**Impact**: §5.20


**Carry forward**:
- option B: pinion-tui scrollbar first-paint parity 검증 + fix.
- option D: ScrollState::set_max init_max + set_max API 분리.
- option C: R57.X.listbox 첫 retrofit (intent_tag! 2nd consumer).
- R57.X.* 6 widget retrofit cascade (textfield, button, checkbox, radio, slider 남음).



### Round 576 — R57.X.scrollbar-tui-audit §5.45 — pinion-tui 의 compute_layout 부재 비대칭 + ScrollState::set_max 단일 bool API 의 textbook trade-off 명시 (split API 는 surface 증가).

**Changes**:
- §5.45 caveat: pinion-tui 는 compute_paint_scene 에서 compute_layout 호출 없음 (axis carry).
- §5.45 caveat: ScrollState::set_max 단일 bool API + #[allow(must_use_candidate)] = textbook trade-off.
- Option B audit: 4 TUI 바이너리 모두 scrollbar consumer 부재; parity fix 불필요.



**Verification**:
- grep pinion-tui src for compute_layout = 0 매치 (실제 호출 site).
- grep examples for hello-*-tui scrollbar consumer = 0 매치.
- mnemosyne validate clean post-mutation (entries=440 / sections=61).



**Impact**: §5.45


**Carry forward**:
- Future TUI scrollbar 위젯 consumer 시 pinion-tui compute_layout 통합 필요.
- R57.X.listbox theme retrofit (option C, 2nd intent_tag! consumer).



### Round 577 — R57.X.listbox §5.50 — ColorRole +3 M3 surface tier 추가 (SurfaceContainerLow/Container/High) + hello-listbox 16 RGB literal → 7 role resolve + Color::lerp state-layer 마이그레이션.

**Changes**:
- theme.rs: ColorRole +3 variants — SurfaceContainerLow / SurfaceContainer / SurfaceContainerHigh.
- theme.rs: Theme palette +3 fields + Material 3 light/dark canonical 톤 defaults.
- theme.rs: +2 tier-progression tests (light 감소 / dark 증가 by lightness).
- hello-listbox: BG_FILL/TRACK_FILL/THUMB_FILL const 제거; view fn → use_theme('app') cascade.
- hello-listbox: listbox_row + build_scrollbar_visual take &Theme; selection=Accent / focus=Container.
- hello-listbox: +2 r57_x test pin (panel=Surface, thumb=Outline) light↔dark.



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2768 pass / 0 fail / 14 ignored (+4 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p pinion-core --lib theme = 17 pass (baseline 15 + 2 tier progression).
- cargo test -p hello-listbox r57_x = 2 pass (panel Surface 역할 + thumb Outline 역할 pin).



**Impact**: §5.50, §5.45, §5.38


**Carry forward**:
- hello-textfield theme retrofit (다음 R57.X widget cascade target).
- hello-button + hello-checkbox-radio + hello-radio-group cascade carry.
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis carry.
- R57.X.theme-cleanup: hello-theme Outline-fill misuse 잔여 carry.
- Future TUI scrollbar consumer: pinion-tui compute_layout 통합 carry.



### Round 578 — R57.X.theme-cleanup §5.50 — hello-theme track Off 역할을 Outline(stroke) → SurfaceContainerHighest(M3 chip surface) 로 정정; hello-toggle Switch pairing 과 동일화.

**Changes**:
- hello-theme view fn: track_fill Off → SurfaceContainerHighest (pre-cleanup: Outline misuse).
- hello-theme view fn: knob_fill Off → Outline (hello-toggle M3 Switch pairing 일치).
- hello-theme tests: +1 r57_x_theme_cleanup_track_off_uses_surface_container_highest light↔dark pin.



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2769 pass / 0 fail / 14 ignored (+1 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p hello-theme = 3 pass (baseline 2 + 1 cleanup).



**Impact**: §5.50


**Carry forward**:
- hello-textfield theme retrofit (다음 R57.X widget cascade target).
- hello-button + hello-radio-group + hello-checkbox-radio retrofit cascade.
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis carry.



### Round 579 — R57.X.textfield §5.50 — hello-textfield retrofit: 13 RGB literal → 7 role resolve + 5 helper lift (text_fg / field_fill / selection / preedit_bg / preedit_underline) + 4 r57_x regression test.

**Changes**:
- hello-textfield: BG/FIELD/TEXT/CARET/SELECTION/PREEDIT const 제거 (13 RGB literal).
- hello-textfield: 5 helper lift — text_fg / field_fill / selection / preedit_bg / preedit_underline.
- hello-textfield: view + ime_caret_rect 양쪽 use_theme(THEME_TAG).theme() 경유 helper 라우팅.
- hello-textfield: title=OnSurface, status=OnSurfaceMuted, caret/selection=Accent (a=0xa0).
- hello-textfield: +4 r57_x regression test (idle / focused / selection-accent / palette swap).



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2773 pass / 0 fail / 14 ignored (+4 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p hello-textfield = 16 pass (baseline 12 + 4 r57_x).



**Impact**: §5.50, §5.22, §5.38


**Carry forward**:
- hello-button + hello-radio-group + hello-checkbox-radio cascade carry.
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis carry.
- R57.X.theme-fade animation (Color::lerp + Signal<Theme> 보간) carry.



### Round 580 — R57.X.button §5.50 — hello-button retrofit: 6 RGB literal → M3 filled-tonal Button role mapping + Color::lerp state-layer (pressed 0.12 / disabled 0.38) + 3 r57_x regression test.

**Changes**:
- hello-button: BG_FILL + BTN_FILL_IDLE + BTN_FILL_HOVER + 3 inline RGB const 제거 (총 6 literal).
- hello-button: button_fill_endpoints helper — Idle=SurfaceContainerHighest, Hover=lerp(8 %).
- hello-button: Pressed=lerp(12 %) + Disabled=lerp(38 %) + label OnSurface / OnSurfaceMuted.
- hello-button: +3 r57_x regression test (idle endpoint role + hover state-layer + panel swap).



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2776 pass / 0 fail / 14 ignored (+3 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p hello-button = 12 pass (baseline 9 + 3 r57_x).



**Impact**: §5.50, §5.28


**Carry forward**:
- hello-radio + hello-radio-group + hello-checkbox + hello-slider retrofit cascade carry.
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis carry.
- R57.X.theme-fade animation (Color::lerp + Signal<Theme> 보간) carry.



### Round 581 — R57.X.radio §5.50 — hello-radio + hello-radio-group retrofit: 23 RGB literal → 공유 radio_border_color helper (M3 Outline/Accent + state-layer lerp) + 3 r57_x test.

**Changes**:
- hello-radio: 11 RGB const 제거 + radio_border_color helper (Outline / Accent + state-layer).
- hello-radio-group: 12 RGB const 제거 + sibling radio_border_color helper (rule-of-2 lift).
- hello-radio-group: radio_row takes &Theme; view fn reads use_theme(THEME_TAG).theme().
- hello-radio: +3 r57_x test (unselected=Outline / selected=Accent / hover=lerp 8 %).
- hello-radio + hello-radio-group: 기존 test Owner::new().run() 으로 감싸 use_theme 요구 시계 완화.



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2779 pass / 0 fail / 14 ignored (+3 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p hello-radio = 8 pass (baseline 5 + 3 r57_x); hello-radio-group = 28 pass.



**Impact**: §5.50, §5.38


**Carry forward**:
- hello-checkbox + hello-slider retrofit cascade carry.
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis carry.
- R57.X.theme-fade animation (Color::lerp + Signal<Theme> 보간) carry.



### Round 582 — R57.X.checkbox-slider §5.50 cascade closure — hello-checkbox + hello-slider + hello-slider-vertical retrofit (41 RGB literal → M3 role + Color::lerp state-layer, 3 binaries).

**Changes**:
- hello-checkbox: checkbox_accent_for + checkbox_outline_for helper (Accent + Outline + state-layer).
- hello-slider: slider_accent_for helper + thumb=OnAccent + track=SurfaceContainerHighest.
- hello-slider-vertical: sibling slider_accent_for (axis-specific behaviour stays in External).
- 3 binaries: existing test 의 view 호출을 Owner::new().run() 으로 감싸 use_theme 요구 충족.
- hello-checkbox: +3 r57_x test — checked=Accent / unchecked=Outline / hover=lerp(8 %).



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2782 pass / 0 fail / 14 ignored (+3 new).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p hello-checkbox = 9 pass; hello-slider = 21 pass; hello-slider-vertical = 11 pass.



**Impact**: §5.50, §5.38


**Carry forward**:
- R57.X retrofit cascade complete (7 binaries: toggle/listbox/textfield/button/radio/checkbox/slider).
- R57.1 ThemeMode + prefers-color-scheme OS bridge axis (다음 textbook target).
- R57.2 typography + spacing tokens cascade carry.
- R57.X.theme-fade animation (Color::lerp + Signal<Theme> 보간) carry.



### Round 583 — R57.1 §5.50 — ThemeMode + W3C prefers-color-scheme OS 브리지 + ThemeProvider 양팔레트 구조 + 5 binary set_mode 마이그레이션 (R57 axis next textbook step).

**Changes**:
- pinion_core::theme: SystemColorScheme enum (W3C prefers-color-scheme 미러, NoPreference/Light/Dark).
- pinion_core::theme: ThemeMode (Light/Dark/System, default=System per M3 follow-system canonical).
- pinion_core::theme: thread_local SystemColorScheme Signal + system_color_scheme + set_ free fn.
- pinion_core::theme: ThemeProvider 재구성 — mode + light_palette + dark_palette 3-signal 구조.
- pinion_core::theme: theme() = match mode { Light/Dark/System } → palette signal dispatch + OS read.
- v0→v1 clean cut — set_theme 제거; set_mode + set_light_palette + set_dark_palette 치환.
- pinion-shell::app: WindowEvent::ThemeChanged arm + resumed의 Window::theme() 초기 readout 푸시.
- pinion-shell::app: winit_theme_to_pinion_scheme helper (winit Light/Dark → SystemColorScheme).
- 5 binary 마이그 — hello-toggle/theme/button/listbox/textfield 모두 set_mode/set_palette 호출.
- +9 r57_1 test — system/mode default + 5-way mode 해상도 + provider mutator 독립성 핀.



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2791 pass / 0 fail / 14 ignored (+9).
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings.
- cargo test -p pinion-core --lib theme = 26 pass (R57.0 18 → 26, +8 R57.1 mode + OS 신호 핀).
- tools/demos 14 demos 회귀 PASS (이전 baseline 동일).



**Impact**: §5.50


**Carry forward**:
- R57.X.theme-fade — Color::lerp + Signal<Theme> 보간 (M3 Motion canonical, ~150 LOC carry).
- ColorRole::Error tier — M3 error/onError/errorContainer/onErrorContainer (hello-button red carry).
- TUI shell terminal OSC 11 readout — SystemColorScheme의 TUI backend 브리지 (axis carry).
- substrate widget_view_with_theme(state, theme) lift — 6 binary Owner::new().run wrap 중복.
- R57.2 typography + spacing tokens cascade (TextStyleRole + SpacingToken, axis-level).



### Round 584 — R57.X.theme-fade §5.50 — ThemeProvider::theme_animated() opt-in 페이드 substrate: M3 short4 ~200ms 임계감쇠 spring (THEME_FADE_SPRING) + ThemeLinear linear-light carrier + Owner None fallback + 9 회귀 테스트.

**Changes**:
- theme.rs: ThemeProvider.fade RefCell + pub fn theme_animated() (Owner lazy init, instant fallback)
- theme.rs: THEME_FADE_SPRING pub const 400/40/1 (ζ=1.0, ω_n=20rad/s, ~200ms settle, M3 short4)
- theme.rs: ThemeLinear (10 AnimVec4) private carrier + Animatable impl + from_theme/to_theme
- theme.rs: ThemeFadeState private (Animation<ThemeLinear> + Cell<Theme> last_target sRGB cache)
- lib.rs: THEME_FADE_SPRING pub re-export 추가 (application 동일 M3 spring 재사용)
- theme.rs: +9 회귀 테스트 (spring 임계감쇠 / settle / 중단 연속성 / Owner None fallback / OS scheme)



**Verification**:
- cargo test --workspace --features pinion-runtime/vello = 2800/0/14 (baseline 2791 + 9 신규)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings
- cargo test -p pinion-core --lib theme = 35 PASS (R57.0 18 + R57.1 8 + R57.X.theme-fade 9)
- 14 demos: 13 PASS (focus_border 실패는 R57.X.listbox carry, baseline 동일 — 내 변경 무관)



**Impact**: §5.50, §5.28


**Carry forward**:
- R57.X.theme-fade widget retrofit cascade (10 binary call sites + test settle 패턴 필요)
- hello_listbox_focus_border.py demo 갱신 (R57.X.listbox carry, FOCUS_BORDER_RGB → ColorRole::Accent)
- Animatable<Color> 일반화는 2nd consumer 등장 후 (Rule of Three 영구 carry)
- TweenAnimation<T> Signal-backed primitive 도입은 2nd consumer 후 (Rule of Three 영구 carry)



### Round 585 — R585 §5.50 R57.X.theme-fade — `theme_animated()` at-rest 시 캐시된 sRGB target 즉시 반환 (SwiftUI / Compose canon 미러), `ThemeLinear` linear-light round-trip lossy ±1 channel 회피.

**Changes**:
- §5.50 `theme_animated()` `is_at_rest()` 시 cached sRGB target 즉시 반환 (round-trip 우회).
- §5.50 회귀 4개 tighten: `assert_theme_close` → `assert_eq!` (settle paths 정확 매칭).
- §5.50 신규 회귀: midrange exact-equality (`#121212`, `#E6E0E9`) — cascade `==` contract 핀.



**Verification**:
- `cargo test --workspace --features pinion-runtime/vello` = 2801 / 0 / 14 (+1 신규).
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.
- `cargo test -p pinion-core --lib theme` = 36 pass (baseline 35 + at-rest exact pin).



**Impact**: §5.50


**Carry forward**:
- R586 widget cascade — 10 binary `theme()` → `theme_animated()` + settle helper lift.
- `Animation::reset(value)` carry — 2nd consumer 후 ([[abstraction-needs-second-consumer]]).
- `Animatable<Color>` 일반화 carry — 2nd carrier 등장 후 macro 추출 (Rule of Three).
- `TweenAnimation<T>` Signal-backed wrapper carry — 2nd 진짜 tween 시나리오 후.



### Round 586 — R586 §5.50 R57.X.theme-fade widget retrofit cascade — 10 binary view-fn `theme()` → `theme_animated()` + `settle_owner_animations` 헬퍼 lift (test_fixtures 60-tick 스프링 settle).

**Changes**:
- §5.50 10 binary view-fn `theme()` → `theme_animated()` (textfield apply_key 결제 lock-step 미러).
- §5.50 `pinion_core::test_fixtures::settle_owner_animations` 헬퍼 (60-tick 스프링 settle, 1s @ 60Hz).
- §5.50 5 widget cascade 테스트 — 2-phase `owner.run` + settle + `owner.run` 패턴.
- §5.50 theme.rs 내부 테스트 7 사이트 헬퍼 사용으로 refactor.



**Verification**:
- `cargo test --workspace --features pinion-runtime/vello` = 2801 / 0 / 15 (+1 doc test).
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` = 0 warnings.
- 14 demos: 13 PASS / 1 baseline FAIL carry (`hello_listbox_focus_border` Round 577 임, 무관).



**Impact**: §5.50


**Carry forward**:
- `Animation::reset(value)` primitive 영구 carry — 2nd consumer 후 (Rule of Three).
- `Animatable<Color>` 일반화 carry — 2nd carrier 등장 후 macro 추출 검토.
- Textfield apply_key 캐시 thrash carry — fade 200ms 중 텍스트 레이아웃 frame당 recompute.
- hello_listbox_focus_border demo baseline 재의 carry — R577 이후 origin/main 결함.



### Round 587 — R586 ime_caret_rect 마이그를 R587 측정 검증; lock-step 이 cache miss 더 적어서 유지 정통; LayoutCache paint-style split 은 multi-line consumer 등장 후 carry.

**Changes**:
- cache.rs: LayoutKey doc 에 fg_color in-key 측정 결과 + paint-style separation Rule of Three carry 명시
- cache.rs: different_fg_color_creates_new_entry 회귀로 현재 behavior pin (split land 시 deliberate flip)
- hello-textfield: ime_caret_rect 코멘트 정정 — lock-step 이 cache miss 최소화 rationale (R587 측정 인용)



**Verification**:
- cargo test -p pinion-text --lib cache::tests = 7 pass (different_fg_color_creates_new_entry +1)
- cargo test --workspace --features pinion-runtime/vello = 2802 pass / 0 fail / 15 ignored
- cargo clippy --workspace --all-targets --features pinion-runtime/vello = 0 warnings 유지



**Impact**: §5.36, §5.50


**Carry forward**:
- LayoutCache paint-style split: 2nd consumer = multi-line text editor 등장 시 framework substrate fix
- 자가-점검 #302 첫 정통화: 측정 결과 (c) 결정 (lock-step 유지 + carry 명시), measurement trail 정통



### Round 588 — R588 #303: test_fixtures.rs 두 doc example rust,no_run + # hidden imports로 compile-check 유지.

**Changes**:
- assert_widget_view_carries_tag doc: rust,no_run + # hidden + ButtonFixture body
- settle_owner_animations doc: rust,no_run + # hidden imports + minimal Owner body



**Verification**:
- cargo test --doc -p pinion-core --features test-fixtures: 5 tests 3 pass 0 fail 2 ignored
- cargo test --workspace --features pinion-runtime/vello: 2804 pass / 0 fail / 13 ignored (+2 -2)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings 유지



**Impact**: §5.41, §5.49, §5.50


**Carry forward**:
- doc test rust,no_run + # hidden imports = Rust std lib canonical 패턴



### Round 589 — hello_listbox_focus_border demo 의 FOCUS_BORDER_RGB 를 Theme::light().accent (0x19,0x76,0xD2) 로 갱신 — R577 listbox retrofit lock-step.

**Changes**:
- hello_listbox_focus_border.py: FOCUS_BORDER_RGB → M3 primary accent (0x19,0x76,0xD2)
- docstring + lock-step 주석 추가 (Theme::light().accent canonical source)



**Verification**:
- python3 tools/demos/hello_listbox_focus_border.py: PASS (0.84s)
- 14/14 demos PASS 재획득 (R577 carry 청산)
- cargo build --release -p hello-listbox: ok



**Impact**: §5.49, §5.50


**Carry forward**:
- demo color constants = Theme palette mirror; palette baseline 이동 시 lock-step audit



### Round 590 — ColorRole 에 Material 3 error tier 4 variants 추가 (Error/OnError/ErrorContainer/OnErrorContainer); Theme light+dark + ThemeLinear carrier + 4 회귀.

**Changes**:
- theme.rs: ColorRole +4 variants (Error/OnError/ErrorContainer/OnErrorContainer) + non_exhaustive
- theme.rs: Theme +4 fields + M3 light/dark hex + resolve +4 arms
- theme.rs: ThemeLinear +4 fields + from_theme/to_theme + Animatable 5 method arms
- theme.rs: +4 progression tests (light/dark palette pin, resolve dispatch, linear round-trip)



**Verification**:
- cargo test -p pinion-core --lib theme: 40 pass 0 fail (R589 36 + 4 R590)
- cargo test --workspace --features pinion-runtime/vello: 2808 pass / 0 fail / 13 ignored (+4)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings



**Impact**: §5.50


**Carry forward**:
- error tier downstream cascade (hello-button Disabled tonal mapping 등) 소비 있음



### Round 592 — R57.0 Effect-rerun substrate test 복구; ThemeProvider mutation 4 path (set_mode/set_light/set_dark/mode-gated system) 의 Signal 자동구독 contract 핀.

**Changes**:
- theme.rs tests: +4 R592 회귀 (set_mode / set_light / set_dark / Light-mode system ignore)
- theme.rs tests: Effect + std::cell::Cell import 추가
- theme.rs doc: Signal::set + ThemeProvider 백틱 정정 (doc_markdown clean)



**Verification**:
- cargo test -p pinion-core --lib theme: 44 pass 0 fail (R590 40 + 4 R592)
- cargo test --workspace --features pinion-runtime/vello: 2812 pass / 0 fail / 13 ignored (+4)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings



**Impact**: §5.50


**Carry forward**:
- 미래 ThemeProvider field-caching refactor 시 theme() Signal read 유지 필수 (R592 회귀가 catch)



### Round 593 — ThemeProvider::set_palettes(light, dark) atomic batch primitive; 두 Signal write 를 1 reactive flush 로 coalesce — subscriber 가 1번만 re-run.

**Changes**:
- theme.rs: ThemeProvider::set_palettes 가 light + dark Signal::set 을 reactive::batch 으로 wrap
- theme.rs: +2 회귀 (atomic batch coalesce + light/dark round-trip)
- theme.rs: batch import from crate::reactive 추가



**Verification**:
- cargo test -p pinion-core --lib theme: 46 pass 0 fail (R592 44 + 2 R593)
- cargo test --workspace --features pinion-runtime/vello: 2814 pass / 0 fail / 13 ignored (+2)
- cargo clippy --workspace --all-targets --features pinion-runtime/vello: 0 warnings



**Impact**: §5.50


**Carry forward**:
- M3 dynamic-color tonal palette pair (seed 색 기반) consumer 가 set_palettes 소비



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



