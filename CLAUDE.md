# pinion — AI-native GUI framework → game engine substrate → AAA game maker

## Quick start for AI agents

Reading order when entering this repo:

1. **`docs/SEED_PROMPT.md`** — single-source entry point. Self-contained: 불변 운영 원칙 + 직전 세션 결과 + 다음 텍스트북 캐논 + watch out + lessons. New-session entry = `load` skill OR `@docs/SEED_PROMPT.md 읽고 R<현재 라운드> 진행`. Read this first; everything else is reference.
2. **Mnemosyne atomic store** (`docs/.atomic/workspace.atomic.json`) — full spec SSOT. Read with `mnemosyne-cli query --list-sections` then `mnemosyne-cli query §<id>` (the markdown-doc render `docs/GENERATED.md` was retired in Mnemosyne R395–R400).
3. **`mnemosyne://concepts/overview`** — Mnemosyne contract; mutations to the store go through typed primitives, never direct edits.
4. **This `CLAUDE.md`** — project-specific operational rules and structure map.
5. **`git log --oneline`** — implementation progress since spec phase ended.

## Project identity

pinion synthesizes interactive UIs **and games** from:

- **SCE statechart** (vendored at `vendor/sce`) for widget/screen/gesture/game-AI state machines
- **Structured scene DSL** (Rust view functions, Xilem-style) for AI-introspectable UI
- **JSON-RPC 2.0 headless API** for AI agent integration (7 typed methods; R660 drag, R663 double_click, R666 v1 multi-External path syntax + character-key auto-discriminator)
- **GUI + TUI dual backends** from one canonical scene structure
- **§2 #4 dual execution**: retained widget tree (idle 30fps) ↔ immediate-mode game loop (60-144fps lockstep) per `Scene::Container` subtree opt-in

§1 vision settled Round 1; spec phase concluded Round 7; **R663.5 vision corrected** (4-phase progression made explicit).

### Northern-star (4-phase progression)

| Phase | Target | Current |
|---|---|---:|
| **A. Foundation** (§1-§4 spec + first composed multi-widget apps) | "Hello-world apps possible" — todomvc + settings panel | **~97%** (R666 todomvc + R667 settings-panel + R668 close = finalised) |
| **B. Professional GUI** (Qt / Flutter / Compose / React-class) | Multi-window + DCC/IDE/CAD-grade widget catalog + pro-tool performance | **~56%** (R931 re-tally, bottom-up weighted: 20 crates / 115 examples / 228 demos / 31+ widget families. Per-axis × weight × evidence-grounded completion: common widget catalog+interaction 18%×70% / text editing 10%×58% (single+multi-line+IME+selection + R903 find-replace + R904 syntax + R926 bracket-match + R928 styled-run format-undo) / **advanced DCC widgets 22%×73%** (property-grid ~80 [tree+multi-object+array] + editable data-grid ~90 [dynamic rows] + node-editor ~85 [reconnection] — example-grade, node *eval*=Phase C) / Model/View-at-scale 18%×68% (windowing+3 composable proxies + async/lazy/source-side-sort [R923/R924/R927] + LRU-bounded million-row [R934] now landed; no unified data layer [deliberate]) / pro-tool perf 10%×43% (frame-timing+jank-profiler infra [R907/R925]; no measured opt) / OS-native 12%×48% (file-dialog/clipboard/drag-drop/prefers-scheme + PDF render[R908]/PDF-print[R911]/CUPS[R833] Linux-verified done; native-menu Mac/Win-only; tray SNI substrate + real D-Bus bridge LANDED R949/R950 [`pinion-platform-tray` ksni/SNI + `InMemoryTrayBackend` + `hello-tray`] — the "0%" in prior tallies predates R949/R950; the headless SNI CI-coverage gap CLOSED R1267 [re-exec under `dbus-run-session` + an `org.kde.StatusNotifierWatcher` test-double asserts register + dbusmenu-export, ZERO-FLAKE, no panel pixels], so the remaining OS-native gap = Mac/Win-specific native surfaces only, not Linux tray [docs/cross-platform-native-strategy.md]) / §7 API 10%×15% = ≈57%, reported ~56% conservative. Jump from R900's ~48%: R901-R934 added rich-text [find-replace/syntax/bracket/format-undo] + Model/View async/lazy/source-sort + LRU million-row + DCC depth [array/dynamic-rows/reconnect] + perf profiler) |
| **C. Game engine substrate** (§2 #4 entry) | Immediate-mode game loop ↔ retained widget tree dual; 3D scene graph; asset pipeline; physics; audio; gamepad; PBR | **~5%** (§2 #4 immediate-mode game loop I/O surface + fixed-timestep done R681/R827-R831; 3D / asset / physics / audio / gamepad / PBR all 0%) |
| **D. AAA game maker** | Unreal-class editor **self-hosted in pinion**; visual scripting; Nanite/Lumen-class rendering; multiplayer netcode | **0%** |

**True north**: AAA game shippable + Unreal-class editor self-hosted in pinion itself, with AI-introspection 1st-class through every phase.

Current weighted progress against true north: **~20.5%** (Phase A 97% × 5% phase-weight + Phase B 56% × 25% + Phase C 5% × 35% + Phase D 0% × 35%), R931 re-tally. Figures are soft self-estimates with a per-axis breakdown (do not over-read precision; ±5%); the Phase B figure was re-tallied bottom-up at R931 (axes × weights × evidence-grounded completion; R900 was the prior tally at ~48%, advanced by the R901-R931 rich-text + Model/View-async/lazy + DCC-depth + perf-profiler build-out).

R655-R666 todomvc + R667 settings-panel = Phase A finalisation. R700+ = Phase B entry (multi-window first). R1000+ = Phase C entry (ImmediateModeNode + game loop). R2500+ = Phase D entry (editor self-hosted dogfood).

**Phase B value order (R835 directive — do the highest-value-for-the-northern-star first).** The true north is the Unreal-class editor self-hosted in pinion; Phase B is the substrate that editor is built on, so "high value" = what the self-hosted editor most needs and what is high-weight × low-completion. Work this order (audit-first within each):

1. **Advanced DCC/IDE widgets** — property-grid / inspector panel (typed editable rows), advanced data-grid (cell editors / grouping / frozen panes), node-graph editor substrate (visual scripting / material graph). **~73% done** (R931 re-tally — property-grid ~80 [R921 struct-tree + R922 multi-object inspector + R931 array/Vec element editing] / editable data-grid ~90 [R930 dynamic add/remove rows] / node-editor ~85 [R929 edge reconnection], all dogfood-verified; remaining: node *evaluation* [Phase C], advanced delegates / per-element modified-reset). R935 added tree drag-to-reparent (scene outliner: `hello-tree-reparent`, drag onto=child / between=reorder, cycle-guarded) + lifted `remove_subtree`/`insert_subtree` to `tree_nav`. **Edit-machinery crate-lift is audited-PREMATURE (R935): `edit_field_keymap`/`UndoStack`/`CellValue`/`TreeNode` already lifted; the per-widget commit/edit-latch/keymap code is divergent domain logic, not missed abstraction — do not chase a wholesale property-grid/data-grid/node-editor crate extraction.** Leverage now = depth + new gestures, not crate extraction.
2. **Model/View at scale** — the large-data backbone (asset browser / scene outliner: 10k+ rows, unified sort/filter/group). **~68% done** (windowing [list/grid/tree] + 3 orthogonally-composable proxies [sort/filter/group/tree-filter] + data-indexed selection + **async/lazy + LRU million-row now landed**: R923 paged `Resource` view + R924 virtualized lazy-load infinite-scroll + R927 out-of-memory source-side sort/filter via `ResourceCache` + R934 LRU-bounded `ResourceCache::with_capacity` (1M-row `hello-million-row`: bounded memory + scroll-back eviction witness); **no unified data layer** [deliberate, R780/R821 4th-consumer gate]).
3. **Pro-tool performance** — 60fps with large scenes; profiling. ~43% (virtualization + dirty-cache + **measure-first infra: R907 `scene/frame_timings` + R925 frame-budget jank profiler** [over-budget/jank-ratio classification]; no measured large-scene hot-path opt yet, no GPU-timestamp render time).
4. **OS-native maturity** — finish file-dialog / print / drag-drop / tray. ~48% (file-dialog / clipboard / drag-drop / prefers-color-scheme + **scene→vector PDF render [R908] + vector-PDF print spool [R911] + CUPS spool [R833] all Linux-verified** done; native-menu rendered-not-native [Mac NSMenu / Win HMENU need those OSes], no Win/Mac native print dialog, **Linux tray SNI substrate + real D-Bus bridge LANDED R949/R950** (`pinion-platform-tray` = pure ksni/SNI StatusNotifierItem, no gtk — sidesteps the gtk-tray-crate/winit incompat; `InMemoryTrayBackend` headless fallback + `hello-tray` + `r949_tray.py`; R932.2 was the DESIGN round, R949/R950 the IMPL — prior "tray 0%" tallies predate them). Tray CI-coverage gap CLOSED R1267: the headless session-bus **test-watcher fixture** (a launcher re-execs an ignored inner scenario under `dbus-run-session` — ksni hard-wires the session bus and `unsafe_code=forbid` blocks `set_var`, so the private bus is provisioned in the child env — hosting an in-process `org.kde.StatusNotifierWatcher` test-double that asserts SNI-register + dbusmenu-export, ZERO-FLAKE, no panel pixels) landed and CI-covers `pinion-platform-tray`'s SNI integration path [was `#[ignore]d` = zero CI coverage]; `docs/cross-platform-native-strategy.md` staged-path step ① "Verification harness" satisfied). The remaining real blocker is Mac/Win-specific native surfaces (need those OSes' CI runners), NOT Linux tray — PDF/print/file-dialog/clipboard/DnD all Linux-native + verified, and the tray capability itself is built.
5. **API stabilisation (§7)** — LATER: freeze a *mature* surface, not a churning one. ~15%.
6. **rich-text editing/selection** — code-editor-grade text. ~58% (single+multi-line+IME+selection+caret + **R903 find-replace + R904 syntax highlighting + R926 matching-bracket + R928 styled-run formatting undo** now landed; full styled-run *text-editing* depth / code-folding still partial).

NOT catalog regression (R760 PIVOT): a property grid / node editor / advanced data-grid are NEW high-value editor widgets, not re-doing finished basic widgets.

## Hard invariants (never violate)

§2 — 7 binding invariants:

1. Structured scene mandatory (no opaque paint callbacks)
2. RPC headless as AI primary path
3. dry_run primitive (zero-cost scenario exploration via SCE determinism)
4. Mode toggle immediate vs retained (runtime flag, single binary) — **Phase C entry**: immediate-mode game loop ↔ retained widget tree dual execution per `Scene::Container` subtree (NOT GUI diff optimisation)
5. SCE statechart state (hierarchical: root + scoped child SCEs)
6. GUI/TUI dual (one scene, two render dispatch paths)
7. Scene-as-data (queryable as text, no pixels in introspection)

§3 — capability boundaries:

- `Effect(opaque)` and `External(opaque)` are the **only** escape hatches
- Embedded WebEngine/Chromium-class is **out of scope**
- Multimedia codec embed is **out of scope**

§5.15 — External primitive integration contract (8 mandatory items):

backend declaration / repaint trigger / thread ownership / lifecycle callbacks / input forwarding / DPI notification / async state channel / optional symbolic introspection

**If a new feature requires breaking any of these, STOP and propose a new spec round. Do not work around.**

## Decision audit trail

All non-trivial decisions live in the **Mnemosyne atomic store** at `docs/.atomic/workspace.atomic.json` — the sole directly-validated SSOT. Read it via `mnemosyne-cli query` (the rendered `docs/GENERATED.md` view was retired in Mnemosyne R395–R400).

Spec phase summary:

| Round | Outcome |
|---|---|
| 1 | Vision + 7 invariants + boundaries + first dogfood |
| 2 | 10 open implementation axes enumerated |
| 3 | 9 axes ratified (framework-first / closed-form / view-fn DSL / JSON-RPC / hierarchical SCE / etc.) |
| 4 | §6 bootstrap (workspace / toolchain / async) + 4 new axes added |
| 5 | 4 axes ratified (layered primitives / hybrid RPC / closed core events / hierarchical SCE) |
| 6 | Cargo workspace skeleton (first impl commit) |
| 7 | §5.15 External contract (8 items) + §5.12 screenshot RPC method |

**Never edit files under `docs/.atomic/` directly.** Use the Mnemosyne typed primitives — `set_section_*` etc. via MCP, and append the changelog with the CLI `mnemosyne-cli append-changelog-entry` (the MCP `append_changelog_entry_v2` wrapper is removed: it shells out to a dropped `append-changelog-entry-v2` subcommand and now errors `unknown command`).

## Repository structure

```
Cargo.toml              workspace root (resolver=3, edition 2024, MSRV 1.85)
rust-toolchain.toml     stable channel pin + rustfmt + clippy
crates/
  pinion-core/          scene primitives, Style trait, Modifier, view-fn types, Event enum
  pinion-runtime/       render loop, SCE hierarchical state machinery
  pinion-rpc/           JSON-RPC 2.0 server (7 typed methods + path/filter)
  pinion-cli/           developer CLI binary
docs/
  .atomic/              spec SSOT (do not hand-edit; read via `mnemosyne-cli query`)
vendor/
  sce/                  scxml-core-engine submodule, branch=main
.githooks/              pre-commit, commit-msg, pre-push (active via core.hooksPath)
mnemosyne.toml          workspace config (schema, locale, validators, ledgers)
```

## Working contract

- Build: `cargo check --workspace`, `cargo test --workspace`
- System deps (Linux): `libfontconfig1-dev libxkbcommon-dev libasound2-dev` — ALSA
  headers are needed because `examples/hello-audio-device` enables
  `pinion-audio/cpal-backend` (it opens a real output device; R1310). Depending on
  `pinion-audio` *without* that feature needs no ALSA. The device demo additionally
  wants a silent card: `sudo modprobe snd-dummy`
- Lints (workspace-wide): `unsafe_code = "forbid"`, `clippy::pedantic = "warn"`
- All new Rust code lives in `crates/`; no top-level Rust files
- `view-fn` is **sync** (purity invariant per §6.3 — required for dry_run guarantee)
- RPC and IO are **async via tokio** (boundary at `pinion-rpc` server entry per §6.3)
- Commit message style: include section refs (e.g. `impl §5.2 + §5.11 scene primitive enum`)

## Cross-repo discipline (NEVER edit other repositories directly)

**Hard rule (user directive, 2026-06-19):** from a pinion session, NEVER directly
modify any repository other than pinion itself. In particular **`sprag`**
(`/home/coin/sprag`, a separate consumer repo that path-deps pinion) is
off-limits: do not edit its source, run its build / `cargo` / `git` /
`mnemosyne-cli`, commit there, or mutate its store. This applies even when the
user says "continue sprag work" — that does NOT authorise editing the sprag tree
from here.

- **What pinion sessions DO for sprag:** deliver pinion-side seams *here* (the
  consumer-forced seam rounds — R1007–R1011 pattern), and hand off requirements /
  status as docs (e.g. `sprag/claudedocs/PINION-REQUIREMENTS.md`, or a
  `claudedocs/HANDOFF-*.md`). Writing such a handoff doc into the other repo's
  gitignored `claudedocs/` is the only permitted cross-repo write, and only when
  the user explicitly asks for it.
- **The sprag-side consumer rounds** (editing sprag crates, sprag commits, sprag
  Mnemosyne) are done in a **sprag-native session**, never from pinion.
- If a task seems to require editing another repo, STOP and produce a handoff
  instead; do not work around this.

## Pre-commit / pre-push hooks

`.githooks/pre-commit` runs:

- `mnemosyne-cli validate-workspace` — T1 cross-ref + T2 frozen ledger + atomic-store consistency (store-direct since Mnemosyne R400)
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` — only when staged files include `*.rs`; enforces the workspace.lints baseline (forbid unsafe / deny warnings / clippy::pedantic deny)

`.githooks/pre-push` repeats both gates unconditionally, so amends / rebases / `--no-verify` bypasses cannot publish a state that fails either check.

If a hook fails, **fix the underlying issue**. Never bypass with `--no-verify` unless the user explicitly requests it.

## Vendor submodule (vendor/sce)

- Tracks `scxml-core-engine` on `branch=main`
- Update: `git submodule update --remote vendor/sce`
- Embedded directly into `pinion-runtime` per §5.4 ratify (Forge Rust emit, no FFI)
- Do not edit `vendor/sce/` directly; PR upstream

## License

- Framework code: **LGPL-3.0-or-later** (`workspace.package.license`)
- Commercial dual-license track: see `LICENSE-COMMERCIAL.md`
- `LICENSE-GPL-3.0.md` and `LICENSE-LGPL-3.0.md` are verbatim copies for compliance shipping

## Active carry-forward

> R666.1 cleanup — the Round-7 dogfood-sequencing carry below is historical
> (all items long since landed; R7 was the spec-phase exit). Live carry
> tracking moved to **`docs/SEED_PROMPT.md`** 's 'watch out' + 'R<NNN>
> carry' sections, refreshed every round close. This list stays as a
> historical anchor for the spec-phase → impl-phase transition.

Historical (R7 → R8 spec-exit dogfood sequencing — all done by R51+):

- §1 vision update consideration (`introspection protocol for interactive apps`)
- External example widgets (game viewport / video / PDF) as reference implementations
- Tier 2 streaming axes: screenshot video, partial repaint subscription
- `pinion-core`: scene primitive enum + Style trait + Modifier per §5.2 §5.11
- `pinion-core`: Event enum + `External` opaque variant per §5.13
- `pinion-rpc`: JSON-RPC server skeleton + 7 typed methods per §5.7 §5.12
- `pinion-runtime`: SCE hierarchical embedding per §5.4 §5.14
- First dogfood sequencing per §4

Current live carry (R673 → R674): see `docs/SEED_PROMPT.md` 【watch out】 +
"R673 carry" subsections. R674 atomic list is the authoritative entry plan
(TreeView click-to-expand + per-row TreeItem AccessNodes).
