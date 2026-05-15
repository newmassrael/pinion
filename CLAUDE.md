# pinion-gui — AI-native cross-platform GUI framework

## Quick start for AI agents

Reading order when entering this repo:

1. **`docs/GENERATED.md`** — full spec, auto-rendered from atomic store. Read top to bottom.
2. **`mnemosyne://concepts/overview`** — Mnemosyne contract; mutations to docs go through typed primitives, never direct edits.
3. **This `CLAUDE.md`** — project-specific operational rules and structure map.
4. **`git log --oneline`** — implementation progress since spec phase ended.

## Project identity

pinion-gui synthesizes interactive UIs from:

- **SCE statechart** (vendored at `vendor/sce`) for widget/screen/gesture state machines
- **Structured scene DSL** (Rust view functions, Xilem-style) for AI-introspectable UI
- **JSON-RPC 2.0 headless API** for AI agent integration (7 typed methods)
- **GUI + TUI dual backends** from one canonical scene structure

§1 vision settled Round 1; spec phase concluded Round 7.

## Hard invariants (never violate)

§2 — 7 binding invariants:

1. Structured scene mandatory (no opaque paint callbacks)
2. RPC headless as AI primary path
3. dry_run primitive (zero-cost scenario exploration via SCE determinism)
4. Mode toggle immediate vs retained (runtime flag, single binary)
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

All non-trivial decisions live in the **Mnemosyne atomic store** at `docs/.atomic/workspace.atomic.json`. `docs/GENERATED.md` is the rendered view.

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

**Never edit `docs/GENERATED.md` or files under `docs/.atomic/` directly.** Use the Mnemosyne MCP primitives (`set_section_*`, `append_changelog_entry_v2`, etc.).

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
  GENERATED.md          human-readable spec (auto-rendered)
  .atomic/              source of truth (do not hand-edit)
vendor/
  sce/                  scxml-core-engine submodule, branch=main
.githooks/              pre-commit, commit-msg, pre-push (active via core.hooksPath)
mnemosyne.toml          workspace config (docs scope, schema, locale)
```

## Working contract

- Build: `cargo check --workspace`, `cargo test --workspace`
- Lints (workspace-wide): `unsafe_code = "forbid"`, `clippy::pedantic = "warn"`
- All new Rust code lives in `crates/`; no top-level Rust files
- `view-fn` is **sync** (purity invariant per §6.3 — required for dry_run guarantee)
- RPC and IO are **async via tokio** (boundary at `pinion-rpc` server entry per §6.3)
- Commit message style: include section refs (e.g. `impl §5.2 + §5.11 scene primitive enum`)

## Pre-commit hook

`.githooks/pre-commit` runs:

- `mnemosyne-cli verify-generated` — atomic store ↔ `GENERATED.md` byte sync
- `mnemosyne-cli validate-workspace` — T1 cross-ref + T2 frozen ledger

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

## Active carry-forward (Round 7 → 8+)

- §1 vision update consideration (`introspection protocol for interactive apps`)
- External example widgets (game viewport / video / PDF) as reference implementations
- Tier 2 streaming axes: screenshot video, partial repaint subscription
- `pinion-core`: scene primitive enum + Style trait + Modifier per §5.2 §5.11
- `pinion-core`: Event enum + `External` opaque variant per §5.13
- `pinion-rpc`: JSON-RPC server skeleton + 7 typed methods per §5.7 §5.12
- `pinion-runtime`: SCE hierarchical embedding per §5.4 §5.14
- First dogfood sequencing per §4
