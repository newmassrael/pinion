# pinion — AI-native GUI framework → game engine substrate → AAA game maker

## Quick start for AI agents

Reading order when entering this repo:

1. **`docs/SEED_PROMPT.md`** — single-source entry point. Self-contained: 불변 운영 원칙 + 직전 세션 결과 + 다음 텍스트북 캐논 + watch out + lessons. New-session entry = `load` skill OR `@docs/SEED_PROMPT.md 읽고 R<현재 라운드> 진행`. Read this first; everything else is reference.
   **It is `.gitignore`d** (a local working file, deliberately): a fresh clone has no SEED, and continuity there starts from the auto-loaded `memory/MEMORY.md` + `git log` + the atomic changelog instead. Per-round history does NOT live in SEED — it is kept slim on purpose, because a bloated SEED trips a `/load` auto-compaction bug ([[seed-must-stay-slim]]); replace its land block each round rather than accumulating.
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
| **B. Professional GUI** (the toolkit / another retained-mode toolkit / another declarative toolkit / web UI library-class) | Multi-window + DCC/IDE/CAD-grade widget catalog + pro-tool performance | **~88%** all axes / **~92%** buildable — per-axis detail below |
| **C. Game engine substrate** (§2 #4 entry) | Immediate-mode game loop ↔ retained widget tree dual; 3D scene graph; asset pipeline; physics; audio; gamepad; PBR | **~15%** (R1519 re-tally). §2 #4 immediate-mode game loop I/O surface + fixed-timestep done R681/R827-R831. **Audio is NOT 0% any more** — `pinion-audio` 5.8k LOC, real cpal device backend + RT thread + RPC wire proof, CI-gated on `snd-dummy` (R1274-R1313); `pinion-asset` started (188 LOC). **3D / physics / gamepad / PBR still 0%.** The prior "audio 0%" text predates R1274 by ~250 rounds. |
| **D. AAA game maker** | engine-class editor **self-hosted in pinion**; visual scripting; Nanite/Lumen-class rendering; multiplayer netcode | **0%** |

**True north**: AAA game shippable + engine-class editor self-hosted in pinion itself, with AI-introspection 1st-class through every phase.

Current weighted progress against true north: **~32%** (Phase A 97% × 5% phase-weight + Phase B 88% × 25% + Phase C 15% × 35% + Phase D 0% × 35%). Figures are soft self-estimates; do not over-read precision (±5%). Phase B is tool-backed — run `python3 tools/phase_b_tally.py`, and never hand-edit its numbers. **Phase C is 15%, not the R931-era 5%**: `pinion-audio` is 5.8k LOC with a real cpal device backend, an RT thread and an RPC wire proof (R1274-R1313), and `pinion-asset` exists (188 LOC — started, not built out); the "audio 0%" text predates R1274 by ~250 rounds. 3D / physics / gamepad / PBR remain 0%. **The lesson the R1519 re-tally records**: a progress figure with no recorded evidence and no staleness check is not a measurement — it held still for 587 rounds while the tree nearly doubled.

R655-R666 todomvc + R667 settings-panel = Phase A finalisation. R700+ = Phase B entry (multi-window first). R1000+ = Phase C entry (ImmediateModeNode + game loop). R2500+ = Phase D entry (editor self-hosted dogfood).

### Phase B — the eight axes

**The numbers and the work order are a tool's output. Do not hand-maintain them here.**

```bash
python3 tools/phase_b_tally.py    # weights, completions, evidence, LEVERAGE order
```

It counts the evidence each judgment was made against and reports STALE when that
evidence drifts >25% — which is what *demands* a re-judgment. The pre-push hook
prints it every push and (R1522) runs the tool's own `--selftest` first, withholding
the numbers if that fails.

`LEVERAGE = weight × remaining` (buildable axes only) **is** the work order — R1519
made it derived precisely so it re-ranks itself when a completion moves, and it has
changed 31 times. **Do not write the ranking down.** This file carried a hand-written
copy from R1519 to R1609; by then it had drifted from the tool on two axes while
telling every reader not to hand-edit it. The judgment prose that accumulated
alongside it now lives in `docs/phase-b-axis-history.md` — **do not append to
this file instead.** A re-judgment updates the tool and writes its reasoning
into the round's changelog entry and `docs/phase-b-rounds.tsv` row.

**Gated axes** — OS-native (Mac/Win surfaces need those OSes' CI runners) and §7 API
(deliberately parked: freeze a *mature* surface, not a churning one) — cannot be
advanced from here, so 100% of all-axes is unreachable by construction. **~92%
buildable is the number to move.**

#### What is left on each axis

The tool holds the numbers; it does not hold the reasoning. Below is the gap each
axis's **last audit** recorded — the audit round is stamped because several are older
than the judgment they sit under. Full rationale for every judgment, verbatim:
**`docs/phase-b-axis-history.md`**.

| Axis | w | Gap, and the round that last audited it |
|---|--:|---|
| **Advanced DCC / IDE widgets** | 20 | **R1604** re-judged 98 → 86 — the largest move here and the second downward one. The node-graph third now has a *measured, test-backed* census where the 98 rested on a hand count: **the DCC 54/72 = 75%**, **the engine 60/149 = 40%**, so node graph 97 → 62 beside property grid ~98 / data grid ~98. **Nothing was lost — the meter is new.** Open (R1594): ~7 editor gestures; **execution semantics** (no DCC comparison will surface these — it is pure dataflow too); `hello-node-editor` still holds its own model ([[debt-two-node-graph-models]]) |
| **Model/View at scale** | 16 | **R1563**: the section axis answers 2 of the toolkit's 4 header roles; **drag-select across sections** is blocked on the pointer wire not reporting held buttons (W3C `PointerEvent.buttons`); the keyboard has no two-axis vocabulary (`Ctrl+Space`, `Ctrl+Shift+Arrow`); `SelectColumns` has no binding; the eager `Table` keeps its own rectangle cell-selection, so the tree carries **two** cell-selection models |
| **Common widget catalog** | 16 | **R1672** re-judged 97 → 98, and the point is one item off this list: **the disabled cascade** is closed (R1668 typed reason on the declaration, R1669 branchless builder + a runtime boot census with floor 0). Open, re-measured R1672: wheel on combo box / tab bar is still the largest cross-cutting item (`External::wheel` has exactly **two** widget implementors, `slider` and `spin_button`, unmoved since R1554); **four** absent widget kinds (dial, a paged container, font picker, canned message box / input dialog — `hello-paged-stream` is Model/View paging and `hello-app-font` loads a face rather than picking one; the key-sequence editor landed R1569); mnemonic adoption is **5** declaring sites, measured (`group_box`, `menu`, and three screens) — the list had carried 4 since R1554 |
| **Charting / dataviz** | 10 | **R1568**: three series types left (spline, 3D-surface → Phase C's renderer, and the **OHLC bar**, now the cheapest item); no per-mark a11y on box / candle / polar; polar has no cross-filter leg and no legend interaction; category axis, label thinning and local time all still absent |
| **Rich-text editing/selection** | 9 | **R1642** re-judged 90 → 92 — one of R1560's five named gaps closed: the **character half** moved at last (R1641 `LetterSpacing`, R1641.3 `word_spacing`, both as an absolute-or-em-relative type rather than a number). Open: `setMarkdown` / `toHtml` (still the last of R1551's three); nested tables (needs the general text frame containment axis); text block format's 3 untaken properties; the character half's remainder (super/subscript, overline); the grid's `minmax()` / `fit-content()` / `grid-auto-flow` **authoring** vocabulary. ★Two of those were re-verified past a false positive — `Subscript` matches 74× as a substring of `Subscription*`, and `minmax` 3× in doc comments about taffy lowering |
| **Pro-tool performance** | 9 | **R1558**: **present latency** stays external (wgpu exposes no presentation-timestamp extension); the footprint is what the allocator was *asked for*, not what is resident; per-node replay status is absent by construction; a profile row's address has no general reader; the other five `path::resolve` callers still judge against the SCE topology |
| **OS-native maturity** *(gated)* | 11 | **R1576**: a display's **usable region** (the toolkit `availableGeometry`) needs a platform probe — winit's `MonitorHandle` has no work-area accessor and `_NET_WORKAREA` is one rect for the whole desktop; no hot-plug event (winit 0.30 emits none). What this axis is judged short on is Mac/Win native surfaces, **untouched** |
| **§7 API stabilisation** *(gated)* | 9 | **R1642** re-judged 65 → 72, absorbing R1637–R1640: the declaration is now a **precondition of dispatch** on both channels (this axis's first *guarantee*), an action declares its argument grammar and its **conditional cases**, a widget publishes its driveable verbs, and the gates cover all 39 widget surfaces. ★★**The measured finding: there are TWO self-describing surfaces and only one got the treatment** — probed at R1642, `rpc/methods` answers 111 methods carrying exactly `{name, occ, window}` (no parameters, no return type, no error codes, no revision) and `rpc/version` is not a method at all. Everything above lives on the per-External `$schema` path, and the JSON-RPC **method** surface — the one this axis is named for — is where R1585 left it ⇒ [[debt-two-describing-surfaces-at-different-maturity]]. Still open: method→type binding, version negotiation, deprecation path, freeze, per-method error taxonomy, census covers `pinion-rpc` only, no per-subscription filter, and **`SchemaChannel` still cannot say a slot is WRITABLE** |

#### Standing directives from these judgments

- **Do not add a `have` to the reference census without a proof** (R1602). Each
  verdict carries `proven_by` naming a test that exercises it through the **public
  API only**, and each crate's census asserts a bijection with the pin. A wrong
  `have` is the error direction that inflates a number silently — so it is the one
  that costs a test. A wrong `absent` self-corrects when the next round reaches for it.
- **Do not chase a wholesale property-grid / data-grid / node-editor crate
  extraction** (R935, audited-PREMATURE). `edit_field_keymap` / `UndoStack` /
  `CellValue` / `TreeNode` are already lifted; the per-widget commit / edit-latch /
  keymap code is divergent domain logic, not missed abstraction. Leverage on this
  axis is depth and new gestures.
- **A completion nobody checked against a reference is not a measurement** (R1519,
  and R1577 / R1604 are what it looks like when the check finally runs — both moved
  an axis DOWN). A re-judgment that *holds still* is a legitimate outcome: the tool
  demands a LOOK, not a move.

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
- Embedded WebEngine/embedded browser engine-class is **out of scope**
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
Cargo.toml              workspace root (resolver=3, edition 2024, MSRV 1.88)
rust-toolchain.toml     stable channel pin + rustfmt + clippy
crates/
  pinion-core/          scene primitives, Style trait, Modifier, view-fn types, Event enum
  pinion-runtime/       render loop, SCE hierarchical state machinery
  pinion-rpc/           JSON-RPC 2.0 server (7 typed methods + path/filter)
  pinion-cli/           developer CLI binary
docs/
  .atomic/              spec SSOT (do not hand-edit; read via `mnemosyne-cli query`)
  phase-b-rounds.tsv    round -> Phase B axis it advanced (R1526; `none` + reason
                        is legal). Append one row per round; the tally reports
                        UNDECLARED at push for any round git has and this lacks
vendor/
  sce/                  scxml-core-engine submodule, branch=main
  mnemosyne/            Mnemosyne submodule pinned to `[tool].pin` (R1507) —
                        the gate tool, vendored so a fresh clone can build it
.githooks/              pre-commit, commit-msg, pre-push (active via core.hooksPath)
mnemosyne.toml          workspace config (schema, locale, validators, ledgers)
```

## Working contract

- Build: `cargo check --workspace`, `cargo test --workspace`
- System deps (Linux): `libfontconfig1-dev libxkbcommon-dev libasound2-dev
  libxml2-dev libclang-dev` — ALSA headers are needed because
  `examples/hello-audio-device` enables `pinion-audio/cpal-backend` (it opens a
  real output device; R1310). Depending on `pinion-audio` *without* that feature
  needs no ALSA. The device demo additionally wants a silent card:
  `sudo modprobe snd-dummy`.
  **`libxml2-dev` + `libclang-dev` are for the SCE build (R1688)**: `sce-build`
  build-depends on the `libxml` crate, whose `build.rs` probes
  `pkg-config libxml-2.0` (and **panics** when it is absent) and then runs
  bindgen, which loads libclang. `pinion-forge` build-depends on `sce-build` and
  every example build-depends on `pinion-forge`, so this is on the path of any
  `--workspace` build. Reported by a consumer, and it had been invisible here for
  the reason such things always are: every machine this tree has been built on —
  this one, the build hosts, and GitHub's runner image — already had both for
  other reasons, so only a CLEAN machine ever saw it.
- Lints (workspace-wide): `unsafe_code = "forbid"`, `clippy::pedantic = "warn"`
- All new Rust code lives in `crates/`; no top-level Rust files
- `view-fn` is **sync** (purity invariant per §6.3 — required for dry_run guarantee)
- RPC and IO are **async via tokio** (boundary at `pinion-rpc` server entry per §6.3)
- Commit message style: include section refs (e.g. `impl §5.2 + §5.11 scene primitive enum`)
- **Every round appends one row to `docs/phase-b-rounds.tsv`** declaring which
  Phase B axis it advanced, or `none` with a reason. This is what lets the tally
  register depth work — a round that improves a widget the tree already has
  creates no artifact, and a count of artifacts cannot see it (R1526). Git is
  the census: a round with no row is reported UNDECLARED at every push

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

### The gate tool is resolved, not assumed (R1507)

Both hooks source `.githooks/lib/mnemosyne-tool.sh`, which resolves the
revision `mnemosyne.toml [tool].pin` names and **verifies it** — the resolved
build's `--version` revision must have the pin as a prefix. Order: the
already-built `vendor/mnemosyne` binary (best provenance — its source is
present and its worktree checked), else the installed pin under
`$MN_ROOT/<pin>/bin` (which exists to avoid the *first* build), else
`vendor/mnemosyne` built from source (~1 min, cached), else a loud refusal.

**It never falls back to PATH.** Before R1507 the hooks ran whatever
`mnemosyne-cli` was on PATH and trusted that binary to re-exec the pinned build
itself, which asks the untrusted thing to enforce the trust and has a floor: a
pre-R832 build does not know the `[tool]` key and dies *parsing the config*,
before any hand-off, with `MNEMOSYNE_PIN_SKIP` powerless because that check
runs ahead of the parser too. Measured twice in one week (R1502, R1503) — a
concurrent checkout reinstalled PATH at R807 and every commit and push here was
blocked. A fresh clone had the mirror-image problem: nothing to delegate *to*.
Vendoring closes both.

Bumping the pin moves the submodule too (`git -C vendor/mnemosyne checkout
<pin>` + `git add vendor/mnemosyne`), the same dual discipline `vendor/sce`
has — and since R1508 that pair is **checked on every hook run**, not only when
a build is needed, so `mnemosyne.toml` and the gitlink cannot drift apart
behind a working installed pin.

R1508 also closed a hole in R1507's own check: the revision was matched as a
bare prefix, so `be4c1647-dirty` — Mnemosyne's stamp for "built from modified
sources" — passed as the pin. The revision must now be pure hex, the build must
report the same revision whether or not it is allowed to delegate, and a
vendored build additionally requires a clean submodule worktree (upstream's
stamp derives `-dirty` from git metadata and its own docs say an unstaged edit
can escape it; we are the ones who can watch the worktree). A never-checked-out
submodule is initialised automatically — but never one that already exists,
because `submodule update` would discard local work there.

R1509 then found that R1508's own delegation check could not fail. It compared
`--version` with and without `MNEMOSYNE_PIN_SKIP`, but `--version` is answered
*before* the pin logic — the PATH build here reports its own revision either way
and prints no hand-off note at all — so the check only discriminated against a
synthetic fake modelling behaviour the tool does not have. Delegation is
announced on real commands, so the probe is now a real command (`query
--list-sections`, 39 ms) and the assertion is that the announcement is absent.
The vendored cache is also bounded now (2 GiB; one pinned revision has nothing
worth keeping selectively, so it goes whole and rebuilds).

The libraries are tested (`tools/test_hooks.sh`, 66 assertions) and `pre-push`
runs those tests before trusting them.

`.githooks/pre-commit` runs:

- `mnemosyne-cli validate-workspace` — T1 cross-ref + T2 frozen ledger + atomic-store consistency (store-direct since Mnemosyne R400)
- `cargo clippy --workspace --all-targets --features pinion-runtime/vello` — only when staged files include `*.rs`; enforces the workspace.lints baseline (forbid unsafe / deny warnings / clippy::pedantic deny)

`.githooks/pre-push` repeats both gates unconditionally, so amends / rebases / `--no-verify` bypasses cannot publish a state that fails either check.

If a hook fails, **fix the underlying issue**. Never bypass with `--no-verify` unless the user explicitly requests it.

### Stop-the-line at the push gate (R1495)

`pre-push` refuses to publish onto a base whose **last completed CI run
failed** (`.githooks/lib/ci-status.sh`), and runs the hook libraries' own
tests (`tools/test_hooks.sh`) before trusting them.

- **Why**: stop-the-line is a ratified rule ([[zero-flake-policy]], R882.2),
  and its only enforcement was a sentence in `docs/SEED_PROMPT.md` — a file
  that is `.gitignore`d, so a fresh clone does not have it, read by whoever
  remembers to. R1470 is the case: a red `lint-and-test` kept the
  `needs:`-gated `demo-sweep` from running for **99 consecutive pushes**, and
  the round that found it wrote down "a prose warning is not a gate" while
  leaving this defence as prose.
- **It does not run the tests.** The full suite and the demo sweep are CI's
  job (2026-07-21 directive: local gates cover only the crates a round
  touched). This reads CI's verdict — one `gh` call, no build.
- **Fails open** on a missing `gh`, no network, or no auth — infrastructure
  absence is not evidence of breakage — but always prints, because the
  failure mode it exists to prevent is a check that silently stopped
  happening.
- **Override**: `PINION_PUSH_ON_RED=1 git push`, for publishing the fix. A
  stop-the-line rule with no way to push the fix stops the line permanently.
- **No `--branch` flag**: gh 2.4.0 (what Ubuntu ships, and what is on this
  machine) does not have it, and answers usage text with exit 0 and no rows —
  indistinguishable from "no runs yet". The branch is filtered in the parse.
  The first draft used the flag and would have fail-opened forever; the unit
  tests missed it because the `gh` stub accepted any argument. The stub now
  rejects flags it was not taught.
- **The hook libs are tested** (`tools/test_hooks.sh`, plain bash, runs in
  milliseconds inside `pre-push`). Before R1495 nothing verified any of them —
  including `commit-msg-lint.sh`, which gates every commit message.

### Build-cache budget (R1486, corrected + bounded R1489)

`pre-push` ends by printing `target/`'s size and, when it exceeds a budget,
reclaiming oldest artifacts with `cargo sweep --maxsize` (`.githooks/lib/target-budget.sh`).
Since R1489 that hook is the **trend line, not the bound** — see below.

- **Why**: measured 2026-07-29, `target/` had reached **198 GiB** and the disk
  was 100% full. Nothing ever reported the size, so the growth was invisible
  for ~800 rounds until another session hit the full disk. A sweep reclaimed
  **165 GiB** and incremental builds still ran in seconds — it was all dead
  weight.
- **R1489 correction — the stated cause was wrong.** R1486 said "this hook is
  what grows the cache (unconditional workspace clippy + `cargo doc`)".
  Measured: clippy/doc emit `.rmeta` 2.75 GiB + `.rlib` 5.55 GiB + doc 0.26 GiB,
  and **clippy does not link at all**, while the 27 GiB at `target/debug`'s top
  level is linked binaries. The dominant term is *linking*, from
  `cargo test --workspace` and the demos' release builds.
- **R1489 root cause — the size is structural, not debug-info alone.** One
  example binary is **148 MiB whatever the example contains**: `hello-checkbox`
  (298 LOC) 148.3 MiB vs `hello-node-editor` (8,998 LOC) 155.0 MiB. Each binary
  statically re-links the whole framework and re-embeds its DWARF (`.debug_*` =
  82.6 MiB, **56%**; `strip --strip-debug` takes 148.3 → 65.6 MiB). The
  multiplier is the target structure: 198 `bin` + 25 `lib` + 26 `test` targets,
  and **190 of 197 examples carry `#[cfg(test)] mod tests`**, each producing a
  second linked executable (measured: 54.1 MiB for `hello-checkbox`). So one
  complete workspace test build materialises **~40 GiB** before any staleness;
  198 GiB was that floor times a few generations.
- **R1489 hard bound — `target/` is now a symlink onto a compressed,
  fixed-size volume.** Every project on the machine (`~/.buildcache.btrfs`,
  100 GiB image, `compress=zstd:1`, `nofail` in `/etc/fstab`) holds its
  `target/` there. Measured 3.85x compression on real artifacts, machine-wide
  118 GiB -> 37 GiB, with `cargo check`/`cargo test` timings unchanged
  (0.17s warm, 2.84s after a touch). Because the image size *is* the quota,
  build caches can no longer fill the root filesystem — the 2026-07-29 incident
  becomes impossible rather than late-detected. A user systemd timer
  (`buildcache-sweep.timer`, daily, linger enabled) sweeps **every** project;
  the pre-push hook only ever swept this one, and the repos without it had
  accumulated 66 GiB between them.
- **R1490 — `[profile.dev] split-debuginfo = "unpacked"`.** The DWARF was never
  duplicated at compile time: it lives in the rlibs and the *linker* copies it
  into every binary. Measured on a controlled pair of cold builds of the same 20
  examples (40 executables) into empty target dirs, changing only this setting:
  tree 5.96 -> 4.97 GiB (-16.7%), per executable 101.6 -> 74.6 MiB (-26.6%),
  on disk 1.6 -> 1.4 GiB, `cargo build` 105s -> 91s (-13%). **rlib and rmeta
  were byte-identical**, so nothing was dropped — only the copying stopped. And
  `addr2line` resolves the same file:line in both, so no debugging capability is
  lost; a dev binary is simply only debuggable while its `target/` is intact.
  Release ships no DWARF at all (measured 0.0 MiB), so `dev` is the only profile
  this touches. See the comment on the profile in `Cargo.toml`.
- **The size is printed every push**, over budget or not. That number is the
  fact whose absence caused this; a bound that only speaks when it fires would
  leave the trend unseen. Cost: one `du -sbL`, ~0.12s. **`-L` is load-bearing**
  since R1489: without it `du` measures the symlink (29 bytes) and the gate
  reports `0 GiB` forever. The budget counts *apparent* bytes while the volume
  stores them ~3.9x smaller, so 100 GiB here is ~26 GiB of disk.
- **Size, not age**: what ran out was space. `--time N` reclaims nothing during
  a heavy week and deletes useful artifacts during a quiet one.
- **Dead-toolchain artifacts go first, unconditionally** (R1488, `cargo sweep
  --installed`): a toolchain rustup no longer has cannot build anything, so
  that removal is provably free and is not budget-gated. Measured today it
  reclaims **nothing** — `rust-toolchain.toml` pins an exact `1.88.0`, so the
  project has never rotated toolchains and the 198 GiB was entirely
  same-toolchain accretion. It earns its place when that pin moves.
- **Runs last, after the build gates** — they build, so their artifacts are
  newest and an oldest-first sweep cannot remove them. Verified: an 18 GiB
  sweep left `cargo check -p pinion-rpc --all-targets` at 3.7s.
- Default budget **100 GiB**; override with `PINION_TARGET_BUDGET_GB` (integer
  GiB). Never fails the push — see the lib header for the stated limits
  (timestamp-granularity overshoot; no coordination with a concurrent build).
- Needs `cargo install cargo-sweep`; without it the hook reports and continues.

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
