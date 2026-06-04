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
| **B. Professional GUI** (Qt / Flutter / Compose / React-class) | Multi-window + DCC/IDE/CAD-grade widget catalog + pro-tool performance | **~27%** (multi-window / DevTools / dock-editor + ~35 Tier-1 widgets done; OS-native integration + Model/View-at-scale + API stabilisation remain) |
| **C. Game engine substrate** (§2 #4 entry) | Immediate-mode game loop ↔ retained widget tree dual; 3D scene graph; asset pipeline; physics; audio; gamepad; PBR | **0%** |
| **D. AAA game maker** | Unreal-class editor **self-hosted in pinion**; visual scripting; Nanite/Lumen-class rendering; multiplayer netcode | **0%** |

**True north**: AAA game shippable + Unreal-class editor self-hosted in pinion itself, with AI-introspection 1st-class through every phase.

Current weighted progress against true north: **~11.6%** (Phase A 97% × 5% phase-weight + Phase B 27% × 25% + Phase C 0% × 35% + Phase D 0% × 35%), as of R752. Figures are soft self-estimates, not a measured metric.

R655-R666 todomvc + R667 settings-panel = Phase A finalisation. R700+ = Phase B entry (multi-window first). R1000+ = Phase C entry (ImmediateModeNode + game loop). R2500+ = Phase D entry (editor self-hosted dogfood).

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
- Lints (workspace-wide): `unsafe_code = "forbid"`, `clippy::pedantic = "warn"`
- All new Rust code lives in `crates/`; no top-level Rust files
- `view-fn` is **sync** (purity invariant per §6.3 — required for dry_run guarantee)
- RPC and IO are **async via tokio** (boundary at `pinion-rpc` server entry per §6.3)
- Commit message style: include section refs (e.g. `impl §5.2 + §5.11 scene primitive enum`)

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
