# pinion seed prompt — 매 세션 첫 입력

> R665 (2026-05-25) 갱신. R663.5 canonical baseline 유지 + R665 land 반영. R664+ 각 라운드 종료 시 "직전 세션 결과" + "다음 텍스트북 캐논" + "watch out" + "lessons" 절 갱신.

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

land 완료 (5 commits, daf2a99 → 2d262ad → d8e6810 → bde04f7 → 501f304 + R665 신규):

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

- **R665** (External(opaque) persistence 첫 실증 — Phase A 70%→~80%) 신규:
  - pinion-core::storage 신규 (Storage trait + InMemoryStorage; bytes-only + total surface) — Clipboard 의 mirror substrate
  - pinion-platform-storage 16번째 워크스페이스 crate (FileStorage + open_app_storage; atomic write via tempfile + sync_all + rename; 200-char key sanitization; dirs::data_dir 으로 XDG / Apple / Windows known-folder 해결)
  - examples/todomvc PersistedState 단일 blob schema (todos + filter + next_id; editing_id 의도적 transient 제외) + use_storage + use_persistence_boot (hydrate → batch seed → Effect 설치)
  - PINION_STORAGE_DIR env override (테스트 isolation)
  - 46-assertion R665 demo (정통 launch-kill-relaunch 사이클; schema mismatch + corrupted bytes 복구; filter cycles 영구화)
  - 3352 workspace tests + clippy clean
  - **R663-R664 honest 부채 6개 청산 ✓** (R664 inline mandatory list 전부 처리)

honest 평가 누적 — R665 inline 청산 = 0개 (substrate 첫 land 만, 후속 부채 없음).

R665 honest 부채 (R666 inline 청산 candidate):

1. **scene/invoke v0 primary-External only** — multi-External path syntax (R690+) carry. R666 의 12+ step E2E 가 composite path 도달 시 필연 inline (R690+ R666 으로 앞당김 후보)
2. **schema migration breaking-change 경로 부재** — 현재 schema_version mismatch = 무조건 fall-through. additive 변경은 `#[serde(default)]` 으로 cover, 첫 breaking 변경 시 proper migrator 등장. YAGNI honest carry
3. **next_id Cell non-reactive 의존성** — `allocate_todo_id` 가 `todos.set_with` 직전에 Cell mutation 하므로 Effect 가 fresh next_id 를 본다. 미래에 next_id 독립 mutation 시 영구화 drift. 2nd writer 등장 시 Signal<u64> 로 lift (defensive)
4. **PersistenceBootMarker Effect-retention substrate quirk** — Owner::cleanup queue 가 Weak 만 보관 (R37.5 #2 leak fix). Application 이 Effect handle 영구 retain mandatory. 2nd consumer 등장 시 `framework::OwnedEffect` helper 로 lift candidate (Rule of Three)

framework substrate completeness 부채 (R665 검증):
- **Owner::cache nested factory 금지 룰 명문화** — R665 가 첫 실증 (use_persistence_boot 가 use_storage / use_todos 호출 시 nested borrow_mut panic). pre-resolution pattern 으로 우회. 새 memory `[[owner-cache-no-nested-factory]]` 추가 candidate

carry honest (외부 의존, R666 미청산):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- TUI parity (§6 #6 cascade)
- Figma API token (영구)
- WIN_H 480 magic (flex-grow primitive 누락)

진척도 (R665 후) — **진짜 northern-star 대비**:

| Phase | 비중 | 현재 |
|---|---:|---:|
| A. Foundation (§1-§4 + 첫 composed apps) | 5% | ~80% |
| B. Professional GUI (Qt/Flutter/Compose-class + multi-window) | 25% | 10% |
| C. Game engine substrate (§2 #4 dual execution + 3D + ...) | 35% | 0% |
| D. AAA editor self-hosted | 35% | 0% |

**가중 진척 = 5%×80% + 25%×10% + 35%×0% + 35%×0% = ~7%**

R666-R667 cascade 후 Phase A 90-95% → ~7-8%. Phase B 진입 (R700+) 이 진짜 northern-star 의 +5% 가속. Phase C/D 가 진짜 mass (35%+35% = 70% of work).

가시 결과:
- `./target/release/todomvc` — Tab/Arrow/Home/End/Space filter cycle + M3 hover/press layers + scrollbar drag + 더블클릭 inline 편집 + Enter/Esc commit/cancel + strikethrough on completed + **exit + relaunch 시 state 영구 복원** (R665)
- `python3 tools/demos/todomvc_r665.py` (46 assertion, 13.46s — launch-kill-relaunch persistence cycle)
- `python3 tools/demos/todomvc_r664.py` (34 assertion, 5.89s)
- `python3 tools/demos/todomvc_r660.py` (40 assertion, 6.85s)

【북극성 명확화 — Phase A finalisation (R664-R667) 의미】

§1 Vision (R663.5 정정) + §2 7 invariants 가 가리키는 4-phase progression:

(a) **R664 (✓ land) = todomvc edit-in-place + R663 paint-side 2nd consumer 청산** — R663 substrate consumer 등장. view_field 3rd consumer = R657 lift ROI 확정 시점. text-decoration strikethrough = 1st paint primitive consumer. Phase A 진척 70% → ~78%

(b) **R665 (✓ land) = External(opaque) persistence** — pinion-platform-storage 16번째 crate (FileStorage atomic write via tempfile + sync_all + rename), `Storage` trait + InMemoryStorage substrate, PersistedState 단일 blob schema, use_persistence_boot Effect-retention pattern. §3 capability boundary 정통 escape hatch 첫 실증 완료. Phase A 진척 ~78% → ~80%

(c) **R666 = AI driving 12+ step end-to-end** — composite path routing, multi-state introspection workflow. scene/invoke v1 multi-External path syntax (R690+ carry, 또는 R666 inline). Persistence 가 있으므로 launch-kill-relaunch 시나리오까지 단일 워크플로우에 포함 가능. Phase A 진척 80% → ~85-87%

(d) **R667 = 2nd composed app (settings panel)** — view_vertical_scrollbar 4th consumer + view_field 4th consumer + Storage 2nd application consumer = substrate ROI curve fully positive 검증. **Phase A 종료** = 진짜 북극성의 ~7-8% 도달

(e) **R700+ = Phase B 진입** — Multi-window substrate 첫 라운드. winit 이미 multi-window 지원, `pinion-shell::WindowManager` + `Scene::Window` enum. DevTools / Inspector 가 첫 multi-window consumer. **이 시점이 진짜 framework 가 "professional tool 가능" 으로 도약**

(f) **R1000+ = Phase C 진입 = §2 #4 의 진짜 구현** — `ImmediateModeNode` primitive + game-loop infrastructure (60-144fps lockstep + delta time + frame budget cap) + retained↔immediate runtime switch per `Scene::Container` subtree. 동일 binary 안 settings panel = retained / 3D viewport = immediate

(g) **R2500+ = Phase D 진입** — Editor self-hosted in pinion itself. Unreal-class IDE 작성 시작. 진짜 northern-star 의 본격 진입

【다음 텍스트북 캐논 — R666 atomic 청산 (12+ step E2E composite path)】

R666 = **AI driving 12+ step end-to-end (composite path + persistence cycle)**

핵심 청산 항목:

(1) **scene/invoke v1 multi-External path syntax** — R690+ carry 를 R666 inline 청산 candidate. 현재 v0 는 primary-External only (R664/R665 demo 가 state-snapshot introspect-walk 로 우회). 정통 path syntax 결정:
- 옵션 A: `path = "todo_item#1"` 으로 ExtraExternal 직접 addressing
- 옵션 B: `path = "/external/extra/todo_item/1"` JSON-RPC tree-walk
- 옵션 C: `path = "todomvc/items/1"` semantic alias
- 캐논: 옵션 A (R55.D.5 composite-tag wire 와 일치, 별 alias 시스템 없음, R660-R664 의 composite-tag 5-of-5 framework consumer maturity 와 자연 수렴)

(2) **12+ step end-to-end workflow demo** — `tools/demos/todomvc_r666.py`:
- add 3 → toggle 1 → filter Active → snapshot (verify visual + state) → double_click edit row 2 → backspace+retype → Enter commit → assert text changed → kill process → relaunch → assert state restored → add 1 more → toggle the persisted-completed one off → snapshot → delete 1 → assert delete persisted across cycles
- 모든 step 이 AI introspection 으로 self-verify (scene/snapshot + scene/invoke + scene/intervene + storage path read for cross-process verification)
- ≥ 50 assertion 목표

(3) **(carry candidate) `[[owner-cache-no-nested-factory]]` memory 신규** — R665 가 nested factory 의 panic 을 첫 실증. R666 단축화 시점에 framework-side guard (Owner::cache 자체가 nested 호출 감지 + 친절한 panic 메시지) 추가 후보 — 또는 단순 documentation only carry

(4) **scene/key character key 의 RPC binding 보강 candidate** — `[[scene-key-character-named-gap]]` memory 의 R660+ carry 가 R666 의 12+ step demo 에서 type_text path 의 정통성 확보 시 inline 청산 후보

(5) **PersistenceBootMarker 2nd consumer 검토** — R665 가 Effect-retention substrate 첫 실증. R666 진행 중 다른 Effect-retain 응용 등장 시 (예: cross-process logging Effect, audit-trail Effect) `framework::OwnedEffect` lift candidate

R666 visible deliverable:
- `cargo run -p todomvc` 변화 없음 (R666 은 substrate + RPC primitive level)
- `python3 tools/demos/todomvc_r666.py` — 12+ step E2E workflow 가 모두 PASS

RPC demo R666 = ≥ 50 assertion (R665 baseline 46 + 4+ for cross-process workflow + multi-External path syntax)

honest LOC 예측: ~+400-700 LOC net (substrate-light, demo-heavy 라운드)
- (1) scene/invoke v1 path syntax: +200-300 LOC (pinion-rpc + Scene::walk_external_path)
- (2) todomvc_r666.py: +400 LOC
- (3-5) substrate guards / memory: +50-100 LOC

실측 후 honest 정정 의무. **R666 = AI-first invariant (§2 #2) 의 처음으로 진짜 stress test** — 12+ step E2E 가 통과하면 §2 #2 의 production-ready 수준 도달

진척도 +1-2%p 예상 (~7% → ~8-9% 진짜 northern-star 대비)

【R666-R667 Phase A 완성 cascade】

R665 (✓ land) — External(opaque) persistence
- pinion-core::storage + pinion-platform-storage 16번째 crate
- FileStorage atomic write (tempfile + sync_all + rename)
- PersistedState 단일 blob (todos + filter + next_id; editing_id 의도적 transient 제외)
- use_persistence_boot Effect-retention pattern + nested Owner::cache panic avoidance
- 46-assertion R665 demo (launch-kill-relaunch cycle)
- §3 Effect/External 정통 escape hatch 첫 실증 완료

R666 — AI driving 12+ step end-to-end
- scene/invoke v1 multi-External path syntax (R690+ carry, 또는 inline)
- 12+ step workflow: add 3 → toggle 1 → filter Active → edit 1 → commit → kill → relaunch → assert restored → add 1 more → toggle off persisted-completed → delete 1 → assert delete persisted
- 모든 step 이 AI introspection 으로 self-verify (R665 storage path read 와 결합)

R667 — 2nd composed app (settings panel) — Phase A 종료
- view_vertical_scrollbar 4th consumer
- view_field 4th consumer
- Storage 2nd application consumer
- R657/R659/R660/R665 substrate ROI curve fully positive
- Phase A 완료 = 진짜 northern-star ~5%-7% 도달

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

기존 누적 + R665 land 청산:
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

R665 carry (R666 inline 청산 candidate):
- scene/invoke v0 primary-External only → R666 multi-External path syntax inline 청산 (R690+ → R666 candidate)
- PersistenceBootMarker Effect-retention substrate quirk (Owner::cleanup Weak only; 2nd consumer 등장 시 framework helper lift)
- Owner::cache no-nested-factory 룰 docs + memory 명문화

영구 carry (외부 의존):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- SCE-002 (consumer-injectable derive list) — SCE-004 와 같은 axis
- WIN_H 480 magic (flex-grow primitive)
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- Figma API token
- TUI parity (§6 #6 cascade)
- Persistence schema breaking-change migrator 부재 (현재 fall-through; 첫 breaking 변경 axis 등장 시 lift)

【R661-R665 lessons — 명시】

- **R663.5 정정의 가장 큰 lesson — Vision spec 명시가 모든 라운드 axis 선택의 anchor.** R660-R663 동안 "R667 settings panel = 북극성" misdirection 으로 substrate / process maturity / framework debt 만 진행. 진짜 northern-star (AAA + editor self-hosted) 가 spec / CLAUDE / memory / seed prompt 5-layer 어디에도 명시 안 됨. **axis 선택 시 매번 self-check: 이 라운드가 진짜 northern-star (4-phase 종착) 에 얼마나 가까이 가는가?**
- **Substrate-first ordering 정통** — R663 framework-first double-click → R664 substrate-consumer; R665 framework-first Storage trait (pinion-core) + FileStorage (sibling crate) → todomvc consumer. [[r47-class-incident-prevention]] textbook
- **Mirror-substrate pattern** — R665 의 Storage / FileStorage 가 R56.1.e/R56.2.b 의 Clipboard / ArboardClipboard 구조 정확히 미러. 비슷한 platform-bridge 추가 시 (`pinion-platform-network`? `pinion-platform-audio`?) 동일 패턴 재사용
- **Effect-retention substrate quirk (R665 신규)** — Owner::cleanup queue 가 Weak 만 보관 (R37.5 #2 leak fix). Application 이 Effect handle 영구 retain mandatory. 모든 production Effect 사이트 (PersistenceBootMarker 등) Rc 보관 필요. 2nd consumer 등장 시 framework lift candidate
- **Owner::cache nested factory panic (R665 신규)** — Owner::cache 가 RefCell::borrow_mut 을 factory 동안 hold. 중첩 Owner::cache 호출 panic. Pre-resolution pattern (모든 dependent slot 먼저 resolve, 그 다음 outer cache 진입) 으로 우회. 새 memory `[[owner-cache-no-nested-factory]]` 추가 candidate
- **Doc compression baseline (R661) effective** — process maturity 라운드 분리 = substrate refactor 안전
- **SCE upstream debt 의 doc-anchor pattern** — R662 stop-gap 의 retire path 를 코드 doc 에 명시. 미래 Forge serde derive land 시 automatic retire
- **parent_tag substrate 의 multi-composite 일반화** — R662 access_child_invoke 확장이 R664 에서 4-of-4 application 도달; R667 settings panel sections 자동 활용
- **5-of-5 substrate maturity** — composite_tag mature substrate 는 sublinear 비용 증가. R664 의 TodoEditExternal = 6th consumer 무비용 추가

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

R666 AI driving 12+ step end-to-end 라운드 자동 진행. 5개 atomic land:

(1) scene/invoke v1 multi-External path syntax — composite-tag `path = "todo_item#1"` ExtraExternal addressing (R55.D.5 wire 와 자연 수렴). pinion-rpc::resolve_external_path + Scene::walk_external_path
(2) `tools/demos/todomvc_r666.py` — 12+ step E2E workflow (add → toggle → filter → edit → commit → kill → relaunch → assert restored → add → toggle off persisted → delete → assert delete persisted)
(3) `[[owner-cache-no-nested-factory]]` memory 신규 — R665 lesson 의 formal capture
(4) (optional) Owner::cache 자체의 nested-detect guard — RefCell::try_borrow_mut + 친절 panic message; 또는 docs-only carry
(5) scene/key character key RPC binding 청산 candidate ([[scene-key-character-named-gap]] R660+ carry) — R666 demo 의 type_text path 정통성 확보 시점

visible: `cargo run -p todomvc` 변화 없음 (R666 = substrate + RPC primitive level)

RPC verify demo R666 = ≥ 50 assertion. honest LOC 예측: ~+400-700 LOC net. 실측 후 honest 정정 의무.
