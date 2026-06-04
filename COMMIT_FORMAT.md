# Commit Message Format Guide (pinion)

## Structure

```
<type>(<scope>): <subject>

- <detail 1>
- <detail 2>
- <detail 3>
```

## Rules

### 1. Subject Line
- Format: `<type>(<scope>): <subject>` (scope is optional but recommended)
- Types: `feat`, `fix`, `refactor`, `test`, `docs`, `build`, `chore`
- Subject: Clear and concise description of the change
- No period at the end
- Max 72 characters

### 2. Scope (Optional but recommended)

Pinion specific scopes:

| Scope | Domain |
|---|---|
| `mnemosyne` | Atomic store / Round changelog mutations |
| `atomic` | Direct atomic JSON edits (anti-pattern; only for override) |
| `meta` | Workspace metadata, `mnemosyne.toml`, `CLAUDE.md` |
| `scaffold` | Initial project setup, workspace skeleton |
| `core` | `pinion-core` crate (scene, widgets, types) |
| `runtime` | `pinion-runtime` crate (render loop, SCE embed) |
| `rpc` | `pinion-rpc` crate (JSON-RPC server, 7 methods) |
| `cli` | `pinion-cli` crate (developer CLI) |
| `render` | `pinion-render-*` future crates (RHI, shader, core) |
| `widgets` | UI widget SCXMLs + Rust wrappers |
| `scxml` | SCXML authoring (state machine docs) |
| `arch` | Architecture decisions (§5.X axis ratify, supersede) |
| `hooks` | `.githooks/` (commit-msg, pre-commit, pre-push) |
| `vendor` | `vendor/sce` submodule updates |
| `build` | Cargo workspace, `build.rs`, `rust-toolchain.toml` |
| `docs` | `CLAUDE.md`, README, comment-only fixes |

### 3. Body
- One blank line after subject
- Bullet points (`- ` prefix) only — no prose lead paragraph
- Bullets must be **contiguous** — no blank line between bullets
- **1-3 items** — focus on key changes (fewer is better)
- The `commit-msg` hook enforces bullet-only + contiguity + the 1-3
  cap (a prose body line, a blank line between bullets, or a 4th
  bullet is rejected, not just discouraged)
- **One bullet = one line, max 72 bytes total (incl. `- ` prefix)**
  - No continuation / indented wrap lines. If a bullet does not fit in
    72 bytes, rewrite it tighter or split into a separate bullet.
  - Verify with: `git log -1 --format=%B | awk '{print length, $0}'`
- Be specific and technical
- Reference Mnemosyne sections in `§N.M` form (e.g., §5.16, §6.4, §5.15)
- Reference Mnemosyne rounds as `R<N>` (e.g., R10, R12, R297)
- Reference SCE upstream via 8-char commit SHA when relevant

### 4. Style
- **English only** — subject and body must be written in English so the
  log stays accessible to every collaborator. ASCII printable (U+0020 to
  U+007E) plus the whitelist of typographic symbols below are the only
  permitted code points; any character outside this set (Hangul, Kana,
  CJK ideographs, Cyrillic, Greek, etc.) is rejected by the commit-msg
  hook.
  - Typographic whitelist: `§` (U+00A7), `–` (en-dash U+2013), `—`
    (em-dash U+2014), `•` (bullet U+2022), `…` (ellipsis U+2026), `→`
    (rightwards arrow U+2192). These are the only non-ASCII code points
    the hook lets through.
  - Round summaries / progress notes that need Korean phrasing belong
    in `docs/SEED_PROMPT.md` (a regular file edit) or auto-memory under
    `memory/`, never in the commit message.
- **No emojis** (Unicode pictograph ranges U+1F300-U+1FAFF and
  U+1F1E6-U+1F1FF are rejected; the typographic symbols above are
  explicitly allowed)
- **No "Generated with Claude Code"**
- **No "Co-Authored-By" tags**
- Professional and technical tone
- Focus on "what" and "why", not "how"
- Quantify with Mnemosyne validate deltas where possible
  (e.g., `entries 9 → 11`, `T3 warn 6 → 13`)

## Type Guidelines

| Type | When to Use | Examples |
|------|-------------|----------|
| `feat` | New §N axis, Round close, widget, RHI binding, code module | R10 §5.16 codegen ratify, R12.0 Button SCXML widget |
| `fix` | Spec correctness fix, supersede after wrong design call | R11 §5.16 supersede (codegen reject) |
| `refactor` | Structural change without semantic shift | Move widget SCXMLs into per-class subdirs |
| `test` | Round-trip / unit / integration test addition | Add Button click-cycle state assertions |
| `docs` | Comment-only fix, README, CLAUDE.md clarification | Clarify Round 9 redaction path in CLAUDE.md |
| `build` | Cargo workspace, build.rs, toolchain, hooks | Pin rust-toolchain 1.85, add pre-push validate |
| `chore` | Project structure, gitignore, housekeeping | Scaffold initial Mnemosyne workspace |

## Examples

### Good: Round close (feat with mnemosyne scope)
```
feat(mnemosyne): R12.0 Button SCXML state machine (first impl)

- pinion-core/widgets/button.scxml: 4-state SCXML (null datamodel)
- sce-rust-runtime + sce-build deps; build.rs strip + module wrap
- 7 unit tests pass; entries 11 → 12; SCE R15 consumer pattern proven
```

### Good: Spec supersede (feat with arch scope)
```
feat(arch): R11 §5.16 supersede — codegen reject + thin RHI ratify

- SCE Forge GPU codegen RFC withdrawn (Futamura projection limit)
- New §5.16: SCXML state + Forge codec/buffer-pool/worker + naga
- AAA dispatch industry validation; 9-12mo SCE Phase B wait removed
```

### Good: Initial scaffold (chore)
```
chore(scaffold): SCE submodule + Mnemosyne workspace + R1 spec

- SCE submodule branch=main tracking; Mnemosyne workspace baseline
- License: LGPL-3.0-or-later OR LicenseRef-pinion-Commercial
- Round 1: 4 sections + changelog with full audit fields
```

### Good: Widget extension (feat with widgets scope)
```
feat(widgets): TextField SCXML — empty/typing/committed/disabled

- 4-state machine + onCommit raise on Enter / pointer_leave
- Rust wrapper + 5 unit tests; pinion-core widgets module expanded
- mnemosyne: no atomic changes (widget-only)
```

### Good: Hook installation (build)
```
build(hooks): pre-push mnemosyne-cli validate-workspace gate

- pre-push runs validate-workspace before remote publish
- COMMIT_FORMAT.md authored; commit-msg hook enforces rules
- core.hooksPath unchanged (.githooks/); 3 hooks active
```

### Bad: Multi-line bullet (continuation/indented wrap)
```
feat(arch): R10 §5.16 codegen renderer ratify (wrong design call —
    superseded in R11)
```
**Problem**: subject wraps onto continuation. Rule is one bullet =
one line ≤72 bytes. Rewrite tighter:
```
feat(arch): R10 §5.16 codegen renderer ratify (R11 supersedes)
```

### Bad: Too many bullets
```
feat(mnemosyne): R2-R9 spec evolution

- R2: §5.1-§5.10 axes enumerated
- R3: 9 axes ratified
- R4: §6 bootstrap added
- R5: 4 axes ratified
- R6: Cargo workspace skeleton
- R7: §5.15 External contract
- R8: CLAUDE.md guide
- R9: privacy redaction
```
**Problem**: 8 items — condense to 2-3 key changes covering rounds.

## Domain-Specific Guidelines

### Mnemosyne workspace mutations (`feat(mnemosyne)` or `fix(mnemosyne)`)
- Reference primitive name (`set_section_intent`,
  `append_changelog_entry_v2`, `set_section_impact_scope`)
- Cite atomic ledger delta: `entries N → M`, `sections N → M`
- Cite validate metrics: `T1=0`, `RT 1/1`, `T3 warn N → M`
- Round N audit entry as the close marker

### Spec axis decisions (`feat(arch)`)
- `§N.M` reference style (e.g., §5.16, §6.4, §5.15)
- Cite alternatives rejected when material
- Note SCE consumer pattern alignment (R15 precedent) when relevant

### Widget authoring (`feat(widgets)`)
- SCXML file path + state count
- Wrapper test coverage (assertions)
- SCE consumer pattern compliance

### Code module first-impl (`feat(core)`, `feat(runtime)`, `feat(rpc)`)
- Crate + module path
- Public API surface introduced
- Test pass count

### Atomic ledger metric format

Quantify mutations with validate output deltas:
- **Entries**: `entries 9 → 12` (atomic ledger growth)
- **Sections**: `sections 24 → 26`
- **Validate**: `T1=0`, `T3 warn N → M` (store-direct since Mnemosyne R400)
- **Divergence** (R294 publishable/audit split): `divergence=N/M SHA256 match`

## Key Points

- 1-3 bullets (use fewer when sufficient)
- No emojis, no attribution tags
- Specific §N.M sections, Round R##, primitive names
- Quantify Mnemosyne deltas
- Note carve-out vs typed-primitive mutation path
