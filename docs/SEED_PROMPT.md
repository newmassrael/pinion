# pinion seed prompt — 매 세션 첫 입력

> R666 (2026-05-25) 갱신. R663.5 canonical baseline 유지 + R664-R666 land 반영. R664+ 각 라운드 종료 시 "직전 세션 결과" + "다음 텍스트북 캐논" + "watch out" + "lessons" 절 갱신.

---

【불변 운영 원칙】 (첫 7줄 — 매 세션 동일)
- 비용 무시. 항상 장기적으로 올바른 textbook-canonical 선택
- **진짜 북극성 = AAA game shippable + Unreal-class editor self-hosted in pinion itself, AI-introspection 1st-class through every phase.** 4-phase progression: A. Foundation (현재 ~80%, R655-R667 todomvc+settings panel) → B. Professional GUI (Qt/Flutter/Compose-class + multi-window + DCC widget catalog, R700+) → C. Game engine substrate (§2 #4 immediate-mode game loop ↔ retained widget tree dual execution + 3D + assets + physics + audio + PBR, R1000+) → D. AAA game maker (editor self-hosted in pinion + visual scripting + Nanite/Lumen-class rendering + multiplayer netcode, R2500+). 현재 가중 진척 ~7%. R666-R667 cascade 후 ~8%. R667 = Phase A 종료 = 진짜 북극성의 ~5%, **NOT 북극성 도달**
- 부채 즉시 상환. 라운드 안 발견 부채 inline 청산, carry 영원 누적 금지. 이전 라운드 honest 약점 → 다음 라운드 inline 청산 mandatory. 외부 의존 (vendor/sce upstream, 환경) 만 honest carry 정당
- 라운드 자동 선택. 세션 80% 까지 계속
- "부채는 양파다" — 청산 시 새 부채 surface 정직 받아들임
- 1 commit = 1 round = 1 atomic Mnemosyne entry
- 사용자 명시 동의 없으면 git push 금지 (CLAUDE.md 영구 원칙). "진행" / "continue" / "go" 는 push 권한 아님

【진입 시 필독 순서】
1. `docs/SEED_PROMPT.md` (이 파일 — R666+ matters 의 baseline)
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

land 완료 (6 commits, daf2a99 → 2d262ad → d8e6810 → bde04f7 → 501f304 → bf23117 + R666 신규):

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

- **R666** (AI-first §2 #2 첫 production stress — Phase A ~80%→~85%) 신규:
  - pinion-rpc::invoke / intervene / dry_run R42 mirror migration (rewind.rs canonical) — v1 path `/{tag}/external/{action}` 으로 모든 ExtraExternal singleton 을 base tag 로 addressing. composite-tag (`{tag}#{id}`) 는 paint-side router artefact 임을 명문화. +12 R666 tests (composite-tag DFS, window prefix, unknown segment, non-External target)
  - pinion-core::reactive::Owner::cache nested-factory guard — `try_borrow_mut` 가 cryptic `BorrowMutError` → actionable panic message ("Owner::cache factory closures must not call Owner::cache; pre-resolve dependent slots first") 업그레이드. R665 의 use_persistence_boot 첫 실증 청산 + framework-side guard land. +3 R666 tests (panic 메시지 검증, pre-resolved path 정통, distinct-Owner nesting 허용)
  - pinion-rpc::DeferredInput::CharacterKey 신규 variant + handle_scene_key 가 `key.chars().count() == 1` 자동 판별 → CharacterKey (handle_character_key → V::keybinding intercept); 그 외 → Key (handle_named_key). pinion-shell drain CharacterKey arm. 사전 R666 carry `[[scene-key-character-named-gap]]` 청산. +4 R666 tests (single ascii, U+0020 space, 사전조립 CJK 음절, multi-char W3C named)
  - examples/todomvc — 상속된 `'d'`/`'e'` letter-key V::keybinding intercept 청산 (R655 scaffolding copy-paste from hello-textfield; R666 #3 가 gap 닫은 후 'eggs'/'delete' 타이핑이 깨졌던 latent bug 표면화 → 정통 청산)
  - tools/demos/todomvc_r666.py — 12+ step E2E (cycle 1 boot+type+toggle+filter+edit+commit; cycle 2 relaunch+verify+add+toggle-off+delete; cycle 3 두번째 relaunch+verify 모두 persist). 55 assertion, scene/invoke v1 path 5회 사용, scene/key character arc 모든 typed char 에 사용
  - tools/rpc_verify.py — `isolated_storage_dir(prefix)` context manager helper + `tf.text(body, path)` typing convenience. R666 inline retrofit: todomvc_r655/r656/r658/r659/r660/double_click_r663/r664 모두 `isolated_storage_dir` 으로 wrap → 순차 실행 시 `$XDG_DATA_HOME/pinion-todomvc/` 오염 없음. R665 carry 청산
  - 3369 workspace tests + clippy clean
  - **9-demo sequential regression sweep PASS** (todomvc_r655→r666 + double_click_r663; 두번째 run 도 PASS — per-demo tempdir 격리 검증)

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

진척도 (R666 후) — **진짜 northern-star 대비**:

| Phase | 비중 | 현재 |
|---|---:|---:|
| A. Foundation (§1-§4 + 첫 composed apps) | 5% | ~85% |
| B. Professional GUI (Qt/Flutter/Compose-class + multi-window) | 25% | 10% |
| C. Game engine substrate (§2 #4 dual execution + 3D + ...) | 35% | 0% |
| D. AAA editor self-hosted | 35% | 0% |

**가중 진척 = 5%×85% + 25%×10% + 35%×0% + 35%×0% = ~6.75%**

R667 cascade 후 Phase A 95-100% → ~7.5%. Phase B 진입 (R700+) 이 진짜 northern-star 의 +5% 가속. Phase C/D 가 진짜 mass (35%+35% = 70% of work).

가시 결과:
- `./target/release/todomvc` — Tab/Arrow/Home/End/Space filter cycle + M3 hover/press layers + scrollbar drag + 더블클릭 inline 편집 + Enter/Esc commit/cancel + strikethrough on completed + exit + relaunch 시 state 영구 복원 (R665) + AI 가 scene/invoke v1 path 로 모든 ExtraExternal singleton 을 직접 조작 가능 (R666)
- `python3 tools/demos/todomvc_r666.py` (55 assertion, 7.10s — 12+ step E2E + scene/invoke v1 path × 5 + scene/key character arc + 3-cycle launch-kill-relaunch)
- `python3 tools/demos/todomvc_r665.py` (46 assertion, 13.47s — launch-kill-relaunch persistence cycle)
- `python3 tools/demos/todomvc_r664.py` (34 assertion, 5.83s)
- `python3 tools/demos/todomvc_r660.py` (40 assertion, 6.89s)
- 9-demo sequential regression sweep: PASS 9/9 from clean baseline + PASS 9/9 second run (per-demo tempdir isolation via R666 `isolated_storage_dir` helper)

【북극성 명확화 — Phase A finalisation (R664-R667) 의미】

§1 Vision (R663.5 정정) + §2 7 invariants 가 가리키는 4-phase progression:

(a) **R664 (✓ land) = todomvc edit-in-place + R663 paint-side 2nd consumer 청산** — R663 substrate consumer 등장. view_field 3rd consumer = R657 lift ROI 확정 시점. text-decoration strikethrough = 1st paint primitive consumer. Phase A 진척 70% → ~78%

(b) **R665 (✓ land) = External(opaque) persistence** — pinion-platform-storage 16번째 crate (FileStorage atomic write via tempfile + sync_all + rename), `Storage` trait + InMemoryStorage substrate, PersistedState 단일 blob schema, use_persistence_boot Effect-retention pattern. §3 capability boundary 정통 escape hatch 첫 실증 완료. Phase A 진척 ~78% → ~80%

(c) **R666 (✓ land) = AI driving 12+ step end-to-end + 3 substrate gap 청산** — scene/invoke v1 multi-External path syntax (rewind.rs canonical 을 invoke/intervene/dry_run 3 site 에 mirror, composite-tag vs ExtraExternal base tag 명확화) + Owner::cache nested-factory framework guard + scene/key character vs named auto-discriminator. todomvc_r666 demo = 12+ step / 3-cycle relaunch / 55 assertion. todomvc demos 의 R665-induced state pollution 청산 (rpc_verify::isolated_storage_dir helper + 7 demo retrofit). Phase A 진척 ~80% → ~85%

(d) **R667 = 2nd composed app (settings panel)** — view_vertical_scrollbar 4th consumer + view_field 4th consumer + Storage 2nd application consumer = substrate ROI curve fully positive 검증. **Phase A 종료** = 진짜 북극성의 ~7-8% 도달

(e) **R700+ = Phase B 진입** — Multi-window substrate 첫 라운드. winit 이미 multi-window 지원, `pinion-shell::WindowManager` + `Scene::Window` enum. DevTools / Inspector 가 첫 multi-window consumer. **이 시점이 진짜 framework 가 "professional tool 가능" 으로 도약**

(f) **R1000+ = Phase C 진입 = §2 #4 의 진짜 구현** — `ImmediateModeNode` primitive + game-loop infrastructure (60-144fps lockstep + delta time + frame budget cap) + retained↔immediate runtime switch per `Scene::Container` subtree. 동일 binary 안 settings panel = retained / 3D viewport = immediate

(g) **R2500+ = Phase D 진입** — Editor self-hosted in pinion itself. Unreal-class IDE 작성 시작. 진짜 northern-star 의 본격 진입

【다음 텍스트북 캐논 — R667 atomic 청산 (2nd composed app = settings panel; Phase A 종료)】

R667 = **settings panel = 2nd composed application** = Phase A 종료 라운드

핵심 청산 항목:

(1) **examples/settings-panel 신규 binding** — Phase A 의 2nd composed app. 최소 형상:
- 좌측 nav rail (List + selection state) + 우측 detail pane
- detail 안 multiple sections: theme toggle + slider (font scale) + textfield (display name) + checkbox group (notification prefs) + button (apply / cancel)
- 모든 변경 R665 Storage 로 persistence 영구화 — Storage 2nd application consumer = R665 substrate ROI 확정
- view_vertical_scrollbar 4th consumer (detail pane 가 viewport 넘으면 scroll)
- view_field 4th consumer (display name textfield)

(2) **flex-grow primitive 첫 등장** — WIN_H 480 magic 영구 carry 청산 candidate. settings panel 의 detail pane 가 window 높이 가득 채워야 하는 첫 비-todomvc 케이스. `LayoutStyle::flex_grow` (CSS-mirror) 신규 + Container 분배 계산. todomvc 가 2nd consumer 로 migrate

(3) **substrate ROI curve 확정** — R657 (widget-paint lift) / R659 (composite_tag + scrollbar paint lift) / R665 (Storage) 모두 2nd application consumer 달성 = 정통 lift 정당화. Phase A 의 substrate 결정들이 textbook-canonical 임을 증명

(4) **R666 carry 평가 시점** — 신규 path resolver 사용처가 settings panel 에서 등장 시 `pinion-rpc::resolve_external` helper lift 결정 (현재 5-of-5 inline 패턴 → 6-of-6 시 lift 또는 5-of-5 sublinear 비용 시 carry)

(5) **PersistenceBootMarker 2nd consumer** — settings panel 도 PersistedState (persisted theme + nav selection 등) → use_settings_persistence 두 번째 Effect-retain 응용 등장 시 `framework::OwnedEffect` lift candidate

R667 visible deliverable:
- `cargo run -p settings-panel` — nav rail 클릭 시 detail 전환 + 모든 입력 영구화 (exit + relaunch 시 복원)
- `python3 tools/demos/settings_panel_r667.py` — nav cycle + detail mutate + persistence cycle, ≥ 40 assertion

honest LOC 예측: ~+1500-2500 LOC net (composed-app heavy)
- (1) settings-panel binding: +1200-1800 LOC (todomvc baseline 의 절반 + view-fn 복제)
- (2) flex-grow primitive: +200-400 LOC (pinion-core::layout)
- (3) settings_panel_r667.py: +400-600 LOC

실측 후 honest 정정 의무. **R667 = Phase A 종료 라운드** — 진짜 northern-star 의 ~7-8% 도달, Phase B (R700+ multi-window) 진입 권리 획득

진척도 +0.75-1%p 예상 (~6.75% → ~7.5-7.75% 진짜 northern-star 대비)

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
- view_vertical_scrollbar 4th consumer
- view_field 4th consumer
- Storage 2nd application consumer (R665 substrate ROI 확정)
- flex-grow primitive (LayoutStyle::flex_grow) — WIN_H 480 magic 청산 candidate
- R657/R659/R660/R665 substrate ROI curve fully positive
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

R666 carry (R667 진입 전 평가 / 미래 inline 청산 candidate):
- `pinion-rpc::resolve_external` helper lift — 5 site inline 패턴 (invoke/intervene/dry_run/query/rewind), 6번째 consumer 등장 시 lift (Rule of Three; settings panel 의 첫 path consumer 가 결정점)
- DeferredInput::CharacterKey explicit-kind override — 자동 판별 cover 미부족 사례 등장 시 `kind` param 추가 (YAGNI carry)
- PersistenceBootMarker 2nd consumer (settings panel persistence 가 2nd consumer 후보 — Effect-retention helper lift candidate)
- next_id Cell non-reactive 의존성 — 2nd writer 등장 전 premature lift

영구 carry (외부 의존):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- SCE-002 (consumer-injectable derive list) — SCE-004 와 같은 axis
- WIN_H 480 magic (flex-grow primitive — R667 settings panel 첫 consumer 후보)
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- Figma API token
- TUI parity (§6 #6 cascade) — pinion-tui 가 DeferredInput drain 미사용; RPC drain 등장 시 R666 패턴 자동 상속
- Persistence schema breaking-change migrator 부재 (현재 fall-through; 첫 breaking 변경 axis 등장 시 lift)

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

R667 = **2nd composed app (settings panel) = Phase A 종료 라운드**. 5개 atomic land:

(1) `examples/settings-panel` 신규 binding — 좌측 nav rail (List + selection) + 우측 detail pane (multiple sections: theme + slider + textfield + checkbox group + button). flex-grow 첫 consumer. WIN_H 480 magic 청산 candidate
(2) Storage 2nd application consumer — settings persistence (theme + nav selection + section state). R665 substrate ROI 정통 정당화. PersistenceBootMarker 2nd consumer 등장 시 `framework::OwnedEffect` lift 결정점
(3) view_vertical_scrollbar 4th consumer + view_field 4th consumer — R657/R659 substrate ROI curve fully positive 검증
(4) `tools/demos/settings_panel_r667.py` — nav cycle + detail mutate + persistence cycle, ≥ 40 assertion. scene/invoke v1 path (R666 substrate) + scene/key character arc (R666 substrate) 두 번째 application 활용
(5) flex-grow primitive (`LayoutStyle::flex_grow` CSS-mirror) — settings panel detail pane fill axis. todomvc 가 2nd consumer 로 migrate, WIN_H 480 magic 청산

visible:
- `cargo run -p settings-panel` 신규 가시 결과 (nav rail 클릭 → detail 전환 + 모든 입력 영구화 + exit + relaunch 시 복원)
- `python3 tools/demos/settings_panel_r667.py` ≥ 40 assertion

honest LOC 예측: ~+1500-2500 net (composed-app heavy). 실측 후 honest 정정 의무.

**R667 = Phase A 종료** — 진척 ~7.5% 진짜 northern-star 대비. Phase B (R700+ multi-window) 진입 권리 획득.

R666 carry 1-5 (`pinion-rpc::resolve_external` / DeferredInput kind override / PersistenceBootMarker 2nd consumer / next_id lift / migrator) 는 모두 premature 또는 외부 의존 — R667 진입 자연 trigger, R666 inline 추가 작업 없음.
