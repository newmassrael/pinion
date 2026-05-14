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
- ivis WP5 Analyzer widget catalogue first dogfood scope driver



**Outputs**:
- Framework Rust impl with invariants enforced at type/build level
- AI-introspectable RPC headless API exposed to MCP-compatible clients
- Cascade-emit GUI/TUI/RPC backends from canonical scene structure
- SCE Forge second domain demo (after watching-zenoh Zenoh protocol)








### §2. Settled invariants


**Intent**: v1 invariants: structured scene mandatory; RPC headless; dry_run; mode toggle; SCE state; GUI/TUI dual; scene-as-data


**Rationale**:
- Structured scene enforced means AI introspect everywhere not pixel-blind
- Event-with-input contract collapses Tier-2 hypothetical-input awkwardness
- RPC headless = AI primary path; TUI dump = fallback; GUI = humans
- dry_run primitive enables zero-cost scenario exploration via SCE determinism
- Mode toggle immediate vs retained = same view fn, two execution strategies
- SCE statechart kind already 6-backend byte-golden parity per watching-zenoh
- Visual state (geometry, z-order, opacity stack) queryable as text, no pixels



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






### §4. First dogfood: ivis WP5 Analyzer


**Intent**: First dogfood: ivis WP5 Analyzer (Zenoh network analyzer); autonomous, no deadline; validates framework on real surface



**Inputs**:
- ivis Work Package 5 slide widget catalogue (Data Interp Viz Diag Util Sim)
- watching-zenoh wire spec subset for live capture decode (out-of-process)
- Approximately 12 core widgets and 6 domain-specific per slide analysis
- Network capture/replay/fuzz declared as external actor not framework widget



**Outputs**:
- Analyzer 90%+ widgets covered by Tier-1 pure (event-with-input contract)
- Framework MVP scope tied to actual customer-driven widget requirements
- Validates dry_run and RPC introspection on real-world non-trivial GUI surface
- Node graph topology widget tests Canvas + custom scene primitive coverage



**Caveats**:
- Analyzer autonomous; no delivery deadline binding framework cadence
- Framework MVP first then Analyzer slice; not parallel deadline pressure
- Node graph widget largest single component; potentially 1-2 month sub-effort







## Changelog (atomic ledger)

### Round 1 — Initial pinion-gui spec capture: 7 framework invariants, 2 opaque escapes, Analyzer dogfood, dual license, scaffold

**Changes**:
- §1 Vision: AI-native cross-platform GUI framework via SCE statechart + structured scene
- §2 Settled invariants: 7 binding rules (scene, RPC headless, dry_run, mode, SCE, dual, DSL)
- §3 Capability boundaries: Effect/External opaque escape; WebEngine/codec out of scope
- §4 First dogfood: ivis WP5 Analyzer (autonomous, no deadline, real widget surface)
- Project scaffold: SCE submodule branch=main, Mnemosyne workspace, .githooks copy
- License files: LICENSE + LICENSE-COMMERCIAL + LGPL-3 verbatim + GPL-3 verbatim
- .gitignore: GENERATED.md committed (greenfield doc surface, atomic-first design)
- .mcp.json: mnemosyne-mcp pointing at /home/coin/pinion-gui workspace



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
- Axis #1 decision (framework-first vs Analyzer-slice-first) gating for #3 and #6
- CLAUDE.md authoring for pinion-gui (SSOT contract + auto-kickoff trigger)
- Initial git commit (SCE submodule + Mnemosyne workspace + license + atomic + Round 1)
- Open axis #5 MCU v1 backend decision (recommend AP-only first cut)
- Open axis #7-#10 AI-native core invariants (RPC headless, dry_run, TUI dual)



