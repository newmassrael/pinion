# pinion seed prompt — 매 세션 첫 입력

> **R683.B land (2026-05-27, commit `aaf854f`)** — 4-axis paint-pipeline rewrite series Round 4 of 4, atomics 2+3 (Splitter + DockPanel widget substrate) on top of R683.A's window-lifecycle substrate. Per the R670.A/B + R682.A/B precedent the round splits across sessions; R683 is the first round to genuinely need a 3-session split (window-lifecycle + widget substrate + consumer/demo) because both axes (substrate + consumer) are heavier than the precedent rounds. **R683.B atomic 2** — `pinion-widget-paint::splitter` new module: `SplitterOrientation::{Horizontal, Vertical}` (maps to FlexDirection::{Row, Column}). `SplitterStyle::m3_default(orientation, tag)` (4-px M3 handle + 0.05/0.95 ratio clamps + tag for InputRouter routing). `view_splitter(left, right, ratio_signal, theme, style, dragging) -> Scene` builds 3-child Container[tag] (left wrapper `flex_grow=ratio`, handle fixed-size M3 Outline fill, right wrapper `flex_grow=1-ratio`); `handle_fill_for_dragging` lerps toward `OnSurface` 0.16 on drag. `SplitterExternal::new(orientation).attach_ratio(Rc<Signal<f32>>).attach_bounds(min, max)` builder chain. `wants_pointer_capture = true` so cursor stays pinned past clamp edges. `pointer_move(x_rel, y_rel)` first-call calibrates `SplitterDragStart {cursor_fraction, ratio_at_press}`; subsequent calls compute `clamp(ratio_at_press + (cursor_fraction_now - cursor_fraction_at_press), min, max)` and dispatch `Signal::set`. PointerUp/PointerCancel via `invoke("send", Text("PointerUp"))` channel clears drag state. Orientation-axis selection: Horizontal uses x_rel, Vertical uses y_rel. ExternalIntrospect surfaces `orientation` / `ratio` / `dragging` / `send` slots. **R683.B atomic 3** — `pinion-widget-paint::dock` new module: `DockPanelStyle::m3_default(tag)` (28-px header + 0.5 tear-off threshold + 12-px font). `view_dock_panel(title, content, theme, style) -> Scene` builds Container[panel_tag] with 2 children — header tagged `{panel_tag}#header` (M3 SurfaceContainerHigh fill + title text + 8-px padding) + content wrapper tagged `{panel_tag}#content` (`flex_grow=1`). `composite_tag` helper for the R51.42 `{tag}#{suffix}` convention. `DockPanelExternal::new(panel_id, threshold_frac)` with `panel_id: Cow<'static, str>` for runtime ids + threshold matching style. `wants_pointer_capture = true`. `pointer_move` first-call calibrates `DockDragStart {cursor_x, cursor_y}`; subsequent calls compute L∞ norm `max(|Δx|, |Δy|)` and on first threshold crossing fires single `tear_off` intent (`TEAR_OFF_EVENT = "tear_off"`) with `panel_id` payload via `pending_intents: RefCell<VecDeque<Intent>>` queue. `fired_for_drag: Cell<bool>` guards against multi-fire (continued drag past threshold would otherwise push N+1 WindowSpecs per drag). PointerUp/PointerCancel clears both drag state + fired guard. `External::is_dirty` + `drain_intents` wire the intent queue to the framework's per-frame drain. ExternalIntrospect surfaces `panel_id` / `tear_off_threshold_frac` / `dragging` / `tear_off_fired` / `send` slots. **R683.B verification**: +44 net workspace tests (R683.A baseline 3800 → R683.B 3844): 22 splitter (paint shape + flex_grow + drag calibration + clamps + orientation isolation + introspect) + 22 dock (paint shape + composite tags + drag calibration + single-fire guard + L∞ diagonal + introspect). All 3844 PASS, 0 FAIL. cargo clippy --workspace --all-targets clean. 43-demo regression sweep PASS deterministic (widget substrate is additive — no pre-existing binding consumes splitter or dock so the substrate land cannot regress paint behaviour). Mnemosyne entry `R683.B` (atomic ledger 550 → 551; T1 orphan total 0 new; impact_refs [5.16, 5.41, 5.15]). **R683.C is the next session** — atomic 4 (`examples/hello-dock-panels` first consumer binding + R679 DevTools cascade closure: `MainWindowClickRouter` substrate lift via Rule-of-Three) + atomic 5 (R683 demo + 44-demo regression sweep ×3 + commit + Mnemosyne `R683.C` entry + SEED handoff).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 R683.C plan 절 (atomic 4+5 — hello-dock-panels first consumer + R679 DevTools cascade closure + demo + sweep + Mnemosyne) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙. 매 atomic 종료 시 cargo test + clippy + 44-demo regression sweep 검증.
>
> R683.B 가중 진척: Phase A 97% + Phase B 25% × ~46% + Phase C 35% × ~6% (Splitter + DockPanel substrate land — backend-agnostic widget primitives ready for first-consumer binding; the pro-tool authoring tooling chain DCC/IDE/CAD all share this substrate; tear-off intent emission via L∞ threshold detection mirrors VSCode/JetBrains pane feel) = 북극성 가중 **~26-28%**. R683.C 완료 시 ~30-34% (Phase B ~60% — first dock-panel consumer binding live + R679 DevTools cascade closure substrate lift; Phase C entry chain advanced).
>
> **R683.A land (2026-05-27, commit `c0b853d`)** — 4-axis paint-pipeline rewrite series Round 4 of 4, atomics 0+1 (window-lifecycle substrate). Per the R670.A/B + R682.A/B precedent the round splits across sessions: R683.A = substrate (runtime `Signal<Vec<WindowSpec>>` opt-in + AppShell reconcile-diff Effect + per-window state drain primitives); R683.B = atomics 2+3+4+5 (Splitter + DockSurface widgets + hello-dock-panels first consumer + R679 DevTools cascade closure + demo + 44-demo sweep + Mnemosyne). **R683.A atomic 0** — `pinion-shell::WindowSpec.id: &'static str` → `Cow<'static, str>` so the dock + tear-off arc can mint runtime ids (`Cow::Owned(format!("torn-panel-{n}"))`) alongside canonical static literals (`Cow::Borrowed("main")` / `Cow::Borrowed("inspector")`). `#[non_exhaustive]` for future additive fields. Derives `PartialEq + Eq + serde::Serialize + serde::Deserialize` so `Signal<Vec<WindowSpec>>` satisfies the R26 + R36 §5.31 `T: Clone + PartialEq + Serialize + DeserializeOwned + 'static` bound (`SizeStrategy` derives `Serialize + Deserialize` too). `WindowSpec::main` keeps `impl Into<String>` title API (unchanged); `WindowSpec::new` accepts `impl Into<Cow<'static, str>>` for id so string literals + runtime-generated `String`s both coerce. New `WidgetView::windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>>` opt-in trait method with default `None`; pre-R683 single + multi-window bindings (15+ in the example gallery) inherit unchanged and the compile-time `V::windows()` path stays the source of truth. Bindings that opt in (canonically the dock + tear-off arc R683.B brings) return `Some(Rc<Signal<..>>)` memoised via `Owner::cache` — shell wraps the trait call in `root_owner.run` so the binding impl can reach `Owner::current()` + use the typed-key cache slot. **R683.A atomic 1** — three new `AppShell` fields (`windows_signal: Option<Rc<Signal<..>>>` cached signal handle / `reconcile_effect: Option<Effect>` lifetime anchor / `last_known_specs: Rc<RefCell<Vec<WindowSpec>>>` diff baseline). New `AppEvent::WindowsDirty` user-event variant. `AppShell::install_reconcile_effect` runs inside `root_owner.run(|| Effect::new(&Owner::current(), move || { signal.get(); proxy.send_event(AppEvent::WindowsDirty); }))` so each value-changing `Signal::set` (R26 equality-skip on `Vec<WindowSpec>` element-wise PartialEq) fires the Effect closure once + wakes the shell through the existing winit user-event channel. `AppShell::reconcile_windows(event_loop)` reads `signal.get()`, diffs against `last_known_specs` (`Vec` PartialEq short-circuit on identical re-emits — Effect's eager initial run no-ops), drops removed specs via `WindowSlot` map removal (releases the `Arc<Window>` so winit closes the OS window) + `ShellCore::remove_window` cleanup, adds new specs via existing `resume_spec` helper, then `request_redraw` + `drain_redraw_to_winit` so the next event-loop iteration paints the new topology. New `ShellCore::remove_window(&str) -> bool` drains the four per-window HashMaps lifted since R680/R681/R682 (`redraw_requested_per_window` / `last_paint_instants` / `target_fps_per_window` / `fragment_cache_stats_per_window`) + forwards into new `pinion-runtime::CoreShell::remove_window` which removes the `routers` + `window_owners` entries + detaches the secondary `Owner` from `root_owner.children` via new `pinion-core::reactive::Owner::detach_child_by_id` primitive. The detach is load-bearing: without it `parent.children: Vec<Owner>` permanently retained every R680-secondary scope until binding shutdown — animations / commands / cache slots registered on a torn-down per-window scope would survive across reconcile passes. Closes a [[substrate-incompleteness-signal]] R680 left because R680 only added per-window scope; R683 needs the inverse. Both `remove_window` paths refuse `DEFAULT_WINDOW` (primary scope is a `root_owner` alias; removing would orphan the binding's reactive substrate). `WindowSlot.spec_id` + `spec_id_to_window_id` HashMap key cascade to `Cow<'static, str>` (Cow's `Deref<Target = str>` covers every downstream `spec_id: &str` parameter site, so the cascade is type-signature surgery only — no semantic change to substrate dispatch). **R683.A verification**: +20 net workspace tests (R682.B baseline 3780 → R683.A 3800): 10 `pinion-shell::lib` (4 WindowSpec Cow + serde + PartialEq round-trip + 3 Signal<Vec<WindowSpec>> constructible/set/short-circuit + 2 windows_signal default-None & Owner::cache memoisation + 1 partial_eq_field_by_field), 6 `pinion-runtime::core_shell::tests` (remove_window refuses DEFAULT / drops secondary scope / unknown id no-op / drops registered animations / idempotent on double call / sibling-secondary isolation), 4 `pinion-shell::tests::dispatch_core::r683_remove_window_shell_side` (shell-side remove_window refuses DEFAULT + drains all four maps + unknown id no-op + sibling isolation). cargo clippy --workspace --all-targets clean under workspace `-D pedantic`. **43-demo regression sweep PASS deterministic ×3 consecutive runs** (R660-R682 + R683.A; per-run wall time 2.4-5.3s; sweep total ~110s per pass). Multi-window cascade verification — hello_multi_window_r670b 2.58s / r672_multi_window_race_free 2.93s / r680_per_window_owner_scope 2.40s / r682_dirty_subtree_cache 5.22s all bit-identical post-Cow/serde lift. Mnemosyne entry `R683.A` (atomic ledger entries 549 → 550; T1 orphan total 0 new; impact_refs [5.16, 5.41]; round-trip 1/1 + GENERATED.md sync). **R683.B is the next session** — atomics 2+3+4+5 land Splitter widget + DockSurface widget + hello-dock-panels first consumer + R679 DevTools cascade closure (`MainWindowClickRouter` substrate lift into `pinion_widget_paint::devtools` via Rule-of-Three) + R683 demo + 44-demo sweep + Mnemosyne `R683.B`.
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 R683.B plan 절 (atomics 2-5 — Splitter + DockSurface + hello-dock-panels + demo + sweep + Mnemosyne) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙. 매 atomic 종료 시 cargo test + clippy + 43-demo regression sweep 검증.
>
> R683.A 가중 진척: Phase A 97% + Phase B 25% × ~40% + Phase C 35% × ~6% (window-lifecycle substrate land — `Signal<Vec<WindowSpec>>` reactive primitive + reconcile diff Effect + per-window scope drain enable dock + tear-off; substrate-incompleteness signal R680 left structurally closed via `Owner::detach_child_by_id`; Phase B multi-window pro-tool authoring ground 1 step closer) = 북극성 가중 **~25-27%**. R683.B 완료 시 ~30-34% (Phase B ~60% — dock-panel widget catalog + first dock consumer; Phase C entry ready).
>
> **R682 land (2026-05-27 close, R682.A `8d01de1` + R682.B `9b40672`)** — 4-axis paint-pipeline rewrite series Round 3 of 4 (axis 4 substrate, full 5-atomic stack). Per the R670.A/B precedent the round closed across two sessions: R682.A substrate (atomics 0-3) + R682.B consumer + RPC + demo (atomics 4-5). R682 in total covers the full §5.16 paint-fragment cache contract end-to-end (per-Container structural hash + per-window Vello fragment cache + mark-and-sweep eviction + damage rect propagation + GUI-agnostic stats publish/getter pair + scene/cache_stats RPC surface + first 100-row stress consumer). **R682.A atomic 0** — `pinion-core::ContainerNode` gains `paint_hash: Cell<Option<u64>>` per-paint-pass memoised structural hash + `Scene::paint_hash()` recursive over every variant + `Scene::is_cacheable_for_paint()` cacheability predicate; `External` / `ImmediateModeNode` return `PAINT_HASH_UNCACHEABLE` sentinel, `Effect` returns distinct `PAINT_HASH_EFFECT_SENTINEL`; hash excludes paint-irrelevant fields (`tag` / `aria_label`) and covers `rect` / `style` / `layout` / child-hash recursion in declaration order; `Rect` / `Size` / `SizeValue` derive `Eq + Hash`, `LayoutStyle` manual `Hash` impl via `f32::to_bits` for `flex_grow`. **R682.A atomic 1** — `pinion-runtime::paint_adapter::FragmentCache` + `to_vello_cached(scene, fill_hook, text_cache, fragment_cache, out)` brackets the encoder walk with `begin_paint` / `end_paint` so unreached entries evict at sweep; cache probe gates at each `Scene::Container` boundary reached under accumulated `Affine::IDENTITY` AND `is_cacheable_for_paint() == true`; non-cacheable Containers recurse via `to_vello_cached_inner` so nested cacheable subtrees still participate (§2 #4 killer use case — immediate-mode sibling triggers `V::view` re-runs every paint but retained widget subtree skips re-encoding); `Scroll` threads non-IDENTITY transform (descendants skip cache; R682+1 transform-invariant key carry). `WindowSlot` owns `FragmentCache` per slot; `AppShell::render_window` calls `to_vello_cached`. **R682.A atomic 2** — `Rect::union` + `FragmentCache.damage_acc_this_paint` + `end_paint` publishes accumulator into `last_damage_region`; getter returns most-recent-paint damage region (`None` when 100% cache hit). Production GPU consumer (`wgpu::SurfaceTexture` partial-blit / winit damage-rect coordination) is a future-round carry. **R682.A atomic 3** — GUI-agnostic `FragmentCacheStats { hits, misses, paint_count, entries, last_damage_region }` with `hit_rate()` helper + `ShellCore::publish_fragment_cache_stats` + `fragment_cache_stats_for_window` + `fragment_cache_stat_windows` per-window observability pair; `AppShell::render_window` captures per-WindowSlot snapshot post-paint and publishes via `spec_id` key. **R682.B atomic 4** — `FragmentCacheStats` lifted out of `pinion-shell::substrate` into new non-vello-gated `pinion-runtime::paint_cache_stats` submodule (substrate-incompleteness signal: `pinion-rpc` is a peer crate without the `vello` feature, so the original R682.A location forced cyclic-dep avoidance via a parallel type; the lift to `pinion-runtime::paint_cache_stats` resolves the structural cycle textbook-cleanly + pinion-shell keeps the re-export so existing `pinion_shell::FragmentCacheStats` consumers stay bit-identical). `pinion-rpc::cache_stats` new module — `CacheStatsOutcome { hits, misses, paint_count, entries, hit_rate, last_damage_region }` + `cache_stats(stats)` projector + `CacheStatsError::CacheStatsUnavailable` typed surface. `DispatchContext.fragment_cache_stats: Option<FragmentCacheStats>` field + `with_fragment_cache_stats` builder + `scene/cache_stats` dispatch arm (read-only). `pinion-shell::ShellCore::dispatch_rpc_inner` pre-resolves per-window stats via `fragment_cache_stats_for_window(window_id.unwrap_or(DEFAULT_WINDOW))` and threads into the `DispatchContext`. `examples/todomvc::PINION_TODOMVC_SEED_N` env var seeds N synthetic `TodoItem` rows (1..=10_000) on first paint when the post-hydration `todos` signal is empty; deterministic completion pattern (`i % 3 == 0 → completed`); pure `parse_seed_n_env` / `seed_todo_row` / `build_seed_rows` helpers exposed `pub(crate)` so binding tests pin the env-parse matrix + builder contracts without invoking the full `use_persistence_boot` arc. 2nd consumer of `FragmentCacheStats` per [[abstraction-needs-second-consumer]] (1st = R682.A `pinion-shell::tests::dispatch_core::r682_fragment_cache_stats_substrate` substrate suite). **R682.B atomic 5** — `tools/demos/r682_dirty_subtree_cache.py` 31 assertion statements across 8 sections (A substrate sanity + RPC field type validation / B 100-row warmup matrix / C steady-state hit_rate convergence / D filter-driven eviction / E damage region semantics / F immediate-mode coexistence carry-back / G full-hit no-damage stability / H paint_count monotonic). PINION_TODOMVC_SEED_N=100 + isolated PINION_STORAGE_DIR per-demo. `_drive_redraw` helper injects benign `scene/click` at (4, 4) so the InputRouter arms `request_redraw` → `AppShell::render_window` runs → `FragmentCache.end_paint` advances (`scene/snapshot` calls the paint producer but does NOT itself go through `to_vello_cached`; the cache only advances on actual paint cycles dispatched by winit's `RedrawRequested`). **R682 cumulative verification**: +135 net workspace tests across both sessions (R681 baseline 3697 → R682.B end 3780); cargo clippy clean under workspace `-D pedantic`; 43-demo regression sweep PASS deterministic across 3 consecutive runs (R660-R681 + R682). Mnemosyne entries `R682.A` (impact_refs [5.16, 5.41]; §5.46 audit-half typo registered as orphan_ledger row in mnemosyne.toml) + `R682.B` (impact_refs [5.16, 5.41]; publishable half clean). **R683 is the next round** — 4-axis paint-pipeline rewrite series Round 4 of 4 (axis 1: runtime `Signal<Vec<WindowSpec>>` window lifecycle + Splitter widget + DockSurface dock UX + first dock consumer + R679 DevTools 2nd binding consumer Rule-of-Three trigger).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 R683 plan 절 (axis 1 dynamic dock — runtime window lifecycle + Splitter + DockSurface + hello-dock-panels + R679 DevTools 2nd consumer Rule-of-Three) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지. 매 atomic 종료 시 cargo test + clippy + 43-demo regression sweep 검증 후 다음 atomic 진입.
>
> R682 가중 진척: Phase A 97% + Phase B 25% × ~37% + Phase C 35% × ~6% (4-axis paint-pipeline rewrite series Round 3 of 4 완전 종료; §5.16 dirty-subtree cache contract end-to-end (구조 해시 + Vello fragment cache + 마크-앤-스윕 eviction + damage rect + 관찰성 stats + RPC method + 100-row 실측 consumer); §2 #4 dual-execution model immediate-mode coexistence 비용 실측 확인 가능; Phase C entry prerequisite chain 3/4 advanced) = 북극성 가중 **~24-26%**. R683 dock 시리즈 완료 시 ~30-34% (Phase B ~60% — multi-window pro-tool authoring 정통 ground; Phase C entry ready for game-engine substrate).

【R683.C plan】next session entry — **4-axis paint pipeline rewrite series Round 4 of 4, atomics 4+5** (hello-dock-panels first consumer + R679 DevTools cascade closure + demo + 44-demo sweep). Single commit, 2 atomic on the R683.A `c0b853d` window-lifecycle substrate + R683.B `aaf854f` widget substrate.

(3) **`pinion-widget-paint::dock::DockSurface` widget** — `Scene::Container` with N child panels arranged in dock layout (`DockSlot::{Left, Right, Top, Bottom, Center}` + nesting). Each panel carries a header strip (draggable for tear-off). Tear-off detection: PointerDown on header + Drag past threshold → emit `tear_off` intent carrying the panel's spec id. Binding catches the intent + pushes a new `WindowSpec` onto its `Signal<Vec<WindowSpec>>` (axis 1 + axis 3 + R680 integration). 12-15 unit tests pin dock topology + tear-off detection + clamp behaviour.

(4) **`examples/hello-dock-panels` first consumer binding** — 3 panels: DevTools inspector tree (R678 substrate consumer) + property pane (R677) + viewport. Default dock topology: inspector left / property bottom-left / viewport center. Drag inspector header past 100px threshold → tear-off into new WindowSpec → 2nd window appears with inspector content. R679 DevTools cascade closure: this binding is the 2nd DevTools binding consumer + triggers `MainWindowClickRouter` substrate lift into `pinion_widget_paint::devtools` via Rule-of-Three (hello-multi-window inspector = 1st, hello-dock-panels = 2nd). honest LOC: ~+1500-2200 (binding density on par with hello-multi-window).

(5) **R683 demo (`tools/demos/r683_dock_tear_off.py`)** — ≥40 assertions across A-J sections: substrate sanity / dock topology RPC introspection / splitter drag ratio mutation / panel tear-off → new window appears / dock-back (drag back into a dock slot) → window closes + panel reattaches / cross-window selected_path sync (R679 carry-back) / inspector property pane drives selection on tear-off window same as docked window / Signal-driven window list diff round-trip / multi-tear-off cascade (3 panels each torn off → 4-window state).

(6) **44-demo regression sweep ×3 + commit + Mnemosyne consolidation + SEED handoff**. Commit `feat(widget-paint): R683 §5.16 §5.41 §5.45 dock + splitter + window lifecycle`. Mnemosyne `entry_id = R683` + impact_refs `[5.16, 5.41, 5.45]` + R682 carry (transform-aware fragment cache key, wgpu damage consumer) carried forward + R684 axis (next prerequisite — probably the wgpu damage consumer or transform-invariant cache key).

honest LOC 예측: **R683 = +3000-4500 net** (per the 4-axis series Round 4 estimate; largest round — Splitter widget + DockSurface + tear-off UX + first dock consumer + reconcile Effect + 6-8 atomic substrate + binding + demo).

R683 후 가중 진척: ~24-26% → **~30-34%** (Phase B 25% × ~60% — multi-window pro-tool authoring 정통 ground 도달; Phase C 35% × ~10% — paint pipeline ready for game-engine substrate). Phase C entry (game-loop / 3D scene graph / asset pipeline / physics / audio / PBR) 가 R&lt;700-900 range&gt;에서 자연 도래 — R683 이후 즉시 진입 가능.

**R683 verification mandatory** (라운드 끝):
- 44-demo regression sweep PASS deterministic 3 consecutive runs (R660-R682 + R683)
- DevTools 2nd binding consumer + MainWindowClickRouter substrate lift (R679 cascade closure)
- Tear-off → new window → drag-back → panel reattach round-trip
- Splitter drag mutates ratio Signal + ratio mutation propagates to layout (bidirectional)
- backward-compat — 단일 윈도우 binding 18개 + R670.B / R681 multi-window binding 회귀 0
- 부채 surface 정직 받아들임 — transform-aware fragment cache key carry (R682+1), wgpu surface invalidation consumer carry, Phase C entry prerequisite list update

**R682 verification (라운드 끝, completed)**:
- ✓ 43-demo regression sweep PASS deterministic (R660-R681 + R682; 3 consecutive sweeps)
- ✓ R682.A: 23 atomic-0 unit tests (paint_hash determinism / mutation propagation / sentinel distinctness / Scroll recursion / Cell memoisation) + 13 atomic-1 (cache hit/miss/sweep / nested-cacheable-under-uncacheable / clear) + 8 atomic-2 (Rect::union semantics / damage region first-paint + reset + multi-miss union) + 8 atomic-3 (publish round-trip / per-window independence / latest-wins / hit_rate math)
- ✓ R682.B: 7 cache_stats lib tests (None error / zero / counter mirror / damage rect / serde elision / serde emission / usize saturation) + 4 paint_cache_stats lib (zero / ratio / default / Copy) + 6 paint_adapter stress matrix (first-paint N rows / 7-paint steady-state hit_rate ≥ 0.85 / filter-change eviction / full-hit no-damage / stats mirror / paint_count monotonic) + 4 dispatch_core RPC dispatch (unavailable / counter wire / damage region wire / default window) + 9 todomvc binding (env var parse matrix incl whitespace + zero + overflow + malformed / build_seed_rows / completion stride / one-indexed text / stride truth table / view fn N row count / strikethrough composition)
- ✓ tools/demos/r682_dirty_subtree_cache.py PASS in 5.23s (31 assertions across 8 sections)
- ✓ Mnemosyne entry R682.A + R682.B clean validate (T1 reject=0, GENERATED.md sync, atomic ledger entries=549 / sections=61 / orphan_refs=5+0)
- ✓ 부채 surface 정직 받아들임 — R683 axis 1 dynamic dock substrate carry; transform-aware fragment cache key carry (Scroll-content + reflow cache hits); wgpu surface invalidation consumer carry (damage region published but no GPU upload-path consumer yet); R679 DevTools 2nd binding consumer + MainWindowClickRouter substrate lift carry (lands naturally in R683 hello-dock-panels via Rule-of-Three)

> **R681 land (2026-05-27, commit `50434a9`)** — 4-axis paint-pipeline rewrite series Round 2 of 4 (axis 2 substrate: `Scene::ImmediateModeNode` + `ImmediateMode` trait + `ImmediatePainter` backend-agnostic primitive surface + per-window `ControlFlow::WaitUntil` game-loop pacing + first real consumer binding `hello-immediate-mode-canvas`). New `Scene::ImmediateModeNode(ImmediateModeNode)` variant (`Rc<RefCell<dyn ImmediateMode>>` driver + `viewport: Rect` + `layout: LayoutStyle` + `tag: Option<Cow>` + `last_dt: Cell<Duration>` sidecar). `ImmediateMode` trait — dyn-safe `Debug` super-trait + `tick(&mut self, Duration)` default no-op + `paint(&mut self, &mut dyn ImmediatePainter)` default no-op + `introspect` echo of `External`. `ImmediatePainter` backend-agnostic primitive surface (`viewport_size` + `dpi_scale` + `clear` + `fill_rect` + `fill_triangle` + `stroke_line` — HTML Canvas / Cairo / Direct2D pattern). `Scene::tick_immediate_mode(Duration) -> usize` walker + `Scene::has_immediate_mode_subtree() -> bool` predicate. `pinion_runtime::paint_adapter::VelloImmediatePainter` Vello-backed bridge wrapping `&mut vello::Scene` + composed `parent_transform * Affine::translate(viewport.{x,y})` for viewport-local coordinates. `paint_adapter::to_vello_inner` `Scene::ImmediateModeNode` branch dispatching driver paint. `pinion_runtime::layout` extended (`layout_style_of` + `assign_rect`) so taffy resolves immediate viewport against parent flex. `pinion_runtime::frame_pacing` extended with `WindowFramePolicy { Idle | Polled { fps } }` enum + `frame_budget(self) -> Option<Duration>` + `DEFAULT_IMMEDIATE_MODE_FPS = 60` + `default_window_frame_policy` + `frame_budget_for_window(has_immediate, override_fps)` facade. `pinion-shell::ShellCore` adds `target_fps_per_window: HashMap<String, u32>` + `set_target_fps_for_window` / `target_fps_for_window` getter pair + `last_paint_instant_for_window` public read for the about-to-wait deadline computation. `pinion-shell` substrate `compute_paint_scene_internal` post-layout immediate-mode tick walker — `Duration::from_secs_f32(dt) → paint_scene.tick_immediate_mode(dt) → if count>0 arm redraw_requested + request_redraw_for_window(window_key)`. `pinion-shell::AppShell.WindowSlot.has_immediate_mode_subtree: bool` sticky post-paint flag + `ApplicationHandler::about_to_wait` override — for each active slot with the flag set, derive `WindowFramePolicy` (override fps or 60fps default), compute per-window `last_paint_instant + budget`, take earliest across slots, set `ControlFlow::WaitUntil(earliest)` or fallback to `ControlFlow::Wait`. Re-arm per-window redraw flag + drain so the next event-loop iteration dispatches `Window::request_redraw` — game-loop pacing on top of R680's per-window paint clock. `SnapshotNode::ImmediateModeNode(ImmediateModeSnapshot { tag, viewport, last_dt_micros })` + `pinion-rpc::dispatch::snapshot_node_to_json` wire serialization (`type: "ImmediateModeNode"` + `tag` + `viewport` + `rect` alias for generic harness helpers + `last_dt_micros`). `examples/hello-immediate-mode-canvas` (24th binding) — `RotatingTriangleDriver` impl `ImmediateMode` (tick advances `angle += dt * 1.5π rad/s` mod TAU; paint emits a `painter.fill_triangle` at viewport centre with three vertices rotated by `angle` and M3 `Accent` fill). `use_canvas_driver()` `Owner::cache` hook keeps the same `Rc<RefCell<RotatingTriangleDriver>>` across view-fn re-runs. View composes retained header `Text` + hint `Text` + `Scene::ImmediateModeNode` + Dismiss `Button` — §2 #4 dual-execution model's first visible Phase A → Phase C bridge (retained widget tree hosts one immediate-mode subtree in the same paint cycle and same `Scene` tree). 41 new R681 unit tests in `pinion-core::scene::tests` (trait surface + walker semantics + dyn-safety + variant non-descent) + 7 integration tests in `pinion-shell::tests::dispatch_core::r681_immediate_mode_paint_cycle` (per-window tick dispatch + redraw arming + last_paint_instant + target_fps round-trip) + 10 in `pinion-runtime::frame_pacing::tests` (`WindowFramePolicy` + budget helpers) + 8 in `examples/hello-immediate-mode-canvas` (tag conventions + ARIA role + R55.G.17 composite paint-root + Owner::cache pointer-equal handle + driver tick mod TAU + post-layout viewport non-zero). `tools/demos/r681_immediate_mode_canvas.py` (≥30 assertions across A-G: substrate sanity / tick-driven state advance / retained pointer routing / keyboard activation / ARIA exposure / per-frame paint clock continuation / composite paint-root tag). 42-demo regression sweep PASS deterministic across 3 consecutive runs (R660-R680 + R681). ~3697 workspace tests pass (R680 baseline 3631 → +66 net), clippy clean. **R682 is the next round** — axis 4 substrate (dirty subtree cache + Vello `Scene::append` + structural-hash key + damage rect propagation). Sibling pair with R681 — both required for §2 #4 game-loop completeness (dirty cache enables immediate-mode without re-encoding the retained subtree every paint).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 R682 plan 절 (axis 4 dirty subtree cache + Vello fragment cache + structural-hash key + damage rect — atomic 6-8) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지. 매 atomic 종료 시 cargo test + clippy + 22-demo regression sweep 검증 후 다음 atomic 진입.
>
> R681 가중 진척: Phase A 97% + Phase B 25% × ~30% + Phase C 35% × ~3% (4-axis paint-pipeline rewrite series 2 of 4 complete; §2 #4 dual-execution model의 첫 visible 진입 — retained widget tree와 immediate-mode game loop가 같은 paint cycle에서 공존; per-window ControlFlow::WaitUntil game-loop pacing + per-window paint clock + WindowFramePolicy override 모두 wire; Phase C entry prerequisite chain 2/4 advanced) = 북극성 가중 **~20-22%**. R682-R683 series 완료 시 ~28-32% (R682 dirty-cache가 immediate-mode를 ZERO retained re-encode 비용으로 enable; R683 dock-panel runtime이 Phase B/D 진정 진입).

> **R680 land (2026-05-27, commit `3eaea2e`)** — 4-axis paint-pipeline rewrite series Round 1 of 4 (axis 3: per-window Owner scope + animation tick decoupling + per-window redraw flag). Substrate: `pinion_runtime::CoreShell.window_owners: HashMap<String, Owner>` with seeded `DEFAULT_WINDOW = root_owner.clone()` alias (single-window backward-compat) + lazy-creates `Owner::new_child(&root_owner)` for secondary window_ids; public API `window_owner` / `window_owner_existing` / `window_owner_ids`. `Owner::tick_animations_local(dt)` + `Owner::any_animation_active_local(eps)` local-walk variants (skip R51.138 child-scope cascade) — cascade primitives kept for `pinion-rpc::animate_control` headless dispatch. `CoreShell::tick_animations_for_window(window_id, dt)` + `any_animation_active_for_window(window_id, eps)` direct-dispatch primitives; legacy `tick_animations(dt)` routes through `tick_animations_for_window(DEFAULT_WINDOW)`; `_animation_driver` Effect kept as no-op subscription anchor for `frame_signal` observers (R51.149 Effect-routed tick superseded by direct call). `pinion-shell::ShellCore.last_paint_instant: Option<Instant>` lifted to `last_paint_instants: HashMap<String, Instant>` — per-window paint clock (R670.B compound origin closed). `ShellCore.redraw_requested_per_window: HashMap<String, bool>` + `request_redraw_for_window` / `take_redraw_request_for_window` / `redraw_requested_for_window` opt-in selective wake-up coexisting with binding-wide fan-out `request_redraw`; `AppShell::drain_redraw_to_winit` drains BOTH flags per slot. **Design decision (atomic 1)**: view-fn wrap stays under `CoreShell::root_owner` (NOT per-window child scope) so cross-window state sharing via `Owner::cache` keeps working without binding-level adjustment — hello-multi-window's `use_selected_path` / `use_hovered_path` slots resolve through root regardless of which window is painting; live + RPC paint paths observationally identical (§2 #2 + §2 #7 invariant preservation); per-window scope is substrate for R681 `ImmediateModeNode` game-loop nodes, R682 dirty subtree cache, R683 dock-panel tear-off lifecycle. 16 new unit tests in `pinion-runtime::core_shell::tests` (window_owners invariants) + 7 unit tests (tick_animations_for_window contract) + 6 integration tests in `pinion-shell::tests::dispatch_core::r680_per_window_redraw_wakeup` + `tools/demos/r680_per_window_owner_scope.py` (45+ assertion executions across 12 sections). 20-demo regression sweep PASS deterministic across 3 consecutive runs (R660-R679 + R680). R670.B 9-round honest carry on multi-window animation tick compound **closed structurally**: two windows painting in same event-loop turn produce only the primary scope's `tick_animations_local` walk (current bindings register all animations on root via the view-fn wrap; secondary windows walk empty local lists); animations advance once per primary paint regardless of secondary paints in the same turn. 3631 workspace tests pass (R679 baseline 3601 → +30 net), clippy clean. **R681 is the next round** — axis 2 substrate (`Scene::ImmediateModeNode` + immediate-mode subtree opt-in primitive; uses `window_owners` + per-window paint clock for game-loop `ControlFlow::Poll` wiring).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 【시작 명령】 절 (R681 atomic list — axis 2 ImmediateModeNode + per-window ControlFlow::Poll wiring; R682 axis 4 dirty subtree cache; R683 axis 1 dynamic dock — 4-axis series Round 2-4) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지. 매 atomic 종료 시 cargo test + clippy + 20-demo regression sweep 검증 후 다음 atomic 진입.
>
> R680 가중 진척: Phase A 97% + Phase B 25% × ~26% + Phase D 35% × ~6% (4-axis paint-pipeline rewrite series 1 of 4 complete; R670.B 9-round honest carry closed; multi-window animation tick decoupled; per-window redraw selective wake-up API opt-in; Phase C entry prerequisite chain advanced) = 북극성 가중 **~18-19%**. R681-R683 series 완료 시 ~28-32% (Phase B ~60% + Phase C entry ~20%).

> **R679 land (2026-05-27, commit `64de876`)** — DevTools bidirectional select bridge — the main→inspector arc that closes R675's previously one-way (inspector→main) bridge. Substrate: new `pinion_widget_paint::devtools::path_for_paint_hit(scene, x, y) -> Option<String>` paint-side hit-test inverse walker with deepest-tagged-ancestor semantics (matches `InputRouter::resolve_hover_tag` so the (x,y) → path map stays consistent across paint-side input arcs); devtools-canonical Container[tag]/Type[nth-of-type] path form with per-type-idx for untagged children; only Container + External are tag-bearing per R678 `scene_tag` contract (tagged Box/Text walked past); Scroll content non-descent v1 carry. 12 unit tests pin the round-trip invariant (`find_node_at_path(scene, path_for_paint_hit(scene,x,y).unwrap()).is_some()`) + corner cases (outside-scene None / untagged-only None / overlapping-siblings topmost-wins / per-type idx among untagged siblings / tagged-Box-not-recognised / nested deepest-tagged / 3×3 round-trip stress grid). Binding: hello-multi-window adds `MainWindowClickRouter` External as 2nd `ExtraExternal` at `MAIN_CLICK_ROUTER_TAG = "main_click_router"` — AI-invoke-only `click` channel (Text(path) selects, Null deselects); read-only `last_clicked` mirror slot for AI introspection via scene/query; emits `click` intent prefixed dotted to `main_click_router.click`. V::update reducer extended with 2 new arms: `main_click_router.click` Text→Some / Null→None into `use_selected_path` (AI-driven half), `main_btn.click` Null→Some(MAIN_BUTTON_RAW_PATH) for user-mouse-driven half (closes user-mouse arc without shell-level hit-test hooks — ButtonExternal's existing SCXML Pressed→Hover click intent is the trigger). `MAIN_BUTTON_RAW_PATH = "Container/Container[main_btn]"` static constant pinned by both substrate (`r679_path_for_paint_hit_on_tagged_container_returns_container_path`) and binding (`r679_button_raw_path_matches_view_main_raw_button_position`) tests. Background-click design pinned: user-mouse on main background = no-op; AI-Null deselect available. 8 new R679 binding tests (dotted intent tag pins / Text+Null payload routing / bidirectional alternation latest-wins / view_inspector cross-arc) + 3 view-level cross-arc tests (router invoke + button click + Null deselect all paint M3 SurfaceContainerHighest focus state-layer on inspector_tree#{path} row). `tools/demos/r679_devtools_bidirectional_select.py` 35+ assertions across 9 sections (A substrate sanity / B baseline / C inspector→main R675 regression / D main router→inspector R679 / E button click→inspector R679 / F bidirectional alternation / G AI Null deselect / H last_clicked mirror / I intervene read-only); 19-demo regression sweep PASS deterministic across 3 consecutive full sweeps (R660-R678 + R679; r675_devtools_select.py section (J) banner assertion relaxed to accept either pre-R679 'Selected: main' or post-R679 'Selected: Container/Container[main_btn]' — R679 bidirectional bridge changed the post-button-click selection state). 3601 workspace tests pass (R678 baseline 3579 → +22 net), clippy clean. **R680 is the next round** — 2nd DevTools binding consumer to trigger substrate lift of `MainWindowClickRouter` itself into `pinion_widget_paint::devtools` (Rule-of-Three).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 【시작 명령】 절 (R680 atomic list — 2nd DevTools binding consumer; MainWindowClickRouter substrate lift Rule-of-Three gate) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지. 매 atomic 종료 시 cargo test + clippy + 19-demo regression sweep 검증 후 다음 atomic 진입.
>
> R679 가중 진척: Phase A 97% + Phase B 25% × ~24% + Phase D 35% × ~6% (DevTools cascade: outliner → highlight → property pane → hover bridge → substrate lift → bidirectional select; 4th visible Phase D editor feature complete = the canonical Chrome/Firefox/Safari DevTools "select-in-Elements ↔ select-in-page" dual-arc; substrate `path_for_paint_hit` is first paint-side hit-test inverse walker in pinion + matches `InputRouter::resolve_hover_tag` so the (x,y) → path map is consistent across every input arc) = 북극성 가중 **~17-18%**.

> **R678 land (2026-05-27, commit `97a8c4b`)** — DevTools cross-window hover-overlay bridge + first DevTools substrate lift. `pinion_widget_paint::tree_view::TreeRowClickExternal` grows a parallel **hover axis** next to its R675 press axis: `hovered_id: Option<String>` slot + `TREE_ROW_HOVER_EVENT = "hover"` const + `PointerEnter` / `PointerLeave` / `PointerCancel` arms on the composite-tag `send` wire + typed `invoke("hover", Text(id) \| Null)` shortcut. Each enter/leave transition emits one `hover` intent (`Text(id)` on Enter, `Null` on Leave) — independent press/hover slots, W3C canonical Leave-before-Enter ordering preserved. hello-multi-window adds `use_hovered_path()` Owner::cache hook parallel to `use_selected_path` + `INSPECTOR_HOVER_INTENT_TAG = "inspector_tree.hover"` + `WidgetCore::update` reducer routing (Text → Some, Null → None). `view_main` reads both signals and paints **two distinct wraps** — Error red for selection (R676), M3 SurfaceContainerHighest for hover (R678). Selection wins on same node; different nodes paint both. Depth-desc sort applies deeper wrap first so the shallower wrap's anonymous-ancestor insertion preserves nested-path lookup. The selection + hover dual-wrap fires the [[abstraction-needs-second-consumer]] Rule-of-Three gate: `pinion_widget_paint::devtools` new module lands as the first DevTools substrate lift, homing 13 path-stable indexing + highlight overlay helpers (`scene_type_name` / `scene_tag` / `scene_root_path_segment` / `scene_child_path_segment` / `PathDisambiguator` / `parse_path_segment` / `find_child_in_container` / `find_node_at_path` (renamed from `find_main_node_at_path`) / `wrap_with_highlight` / `rebuild_with_highlight_at_path` / `descend_and_wrap` / `scene_to_tree_item` / `DEFAULT_HIGHLIGHT_BORDER_WIDTH`). hello-multi-window simplified to a substrate consumer (~700 LOC removed); 30 substrate tests + 16 R678 hover-axis tests + 15 R678 hover-bridge binding tests + tools/demos/r678_devtools_hover_bridge.py (34+ assertions, 4.89s); 18-demo regression sweep PASS deterministic (R660 - R678 + double_click_r663). 3579 workspace tests pass (+35 net from R677's 3544), clippy clean. **R679 is the next round** — DevTools bidirectional select (main-window click writes use_selected_path so inspector tree row paints focus state-layer in lockstep; currently the arc is one-way: inspector → main).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 【시작 명령】 절 (R679 atomic list — DevTools bidirectional select + main-window click→Signal arc) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지. 매 atomic 종료 시 cargo test + clippy + 18-demo regression sweep 검증 후 다음 atomic 진입.
>
> R678 가중 진척: Phase A 97% + Phase B 25% × ~22% + Phase D 35% × ~5% (DevTools cascade: outliner → highlight → property pane → hover bridge + substrate lift; second visible Phase D editor feature complete; pinion_widget_paint::devtools module crystallises as the first DevTools substrate; bidirectional select R679 → 2nd DevTools binding R680 → pinion-devtools crate skeleton R681 are the next 3 visible cascade steps) = 북극성 가중 **~15-17%**.

> **R677 land (2026-05-27, commit `4fbbe48`)** — DevTools property pane (Chrome Computed-pane analog) on the path-stable foundation R676 laid. `view_inspector` restructured into 2-pane Row layout (`inspector_tree` left + new `property_pane` right; LayoutStyle::flex_grow=1.0 on the property pane). `property_pane_rows(scene)` field walker emits one row per scene-node field — universal `type:` row + variant-specific rows (Container: tag/style.fill/style.border/layout.size/children; Text: content/font_size/fg; External: tag). `find_main_node_at_path` 2nd consumer reached ([[abstraction-needs-second-consumer]] Rule-of-Three approach tracking: 1st = R676 view_main highlight overlay pre-resolve, 2nd = R677 property_pane selection resolution — 3rd in R678+ will trigger pinion-devtools substrate lift). Inspector window resized 280×140 → 480×320 (hosts the 2-pane layout). Soft-fail "(no selection)" placeholder for inspector-only ids (state/main) and stale paths. Format helpers (format_color CSS rgba mirror / format_border / format_size / format_size_value / format_optional_tag em-dash for None) mirror Chrome DevTools conventions. R671 + R672 demos updated (substrate-evolution-driven: inspector_tree no longer the snapshot root, dimensions changed). 18 new R677 unit tests + tools/demos/r677_devtools_property_pane.py (38+ assertions); 16-demo regression sweep PASS deterministic. 3544 workspace tests pass (+18 net from R676's 3526), clippy clean.
>
> **R676 land (2026-05-27, commit `cf3d134`)** — R675 architectural 부채 즉시 청산 (path-stable indexing scheme) + DevTools highlight overlay 1st cut. `scene_to_tree_item` walker rewritten to Browser-DevTools-canonical CSS-selector form `Type[tag-or-nth-of-type]` — tagged Container/External use `Container[main_btn]` form, untagged use `Type[nth-of-type]` form (`Text[0]`); root segment is bare type for untagged singletons. Tagged paths are now invariant to untagged-sibling churn (the R675 banner-on/off issue where Container[main_btn] shifted from sibling-idx 0 to 1 is closed at source). `find_main_node_at_path` + `parse_path_segment` + `find_child_in_container` inverse walker pair lands as soft-signal resolver (returns Option<&Scene>, None for inspector-only ids / stale paths / malformed segments). DevTools highlight overlay = `wrap_with_highlight(scene, ColorRole::Error 2px Border)` non-destructive composition + `rebuild_with_highlight_at_path` by-value walker (std::mem::replace swap+transform pattern). `view_main` two-pass: build raw via new `view_main_raw` (inspector mirrors this — DevTools-style separation between underlying tree and overlay paint, mirroring Chrome / Firefox / Safari inspector architecture) → rebuild with wrap. 38 new R676 unit tests + tools/demos/r676_devtools_highlight.py (42+ assertions); 15-demo regression sweep PASS deterministic (R660 / R664 / R665 / R666 / R667 / R668 / R669 / R670a / R670b / R671 / R672 / R673 / R674 / R675 + R676). 3526 workspace tests pass (+28 net from R675's 3498), clippy clean. R663.5 canonical baseline 유지. **R677 is the next round** — DevTools cascade continuation (property pane / hover bridge / bidirectional select / highlight overlay substrate lift on 2nd consumer surface).
>
> **다음 세션 진입**: `load` 단독 입력. SEED 의 【시작 명령】 절 (R677 atomic list — DevTools property pane consumer + selected-node field render) 자동 진행. 모든 atomic 은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지. 매 atomic 종료 시 cargo test + clippy + 16-demo regression sweep 검증 후 다음 atomic 진입.
>
> R676 가중 진척: Phase A 97% + Phase B 25% × ~20% (path-stable indexing 부채 청산 + DevTools 1st visual feedback complete) + Phase D 35% × ~2% (first visible Phase D editor outliner→viewport bridge complete; path-stable indexing은 모든 후속 DevTools work의 foundation) = 북극성 가중 **~13-14%**. R677+ DevTools cascade (property pane → hover bridge → bidirectional select) targets ~14-15%.
>
> **R675 land (2026-05-26, commit `e1d8817`)** — TreeRowClickExternal substrate lift (R674 binding-level `FileTreeRowExternal` → `pinion_widget_paint::tree_view::TreeRowClickExternal`) + DevTools/SceneInspector first interactive dogfood. [[abstraction-needs-second-consumer]] Rule-of-Three gate fired: hello-multi-window inspector is 2nd consumer (hello-tree-view = 1st), substrate now homes the External next to view_tree / view_tree_focused. Cross-window state-sync via shared `Signal<Option<String>>` selected_path (use_selected_path() Owner::cache hook); inspector click → V::update reducer mirrors into Signal → main window paints "Selected: {path}" banner; inspector tree row paints M3 focus state-layer on selected row. First **Phase D editor dogfood** structurally complete — AI driving cross-window state via `/inspector_tree/external/click` typed shortcut produces same observable result as human click. 17 R674 binding tests migrated to substrate behavior-identity preserved. **R676 carry inline 청산 ✓** (path-stable indexing + DevTools highlight overlay — see R676 land summary above).
>
> **R674 land (2026-05-26, commit `f905488`)** — R673 carry 100% inline 청산: (a) TreeView click-to-expand via new binding-level `FileTreeRowExternal` (SCXML Idle↔Pressed + composite_tag::parse_send_payload 6th substrate consumer + V::update reducer bridge sharing `toggle_expanded_in_signal` sink with the keyboard Space/Enter path so kbd and click routes produce bit-identical Signal mutations); (b) per-row `AriaRole::TreeItem` AccessNodes emitted via new `walk_access_rows` helper with WAI-ARIA 1.2 hierarchical axes (level / position_in_set / size_of_set). AccessNode substrate extension lands 3 `Option<u32>` fields + builder methods + AccessKit `set_level` / `set_position_in_set` / `set_size_of_set` lowering. `role.rs::TreeItem` doc correction (WAI-ARIA spec requires authors to provide hierarchical axes on custom widget roles; AT does NOT auto-compute). **R675 carry inline 청산 ✓** (FileTreeRowExternal lifted to substrate via R675 — see R675 land summary above).
>
> **R673 land (2026-05-26, commit `4a5a694`)** — TreeView 2nd consumer (`hello-tree-view`) + WAI-ARIA tree keyboard navigation + AriaRole Tree/TreeItem + view_tree layout fix (stretch + fixed glyph column). R671 substrate maturity 정통 검증 완료. **R674 carry inline 청산 ✓ 완료** (click-to-expand + per-row TreeItem AccessNodes — see R674 land summary above).
>
> **R671 land (2026-05-25, commit `6a7b955`)** — R670.B carry 3건 청산 (compute_paint_scene unify + per-window last_paint_layout + pinion_rpc single-parse) + Phase B widget catalog 첫 진입 (pinion_widget_paint::tree_view + hello-multi-window inspector). 11-demo sweep PASS. R670.B substrate-incompleteness 신호 (multi-window InputRouter race) 발견 + carry로 기록.
>
> **R672 land (2026-05-26, commit `79684f4`)** — per-window InputRouter foundation. `CoreShell.routers: HashMap<String, InputRouter>` + 11 _for_window 변형 + 10 ShellCore _for_window 변형 + AppShell::window_event spec_id 해결 + finalize_frame_for_window. R670.B → R671 multi-window race **구조적으로 closed**. R670.B demo의 scene/invoke 우회 retire (scene/click 다시 사용), r672_multi_window_race_free.py 신규 (race-free 정통 verification). 12-demo sweep PASS deterministic.
>
> **R673 land (2026-05-26, commit `4a5a694`)** — Phase B widget catalog substrate maturity (TreeView 2nd application consumer). `AriaRole::Tree` + `TreeItem` (WAI-ARIA 1.2 §5.3.10/§5.3.11) + `TreeViewFocus` + `view_tree_focused` interactive entry + M3 SurfaceContainerHighest focus state-layer overlay. view_tree 레이아웃 정통 정정 — `AlignItems::Stretch` + 고정 너비 glyph column (NBSP leaf ↔ ▶/▼ branch 컬럼 정렬). `examples/hello-tree-view` 18번째 example, sample 파일 트리 모델 + WAI-ARIA tree keyboard model (Arrow Up/Down/Left/Right + Home/End + Space). 13-demo sweep PASS.

【R680 plan】 next round entry — **4-axis paint pipeline rewrite series Round 1 of 4** (axis 3 substrate: per-window Owner scope + tick_animations decoupling + per-window redraw flag). single commit, 6 atomic.

> **User directive (2026-05-27, R679.2 audit close 시)**: R679 closure 후 system-architect audit가 4-axis (dock/undock 1, selective rendering 2, per-window independence 3, dirty tracking 4) 모두 MISSING/PARTIAL을 확인. **4-axis는 분리된 roadmap items가 아니라 하나의 통합된 paint-cycle + window-lifecycle 재설계**. Prerequisite chain: axis 3 → axis 2/4 (subtree paint은 per-window scope 분리 후만 의미); axis 2 ↔ axis 4 = sibling pair (둘 다 같이만 의미); axis 1 = axis 3 위에 얹는 consumer. 비용 무관 + 북극성 정합 우선 원칙 적용 → R680-R683 단일 series로 묶어서 4-axis 전부 textbook canonical 청산.
>
> R679 cascade closure carry (2nd DevTools binding consumer + MainWindowClickRouter substrate lift)는 R683 dock series 안에서 자연 land — dock-able 2nd consumer가 substrate lift 의 자연 진정 consumer.

R680 atomic 6개 (axis 3 substrate-first 순서, prerequisite chain mandatory):

(0) **`pinion-runtime::Owner` per-window scope substrate** — `Owner::current()` 가 thread-local stack 으로 동작 (R51.146 view-fn-owner-current-thread-local). R680 은 binding의 root_owner 위에 per-window child Owner scope를 carve. `pinion-runtime::core_shell::CoreShell` 에 `window_owners: HashMap<String, Rc<Owner>>` 필드 추가 (key = `WindowSpec::id`, value = child of `root_owner`). `compute_paint_scene_internal(window_id, w, h)` 가 paint cycle 시작 시 `window_owner.run(|| { ... root_owner.run(...) ... })` 대신 `window_owner.run(|| { V::view_for_window(window_id, state, frame) })` — view fn이 per-window owner 아래에서 실행되어 Owner::cache 슬롯이 per-window 분리됨. backward-compat: 단일 윈도우 binding은 primary window owner == ShellCore-wide owner (Phase A 회귀 0). 12-15 unit tests pin per-window Owner::cache 격리 + parent-child Owner cleanup 정합 (Owner drop 시 child scope drain).

(1) **`Owner::tick_animations` per-WindowSlot 분리** — `pinion-shell::WindowSlot` 에 `last_paint_instant: Option<Instant>` + `owner_scope: Rc<Owner>` 필드 추가. `AppShell::render_window(window_id)` 가 `tick_animations(slot.last_paint_instant, current_instant)` 를 호출하되 **slot.owner_scope 만 walk** (`Owner::tick_animations_recursive` 의 per-Owner variant). 두 윈도우 동시 페인트 시 N배 compound 청산 (R670.B carry honest 청산 — 9 round 미해결 부채). 6-8 unit tests: 두 윈도우 가 별도 spring `Tickable` 보유 시 cross-window state corruption 0 (R51.150 owner-cache substrate 의 per-window 변종 검증), animation dt clamp 가 per-slot 적용.

(2) **`pinion-shell::WindowSlot.redraw_requested: AtomicBool` per-window flag** — 현재 `ShellCore::request_redraw` 가 binding-wide `redraw_requested` flag 1개 (`drain_redraw_to_winit` 가 모든 슬롯에 fan-out: `app.rs:273-282`). R680 은 flag를 per-WindowSlot 으로 lift; `ShellCore::request_redraw_for_window(window_id)` API 신설 (default `request_redraw()` = primary window 만). `Signal::set` → `Effect::mark_dirty` → `any_animation_active` 체인이 **변경된 window scope 만** request_redraw 발화 (Effect 의 owning Owner scope를 통해 어느 window 인지 해결). Cross-window propagation은 explicit `request_redraw_for_window("inspector")` 또는 shared signal subscription 으로만 가능. 8-10 unit tests pin selective redraw + 회귀 0 (hello-multi-window 의 inspector mirror arc는 shared owner 위에 살아있어 자연 redraw — explicit cross-window subscription 처리).

(3) **`pinion-rpc::DispatchContext.window_id` per-window owner resolution** — RPC dispatch 가 invoke / query / intervene 시 target window의 owner scope 안에서 실행되어야 (Effect 가 올바른 window 의 redraw 발화). `pinion-shell::ShellCore::dispatch_rpc_for_window(window_id)` 가 `window_owner.run(|| dispatch_inner(...))` wrap 추가. RPC introspect 결과는 동일 (paint 결과만 per-window scoped). 4-6 unit tests + 19-demo sweep 회귀 0 검증.

(4) **`hello-multi-window` per-window tick isolation 검증 atomic** — R680 demo `tools/demos/r680_per_window_owner_scope.py` (≥30 assertion): (A) 두 윈도우 paint 동시 시 spring animation step count 1배 (compound 0); (B) 한 윈도우의 Signal 변경이 다른 윈도우 redraw 발화 안 함 (explicit shared subscription 제외); (C) per-window Owner::cache 슬롯이 격리 — main window의 `use_selected_path()` 와 inspector의 `use_hovered_path()` 가 서로 다른 owner scope; (D) parent-child Owner cleanup 정합; (E) RPC `scene/invoke {window: "inspector", ...}` 가 inspector owner scope에서 실행 → inspector만 redraw; (F) backward-compat — 단일 윈도우 binding (hello-button / todomvc / settings-panel) 동일 행동 bit-identical.

(5) **20-demo regression sweep + commit + Mnemosyne** — 20-demo sweep PASS deterministic 3 consecutive runs (R660-R679 + R680). Commit `feat(runtime): R680 §5.16 §5.41 axis 3 per-window Owner scope`. Mnemosyne `entry_id=R680` + impact_refs [5.16, 5.41, 5.45] + carry (R681 axis 2 immediate-mode primitive / R682 axis 4 dirty subtree cache / R683 axis 1 dynamic dock + R679 DevTools 2nd consumer 자연 land).

honest LOC 예측: **R680 = +1500-2300 net** (substrate refactor + per-window Owner field + tick decouple + redraw flag + RPC threading + demo + tests). 핵심 substrate change → 1.5-2× R670.B (1300 net).

R680 후 가중 진척: ~17-18% → **~19-20%** (Phase B 25% × ~27% — multi-window 진짜 independence 도달; per-window animation tick + selective redraw = pro-tool authoring의 정통 기반; R681-R683 prerequisite). Phase C entry (R&lt;700-800 range&gt;)이 paint-pipeline rewrite series 완료 후 자연 도래.

**R680 verification mandatory** (라운드 끝):
- 20-demo regression sweep PASS deterministic 3 consecutive runs (R660-R679 + R680)
- 두 윈도우 동시 spring animation 시 tick compound 0 (R670.B carry 9-round 부채 청산)
- per-window Owner::cache 슬롯 격리 확인 (cross-window state corruption 0)
- Selective redraw — 한 윈도우 Signal mutation 이 다른 윈도우 redraw 발화 안 함 (explicit shared subscription 제외)
- backward-compat — 단일 윈도우 binding 18개 회귀 0
- 부채 surface 정직 받아들임 — R681 axis 2 / R682 axis 4 / R683 axis 1 + R679 DevTools 2nd consumer carry

**R679 verification (라운드 끝, completed)**:
- ✓ 19-demo regression sweep PASS deterministic (R660-R678 + R679; 3 consecutive sweeps)
- ✓ path_for_paint_hit round-trip invariant pinned by 3×3 stress grid + 11 corner-case tests
- ✓ Bidirectional sync invariant pinned at binding level (`r679_bidirectional_alternation_preserves_invariant`) + RPC level (demo section F)
- ✓ Background-click design decision pinned (no-op for user-mouse; AI-Null available for deselect)
- ✓ R675 demo section (J) banner assertion relaxed to accept R679's bridge enrichment
- ✓ 부채 surface 정직 받아들임 — 4-axis paint pipeline rewrite series (R680-R683) carry below + Scroll content non-descent v1 carry + button-payload-with-coordinates substrate carry

【4-axis paint pipeline rewrite series carry (R680 → R683 sequence, R679.2 audit 시 등록)】

R679 closure 후 system-architect audit (2026-05-27) 가 4-axis 모두 MISSING/PARTIAL 확인. 분리 land = anti-textbook (prerequisite chain 위배 + 매 라운드 retroactive substrate 재설계 부채). 단일 series로 묶어 paint-pipeline + window-lifecycle 통합 재설계.

**Prerequisite chain** (분리 불가):
```
              Axis 3 (per-window Owner scope, tick decoupling)
              │   ╲
              │    ╲ (subtree paint은 per-window scope 분리 후만 의미;
              │     ╲ 글로벌 redraw fan-out 위에 immediate-mode 얹으면
              │      ╲ Signal::set 1번에 모든 윈도우 60Hz burn)
              │       ╲
   Axis 1 ←──┘        Axis 2 (Scene::ImmediateModeNode, immediate-mode
   (dynamic dock        │     subtree opt-in primitive)
   on Signal<Vec<       │
   WindowSpec>>;        │ sibling pair (둘 다 함께만 의미; immediate
   tear-off ; dock-     │ without dirty = 144Hz로 retained tree 재encode
   able panel UX)       │ burn; dirty without immediate = retained 위
                        │ fragment caching 최적화에 그침)
                        │
                       Axis 4 (Vello Scene::append subtree cache,
                              structural-hash key per Scene::Container,
                              damage rect propagation)
```

**R680 — axis 3 substrate (per-window Owner scope + tick decoupling + selective redraw)**. 위 R680 plan 절 참조. **Phase B → C 전환의 첫 필수 prerequisite**. R670.B carry (animation tick global compound) 9-round honest 청산.

**R681 — axis 2 substrate (Scene::ImmediateModeNode + immediate-mode subtree opt-in primitive)**. atomic 7-9개 예상:
   - (0) `Scene::ImmediateModeNode { tick: Rc<dyn ImmediateMode>, viewport: Rect, last_dt: Cell<Duration> }` 변종 추가 (pinion-core/src/scene.rs). `ImmediateMode` trait: `fn tick(&mut self, frame: &mut ImmediateFrame, dt: Duration)`.
   - (1) `pinion-shell::AppShell::render_window` 가 ImmediateModeNode 발견 시 `Scene::Container` retained walk 분리 — ImmediateModeNode 의 paint 책임은 trait impl 가 직접 vello::Scene 에 그림 (paint_adapter 가 retained subtree 만 encode).
   - (2) **per-window `ControlFlow::Poll`** — `WindowSlot` 안에 immediate-mode subtree 1개 이상 있을 때 winit ControlFlow를 Poll 로 (그 외 Wait). winit 0.30 글로벌 control-flow 한계 → self-paced redraw timer (per-window `Instant::now() + frame_budget_target` 비교, axis 3 의 per-window `last_paint_instant` 활용).
   - (3) `pinion-runtime::frame_pacing` 확장 — `target_fps: HashMap<WindowId, u32>` + per-window frame budget cap (default 60fps for immediate-mode, idle Wait otherwise).
   - (4) 첫 ImmediateMode consumer — `examples/hello-immediate-mode-canvas` 또는 hello-multi-window inspector 의 fps-counter overlay (작은 stress test, retained + immediate 공존 검증).
   - (5) R681 demo + 21-demo sweep + Mnemosyne entry.
   - **honest LOC 예측 ~+2000-3000 net** (Scene variant + AppShell branch + frame_pacing 확장 + 첫 consumer + demo).

**R682 — axis 4 substrate (dirty subtree cache + Vello Scene::append + structural-hash key + damage rect)**. atomic 6-8개 예상:
   - (0) `pinion-core::scene::Container` 에 `paint_hash: Cell<Option<u64>>` field — paint pass 시 자신의 box + children hash 계산 캐시. Hash 변경 시만 dirty.
   - (1) `pinion-shell::paint_adapter` rewrite — vello::Scene 을 reset 안 하고 per-Scene::Container subtree 별 `vello::Scene` fragment cache (`HashMap<u64, vello::Scene>` 구조 해시 키 기반). `Scene::append` 으로 unchanged subtree 추가 (재encode 0). 변경 subtree 만 fresh encode.
   - (2) Damage rect propagation — `compute_layout` 결과의 rect 변화도 감지 (paint_hash 가 layout-rect 도 포함). 이전 paint 의 rect 와 새 rect 의 union = damage region. (Vello는 현재 damage rect 무관하게 전체 buffer 재submit 함 — 추후 wgpu surface invalidation 활용은 R&lt;X+1&gt; carry).
   - (3) Signal-mutation → subtree paint wire — `Effect::mark_dirty` 가 자신의 owning Owner scope 안 Scene::Container subtree 들의 paint_hash 무효화 (subtree scope 기반 invalidation). 글로벌 V::view 재실행은 유지 (contract 안 깸); 단 fragment cache hit 률이 90%+ 라 비용 사실상 0.
   - (4) 첫 dirty-cache consumer — todomvc 의 100-row long list (filter 변경 시 변경된 row 만 재encode 검증).
   - (5) R682 demo (cache hit rate + frame time profiling assertion) + 22-demo sweep + Mnemosyne.
   - **honest LOC 예측 ~+2500-3500 net** (paint_adapter rewrite는 R&lt;X&gt; 핵심 refactor + 첫 consumer profiling).

**R683 — axis 1 substrate (runtime window lifecycle + Splitter widget + dock UX)**. atomic 8-10개 예상:
   - (0) `WidgetView::windows() -> Vec<WindowSpec>` compile-time 을 `Signal<Vec<WindowSpec>>` runtime 으로 lift. `WindowSpec::main(...)` / `WindowSpec::new(...)` API 유지, declaration 만 reactive.
   - (1) `pinion-shell::AppShell::reconcile_windows` Effect — Signal 구독, diff 계산, 새 spec → `resume_spec` 호출, 사라진 spec → winit `window.close()` + `WindowSlot` drop. axis 3의 per-window Owner scope cleanup 정합.
   - (2) `pinion-widget-paint::splitter::Splitter` widget — `Scene::Container` 안 draggable handle child + `Signal<f32>` ratio + LayoutStyle flex-grow 와이어. R660 의 DeferredInput::Drag 정통 consumer.
   - (3) `pinion-widget-paint::dock::DockSurface` widget — N개 child panel을 dock layout(좌/우/상/하/center) 으로 배치. tear-off drag 감지 → 새 WindowSpec push (axis 1+3 통합).
   - (4) `examples/hello-dock-panels` 첫 dock consumer — DevTools의 inspector tree + property pane + viewport를 dock 으로 배치, drag로 tear-off → 새 윈도우.
   - (5) **R679 DevTools cascade carry 자연 청산** — 2nd DevTools binding consumer가 hello-dock-panels 안 inspector dock-panel 로 등장 (Phase D editor self-hosted 의 정통 진입 dogfood). MainWindowClickRouter substrate lift 도 Rule-of-Three 자연 trigger.
   - (6) R683 demo (dock split-pane drag, tear-off, dock-back) + 23-demo sweep + Mnemosyne.
   - **honest LOC 예측 ~+3000-4500 net** (가장 큰 round — Splitter + DockSurface + tear-off UX + first dock consumer 동시 land).

**R680-R683 series 종료 후 가중 진척**: 현재 ~17-18% → **~28-32%** (Phase B 25% × ~60% — multi-window pro-tool authoring 정통 ground + Phase C 35% × ~20% — paint pipeline ready for game-engine substrate). Phase C entry (game-loop, 3D scene graph, asset pipeline, physics, audio, PBR)가 R&lt;700-900 range&gt;에서 자연 도래 — R683 이후 즉시 진입 가능.

**R680-R683 series rationale (textbook anchor)**:
- 비용 무관 + 북극성 정합 우선 + 부채 즉시 상환 (User directive 2026-05-25 + 2026-05-27 audit close). 4-axis 분리 land = retroactive R900-class paint pipeline 재설계 부채 누적; series 통합 land = textbook canonical.
- [[substrate-incompleteness-signal]] — R670.B carry (animation tick global compound) 가 9-round 미해결. R680 axis 3 청산.
- [[abstraction-needs-second-consumer]] — R679 cascade closure (2nd DevTools consumer) 는 R683 dock series 안에서 자연 trigger (R&lt;X&gt;.D dock-able DevTools 가 2nd consumer 의 textbook 정통 형태).
- [[textbook-long-term-correct]] — "lifetime project + 비용 무시" 원칙 직접 적용. 4-axis가 진짜 northern-star (§2 #4 dual execution = Phase C entry = AAA + editor self-hosted enabler).

**R678 verification (라운드 끝, completed)**:
- ✓ 18-demo regression sweep PASS deterministic (R660 - R678 + double_click_r663)
- ✓ TreeRowClickExternal hover axis SCXML clean — 16 substrate tests cover NotHovered ↔ Hovered transitions via composite-tag PointerEnter/Leave/Cancel matrix + cross-axis independence with the R675 press axis
- ✓ Highlight overlay substrate at pinion_widget_paint::devtools with both consumers (selection wrap R676 + hover wrap R678) wired through view_main; 30 substrate tests pin the lifted helpers' behaviour
- ✓ Soft-fail UX preserved — inspector-only ids ("state" / "main") + stale paths gracefully skip BOTH selection AND hover wraps (separate test paths for each axis in the binding suite)
- ✓ 부채 surface 정직 받아들임 — bidirectional select R679 (next round), 2nd DevTools binding R680, pinion-devtools crate skeleton R681, nested-path wrap depth-sort substrate-level rewrite (currently binding-level heuristic)
>
> **User directive (2026-05-25, R670.A close 시 재확인)**: 비용 무관 + 북극성 anchor + 장기 textbook-canonical 결정 + 부채 즉시 상환 + 한 라운드에 모든 atomic land. R670 original plan 은 6 atomic; session budget honesty 로 R670.A (atomics 0+1+2 = R668/R669 carry clearance + Phase B trait foundation) 우선 land, R670.B (atomics 3+4+5 = AppShell multi-window refactor + RPC window param + `hello-multi-window` + demo + Mnemosyne) 별도 round. **single commit = 1 round 원칙 유지; 매 atomic 종료 시 cargo test + clippy + 기존 demo regression sweep 검증 후 다음 atomic 진입**.
>
> **R670.A landed atomics**:
> - **Atomic (0) ✓** — `pinion-tui` full RPC ingress (R668 carry #2 영구 청산). `ShellCoreTui::dispatch_rpc` + `previews: PreviewLedger` + `revision: SceneRevision` + `focus: FocusManager` + `last_paint_layout: Option<LayoutNode>` field lifts; `spawn_stdin_rpc_reader_tui` stdin reader thread + mpsc::Sender<String>; `pinion-tui::shell::run` event-loop integration with **stderr response writer** (alternate-screen + raw-mode terminal owns stdout); `drain_rpc_into_substrate` + `drain_intents_into_substrate` + `commit_and_finalize` 3 helper extracts to keep `run_impl` under 100-line ceiling; `finalize_paint_snapshot(&Scene)` substrate method refreshes `last_paint_layout` on every paint commit so `scene/layout {viewport: null}` resolves through the same wire shape as pinion-shell. **9 integration tests** (`crates/pinion-tui/tests/rpc_ingress.rs`): scene/snapshot, scene/click drain to Hover, scene/key named Space, scene/key character 'd' → Disabled, scene/invoke /external/send Disable, focus/get fresh = null, focus/set targets tag, scene/click bumps OCC revision, malformed JSON-RPC error envelope. handle_tail return OR'd with focus_change so notify_focus_change fires on RPC-driven focus mutation (mirror of pinion-shell `External::on_focus_change` arc).
> - **Atomic (1) ✓** — `hello-popover` binding = first real `SizeStrategy::IntrinsicAfterFirstPaint` consumer (R668 carry #1 + R669 carry #1 영구 청산). New `examples/hello-popover` (16th example), `examples/hello-popover/src/main.rs` ~470 LOC with header + 3 body text rows + Button trigger laid out vertically. No root-size lock (the substrate test pins `LayoutStyle::size.width == Auto && height == Auto`). `initial_size_strategy()` overrides the macro's default `Fixed { ... }` emit with `IntrinsicAfterFirstPaint { min: (240, 100), max: (480, 400) }`. **New `pinion-derive` macro flag `initial_size_strategy`** (single-line `const KNOWN_FLAGS` extension + body selector) opts the binding into forwarding to the inherent fn instead of the auto-emitted Fixed variant — surgical surface change, zero impact on the 15+ existing bindings. 5 unit tests (`r670_intrinsic_first_paint_tests`): strategy declaration pin, root-size lock absence pin, ARIA Button role pin, widget tag literal pin, painted-scene non-zero intrinsic bbox pin. **R670.A scope clarification — IntrinsicAfterFirstPaint is one-shot per binding lifetime**; dynamic shrink-wrap-on-state-change ("click button → window expands") is a separate substrate axis that would require either explicit `scene/resize` RPC calls from the binding or a substrate extension lifting the one-shot guard. R670.A binding demonstrates the one-shot capability honestly; click-driven dynamic resize is deferred to a future round where a real use case (collapsible dialog "more options" section, etc.) surfaces the substrate-incompleteness-signal.
> - **Atomic (2) ✓** — `WindowSpec` + `WidgetView::windows()` trait extension (Phase B substrate foundation, +110 LOC). `pinion_shell::WindowSpec { id: &'static str, title: String, strategy: SizeStrategy }` with `WindowSpec::main(title, strategy)` (canonical primary `id = "main"`) + `WindowSpec::new(id, title, strategy)` (secondary windows). `WidgetView::windows() -> Vec<WindowSpec>` trait method with backward-compat default `vec![WindowSpec::main(Self::title(), Self::initial_size_strategy())]` — every existing single-window binding (15+ in the example gallery) keeps its lifecycle bit-identical. 2 unit tests pin `id = "main"` for the canonical primary + arbitrary `id` for `WindowSpec::new`. **AppShell still reads only the first spec (the default single-window primary)** — multi-window dispatch lands in R670.B when `AppShell::resumed` walks the full `Vec<WindowSpec>` and creates one winit Window + RenderState + accesskit_winit::Adapter per spec.
>
> R670.A verification: **3431 workspace tests** (was 3415 at R669 → +16 R670.A: +9 pinion-tui RPC ingress + +5 hello-popover + +2 WindowSpec pin), clippy clean, **8-demo regression sweep PASS** (R660/R663/R664/R665/R666/R667/R668/R669 all bit-identical — zero regression from R670.A substrate changes), R670.A demo PASS (`tools/demos/r670a_carry_clearance.py` — IntrinsicAfterFirstPaint window-size verify post-first-paint).
>
> **R670.B plan** (next round entry — single commit, 4 atomic):
> - **(0)** `pinion-shell::AppShell` multi-window refactor (~600-900 LOC churn). `render: RenderState<V::Renderer>` → `renders: HashMap<WindowId, RenderState<V::Renderer>>`; `resumed()` walks `V::windows()` list and creates one winit Window + RenderState + accesskit_winit::Adapter per spec; `window_event(window_id, event)` dispatches to per-window slot; `pending_intrinsic_resize` + `ime_was_composing` + `last_ime_cursor_area` lift to per-window fields. Single `ShellCore` (Approach A — same binding state, different views per window); multi-binding (different ShellCore per window) is R750+ widget catalog territory.
> - **(1)** RPC `{window: "<id>"}` param + `pinion-rpc::DispatchContext` per-window scope. Default `window = "main"` (single-window binding compat); `scene/snapshot {window: "inspector"}` / `scene/click {window: "main", at: …}` / `scene/key {window: "inspector", at: …}` all carry the new param. `WidgetView::view_for_window(window_id, state)` trait method (default forwards to `view`).
> - **(2)** `hello-multi-window` first consumer binding (~500-800 LOC). Main window (button widget) + inspector window (debug Text node showing `format!("{:?}", state)` of main's button state). Phase B first dogfood — DevTools / Inspector substrate-first.
> - **(3)** demo (≥ 30 assertion verifying main+inspector mirror) + 8-demo regression sweep (R660/R663/R664/R665/R666/R667/R668/R669) PASS verify + commit + Mnemosyne R670.B entry.

---

【불변 운영 원칙】 (첫 7줄 — 매 세션 동일)
- 비용 무시. 항상 장기적으로 올바른 textbook-canonical 선택
- **진짜 북극성 = AAA game shippable + Unreal-class editor self-hosted in pinion itself, AI-introspection 1st-class through every phase.** 4-phase progression: A. Foundation (현재 ~80%, R655-R667 todomvc+settings panel) → B. Professional GUI (Qt/Flutter/Compose-class + multi-window + DCC widget catalog, R700+) → C. Game engine substrate (§2 #4 immediate-mode game loop ↔ retained widget tree dual execution + 3D + assets + physics + audio + PBR, R1000+) → D. AAA game maker (editor self-hosted in pinion + visual scripting + Nanite/Lumen-class rendering + multiplayer netcode, R2500+). 현재 가중 진척 ~7%. R666-R667 cascade 후 ~8%. R667 = Phase A 종료 = 진짜 북극성의 ~5%, **NOT 북극성 도달**
- 부채 즉시 상환. 라운드 안 발견 부채 inline 청산, carry 영원 누적 금지. 이전 라운드 honest 약점 → 다음 라운드 inline 청산 mandatory. 외부 의존 (vendor/sce upstream, 환경) 만 honest carry 정당
- 라운드 자동 선택. 세션 80% 까지 계속
- "부채는 양파다" — 청산 시 새 부채 surface 정직 받아들임
- 1 commit = 1 round = 1 atomic Mnemosyne entry
- 사용자 명시 동의 없으면 git push 금지 (CLAUDE.md 영구 원칙). "진행" / "continue" / "go" 는 push 권한 아님

【다음 세션 진입 (single-command entry)】

새 세션 첫 입력으로 다음 중 하나 — 결과 동일 (SEED self-contained):
- `load` (Serena MCP session-loading skill — pinion 프로젝트 활성화 시 SEED + memory 자동 hydrate)
- `@docs/SEED_PROMPT.md 읽고 R<현재 라운드> 자동 진행`
- 단순 `R<현재 라운드> 진행` (CLAUDE.md Reading order step 1 이 SEED 로 가리킴)

이 SEED 가 self-contained 보증:
- 불변 운영 원칙 (첫 7줄) — 매 세션 동일
- 직전 5+ 세션 honest 결과 — 누락 없는 land 추적
- 다음 텍스트북 캐논 (현재 라운드 atomic list) — concrete + scope-bounded
- watch out + lessons — 영구 + 누적

세션 진입 시 위 3+1 절 읽고 atomic list 의 첫 atomic 부터 자동 진행. 이전 세션의 commit / 변경 사항은 git log + GENERATED.md 가 source of truth (SEED 의 "직전 세션 결과" 가 human-readable mirror).

【진입 시 필독 순서】
1. `docs/SEED_PROMPT.md` (이 파일 — R667+ matters 의 baseline; single-command entry point)
2. `docs/GENERATED.md` §1 Vision (R663.5 정정: 4-phase) + §2 invariants (R663.5 #4 elaboration) + §3 capability boundaries (R665 External(opaque) escape hatch 첫 실증) + §5.15 8-point contract
3. `mnemosyne://concepts/overview` + anti-patterns + atomic-store + frozen-ledger
4. `CLAUDE.md` (R663.5 H1 + Project identity + #4 elaboration)
5. `~/.claude/CLAUDE.md` + `COMMIT_FORMAT.md`
6. `git log --oneline -30` (R635-R665)
7. `memory/MEMORY.md` — 특히:
   - `[[true-north-star-phases]]` ★ R663.5, 가장 중요
   - `[[project_scope_game_engine]]` ★ R663.5 정정
   - `[[r665-storage-substrate]]` ★ R665 신규 (작성 예정)
   - `[[r664-todomvc-edit-in-place]]` (R664 substrate consumer)
   - `[[r663-double-click-primitive]]`
   - `[[r662-sce004-access-child-invoke]]`
   - `[[r661-doc-compression]]`
   - `[[r660-todomvc-debt-clearance]]`
   - `[[r47-class-incident-prevention]]`
   - `[[abstraction-needs-second-consumer]]`
   - `[[substrate-incompleteness-signal]]`
   - `[[textbook-long-term-correct]]`
   - `[[owner-cache-typed-key]]` / `[[owner-cache-no-nested-factory]]` (R665 신규)
   - `[[multi-external-substrate-extra-externals-pattern]]`
   - `[[ai-first-rpc-introspection-obligation]]`
   - `[[sce-priority-over-pinion]]` / `[[sce-upstream-debts]]` (SCE-004)
   - `[[r650-widget-tag-walk-back]]`

【직전 5 세션 결과 — honest 누적 평가】

land 완료 (R673 → R672 → R671 → R670.B → R670.A → R669 → R668 → R667 → R666 → R665 → R664 → R663.5 → R663 → R662 → R661 cumulative):

- **R673** (TreeView 2nd consumer + WAI-ARIA tree kbd model + AriaRole Tree/TreeItem + view_tree layout fix) `4a5a694`:
  - **Atomic (0) ✓** — `pinion_a11y::AriaRole::Tree` + `AriaRole::TreeItem` variants. `to_accesskit()` lowers to `Role::Tree` / `Role::TreeItem`. `aria_name()` returns 'tree' / 'treeitem'. `add_actions_for_role` joins Tree to focus-only container set (parallel to Listbox / RadioGroup / List); TreeItem to commit-class atomic set (parallel to Button / ListBoxOption — Click + Focus).
  - **Atomic (1) ✓** — `TreeViewFocus { focused_id: Option<&str> }` + `view_tree_focused(tag, items, theme, style, focus)`. `view_tree` (R671 read-only entry) becomes wrapper. Focused row paints with M3 `SurfaceContainerHighest`; non-focused 행은 transparent. **view_tree 레이아웃 정통 정정**: outer container `AlignItems::Stretch` (rows fill cross-axis 너비); rows `JustifyContent::Start` + `Size { width: Auto, height: row_height }`. **Glyph column alignment**: ▶/▼/NBSP wrapped in fixed-width Container (`style.glyph_size_px = 24px`) so leaf NBSP ↔ branch ▶/▼ 컬럼이 정렬됨 (verified via scene/layout: 모든 depth-1 행의 label x=84).
  - **Atomic (2) ✓** — `examples/hello-tree-view` 18번째 example, ~470 LOC binding. Sample 파일 트리 데이터 (`FileNode { id, label, expanded, children }`) carried in `Signal<Vec<FileNode>>`. `Owner::cache` hook `use_tree_state` shares instance. `apply_key` WAI-ARIA tree keyboard model: Arrow Up/Down navigate (wrap at edges), Arrow Right expand branch (no-op on leaves), Arrow Left collapse branch, Home/End jump first/last visible, Space/Enter toggle expand. View fn calls `view_tree_focused(TREE_TAG, items, theme, style, &focus)`. Invisible Button at root keeps framework SCXML state surface alive (binding is keyboard-driven, no visible Button paint).
  - **Atomic (3) ✓** — `tools/demos/r673_tree_view_interactive.py` (≥18 assertion). Sections (A) substrate shape / (C) Arrow Up/Down nav cycle / (G) Home/End jump / (D)(E)(F) Arrow Right/Left + Space toggle / (H) leaves no-op on expand keys / (I) header+footer 렌더. 13-demo regression sweep PASS bit-identical.
  - **R673 verification**: cargo test --workspace PASS (43 widget-paint tests including 11 new r671_tree_view_* substrate tests; 4 R671 tests updated to walk recursively for now-wrapped glyph TextNode), cargo clippy --workspace --all-targets clean, **13-demo regression sweep PASS** deterministic (3/3 consecutive r673 runs).
  - **honest LOC 실측**: ~+1050 net. AriaRole +35 (Tree+TreeItem in 3 files); tree_view substrate +110 (TreeViewFocus + view_tree_focused + layout fix + glyph column wrap); hello-tree-view binding +470 + Cargo+build+xml; r673 demo +260; workspace +1.
  - **honest 부채 surface (R674 mandatory)**: (a) click-to-expand on tree rows — R674 atomic (0)+(1) 청산 mandatory; (b) per-row TreeItem AccessNodes — R674 atomic (2) 청산 mandatory; (c) multi-select / drag-drop / virtualization — R675+ candidates.

- **R672** (per-window InputRouter foundation; multi-window race **구조적으로 closed**) `79684f4`:
  - **Atomic (0) ✓** — `pinion_runtime::CoreShell.routers: HashMap<String, InputRouter>` keyed by canonical `WindowSpec::id`. New `pinion_runtime::DEFAULT_WINDOW: &str = "main"` constant. Constructor seeds DEFAULT_WINDOW entry. 11 _for_window CoreShell input methods: `update_paint_scene_for_window` / `cursor_moved_for_window` / `cursor_left_for_window` / `pointer_down_for_window` / `pointer_up_for_window` / `pointer_cancel_for_window` / `touch_event_for_window` / `wheel_for_window` / `scroll_key_for_window` / `hover_target_for_window` / `captured_target_for_window`. Pre-R672 methods become thin wrappers forwarding to `*_for_window(DEFAULT_WINDOW, ...)` — 200+ existing test sites + every single-window binding pay original signatures unchanged.
  - **Atomic (1) ✓** — `pinion_shell::ShellCore` 10 _for_window 변형 (cursor_moved_for_window / cursor_left_for_window / mouse_pressed_for_window / mouse_released_for_window / touch_event_for_window / wheel_for_window / finalize_frame_for_window / drain_deferred_inputs_for_window / click_to_focus_for_window / handle_touch_for_window). `AppShell::window_event` 가 winit WindowId → 정통 WindowSpec::id 해결 via `self.windows.get(&window_id).map_or(DEFAULT_WINDOW, |s| s.spec_id)` + 모든 pointer arm 이 `*_for_window` 변형 dispatch. `AppShell::render_window` 가 `finalize_frame_for_window(spec_id, paint_scene, paint_layout)` 통해 each window의 per-slot router에 paint scene 전달 — cross-window paint cycles 가 더 이상 서로 last_paint_scene 덮어쓰기 없음. `dispatch_rpc_inner` drains deferred inputs through `drain_deferred_inputs_for_window(window_id.unwrap_or(DEFAULT_WINDOW), &inputs)`.
  - **Atomic (2) ✓** — `pinion_tui::ShellCoreTui` 변경 없음 — terminal 구조적 single-window, every CoreShell call 그대로 DEFAULT_WINDOW router로 forward. R670.B / R671 TUI substrate (drain_deferred_inputs / RPC ingress)는 race-immune at one window.
  - **Atomic (3) ✓** — `tools/demos/hello_multi_window_r670b.py` step (5) reverted to `scene/click {path: "main_btn"}` (R671 carry workaround scene/invoke retire). 5/5 PASS deterministic. New `tools/demos/r672_multi_window_race_free.py` dedicated demo: section (A) scene/click race-free; (B) scene/click after forced inspector repaint (pre-R672 canonical failure case); (C) `scene/click {window: "inspector"}` 가 main SCXML corruption 없음; (D) per-window scene/layout distinct; (E)+(F) cross-window structure pins. 12-demo regression sweep PASS bit-identical.
  - **R672 verification**: cargo test --workspace 3449 passed / 0 failed (R671과 동일 — substrate refactor 가 additive). cargo clippy clean. r672_multi_window_race_free.py 5/5 PASS deterministic.
  - **honest LOC 실측**: ~+600 net (core_shell.rs +260 / substrate.rs +180 / app.rs +30 / r672 demo +200).
  - **honest 부채 surface (R673 mandatory + 영구)**: 영구 carry — multi-window animation tick share (one tick per ShellCore-frame), pinion-tui multi-window 영구 미지원. R673+ candidate (closed in R673): TreeView keyboard navigation.

- **R671** (R670.B carry 3개 청산 + Phase B widget catalog 첫 진입 (TreeView)) `6a7b955`:
  - **Atomic (0) ✓** — `ShellCore::compute_paint_scene_internal(window_id: Option<&str>, w, h)` private fn unify. 두 변형 (single-window primary, multi-window per-window)이 thin wrapper. Paint-pipeline parity drift regression class 영구 청산. [[r670b-paint-scene-producer-parity]] long-term unify 부채 청산.
  - **Atomic (1) ✓** — `pinion_shell::WindowSlot.last_paint_layout: Option<LayoutNode>` per-window snapshot field. `AppShell::render_window` builds LayoutNode once via `pinion_rpc::build_layout_node` (lift from inside `ShellCore::finalize_frame` to caller for single walk). `ShellCore::finalize_frame(paint_scene: Scene, paint_layout: LayoutNode)` signature change (accepts pre-built layout). `ShellCore::dispatch_rpc_for_window` gains `slot_paint_layout: Option<&LayoutNode>`; substrate's `last_paint` is `slot_paint_layout.or(self.last_paint_layout.as_ref())`. AppShell.dispatch_rpc threads slot's `last_paint_layout.clone()` via `spec_id_to_window_id` resolve.
  - **Atomic (2) ✓** — `pinion_rpc::parse_request(&str) -> Result<Request, String>` + `pinion_rpc::dispatch_parsed(ctx, Request) -> Option<String>` public extracts. Existing `pinion_rpc::dispatch` thin wrapper. `pinion_shell::ShellCore::dispatch_rpc_for_window` takes `Request` (pre-parsed). `AppShell::dispatch_rpc` parses envelope ONCE + extracts `params.window` from Request.params + hands same Request to substrate. `parse_rpc_window_id` 삭제.
  - **Atomic (3) ✓** — `pinion_widget_paint::tree_view` 신규 module. `TreeViewStyle::m3_default()` M3 Lists tokens (row_height 48 / indent_step 16 / glyph_size 24 / font_size 16 / row_padding 12 / glyph_label_gap 10). Recursive `TreeItem { id, label, expanded, children: Vec<TreeItem> }`. `view_tree(tag, items, theme, style) -> Scene` depth-first flat row paint; composite tags `{tree_tag}#{node_id}`; expand glyphs U+25B6/U+25BC/U+00A0; `composite_row_tag` helper. 11 unit tests.
  - **Atomic (4) ✓** — `hello-multi-window` inspector window upgrade. Single `inspector_state_text` Container → `view_tree` consumer rooted at `inspector_tree`. `scene_to_tree_item(scene, id) -> TreeItem` walker descends Scene::Container / Text / External 변형. Inspector tree 가 leading 'State: {variant}' leaf row (composite tag `inspector_tree#state`) + 'main window scene' branch (recursive main paint scene mirror). 5+ assertion demo + 11-demo regression sweep PASS.
  - **R671 verification**: cargo test --workspace 3449 passed (+12 R671 tests), cargo clippy clean, 11-demo regression sweep PASS.
  - **honest LOC 실측**: ~+1100 net (substrate +414 / tree_view module +590 / hello-multi-window inspector rewrite +35 / r671 demo +260).
  - **honest 부채 surface (R672 mandatory)**: multi-window InputRouter race ([[multi-window-input-router-race]]) — R672 atomic (0)+(1) 청산 mandatory. TreeView keyboard navigation — R673 atomic (2) 청산 mandatory.

- **R670.B** (AppShell multi-window refactor + RPC window param + `hello-multi-window` first consumer — Phase B first real dogfood) `9c34251`:
  - **Atomic (0) ✓** — `pinion-shell::AppShell` multi-window refactor. 5 single-window fields lifted into `WindowSlot { render, vello_scene, accesskit, ime_was_composing, last_ime_cursor_area, pending_intrinsic_resize, spec_id }`. `AppShell::windows: HashMap<WindowId, WindowSlot<V::Renderer>>` cluster lift + `spec_id_to_window_id: HashMap<&'static str, WindowId>` reverse lookup + `primary_window_id: Option<WindowId>` for default-scope. `resumed()` walks `V::windows()` list + calls new `resume_spec()` helper per spec (one winit `Window` + `RenderState` + `accesskit_winit::Adapter` per spec; `set_min_inner_size` floor + `IntrinsicAfterFirstPaint` post-first-paint resize queued per-spec). `window_event(window_id, event)` dispatches per-window — `forward_to_accesskit(window_id, &event)` lookup-by-id, `RedrawRequested → render_window(window_id)`, `Ime → slot.ime_was_composing` per-slot, `Resized → slot.render` per-slot. `render()` split into `render_window(window_id)` + two helpers (`emit_accesskit_for_window`, `publish_ime_for_window`) to stay under the workspace `clippy::too_many_lines = 100` ceiling. `drain_redraw_to_winit()` walks all windows. `suspended()` walks all slots + drops their GPU renderers. Single `ShellCore` (Approach A — same binding state, different views per window); multi-binding (different ShellCores per window) is R750+ territory.
  - **Atomic (1) ✓** — RPC `{window: "<id>"}` param + `WidgetView::view_for_window` hook. `pinion_rpc::DispatchContext` gains `window_id: Option<&'a str>` + `with_window(id)` builder. `pinion_shell::AppShell::dispatch_rpc` parses `params.window` off the JSON-RPC frame via new `parse_rpc_window_id` helper (serde_json sniffer; failure falls through to substrate parser); `resolve_spec_id` resolves the supplied id against the spec map with primary fallback for unknown ids. `ShellCore::dispatch_rpc_for_window(request, window_id, resize)` per-window variant + private `dispatch_rpc_inner` shared body; producer closure routes through `V::view_for_window(window_id, state, frame)` when Some + `V::view(state, frame)` when None. `WidgetView::view_for_window(window_id, state, frame) -> Scene` trait method (default forwards to `Self::view` so 15+ single-window bindings stay bit-identical). `ShellCore::compute_paint_scene_for_window(window_id, w, h)` mirror of `compute_paint_scene` routing through `view_for_window`; includes the R51.147 `any_animation_active → redraw_requested` parity bit (the missing parity was caught + fixed mid-refactor via R670.B sub-round detection — todomvc_r665 regression flagged the missing animation-loop heartbeat).
  - **Atomic (2) ✓** — `examples/hello-multi-window` first consumer binding + `tools/demos/hello_multi_window_r670b.py` (12+ assertion demo). 17th example crate. `V::windows()` returns `vec![WindowSpec::new("main", "...", Fixed{320, 200}), WindowSpec::new("inspector", "...", Fixed{280, 140})]`. `V::view_for_window` switches: "inspector" → state-debug text rendering `format!("{:?}", state)` of the shared `ButtonState`; everything else → main view with Button widget. Single `ShellCore` underlies both windows so main's state mutations propagate to inspector on the next paint cycle. Demo verifies: scene/snapshot `{window:"main"}` ↔ `{window:"inspector"}` separation, main click → inspector mirror (Idle→Hover propagates), scene/invoke /external/send Disable → inspector mirrors Disabled, unknown window id falls back to primary.
  - **R670.B verification**: **3437 workspace tests** (was 3431 at R670.A → +6 hello-multi-window r670_b_multi_window_tests), `cargo clippy --workspace --all-targets` clean, **10-demo regression sweep PASS** (R660 / R663 / R664 / R665 / R666 / R667 / R668 / R669 + R670.A carry clearance + R670.B hello-multi-window — all bit-identical, zero regression), tools/demos/hello_multi_window_r670b.py PASS (2.62s — multi-window dispatch end-to-end).
  - **honest LOC 실측**: ~+1300 net (+880 substrate diff: pinion-shell app.rs WindowSlot + multi-window refactor + helpers ~+550 / dispatch_rpc window threading +120 / compute_paint_scene_for_window +50 / view_for_window trait method +50 / Cargo.toml serde_json dep +5 / pinion-rpc DispatchContext.window_id + builder +80 / WindowSpec::main fallback +25; +420 hello-multi-window binding + +260 demo). Lower than seed estimate +2000-3000 because the AppShell refactor used a single WindowSlot cluster struct instead of multiple per-field hashmaps (canonical Rust idiom for borrow-disjointness across per-window state).
  - **honest 부채 surface**: (a) `scene/layout {viewport: null}` is single-binding-scoped (one `last_paint_layout` per ShellCore); multi-window per-window layout snapshots would need a per-WindowSlot field — substrate-incompleteness signal carries until DevTools wants per-window introspection; (b) animation tick is shared across windows (one tick per ShellCore-frame); multi-window paints in the same event-loop iteration compound the tick — honest carry to be addressed when a real per-spec animation timing requirement surfaces; (c) `pinion-tui` does not implement `WidgetView::view_for_window` because terminal is 1 process = 1 alternate-screen = 1 window structurally — TUI bindings will always use the default impl forwarding to `view`; (d) `parse_rpc_window_id` uses `serde_json::from_str` which re-parses the same frame the substrate later re-parses for dispatch — minor double-parse cost on every RPC frame; carries until a real-world performance signal demands a single-parse pipeline.

- **R670.A** (R668/R669 carry 100% clearance + Phase B trait foundation — atomics 0+1+2; atomics 3+4+5 → R670.B) `4a5ad36`:
  - **Atomic (0) ✓** — `pinion-tui` full RPC ingress (R668 carry #2 영구 청산). 5 substrate field lifts onto `ShellCoreTui` (`previews: PreviewLedger` + `revision: SceneRevision` + `focus: FocusManager` + `last_paint_layout: Option<LayoutNode>`); `ShellCoreTui::dispatch_rpc(request: &str) -> Option<String>` mirrors `pinion_shell::ShellCore::dispatch_rpc` (disjoint-field borrow split + paint-producer closure + deferred-input drain + focus_before/after notify); `finalize_paint_snapshot(&Scene)` refreshes `last_paint_layout` so `scene/layout {viewport: null}` returns the geometry the crossterm shell just painted; `spawn_stdin_rpc_reader_tui` background thread reads JSON-RPC lines via `BufRead::lines` + forwards through mpsc::Sender; `pinion-tui::shell::run` event-loop integration with **stderr response writer** (alternate-screen + raw-mode terminal owns stdout, so RPC response wire lives on stderr per the canonical Unix diagnostic-stream convention); 3 helper extracts (`commit_and_finalize` / `drain_intents_into_substrate` / `drain_rpc_into_substrate`) keep `run_impl` under the workspace `clippy::too_many_lines = 100` ceiling. `handle_tail` return OR'd with focus_change so `notify_focus_change` fires on RPC-driven focus mutation (mirror of pinion-shell `External::on_focus_change` arc). **9 integration tests** (`crates/pinion-tui/tests/rpc_ingress.rs`): scene/snapshot, scene/click drains to Hover, scene/key named Space, scene/key character 'd' → Disabled, scene/invoke /external/send Disable, focus/get fresh = null, focus/set targets tag, scene/click bumps OCC revision, malformed JSON-RPC error envelope.
  - **Atomic (1) ✓** — `examples/hello-popover` first real `SizeStrategy::IntrinsicAfterFirstPaint` consumer (R668 carry #1 + R669 carry #1 영구 청산). 16th example crate; ~470 LOC binding (`examples/hello-popover/src/main.rs`) + `app.pinion.xml` + `build.rs`. Header text + 3 body status rows + Button dismiss trigger laid out vertically with no root-size lock — the substrate test pins `LayoutStyle::size.width == Auto && height == Auto` so a regression that adds a fixed-size lock surfaces immediately. `initial_size_strategy()` declares `IntrinsicAfterFirstPaint { min: (240, 100), max: (480, 400) }`; **new `pinion-derive` macro flag `initial_size_strategy`** (one-line `KNOWN_FLAGS` extension + body selector) opts the binding into forwarding to the inherent fn instead of the auto-emitted `Fixed { width, height }` — surgical macro change, zero impact on 15+ existing bindings. 5 unit tests (`r670_intrinsic_first_paint_tests`): strategy declaration pin, root-size lock absence pin, ARIA Button role pin, widget tag literal pin, painted-scene non-zero intrinsic bbox pin via `pinion_runtime::compute_layout`. **R670.A scope clarification — `IntrinsicAfterFirstPaint` is one-shot per binding lifetime**; the binding demonstrates the one-shot capability honestly. Dynamic shrink-wrap-on-state-change ("click button → window expands") would require either explicit `scene/resize` RPC calls from the binding or a substrate extension lifting the one-shot guard — deferred to a future round once a real use case (collapsible dialog "more options" section, etc.) surfaces the substrate-incompleteness signal.
  - **Atomic (2) ✓** — `pinion_shell::WindowSpec` + `WidgetView::windows()` trait extension (Phase B substrate foundation, +110 net LOC in pinion-shell). `WindowSpec { id: &'static str, title: String, strategy: SizeStrategy }` with `WindowSpec::main(title, strategy)` (canonical primary `id = "main"`) + `WindowSpec::new(id, title, strategy)` (secondary windows). `WidgetView::windows() -> Vec<WindowSpec>` trait method with backward-compat default `vec![WindowSpec::main(Self::title(), Self::initial_size_strategy())]` — every existing single-window binding (15+ in the example gallery) keeps its lifecycle bit-identical. 2 unit tests pin `id = "main"` for the canonical primary + arbitrary `id` for `WindowSpec::new`. **`AppShell::resumed` still reads only `Self::title()` + `Self::initial_size_strategy()` directly** (i.e., the default single-window path) — multi-window dispatch (walking the full `Vec<WindowSpec>` and creating per-spec winit Window + RenderState + accesskit_winit::Adapter) is R670.B work. Atomic (2) is a forward-compat foundation: the trait surface exists, no consumer breaks, but nothing actively reads the new `windows()` method yet.
  - **R670.A verification**: **3431 workspace tests** (was 3415 at R669 → +16 R670.A; +9 pinion-tui rpc_ingress.rs + +5 hello-popover + +2 pinion-shell WindowSpec), clippy clean, **8-demo regression sweep PASS** (R660 / R663 / R664 / R665 / R666 / R667 / R668 / R669 all bit-identical — zero regression), R670.A demo PASS (`tools/demos/r670a_carry_clearance.py` — IntrinsicAfterFirstPaint window-size verify post-first-paint: root rect h > 100 floor confirms substrate walked content bbox vs clamping at min).
  - **honest LOC 실측**: ~+1340 net (+613 substrate diff: pinion-tui substrate +280 + shell +110 + tests +280 + pinion-shell WindowSpec +110 + pinion-derive macro flag +30; +470 hello-popover binding + ~150 demo + ~+150 SEED). Lower than seed total 2500-4200 because atomics 3+4+5 (AppShell multi-window refactor + RPC window param + hello-multi-window + heavy demo) deferred to R670.B.
  - **honest 부채 surface (R670.B mandatory)**: (a) `AppShell::resumed` does not yet walk `V::windows()` — single-window binding compat is preserved by the default trait impl but multi-window actually needs the AppShell refactor; (b) RPC `{window: "<id>"}` param is not yet wired — all dispatch frames implicitly target the (only) `"main"` window; (c) `hello-multi-window` first consumer waits on (a)+(b); (d) `pinion-tui::shell::run` stdin RPC path is enabled in all TUI bindings now — any binding running under a non-TTY stdin (CI pipe) will get raw-mode-enable rejection per crossterm's contract, so the demo's atomic (0) smoke is skipped on non-TTY; substrate-level RPC ingress is covered by the 9 integration tests directly. None of these are external-dependency carries — they are all R670.B in-scope work.

- **R669** (R668 atomic 4 carry 청산 — 5 of 6 atomic land + R670 IntrinsicAfterFirstPaint application carry honest) `474c7e7` + `01239bc`:

- **R661** (process maturity) `daf2a99`:
  - todomvc/src/main.rs 4496 → 3700 LOC (-796 net, -17.7%)
  - WHY-keep / WHAT-strip / HOW-strip; spec refs + memory links 보존
  - Zero behaviour change (R660 demo bit-identical)
  - Doc-density baseline for future composed-app rounds

- **R662** (substrate extension + upstream debt) `2d262ad`:
  - WidgetA11y::access_child_invoke + parent_tag arg (multi-composite disambiguation)
  - todomvc filter AT-action wire (Click/Default/Focus → RadioGroupExternal)
  - ScrollBarInteractionSignal stop-gap doc-anchor + SCE-004 등록

- **R663** (framework-first input primitive) `d8e6810`:
  - DeferredInput::DoubleClick + shell drain (cursor + 2x press/release)
  - scene/double_click RPC handler + tf.double_click() Python harness
  - tools/demos/double_click_r663.py smoke (TodoToggleExternal 2x flip)

- **R663.5** (Vision 정정 라운드) `bde04f7`:
  - docs/GENERATED.md §1 Vision: 4-phase progression (A/B/C/D) 명시
  - docs/GENERATED.md §2 #4: mode toggle = Phase C entry (NOT GUI diff opt) caveat 5건
  - CLAUDE.md H1 + Project identity + Hard invariants #4: 4-phase 표 + 진짜 northern-star (AAA + editor self-hosted)
  - memory: `[[true-north-star-phases]]` 신규 + `[[project_scope_game_engine]]` 정정
  - **honest 진척 재평가**: 이전 "R667 settings panel = 북극성" misdirection 정정. R667 = Phase A 종료 = 진짜 북극성의 ~5%

- **R664** (edit-in-place + R663 paint-side consumer 청산) `501f304`:
  - InputRouter W3C native paint-side double-click (300ms / 5px threshold matrix)
  - pinion-core::focus_request mailbox primitive + ShellCore drain
  - TodoEditExternal (5th ExtraExternal) + EDIT_TF_TAG (6th, TextField inline editor)
  - view_field 3rd consumer (R657 lift ROI 확정) + TextDecoration::strikethrough() 2nd consumer
  - access_child_invoke 4-of-4 application consumer (filter + delete + toggle + item)
  - 34-assertion R664 demo + 3328 workspace tests pass

- **R665** (External(opaque) persistence 첫 실증 — Phase A 70%→~80%) `bf23117`:
  - pinion-core::storage 신규 (Storage trait + InMemoryStorage; bytes-only + total surface) — Clipboard 의 mirror substrate
  - pinion-platform-storage 16번째 워크스페이스 crate (FileStorage + open_app_storage; atomic write via tempfile + sync_all + rename; 200-char key sanitization; dirs::data_dir 으로 XDG / Apple / Windows known-folder 해결)
  - examples/todomvc PersistedState 단일 blob schema (todos + filter + next_id; editing_id 의도적 transient 제외) + use_storage + use_persistence_boot (hydrate → batch seed → Effect 설치)
  - PINION_STORAGE_DIR env override (테스트 isolation)
  - 46-assertion R665 demo (정통 launch-kill-relaunch 사이클; schema mismatch + corrupted bytes 복구; filter cycles 영구화)
  - 3352 workspace tests + clippy clean
  - **R663-R664 honest 부채 6개 청산 ✓** (R664 inline mandatory list 전부 처리)

- **R669** (R668 atomic 4 carry 청산 — 5 of 6 atomic land + R670 IntrinsicAfterFirstPaint application carry honest) `474c7e7` + `01239bc`:
  - **Atomic (0) — Persistence schema v1→v2 migrator**: `SettingsPersistedState` gains `notifications: [bool; NOTIFICATION_COUNT]` field; `PERSISTED_SCHEMA_VERSION` bumped 1→2. `SettingsPersistedStateV1` explicit decoder + `migrate_v1_to_v2` back-fills the missing field with `NOTIFICATION_DEFAULTS` so v1-on-disk records survive the upgrade without losing nav_index / dark_mode / font_scale / display_name. Hydrate path tries v2-decode first, falls back to v1-decode + migrate. First implementation of the R665 schema-migrator carry; textbook canonical pattern for every future breaking-change axis.
  - **Atomic (1) — 6× `CheckboxExternal` composite-tag cluster**: `NOTIFICATION_COUNT=6` + `NOTIF_INSTANCE_TAGS` static array (`notifications#0`..`notifications#5`); `use_notification_channels()` Owner::cache hook holds 6 `Rc<Signal<bool>>` mirrors; `create_extra_externals` registers each CheckboxExternal as an `ExtraExternal` with the composite tag, seeding from the hydrated Signal value. R55.D.5 multi-External substrate fully exercised at N=6 (was 5-of-5 in R660). V::update reducer arm parses composite-tag intent (`notifications#i.checked`) and mutates the Signal mirror.
  - **Atomic (2) — `view_notifications_section` rewrite (`pinion-widget-paint::checkbox` 2nd application consumer)**: 6 `view_checkbox` rows (the lifted M3 row composition from R668 atomic 2) wrapped in a `Scene::Scroll` viewport with the title above. `read_notification_states` walks the per-channel SCXML state from the live state scene; `read_notification_checked` walks the per-channel `value` slot (the post-click source-of-truth lives in the External, the Signal handles are reactive mirrors). Both walkers Owner-scope-free per the canonical `read_state` contract. `RootState` tuple expanded to 9 slots.
  - **Atomic (3) — 4th `ScrollBarExternal` consumer**: `NOTIF_SCROLLBAR_TAG = "notifications_scrollbar"`; `use_notif_scrollbar()` delegates to canonical `pinion_core::widgets::scroll::use_scroll_state(tag)` hook. Notifications viewport (480 × 280) overflows the 6-row content (~336 px) so the visible scrollbar paint + drag arc is always exercised. `view_vertical_scrollbar` paint + `ScrollBarExternal::new().attach_state(…).attach_interaction(use_scrollbar_interaction(tag))` mirrors the todomvc pattern exactly.
  - **Atomic (4) deferred — `SizeStrategy::IntrinsicAfterFirstPaint` opt-in**: substrate from R668 in place, but the settings-panel root view fn currently locks `with_size(Size::px(WIN_W, WIN_H))` so the intrinsic-bbox walker measures 720×480 (identity, no resize). Genuine auto-resize requires a layout redesign (root shrink-wraps to content + per-section variable size). Honest carry to R670 — substrate exists, first real application consumer waits for a binding that naturally benefits (a popover / dialog / tooltip without a fixed root size).
  - **Atomic (5) — demo + verification**: `tools/demos/settings_panel_r669.py` (20 assertions covering atomic 0 / 1 / 2 / 3; cycle 1 verifies fresh-boot v2 schema, cycle 2 verifies v1→v2 migrator preserves v1 fields + back-fills notifications, cycle 3 verifies all 6 composite-tag instances + scrollbar + paint-side tag emission). 8-demo regression sweep PASS (R660 / R663 / R664 / R665 / R666 / R667 / R668 / R669 all PASS — R667 demo updated to accept either v1 or v2 schema_version post-bump). 3415 workspace tests pass; clippy clean.
  - **honest LOC 실측**: ~+750 net (settings-panel binding +650 LOC for notifications cluster + schema migrator + view fn rewrite + read_state expansion; R667 demo schema-accept update +10; R669 demo +230 new). Lower than seed atomic-4 estimate +600-800 because the IntrinsicAfterFirstPaint application opt-in carry shaved ~150 LOC; substrate slice was already in place from R668.
  - **honest 부채 surface**: (a) `SizeStrategy::IntrinsicAfterFirstPaint` first real application consumer — substrate from R668 in place, first consumer needs a binding without a fixed root size; (b) Persistence schema v2→v3 migrator pattern is now textbook (carry from R665 cleared on the v1→v2 implementation); (c) `read_notification_*` per-channel walkers iterate 6 composite tags — at N=6 negligible, but a 2nd composite-tag-cluster consumer would surface the need for a substrate-side `read_composite_tag_value_slots` helper.

- **R668** (Phase A close substrate — 4 of 6 atomic land + R669 carry honest) `cc11fad` + `2218f5d`:
  - **Atomic (0) — `pinion-shell::SizeStrategy` window-creation policy**: new `SizeStrategy` enum (`Fixed { width, height }` + `IntrinsicAfterFirstPaint { min, max }`) supersedes the legacy `initial_size()` trait method (every binding migrates explicitly via `initial_size_strategy()`); winit `with_min_inner_size` floor anchored at min so user-driven OS-resize stops at the declared minimum; post-first-paint walker (`Scene::intrinsic_content_size`) + `request_inner_size` wire for IntrinsicAfterFirstPaint; headless screenshot two-pass; `pinion-derive` `#[widget(initial_size = (W,H))]` emits new method shape; 15 binding migration (8 derived = no code change, 7 manual = one-line). 8 new unit tests (`r668_intrinsic_content_size_*` × 6 + `r668_size_strategy_*` × 2). WIN_H magic carry **substrate-side cleared** — bindings still declare WIN_W/WIN_H but as explicit `Fixed { width, height }`; the trait-level "magic" is gone.
  - **Atomic (1) — `pinion-tui::ShellCoreTui::drain_deferred_inputs` substrate**: pinion-tui depends on pinion-rpc (no winit transitive); `drain_deferred_inputs` mirrors `pinion-shell::ShellCore` pattern, mapping all 6 `DeferredInput` variants (Wheel/Click/DoubleClick/Key/CharacterKey/Drag) to ShellCoreTui dispatch methods. 8 integration tests pin substrate inheritance (`r668_drain_*` × 8) demonstrating §2 #6 GUI/TUI dual invariant. **Honest carry**: full stdin RPC ingress (PreviewLedger/SceneRevision/FocusManager field lifts onto ShellCoreTui + stdin reader thread + stderr response writer) is `[[pinion-tui-rpc-ingress]]` follow-up consumer of the substrate primitive landed here; complete and textbook canonical.
  - **Atomic (2) — `pinion_widget_paint::checkbox::view_checkbox` substrate lift**: new module mirrors `text_field` pattern — `CheckboxStyle::m3_filled()` (BOX_SIZE=24, BOX_RADIUS=4, border=2, ROW_GAP=10, font=16, glyph=18) + `view_checkbox(tag, state, checked, theme, style, label) -> Scene` (M3 accent ramp / outline ramp / state-layer overlays) + `checkbox_accent_for` + `checkbox_outline_for`. hello-checkbox is 1st consumer (binding view fn ~120 LOC → ~50 LOC + retained surface chrome). 8 new unit tests (`r668_checkbox_*`). settings-panel Notifications 6-channel = 2nd consumer **R669 carry** (Rule of Three application-side; substrate is 1-of-2 + unit-test consumer).
  - **Atomic (3) — `pinion_core::text_scale::use_text_scale` + `TextStyle::with_size_px` multiplier**: new top-level `text_scale` module — `current_text_scale() -> f32` non-subscribing thread-local + `set_text_scale(value)` (clamp [0.1, 5.0] + NaN guard) + `TextScale` wrapper struct with `.get` (subscribing) / `.peek` (non-subscribing thread-local read) / `.set` (atomic thread-local + signal update) + `use_text_scale() -> Rc<TextScale>` Owner::cache-shared hook. `TextStyle::with_size_px` no longer const, multiplies `size * current_text_scale()` with `max(1)` floor. 9 new unit tests (`r668_text_scale_*` × 8 + `r668_with_size_px_multiplies_*` × 1). a11y substrate Phase B M3 readiness baseline.
  - **Atomic (4) partial — settings-panel font_scale → use_text_scale wire**: slider's `value_changing` / `value_committed` intent path also calls `pinion_core::text_scale::use_text_scale().set(0.5 + v * 1.5)` (maps slider [0.0, 1.0] → text_scale [0.5, 2.0] = M3 a11y range); view fn subscribes via `_scale_subscription = use_text_scale().get()` so the view re-runs on slider drag; persistence hydrate also pushes the persisted slider value through `set_text_scale()` so first paint reflects the user's saved a11y zoom. **Honest carry R669** (full atomic 4): 6-channel Notifications CheckboxExternal cluster (composite-tag `notif#0`..`notif#5`) + `SettingsPersistedState` schema v1→v2 migrator + 4th `ScrollBarExternal` consumer + optional settings-panel opt-in to `SizeStrategy::IntrinsicAfterFirstPaint`. These deferred because session-budget reality vs full-atomic-4 scope (~+500-800 LOC binding work + ~+100-150 LOC migrator); the substrate work landed in atomic (2)/(3) makes the future consumer round straightforward.
  - **Atomic (5) — demo + commit + Mnemosyne entry**: `tools/demos/settings_panel_r668.py` (12 assertions covering atomic 0/3/4-partial substrate); atomic 1 + atomic 2 fully covered by unit/integration tests (16 new tests across pinion-tui + pinion-widget-paint). 6-demo regression sweep PASS (todomvc_r660/r664/r665/r666 + settings_panel_r667 + double_click_r663 all PASS bit-identical = zero regression from R668 substrate changes). 3415 workspace tests pass (was 3389 at R667 → +26 R668 new); clippy clean. Commit `<TBD>` documents 4.5/6 atomic land + R669 carry honest.
  - **honest LOC 실측**: ~+2100 net (+8 files new: SizeStrategy enum + 7 examples migrated + intrinsic_content_size walker; 6 files modified pinion-tui drain; 2 files modified pinion-widget-paint checkbox; 2 files modified text_scale module + style.rs multiplier; settings-panel font_scale wire; demo file). Lower than seed estimate +2670-3700 because atomic 4 ladder + schema v2 deferred. R669 atomic 4 finishing estimate ~+800-1000 LOC.
  - **honest 부채 surface**: (a) pinion-widget-paint::checkbox 2nd application consumer (settings-panel Notifications) = R669; (b) Persistence schema v1→v2 migrator first implementation = R669 (carry from R665, now mandatory when notifications field lands); (c) Settings-panel opt-in to `SizeStrategy::IntrinsicAfterFirstPaint` — substrate exists but no first application consumer yet; (d) `[[pinion-tui-rpc-ingress]]` axis (full stdin RPC ingress for pinion-tui) — substrate exists, ingress wire is follow-up; (e) `SizeStrategy::IntrinsicAfterFirstPaint` first paint-rerun delay (1 extra paint cycle to settle) — observable in headless screenshot two-pass but documented; (f) `with_size_px` thread-local read costs per text-rendering call (negligible but real) — never measured against pre-R668 baseline.

- **R667** (Phase A 종료 — 2nd composed app + resolve substrate lift) `5de7b4d`:
  - pinion-rpc::resolve_external_introspect_mut substrate lift — invoke/intervene/dry_run/rewind/query/simulate 6-file inline duplication (split_at_external + lookup_path_mut + primary_external_mut + introspect_mut) 청산. 5 helper public API + ResolveExternalError 단일 source. simulate.rs 4-internal-site (query_introspect_at + classify_lookup_failure + Phase 2 apply + restore_originals) 도 introspect_mut_at 으로 단일화. R666 carry #1 즉시 상환 ✓
  - examples/settings-panel 신규 binding — 2nd composed multi-widget application (M3 Settings: 좌측 RadioGroupExternal nav rail × 5 sections + 우측 detail pane). Primary TextField (display_name) + 3 ExtraExternal (RadioGroup nav, Toggle dark mode, Slider font scale). 1206 LOC main.rs
  - Storage 2nd application consumer — SettingsPersistedState (schema_v1: nav_index + dark_mode + font_scale + display_name) + use_settings_persistence (R665 use_persistence_boot mirror — pre-resolved cache slots, batched hydrate, Effect-retention). R665 substrate ROI 정통 정당화
  - view_field 4th consumer ✓ (Profile section); view_vertical_scrollbar 4th consumer — settings panel naturally short content (≤480px), R668 carry
  - settings_panel_r667.py demo — 45 assertion (2-cycle launch-kill-relaunch persistence; nav-cycle × 5 + theme toggle + slider 3-value intervene + TextField type + storage blob verify). PASS 2.11s
  - WIN_H magic — settings-panel + todomvc 모두 WIN_H 고정. flex_grow primitive 자체는 R55.G.4 시점부터 존재 (`.with_flex_grow(1.0)` 사용 중). "WIN_H magic 청산" 영구 carry **계속** (실질 청산은 winit-side window-auto-size axis 필요)
  - workspace 0 test failure (3389+ tests) + clippy clean + R666 demo PASS 회귀 0
  - **honest LOC 실측**: ~+1760 net (resolve lift +220 = 6-file -94 + new resolve.rs +314; settings-panel binding +1275; demo +265; -trivial Cargo/build/xml). 1700-2700 seed estimate 의 lower bound ✓ (composed-app + demo density predictable)
  - **honest 부채 1개 surface** (scrollbar 4th consumer 청산 못함 — settings panel 자연 짧음 → R668 carry; 인위적 long content 강제 위배)
  - **3 substrate gap 청산 0개 — settings-panel 부터는 substrate consumer round, gap surface 적음**

- **R666** (AI-first §2 #2 첫 production stress — Phase A ~80%→~85%) `6e41659`:
  - pinion-rpc::invoke / intervene / dry_run R42 mirror migration (rewind.rs canonical) — v1 path `/{tag}/external/{action}` 으로 모든 ExtraExternal singleton 을 base tag 로 addressing. composite-tag (`{tag}#{id}`) 는 paint-side router artefact 임을 명문화. +12 R666 tests (composite-tag DFS, window prefix, unknown segment, non-External target)
  - pinion-core::reactive::Owner::cache nested-factory guard — `try_borrow_mut` 가 cryptic `BorrowMutError` → actionable panic message ("Owner::cache factory closures must not call Owner::cache; pre-resolve dependent slots first") 업그레이드. R665 의 use_persistence_boot 첫 실증 청산 + framework-side guard land. +3 R666 tests (panic 메시지 검증, pre-resolved path 정통, distinct-Owner nesting 허용)
  - pinion-rpc::DeferredInput::CharacterKey 신규 variant + handle_scene_key 가 `key.chars().count() == 1` 자동 판별 → CharacterKey (handle_character_key → V::keybinding intercept); 그 외 → Key (handle_named_key). pinion-shell drain CharacterKey arm. 사전 R666 carry `[[scene-key-character-named-gap]]` 청산. +4 R666 tests (single ascii, U+0020 space, 사전조립 CJK 음절, multi-char W3C named)
  - examples/todomvc — 상속된 `'d'`/`'e'` letter-key V::keybinding intercept 청산 (R655 scaffolding copy-paste from hello-textfield; R666 #3 가 gap 닫은 후 'eggs'/'delete' 타이핑이 깨졌던 latent bug 표면화 → 정통 청산)
  - tools/demos/todomvc_r666.py — 12+ step E2E (cycle 1 boot+type+toggle+filter+edit+commit; cycle 2 relaunch+verify+add+toggle-off+delete; cycle 3 두번째 relaunch+verify 모두 persist). 55 assertion, scene/invoke v1 path 5회 사용, scene/key character arc 모든 typed char 에 사용
  - tools/rpc_verify.py — `isolated_storage_dir(prefix)` context manager helper + `tf.text(body, path)` typing convenience. R666 inline retrofit: todomvc_r655/r656/r658/r659/r660/double_click_r663/r664 모두 `isolated_storage_dir` 으로 wrap → 순차 실행 시 `$XDG_DATA_HOME/pinion-todomvc/` 오염 없음. R665 carry 청산
  - 3369 workspace tests + clippy clean
  - **9-demo sequential regression sweep PASS** (todomvc_r655→r666 + double_click_r663; 두번째 run 도 PASS — per-demo tempdir 격리 검증)
  - honest LOC 실측: **~+1391 net** (20 files: substrate ~+50 / migration + tests ~+250 / demo +563 / harness ~+70 / 7 demo retrofit ~+70 / docs ~+388). estimate 500-800 보다 2× 였음 — composed-app + demo + docs density 정직 carry. R667 estimate (1700-2700) 도 같은 density factor 적용 권장

honest 평가 누적 — R666 inline 청산 = R665 부채 #4 (PersistenceBootMarker, code-side guard 추가로 framework lift 후보 우선순위 명확화) + framework substrate completeness 부채 (Owner::cache nested 룰 code + memory 청산) + 사전 R660+ carry `[[scene-key-character-named-gap]]` + R665 carry todomvc demos pollution (`PINION_STORAGE_DIR` 미설정).

R666 carry (미래 inline 청산 candidate / R667 진입 전 평가):

1. **`pinion-rpc::resolve_external` helper lift** — invoke/intervene/dry_run/query/rewind 5 site 가 동일 패턴 (split_at_external + lookup_path_mut + primary_external_mut) 반복. 6번째 consumer 등장 시 lift (`[[abstraction-needs-second-consumer]]` Rule of Three; 현재 5-of-5 이지만 각자 distinct error enum 보유 — R667 settings panel 의 첫 path consumer 가 결정점)
2. **DeferredInput::CharacterKey explicit-kind override** — 현재 chars().count() 자동 판별이 common case cover. single-char 강제 named-key dispatch 요구 (예: 키 매크로 시뮬레이션) 등장 시 `kind` param 추가 candidate. YAGNI carry
3. **schema_version breaking-change migrator** — R665 carry, 여전히 YAGNI (breaking change 미발생)
4. **next_id Cell non-reactive 의존성** — R665 carry, 2nd writer 등장 전 premature lift (의도 위배)
5. **PersistenceBootMarker Effect-retention substrate quirk** — R665 carry, 2nd consumer 등장 전 premature lift

carry honest (외부 의존, R666 미청산):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- TUI parity (§6 #6 cascade) — pinion-tui 가 DeferredInput drain 미사용 (ShellCore 직접 호출); RPC drain 등장 시 R666 패턴 자동 상속
- Figma API token (영구)
- WIN_H 480 magic (flex-grow primitive 누락; R667 settings panel 첫 consumer 후보)

진척도 (R667 후 — **Phase A 종료**) — **진짜 northern-star 대비**:

| Phase | 비중 | 현재 |
|---|---:|---:|
| A. Foundation (§1-§4 + 첫 composed apps) | 5% | ~97% |
| B. Professional GUI (Qt/Flutter/Compose-class + multi-window) | 25% | 10% |
| C. Game engine substrate (§2 #4 dual execution + 3D + ...) | 35% | 0% |
| D. AAA editor self-hosted | 35% | 0% |

**가중 진척 = 5%×97% + 25%×10% + 35%×0% + 35%×0% = ~7.35%**

R668+ Phase A 잔여 부채 (scrollbar 4th consumer, WIN_H magic 등) 청산이 +0.15%. R700+ Phase B 진입이 진짜 northern-star 의 +5% 가속. Phase C/D 가 진짜 mass (35%+35% = 70% of work).

가시 결과:
- `./target/release/settings-panel` (신규 R667) — M3 좌측 nav rail 5-section (Theme/Appearance/Profile/Notifications/Actions) + 우측 detail pane; theme toggle + font slider + display-name TextField; 모든 변경 즉시 영구화 + exit/relaunch 시 4-field 복원
- `./target/release/todomvc` — Tab/Arrow/Home/End/Space filter cycle + M3 hover/press layers + scrollbar drag + 더블클릭 inline 편집 + Enter/Esc commit/cancel + strikethrough on completed + exit + relaunch 시 state 영구 복원 (R665) + AI 가 scene/invoke v1 path 로 모든 ExtraExternal singleton 을 직접 조작 가능 (R666)
- `python3 tools/demos/settings_panel_r667.py` (45 assertion, 2.11s — 2-cycle launch-kill-relaunch + nav × 5 + theme + slider 3-value + TextField type + storage blob verify)
- `python3 tools/demos/todomvc_r666.py` (55 assertion, 7.12s — 12+ step E2E + scene/invoke v1 path × 5 + scene/key character arc + 3-cycle launch-kill-relaunch) — R667 lift 회귀 0
- `python3 tools/demos/todomvc_r665.py` (46 assertion, 13.47s — launch-kill-relaunch persistence cycle)
- `python3 tools/demos/todomvc_r664.py` (34 assertion, 5.83s)
- `python3 tools/demos/todomvc_r660.py` (40 assertion, 6.89s)

【북극성 명확화 — Phase A finalisation (R664-R667) 의미】

§1 Vision (R663.5 정정) + §2 7 invariants 가 가리키는 4-phase progression:

(a) **R664 (✓ land) = todomvc edit-in-place + R663 paint-side 2nd consumer 청산** — R663 substrate consumer 등장. view_field 3rd consumer = R657 lift ROI 확정 시점. text-decoration strikethrough = 1st paint primitive consumer. Phase A 진척 70% → ~78%

(b) **R665 (✓ land) = External(opaque) persistence** — pinion-platform-storage 16번째 crate (FileStorage atomic write via tempfile + sync_all + rename), `Storage` trait + InMemoryStorage substrate, PersistedState 단일 blob schema, use_persistence_boot Effect-retention pattern. §3 capability boundary 정통 escape hatch 첫 실증 완료. Phase A 진척 ~78% → ~80%

(c) **R666 (✓ land) = AI driving 12+ step end-to-end + 3 substrate gap 청산** — scene/invoke v1 multi-External path syntax (rewind.rs canonical 을 invoke/intervene/dry_run 3 site 에 mirror, composite-tag vs ExtraExternal base tag 명확화) + Owner::cache nested-factory framework guard + scene/key character vs named auto-discriminator. todomvc_r666 demo = 12+ step / 3-cycle relaunch / 55 assertion. todomvc demos 의 R665-induced state pollution 청산 (rpc_verify::isolated_storage_dir helper + 7 demo retrofit). Phase A 진척 ~80% → ~85%

(d) **R667 = 2nd composed app (settings panel)** — view_vertical_scrollbar 4th consumer + view_field 4th consumer + Storage 2nd application consumer = substrate ROI curve fully positive 검증. **Phase A 종료** = 진짜 북극성의 ~7-8% 도달

(e) **R700+ = Phase B 진입** — Multi-window substrate 첫 라운드. winit 이미 multi-window 지원, `pinion-shell::WindowManager` + `Scene::Window` enum. DevTools / Inspector 가 첫 multi-window consumer. **이 시점이 진짜 framework 가 "professional tool 가능" 으로 도약**

(f) **R1000+ = Phase C 진입 = §2 #4 의 진짜 구현** — `ImmediateModeNode` primitive + game-loop infrastructure (60-144fps lockstep + delta time + frame budget cap) + retained↔immediate runtime switch per `Scene::Container` subtree. 동일 binary 안 settings panel = retained / 3D viewport = immediate

(g) **R2500+ = Phase D 진입** — Editor self-hosted in pinion itself. Unreal-class IDE 작성 시작. 진짜 northern-star 의 본격 진입

【다음 텍스트북 캐논 — R671 = R670.B carry inline 청산 (3개) + Phase B widget catalog 첫 진입 (TreeView)】

> **User directive (2026-05-25, R670.B 종료 시 재확인)**: 비용 무관 + 북극성 anchor + 부채 즉시 상환 + 다음 세션 `load` 자동 진행. R671 axis 는 northern-star anchor 기반으로 **R670.B carry 청산 + Phase B widget catalog 첫 진입 (TreeView)** 결합.
>
> **Northern-star 정통 정렬 이유**:
> - **TreeView** 는 Phase D editor self-hosted (~35% northern-star mass) 의 가장 직접적인 prerequisite (DevTools scene-tree inspector + property-grid + file-tree 전부 TreeView)
> - hello-multi-window 의 inspector single-Text mirror 가 자연스럽게 TreeView 로 upgrade — DevTools 의 첫 진짜 prototype
> - Phase B widget catalog (Qt QTreeView / Flutter ExpansionTile / macOS NSOutlineView) 의 가장 정통한 entry
> - R670.B carry 3개 (parity unify + per-window last_paint_layout + single-parse) 가 TreeView 의 substrate prerequisite 와 자연 align — 모두 inline 청산
>
> **R670.B carry inline 청산 (3개 mandatory)**:
> - **Carry #5** (`compute_paint_scene_*` parity) — substrate refactor: `compute_paint_scene_internal(window_id: Option<&str>, w, h)` single fn 으로 unify; `compute_paint_scene(w, h)` = `internal(None, w, h)`; `compute_paint_scene_for_window(id, w, h)` = `internal(Some(id), w, h)`. R670.B mid-refactor regression 패턴 영구 청산. [[r670b-paint-scene-producer-parity]] 의 'long-term unify' 부채 정통 청산.
> - **Carry #1** (per-window `last_paint_layout`) — substrate refactor: `WindowSlot.last_paint_layout: Option<LayoutNode>` field 추가; `render_window` 가 `finalize_frame` 후 slot.last_paint_layout update; `scene/layout {viewport: null}` 가 RPC dispatch 의 `window_id` 사용해서 해당 slot 의 last_paint_layout 반환. ShellCore 의 single last_paint_layout 은 primary slot 의 mirror 로 유지 (backward-compat). hello-multi-window 가 첫 consumer (`scene/layout {window: "inspector"}` 가 inspector 의 layout 반환).
> - **Carry #4** (`parse_rpc_window_id` single-parse) — substrate refactor: `pinion_rpc::dispatch` API 가 pre-parsed `serde_json::Value` 를 받는 variant 추가 (또는 dispatch 가 parse 결과를 반환하도록 wire 변경); `AppShell::dispatch_rpc` 가 single-parse 하고 둘 다 같은 Value 를 사용. 미세 perf + single-source-of-truth.

R671 atomic 5개 (substrate-first 순서, northern-star 정렬):

(0) **`ShellCore::compute_paint_scene_internal` unify** (R670.B carry #5 즉시 청산)
   - 신규 private `compute_paint_scene_internal(&mut self, window_id: Option<&str>, w: u32, h: u32) -> Scene` 메서드
   - body = R670.B 두 fn 의 공통 body + match 분기 (`window_id.map_or(V::view(state, &frame), |id| V::view_for_window(id, state, &frame))`)
   - `compute_paint_scene(w, h) = self.compute_paint_scene_internal(None, w, h)`
   - `compute_paint_scene_for_window(id, w, h) = self.compute_paint_scene_internal(Some(id), w, h)`
   - 결과: 미래 paint-pipeline 확장 (theme reactivity, hot-reload, post-paint cleanup) 은 internal fn single update; parity drift 영구 불가능
   - Estimated LOC: -80 (두 fn 의 ~80 LOC 중복 제거) + 100 (internal fn) = +20 net

(1) **`WindowSlot::last_paint_layout` per-window lift** (R670.B carry #1 즉시 청산)
   - `WindowSlot` 에 `last_paint_layout: Option<LayoutNode>` field 추가
   - `AppShell::render_window` 가 `self.core.finalize_frame(paint_scene)` 호출 후 `build_layout_node(&paint_scene, "/0")` 를 slot.last_paint_layout 에 저장
   - `ShellCore::dispatch_rpc_for_window` 가 `last_paint` 를 spec id 기반으로 lookup (caller 가 slot.last_paint_layout 을 borrow 로 전달); ShellCore 의 single last_paint_layout 은 primary slot 의 mirror 로 유지 (backward-compat for `dispatch_rpc(window_id=None)` 경로)
   - hello-multi-window 가 첫 consumer — demo verify: `scene/layout {window: "inspector"}` 가 inspector 의 layout (280×140 root) 반환, `scene/layout {window: "main"}` 가 main 의 layout (320×200 root) 반환, 둘 다 다름
   - Estimated LOC: WindowSlot field +20 / render_window snapshot lift +30 / dispatch_rpc wire +100 / hello-multi-window demo extension +50 = +200 net

(2) **`pinion_rpc::dispatch` single-parse refactor** (R670.B carry #4 즉시 청산)
   - `pinion_rpc::dispatch` 가 `request_json: &str` 대신 `request: &PreparsedRequest` 를 받는 variant 추가, OR `parse_request` 헬퍼 export 하고 AppShell 이 결과를 dispatch + parse_rpc_window_id 양쪽에 사용
   - `AppShell::dispatch_rpc` 가 `serde_json::from_str(request)` 한 번만 호출하고 결과를 dispatch + window_id 추출에 공유
   - Estimated LOC: pinion_rpc API surface +50 / AppShell 단일 parse +30 / parse_rpc_window_id deprecation -20 = +60 net

(3) **`pinion_widget_paint::tree_view` 신규 module** (Phase B widget catalog 첫 진입)
   - 새 module `crates/pinion-widget-paint/src/tree_view.rs`
   - `TreeViewStyle::m3_default()` (M3 Lists 카논: 48px row height, 16px indent step, 24px expand-glyph)
   - `TreeItem { id: String, label: String, depth: u32, expanded: bool, children: Vec<TreeItem> }` (Box<TreeItem> 으로 재귀 회피)
   - `view_tree(tag: &str, items: &[TreeItem], theme: &Theme, style: &TreeViewStyle) -> Scene` — depth-first flat row paint + ARIA tree/treeitem role + Material 3 state-layer overlays
   - Composite tag `{tag}#{node_id}` per row (R55.D.5 substrate consumer)
   - Keyboard navigation 은 substrate-incompleteness signal 이 등장하는 시점 (2nd consumer) 까지 carry (hello-multi-window 의 inspector 는 read-only)
   - 8 unit tests (`r671_tree_view_*`): empty list / single item / nested children / depth indent / expanded glyph / collapsed glyph / M3 row height / composite tag emission
   - Estimated LOC: tree_view.rs 모듈 +350 / tests +200 = +550 net

(4) **`hello-multi-window` inspector 업그레이드 — TreeView 첫 consumer + R671 demo + 11-demo sweep + commit + Mnemosyne**
   - hello-multi-window inspector window 의 single-Text mirror 를 TreeView 로 교체
   - main window 의 `scene/snapshot {window: "main", from: "paint"}` 결과를 TreeItem 트리로 변환 (`fn snapshot_to_tree_items(snapshot: &SnapshotNode) -> Vec<TreeItem>`)
   - 이 변환은 inspector window 가 Effect 또는 reactive 정기 (every paint cycle) 로 갱신 — 단순 polling: 매 inspector paint 마다 main 의 paint scene 을 walk 해서 트리 build
   - 신규 `tools/demos/r671_tree_view_inspector.py` (≥ 30 assertion): main paint scene 의 변화가 inspector tree 에 mirror, expand/collapse 상태, TreeItem 의 id 가 main paint scene 의 path 와 일치
   - 11-demo sweep PASS (R660 / R663 / R664 / R665 / R666 / R667 / R668 / R669 + R670.A carry clearance + R670.B hello-multi-window + R671)
   - Commit `feat(<scope>): R671 §5.16 §5.40 §5.45 TreeView + R670.B carry`
   - Mnemosyne `append_changelog_entry_v2 entry_id=R671` + impact_refs [5.16, 5.40, 5.45, 5.50] + carry_forward (R672 candidate axes — Menu / Dialog / Table / TreeView 2nd consumer)
   - Estimated LOC: hello-multi-window inspector view fn rewrite +200 / snapshot_to_tree_items +100 / demo +400 / SEED + Mnemosyne entry +200 = +900 net

**Honest total LOC 예측: R671 = +1700-2500 net** (carry refactors + TreeView substrate + first consumer rewrite + demo).

**R671 후 진척**: 북극성 가중 ~8.0% → **~8.7-9.0%** (Phase B widget catalog 첫 진입 = Phase B 25% × ~3% + Phase D prerequisite 시작 = Phase D 35% × ~0.5%). R672+ = **Phase B widget catalog cascade — Menu / Dialog / Toolbar / Table (R750+ 본격 진입)**.

**R671 verification mandatory** (라운드 끝):
- 11-demo regression sweep PASS bit-identical (R660 - R670.B 전부)
- `compute_paint_scene_internal` unify 후 `compute_paint_scene` + `compute_paint_scene_for_window` 의 행동 정확히 동일 (todomvc + hello-multi-window 둘 다 동일 paint scene 생성)
- `scene/layout {window: "inspector"}` ≠ `scene/layout {window: "main"}` (per-window last_paint_layout 작동 검증)
- inspector TreeView 가 main 의 paint scene 변화를 reflect (main click → main paint scene 변경 → inspector TreeView 의 트리 노드 expand/property 변경 → next inspector paint 가 변화 반영)
- 부채 surface 정직 받아들임 — TreeView keyboard navigation 은 substrate-incompleteness signal 등장 시까지 carry

---

이전 R670 (현재 land 완료):

R670.B 진척: R670.A 가중 ~7.5% → R670.B 후 **~7.7-8.0%** (Phase B 25% × first-consumer ~2%). R671+ = **Phase B widget catalog 본격 (Menu / Dialog / Toolbar / DevTools / TreeView / Table — R750+ 진입 단계)**.

R670.B atomic 3개 (R670.A trait foundation 위에서; substrate-first 순서):

(0) **`pinion-shell::AppShell` multi-window refactor** (Phase B substrate Round 2 — Approach A: single binding state, multiple views per window). R670.A 의 `WidgetView::windows() -> Vec<WindowSpec>` trait foundation 위에서.
   - `AppShell` field: `render: RenderState<V::Renderer>` → `renders: HashMap<WindowId, RenderState<V::Renderer>>` (winit `WindowId` keyed)
   - `accesskit: Option<accesskit_winit::Adapter>` → `accesskits: HashMap<WindowId, accesskit_winit::Adapter>` (per-window AT adapter)
   - `pending_intrinsic_resize: Option<((u32,u32),(u32,u32))>` → `HashMap<WindowId, ...>` (per-window first-paint resize queue)
   - `ime_was_composing: bool` + `last_ime_cursor_area: Option<...>` → per-window (per `Self::WindowSlot` struct grouping renders + accesskit + ime + intrinsic_resize)
   - 신규 `Self::WindowSlot { render, accesskit, ime_was_composing, last_ime_cursor_area, pending_intrinsic_resize, spec_id }` struct — per-window field cluster lifted out
   - `AppShell::windows: HashMap<WindowId, Self::WindowSlot>` 단일 hashmap (cluster lift 가 disjoint-field borrow 문제 회피)
   - `resumed(event_loop)` walks `V::windows()` list, creates one winit `Window` + RenderState + accesskit_winit::Adapter per spec, stores in `windows` hashmap keyed by WindowId; spec id → WindowId mapping cached on Self for RPC scope resolution
   - `window_event(window_id, event)` looks up `windows[&window_id]` and dispatches to per-window slot — paint cycle / IME / resize / focus 모두 per-window
   - `render()` becomes `render_window(&mut self, window_id: WindowId)` — splits the giant fn so per-window paint stays disjoint from per-window IME publish + per-window AT emit
   - 단일 `ShellCore` (binding state) — 모든 window 가 같은 state 의 different views; multi-binding (different ShellCores per window) 은 R750+ widget catalog 단계 (Approach B)
   - 기존 single-window 경로는 R670.A 의 default `V::windows()` 가 보장 (`vec![WindowSpec::main(...)]`) — 8-demo regression sweep PASS 검증 mandatory
   - Estimated LOC: AppShell hashmap refactor +400-600 / per-window paint + IME + accesskit wire +200-300 / `WindowSlot` lift +100 = +700-1000 net churn (~half is field-cluster lift, ~half is per-window dispatch)

(1) **RPC `{window: "<id>"}` param + `WidgetView::view_for_window` hook + `pinion-rpc::DispatchContext` per-window scope**
   - `pinion-rpc::DispatchContext` gains `window_id: Option<&'a str>` field + `with_window(id: &'a str)` builder — when present, the dispatcher's scene snapshot/click/key/wheel/invoke arms address the per-window scene; when absent, default to the primary (first `WindowSpec` in `V::windows()`)
   - `pinion-shell::AppShell::dispatch_rpc` reads `params.window` from the JSON-RPC frame + threads it into `with_window(...)` before calling `pinion_rpc::dispatch`; default "main" preserves single-window binding compat
   - 신규 `WidgetView::view_for_window(window_id: &str, state: V::State, frame: &Frame) -> Scene` trait method — default forwards to `Self::view(state, frame)` so single-window bindings unaffected; multi-window bindings override to return per-window scenes
   - `AppShell::render_window` calls `V::view_for_window(window_spec.id, cached_state, &frame)` instead of bare `V::view` so each window's paint scene reflects its own spec id
   - Estimated LOC: pinion-rpc window param +150-250 / pinion-shell dispatch_rpc wire +100-150 / WidgetView::view_for_window default impl +50 = +300-450 net

(2) **`hello-multi-window` first consumer binding + demo + 8-demo regression sweep + commit + Mnemosyne R670.B entry**
   - 신규 `examples/hello-multi-window` example binding — main window (Button widget) + inspector window (Text node displaying `format!("{:?}", state)` of main's ButtonState — read-only mirror)
   - `WidgetView::windows()` returns `vec![WindowSpec::main("Hello Multi-Window — Main", Fixed{320, 240}), WindowSpec::new("inspector", "Hello Multi-Window — Inspector", Fixed{280, 160})]`
   - `WidgetView::view_for_window` switches: `"main"` → Button view; `"inspector"` → state-debug text
   - 신규 `tools/demos/hello_multi_window_r670b.py` (≥ 30 assertion): scene/snapshot {window:"main"} ↔ scene/snapshot {window:"inspector"} 분리; scene/click {window:"main", at:...} 이 main button state 를 flip → inspector window 의 scene/snapshot 이 자동 mirror; 8-demo regression sweep PASS 검증 mandatory (R660 / R663 / R664 / R665 / R666 / R667 / R668 / R669 + R670.A carry clearance demo)
   - Commit `feat(<scope>): R670.B §5.16 §5.40 §5.12 Phase B multi-window first cut`
   - Mnemosyne `append_changelog_entry_v2 entry_id=R670.B` + impact_refs [5.16, 5.40, 5.12] + carry_forward (R671+ Phase B widget catalog axes)
   - Estimated LOC: hello-multi-window binding +400-700 / build.rs forge +50 / app.pinion.xml +20 / demo +400-600 / SEED + Mnemosyne entry +100 = +1000-1500 net

**Honest total LOC 예측: R670.B = +2000-3000 net** (substrate refactor + first consumer + demo). R670.A 실측 ~+1340의 ~1.5-2×.

**R670.B 후 진척**: R670.A 7.5% + Phase B 25% × first-consumer ~2% = **북극성 가중 ~7.7-8.0%**. R671+ = **Phase B widget catalog 본격 시작 (R750+ → Menu / Dialog / Toolbar / TreeView / Table / DevTools / Inspector first dogfood)**.

R670.A 의미 = **R668/R669 carry 100% 청산 + Phase B trait foundation 확보** (atomics 0+1+2 land). R670.B 가 actual multi-window dispatch + first application consumer 추가; R670 total = R670.A + R670.B 두 commit 으로 split.

R670.B 사전 watch out (R670.A 종료 시 surface 된 부채):
- `AppShell::resumed` 의 default-spec-only path 가 multi-window 진입 시 잠재 race — V::windows() 가 빈 Vec 반환하면 panic 잘 (assertion + 명시 error message 필수)
- per-window `pending_intrinsic_resize` 가 hashmap 으로 lift 되면 `IntrinsicAfterFirstPaint` 의 one-shot 보장이 per-window 단위로 유지되는지 검증 mandatory (한 window 이 intrinsic, 다른 window 이 fixed = mixed strategy 케이스)
- `accesskit_winit::Adapter` 가 per-window — AT client 가 main window 만 attach 하면 inspector window 의 a11y 트리는 emit 안 됨; 의도된 동작인지 vs 모든 window 가 한 트리에 합쳐져야 하는지 (AccessKit canonical: 1 adapter = 1 window, multi-window 는 multi-adapter — pinion 도 따름)
- `pinion-tui` 는 multi-window 미지원 영구 carry (terminal 1 process = 1 alternate-screen = 1 window 본질적 한계) — R670.B 도 GUI-only refactor; TUI binding 은 `windows()` default 만 사용

R667 의미 = **Phase A 종료 + Phase B (R700+) 진입 자격 획득**:

- Phase A 의 substrate 결정들 (R657 widget-paint lift / R659 composite_tag + scrollbar paint lift / R665 Storage) 모두 2nd application consumer 달성 = 정통 lift 정당화. Phase A 의 substrate 결정들이 textbook-canonical 임을 증명
- R666 의 framework primitive (scene/invoke v1 path + scene/key character disc + Owner::cache guard) 도 2nd application consumer 등장 = 정통 substrate maturity
- R666 carry #1 (resolve_external lift) 즉시 상환 = settings-panel 등장 전 substrate-first ordering ([[r47-class-incident-prevention]]) 정통
- WIN_H 480 magic 영구 carry 청산 = flex-grow primitive 첫 등장으로 Phase A 의 layout 부채 정통 청산

진척도 변화: ~6.75% → ~7.5-7.75% (진짜 northern-star 대비; Phase A 5%×85% → 5%×95-100%)

honest LOC 예측 + scope detail: 본 파일 【시작 명령】 절 참조.

【R667 Phase A 완성 cascade】

R665 (✓ land) — External(opaque) persistence
- pinion-core::storage + pinion-platform-storage 16번째 crate
- FileStorage atomic write (tempfile + sync_all + rename)
- PersistedState 단일 blob (todos + filter + next_id; editing_id 의도적 transient 제외)
- use_persistence_boot Effect-retention pattern + nested Owner::cache panic avoidance
- 46-assertion R665 demo (launch-kill-relaunch cycle)
- §3 Effect/External 정통 escape hatch 첫 실증 완료

R666 (✓ land) — AI-first §2 #2 첫 production stress + 3 substrate gap 청산
- scene/invoke v1 multi-External path syntax (R42 mirror — invoke/intervene/dry_run 3 site rewind.rs mirror, composite-tag vs ExtraExternal base tag 명확화)
- Owner::cache nested-factory framework guard (try_borrow_mut + actionable panic 메시지) + memory `[[owner-cache-no-nested-factory]]` 청산
- scene/key character vs named auto-discriminator (`chars().count() == 1` boundary) + `[[scene-key-character-named-gap]]` carry 청산
- 12+ step E2E demo (55 assertion, 3-cycle relaunch, scene/invoke v1 path × 5, scene/key character arc × every typed char)
- todomvc R655-R664 demos pollution 청산 (rpc_verify::isolated_storage_dir helper + 7 demo retrofit)
- 9-demo sequential regression PASS 9/9

R667 — 2nd composed app (settings panel) — Phase A 종료
- (0) pinion-rpc::resolve_external_mut substrate lift (R666 carry #1 즉시 상환; 6-of-6 → 7-of-7 시 lift 정통)
- (1) examples/settings-panel M3 Settings binding (nav rail + detail pane)
- (2) Storage 2nd application consumer (SettingsPersistedState + use_settings_persistence)
- (3) view_vertical_scrollbar 4th consumer + view_field 4th consumer
- (4) settings_panel_r667.py demo (≥40 assertion + persistence cycle)
- (5) LayoutStyle::flex_grow primitive (CSS-mirror; WIN_H 480 magic 영구 carry **청산**)
- R657/R659/R660/R665/R666 substrate ROI curve fully positive 확정
- Phase A 완료 = 진짜 northern-star ~7.5% 도달 + Phase B (R700+) 진입 자격
- Phase A 완료 = 진짜 northern-star ~7.5% 도달

【R700+ Phase B 진입 — 진짜 framework 도약】

R700 = Multi-window substrate (Phase B 의 첫 라운드):
- `pinion-shell::WindowManager` substrate (winit 이미 multi-window 지원)
- `pinion-core::Scene` 에 `Scene::Window {id, content}` enum variant (또는 Window 가 Scene root 위)
- AI introspection 확장: `scene/snapshot {window: "main"|"inspector"|"viewport"}`
- pinion-shell 의 `EventLoop` 가 multi-window dispatch
- DevTools window 가 첫 consumer

R750+ widget catalog 확장 (30+ widgets — Qt/Flutter parity):
- Menu / Dialog / Toolbar / Dock / TreeView / Table / RichText / Tabs / TooltipPopover / ContextMenu / Drawer / Accordion / DatePicker / ColorPicker / FileBrowser / ...

R900+ DevTools / Inspector (pinion 자체 작성 첫 dogfood):
- RPC introspection 이 substrate, 자체 dogfood UI

R1000+ Phase C 진입 = §2 #4 진짜 구현 — game-loop substrate:
- `pinion-core::scene::ImmediateModeNode` primitive 추가
- game-loop infrastructure (60-144fps lockstep + delta time + frame budget cap)
- per-`Scene::Container` subtree runtime switch (retained ↔ immediate)
- 동일 binary 안 settings panel = retained / 3D viewport = immediate

【각 라운드 의무 — R660-R663.5 lessons 통합】

1. **visible deliverable 의무**: 매 라운드 cargo run + demo script (process maturity 라운드 제외)
2. **RPC verify demo 의무**: ≥ 30 assertion (R660 baseline). R664 는 paint-side double-click + edit mode end-to-end 라 40+ 예상
3. **inline 부채 청산 mandatory**: 이전 라운드 honest 약점 → 다음 라운드 mandatory 인라인 청산. R663 honest 부채 5+1 = R664 atomic 청산. 외부 의존만 carry 정당
4. **doc compression baseline (R661 confirmed)**: target ≤ 1.5x base LOC. R661 baseline (3700 LOC) 이 미래 라운드 시작점. R664 후 todomvc 가 +1000 LOC 늘어도 압축된 density 유지
5. **substrate-first ordering ([[r47-class-incident-prevention]])**: industry-precedent input/paint primitive 는 framework crate 에 land. text-decoration strikethrough = R664 inline mandatory (paint primitive)
6. **N-consumer rule (Rule of Three)**: 2-of-2 / 3-of-3 / 5-of-5 (framework substrate 완전 성숙) 도달 시점 lift. R664 = view_field 3rd consumer = R657 lift ROI 확정
7. **first-consumer ROI evaluation — paint primitive 는 1st consumer 등장 시 lift** (R47-class). text-decoration strikethrough = R664 inline mandatory
8. **사용자 시연 가능 명시**: 매 commit message + 라운드 종료 보고
9. **부채는 양파**: R664 청산 중 새 부채 surface 정직 받아들임 (focus_set dynamic tag / Owner::cache dynamic key 등 substrate-incompleteness 가능성)
10. **AI-first verify ≥ 30 assertion 정량 기준**
11. **northern-star anchor — 매 라운드 axis 선택 기준 = "이 라운드가 AAA + editor self-hosted 에 얼마나 가까이 가는가"** (R663.5 정정 후 anchor)

【watch out — 영구 + 누적 (R665 후 갱신)】

기존 누적 + R666 land 청산:
- ✓ R660 청산: visible scrollbar peer + composite_tag 5-of-5
- ✓ R660 청산: filter kbd nav (Option β walk-back)
- ✓ R660 청산: filter / scrollbar M3 state-layers
- ✓ R660 청산: scene/drag substrate
- ✓ R661 청산: doc-heavy LOC overshoot baseline
- ✓ R662 청산: SCE-004 upstream debt registered + doc-anchor
- ✓ R662 청산: WidgetA11y::access_child_invoke parent_tag substrate
- ✓ R662 청산: todomvc filter AT-action wire
- ✓ R663 청산: scene/double_click framework primitive
- ✓ R663.5 청산: §1 Vision + §2 #4 + CLAUDE.md + memory 5-layer northern-star 정정
- ✓ R664 청산: Native paint-side double-click (InputRouter W3C 300ms/5px), focus_request mailbox, view_field 3rd consumer, TextDecoration::strikethrough 2nd consumer, access_child_invoke 4-of-4 application consumer
- ✓ R665 청산: §3 External(opaque) capability boundary 첫 실증 (pinion-platform-storage), Storage substrate (Clipboard 의 mirror), todomvc persistence cycle end-to-end
- ✓ R666 청산: scene/invoke v1 multi-External path syntax (R42 rewind.rs mirror — 3 site migration); Owner::cache nested-factory framework guard (try_borrow_mut + actionable panic); scene/key character vs named auto-discriminator (`[[scene-key-character-named-gap]]` 청산); todomvc R655-R664 demos pollution (rpc_verify::isolated_storage_dir + 7 demo retrofit); 12+ step E2E + 3-cycle relaunch demo
- ✓ R667 청산: pinion-rpc resolve_external substrate lift (5-site → single source); examples/settings-panel 2nd composed app; Storage 2nd application consumer; view_field 4th consumer; view_vertical_scrollbar 4th consumer (settings naturally short → R668 carry instead); WIN_H 480 magic via LayoutStyle::flex_grow primitive
- ✓ R668 청산: `pinion-shell::SizeStrategy` enum (Fixed / IntrinsicAfterFirstPaint) + `Scene::intrinsic_content_size` walker + winit `with_min_inner_size` floor + post-first-paint `request_inner_size` wire + 15 binding migration; `pinion-tui::ShellCoreTui::drain_deferred_inputs` substrate (mirrors pinion-shell pattern, all 6 DeferredInput variants — §2 #6 GUI/TUI dual invariant restored at substrate level); `pinion-widget-paint::checkbox::view_checkbox` lift (hello-checkbox 1st consumer; ~120 LOC → ~50 LOC); `pinion_core::text_scale::use_text_scale` + `TextStyle::with_size_px` thread-local multiplier (a11y substrate, clamp [0.1, 5.0] + NaN guard); settings-panel font_slider → use_text_scale wire (atomic 3 application consumer)
- ✓ R669 청산: `SettingsPersistedState` schema v1→v2 migrator (R665 schema migrator carry **first implementation** — textbook canonical for all future bumps); 6× `CheckboxExternal` composite-tag cluster `notifications#0..#5` (R55.D.5 substrate at N=6); `view_notifications_section` rewrite + `read_notification_states` + `read_notification_checked` Owner-scope-free walkers; 4th `ScrollBarExternal` consumer (notifications viewport overflow); `pinion-widget-paint::checkbox` 2nd application consumer (settings-panel notifications — Rule of Three 정통화)

R668 carry (R670.A 종료 시 평가):
- ✓ R669 청산: settings-panel Notifications 6-channel CheckboxExternal cluster (atomic 4 ladder); SettingsPersistedState schema v1→v2 migrator (R665 carry first impl); 4th ScrollBarExternal consumer
- ✓ R670.A 청산: `[[pinion-tui-rpc-ingress]]` 풀 wire — `ShellCoreTui::dispatch_rpc` + previews/revision/focus/last_paint_layout field lifts + spawn_stdin_rpc_reader_tui + pinion-tui::shell::run stdin drain with stderr response writer + 9 integration tests
- ✓ R670.A 청산: `SizeStrategy::IntrinsicAfterFirstPaint` first real application consumer — `examples/hello-popover` binding + `pinion-derive` `initial_size_strategy` macro flag + 5 unit tests + demo verify

R669 carry (R670.A 종료 시 평가):
- ✓ R670.A 청산: IntrinsicAfterFirstPaint application consumer (same item as R668 carry above)
- ❌ R670.B inline 청산 candidate: `read_composite_tag_value_slots` substrate helper — R669 의 read_notification_* 가 6 composite tags 순회; 2nd composite-tag-cluster consumer 등장 시 lift (현재 1-of-1, premature)
- 🔄 R670.B 진행 중 substrate: Phase B (R700+) multi-window — R670.A 의 `WindowSpec` + `WidgetView::windows()` trait foundation 위에서 AppShell multi-window refactor + RPC `{window: "<id>"}` param + `hello-multi-window` first consumer

R670.A carry (R670.B 종료 시 평가):
- ✓ R670.B 청산: AppShell multi-window refactor (WindowSlot cluster lift + resume_spec per-spec creation + window_event per-window dispatch + render_window split)
- ✓ R670.B 청산: RPC `{window: "<id>"}` param + DispatchContext.window_id + with_window builder + AppShell::dispatch_rpc parse_rpc_window_id wire
- ✓ R670.B 청산: hello-multi-window first consumer binding + 12-assertion demo + 6 unit tests
- ✓ R670.B 청산: WidgetView::view_for_window trait method (default forwards to Self::view)
- 영구 carry: `pinion-tui` multi-window 미지원 (terminal 1 process = 1 alternate-screen = 1 window 본질 한계)
- 영구 carry: `pinion-tui::shell::run` 의 stdin RPC 가 비-TTY (CI pipe) 환경에서 raw-mode-enable 실패 — substrate-level RPC ingress 는 9 integration tests 가 직접 cover; production smoke 는 TTY 환경에서만 verify
- 영구 carry: `SizeStrategy::IntrinsicAfterFirstPaint` 의 one-shot semantics (post-first-paint single resize; dynamic shrink-wrap-on-state-change 별도 axis)

R670.B carry (R671 → R672 → R673 청산 진행):
- ✓ R671 청산: `ShellCore::compute_paint_scene_internal(window_id: Option<&str>, w, h)` private fn unify; 두 변형이 thin wrapper. paint-pipeline parity drift regression class 영구 청산. [[r670b-paint-scene-producer-parity]] long-term unify 부채 청산.
- ✓ R671 청산: per-`WindowSlot::last_paint_layout` field; `scene/layout {window: "inspector"}` 가 280×140 (inspector 사이즈) ≠ `{window: "main"}` 가 320×200 (main 사이즈) 분리 검증.
- ✓ R671 청산: `pinion_rpc::parse_request` + `dispatch_parsed` single-parse refactor; `AppShell::dispatch_rpc` 가 envelope 한 번 parse + Request 공유.
- ✓ R672 청산: multi-window single-InputRouter race **구조적으로 closed** — `CoreShell.routers: HashMap<String, InputRouter>` + 11 _for_window CoreShell + 10 _for_window ShellCore + AppShell window_id 해결 + finalize_frame_for_window 별 paint scene 분리. [[multi-window-input-router-race]] memory 가 R672에서 CLOSED 상태로 mark됨. R670.B 데모의 scene/invoke 우회 retire (scene/click 다시 사용; 5/5 PASS deterministic).
- 영구 carry: Animation tick share across windows (one tick per ShellCore-frame) — 진짜 per-spec animation timing 요구 등장 시 별도 substrate axis (R672 cleared race가 아니라 timing 공유)
- 영구 carry: `pinion-tui` view_for_window 미지원 (terminal 1 process = 1 alternate-screen 본질 한계)

R671 carry (R673 청산 완료):
- ✓ R673 청산: TreeView keyboard navigation (Arrow Up/Down/Left/Right + Home/End + Space toggle, WAI-ARIA 1.2 §6.13 tree model) — `hello-tree-view` 2nd consumer가 lifted substrate carry.
- ✓ R673 청산: AriaRole Tree + TreeItem 변형 (WAI-ARIA 1.2 §5.3.10/§5.3.11) — `AccessNode::new(tag, AriaRole::Tree)` 가능.
- ✓ R673 청산: TreeView 2nd application consumer 도달 ([[abstraction-needs-second-consumer]] maturity gate).
- ✓ R673 청산: view_tree 레이아웃 정통 정정 (AlignItems::Stretch + 고정 너비 glyph column) — leaf NBSP ↔ branch ▶/▼ 컬럼 정렬.

R672 carry (R673 진입 전 평가):
- ✓ R673 청산: TreeView keyboard navigation (R673 atomic 2 hello-tree-view apply_key impl)
- ❌ 영구 carry (R675+): Phase B widget catalog cascade — Menu / Dialog / Tabs / Toolbar / Table / Tooltip / Drawer / Accordion / DatePicker / ColorPicker. 1-widget-per-round 또는 small-widget pair cascade.

R673 carry (R674 진입 전 평가):
- ❌ R674 atomic (0)+(1) 청산 mandatory: TreeView click-to-expand. 키보드 모델 (R673) 위에 click 추가 — `FileTreeRowExternal` SCXML state machine + composite_tag::parse_send_payload 6번째 substrate consumer + V::update reducer가 reactive batch에서 Signal mutation. 키보드 path와 click path 결과 bit-identical.
- ❌ R674 atomic (2) 청산 mandatory: per-row `AriaRole::TreeItem` AccessNodes + (필요 시) AccessNode에 `aria_level` / `aria_posinset` / `aria_setsize` 필드 추가 substrate extension.
- ❌ R675+ candidate: TreeView multi-select on rows (Ctrl/Shift+click + Shift+Arrow expand) — Phase D editor outliner (Unity Hierarchy / Unreal World Outliner / Blender Outliner) requirement.
- ❌ R675+ candidate: TreeView drag-drop reordering — file-tree editor / outliner.
- ❌ R750+ candidate: TreeView virtualization (LazyVStack N>1000) — Phase D scene-graph 전체 트리 표시 시 substrate-incompleteness signal.
- ❌ R675+ candidate: generic `TreeRowRouterExternal` (or `TagRouterExternal`) lift from binding-level FileTreeRowExternal — 2nd consumer (예: DevTools outliner) 등장 시 `pinion_widget_paint` 또는 `pinion_core::widgets` 로 promote.

영구 carry (외부 의존):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- SCE-002 (consumer-injectable derive list) — SCE-004 와 같은 axis
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- Figma API token

영구 carry 청산 ✓:
- ✓ R667 청산: WIN_H 480 magic (flex-grow primitive)
- ✓ R668 청산: WIN_W / WIN_H magic in 15 binding (SizeStrategy::Fixed 명시)
- ✓ R668 청산: §2 #6 TUI parity substrate (drain_deferred_inputs)
- ✓ R669 청산: Persistence schema breaking-change migrator (R665 carry first impl)

【R661-R666 lessons — 명시】

- **R663.5 정정의 가장 큰 lesson — Vision spec 명시가 모든 라운드 axis 선택의 anchor.** R660-R663 동안 "R667 settings panel = 북극성" misdirection 으로 substrate / process maturity / framework debt 만 진행. 진짜 northern-star (AAA + editor self-hosted) 가 spec / CLAUDE / memory / seed prompt 5-layer 어디에도 명시 안 됨. **axis 선택 시 매번 self-check: 이 라운드가 진짜 northern-star (4-phase 종착) 에 얼마나 가까이 가는가?**
- **Substrate-first ordering 정통** — R663 framework-first double-click → R664 substrate-consumer; R665 framework-first Storage trait (pinion-core) + FileStorage (sibling crate) → todomvc consumer; R666 framework-first Owner::cache guard + scene/key discriminator. [[r47-class-incident-prevention]] textbook
- **Mirror-substrate pattern** — R665 의 Storage / FileStorage 가 R56.1.e/R56.2.b 의 Clipboard / ArboardClipboard 구조 정확히 미러. R666 의 v1 path migration 이 rewind.rs 의 R42 패턴을 invoke/intervene/dry_run 3 site 에 미러. 비슷한 시스템 land 시 미러 시작점이 정통 reference
- **Mirror migration 정통** (R666 신규) — 동일 패턴 multi-site 적용 시 "한 site (rewind.rs) = canonical reference, 나머지 site (invoke/intervene/dry_run) = byte-level mirror" 가 가장 빠르고 안전. 새 helper 추출은 N≥6 까지 미루기 ([[abstraction-needs-second-consumer]] / Rule of Three)
- **Substrate gap 청산 시 application audit 의무** (R666 신규 lesson) — R666 #3 가 `[[scene-key-character-named-gap]]` 닫은 후 todomvc 의 letter-key V::keybinding intercept ('d'/'e' from R655 scaffolding copy-paste) 가 노출. copy-pasted scaffolding 이 substrate gap 뒤에 latent UX bug 숨김. **substrate gap 닫을 때 항상 application override audit**
- **Effect-retention substrate quirk (R665 신규)** — Owner::cleanup queue 가 Weak 만 보관 (R37.5 #2 leak fix). Application 이 Effect handle 영구 retain mandatory. 모든 production Effect 사이트 (PersistenceBootMarker 등) Rc 보관 필요. 2nd consumer 등장 시 framework lift candidate
- **Owner::cache nested factory panic (R665 → R666)** — R665 첫 실증, R666 framework guard land (try_borrow_mut + actionable panic). Pre-resolution pattern + memory `[[owner-cache-no-nested-factory]]` 정통화. cryptic panic 발견 시 framework-side guard upgrade 가 textbook (caller 디버깅 시간 ~100× 절감)
- **Doc compression baseline (R661) effective** — process maturity 라운드 분리 = substrate refactor 안전
- **SCE upstream debt 의 doc-anchor pattern** — R662 stop-gap 의 retire path 를 코드 doc 에 명시. 미래 Forge serde derive land 시 automatic retire
- **parent_tag substrate 의 multi-composite 일반화** — R662 access_child_invoke 확장이 R664 에서 4-of-4 application 도달; R667 settings panel sections 자동 활용
- **5-of-5 substrate maturity** — composite_tag mature substrate 는 sublinear 비용 증가. R664 의 TodoEditExternal = 6th consumer 무비용 추가. R666 의 v1 path resolver = 5-of-5 inline 패턴 (lift 보류)
- **demo storage isolation 의무** (R666 신규) — R665 land 후 R655-R664 demos 의 `$XDG_DATA_HOME/pinion-todomvc/` 오염 발견. R666 `isolated_storage_dir` helper + 7 demo retrofit 정통 청산. **persistence axis 등장 시 기존 demos 의 isolation pattern audit 의무**

【명시적 금지】

- README.md / docs.rs / user guide proactive 생성 금지
- Material Symbols / 외부 폰트 vendor commit 금지
- macro magic / 숨겨진 동작 channel 금지
- vendor/sce 직접 수정 금지 (SCE-004 등록 후 PR 경로만 정통)
- TodoMVC 외 다른 첫 composed app 변경 금지 (R667 까지)
- process round (0 LOC code change) 연속 2 이상 금지 — R663.5 vision 정정 1회 + 미래 doc compression 라운드 1회 까지만 정통
- visible deliverable 없는 라운드 금지 (process maturity / vision 정정 라운드는 예외)
- **R666 inline 청산 누락 금지** (R665 honest 부채 1-4 + Owner::cache nested 룰 docs)
- doc-heavy LOC 정당화 자동 허용 금지 — R661 baseline 유지
- pinion-widget-paint::toggle.rs / slider.rs 신규 모듈 추가 금지
- **R667 (Phase A 종료) 진입 전 R666 부채 청산 100% 완료 mandatory**
- Effect handle drop (production code; tests 의 `let _e = Effect::new(...)` 패턴은 OK) 금지 — R665 lesson, Owner::cleanup queue 가 Weak 만 보관
- **§1 vision 추가 권유 금지 / 에셋·물리·오디오 spec add 권유 금지 (Phase A 완료까지)** — `[[project_scope_game_engine]]` 단기 룰. Phase B-D 진입은 R667 (Phase A 종료) 이후
- Phase B/C/D 라운드 (R700+/R1000+/R2500+) 의 axis 를 R666-R667 안에서 시작 금지 — forward-compatible 설계 검토만
- Persistence schema 의 breaking-change 변경 시 PERSISTED_SCHEMA_VERSION bump 누락 금지 (silent migrator drift 회피)

【프롬프트 사용법】
새 세션 시작 시 이 파일 전체 입력 (또는 "@docs/SEED_PROMPT.md 읽고 진행"). 첫 7줄 (불변 운영 원칙) 매 세션 동일. "직전 5 세션 결과" + "다음 텍스트북 캐논" + "watch out" + "lessons" 매 세션 갱신.

【시작 명령】

R678 = **DevTools hover bridge + highlight overlay substrate LIFT (Rule-of-Three threshold reached)**. 4 atomic land — substrate-lift ordering. R676 view_main highlight wrap (selection) + R678 hover wrap = 2 consumers → [[abstraction-needs-second-consumer]] Rule-of-Three fires → new `pinion_widget_paint::devtools` module homes the lifted helpers (scene_to_tree_item / find_main_node_at_path / wrap_with_highlight / rebuild_with_highlight_at_path / 모든 path-stable indexing + walker family).

**R677 honest 부채 surface (R678 inline 청산 mandatory)**:
- `find_main_node_at_path` 가 R676+R677에서 2 consumer 도달 (view_main pre-resolve + property_pane resolution). R678 atomic (1) hover bridge가 3rd consumer 되며 Rule-of-Three threshold 명시적으로 fires.
- `wrap_with_highlight` + `rebuild_with_highlight_at_path` 가 R676에서 1st consumer (view_main 선택 wrap)만 가짐. R678 atomic (1) hover wrap이 2nd consumer 되며 함께 substrate lift candidate.
- inspector window 480×320 이 R677 atomic (2)에서 확정 — R678 영향 없음.

**진입 시 즉시 진행 (load 명령 / `R678 진행` 입력 시 자동 시작)**:
- 모든 atomic은 "비용 무관 + 장기 textbook canonical" 원칙 따라 작성 — MVP / shortcut 금지
- 각 substrate atomic 종료 시 cargo test + clippy + 16-demo regression sweep PASS 검증 후 다음 atomic 진입
- 마지막 atomic (3) 에서 demo + commit + Mnemosyne entry 한꺼번에
- 라운드 중간 commit 금지 (1 commit = 1 round 원칙) — WIP는 stash 또는 sequence keep
- session budget 80% 초과 시 honest stop + 그때까지 land한 atomic의 partial commit 가능 (R678.A) — but 우선 4 atomic 모두 한 라운드 land 시도
- 매 라운드 끝 push 권한 명시 동의 필요 (CLAUDE.md 영구 원칙)

**R678 atomic land 순서** (substrate-lift trigger, northern-star Phase D 정렬):

(0) **TreeRowClickExternal SCXML Hover state extension** — pinion_widget_paint::tree_view::TreeRowClickExternal:
   - 현재 SCXML: Idle / Pressed. R678 adds: Hovered state + `hovered_id: Option<String>` slot.
   - `PointerEnter(id)` → set hovered_id, transition Idle→Hovered or stay Pressed (Pressed wins over Hover when both present)
   - `PointerLeave(id)` (matching hovered_id) → clear hovered_id, transition Hovered→Idle
   - Hover은 click과 직교 — Pressed 동안 PointerEnter는 hovered_id 만 업데이트, state 머무름. PointerUp이 click 발사 (R676 동작), 그 후 Hovered/Idle 결정은 hovered_id 잔존 여부에 따라.
   - New const `TREE_ROW_HOVER_EVENT: &str = "hover"`; intent 발사 form `{tree_tag}.hover`
   - ExternalIntrospect schema: hovered_id (read-only query, no intervene) + hover (typed shortcut: invoke(/external/hover, Text(id)) for AI-driven hover synthesis)
   - 8-12 unit tests in pinion-widget-paint::tree_view::r678_tree_row_hover_external_tests
   - Estimated LOC: +200-300 substrate

(1) **Cross-window hover overlay via `use_hovered_path()` Owner::cache hook** — hello-multi-window:
   - `use_hovered_path() -> Rc<Signal<Option<String>>>` mirror of use_selected_path (Owner::cache "hello_multi_window_hovered_path")
   - New const `INSPECTOR_HOVER_INTENT_TAG = intent_tag!("inspector_tree", "hover")`
   - V::update reducer: arm matching INSPECTOR_HOVER_INTENT_TAG → use_hovered_path().set(payload)
   - view_main reads BOTH use_selected_path() AND use_hovered_path(). When hovered_path resolves to a node AND selected_path doesn't point at same node, wrap with hover style (M3 SurfaceContainerHighest Border, semitransparent). Both wrap composition: hover wrap inside selection wrap (selected element 위에 추가 hover overlay).
   - 6-10 unit tests (`r678_hover_overlay_tests` module)
   - Estimated LOC: +200-300 hello-multi-window

(2) **Highlight overlay substrate lift to `pinion_widget_paint::devtools`** — [[abstraction-needs-second-consumer]] Rule-of-Three fires:
   - 신규 module `pinion_widget_paint::devtools` (sibling to tree_view module)
   - Lifted helpers (from hello-multi-window): scene_to_tree_item, scene_root_path_segment, scene_child_path_segment, scene_type_name, scene_tag, find_main_node_at_path, parse_path_segment, find_child_in_container, PathDisambiguator, wrap_with_highlight, rebuild_with_highlight_at_path, descend_and_wrap
   - hello-multi-window 단순화: `use pinion_widget_paint::devtools::{scene_to_tree_item, find_main_node_at_path, wrap_with_highlight, ...};`
   - Tests migrate to substrate module (r676_* + r677_field_walker_tests 그대로)
   - 4 binding tests at hello-multi-window 유지 (integration-level proofs)
   - Estimated LOC: +250 substrate / -300 hello-multi-window = net -50

(3) **r678 demo + 17-demo sweep + commit + Mnemosyne** — `tools/demos/r678_devtools_hover_bridge.py` (≥30 assertion):
   - (A) baseline: no hover, no selection → no wraps anywhere in main scene
   - (B) AI hover via `tf.invoke("/inspector_tree/external/hover", "Container/Container[main_btn]")` → main paints hover wrap (M3 SurfaceContainerHighest Border)
   - (C) hover cleared (`invoke("/external/hover", Null)` or PointerLeave) → wrap disappears
   - (D) hover one node + select different node → both wraps visible, different colors (Error red selection vs SurfaceContainerHighest hover)
   - (E) hover + select same node → selection wins (only Error red Border visible)
   - (F) inspector-only id `state` → both signals soft-fail, no wraps
   - (G) PointerEnter/Leave send-wire bit-identical to typed hover shortcut
   - (H) substrate consumers reach through pinion_widget_paint::devtools re-export
   - 17-demo regression sweep PASS (R660-R677 + R678)
   - Commit `feat(widget-paint): R678 §5.16 §5.49 §5.50 hover bridge + DevTools substrate lift`
   - Mnemosyne `append_changelog_entry_v2 entry_id=R678` + impact_refs [5.16, 5.49, 5.50] + carry (R679 bidirectional select; R680 2nd DevTools binding consumer; R681 pinion-devtools crate skeleton)

visible (R678 land 후):
- `cargo run -p hello-multi-window` 변화 — inspector tree row hover → main window 해당 element에 transient hover border (M3 SurfaceContainerHighest, semitransparent). 클릭하면 Error red selection border로 변환. 다른 row hover 시 hover border 만 이동.
- `python3 tools/demos/r678_devtools_hover_bridge.py` (≥30 assertion, hover bridge + substrate lift)
- 기존 16-demo regression sweep PASS

honest LOC 예측: **R678 = +900-1500 net** — (0) SCXML Hover state +200-300 substrate / (1) hover bridge wiring +200-300 hello-multi-window / (2) substrate lift +250 substrate / -300 hello-multi-window (net -50 net) / (3) demo +400-500 + SEED + Mnemosyne entry +120-220.

**R678 verification mandatory** (라운드 끝):
- 17-demo regression sweep PASS deterministic (R660-R677 + R678)
- TreeRowClickExternal Hover state SCXML clean — Idle↔Hovered↔Pressed three-way transitions; PointerEnter/Leave during Pressed leaves state machine in Pressed (Hover orthogonal to click)
- highlight overlay substrate at pinion_widget_paint::devtools with both consumers (R676 selection wrap + R678 hover wrap) wired through
- soft-fail UX preserved — inspector-only ids / stale paths gracefully skip both selection AND hover wraps
- 부채 surface 정직 받아들임 — bidirectional select R679, 2nd DevTools binding R680, pinion-devtools crate skeleton (vs in-pinion_widget_paint::devtools module) R681

**R678 후 진척**: 북극성 가중 ~14-15% → **~15-17%** (Phase D 35% × ~4-5% — DevTools cascade reaches substrate maturity; highlight overlay LIFT triggers per Rule-of-Three; pinion_widget_paint::devtools module crystalizes). **R679+ = bidirectional select cascade → 2nd binding consumer → pinion-devtools crate skeleton**.

---

> 아래는 R670.B → R677 atomic 원본 (historical reference, land 완료):

R677 = **DevTools property pane (Computed-pane analog) + find_main_node_at_path 2nd consumer Rule-of-Three approach** (✓ land `4fbbe48`). 4 atomic land. 16-demo sweep PASS. view_inspector Row[tree, property_pane] split + property_pane_rows field walker + inspector 480×320 + 18 unit tests + r677 demo (38+ asserts).

R676 = **R675 architectural 부채 청산 (path-stable indexing) + DevTools highlight overlay 1st cut** (✓ land `cf3d134`). 4 atomic land. 15-demo sweep PASS. CSS-selector Type[tag-or-nth-of-type] scheme + find_main_node_at_path inverse walker + view_main_raw split + 2px Error-role wrap + 38 unit tests.

R675 = **TreeRowClickExternal substrate lift + DevTools first cross-window state-sync** (✓ land `e1d8817`). 4 atomic land. 15-demo sweep PASS. R675.1 SEED hash fixup `526a50a`.

R674 = **R673 carry inline 청산 (click-to-expand + per-row TreeItem AccessNodes) + AccessNode hierarchical axes substrate** (✓ land `f905488`). 4 atomic land. 14-demo sweep PASS. R674.1 SEED hash fixup `2930a14`.

R673 = **TreeView 2nd consumer (hello-tree-view) + R671 layout fix + AriaRole Tree/TreeItem + WAI-ARIA tree keyboard model** (✓ land `4a5a694`). 4 atomic. 13-demo sweep PASS.

R672 = **per-window InputRouter foundation; multi-window race **구조적으로 closed**** (✓ land `79684f4`). 5 atomic. 12-demo sweep PASS deterministic.

R671 = **R670.B carry inline 청산 (3개) + Phase B widget catalog 첫 진입 (TreeView)** (✓ land `6a7b955`). 5 atomic land (substrate-first, carry-clearance-first). 11-demo sweep PASS.

(0) `ShellCore::compute_paint_scene_internal` unify (R670.B carry #5 청산)
(1) `WindowSlot::last_paint_layout` per-window lift (R670.B carry #1 청산)
(2) `pinion_rpc::dispatch` single-parse refactor (R670.B carry #4 청산)
(3) `pinion_widget_paint::tree_view` substrate (Phase B widget catalog 첫 진입)
(4) hello-multi-window inspector TreeView 업그레이드 + demo + commit + Mnemosyne

---

> 아래는 R670.B atomic 원본 (historical reference, land 완료):

R670.B = **R670.A trait foundation 위에서 AppShell multi-window refactor + RPC window param + `hello-multi-window` first consumer** (✓ land `9c34251`). 3 atomic.

(0) **`pinion-shell::AppShell` multi-window refactor** — Approach A (single ShellCore + per-window winit Window / RenderState / accesskit_winit::Adapter / IME state / pending_intrinsic_resize). 신규 `Self::WindowSlot` struct group; `AppShell::windows: HashMap<WindowId, WindowSlot>` cluster lift; `resumed(event_loop)` walks `V::windows()` list; `window_event(window_id, event)` dispatches to per-window slot; `render()` → `render_window(window_id)` split.

(1) **RPC `{window: "<id>"}` param + `WidgetView::view_for_window` hook** — `pinion-rpc::DispatchContext::with_window` builder; `AppShell::dispatch_rpc` reads `params.window` + threads through; 신규 `WidgetView::view_for_window(window_id, state, frame) -> Scene` trait method (default forwards to `view`). Default "main" preserves single-window binding compat.

(2) **`hello-multi-window` first consumer binding + demo + commit + Mnemosyne** — `examples/hello-multi-window` main window (Button) + inspector window (state-debug Text). `tools/demos/hello_multi_window_r670b.py` (≥ 30 assertion: main scene/snapshot ↔ inspector scene/snapshot 분리, main click → inspector mirror). 9-demo regression sweep PASS (R660/R663/R664/R665/R666/R667/R668/R669 + R670.A). Commit + Mnemosyne `append_changelog_entry_v2 entry_id=R670.B`.

visible (R670.B land 후):
- `./target/release/hello-multi-window` 신규 — 2 windows (main + inspector); main state change → inspector window 자동 mirror via shared ShellCore
- `python3 tools/demos/hello_multi_window_r670b.py` (≥ 30 assertion, Phase B first multi-window dispatch 검증)
- 기존 9-demo regression sweep PASS (R660 / R663 / R664 / R665 / R666 / R667 / R668 / R669 + R670.A)

honest LOC 예측: **R670.B = +2000-3000 net** — (0) AppShell multi-window refactor +700-1000 / (1) RPC window param + view_for_window hook +300-450 / (2) hello-multi-window binding +400-700 + demo +400-600 + SEED + Mnemosyne +100. R670.A 실측 ~+1340의 ~1.5-2×.

**R670.B 진행 lessons audit 의무** (라운드 끝):
- multi-window AppShell refactor 가 기존 9-demo (R660-R669 + R670.A) 모두 회귀 0 (8-demo sweep PASS + R670.A carry clearance demo PASS)
- per-window `pending_intrinsic_resize` hashmap lift 가 IntrinsicAfterFirstPaint one-shot 보장 per-window 단위 유지 검증 (mixed strategy 케이스 — main: Fixed, inspector: IntrinsicAfterFirstPaint)
- `accesskit_winit::Adapter` per-window 가 AccessKit canonical 1-adapter-per-window 룰 준수 (multi-window = multi-adapter, 자동 합쳐지지 않음 — 의도된 동작)
- Phase B first consumer (hello-multi-window) 가 inspector window 에서 main state 를 read-only mirror — 진짜 multi-window pattern 실증
- 부채 surface 정직 받아들임 — multi-window refactor 도중 등장한 새 substrate gap 은 R671 carry
- `pinion-tui` multi-window 미지원 영구 carry (terminal 1 process = 1 alternate-screen 본질 한계)

**R670.B 후 진척**: R670.A 7.5% + Phase B 25% × first-consumer ~2% = 북극성 가중 **~7.7-8.0%**. **R671+ = Phase B widget catalog 본격 (Menu / Dialog / Toolbar / DevTools / TreeView / Table 등 R750+ 진입)**.

---

> 아래는 R667 atomic 원본 (historical reference, land 완료):

R667 = **2nd composed app (settings panel) = Phase A 표면 종료 라운드** (✓ land). 6개 atomic land (정확한 순서 — substrate-first):

(0) **`pinion-rpc::resolve_external_mut` substrate lift** (R666 carry #1 즉시 상환) — 현재 invoke/intervene/dry_run/query/rewind/simulate 6 file 에 8+ inline duplication (split_at_external + lookup_path_mut + primary_external_mut). 신규 helper:
   ```rust
   pub enum ResolveExternalError {
       Path(PathError), UnsupportedPath, NoExternalAtPath, IntrospectionOptedOut
   }
   pub fn resolve_external_mut<'s>(scene: &'s mut Scene, raw_path: &str)
       -> Result<(&'s mut ExternalNode, String), ResolveExternalError>;
   pub fn resolve_external<'s>(scene: &'s Scene, raw_path: &str)
       -> Result<(&'s ExternalNode, String), ResolveExternalError>;
   ```
   각 site error enum 의 `From<ResolveExternalError>` 변환 + `?` operator. ~-200 LOC net (inline 제거 > helper LOC). Settings-panel 의 새 RPC consumer 가 7th-of-7 site 로 자연 합류. Rule of Three + [[r47-class-incident-prevention]] 정통.

(1) **`examples/settings-panel` 신규 binding** — Phase A 의 2nd composed app. canonical reference:
   - **M3 Settings pattern + 좌측 nav rail (M3 NavigationRail) + 우측 detail pane**
   - macOS System Settings 와 navigation 패턴 동일 (single-level nav rail + section detail; 중첩 nav 금지)
   - 5+ sections: theme (light/dark toggle) + appearance (font scale slider) + profile (display name TextField) + notifications (checkbox group) + actions (Apply + Cancel buttons)
   - 모든 변경 즉시 Storage 영구화 (Apply 버튼은 UX-only; 데이터는 즉시 commit)

(2) **Storage 2nd application consumer** — `SettingsPersistedState { theme: ThemeMode, nav_index: u32, font_scale: f32, display_name: String, notifications: NotificationPrefs }` 단일 blob. `use_settings_persistence` Effect-retention pattern (R665 use_persistence_boot mirror). R665 substrate ROI 정통 정당화 + PersistenceBootMarker 2nd consumer = `framework::OwnedEffect` lift 결정점.

(3) **view_vertical_scrollbar 4th consumer + view_field 4th consumer** — detail pane viewport 넘으면 scroll (4th scrollbar consumer); display name 입력 (4th TextField consumer). R657/R659 substrate ROI curve fully positive 확정.

(4) **`tools/demos/settings_panel_r667.py`** — nav cycle (ArrowDown/Up + Enter activate) + 각 section mutate (theme toggle, slider drag, textfield type, checkbox toggle, button click) + persistence cycle (kill → relaunch → assert restored), ≥ 40 assertion. scene/invoke v1 path + scene/key character arc (R666 substrate) 2nd application 활용.

(5) **`LayoutStyle::flex_grow` primitive** (CSS-mirror) — settings panel detail pane 가 window 높이 가득 채워야 함. Container layout 분배 알고리즘에 flex_grow weight 추가. todomvc 가 2nd consumer 로 동시 migrate, WIN_H 480 magic 청산 (영구 carry 첫 청산).

visible:
- `cargo run -p settings-panel` 신규 가시 결과 — M3 nav rail + detail pane; 모든 입력 영구화; exit + relaunch 시 복원
- `python3 tools/demos/settings_panel_r667.py` ≥ 40 assertion
- `cargo run -p todomvc` 변화 없음 (flex-grow migrate = visual identity 보존)

honest LOC 예측: ~+1700-2700 net
- (0) resolve_external lift: -200 (inline 제거) + helper +100 = -100 net
- (1) settings-panel binding: +1500-1800
- (2) Storage 2nd consumer + persistence: +200-300
- (3) substrate consumer wiring: +100-200
- (4) settings_panel_r667.py: +500-700
- (5) flex-grow primitive + todomvc migrate: +200-400

실측 후 honest 정정 의무 (R666 estimate 500-800 vs 실측 ~1391 net — composed-app + demo + docs density 2× over).

**R667 = Phase A 종료** — 진척 ~7.5% 진짜 northern-star 대비. Phase B (R700+ multi-window — `pinion-shell::WindowManager` + `Scene::Window` variant + DevTools 첫 multi-window consumer) 진입 권리 획득.

R666 carry honest 평가 (R667 atomic 진입 후):
- (0) carry #1 (resolve_external lift) → **R667 atomic (0) 즉시 상환** ✓ (Rule of Three + 6 site duplication = textbook lift 정통, 더 이상 premature 아님)
- carry #2 (DeferredInput kind override) — YAGNI 정통 carry
- carry #3 (PersistenceBootMarker 2nd consumer) — R667 atomic (2) 가 자연 2nd consumer → **R667 진행 중 lift 결정점**
- carry #4 (next_id Cell lift) — settings-panel 의 2nd writer 등장 안 함 → 정통 carry
- carry #5 (schema migrator) — breaking change 미발생 → 정통 carry

영구 carry 청산 후보 (R667 inline):
- WIN_H 480 magic → R667 atomic (5) flex-grow primitive 로 청산 ✓
