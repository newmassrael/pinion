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
| **B. Professional GUI** (Qt / Flutter / Compose / React-class) | Multi-window + DCC/IDE/CAD-grade widget catalog + pro-tool performance | **~70%** (all axes) / **~76%** (buildable axes only) — **R1519 re-tally, and the tally is now a tool**: `python3 tools/phase_b_tally.py` counts the evidence, holds each judgment next to the evidence it was made against, and reports STALE when that evidence drifts >25%. The pre-push hook prints it every push (and, since R1522, runs the tool's own `--selftest` first and withholds the numbers if it fails). Do NOT hand-edit these numbers without re-running it. Tree at R1526: 27 crates / 198 examples / 479 demos. Per-axis w×done: DCC 20×85 / **Model-View 16×80 (R1526 re-judged, was 75)** / catalog 16×82 / **charting 10×65 (NEW AXIS)** / rich-text 9×70 / **pro-tool perf 9×60 (R1522 re-judged, was 50)** / OS-native 11×58 [gated] / §7 API 9×30 [gated]. **R1526 — a round declares the axis it advanced, and evidence is snapshotted per kind.** R1522 (below) fixed the perf axis and left the mechanism under it intact, so the same blindness returned on the next axis three rounds later: R1523, R1524 and R1525 each closed a named gap in the Model/View contract and each moved that axis's evidence by **+0%**. The cause is not which artifacts an axis counts — it is that evidence is a **count**, so only work that *creates* an artifact registers, and depth work modifies what already exists (all three edited `hello-grid-*` examples Model/View already owned). perf's `demo-body` probe worked only because an optimisation happens to leave a new demo behind. So the unit is now the project's own unit of work: a round declares its axis in `docs/phase-b-rounds.tsv`, **git — not the ledger — enumerates which rounds exist** (a round with no row is reported UNDECLARED at every push, so forgetting is visible rather than silent), and `none` is a legal declaration that must carry a reason. Two alternatives were measured and rejected: attributing rounds by git *paths* (R1511 and R1513 each touched ~40 examples across six axes, and unlike a false positive in a count that noise does not cancel), and total example *bytes* (verbosity would read as progress, which the R661 LOC baseline exists to prevent). Snapshots are now **per kind** — R1522 summed perf's 4 examples + 7 demos into one `11`, which buries a kind that moves inside one that does not: one depth round against 37 examples is +2.7%, so nine would be needed to cross a threshold the round count crosses at once. **R1522 — an axis declares WHAT its evidence is made of.** R1519 counted example directory names for every axis, which is right for six of the eight (a new widget is a new example) and structurally wrong for perf: an optimisation creates no example, so R1520 and R1521 — each closing the very gap this axis's judgment named — moved its evidence by **+0%**, twice running. Measured while fixing it: the 476 demos were counted in the report header and used for nothing; the perf axis's four patterns were the names of the four examples that existed at R1519, one pattern per match, so it could only grow if a future round were named in advance; demo *names* do not rescue it (63% match no axis, 29% after normalising `_`→`-`); demo *bodies* do, because what an optimisation leaves behind is a demo asserting on a cost counter. Evidence is now a list of `(kind, patterns)`, `example-name` being a **census** (an unmatched example is a finding) and `demo-body` a **probe** (only the axes that declare it look, so an unmatched demo is not). **Why the jump from R931's ~56%**: (a) the number was never re-judged in **587 rounds** while demos went 228→474 — 240 of the 474 demos postdate the tally that described the tree as 56%; (b) the R931 axis set had **no charting axis**, so the entire R1372-R1442 dataviz campaign (22 examples, 72 demos, `pinion-chart` + `pinion-graph`) could not move it by a single point. The jump is NOT re-weighting: scoring today's completions against R931's OWN 7-axis weights gives ~69%, within a point of the 8-axis ~68%. **Buildable vs gated**: OS-native (Mac/Win surfaces need those OSes' runners) and §7 API (deliberately parked — freeze a mature surface, not a churning one) cannot be advanced from here, so 100% of all-axes is unreachable by construction; **~75%** buildable is the number to move. |
| **C. Game engine substrate** (§2 #4 entry) | Immediate-mode game loop ↔ retained widget tree dual; 3D scene graph; asset pipeline; physics; audio; gamepad; PBR | **~15%** (R1519 re-tally). §2 #4 immediate-mode game loop I/O surface + fixed-timestep done R681/R827-R831. **Audio is NOT 0% any more** — `pinion-audio` 5.8k LOC, real cpal device backend + RT thread + RPC wire proof, CI-gated on `snd-dummy` (R1274-R1313); `pinion-asset` started (188 LOC). **3D / physics / gamepad / PBR still 0%.** The prior "audio 0%" text predates R1274 by ~250 rounds. |
| **D. AAA game maker** | Unreal-class editor **self-hosted in pinion**; visual scripting; Nanite/Lumen-class rendering; multiplayer netcode | **0%** |

**True north**: AAA game shippable + Unreal-class editor self-hosted in pinion itself, with AI-introspection 1st-class through every phase.

Current weighted progress against true north: **~28%** (Phase A 97% × 5% phase-weight + Phase B 70% × 25% + Phase C 15% × 35% + Phase D 0% × 35%), **R1519 re-tally, Phase B axes re-judged R1522 (perf) and R1526 (Model/View)**. Figures are soft self-estimates with a per-axis breakdown (do not over-read precision; ±5%). Phase B is now tool-backed (`tools/phase_b_tally.py`, see the table above). **Phase C moved 5% → 15%** on evidence the old figure predates: `pinion-audio` is 5.8k LOC with a real cpal device backend, an RT thread and an RPC wire proof (R1274-R1313), and `pinion-asset` exists (188 LOC — started, not built out); the R931-era text "audio 0%" is simply out of date. 3D / physics / gamepad / PBR remain 0%. **The lesson this re-tally records**: a progress figure with no recorded evidence and no staleness check is not a measurement — it held still for 587 rounds while the tree nearly doubled.

R655-R666 todomvc + R667 settings-panel = Phase A finalisation. R700+ = Phase B entry (multi-window first). R1000+ = Phase C entry (ImmediateModeNode + game loop). R2500+ = Phase D entry (editor self-hosted dogfood).

**Phase B value order (R835 directive — do the highest-value-for-the-northern-star first).** The true north is the Unreal-class editor self-hosted in pinion; Phase B is the substrate that editor is built on, so "high value" = high-weight × low-completion.

**R1519 — the order is now DERIVED, not written down.** `python3 tools/phase_b_tally.py` prints it (`LEVERAGE = weight × remaining`, buildable axes only), so it re-ranks itself when a completion moves. The hand-written order below had gone out of date: it puts DCC first, but on today's numbers the ranking is

> **1. Pro-tool performance (360) · 2. Charting (350) · 3. Model/View at scale (320) · 4. Advanced DCC (300) · 5. Common catalog (288) · 6. Rich-text (270)**

**Do not hand-edit that line — re-run the tool.** It has already changed twice: at R1519 perf led with 450, being the axis that had moved least (43% → 50% in 587 rounds); R1520 and R1521 optimised it and R1522 re-judged it to 60%, dropping it behind Model/View; then R1523-R1525 deepened Model/View and R1526 re-judged that to 80%, putting perf back on top. That is the mechanism working — the order is a function of the completions, so it moves when they do. Note what each re-judgment needed first: an evidence kind that could *see* the work. Both of those rounds were forced by an axis registering **+0%** while real work landed on it. Take the tool's order; the numbered sections below are the per-axis DETAIL, not the priority. Audit-first within each:

1. **Advanced DCC/IDE widgets** — property-grid / inspector panel (typed editable rows), advanced data-grid (cell editors / grouping / frozen panes), node-graph editor substrate (visual scripting / material graph). **~85% done** (R1519 re-tally; R931 said 73% — node *evaluation* has since LANDED [R1255-R1264], plus the R1087-R1173 dock campaign [34 demos], tree reparent, column reorder/visibility, header menus and inspector depth. R931 detail — property-grid ~80 [R921 struct-tree + R922 multi-object inspector + R931 array/Vec element editing] / editable data-grid ~90 [R930 dynamic add/remove rows] / node-editor ~85 [R929 edge reconnection], all dogfood-verified; remaining: node *evaluation* [Phase C], advanced delegates / per-element modified-reset). R935 added tree drag-to-reparent (scene outliner: `hello-tree-reparent`, drag onto=child / between=reorder, cycle-guarded) + lifted `remove_subtree`/`insert_subtree` to `tree_nav`. **Edit-machinery crate-lift is audited-PREMATURE (R935): `edit_field_keymap`/`UndoStack`/`CellValue`/`TreeNode` already lifted; the per-widget commit/edit-latch/keymap code is divergent domain logic, not missed abstraction — do not chase a wholesale property-grid/data-grid/node-editor crate extraction.** Leverage now = depth + new gestures, not crate extraction.
2. **Model/View at scale** — the large-data backbone (asset browser / scene outliner: 10k+ rows, unified sort/filter/group). **~80% done** (**R1526 re-judged**, forced by the round ledger taking this axis's declared rounds 0 → 3; R1519 said 75%, R931 68%. What moved it is the core of Qt's `QAbstractItemModel` data path: **R1523** windows the column axis as well as the row axis (200 → 5 cells a row), **R1524** makes the contract per-cell rather than per-row (`data(QModelIndex)`; 2400 → 84 cells asked a frame), **R1525** makes the painted string the one the ordering read. Deliberately not higher — **header data is still per-slice** (the binding takes `headers: &[&str]` for all 200 columns where Qt's `headerData` is per-section) and **`cell` returns a String with no role dimension** (Qt's Display/Edit/Decoration/ToolTip roles). Older detail: R1519 credited the sprag/tide consumer work adding paged-stream / variable-list / measured-list / tail-reveal / streaming-log / async-data; R931 said 68% — since then the sprag/tide consumer work added paged-stream / variable-list / measured-list / tail-reveal / streaming-log / async-data. R931 detail — windowing [list/grid/tree] + 3 orthogonally-composable proxies [sort/filter/group/tree-filter] + data-indexed selection + **async/lazy + LRU million-row now landed**: R923 paged `Resource` view + R924 virtualized lazy-load infinite-scroll + R927 out-of-memory source-side sort/filter via `ResourceCache` + R934 LRU-bounded `ResourceCache::with_capacity` (1M-row `hello-million-row`: bounded memory + scroll-back eviction witness); **no unified data layer** [deliberate, R780/R821 4th-consumer gate]).
3. **Pro-tool performance** — 60fps with large scenes; profiling. **~60%** (**R1522 re-judged**; R1519 said 50%, R931 43%. What moved it: R1519's stated reason for 50% was "measure-first infra mature — R907 `scene/frame_timings` + R925 frame-budget jank profiler — **measured large-scene hot-path opt 0**", and that 0 is now 2, both with recorded before/after and a *deterministic counter* guard rather than a wall-clock one: **R1520** the §5.16 paint fragment cache survives a scroll (scroll-frame encode 1360µs→42µs, cacheable fragments 4→36), **R1521** the §5.36 shape cache grows into its working set (a fixed LRU hits **0%** on cyclic paint access once the cycle exceeds capacity — 1200 leaves 27.4ms→1.59ms). Still absent, and the reason this is 60 and not higher: **no GPU-timestamp render time**, **no large-scene 60fps end-to-end measurement**, and the paint walk's **2.6µs per text node** (glyph-run walk + `draw_glyphs` encoding), which R1521 left as the dominant term. Note this axis is *also* where R1522 fixed the tally's own blindness — see the Phase B table: two rounds of exactly this work had registered as +0%, because the evidence proxy counted examples and an optimisation makes none.
4. **OS-native maturity** [**GATED** — Mac/Win surfaces need those OSes' CI runners] — finish file-dialog / print / drag-drop / tray. **~58%** (R1519 re-tally; R931 said 48% — tray SNI + its CI coverage and file-drop window identity landed since. (file-dialog / clipboard / drag-drop / prefers-color-scheme + **scene→vector PDF render [R908] + vector-PDF print spool [R911] + CUPS spool [R833] all Linux-verified** done; native-menu rendered-not-native [Mac NSMenu / Win HMENU need those OSes], no Win/Mac native print dialog, **Linux tray SNI substrate + real D-Bus bridge LANDED R949/R950** (`pinion-platform-tray` = pure ksni/SNI StatusNotifierItem, no gtk — sidesteps the gtk-tray-crate/winit incompat; `InMemoryTrayBackend` headless fallback + `hello-tray` + `r949_tray.py`; R932.2 was the DESIGN round, R949/R950 the IMPL — prior "tray 0%" tallies predate them). Tray CI-coverage gap CLOSED R1267: the headless session-bus **test-watcher fixture** (a launcher re-execs an ignored inner scenario under `dbus-run-session` — ksni hard-wires the session bus and `unsafe_code=forbid` blocks `set_var`, so the private bus is provisioned in the child env — hosting an in-process `org.kde.StatusNotifierWatcher` test-double that asserts SNI-register + dbusmenu-export, ZERO-FLAKE, no panel pixels) landed and CI-covers `pinion-platform-tray`'s SNI integration path [was `#[ignore]d` = zero CI coverage]; `docs/cross-platform-native-strategy.md` staged-path step ① "Verification harness" satisfied). The remaining real blocker is Mac/Win-specific native surfaces (need those OSes' CI runners), NOT Linux tray — PDF/print/file-dialog/clipboard/DnD all Linux-native + verified, and the tray capability itself is built.
5. **API stabilisation (§7)** [**GATED** — deliberately parked] — LATER: freeze a *mature* surface, not a churning one. **~30%** (R1519 re-tally; R931 said 15% — the R1469-R1487 surface-identity / exposure / answer-origin series matured the surface substantially without freezing it).
6. **rich-text editing/selection** — code-editor-grade text. **~70%** (R1519 re-tally; R931 said 58% — code folding, CJK/TUI text, the TextGrid cursor series R1419-R1428 and the app-font axis landed since. (single+multi-line+IME+selection+caret + **R903 find-replace + R904 syntax highlighting + R926 matching-bracket + R928 styled-run formatting undo** now landed; full styled-run *text-editing* depth / code-folding still partial).

**7. Charting / data visualisation — ~65%, THE AXIS THAT DID NOT EXIST.** `pinion-chart` + `pinion-graph`, 22 examples, 72 demos (R1372-R1442): line/scatter/donut/treemap/heatmap/histogram + brushing, cross-filter, linked legend, continuous colour scales with WCAG lift, Brandes-Köpf graph layout. Qt ships QtCharts, so this is Phase B under [[qt-parity-over-yagni]] — but the R931 axis set had no slot for it, so ~15% of the tree could not register as progress. Remaining vs QtCharts: polar / candlestick / 3D surface, and chart *interaction* depth.

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
